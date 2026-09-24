//! Command Tauri yang dipanggil frontend lewat `invoke`.
//!
//! - Mode **server lokal**: sinkron dengan server pusat, dasbor proktor, API LAN untuk PC peserta.
//! - Mode **PC peserta**: semua aksi ujian diteruskan ke server lokal lewat `ParticipantLink`.
//!
//! Menu proktor (di kedua mode) memerlukan login proktor dengan akun dari server pusat.

use std::sync::atomic::Ordering;

use base64::Engine;
use chrono::Utc;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State};

use crate::core::api::{AuthInfo, RemoteSchedule};
use crate::core::error::{AppError, AppResult};
use crate::core::exam::{AttemptState, ExamSession, LoginRequest, SaveAnswerRequest, SavedAttachment};
use crate::core::lan::LanSchedule;
use crate::core::participant::LinkStatus;
use crate::core::proctor::{Device, LogTarget, MonitorRow, PairResult, Proctor, ProctorLogRow, DEVICE_APPROVED, DEVICE_REVOKED};
use crate::core::sync::{self, BatchRow, DownloadResult, UploadResult};
use crate::core::{AttemptCounts, ConfigView, LocalSchedule, ParticipantConfigInput, ServerConfigInput};
use crate::state::AppState;

fn require_server(state: &AppState) -> AppResult<()> {
    if state.core.is_server() {
        Ok(())
    } else {
        Err(AppError::user("Menu ini hanya ada di server lokal"))
    }
}

/// Catat aksi proktor (mode server lokal).
fn log_action(state: &AppState, p: &Proctor, action: &str, target: LogTarget<'_>, data: Option<Value>) -> AppResult<()> {
    state.core.log_proctor(p, action, target, data, Utc::now())
}

// ---------------------------------------------------------------------- konfigurasi

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> AppResult<ConfigView> {
    state.core.config()
}

#[tauri::command]
pub async fn save_server_config(app: AppHandle, state: State<'_, AppState>, input: ServerConfigInput) -> AppResult<ConfigView> {
    let before = state.core.config()?;
    // Setelah ada proktor, perubahan pengaturan hanya oleh proktor.
    let proctor = if before.configured && state.core.proctor_count()? > 0 {
        Some(state.require_proctor()?)
    } else {
        None
    };
    let view = state.core.save_server_config(input)?;
    state.reset_clients().await;
    if let Some(p) = proctor {
        log_action(&state, &p, "settings_change", LogTarget::default(), None)?;
    }
    crate::ensure_lan_server(&app);
    Ok(view)
}

#[tauri::command]
pub async fn save_participant_config(state: State<'_, AppState>, input: ParticipantConfigInput) -> AppResult<ConfigView> {
    let before = state.core.config()?;
    if before.configured && state.current_proctor().is_none() {
        // Tanpa login proktor, alamat hanya boleh diubah selama PC ini belum disetujui oleh
        // server lokal yang sekarang (salah alamat, atau IP server berganti) dan tidak ada ujian
        // berjalan. PC tetap harus disetujui proktor di server yang baru.
        let approved = match state.link().await {
            Ok(link) => link.client.me().await.is_ok(),
            Err(_) => false,
        };
        if approved || state.exam_active.load(Ordering::SeqCst) {
            return Err(AppError::user("Login proktor dulu untuk mengubah pengaturan komputer ini"));
        }
    }
    let view = state.core.save_participant_config(input)?;
    state.reset_clients().await;
    Ok(view)
}

// ---------------------------------------------------------------------- proktor

#[tauri::command]
pub async fn proctor_login(state: State<'_, AppState>, username: String, password: String) -> AppResult<Proctor> {
    let p = if state.core.is_participant() {
        state.link().await?.proctor_verify(&username, &password).await?
    } else {
        require_server(&state)?;
        let core = state.core.clone();
        let p = tauri::async_runtime::spawn_blocking(move || core.verify_proctor(&username, &password))
            .await
            .map_err(|e| AppError::Other(e.to_string()))?;
        match p {
            Ok(p) => p,
            Err(e) => {
                tokio::time::sleep(std::time::Duration::from_millis(800)).await;
                return Err(e);
            }
        }
    };
    if state.core.is_server() {
        log_action(
            &state,
            &p,
            "login",
            LogTarget::default(),
            Some(json!({ "from": "server_lokal" })),
        )?;
    }
    state.set_proctor(Some(p.clone()));
    Ok(p)
}

