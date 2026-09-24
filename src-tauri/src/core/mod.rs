//! Logika inti aplikasi, terpisah dari Tauri supaya bisa dites tanpa GUI.

pub mod api;
pub mod crypto;
pub mod db;
pub mod error;
pub mod exam;
pub mod package;
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

/// Konfigurasi yang boleh dilihat frontend (secret tidak pernah dikirim balik).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigView {
    pub server_url: Option<String>,
    pub site_code: Option<String>,
    pub has_secret: bool,
    pub has_pin: bool,
    pub device_id: String,
    pub device_name: Option<String>,
    pub auto_sync: bool,
    pub configured: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigInput {
    pub server_url: String,
    pub site_code: String,
    /// Kosong = pertahankan secret lama.
    pub secret: Option<String>,
    /// Kosong = pertahankan PIN lama.
    pub operator_pin: Option<String>,
    pub device_name: Option<String>,
    pub auto_sync: Option<bool>,
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

    pub fn config(&self) -> AppResult<ConfigView> {
        let db = self.db();
        let server_url = db.get_config("server_url")?;
        let site_code = db.get_config("site_code")?;
        let has_secret = db.get_config("site_secret")?.is_some();
        Ok(ConfigView {
            configured: server_url.is_some() && site_code.is_some() && has_secret,
            server_url,
            site_code,
            has_secret,
            has_pin: db.get_config("operator_pin_hash")?.is_some(),
            device_id: db.get_config("device_id")?.unwrap_or_default(),
            device_name: db.get_config("device_name")?,
            auto_sync: db.get_config("auto_sync")?.as_deref() != Some("0"),
        })
    }

    pub fn save_config(&self, input: ConfigInput) -> AppResult<ConfigView> {
        let url = input.server_url.trim().trim_end_matches('/').to_string();
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(AppError::user("Alamat server harus diawali http:// atau https://"));
        }
        let code = input.site_code.trim().to_uppercase();
        if code.is_empty() {
            return Err(AppError::user("Kode lokasi wajib diisi"));
        }
        {
            let db = self.db();
            let has_pin = db.get_config("operator_pin_hash")?.is_some();
            let new_pin = input.operator_pin.as_deref().map(str::trim).filter(|p| !p.is_empty());
            if !has_pin && new_pin.is_none() {
                return Err(AppError::user("PIN operator wajib dibuat"));
            }
            if let Some(pin) = new_pin {
                if pin.len() < 4 {
                    return Err(AppError::user("PIN operator minimal 4 karakter"));
                }
                db.set_config("operator_pin_hash", &crypto::hash_pin(pin).map_err(AppError::Other)?)?;
            }
            let secret = input.secret.as_deref().map(str::trim).filter(|s| !s.is_empty());
            if secret.is_none() && db.get_config("site_secret")?.is_none() {
                return Err(AppError::user("Secret lokasi wajib diisi"));
            }
            db.set_config("server_url", &url)?;
            db.set_config("site_code", &code)?;
            if let Some(s) = secret {
                db.set_config("site_secret", s)?;
            }
            match input.device_name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
                Some(n) => db.set_config("device_name", n)?,
                None => db.delete_config("device_name")?,
            }
            if let Some(a) = input.auto_sync {
                db.set_config("auto_sync", if a { "1" } else { "0" })?;
            }
        }
        self.config()
    }

    pub fn verify_pin(&self, pin: &str) -> AppResult<bool> {
        let hash = self.db().get_config("operator_pin_hash")?;
        Ok(hash.map(|h| crypto::verify_phc(&h, pin)).unwrap_or(false))
    }

    pub fn credentials(&self) -> AppResult<Credentials> {
        let db = self.db();
        let get = |k: &str| -> AppResult<String> {
            db.get_config(k)?
                .ok_or_else(|| AppError::NotConfigured("isi alamat server, kode lokasi, dan secret di menu Pengaturan".into()))
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
