use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::commands::build_api;
use crate::core::api::SyncApi;
use crate::core::error::AppResult;
use crate::core::Core;

pub struct AppState {
    pub core: Arc<Core>,
    api: tokio::sync::Mutex<Option<Arc<SyncApi>>>,
    /// Sesi operator aktif sampai waktu ini (setelah PIN benar).
    pub operator_until: Mutex<Option<Instant>>,
    /// Ujian sedang berlangsung di jendela ini: jendela tidak boleh ditutup.
    pub exam_active: AtomicBool,
    /// Mencegah dua proses sinkronisasi berjalan bersamaan.
    pub sync_lock: tokio::sync::Mutex<()>,
    /// (waktu sinkron terakhir yang berhasil, pesan error terakhir)
    pub last_sync: Mutex<(Option<String>, Option<String>)>,
}

impl AppState {
    pub fn new(core: Core) -> Self {
        Self {
            core: Arc::new(core),
            api: tokio::sync::Mutex::new(None),
            operator_until: Mutex::new(None),
            exam_active: AtomicBool::new(false),
            sync_lock: tokio::sync::Mutex::new(()),
            last_sync: Mutex::new((None, None)),
        }
    }

    /// Klien server pusat (dibuat ulang setelah konfigurasi berubah).
    pub async fn api(&self) -> AppResult<Arc<SyncApi>> {
        let mut guard = self.api.lock().await;
        if let Some(api) = guard.as_ref() {
            return Ok(api.clone());
        }
        let api = build_api(self)?;
        *guard = Some(api.clone());
        Ok(api)
    }

    pub async fn reset_api(&self) {
        *self.api.lock().await = None;
    }
}