#[tauri::command]
pub fn proctor_logout(state: State<'_, AppState>) -> AppResult<()> {
    if let Some(p) = state.current_proctor() {
        if state.core.is_server() {
            log_action(&state, &p, "logout", LogTarget::default(), None)?;
        }
    }
    state.set_proctor(None);
    Ok(())
}

#[tauri::command]
pub fn current_proctor(state: State<'_, AppState>) -> Option<Proctor> {
    state.current_proctor()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProctorSyncResult {
    pub count: usize,
}

/// Ambil akun proktor terbaru dari server pusat. Boleh tanpa login (dibutuhkan saat belum
/// ada proktor sama sekali); hanya menarik data dari server pusat.
#[tauri::command]
pub async fn sync_proctors(state: State<'_, AppState>) -> AppResult<ProctorSyncResult> {
    require_server(&state)?;
    let api = state.api().await?;
    Ok(ProctorSyncResult {
        count: sync::sync_proctors(&state.core, &api).await?,
    })
}

#[tauri::command]
pub fn proctor_count(state: State<'_, AppState>) -> AppResult<i64> {
    state.core.proctor_count()
}

// ---------------------------------------------------------------------- server lokal: server pusat

#[tauri::command]
pub async fn test_connection(state: State<'_, AppState>) -> AppResult<AuthInfo> {
    state.require_proctor()?;
    let api = state.api().await?;
    let info = api.auth().await?;
    sync::sync_proctors(&state.core, &api).await?;
    Ok(info)
}

#[tauri::command]
pub async fn remote_schedules(state: State<'_, AppState>) -> AppResult<Vec<RemoteSchedule>> {
    state.require_proctor()?;
    let api = state.api().await?;
    sync::remote_schedules(&state.core, &api).await
}

#[tauri::command]
pub async fn download_schedule(state: State<'_, AppState>, schedule_id: String) -> AppResult<DownloadResult> {
    let p = state.require_proctor()?;
    let api = state.api().await?;
    let res = sync::download_schedule(&state.core, &api, &schedule_id).await?;
    // Token sesi terbaru ikut disimpan (bila admin menggantinya bersama paket baru).
    let _ = sync::remote_schedules(&state.core, &api).await;
    state.core.prune_packages()?;
    log_action(
        &state,
        &p,
        "package_download",
        LogTarget {
            schedule_id: Some(&schedule_id),
            ..Default::default()
        },
        Some(json!({ "version": res.package_version, "updated": res.updated })),
    )?;
    Ok(res)
}

#[tauri::command]
pub fn local_schedules(state: State<'_, AppState>) -> AppResult<Vec<LocalSchedule>> {
    require_server(&state)?;
    state.require_proctor()?;
    state.core.local_schedules()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub counts: AttemptCounts,
    pub last_sync_at: Option<String>,
    pub last_error: Option<String>,
    pub batches: Vec<BatchRow>,
    pub auto_sync: bool,
    pub proctor_log_pending: usize,
}

#[tauri::command]
pub fn sync_status(state: State<'_, AppState>) -> AppResult<SyncStatus> {
    require_server(&state)?;
    let last = state.last_sync.lock().unwrap_or_else(|e| e.into_inner()).clone();
    Ok(SyncStatus {
        counts: state.core.attempt_counts(None)?,
        last_sync_at: last.0,
        last_error: last.1,
        batches: sync::recent_batches(&state.core, 20)?,
        auto_sync: state.core.config()?.auto_sync,
        proctor_log_pending: state.core.unsynced_proctor_log(10_000)?.len(),
    })
}

#[tauri::command]
pub async fn sync_now(state: State<'_, AppState>) -> AppResult<UploadResult> {
    let p = state.require_proctor()?;
    let res = run_sync(&state).await;
    let data = match &res {
        Ok(r) => json!({ "attempts": r.attempts_sent, "attachments": r.attachments_uploaded }),
        Err(e) => json!({ "error": e.to_string() }),
    };
    log_action(&state, &p, "results_upload", LogTarget::default(), Some(data))?;
    res
}

/// Kirim hasil + log proktor, perbarui status batch dan akun proktor. Dipakai tombol
/// proktor dan sinkron otomatis.
pub async fn run_sync(state: &AppState) -> AppResult<UploadResult> {
    let _guard = state.sync_lock.lock().await;
    let result = async {
        let api = state.api().await?;
        let up = sync::upload_results(&state.core, &api).await?;
        sync::refresh_batches(&state.core, &api).await?;
        sync::sync_proctors(&state.core, &api).await?;
        let now = Utc::now();
        let status = json!({
            "attempts": state.core.attempt_counts(None)?,
            "devicesOnline": state.core.devices_online(120, now)?,
            "proctorLogPending": state.core.unsynced_proctor_log(10_000)?.len(),
        });
        let _ = api.heartbeat(status).await;
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

// ---------------------------------------------------------------------- server lokal: dasbor proktor

#[tauri::command]
pub fn monitor(state: State<'_, AppState>, schedule_id: String) -> AppResult<Vec<MonitorRow>> {
    require_server(&state)?;
    state.require_proctor()?;
    state.core.monitor(&schedule_id, Utc::now())
}

/// Info attempt untuk log: (schedule_id, participant_id).
fn attempt_target(state: &AppState, attempt_id: &str) -> (Option<String>, Option<String>) {
    state
        .core
        .db()
        .conn
        .query_row(
            "SELECT schedule_id, participant_id FROM attempts WHERE id = ?1",
            [attempt_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .map(|(s, p)| (Some(s), Some(p)))
        .unwrap_or((None, None))
}

fn log_attempt_action(state: &AppState, p: &Proctor, action: &str, attempt_id: &str, data: Option<Value>) -> AppResult<()> {
    let (schedule_id, participant_id) = attempt_target(state, attempt_id);
    log_action(
        state,
        p,
        action,
        LogTarget {
            schedule_id: schedule_id.as_deref(),
            attempt_id: Some(attempt_id),
            participant_id: participant_id.as_deref(),
        },
        data,
    )
}

#[tauri::command]
pub fn release_device(state: State<'_, AppState>, attempt_id: String) -> AppResult<()> {
    require_server(&state)?;
    let p = state.require_proctor()?;
    state.core.release_device(&attempt_id, Utc::now())?;
    log_attempt_action(&state, &p, "attempt_reset_device", &attempt_id, None)
}

#[tauri::command]
pub fn extend_time(state: State<'_, AppState>, attempt_id: String, minutes: i64) -> AppResult<AttemptState> {
    require_server(&state)?;
    let p = state.require_proctor()?;
    let st = state.core.extend_time(&attempt_id, minutes, Utc::now())?;
    log_attempt_action(
        &state,
        &p,
        "attempt_extra_time",
        &attempt_id,
        Some(json!({ "minutes": minutes })),
    )?;
    Ok(st)
}

#[tauri::command]
pub fn terminate_attempt(state: State<'_, AppState>, attempt_id: String, reason: Option<String>) -> AppResult<()> {
    require_server(&state)?;
    let p = state.require_proctor()?;
    state.core.terminate(&attempt_id, reason.as_deref(), Utc::now())?;
    log_attempt_action(
        &state,
        &p,
        "attempt_terminate",
        &attempt_id,
        reason.map(|r| json!({ "reason": r })),
    )
}

#[tauri::command]
pub fn unlock_attempt(state: State<'_, AppState>, attempt_id: String, extra_minutes: Option<i64>) -> AppResult<AttemptState> {
    require_server(&state)?;
    let p = state.require_proctor()?;
    let st = state.core.unlock(&attempt_id, extra_minutes, Utc::now())?;
    log_attempt_action(
        &state,
        &p,
        "attempt_unlock",
        &attempt_id,
        Some(json!({ "extraMinutes": extra_minutes })),
    )?;
    Ok(st)
}

/// Hapus attempt lokal (peserta mengulang dari awal). Bila attempt sudah terkirim ke server
/// pusat, admin juga harus menghapusnya di sana.
#[tauri::command]
pub fn delete_attempt(state: State<'_, AppState>, attempt_id: String) -> AppResult<()> {
    require_server(&state)?;
    let p = state.require_proctor()?;
    log_attempt_action(&state, &p, "attempt_delete", &attempt_id, None)?;
    state.core.reset_attempt(&attempt_id)
}

#[tauri::command]
pub fn list_devices(state: State<'_, AppState>) -> AppResult<Vec<Device>> {
    require_server(&state)?;
    state.require_proctor()?;
    state.core.devices()
}

#[tauri::command]
pub fn set_device_status(state: State<'_, AppState>, device_id: String, approved: bool) -> AppResult<()> {
    require_server(&state)?;
    let p = state.require_proctor()?;
    let status = if approved { DEVICE_APPROVED } else { DEVICE_REVOKED };
    state.core.set_device_status(&device_id, status, &p, Utc::now())?;
    let name = state.core.device_name(&device_id);
    log_action(
        &state,
        &p,
        if approved { "device_approve" } else { "device_revoke" },
        LogTarget::default(),
        Some(json!({ "deviceId": device_id, "name": name })),
    )
}

#[tauri::command]
pub fn delete_device(state: State<'_, AppState>, device_id: String) -> AppResult<()> {
    require_server(&state)?;
    let p = state.require_proctor()?;
    let name = state.core.device_name(&device_id);
    state.core.delete_device(&device_id)?;
    log_action(
        &state,
        &p,
        "device_revoke",
        LogTarget::default(),
        Some(json!({ "deviceId": device_id, "name": name, "deleted": true })),
    )
}

#[tauri::command]
pub fn proctor_log(state: State<'_, AppState>) -> AppResult<Vec<ProctorLogRow>> {
    require_server(&state)?;
    state.require_proctor()?;
    state.core.proctor_log(300)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanInfoView {
    pub port: u16,
    pub running: bool,
    pub error: Option<String>,
    /// Alamat yang diketik di PC peserta, mis. `192.168.1.10:8787`.
    pub addresses: Vec<String>,
    pub devices_online: i64,
}

#[tauri::command]
pub fn lan_info(state: State<'_, AppState>) -> AppResult<LanInfoView> {
    require_server(&state)?;
    let status = state.lan_server.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let port = state.core.config()?.lan_port;
    let addresses = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback())
        .filter_map(|i| match i.ip() {
            std::net::IpAddr::V4(ip) if !ip.is_link_local() => Some(format!("{ip}:{port}")),
            _ => None,
        })
        .collect();
    Ok(LanInfoView {
        port,
        running: status.port == Some(port) && status.error.is_none(),
        error: status.error,
        addresses,
        devices_online: state.core.devices_online(120, Utc::now())?,
    })
}

// ---------------------------------------------------------------------- PC peserta

#[tauri::command]
pub async fn pair_device(state: State<'_, AppState>) -> AppResult<PairResult> {
    state.link().await?.pair().await
}

#[tauri::command]
pub async fn link_status(state: State<'_, AppState>) -> AppResult<LinkStatus> {
    Ok(state.link().await?.status().await)
}

#[tauri::command]
pub async fn exam_schedules(state: State<'_, AppState>) -> AppResult<Vec<LanSchedule>> {
    state.link().await?.schedules().await
}

#[tauri::command]
pub async fn participant_login(state: State<'_, AppState>, request: LoginRequest) -> AppResult<ExamSession> {
    state.link().await?.login(&request).await
}

#[tauri::command]
pub async fn get_session(state: State<'_, AppState>, attempt_id: String) -> AppResult<ExamSession> {
    state.link().await?.session(&attempt_id).await
}

#[tauri::command]
pub async fn save_answer(state: State<'_, AppState>, request: SaveAnswerRequest) -> AppResult<AttemptState> {
    state.link().await?.save_answer(&request).await
}

#[tauri::command]
pub async fn log_event(
    state: State<'_, AppState>,
    attempt_id: String,
    kind: String,
    data: Option<Value>,
) -> AppResult<AttemptState> {
    state.link().await?.log_event(&attempt_id, &kind, data).await
}

#[tauri::command]
pub async fn submit_exam(state: State<'_, AppState>, attempt_id: String, manual: bool) -> AppResult<AttemptState> {
    state.link().await?.submit(&attempt_id, manual).await
}

#[tauri::command]
pub async fn attempt_state(state: State<'_, AppState>, attempt_id: String) -> AppResult<AttemptState> {
    state.link().await?.attempt_state(&attempt_id).await
}

#[tauri::command]
pub async fn save_attachment(
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
        .link()
        .await?
        .save_attachment(&attempt_id, &question_id, &name, &mime, &bytes)
        .await
}

// ---------------------------------------------------------------------- jendela

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
    let p = state.require_proctor()?;
    if state.core.is_server() {
        log_action(&state, &p, "logout", LogTarget::default(), Some(json!({ "quit": true })))?;
    }
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
