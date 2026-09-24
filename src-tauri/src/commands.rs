//! Command Tauri yang dipanggil frontend lewat `invoke`. Sebagian besar hanya
//! membungkus `core`. Command operator memerlukan PIN operator (sesi 30 menit).

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine;
use chrono::Utc;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State};

use crate::core::api::{AuthInfo, RemoteSchedule, SyncApi};
use crate::core::error::{AppError, AppResult};
use crate::core::exam::{AttemptState, ExamSession, LoginRequest, SaveAnswerRequest, SavedAttachment};
use crate::core::sync::{self, BatchRow, DownloadResult, UploadResult};
use crate::core::{AttemptCounts, ConfigInput, ConfigView, LocalSchedule};
use crate::state::AppState;

const OPERATOR_SESSION: Duration = Duration::from_secs(30 * 60);

fn require_operator(state: &AppState) -> AppResult<()> {
    let until = *state.operator_until.lock().unwrap_or_else(|e| e.into_inner());
    match until {
        Some(t) if Instant::now() < t => Ok(()),
        _ => Err(AppError::user("Sesi operator berakhir. Masukkan PIN operator lagi.")),
    }
}

// ---------------------------------------------------------------------- konfigurasi & operator

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> AppResult<ConfigView> {
    state.core.config()
}

#[tauri::command]
pub async fn save_config(state: State<'_, AppState>, input: ConfigInput) -> AppResult<ConfigView> {
    // Konfigurasi awal boleh tanpa PIN; perubahan berikutnya harus sebagai operator.
    if state.core.config()?.has_pin {
        require_operator(&state)?;
    }
    let view = state.core.save_config(input)?;
    state.reset_api().await;
    Ok(view)
}

#[tauri::command]
pub fn unlock_operator(state: State<'_, AppState>, pin: String) -> AppResult<bool> {
    let ok = state.core.verify_pin(pin.trim())?;
    if ok {
        *state.operator_until.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now() + OPERATOR_SESSION);
    }
    Ok(ok)
}

#[tauri::command]
pub fn lock_operator(state: State<'_, AppState>) {
    *state.operator_until.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

#[tauri::command]
pub async fn test_connection(state: State<'_, AppState>) -> AppResult<AuthInfo> {
    require_operator(&state)?;
    state.api().await?.auth().await
}

// ---------------------------------------------------------------------- jadwal & paket

#[tauri::command]
pub async fn remote_schedules(state: State<'_, AppState>) -> AppResult<Vec<RemoteSchedule>> {
    require_operator(&state)?;
    state.api().await?.schedules().await
}

#[tauri::command]
pub async fn download_schedule(state: State<'_, AppState>, schedule_id: String) -> AppResult<DownloadResult> {
    require_operator(&state)?;
    let api = state.api().await?;
    let res = sync::download_schedule(&state.core, &api, &schedule_id).await?;
    state.core.prune_packages()?;
    Ok(res)
}

#[tauri::command]
pub fn local_schedules(state: State<'_, AppState>) -> AppResult<Vec<LocalSchedule>> {
    state.core.local_schedules()
}

// ---------------------------------------------------------------------- sinkronisasi hasil

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub counts: AttemptCounts,
    pub last_sync_at: Option<String>,
    pub last_error: Option<String>,
    pub batches: Vec<BatchRow>,
    pub auto_sync: bool,
}

#[tauri::command]
pub fn sync_status(state: State<'_, AppState>) -> AppResult<SyncStatus> {
    let last = state.last_sync.lock().unwrap_or_else(|e| e.into_inner()).clone();
    Ok(SyncStatus {
        counts: state.core.attempt_counts(None)?,
        last_sync_at: last.0,
        last_error: last.1,
        batches: sync::recent_batches(&state.core, 20)?,
        auto_sync: state.core.config()?.auto_sync,
    })
}

#[tauri::command]
pub async fn sync_now(state: State<'_, AppState>) -> AppResult<UploadResult> {
    require_operator(&state)?;
    run_sync(&state).await
}

