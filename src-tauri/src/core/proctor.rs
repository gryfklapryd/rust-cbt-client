//! Mode server lokal: akun proktor (disinkron dari server pusat), PC peserta yang
//! terdaftar, dan log aksi proktor yang dikirim ke server pusat.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::api::ProctorAccount;
use super::crypto;
use super::error::{AppError, AppResult};
use super::exam::fmt_time;
use super::Core;

pub const DEVICE_PENDING: &str = "pending";
pub const DEVICE_APPROVED: &str = "approved";
pub const DEVICE_REVOKED: &str = "revoked";

/// Proktor yang sedang login (di server lokal atau diverifikasi lewat server lokal).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Proctor {
    pub id: String,
    pub username: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    pub status: String,
    pub pairing_code: String,
    pub app_version: Option<String>,
    pub ip: Option<String>,
    pub created_at: String,
    pub approved_at: Option<String>,
    pub approved_by: Option<String>,
    pub last_seen_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairResult {
    pub status: String,
    pub pairing_code: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProctorLogRow {
    pub id: String,
    pub at: String,
    pub username: String,
    pub action: String,
    pub attempt_id: Option<String>,
    pub participant_id: Option<String>,
    pub data: Option<Value>,
    pub synced: bool,
}

/// Target aksi proktor untuk dicatat di log.
#[derive(Default)]
pub struct LogTarget<'a> {
    pub schedule_id: Option<&'a str>,
    pub attempt_id: Option<&'a str>,
    pub participant_id: Option<&'a str>,
}

/// Satu baris pemantauan peserta di dasbor proktor.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorRow {
    pub participant_id: String,
    pub number: String,
    pub name: String,
    pub group_name: Option<String>,
    pub attempt_id: Option<String>,
    /// `not_started` bila belum login.
    pub status: String,
    pub device_id: Option<String>,
    pub device_name: Option<String>,
    pub device_last_seen: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub deadline: Option<String>,
    pub remaining_seconds: i64,
    pub answered: i64,
    pub question_count: i64,
    pub violation_count: i64,
    pub synced: bool,
    pub sync_error: Option<String>,
}

fn token_hash(token: &str) -> String {
    crypto::sha256_hex(token.as_bytes())
}

impl Core {
    // ------------------------------------------------------------------ proktor

    /// Ganti daftar proktor dengan daftar terbaru dari server pusat.
    pub fn store_proctors(&self, proctors: &[ProctorAccount]) -> AppResult<()> {
        let mut db = self.db();
        let tx = db.conn.transaction()?;
        tx.execute("DELETE FROM proctors", [])?;
        for p in proctors {
            tx.execute(
                "INSERT INTO proctors (id, username, name, role, password_hash) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![p.id, p.username.to_lowercase(), p.name, p.role, p.password_hash],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn proctor_count(&self) -> AppResult<i64> {
        Ok(self.db().conn.query_row("SELECT count(*) FROM proctors", [], |r| r.get(0))?)
    }

    /// Verifikasi login proktor dengan hash dari server pusat (tanpa internet).
    pub fn verify_proctor(&self, username: &str, password: &str) -> AppResult<Proctor> {
        let username = username.trim().to_lowercase();
        let row: Option<(String, String, String)> = self
            .db()
            .conn
            .query_row(
                "SELECT id, name, password_hash FROM proctors WHERE username = ?1",
                [&username],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        match row {
            Some((id, name, hash)) if crypto::verify_phc(&hash, password) => Ok(Proctor { id, username, name }),
            _ => Err(AppError::user("Username atau password proktor salah")),
        }
    }

    /// Catat aksi proktor; dikirim ke server pusat saat sinkronisasi.
    pub fn log_proctor(
        &self,
        proctor: &Proctor,
        action: &str,
        target: LogTarget<'_>,
        data: Option<Value>,
        now: DateTime<Utc>,
    ) -> AppResult<()> {
        self.db().conn.execute(
            "INSERT INTO proctor_log (id, at, proctor_id, username, action, schedule_id, attempt_id, participant_id, data)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                uuid::Uuid::new_v4().to_string(),
                fmt_time(now),
                proctor.id,
                proctor.username,
                action,
                target.schedule_id,
                target.attempt_id,
                target.participant_id,
                data.map(|d| d.to_string()),
            ],
        )?;
        Ok(())
    }

    /// Entri log yang belum terkirim, dalam format kontrak `ProctorLogEntry`.
    pub fn unsynced_proctor_log(&self, limit: i64) -> AppResult<Vec<(String, Value)>> {
        let db = self.db();
        let mut stmt = db.conn.prepare(
            "SELECT id, at, proctor_id, username, action, schedule_id, attempt_id, participant_id, data
             FROM proctor_log WHERE synced = 0 ORDER BY at LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, Option<String>>(7)?,
                r.get::<_, Option<String>>(8)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, at, proctor_id, username, action, schedule_id, attempt_id, participant_id, data) = row?;
            let mut entry = json!({
                "id": id,
                "at": at,
                "proctorId": proctor_id,
                "username": username,
                "action": action,
                "scheduleId": schedule_id,
                "attemptId": attempt_id,
                "participantId": participant_id,
            });
            if let Some(d) = data.map(|s| serde_json::from_str::<Value>(&s)).transpose()? {
                entry["data"] = d;
            }
            out.push((id, entry));
        }
        Ok(out)
    }

    pub fn mark_proctor_log_synced(&self, ids: &[String]) -> AppResult<()> {
        let db = self.db();
        for id in ids {
            db.conn.execute("UPDATE proctor_log SET synced = 1 WHERE id = ?1", [id])?;
        }
        Ok(())
    }

    pub fn proctor_log(&self, limit: i64) -> AppResult<Vec<ProctorLogRow>> {
        let db = self.db();
        let mut stmt = db.conn.prepare(
            "SELECT id, at, username, action, attempt_id, participant_id, data, synced
             FROM proctor_log ORDER BY at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, bool>(7)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, at, username, action, attempt_id, participant_id, data, synced) = row?;
            out.push(ProctorLogRow {
                id,
                at,
                username,
                action,
                attempt_id,
                participant_id,
                data: data.map(|s| serde_json::from_str(&s)).transpose()?,
                synced,
            });
        }
        Ok(out)
    }

