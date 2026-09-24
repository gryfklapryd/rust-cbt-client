pub mod commands;
pub mod core;
pub mod state;

use std::sync::atomic::Ordering;
use std::time::Duration;

use tauri::http::{header, Response, StatusCode};
use tauri::{Emitter, Manager, RunEvent, WindowEvent};

use crate::core::Core;
use crate::state::AppState;

/// Interval sinkron otomatis server lokal ke server pusat (kirim hasil + heartbeat).
const AUTO_SYNC_INTERVAL: Duration = Duration::from_secs(60);
/// Interval PC peserta mengirim antrean yang tertunda ke server lokal.
const OUTBOX_INTERVAL: Duration = Duration::from_secs(5);

/// Jalankan (atau jalankan ulang bila port berubah) API LAN pada mode server lokal.
pub fn ensure_lan_server(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    if !state.core.is_server() {
        return;
    }
    let Ok(cfg) = state.core.config() else { return };
    let port = cfg.lan_port;
    {
        let status = state.lan_server.lock().unwrap_or_else(|e| e.into_inner());
        if status.port == Some(port) && status.error.is_none() {
            return;
        }
    }
    if let Some(old) = state.lan_task.lock().unwrap_or_else(|e| e.into_inner()).take() {
        old.abort();
    }
    let core = state.core.clone();
    let handle = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        let result = match tokio::net::TcpListener::bind(("0.0.0.0", port)).await {
            Ok(listener) => {
                {
                    let st = handle.state::<AppState>();
                    let mut status = st.lan_server.lock().unwrap_or_else(|e| e.into_inner());
                    status.port = Some(port);
                    status.error = None;
                }
                crate::core::lan::serve_on(core, listener).await
            }
            Err(e) => Err(e),
        };
        if let Err(e) = result {
            let st = handle.state::<AppState>();
            let mut status = st.lan_server.lock().unwrap_or_else(|e| e.into_inner());
            status.port = Some(port);
            status.error = Some(format!("Port {port} tidak bisa dipakai: {e}"));
        }
    });
    *state.lan_task.lock().unwrap_or_else(|e| e.into_inner()) = Some(task);
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let dir = app.path().app_data_dir().expect("direktori data aplikasi");
            let core = Core::open(&dir).map_err(|e| format!("gagal membuka data lokal di {}: {e}", dir.display()))?;
            app.manage(AppState::new(core));
            ensure_lan_server(app.handle());

            // Server lokal: sinkron otomatis ke server pusat.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(AUTO_SYNC_INTERVAL).await;
                    let state = handle.state::<AppState>();
                    let Ok(cfg) = state.core.config() else { continue };
                    if !state.core.is_server() || !cfg.configured || !cfg.auto_sync {
                        continue;
                    }
                    let pending = state.core.attempt_counts(None).map(|c| c.unsynced).unwrap_or(0);
                    let result = commands::run_sync(&state).await;
                    if pending > 0 || result.is_err() {
                        let _ = handle.emit("sync-status", result.map(|r| r.attempts_sent).map_err(|e| e.to_string()));
                    }
                }
            });

            // PC peserta: kirim antrean yang tertunda ke server lokal.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(OUTBOX_INTERVAL).await;
                    let state = handle.state::<AppState>();
                    if !state.core.is_participant() || state.core.outbox_count(None).unwrap_or(0) == 0 {
                        continue;
                    }
                    if let Ok(link) = state.link().await {
                        link.flush().await;
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
            commands::save_server_config,
            commands::save_participant_config,
            commands::proctor_login,
            commands::proctor_logout,
            commands::current_proctor,
            commands::sync_proctors,
            commands::proctor_count,
            commands::test_connection,
            commands::remote_schedules,
            commands::download_schedule,
            commands::local_schedules,
            commands::sync_status,
            commands::sync_now,
            commands::monitor,
            commands::release_device,
            commands::extend_time,
            commands::terminate_attempt,
            commands::unlock_attempt,
            commands::delete_attempt,
            commands::list_devices,
            commands::set_device_status,
            commands::delete_device,
            commands::proctor_log,
            commands::lan_info,
            commands::pair_device,
            commands::link_status,
            commands::exam_schedules,
            commands::participant_login,
            commands::get_session,
            commands::save_answer,
            commands::log_event,
            commands::submit_exam,
            commands::attempt_state,
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
