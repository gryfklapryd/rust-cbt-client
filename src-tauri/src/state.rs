use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::core::api::SyncApi;
use crate::core::error::{AppError, AppResult};
use crate::core::participant::ParticipantLink;
use crate::core::proctor::Proctor;
use crate::core::Core;

/// Sesi proktor berakhir setelah sekian lama tanpa aktivitas.
const PROCTOR_IDLE: Duration = Duration::from_secs(30 * 60);

/// Status API LAN (mode server lokal).
#[derive(Default, Clone)]
pub struct LanServerStatus {
    pub port: Option<u16>,
    pub error: Option<String>,
}

pub struct AppState {
    pub core: Arc<Core>,
    api: tokio::sync::Mutex<Option<Arc<SyncApi>>>,
    link: tokio::sync::Mutex<Option<Arc<ParticipantLink>>>,
    /// Proktor yang sedang login beserta batas sesinya.
    proctor: Mutex<Option<(Proctor, Instant)>>,
    /// Ujian sedang berlangsung di jendela ini: jendela tidak boleh ditutup.
    pub exam_active: AtomicBool,
    /// Mencegah dua proses sinkronisasi berjalan bersamaan.
    pub sync_lock: tokio::sync::Mutex<()>,
    /// (waktu sinkron terakhir yang berhasil, pesan error terakhir)
    pub last_sync: Mutex<(Option<String>, Option<String>)>,
    pub lan_server: Mutex<LanServerStatus>,
    pub lan_task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}

impl AppState {
    pub fn new(core: Core) -> Self {
        Self {
            core: Arc::new(core),
            api: tokio::sync::Mutex::new(None),
            link: tokio::sync::Mutex::new(None),
            proctor: Mutex::new(None),
            exam_active: AtomicBool::new(false),
            sync_lock: tokio::sync::Mutex::new(()),
            last_sync: Mutex::new((None, None)),
            lan_server: Mutex::new(LanServerStatus::default()),
            lan_task: Mutex::new(None),
        }
    }

    /// Klien server pusat (mode server lokal), dibuat ulang setelah konfigurasi berubah.
    pub async fn api(&self) -> AppResult<Arc<SyncApi>> {
        if !self.core.is_server() {
            return Err(AppError::user("Hanya server lokal yang terhubung ke server pusat"));
        }
        let mut guard = self.api.lock().await;
        if let Some(api) = guard.as_ref() {
            return Ok(api.clone());
        }
        let api = Arc::new(SyncApi::new(self.core.credentials()?)?);
        *guard = Some(api.clone());
        Ok(api)
    }

    /// Penghubung ke server lokal (mode PC peserta).
    pub async fn link(&self) -> AppResult<Arc<ParticipantLink>> {
        if !self.core.is_participant() {
            return Err(AppError::user("Menu ini hanya untuk PC peserta"));
        }
        let mut guard = self.link.lock().await;
        if let Some(link) = guard.as_ref() {
            return Ok(link.clone());
        }
        let link = Arc::new(ParticipantLink::new(self.core.clone())?);
        *guard = Some(link.clone());
        Ok(link)
    }

    pub async fn reset_clients(&self) {
        *self.api.lock().await = None;
        *self.link.lock().await = None;
    }

    pub fn set_proctor(&self, p: Option<Proctor>) {
        *self.proctor.lock().unwrap_or_else(|e| e.into_inner()) = p.map(|p| (p, Instant::now() + PROCTOR_IDLE));
    }

    pub fn current_proctor(&self) -> Option<Proctor> {
        let guard = self.proctor.lock().unwrap_or_else(|e| e.into_inner());
        guard
            .as_ref()
            .filter(|(_, until)| Instant::now() < *until)
            .map(|(p, _)| p.clone())
    }

    /// Proktor yang login; sesinya diperpanjang setiap dipakai.
    pub fn require_proctor(&self) -> AppResult<Proctor> {
        let mut guard = self.proctor.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_mut() {
            Some((p, until)) if Instant::now() < *until => {
                *until = Instant::now() + PROCTOR_IDLE;
                Ok(p.clone())
            }
            _ => {
                *guard = None;
                Err(AppError::user("Sesi proktor berakhir. Silakan login proktor lagi."))
            }
        }
    }
}