    // ------------------------------------------------------------------ token sesi

    pub fn store_schedule_token(&self, schedule_id: &str, token: Option<&str>) -> AppResult<()> {
        self.db().conn.execute(
            "INSERT INTO schedule_tokens (schedule_id, access_token) VALUES (?1, ?2)
             ON CONFLICT(schedule_id) DO UPDATE SET access_token = excluded.access_token",
            params![schedule_id, token],
        )?;
        Ok(())
    }

    pub fn schedule_token(&self, schedule_id: &str) -> AppResult<Option<String>> {
        Ok(self
            .db()
            .conn
            .query_row(
                "SELECT access_token FROM schedule_tokens WHERE schedule_id = ?1",
                [schedule_id],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
    }

    // ------------------------------------------------------------------ PC peserta

    /// Pendaftaran PC peserta. Token dibuat PC peserta dan hanya hash-nya yang disimpan.
    /// PC yang sudah disetujui tidak bisa diambil alih dengan token lain.
    pub fn pair_device(
        &self,
        device_id: &str,
        name: &str,
        token: &str,
        app_version: Option<&str>,
        ip: Option<&str>,
        now: DateTime<Utc>,
    ) -> AppResult<PairResult> {
        let device_id = device_id.trim();
        if uuid::Uuid::parse_str(device_id).is_err() {
            return Err(AppError::user("ID perangkat tidak valid"));
        }
        if token.len() < 32 {
            return Err(AppError::user("Token perangkat tidak valid"));
        }
        let name: String = name.trim().chars().take(100).collect();
        let name = if name.is_empty() {
            device_id.chars().take(8).collect()
        } else {
            name
        };
        let hash = token_hash(token);
        let db = self.db();
        let existing: Option<(String, String, String)> = db
            .conn
            .query_row(
                "SELECT token_hash, status, pairing_code FROM devices WHERE id = ?1",
                [device_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        match existing {
            None => {
                let code = crypto::pairing_code();
                db.conn.execute(
                    "INSERT INTO devices (id, name, token_hash, status, pairing_code, app_version, ip, created_at, last_seen_at)
                     VALUES (?1, ?2, ?3, 'pending', ?4, ?5, ?6, ?7, ?7)",
                    params![device_id, name, hash, code, app_version, ip, fmt_time(now)],
                )?;
                Ok(PairResult {
                    status: DEVICE_PENDING.into(),
                    pairing_code: code,
                })
            }
            Some((old_hash, status, _)) if crypto::constant_eq(&old_hash, &hash) && status == DEVICE_REVOKED => {
                // Dicabut proktor: PC mengajukan ulang, perlu disetujui lagi.
                let code = crypto::pairing_code();
                db.conn.execute(
                    "UPDATE devices SET name = ?2, status = 'pending', pairing_code = ?3, app_version = ?4, ip = ?5,
                            approved_at = NULL, approved_by = NULL, last_seen_at = ?6 WHERE id = ?1",
                    params![device_id, name, code, app_version, ip, fmt_time(now)],
                )?;
                Ok(PairResult {
                    status: DEVICE_PENDING.into(),
                    pairing_code: code,
                })
            }
            Some((old_hash, status, code)) if crypto::constant_eq(&old_hash, &hash) => {
                db.conn.execute(
                    "UPDATE devices SET name = ?2, app_version = ?3, ip = ?4, last_seen_at = ?5 WHERE id = ?1",
                    params![device_id, name, app_version, ip, fmt_time(now)],
                )?;
                Ok(PairResult {
                    status,
                    pairing_code: code,
                })
            }
            Some((_, status, _)) if status == DEVICE_APPROVED => Err(AppError::user(
                "Komputer dengan ID ini sudah terdaftar. Minta proktor mencabutnya dulu di menu Perangkat.",
            )),
            Some(_) => {
                // Belum disetujui / sudah dicabut: daftar ulang dengan token baru.
                let code = crypto::pairing_code();
                db.conn.execute(
                    "UPDATE devices SET name = ?2, token_hash = ?3, status = 'pending', pairing_code = ?4, app_version = ?5,
                            ip = ?6, approved_at = NULL, approved_by = NULL, last_seen_at = ?7 WHERE id = ?1",
                    params![device_id, name, hash, code, app_version, ip, fmt_time(now)],
                )?;
                Ok(PairResult {
                    status: DEVICE_PENDING.into(),
                    pairing_code: code,
                })
            }
        }
    }

    /// Cocokkan token Bearer dengan PC terdaftar. Mengembalikan (id, status).
    pub fn device_by_token(&self, token: &str) -> AppResult<Option<(String, String)>> {
        let hash = token_hash(token);
        Ok(self
            .db()
            .conn
            .query_row("SELECT id, status FROM devices WHERE token_hash = ?1", [hash], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?)
    }

    pub fn touch_device(&self, device_id: &str, ip: Option<&str>, now: DateTime<Utc>) -> AppResult<()> {
        self.db().conn.execute(
            "UPDATE devices SET last_seen_at = ?2, ip = coalesce(?3, ip) WHERE id = ?1",
            params![device_id, fmt_time(now), ip],
        )?;
        Ok(())
    }

    pub fn device_name(&self, device_id: &str) -> Option<String> {
        self.db()
            .conn
            .query_row("SELECT name FROM devices WHERE id = ?1", [device_id], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }

    pub fn set_device_status(&self, device_id: &str, status: &str, by: &Proctor, now: DateTime<Utc>) -> AppResult<()> {
        let changed = if status == DEVICE_APPROVED {
            self.db().conn.execute(
                "UPDATE devices SET status = ?2, approved_at = ?3, approved_by = ?4 WHERE id = ?1",
                params![device_id, status, fmt_time(now), by.username],
            )?
        } else {
            self.db()
                .conn
                .execute("UPDATE devices SET status = ?2 WHERE id = ?1", params![device_id, status])?
        };
        if changed == 0 {
            return Err(AppError::user("Perangkat tidak ditemukan"));
        }
        Ok(())
    }

    pub fn delete_device(&self, device_id: &str) -> AppResult<()> {
        self.db().conn.execute("DELETE FROM devices WHERE id = ?1", [device_id])?;
        Ok(())
    }

    pub fn devices(&self) -> AppResult<Vec<Device>> {
        let db = self.db();
        let mut stmt = db.conn.prepare(
            "SELECT id, name, status, pairing_code, app_version, ip, created_at, approved_at, approved_by, last_seen_at
             FROM devices ORDER BY status = 'pending' DESC, name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Device {
                id: r.get(0)?,
                name: r.get(1)?,
                status: r.get(2)?,
                pairing_code: r.get(3)?,
                app_version: r.get(4)?,
                ip: r.get(5)?,
                created_at: r.get(6)?,
                approved_at: r.get(7)?,
                approved_by: r.get(8)?,
                last_seen_at: r.get(9)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    // ------------------------------------------------------------------ pemantauan

    /// Status semua peserta sebuah jadwal (termasuk yang belum login).
    pub fn monitor(&self, schedule_id: &str, now: DateTime<Utc>) -> AppResult<Vec<MonitorRow>> {
        let Some((pkg, _)) = self.latest_package(schedule_id)? else {
            return Ok(vec![]);
        };
        struct Row {
            id: String,
            participant_id: String,
            status: String,
            device_id: Option<String>,
            device_name: Option<String>,
            device_last_seen: Option<String>,
            started_at: String,
            finished_at: Option<String>,
            deadline: String,
            plan: String,
            answered: i64,
            violation_count: i64,
            synced: bool,
            sync_error: Option<String>,
        }
        let rows: Vec<Row> = {
            let db = self.db();
            let mut stmt = db.conn.prepare(
                "SELECT a.id, a.participant_id, a.status, a.device_id, d.name, d.last_seen_at, a.started_at, a.finished_at,
                        a.deadline, a.plan,
                        (SELECT count(*) FROM answers w WHERE w.attempt_id = a.id AND w.response IS NOT NULL),
                        a.violation_count, a.sequence <= a.synced_sequence, a.sync_error
                 FROM attempts a LEFT JOIN devices d ON d.id = a.device_id
                 WHERE a.schedule_id = ?1",
            )?;
            let rows = stmt.query_map([schedule_id], |r| {
                Ok(Row {
                    id: r.get(0)?,
                    participant_id: r.get(1)?,
                    status: r.get(2)?,
                    device_id: r.get(3)?,
                    device_name: r.get(4)?,
                    device_last_seen: r.get(5)?,
                    started_at: r.get(6)?,
                    finished_at: r.get(7)?,
                    deadline: r.get(8)?,
                    plan: r.get(9)?,
                    answered: r.get(10)?,
                    violation_count: r.get(11)?,
                    synced: r.get(12)?,
                    sync_error: r.get(13)?,
                })
            })?;
            rows.collect::<Result<_, _>>()?
        };
        let mut by_participant: HashMap<String, Row> = rows.into_iter().map(|r| (r.participant_id.clone(), r)).collect();
        let mut out = Vec::new();
        for p in &pkg.participants {
            let row = match by_participant.remove(&p.id) {
                Some(a) => {
                    let deadline = DateTime::parse_from_rfc3339(&a.deadline).map(|d| d.with_timezone(&Utc)).ok();
                    let in_progress = a.status == super::exam::STATUS_IN_PROGRESS;
                    let question_count = serde_json::from_str::<Vec<super::shuffle::SectionPlan>>(&a.plan)
                        .map(|plan| plan.iter().map(|s| s.question_ids.len() as i64).sum())
                        .unwrap_or(0);
                    MonitorRow {
                        participant_id: p.id.clone(),
                        number: p.number.clone(),
                        name: p.name.clone(),
                        group_name: p.group_name.clone(),
                        attempt_id: Some(a.id),
                        // Waktu habis tetapi belum ada aksi yang menutupnya: tampilkan sebagai habis.
                        status: if in_progress && deadline.is_some_and(|d| d < now) {
                            super::exam::STATUS_TIMED_OUT.into()
                        } else {
                            a.status
                        },
                        device_id: a.device_id,
                        device_name: a.device_name,
                        device_last_seen: a.device_last_seen,
                        started_at: Some(a.started_at),
                        finished_at: a.finished_at,
                        deadline: Some(a.deadline),
                        remaining_seconds: match (in_progress, deadline) {
                            (true, Some(d)) => (d - now).num_seconds().max(0),
                            _ => 0,
                        },
                        answered: a.answered,
                        question_count,
                        violation_count: a.violation_count,
                        synced: a.synced,
                        sync_error: a.sync_error,
                    }
                }
                None => MonitorRow {
                    participant_id: p.id.clone(),
                    number: p.number.clone(),
                    name: p.name.clone(),
                    group_name: p.group_name.clone(),
                    attempt_id: None,
                    status: "not_started".into(),
                    device_id: None,
                    device_name: None,
                    device_last_seen: None,
                    started_at: None,
                    finished_at: None,
                    deadline: None,
                    remaining_seconds: 0,
                    answered: 0,
                    question_count: 0,
                    violation_count: 0,
                    synced: true,
                    sync_error: None,
                },
            };
            out.push(row);
        }
        out.sort_by(|a, b| a.number.cmp(&b.number));
        Ok(out)
    }

    /// Jumlah PC peserta yang menghubungi server lokal dalam `seconds` detik terakhir.
    pub fn devices_online(&self, seconds: i64, now: DateTime<Utc>) -> AppResult<i64> {
        let since = fmt_time(now - chrono::Duration::seconds(seconds));
        Ok(self.db().conn.query_row(
            "SELECT count(*) FROM devices WHERE status = 'approved' AND last_seen_at >= ?1",
            [since],
            |r| r.get(0),
        )?)
    }
}
