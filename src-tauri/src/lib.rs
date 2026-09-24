pub mod commands;
pub mod core;
pub mod state;

use std::sync::atomic::Ordering;
use std::time::Duration;

use tauri::http::{header, Response, StatusCode};
use tauri::{Emitter, Manager, RunEvent, WindowEvent};

use crate::core::Core;
use crate::state::AppState;

/// Interval sinkron otomatis (kirim hasil + heartbeat) saat perangkat online.
const AUTO_SYNC_INTERVAL: Duration = Duration::from_secs(60);

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let dir = app.path().app_data_dir().expect("direktori data aplikasi");
            let core = Core::open(&dir).map_err(|e| format!("gagal membuka data lokal di {}: {e}", dir.display()))?;
            app.manage(AppState::new(core));

            // Sinkron otomatis di latar belakang.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(AUTO_SYNC_INTERVAL).await;
                    let state = handle.state::<AppState>();
                    let Ok(cfg) = state.core.config() else { continue };
                    if !cfg.configured || !cfg.auto_sync {
                        continue;
                    }
                    let pending = state.core.attempt_counts(None).map(|c| c.unsynced).unwrap_or(0);
                    let result = commands::run_sync(&state).await;
                    if pending > 0 || result.is_err() {
                        let _ = handle.emit("sync-status", result.map(|r| r.attempts_sent).map_err(|e| e.to_string()));
                    }
                }
            });
            Ok(())
        })
        // Media soal disajikan dari berkas lokal: cbtasset://localhost/<assetId>
        .register_uri_scheme_protocol("cbtasset", |ctx, request| {
            let state = ctx.app_handle().state::<AppState>();
            let id = request.uri().path().trim_start_matches('/').to_string();
            let path = state.core.asset_path(&id);
            match std::fs::read(&path) {
                Ok(bytes) => {
                    let mime = state
                        .core
                        .asset_mime(&id)
                        .unwrap_or_else(|| "application/octet-stream".into());
                    Response::builder()
                        .header(header::CONTENT_TYPE, mime)
                        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                        .body(bytes)
                        .unwrap()
                }
                Err(_) => Response::builder().status(StatusCode::NOT_FOUND).body(Vec::new()).unwrap(),
            }
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<AppState>();
                if state.exam_active.load(Ordering::SeqCst) {
                    api.prevent_close();
                    let _ = window.emit("close-blocked", ());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::save_config,
            commands::unlock_operator,
            commands::lock_operator,
            commands::test_connection,
            commands::remote_schedules,
            commands::download_schedule,
            commands::local_schedules,
            commands::sync_status,
            commands::sync_now,
            commands::list_attempts,
            commands::reset_attempt,
            commands::participant_login,
            commands::get_session,
            commands::save_answer,
            commands::log_event,
            commands::submit_exam,
            commands::save_attachment,
            commands::set_exam_mode,
            commands::quit_app,
            commands::app_info,
        ])
        .build(tauri::generate_context!())
        .expect("gagal menjalankan aplikasi")
        .run(|app, event| {
            // Cegah keluar (mis. Alt+F4 terakhir) selama ujian berlangsung.
            if let RunEvent::ExitRequested { api, code, .. } = event {
                let state = app.state::<AppState>();
                if code.is_none() && state.exam_active.load(Ordering::SeqCst) {
                    api.prevent_exit();
                }
            }
        });
}