/// Kirim hasil + perbarui status batch. Dipakai tombol operator dan sinkron otomatis.
pub async fn run_sync(state: &AppState) -> AppResult<UploadResult> {
    let _guard = state.sync_lock.lock().await;
    let result = async {
        let api = state.api().await?;
        let up = sync::upload_results(&state.core, &api).await?;
        sync::refresh_batches(&state.core, &api).await?;
        let counts = state.core.attempt_counts(None)?;
        let _ = api.heartbeat(json!({ "attempts": counts })).await;
        Ok::<_, AppError>(up)
    }
    .await;
    let mut last = state.last_sync.lock().unwrap_or_else(|e| e.into_inner());
    match &result {
        Ok(_) => *last = (Some(Utc::now().to_rfc3339()), None),
        Err(e) => last.1 = Some(e.to_string()),
    }
    result
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptRow {
    pub id: String,
    pub participant_number: String,
    pub participant_name: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub answered: i64,
    pub violation_count: i64,
    pub synced: bool,
    pub sync_error: Option<String>,
}

#[tauri::command]
pub fn list_attempts(state: State<'_, AppState>, schedule_id: String) -> AppResult<Vec<AttemptRow>> {
    require_operator(&state)?;
    let Some((pkg, _)) = state.core.latest_package(&schedule_id)? else {
        return Ok(vec![]);
    };
    let db = state.core.db();
    let mut stmt = db.conn.prepare(
        "SELECT a.id, a.participant_id, a.status, a.started_at, a.finished_at, a.violation_count,
                a.sequence <= a.synced_sequence, a.sync_error,
                (SELECT count(*) FROM answers w WHERE w.attempt_id = a.id AND w.response IS NOT NULL)
         FROM attempts a WHERE a.schedule_id = ?1 ORDER BY a.started_at",
    )?;
    let rows = stmt.query_map([&schedule_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, i64>(5)?,
            r.get::<_, bool>(6)?,
            r.get::<_, Option<String>>(7)?,
            r.get::<_, i64>(8)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, pid, status, started_at, finished_at, violations, synced, sync_error, answered) = row?;
        let p = pkg.participants.iter().find(|p| p.id == pid);
        out.push(AttemptRow {
            id,
            participant_number: p.map(|p| p.number.clone()).unwrap_or_default(),
            participant_name: p.map(|p| p.name.clone()).unwrap_or_default(),
            status,
            started_at,
            finished_at,
            answered,
            violation_count: violations,
            synced,
            sync_error,
        });
    }
    Ok(out)
}

#[tauri::command]
pub fn reset_attempt(state: State<'_, AppState>, attempt_id: String) -> AppResult<()> {
    require_operator(&state)?;
    state.core.reset_attempt(&attempt_id)
}

// ---------------------------------------------------------------------- ujian peserta

#[tauri::command]
pub fn participant_login(state: State<'_, AppState>, request: LoginRequest) -> AppResult<ExamSession> {
    let id = state.core.login(&request, Utc::now())?;
    state.core.session(&id, Utc::now())
}

#[tauri::command]
pub fn get_session(state: State<'_, AppState>, attempt_id: String) -> AppResult<ExamSession> {
    state.core.session(&attempt_id, Utc::now())
}

#[tauri::command]
pub fn save_answer(state: State<'_, AppState>, request: SaveAnswerRequest) -> AppResult<AttemptState> {
    state.core.save_answer(&request, Utc::now())
}

#[tauri::command]
pub fn log_event(state: State<'_, AppState>, attempt_id: String, kind: String, data: Option<Value>) -> AppResult<AttemptState> {
    state.core.log_event(&attempt_id, &kind, data, Utc::now())
}

#[tauri::command]
pub fn submit_exam(state: State<'_, AppState>, attempt_id: String, manual: bool) -> AppResult<AttemptState> {
    state.core.submit(&attempt_id, manual, Utc::now())
}

#[tauri::command]
pub fn save_attachment(
    state: State<'_, AppState>,
    attempt_id: String,
    question_id: String,
    name: String,
    mime: String,
    data_base64: String,
) -> AppResult<SavedAttachment> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_base64.as_bytes())
        .map_err(|e| AppError::user(format!("berkas tidak valid: {e}")))?;
    state
        .core
        .save_attachment(&attempt_id, &question_id, &name, &mime, &bytes, Utc::now())
}

/// Masuk / keluar mode ujian: layar penuh + selalu di atas bila `lockdown`, dan cegah menutup jendela.
#[tauri::command]
pub fn set_exam_mode(app: AppHandle, state: State<'_, AppState>, active: bool, lockdown: bool) -> AppResult<()> {
    state.exam_active.store(active, Ordering::SeqCst);
    if let Some(win) = app.get_webview_window("main") {
        let kiosk = active && lockdown;
        let _ = win.set_fullscreen(kiosk);
        let _ = win.set_always_on_top(kiosk);
        if kiosk {
            let _ = win.set_focus();
        }
    }
    Ok(())
}

#[tauri::command]
pub fn quit_app(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    require_operator(&state)?;
    state.exam_active.store(false, Ordering::SeqCst);
    app.exit(0);
    Ok(())
}

#[tauri::command]
pub fn app_info(state: State<'_, AppState>) -> Value {
    json!({
        "version": crate::core::api::APP_VERSION,
        "dataDir": state.core.data_dir().to_string_lossy(),
    })
}

/// Dipakai `api()` di state.
pub fn build_api(state: &AppState) -> AppResult<Arc<SyncApi>> {
    Ok(Arc::new(SyncApi::new(state.core.credentials()?)?))
}
