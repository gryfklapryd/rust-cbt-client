//! Sesi ujian: login peserta offline, pengacakan, jawaban, pelanggaran, pengumpulan.
//! Semua waktu dihitung di backend (bukan di UI) supaya timer tidak bisa dimanipulasi dari WebView.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::crypto;
use super::error::{AppError, AppResult};
use super::package::{ExamSettings, PackageQuestion, Stimulus};
use super::shuffle::{plan_options, plan_questions, SectionPlan};
use super::Core;

/// Toleransi setelah batas waktu untuk jawaban yang sedang dikirim saat waktu habis.
const SAVE_GRACE_SECONDS: i64 = 15;

pub const STATUS_IN_PROGRESS: &str = "in_progress";
pub const STATUS_SUBMITTED: &str = "submitted";
pub const STATUS_TIMED_OUT: &str = "timed_out";
pub const STATUS_TERMINATED: &str = "terminated";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    pub schedule_id: String,
    pub number: String,
    pub password: String,
    pub token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionParticipant {
    pub id: String,
    pub number: String,
    pub name: String,
    pub group_name: Option<String>,
    pub photo_asset_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSection {
    pub id: String,
    pub title: String,
    pub instructions: Option<String>,
    pub question_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionAnswer {
    pub response: Value,
    pub flagged: bool,
    pub time_spent: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionExam {
    pub code: String,
    pub title: String,
    pub instructions: Option<String>,
    pub duration_minutes: i64,
    pub settings: ExamSettings,
}

/// Semua yang dibutuhkan UI untuk menampilkan ujian seorang peserta.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExamSession {
    pub attempt_id: String,
    pub status: String,
    pub participant: SessionParticipant,
    pub exam: SessionExam,
    pub schedule_name: String,
    pub sections: Vec<SessionSection>,
    pub questions: BTreeMap<String, PackageQuestion>,
    pub stimuli: BTreeMap<String, Stimulus>,
    pub option_orders: BTreeMap<String, BTreeMap<String, Vec<String>>>,
    pub answers: BTreeMap<String, SessionAnswer>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub deadline: DateTime<Utc>,
    pub now: DateTime<Utc>,
    pub remaining_seconds: i64,
    /// Tombol "Selesai" baru aktif setelah waktu ini.
    pub can_submit_at: DateTime<Utc>,
    pub violation_count: i64,
    pub current_index: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveAnswerRequest {
    pub attempt_id: String,
    pub question_id: String,
    pub response: Value,
    pub flagged: bool,
    /// Detik yang dihabiskan di soal ini sejak simpanan terakhir.
    #[serde(default)]
    pub time_spent_delta: i64,
    #[serde(default)]
    pub current_index: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptState {
    pub status: String,
    pub remaining_seconds: i64,
    pub violation_count: i64,
    pub deadline: DateTime<Utc>,
    /// PC peserta: false bila server lokal sedang tidak terjangkau.
    #[serde(default = "yes")]
    pub connected: bool,
    /// PC peserta: jumlah perubahan yang menunggu dikirim ke server lokal.
    #[serde(default)]
    pub pending: i64,
}

fn yes() -> bool {
    true
}

impl AttemptState {
    fn new(status: impl Into<String>, deadline: DateTime<Utc>, now: DateTime<Utc>, violation_count: i64) -> Self {
        let status = status.into();
        AttemptState {
            remaining_seconds: if status == STATUS_IN_PROGRESS {
                (deadline - now).num_seconds().max(0)
            } else {
                0
            },
            status,
            violation_count,
            deadline,
            connected: true,
            pending: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedAttachment {
    pub attachment_id: String,
    pub name: String,
    pub size: i64,
    pub mime: String,
}

struct AttemptRow {
    id: String,
    schedule_id: String,
    package_id: String,
    participant_id: String,
    status: String,
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
    deadline: DateTime<Utc>,
    plan: Vec<SectionPlan>,
    option_orders: BTreeMap<String, BTreeMap<String, Vec<String>>>,
    current_index: i64,
    violation_count: i64,
    device_id: Option<String>,
}

fn parse_time(s: &str) -> AppResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|e| AppError::Other(format!("waktu tidak valid: {e}")))
}

pub(crate) fn fmt_time(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

impl Core {
    fn load_attempt(&self, attempt_id: &str) -> AppResult<AttemptRow> {
        let db = self.db();
        let row = db
            .conn
            .query_row(
                "SELECT id, schedule_id, package_id, participant_id, status, started_at, finished_at, deadline,
                        plan, option_orders, current_index, violation_count, device_id
                 FROM attempts WHERE id = ?1",
                [attempt_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, String>(5)?,
                        r.get::<_, Option<String>>(6)?,
                        r.get::<_, String>(7)?,
                        r.get::<_, String>(8)?,
                        r.get::<_, String>(9)?,
                        r.get::<_, i64>(10)?,
                        r.get::<_, i64>(11)?,
                        r.get::<_, Option<String>>(12)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| AppError::user("Sesi ujian tidak ditemukan"))?;
        Ok(AttemptRow {
            id: row.0,
            schedule_id: row.1,
            package_id: row.2,
            participant_id: row.3,
            status: row.4,
            started_at: parse_time(&row.5)?,
            finished_at: row.6.as_deref().map(parse_time).transpose()?,
            deadline: parse_time(&row.7)?,
            plan: serde_json::from_str(&row.8)?,
            option_orders: serde_json::from_str(&row.9)?,
            current_index: row.10,
            violation_count: row.11,
            device_id: row.12,
        })
    }

    fn insert_event(&self, attempt_id: &str, kind: &str, at: DateTime<Utc>, data: Option<&Value>) -> AppResult<()> {
        self.db().conn.execute(
            "INSERT INTO events (attempt_id, type, at, data) VALUES (?1, ?2, ?3, ?4)",
            params![attempt_id, kind, fmt_time(at), data.map(Value::to_string)],
        )?;
        Ok(())
    }

    fn bump_sequence(&self, attempt_id: &str) -> AppResult<()> {
        self.db()
            .conn
            .execute("UPDATE attempts SET sequence = sequence + 1 WHERE id = ?1", [attempt_id])?;
        Ok(())
    }

    fn finish(&self, attempt_id: &str, status: &str, at: DateTime<Utc>, event: &str) -> AppResult<()> {
        let changed = self.db().conn.execute(
            "UPDATE attempts SET status = ?2, finished_at = ?3, sequence = sequence + 1 WHERE id = ?1 AND status = 'in_progress'",
            params![attempt_id, status, fmt_time(at)],
        )?;
        if changed > 0 {
            self.insert_event(attempt_id, event, at, None)?;
        }
        Ok(())
    }

    /// Selesaikan otomatis bila batas waktu terlewati. Mengembalikan status terbaru.
    fn enforce_deadline(&self, a: &AttemptRow, now: DateTime<Utc>) -> AppResult<String> {
        if a.status == STATUS_IN_PROGRESS && now > a.deadline {
            self.finish(&a.id, STATUS_TIMED_OUT, a.deadline, "timeout")?;
            return Ok(STATUS_TIMED_OUT.to_string());
        }
        Ok(a.status.clone())
    }

    /// Login peserta (diverifikasi di server lokal). Membuat attempt baru atau melanjutkan
    /// attempt yang berjalan. `device_id` = PC peserta; attempt terikat ke PC tersebut sampai
    /// proktor mengizinkan pindah komputer.
    pub fn login(&self, req: &LoginRequest, device_id: Option<&str>, now: DateTime<Utc>) -> AppResult<String> {
        let (pkg, _) = self
            .latest_package(&req.schedule_id)?
            .ok_or_else(|| AppError::user("Paket ujian untuk jadwal ini belum diunduh"))?;
        let missing = pkg.assets.iter().filter(|a| !self.asset_present(a)).count();
        if missing > 0 {
            return Err(AppError::user(format!(
                "{missing} media ujian belum lengkap di server lokal. Hubungi proktor."
            )));
        }

        let participant = pkg
            .participant_by_number(&req.number)
            .ok_or_else(|| AppError::user("Nomor peserta atau password salah"))?;
        let ok = participant
            .password_hash
            .as_deref()
            .map(|h| crypto::verify_phc(h, req.password.trim()))
            .unwrap_or(false);
        if !ok {
            return Err(AppError::user("Nomor peserta atau password salah"));
        }
        if let Some(hash) = pkg.schedule.access_token_hash.as_deref() {
            let token = req.token.as_deref().map(|t| t.trim().to_uppercase()).unwrap_or_default();
            if token.is_empty() || !crypto::verify_phc(hash, &token) {
                return Err(AppError::user("Token sesi salah. Minta token kepada pengawas."));
            }
        }

        // Attempt yang sudah ada: lanjutkan di PC yang sama, atau di PC lain setelah diizinkan proktor.
        let existing: Option<String> = self
            .db()
            .conn
            .query_row(
                "SELECT id FROM attempts WHERE schedule_id = ?1 AND participant_id = ?2",
                params![pkg.schedule.id, participant.id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = existing {
            let a = self.load_attempt(&id)?;
            let status = self.enforce_deadline(&a, now)?;
            if status != STATUS_IN_PROGRESS {
                return Err(AppError::user("Anda sudah menyelesaikan ujian ini"));
            }
            if let (Some(bound), Some(dev)) = (a.device_id.as_deref(), device_id) {
                if bound != dev {
                    let name = self.device_name(bound).unwrap_or_else(|| bound.chars().take(8).collect());
                    return Err(AppError::user(format!(
                        "Anda sedang ujian di komputer lain ({name}). Minta proktor mengizinkan pindah komputer."
                    )));
                }
            }
            if a.device_id.is_none() {
                if let Some(dev) = device_id {
                    self.db()
                        .conn
                        .execute("UPDATE attempts SET device_id = ?2 WHERE id = ?1", params![id, dev])?;
                }
            }
            self.insert_event(&id, "resume", now, device_id.map(|d| json!({ "deviceId": d })).as_ref())?;
            self.bump_sequence(&id)?;
            return Ok(id);
        }

        if now < pkg.schedule.start_at {
            return Err(AppError::user(format!(
                "Ujian belum dimulai. Jadwal mulai {}",
                pkg.schedule.start_at.with_timezone(&chrono::Local).format("%d-%m-%Y %H:%M")
            )));
        }
        if now >= pkg.schedule.end_at {
            return Err(AppError::user("Jadwal ujian sudah berakhir"));
        }
        if let Some(late) = pkg.schedule.late_entry_minutes {
            if now > pkg.schedule.start_at + Duration::minutes(late) {
                return Err(AppError::user("Batas waktu terlambat masuk sudah lewat"));
            }
        }

        let attempt_id = uuid::Uuid::new_v4().to_string();
        let plan = plan_questions(&pkg, &attempt_id);
        let all_ids: Vec<String> = plan.iter().flat_map(|s| s.question_ids.clone()).collect();
        if all_ids.is_empty() {
            return Err(AppError::user("Paket ujian tidak berisi soal"));
        }
        let orders = plan_options(&pkg, &attempt_id, &all_ids);
        let deadline = (now + Duration::minutes(pkg.exam.duration_minutes)).min(pkg.schedule.end_at);
        self.db().conn.execute(
            "INSERT INTO attempts (id, schedule_id, package_id, participant_id, status, sequence, started_at, deadline, plan, option_orders, device_id)
             VALUES (?1, ?2, ?3, ?4, 'in_progress', 1, ?5, ?6, ?7, ?8, ?9)",
            params![
                attempt_id,
                pkg.schedule.id,
                pkg.package_id,
                participant.id,
                fmt_time(now),
                fmt_time(deadline),
                serde_json::to_string(&plan)?,
                serde_json::to_string(&orders)?,
                device_id,
            ],
        )?;
        self.insert_event(
            &attempt_id,
            "start",
            now,
            device_id.map(|d| json!({ "deviceId": d })).as_ref(),
        )?;
        Ok(attempt_id)
    }

    pub fn session(&self, attempt_id: &str, now: DateTime<Utc>) -> AppResult<ExamSession> {
        let a = self.load_attempt(attempt_id)?;
        let status = self.enforce_deadline(&a, now)?;
        let a = if status != a.status {
            self.load_attempt(attempt_id)?
        } else {
            a
        };
        let pkg = self.package_by_id(&a.package_id)?;
        let p = pkg
            .participants
            .iter()
            .find(|p| p.id == a.participant_id)
            .ok_or_else(|| AppError::Other("peserta tidak ada di paket".into()))?;

        let sections: Vec<SessionSection> = a
            .plan
            .iter()
            .filter_map(|plan| {
                let s = pkg.exam.sections.iter().find(|s| s.id == plan.section_id)?;
                Some(SessionSection {
                    id: s.id.clone(),
                    title: s.title.clone(),
                    instructions: s.instructions.clone(),
                    question_ids: plan.question_ids.clone(),
                })
            })
            .collect();
        let mut questions = BTreeMap::new();
        let mut stimuli = BTreeMap::new();
        for id in sections.iter().flat_map(|s| s.question_ids.iter()) {
            if let Some(q) = pkg.question(id) {
                if let Some(sid) = &q.stimulus_id {
                    if let Some(st) = pkg.stimuli.iter().find(|s| &s.id == sid) {
                        stimuli.insert(sid.clone(), st.clone());
                    }
                }
                questions.insert(id.clone(), q.clone());
            }
        }

        let answers = {
            let db = self.db();
            let mut stmt = db
                .conn
                .prepare("SELECT question_id, response, flagged, time_spent FROM answers WHERE attempt_id = ?1")?;
            let rows = stmt.query_map([attempt_id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, bool>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })?;
            let mut out = BTreeMap::new();
            for row in rows {
                let (qid, resp, flagged, time_spent) = row?;
                let response = resp.map(|s| serde_json::from_str(&s)).transpose()?.unwrap_or(Value::Null);
                out.insert(
                    qid,
                    SessionAnswer {
                        response,
                        flagged,
                        time_spent,
                    },
                );
            }
            out
        };

        Ok(ExamSession {
            attempt_id: a.id.clone(),
            status: a.status.clone(),
            participant: SessionParticipant {
                id: p.id.clone(),
                number: p.number.clone(),
                name: p.name.clone(),
                group_name: p.group_name.clone(),
                photo_asset_id: p.photo_asset_id.clone(),
            },
            exam: SessionExam {
                code: pkg.exam.code.clone(),
                title: pkg.exam.title.clone(),
                instructions: pkg.exam.instructions.clone(),
                duration_minutes: pkg.exam.duration_minutes,
                settings: pkg.exam.settings.clone(),
            },
            schedule_name: pkg.schedule.name.clone(),
            sections,
            questions,
            stimuli,
            option_orders: a.option_orders.clone(),
            answers,
            started_at: a.started_at,
            finished_at: a.finished_at,
            deadline: a.deadline,
            now,
            remaining_seconds: if a.status == STATUS_IN_PROGRESS {
                (a.deadline - now).num_seconds().max(0)
            } else {
                0
            },
            can_submit_at: a.started_at + Duration::minutes(pkg.exam.settings.min_time_before_submit_minutes),
            violation_count: a.violation_count,
            current_index: a.current_index,
        })
    }

    fn require_active(&self, attempt_id: &str, now: DateTime<Utc>) -> AppResult<AttemptRow> {
        let a = self.load_attempt(attempt_id)?;
        // Jawaban dari antrean PC peserta yang sempat terputus: `now` adalah waktu jawaban
        // dibuat. Diterima bila masih dalam batas waktu walau attempt sudah ditutup otomatis.
        let late_queued = a.status == STATUS_TIMED_OUT && now <= a.deadline + Duration::seconds(SAVE_GRACE_SECONDS);
        if a.status != STATUS_IN_PROGRESS && !late_queued {
            return Err(AppError::user("Ujian sudah selesai"));
        }
        if now > a.deadline + Duration::seconds(SAVE_GRACE_SECONDS) {
            self.enforce_deadline(&a, now)?;
            return Err(AppError::user("Waktu ujian sudah habis"));
        }
        Ok(a)
    }

    pub fn save_answer(&self, req: &SaveAnswerRequest, now: DateTime<Utc>) -> AppResult<AttemptState> {
        let a = self.require_active(&req.attempt_id, now)?;
        if !a.plan.iter().any(|s| s.question_ids.contains(&req.question_id)) {
            return Err(AppError::user("Soal tidak termasuk dalam ujian ini"));
        }
        let response = if req.response.is_null() {
            None
        } else {
            Some(req.response.to_string())
        };
        {
            let db = self.db();
            let previous: Option<Option<String>> = db
                .conn
                .query_row(
                    "SELECT response FROM answers WHERE attempt_id = ?1 AND question_id = ?2",
                    params![req.attempt_id, req.question_id],
                    |r| r.get(0),
                )
                .optional()?;
            let changed = previous.as_ref().map(|p| p != &response).unwrap_or(response.is_some());
            db.conn.execute(
                "INSERT INTO answers (attempt_id, question_id, response, answered_at, time_spent, flagged, change_count)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(attempt_id, question_id) DO UPDATE SET
                   response = excluded.response,
                   answered_at = CASE WHEN ?8 THEN excluded.answered_at ELSE answers.answered_at END,
                   time_spent = answers.time_spent + excluded.time_spent,
                   flagged = excluded.flagged,
                   change_count = answers.change_count + (CASE WHEN ?8 THEN 1 ELSE 0 END)",
                params![
                    req.attempt_id,
                    req.question_id,
                    response,
                    fmt_time(now),
                    req.time_spent_delta.clamp(0, 24 * 3600),
                    req.flagged,
                    i64::from(changed),
                    changed,
                ],
            )?;
            if let Some(i) = req.current_index {
                db.conn.execute(
                    "UPDATE attempts SET current_index = ?2 WHERE id = ?1",
                    params![req.attempt_id, i],
                )?;
            }
            db.conn
                .execute("UPDATE attempts SET sequence = sequence + 1 WHERE id = ?1", [&req.attempt_id])?;
        }
        Ok(AttemptState::new(a.status, a.deadline, now, a.violation_count))
    }

    /// Catat kejadian (mis. `violation`, `focus_lost`). Pelanggaran melewati batas menghentikan ujian.
    pub fn log_event(&self, attempt_id: &str, kind: &str, data: Option<Value>, now: DateTime<Utc>) -> AppResult<AttemptState> {
        let a = self.load_attempt(attempt_id)?;
        if a.status != STATUS_IN_PROGRESS {
            return Ok(AttemptState::new(a.status, a.deadline, now, a.violation_count));
        }
        let kind: String = kind
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .take(64)
            .collect();
        let data = data.filter(Value::is_object);
        self.insert_event(attempt_id, &kind, now, data.as_ref())?;
        let mut violations = a.violation_count;
        if kind == "violation" {
            violations += 1;
            self.db().conn.execute(
                "UPDATE attempts SET violation_count = ?2 WHERE id = ?1",
                params![attempt_id, violations],
            )?;
        }
        self.bump_sequence(attempt_id)?;
        let pkg = self.package_by_id(&a.package_id)?;
        let max = pkg.exam.settings.max_violations;
        if kind == "violation" && max > 0 && violations >= max {
            self.finish(attempt_id, STATUS_TERMINATED, now, "terminated")?;
            return Ok(AttemptState::new(STATUS_TERMINATED, a.deadline, now, violations));
        }
        Ok(AttemptState::new(a.status, a.deadline, now, violations))
    }

    /// Kumpulkan ujian. `manual = true` bila peserta menekan tombol selesai.
    pub fn submit(&self, attempt_id: &str, manual: bool, now: DateTime<Utc>) -> AppResult<AttemptState> {
        let a = self.load_attempt(attempt_id)?;
        // Tombol selesai yang ditekan sebelum batas waktu tetapi baru sampai (antrean PC peserta).
        if manual && a.status == STATUS_TIMED_OUT && now <= a.deadline {
            let db = self.db();
            db.conn.execute(
                "UPDATE attempts SET status = ?2, finished_at = ?3, sequence = sequence + 1 WHERE id = ?1",
                params![attempt_id, STATUS_SUBMITTED, fmt_time(now)],
            )?;
            drop(db);
            self.insert_event(attempt_id, "submit", now, None)?;
            return Ok(AttemptState::new(STATUS_SUBMITTED, a.deadline, now, a.violation_count));
        }
        let status = self.enforce_deadline(&a, now)?;
        if status != STATUS_IN_PROGRESS {
            return Ok(AttemptState::new(status, a.deadline, now, a.violation_count));
        }
        if manual {
            let pkg = self.package_by_id(&a.package_id)?;
            let can_at = a.started_at + Duration::minutes(pkg.exam.settings.min_time_before_submit_minutes);
            if now < can_at {
                let mins = (can_at - now).num_minutes() + 1;
                return Err(AppError::user(format!(
                    "Ujian baru bisa dikumpulkan sekitar {mins} menit lagi"
                )));
            }
        }
        self.finish(attempt_id, STATUS_SUBMITTED, now, "submit")?;
        Ok(AttemptState::new(STATUS_SUBMITTED, a.deadline, now, a.violation_count))
    }

    /// Status terkini attempt (dipanggil berkala oleh PC peserta), sekaligus menutup attempt
    /// yang waktunya habis.
    pub fn attempt_state(&self, attempt_id: &str, now: DateTime<Utc>) -> AppResult<AttemptState> {
        let a = self.load_attempt(attempt_id)?;
        let status = self.enforce_deadline(&a, now)?;
        Ok(AttemptState::new(status, a.deadline, now, a.violation_count))
    }

    /// Daftar media paket milik attempt (untuk disimpan di PC peserta).
    pub fn package_assets_of_attempt(&self, attempt_id: &str) -> AppResult<Vec<super::package::AssetInfo>> {
        let a = self.load_attempt(attempt_id)?;
        Ok(self.package_by_id(&a.package_id)?.assets)
    }

    /// PC peserta pemilik attempt (None = belum terikat / boleh pindah komputer).
    pub fn attempt_device(&self, attempt_id: &str) -> AppResult<Option<String>> {
        Ok(self.load_attempt(attempt_id)?.device_id)
    }

    // ------------------------------------------------------------------ aksi proktor

    /// Lepas ikatan attempt dari PC-nya sehingga peserta bisa login di PC lain.
    pub fn release_device(&self, attempt_id: &str, now: DateTime<Utc>) -> AppResult<()> {
        let a = self.load_attempt(attempt_id)?;
        self.db()
            .conn
            .execute("UPDATE attempts SET device_id = NULL WHERE id = ?1", [attempt_id])?;
        self.insert_event(
            attempt_id,
            "proctor_release_device",
            now,
            a.device_id.map(|d| json!({ "deviceId": d })).as_ref(),
        )?;
        self.bump_sequence(attempt_id)
    }

    /// Tambah waktu ujian seorang peserta (menit, boleh negatif untuk koreksi).
    pub fn extend_time(&self, attempt_id: &str, minutes: i64, now: DateTime<Utc>) -> AppResult<AttemptState> {
        if minutes == 0 || minutes.abs() > 600 {
            return Err(AppError::user("Tambahan waktu harus antara -600 dan 600 menit, bukan 0"));
        }
        let a = self.load_attempt(attempt_id)?;
        let deadline = a.deadline + Duration::minutes(minutes);
        self.db().conn.execute(
            "UPDATE attempts SET deadline = ?2 WHERE id = ?1",
            params![attempt_id, fmt_time(deadline)],
        )?;
        self.insert_event(attempt_id, "proctor_extra_time", now, Some(&json!({ "minutes": minutes })))?;
        self.bump_sequence(attempt_id)?;
        let status = self.enforce_deadline(&self.load_attempt(attempt_id)?, now)?;
        Ok(AttemptState::new(status, deadline, now, a.violation_count))
    }

    /// Hentikan ujian seorang peserta (mis. kecurangan).
    pub fn terminate(&self, attempt_id: &str, reason: Option<&str>, now: DateTime<Utc>) -> AppResult<()> {
        let a = self.load_attempt(attempt_id)?;
        if a.status != STATUS_IN_PROGRESS {
            return Err(AppError::user("Ujian peserta ini sudah selesai"));
        }
        self.finish(attempt_id, STATUS_TERMINATED, now, "proctor_terminate")?;
        if let Some(r) = reason.map(str::trim).filter(|r| !r.is_empty()) {
            self.insert_event(attempt_id, "proctor_note", now, Some(&json!({ "note": r })))?;
        }
        Ok(())
    }

    /// Buka kembali attempt yang sudah selesai / dihentikan agar peserta bisa melanjutkan.
    /// Hitungan pelanggaran direset (riwayat tetap tercatat di event). Bila waktunya sudah
    /// habis, `extra_minutes` wajib diisi.
    pub fn unlock(&self, attempt_id: &str, extra_minutes: Option<i64>, now: DateTime<Utc>) -> AppResult<AttemptState> {
        let a = self.load_attempt(attempt_id)?;
        if a.status == STATUS_IN_PROGRESS {
            return Err(AppError::user("Ujian peserta ini masih berlangsung"));
        }
        let extra = extra_minutes.unwrap_or(0).clamp(0, 600);
        let base = if a.deadline > now { a.deadline } else { now };
        let deadline = if extra > 0 {
            base + Duration::minutes(extra)
        } else {
            a.deadline
        };
        if deadline <= now {
            return Err(AppError::user(
                "Waktu ujian peserta ini sudah habis. Isi tambahan waktu untuk membuka kembali.",
            ));
        }
        self.db().conn.execute(
            "UPDATE attempts SET status = 'in_progress', finished_at = NULL, violation_count = 0, deadline = ?2, sequence = sequence + 1
             WHERE id = ?1",
            params![attempt_id, fmt_time(deadline)],
        )?;
        self.insert_event(
            attempt_id,
            "proctor_unlock",
            now,
            Some(&json!({ "previousStatus": a.status, "extraMinutes": extra })),
        )?;
        Ok(AttemptState::new(STATUS_IN_PROGRESS, deadline, now, 0))
    }

    /// Simpan berkas jawaban (soal unggah berkas) secara lokal; diunggah saat sinkronisasi.
    /// `attachment_id` dari PC peserta membuat pengiriman ulang tidak menggandakan berkas.
    #[allow(clippy::too_many_arguments)]
    pub fn save_attachment(
        &self,
        attempt_id: &str,
        question_id: &str,
        attachment_id: Option<&str>,
        name: &str,
        mime: &str,
        bytes: &[u8],
        now: DateTime<Utc>,
    ) -> AppResult<SavedAttachment> {
        if let Some(id) = attachment_id {
            let existing = self
                .db()
                .conn
                .query_row(
                    "SELECT name, size, mime FROM attachments WHERE id = ?1 AND attempt_id = ?2",
                    params![id, attempt_id],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?)),
                )
                .optional()?;
            if let Some((name, size, mime)) = existing {
                return Ok(SavedAttachment {
                    attachment_id: id.to_string(),
                    name,
                    size,
                    mime,
                });
            }
        }
        let a = self.require_active(attempt_id, now)?;
        let pkg = self.package_by_id(&a.package_id)?;
        let q = pkg
            .question(question_id)
            .ok_or_else(|| AppError::user("Soal tidak ditemukan"))?;
        if q.qtype != "file_upload" {
            return Err(AppError::user("Soal ini tidak menerima unggahan berkas"));
        }
        let max_mb = q.content.get("maxSizeMb").and_then(Value::as_f64).unwrap_or(10.0);
        if bytes.len() as f64 > max_mb * 1024.0 * 1024.0 {
            return Err(AppError::user(format!("Ukuran berkas melebihi {max_mb} MB")));
        }
        let id = match attachment_id {
            Some(id) => uuid::Uuid::parse_str(id)
                .map_err(|_| AppError::user("ID lampiran tidak valid"))?
                .to_string(),
            None => uuid::Uuid::new_v4().to_string(),
        };
        let safe_name: String = name
            .chars()
            .map(|c| if c.is_alphanumeric() || ".-_ ".contains(c) { c } else { '_' })
            .take(200)
            .collect();
        let path = self.data_dir().join("attachments").join(&id);
        std::fs::write(&path, bytes)?;
        let mime = if mime.is_empty() { "application/octet-stream" } else { mime };
        self.db().conn.execute(
            "INSERT INTO attachments (id, attempt_id, question_id, path, name, mime, size) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                attempt_id,
                question_id,
                path.to_string_lossy(),
                safe_name,
                mime,
                bytes.len() as i64
            ],
        )?;
        Ok(SavedAttachment {
            attachment_id: id,
            name: safe_name,
            size: bytes.len() as i64,
            mime: mime.to_string(),
        })
    }

    /// Payload `AttemptUpload` (kontrak server) untuk sebuah attempt.
    pub fn attempt_upload(&self, attempt_id: &str, device_id: &str) -> AppResult<(Value, i64)> {
        let a = self.load_attempt(attempt_id)?;
        let db = self.db();
        let sequence: i64 = db
            .conn
            .query_row("SELECT sequence FROM attempts WHERE id = ?1", [attempt_id], |r| r.get(0))?;
        let mut answers = Vec::new();
        {
            let mut stmt = db.conn.prepare(
                "SELECT question_id, response, answered_at, time_spent, flagged, change_count FROM answers WHERE attempt_id = ?1",
            )?;
            let rows = stmt.query_map([attempt_id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, bool>(4)?,
                    r.get::<_, i64>(5)?,
                ))
            })?;
            for row in rows {
                let (qid, resp, answered_at, time_spent, flagged, change_count) = row?;
                let response: Value = resp.map(|s| serde_json::from_str(&s)).transpose()?.unwrap_or(Value::Null);
                answers.push(json!({
                    "questionId": qid,
                    "response": response,
                    "answeredAt": answered_at,
                    "timeSpentSeconds": time_spent,
                    "flagged": flagged,
                    "changeCount": change_count,
                }));
            }
        }
        let mut events = Vec::new();
        {
            let mut stmt = db
                .conn
                .prepare("SELECT type, at, data FROM events WHERE attempt_id = ?1 ORDER BY id")?;
            let rows = stmt.query_map([attempt_id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?))
            })?;
            for row in rows {
                let (kind, at, data) = row?;
                let mut e = Map::new();
                e.insert("type".into(), Value::String(kind));
                e.insert("at".into(), Value::String(at));
                if let Some(d) = data.map(|s| serde_json::from_str::<Value>(&s)).transpose()? {
                    e.insert("data".into(), d);
                }
                events.push(Value::Object(e));
            }
        }
        let question_order: Vec<String> = a.plan.iter().flat_map(|s| s.question_ids.clone()).collect();
        drop(db);
        // PC peserta tempat ujian dikerjakan; server lokal sebagai cadangan.
        let (device_id, hostname) = match a.device_id.as_deref() {
            Some(d) => (
                format!(
                    "{} ({})",
                    self.device_name(d).unwrap_or_default(),
                    d.chars().take(8).collect::<String>()
                ),
                self.device_name(d),
            ),
            None => (
                device_id.to_string(),
                std::env::var("COMPUTERNAME")
                    .or_else(|_| std::env::var("HOSTNAME"))
                    .ok()
                    .or_else(|| std::fs::read_to_string("/etc/hostname").ok()),
            ),
        };
        let hostname = hostname
            .map(|h| h.trim().chars().take(200).collect::<String>())
            .filter(|h| !h.is_empty());
        let mut client = Map::new();
        client.insert(
            "deviceId".into(),
            json!(device_id.trim().chars().take(200).collect::<String>()),
        );
        client.insert("appVersion".into(), json!(super::api::APP_VERSION));
        if let Some(h) = hostname {
            client.insert("hostname".into(), json!(h));
        }
        let payload = json!({
            "attemptId": a.id,
            "packageId": a.package_id,
            "scheduleId": a.schedule_id,
            "participantId": a.participant_id,
            "status": a.status,
            "sequence": sequence,
            "startedAt": fmt_time(a.started_at),
            "finishedAt": a.finished_at.map(fmt_time),
            "questionOrder": question_order,
            "optionOrders": super::shuffle::flatten_option_orders(&a.option_orders),
            "answers": answers,
            "events": events,
            "client": Value::Object(client),
        });
        Ok((payload, sequence))
    }

    /// Hapus attempt lokal (mis. pengawas mengizinkan peserta mengulang setelah attempt dihapus di server).
    pub fn reset_attempt(&self, attempt_id: &str) -> AppResult<()> {
        let db = self.db();
        db.conn.execute("DELETE FROM attempts WHERE id = ?1", [attempt_id])?;
        Ok(())
    }
}
