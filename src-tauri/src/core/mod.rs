//! Logika inti aplikasi, terpisah dari Tauri supaya bisa dites tanpa GUI.

pub mod api;
pub mod crypto;
pub mod db;
pub mod error;
pub mod exam;
pub mod lan;
pub mod lan_client;
pub mod package;
pub mod participant;
pub mod proctor;
pub mod shuffle;
pub mod sync;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use api::Credentials;
use db::Db;
use error::{AppError, AppResult};
use package::{AssetInfo, ExamPackage};

pub struct Core {
    db: Mutex<Db>,
    data_dir: PathBuf,
}

/// Mode aplikasi: server lokal titik ujian atau PC peserta.
pub const MODE_SERVER: &str = "server";
pub const MODE_PARTICIPANT: &str = "participant";

/// Port bawaan API LAN server lokal.
pub const DEFAULT_LAN_PORT: u16 = 8787;

/// Konfigurasi yang boleh dilihat frontend (secret dan token perangkat tidak pernah dikirim balik).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigView {
    /// `server` / `participant`, `None` sebelum pengaturan awal.
    pub mode: Option<String>,
    pub device_id: String,
    pub device_name: Option<String>,
    pub configured: bool,
    // Mode server lokal
    pub server_url: Option<String>,
    pub site_code: Option<String>,
    pub has_secret: bool,
    pub auto_sync: bool,
    pub lan_port: u16,
    // Mode PC peserta
    pub lan_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerConfigInput {
    pub server_url: String,
    pub site_code: String,
    /// Kosong = pertahankan secret lama.
    pub secret: Option<String>,
    pub device_name: Option<String>,
    pub auto_sync: Option<bool>,
    pub lan_port: Option<u16>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantConfigInput {
    /// Alamat server lokal, mis. `192.168.1.10` atau `http://192.168.1.10:8787`.
    pub lan_url: String,
    pub device_name: Option<String>,
}

/// Lengkapi alamat server lokal: tambah `http://` dan port bawaan bila tidak ditulis.
pub fn normalize_lan_url(input: &str) -> AppResult<String> {
    let raw = input.trim().trim_end_matches('/');
    if raw.is_empty() {
        return Err(AppError::user("Alamat server lokal wajib diisi"));
    }
    let with_scheme = if raw.contains("://") {
        raw.to_string()
    } else {
        format!("http://{raw}")
    };
    let mut url =
        reqwest::Url::parse(&with_scheme).map_err(|_| AppError::user("Alamat server lokal tidak valid, contoh: 192.168.1.10"))?;
    if url.host_str().is_none() || !(url.scheme() == "http" || url.scheme() == "https") {
        return Err(AppError::user("Alamat server lokal tidak valid, contoh: 192.168.1.10"));
    }
    if url.port().is_none() && !raw.contains("://") {
        let _ = url.set_port(Some(DEFAULT_LAN_PORT));
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptCounts {
    pub total: i64,
    pub in_progress: i64,
    pub finished: i64,
    pub unsynced: i64,
    pub sync_errors: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSchedule {
    pub schedule_id: String,
    pub name: String,
    pub exam_code: String,
    pub exam_title: String,
    pub duration_minutes: i64,
    pub start_at: DateTime<Utc>,
    pub end_at: DateTime<Utc>,
    pub package_id: String,
    pub package_version: i64,
    pub checksum: String,
    pub downloaded_at: String,
    pub participant_count: usize,
    pub question_count: usize,
    pub asset_count: usize,
    pub assets_missing: usize,
    pub requires_token: bool,
    /// Token sesi asli (hanya untuk dasbor proktor, tidak dikirim ke PC peserta).
    pub access_token: Option<String>,
    pub attempts: AttemptCounts,
}

impl Core {
    pub fn open(data_dir: &Path) -> AppResult<Self> {
        std::fs::create_dir_all(data_dir.join("assets"))?;
        std::fs::create_dir_all(data_dir.join("attachments"))?;
        let db = Db::open(&data_dir.join("cbt.sqlite3"))?;
        let core = Core {
            db: Mutex::new(db),
            data_dir: data_dir.to_path_buf(),
        };
        core.ensure_device_id()?;
        Ok(core)
    }

    pub fn db(&self) -> MutexGuard<'_, Db> {
        // Panic di thread lain tidak boleh membuat aplikasi terkunci selamanya.
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    fn ensure_device_id(&self) -> AppResult<()> {
        let db = self.db();
        if db.get_config("device_id")?.is_none() {
            db.set_config("device_id", &uuid::Uuid::new_v4().to_string())?;
        }
        Ok(())
    }

    // ------------------------------------------------------------------ konfigurasi

    pub fn mode(&self) -> AppResult<Option<String>> {
        self.db().get_config("mode")
    }

    pub fn is_server(&self) -> bool {
        self.mode().ok().flatten().as_deref() == Some(MODE_SERVER)
    }

    pub fn is_participant(&self) -> bool {
        self.mode().ok().flatten().as_deref() == Some(MODE_PARTICIPANT)
    }

    pub fn config(&self) -> AppResult<ConfigView> {
        let db = self.db();
        let mode = db.get_config("mode")?;
        let server_url = db.get_config("server_url")?;
        let site_code = db.get_config("site_code")?;
        let has_secret = db.get_config("site_secret")?.is_some();
        let lan_url = db.get_config("lan_url")?;
        let configured = match mode.as_deref() {
            Some(MODE_SERVER) => server_url.is_some() && site_code.is_some() && has_secret,
            Some(MODE_PARTICIPANT) => lan_url.is_some() && db.get_config("device_token")?.is_some(),
            _ => false,
        };
        Ok(ConfigView {
            mode,
            device_id: db.get_config("device_id")?.unwrap_or_default(),
            device_name: db.get_config("device_name")?,
            configured,
            server_url,
            site_code,
            has_secret,
            auto_sync: db.get_config("auto_sync")?.as_deref() != Some("0"),
            lan_port: db
                .get_config("lan_port")?
                .and_then(|p| p.parse().ok())
                .unwrap_or(DEFAULT_LAN_PORT),
            lan_url,
        })
    }

    /// Mode hanya bisa dipilih sekali; menggantinya butuh folder data baru.
    fn set_mode(&self, db: &Db, mode: &str) -> AppResult<()> {
        match db.get_config("mode")? {
            Some(m) if m != mode => Err(AppError::user(
                "Komputer ini sudah diatur dengan mode lain. Hapus folder data aplikasi untuk mengganti mode.",
            )),
            Some(_) => Ok(()),
            None => db.set_config("mode", mode),
        }
    }

    fn set_device_name(db: &Db, name: Option<&str>) -> AppResult<()> {
        match name.map(str::trim).filter(|n| !n.is_empty()) {
            Some(n) => db.set_config("device_name", &n.chars().take(100).collect::<String>()),
            None => db.delete_config("device_name"),
        }
    }

    pub fn save_server_config(&self, input: ServerConfigInput) -> AppResult<ConfigView> {
        let url = input.server_url.trim().trim_end_matches('/').to_string();
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(AppError::user("Alamat server pusat harus diawali http:// atau https://"));
        }
        let code = input.site_code.trim().to_uppercase();
        if code.is_empty() {
            return Err(AppError::user("Kode lokasi wajib diisi"));
        }
        if input.lan_port == Some(0) {
            return Err(AppError::user("Port LAN tidak valid"));
        }
        {
            let db = self.db();
            let secret = input.secret.as_deref().map(str::trim).filter(|s| !s.is_empty());
            if secret.is_none() && db.get_config("site_secret")?.is_none() {
                return Err(AppError::user("Secret lokasi wajib diisi"));
            }
            self.set_mode(&db, MODE_SERVER)?;
            db.set_config("server_url", &url)?;
            // Ganti lokasi = daftar proktor lama tidak berlaku lagi.
            if db.get_config("site_code")?.is_some_and(|c| c != code) {
                db.conn.execute("DELETE FROM proctors", [])?;
            }
            db.set_config("site_code", &code)?;
            if let Some(s) = secret {
                db.set_config("site_secret", s)?;
            }
            Self::set_device_name(&db, input.device_name.as_deref())?;
            if let Some(a) = input.auto_sync {
                db.set_config("auto_sync", if a { "1" } else { "0" })?;
            }
            if let Some(p) = input.lan_port {
                db.set_config("lan_port", &p.to_string())?;
            }
        }
        self.config()
    }

    pub fn save_participant_config(&self, input: ParticipantConfigInput) -> AppResult<ConfigView> {
        let url = normalize_lan_url(&input.lan_url)?;
        {
            let db = self.db();
            self.set_mode(&db, MODE_PARTICIPANT)?;
            db.set_config("lan_url", &url)?;
            Self::set_device_name(&db, input.device_name.as_deref())?;
            // Token perangkat dibuat sekali; proktor menyetujuinya di server lokal.
            if db.get_config("device_token")?.is_none() {
                db.set_config("device_token", &crypto::random_token())?;
            }
        }
        self.config()
    }

    /// Kredensial server pusat (mode server lokal).
    pub fn credentials(&self) -> AppResult<Credentials> {
        let db = self.db();
        let get = |k: &str| -> AppResult<String> {
            db.get_config(k)?.ok_or_else(|| {
                AppError::NotConfigured("isi alamat server pusat, kode lokasi, dan secret di menu Pengaturan".into())
            })
        };
        let device_id = get("device_id")?;
        Ok(Credentials {
            server_url: get("server_url")?,
            site_code: get("site_code")?,
            secret: get("site_secret")?,
            device_id: db
                .get_config("device_name")?
                .map(|n| format!("{n} ({})", &device_id[..8]))
                .unwrap_or(device_id),
        })
    }

    // ------------------------------------------------------------------ paket & aset

    /// Simpan paket hasil unduhan setelah memverifikasi checksum dan format.
    pub fn store_package(&self, bytes: &[u8], checksum: &str) -> AppResult<ExamPackage> {
        let actual = crypto::sha256_hex(bytes);
        if actual != checksum {
            return Err(AppError::Other(format!(
                "checksum paket tidak cocok (diterima {actual}, seharusnya {checksum})"
            )));
        }
        let pkg = ExamPackage::parse(bytes).map_err(AppError::Other)?;
        let json = std::str::from_utf8(bytes).map_err(|e| AppError::Other(e.to_string()))?;
        self.db().conn.execute(
            "INSERT INTO packages (package_id, schedule_id, version, checksum, json, downloaded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(package_id) DO UPDATE SET checksum = excluded.checksum, json = excluded.json, downloaded_at = excluded.downloaded_at",
            params![pkg.package_id, pkg.schedule.id, pkg.version, checksum, json, Utc::now().to_rfc3339()],
        )?;
        Ok(pkg)
    }

    pub fn latest_package(&self, schedule_id: &str) -> AppResult<Option<(ExamPackage, String)>> {
        let row: Option<(String, String)> = self
            .db()
            .conn
            .query_row(
                "SELECT json, checksum FROM packages WHERE schedule_id = ?1 ORDER BY version DESC LIMIT 1",
                [schedule_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        row.map(|(json, checksum)| Ok((serde_json::from_str(&json)?, checksum)))
            .transpose()
    }

    pub fn package_by_id(&self, package_id: &str) -> AppResult<ExamPackage> {
        let json: String = self
            .db()
            .conn
            .query_row("SELECT json FROM packages WHERE package_id = ?1", [package_id], |r| r.get(0))
            .optional()?
            .ok_or_else(|| AppError::Other(format!("paket {package_id} tidak ada di komputer ini")))?;
        Ok(serde_json::from_str(&json)?)
    }

    pub fn asset_path(&self, asset_id: &str) -> PathBuf {
        // ID aset berupa UUID; saring karakter lain untuk mencegah path traversal.
        let safe: String = asset_id.chars().filter(|c| c.is_ascii_hexdigit() || *c == '-').collect();
        self.data_dir.join("assets").join(safe)
    }

    pub fn asset_present(&self, info: &AssetInfo) -> bool {
        std::fs::metadata(self.asset_path(&info.id))
            .map(|m| m.len() as i64 == info.size)
            .unwrap_or(false)
    }

    pub fn save_asset(&self, info: &AssetInfo, bytes: &[u8]) -> AppResult<()> {
        let actual = crypto::sha256_hex(bytes);
        if actual != info.sha256 {
            return Err(AppError::Other(format!("checksum media {} tidak cocok", info.filename)));
        }
        let path = self.asset_path(&info.id);
        let tmp = path.with_extension("part");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    /// MIME aset dari paket mana pun yang memuatnya (untuk protokol `cbtasset://`).
    pub fn asset_mime(&self, asset_id: &str) -> Option<String> {
        let db = self.db();
        // PC peserta: MIME dicatat saat media diambil dari server lokal.
        if let Ok(Some(mime)) = db
            .conn
            .query_row("SELECT mime FROM asset_meta WHERE id = ?1", [asset_id], |r| {
                r.get::<_, String>(0)
            })
            .optional()
        {
            return Some(mime);
        }
        let mut stmt = db
            .conn
            .prepare("SELECT json FROM packages ORDER BY downloaded_at DESC")
            .ok()?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).ok()?;
        for json in rows.flatten() {
            if let Ok(pkg) = serde_json::from_str::<ExamPackage>(&json) {
                if let Some(a) = pkg.assets.iter().find(|a| a.id == asset_id) {
                    return Some(a.mime.clone());
                }
            }
        }
        None
    }

    pub fn attempt_counts(&self, schedule_id: Option<&str>) -> AppResult<AttemptCounts> {
        let db = self.db();
        let sql = "SELECT count(*),
                    coalesce(sum(status = 'in_progress'), 0),
                    coalesce(sum(status <> 'in_progress'), 0),
                    coalesce(sum(sequence > synced_sequence), 0),
                    coalesce(sum(sync_error IS NOT NULL), 0)
             FROM attempts WHERE (?1 IS NULL OR schedule_id = ?1)";
        Ok(db.conn.query_row(sql, [schedule_id], |r| {
            Ok(AttemptCounts {
                total: r.get(0)?,
                in_progress: r.get(1)?,
                finished: r.get(2)?,
                unsynced: r.get(3)?,
                sync_errors: r.get(4)?,
            })
        })?)
    }

    /// Jadwal yang paketnya sudah ada di komputer ini (versi terbaru per jadwal).
    pub fn local_schedules(&self) -> AppResult<Vec<LocalSchedule>> {
        let rows: Vec<(String, String, String)> = {
            let db = self.db();
            let mut stmt = db.conn.prepare(
                "SELECT p.json, p.checksum, p.downloaded_at FROM packages p
                 WHERE p.version = (SELECT max(version) FROM packages q WHERE q.schedule_id = p.schedule_id)",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            rows.collect::<Result<_, _>>()?
        };
        let mut out = Vec::new();
        for (json, checksum, downloaded_at) in rows {
            let pkg: ExamPackage = serde_json::from_str(&json)?;
            let missing = pkg.assets.iter().filter(|a| !self.asset_present(a)).count();
            out.push(LocalSchedule {
                schedule_id: pkg.schedule.id.clone(),
                name: pkg.schedule.name.clone(),
                exam_code: pkg.exam.code.clone(),
                exam_title: pkg.exam.title.clone(),
                duration_minutes: pkg.exam.duration_minutes,
                start_at: pkg.schedule.start_at,
                end_at: pkg.schedule.end_at,
                package_id: pkg.package_id.clone(),
                package_version: pkg.version,
                checksum,
                downloaded_at,
                participant_count: pkg.participants.len(),
                question_count: pkg.questions.len(),
                asset_count: pkg.assets.len(),
                assets_missing: missing,
                requires_token: pkg.schedule.access_token_hash.is_some(),
                access_token: self.schedule_token(&pkg.schedule.id)?,
                attempts: self.attempt_counts(Some(&pkg.schedule.id))?,
            });
        }
        out.sort_by_key(|s| s.start_at);
        Ok(out)
    }

    /// Hapus paket lama yang tidak lagi dirujuk attempt, beserta paket jadwal yang sudah lewat lama.
    pub fn prune_packages(&self) -> AppResult<usize> {
        let db = self.db();
        let n = db.conn.execute(
            "DELETE FROM packages WHERE package_id NOT IN (SELECT package_id FROM attempts)
               AND version < (SELECT max(version) FROM packages q WHERE q.schedule_id = packages.schedule_id)",
            [],
        )?;
        Ok(n)
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    pub fn temp_core() -> (Core, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::open(dir.path()).unwrap();
        (core, dir)
    }
}

#[cfg(test)]
mod exam_tests;
