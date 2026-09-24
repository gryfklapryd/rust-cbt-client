//! Mode PC peserta: semua aksi ujian dikirim ke server lokal. Setiap aksi dicatat dulu di
//! antrean lokal (outbox) lalu dikirim berurutan, sehingga bila jaringan ke server lokal
//! terputus peserta tetap bisa mengerjakan dan perubahan terkirim otomatis saat tersambung lagi.

use std::collections::HashMap;
use std::sync::Arc;

use base64::Engine;
use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::error::{AppError, AppResult};
use super::exam::{
    fmt_time, AttemptState, ExamSession, LoginRequest, SaveAnswerRequest, SavedAttachment, SessionAnswer, STATUS_IN_PROGRESS,
    STATUS_SUBMITTED, STATUS_TERMINATED, STATUS_TIMED_OUT,
};
use super::lan::{AnswerBody, AttachmentBody, EventBody, LanSchedule, LanSession, SubmitBody};
use super::lan_client::LanClient;
use super::package::AssetInfo;
use super::proctor::{PairResult, Proctor};
use super::Core;

const KIND_ANSWER: &str = "answer";
const KIND_EVENT: &str = "event";
const KIND_SUBMIT: &str = "submit";
const KIND_ATTACHMENT: &str = "attachment";

/// Kode error server lokal yang berarti PC ini (sementara) tidak boleh mengirim apa pun.
const BLOCKING_CODES: &[&str] = &["unpaired", "pending", "revoked", "not_configured"];
/// Kode error yang berarti attempt sudah bukan milik PC ini.
const MOVED_CODES: &[&str] = &["moved", "other_device"];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueuedAttachment {
    attachment_id: String,
    question_id: String,
    name: String,
    mime: String,
    path: String,
    at: DateTime<Utc>,
}

struct OutboxItem {
    id: i64,
    attempt_id: String,
    kind: String,
    body: String,
}

#[derive(Default)]
pub struct FlushReport {
    /// Status attempt terakhir dari server lokal.
    states: HashMap<String, AttemptState>,
    /// Item yang ditolak server lokal (id item, error).
    rejected: Vec<(i64, AppError)>,
    /// Pengiriman berhenti karena gangguan (jaringan, PC belum disetujui, ...).
    pub blocked: Option<AppError>,
    pub sent: usize,
}

/// Status koneksi PC peserta ke server lokal, untuk ditampilkan di layar.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkStatus {
    pub connected: bool,
    pub pending: i64,
    pub error: Option<String>,
}

impl Core {
    fn set_asset_mime(&self, id: &str, mime: &str) -> AppResult<()> {
        self.db().conn.execute(
            "INSERT INTO asset_meta (id, mime) VALUES (?1, ?2) ON CONFLICT(id) DO UPDATE SET mime = excluded.mime",
            params![id, mime],
        )?;
        Ok(())
    }

    /// Jumlah perubahan yang belum terkirim ke server lokal.
    pub fn outbox_count(&self, attempt_id: Option<&str>) -> AppResult<i64> {
        Ok(self.db().conn.query_row(
            "SELECT count(*) FROM outbox WHERE (?1 IS NULL OR attempt_id = ?1)",
            [attempt_id],
            |r| r.get(0),
        )?)
    }

    fn cache_session(&self, s: &ExamSession) -> AppResult<()> {
        self.db().conn.execute(
            "INSERT INTO cached_sessions (attempt_id, json, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(attempt_id) DO UPDATE SET json = excluded.json, updated_at = excluded.updated_at",
            params![s.attempt_id, serde_json::to_string(s)?, fmt_time(Utc::now())],
        )?;
        Ok(())
    }

    fn cached_session(&self, attempt_id: &str) -> AppResult<Option<ExamSession>> {
        let json: Option<String> = self
            .db()
            .conn
            .query_row("SELECT json FROM cached_sessions WHERE attempt_id = ?1", [attempt_id], |r| {
                r.get(0)
            })
            .optional()?;
        json.map(|j| serde_json::from_str(&j).map_err(AppError::from)).transpose()
    }

    /// Ubah sesi tersimpan (bila ada) lalu simpan lagi.
    fn update_cached<F: FnOnce(&mut ExamSession)>(&self, attempt_id: &str, f: F) -> AppResult<()> {
        if let Some(mut s) = self.cached_session(attempt_id)? {
            f(&mut s);
            self.cache_session(&s)?;
        }
        Ok(())
    }

    fn delete_cached_session(&self, attempt_id: &str) -> AppResult<()> {
        self.db()
            .conn
            .execute("DELETE FROM cached_sessions WHERE attempt_id = ?1", [attempt_id])?;
        Ok(())
    }

    /// Hapus salinan sesi yang sudah tidak diperlukan: ujiannya selesai (atau batas waktunya
    /// lewat lebih dari 1 jam) dan semua datanya sudah terkirim ke server lokal. PC yang
    /// dipakai bergantian tidak menyimpan jawaban peserta sebelumnya. Mengembalikan jumlah
    /// sesi yang dihapus.
    pub fn purge_finished_sessions(&self, now: DateTime<Utc>) -> AppResult<usize> {
        let rows: Vec<(String, String)> = {
            let db = self.db();
            let mut stmt = db.conn.prepare("SELECT attempt_id, json FROM cached_sessions")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<Result<_, _>>()?
        };
        let mut purged = 0;
        for (attempt_id, json) in rows {
            let expired = match serde_json::from_str::<ExamSession>(&json) {
                Ok(s) => s.status != STATUS_IN_PROGRESS || s.deadline + chrono::Duration::hours(1) < now,
                Err(_) => true,
            };
            if expired && self.outbox_count(Some(&attempt_id))? == 0 {
                self.delete_cached_session(&attempt_id)?;
                purged += 1;
            }
        }
        Ok(purged)
    }

    /// Hapus item antrean beserta berkas lampiran yang ikut diantrekan.
    fn drop_outbox(&self, item_id: Option<i64>, attempt_id: Option<&str>) -> AppResult<()> {
        let db = self.db();
        let filter = "(?1 IS NULL OR id = ?1) AND (?2 IS NULL OR attempt_id = ?2)";
        let bodies: Vec<String> = {
            let mut stmt = db
                .conn
                .prepare(&format!("SELECT body FROM outbox WHERE kind = 'attachment' AND {filter}"))?;
            let rows = stmt.query_map(params![item_id, attempt_id], |r| r.get(0))?;
            rows.collect::<Result<_, _>>()?
        };
        for body in bodies {
            if let Ok(q) = serde_json::from_str::<QueuedAttachment>(&body) {
                let _ = std::fs::remove_file(&q.path);
            }
        }
        db.conn
            .execute(&format!("DELETE FROM outbox WHERE {filter}"), params![item_id, attempt_id])?;
        Ok(())
    }

    fn enqueue(&self, attempt_id: &str, kind: &str, body: &impl Serialize) -> AppResult<i64> {
        let db = self.db();
        db.conn.execute(
            "INSERT INTO outbox (attempt_id, kind, body, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![attempt_id, kind, serde_json::to_string(body)?, fmt_time(Utc::now())],
        )?;
        Ok(db.conn.last_insert_rowid())
    }

    fn next_outbox(&self) -> AppResult<Option<OutboxItem>> {
        Ok(self
            .db()
            .conn
            .query_row("SELECT id, attempt_id, kind, body FROM outbox ORDER BY id LIMIT 1", [], |r| {
                Ok(OutboxItem {
                    id: r.get(0)?,
                    attempt_id: r.get(1)?,
                    kind: r.get(2)?,
                    body: r.get(3)?,
                })
            })
            .optional()?)
    }
}

fn apply_state(s: &mut ExamSession, st: &AttemptState, now: DateTime<Utc>) {
    s.status = st.status.clone();
    s.deadline = st.deadline;
    s.violation_count = st.violation_count;
    s.now = now;
    s.remaining_seconds = st.remaining_seconds;
    if st.status != STATUS_IN_PROGRESS && s.finished_at.is_none() {
        s.finished_at = Some(now);
    }
}

/// Sesi tersimpan dengan sisa waktu dihitung ulang dari jam (server) saat ini.
fn refresh_local(mut s: ExamSession, now: DateTime<Utc>) -> ExamSession {
    s.now = now;
    if s.status == STATUS_IN_PROGRESS {
        s.remaining_seconds = (s.deadline - now).num_seconds().max(0);
        if s.remaining_seconds == 0 {
            s.status = STATUS_TIMED_OUT.into();
            s.finished_at = Some(s.deadline);
        }
    } else {
        s.remaining_seconds = 0;
    }
    s
}

fn local_state(s: &ExamSession, pending: i64) -> AttemptState {
    AttemptState {
        status: s.status.clone(),
        remaining_seconds: s.remaining_seconds,
        violation_count: s.violation_count,
        deadline: s.deadline,
        connected: false,
        pending,
    }
}

/// Penghubung PC peserta ke server lokal.
pub struct ParticipantLink {
    core: Arc<Core>,
    pub client: LanClient,
    flush_lock: tokio::sync::Mutex<()>,
}

impl ParticipantLink {
    pub fn new(core: Arc<Core>) -> AppResult<Self> {
        let (url, token) = {
            let db = core.db();
            (db.get_config("lan_url")?, db.get_config("device_token")?)
        };
        let (Some(url), Some(token)) = (url, token) else {
            return Err(AppError::NotConfigured("isi alamat server lokal di pengaturan".into()));
        };
        Ok(Self {
            client: LanClient::new(&url, &token)?,
            core,
            flush_lock: tokio::sync::Mutex::new(()),
        })
    }

    /// Daftarkan PC ini ke server lokal (atau cek status pendaftaran).
    pub async fn pair(&self) -> AppResult<PairResult> {
        let cfg = self.core.config()?;
        let name = cfg
            .device_name
            .or_else(|| std::env::var("COMPUTERNAME").ok())
            .or_else(|| std::env::var("HOSTNAME").ok())
            .unwrap_or_else(|| cfg.device_id.chars().take(8).collect());
        self.client.pair(&cfg.device_id, &name).await
    }

    pub async fn schedules(&self) -> AppResult<Vec<LanSchedule>> {
        self.client.schedules().await
    }

    pub async fn proctor_verify(&self, username: &str, password: &str) -> AppResult<Proctor> {
        self.client.proctor_verify(username, password).await
    }

    /// Simpan media paket di PC ini (diverifikasi SHA-256).
    async fn prefetch(&self, assets: &[AssetInfo]) -> AppResult<()> {
        for a in assets {
            if !self.core.asset_present(a) {
                let bytes = self.client.asset(&a.id).await?;
                self.core.save_asset(a, &bytes)?;
            }
            self.core.set_asset_mime(&a.id, &a.mime)?;
        }
        Ok(())
    }

    async fn accept_session(&self, ls: LanSession) -> AppResult<ExamSession> {
        self.prefetch(&ls.assets)
            .await
            .map_err(|e| AppError::user(format!("Gagal mengambil media ujian dari server lokal: {e}")))?;
        // Ujian yang sudah selesai dan terkirim tidak perlu disalin di PC ini.
        if ls.session.status != STATUS_IN_PROGRESS && self.core.outbox_count(Some(&ls.session.attempt_id))? == 0 {
            self.core.delete_cached_session(&ls.session.attempt_id)?;
        } else {
            self.core.cache_session(&ls.session)?;
        }
        Ok(ls.session)
    }

    pub async fn login(&self, req: &LoginRequest) -> AppResult<ExamSession> {
        let ls = self.client.login(req).await?;
        self.accept_session(ls).await
    }

    /// Sesi dari server lokal; bila terputus, sesi tersimpan di PC ini.
    pub async fn session(&self, attempt_id: &str) -> AppResult<ExamSession> {
        let report = self.flush().await;
        if report.blocked.is_none() {
            match self.client.session(attempt_id).await {
                Ok(ls) => return self.accept_session(ls).await,
                Err(e) if !e.is_retryable() => return Err(e),
                Err(_) => {}
            }
        }
        let cached = self
            .core
            .cached_session(attempt_id)?
            .ok_or_else(|| AppError::user("Tidak dapat terhubung ke server lokal"))?;
        Ok(refresh_local(cached, self.client.now()))
    }

    // ------------------------------------------------------------------ aksi ujian

    pub async fn save_answer(&self, req: &SaveAnswerRequest) -> AppResult<AttemptState> {
        let body = AnswerBody {
            question_id: req.question_id.clone(),
            response: req.response.clone(),
            flagged: req.flagged,
            time_spent_delta: req.time_spent_delta,
            current_index: req.current_index,
            at: Some(self.client.now()),
        };
        let item = self.core.enqueue(&req.attempt_id, KIND_ANSWER, &body)?;
        self.core.update_cached(&req.attempt_id, |s| {
            let spent = s.answers.get(&req.question_id).map(|a| a.time_spent).unwrap_or(0);
            s.answers.insert(
                req.question_id.clone(),
                SessionAnswer {
                    response: req.response.clone(),
                    flagged: req.flagged,
                    time_spent: spent + req.time_spent_delta.max(0),
                },
            );
            if let Some(i) = req.current_index {
                s.current_index = i;
            }
        })?;
        self.deliver_now(&req.attempt_id, item).await
    }

    pub async fn log_event(&self, attempt_id: &str, kind: &str, data: Option<Value>) -> AppResult<AttemptState> {
        let now = self.client.now();
        let body = EventBody {
            kind: kind.to_string(),
            data,
            at: Some(now),
        };
        let item = self.core.enqueue(attempt_id, KIND_EVENT, &body)?;
        if kind == "violation" {
            // Hitung pelanggaran juga di PC ini agar batas tetap berlaku saat terputus.
            self.core.update_cached(attempt_id, |s| {
                s.violation_count += 1;
                let max = s.exam.settings.max_violations;
                if s.status == STATUS_IN_PROGRESS && max > 0 && s.violation_count >= max {
                    s.status = STATUS_TERMINATED.into();
                    s.finished_at = Some(now);
                }
            })?;
        }
        self.deliver_now(attempt_id, item).await
    }

    pub async fn submit(&self, attempt_id: &str, manual: bool) -> AppResult<AttemptState> {
        let now = self.client.now();
        if manual {
            if let Some(s) = self.core.cached_session(attempt_id)? {
                if now < s.can_submit_at {
                    let mins = (s.can_submit_at - now).num_minutes() + 1;
                    return Err(AppError::user(format!(
                        "Ujian baru bisa dikumpulkan sekitar {mins} menit lagi"
                    )));
                }
            }
        }
        let item = self
            .core
            .enqueue(attempt_id, KIND_SUBMIT, &SubmitBody { manual, at: Some(now) })?;
        self.core.update_cached(attempt_id, |s| {
            if s.status == STATUS_IN_PROGRESS {
                s.status = if manual || now < s.deadline {
                    STATUS_SUBMITTED
                } else {
                    STATUS_TIMED_OUT
                }
                .into();
                s.finished_at = Some(now);
            }
        })?;
        self.deliver_now(attempt_id, item).await
    }

    pub async fn save_attachment(
        &self,
        attempt_id: &str,
        question_id: &str,
        name: &str,
        mime: &str,
        bytes: &[u8],
    ) -> AppResult<SavedAttachment> {
        let id = uuid::Uuid::new_v4().to_string();
        let path = self.core.data_dir().join("attachments").join(&id);
        std::fs::write(&path, bytes)?;
        let mime = if mime.is_empty() { "application/octet-stream" } else { mime };
        let queued = QueuedAttachment {
            attachment_id: id.clone(),
            question_id: question_id.to_string(),
            name: name.to_string(),
            mime: mime.to_string(),
            path: path.to_string_lossy().into_owned(),
            at: self.client.now(),
        };
        let item = self.core.enqueue(attempt_id, KIND_ATTACHMENT, &queued)?;
        let report = self.flush().await;
        if let Some((_, e)) = report.rejected.into_iter().find(|(i, _)| *i == item) {
            return Err(e);
        }
        Ok(SavedAttachment {
            attachment_id: id,
            name: name.to_string(),
            size: bytes.len() as i64,
            mime: mime.to_string(),
        })
    }

    /// Status attempt terkini (dipanggil berkala oleh layar ujian): menangkap tambahan waktu
    /// atau penghentian dari proktor, dan mengirim antrean yang tertunda.
    pub async fn attempt_state(&self, attempt_id: &str) -> AppResult<AttemptState> {
        let report = self.flush().await;
        if report.blocked.is_none() {
            match self.client.state(attempt_id).await {
                Ok(st) => {
                    let now = self.client.now();
                    self.core.update_cached(attempt_id, |s| apply_state(s, &st, now))?;
                    // Ujian dibuka lagi oleh proktor setelah salinannya dihapus: ambil ulang
                    // agar peserta tetap bisa mengerjakan bila jaringan putus.
                    if st.status == STATUS_IN_PROGRESS && self.core.cached_session(attempt_id)?.is_none() {
                        if let Ok(ls) = self.client.session(attempt_id).await {
                            self.accept_session(ls).await?;
                        }
                    }
                    let pending = self.core.outbox_count(Some(attempt_id))?;
                    return Ok(AttemptState { pending, ..st });
                }
                Err(e) if !e.is_retryable() => return Err(e),
                Err(_) => {}
            }
        }
        self.local_state(attempt_id)
    }

    fn local_state(&self, attempt_id: &str) -> AppResult<AttemptState> {
        let pending = self.core.outbox_count(Some(attempt_id))?;
        let s = self
            .core
            .cached_session(attempt_id)?
            .ok_or_else(|| AppError::user("Sesi ujian tidak ditemukan di komputer ini"))?;
        Ok(local_state(&refresh_local(s, self.client.now()), pending))
    }

    /// Kirim antrean lalu kembalikan status attempt; error hanya bila item ini sendiri ditolak.
    async fn deliver_now(&self, attempt_id: &str, item: i64) -> AppResult<AttemptState> {
        let mut report = self.flush().await;
        if let Some(pos) = report.rejected.iter().position(|(i, _)| *i == item) {
            return Err(report.rejected.swap_remove(pos).1);
        }
        if report.blocked.is_none() {
            if let Some(st) = report.states.remove(attempt_id) {
                let pending = self.core.outbox_count(Some(attempt_id))?;
                return Ok(AttemptState { pending, ..st });
            }
        }
        self.local_state(attempt_id)
    }

    // ------------------------------------------------------------------ antrean

    /// Kirim semua item antrean secara berurutan. Berhenti pada gangguan pertama.
    pub async fn flush(&self) -> FlushReport {
        let _guard = self.flush_lock.lock().await;
        let mut report = FlushReport::default();
        loop {
            let item = match self.core.next_outbox() {
                Ok(Some(i)) => i,
                Ok(None) => break,
                Err(e) => {
                    report.blocked = Some(e);
                    break;
                }
            };
            match self.deliver(&item).await {
                Ok(state) => {
                    let _ = self.core.db().conn.execute("DELETE FROM outbox WHERE id = ?1", [item.id]);
                    if let Some(st) = state {
                        let now = self.client.now();
                        let _ = self.core.update_cached(&item.attempt_id, |s| apply_state(s, &st, now));
                        report.states.insert(item.attempt_id.clone(), st);
                    }
                    report.sent += 1;
                }
                Err(e) if e.is_retryable() || e.lan_code().is_some_and(|c| BLOCKING_CODES.contains(&c)) => {
                    let _ = self.core.db().conn.execute(
                        "UPDATE outbox SET attempts = attempts + 1, last_error = ?2 WHERE id = ?1",
                        params![item.id, e.to_string()],
                    );
                    report.blocked = Some(e);
                    break;
                }
                Err(e) => {
                    if e.lan_code().is_some_and(|c| MOVED_CODES.contains(&c)) {
                        // Ujian dipindah proktor ke PC lain: sisa antrean dan salinan sesi
                        // attempt ini tidak berlaku lagi di PC ini.
                        let _ = self.core.drop_outbox(None, Some(&item.attempt_id));
                        let _ = self.core.delete_cached_session(&item.attempt_id);
                    } else {
                        let _ = self.core.drop_outbox(Some(item.id), None);
                    }
                    log::warn!("antrean {} ({}) ditolak server lokal: {e}", item.id, item.kind);
                    report.rejected.push((item.id, e));
                }
            }
        }
        if let Err(e) = self.core.purge_finished_sessions(self.client.now()) {
            log::warn!("gagal membersihkan salinan sesi: {e}");
        }
        report
    }

    async fn deliver(&self, item: &OutboxItem) -> AppResult<Option<AttemptState>> {
        let attempt = &item.attempt_id;
        match item.kind.as_str() {
            KIND_ANSWER => Ok(Some(self.client.answer(attempt, &serde_json::from_str(&item.body)?).await?)),
            KIND_EVENT => Ok(Some(self.client.event(attempt, &serde_json::from_str(&item.body)?).await?)),
            KIND_SUBMIT => Ok(Some(self.client.submit(attempt, &serde_json::from_str(&item.body)?).await?)),
            KIND_ATTACHMENT => {
                let q: QueuedAttachment = serde_json::from_str(&item.body)?;
                let bytes = std::fs::read(&q.path)?;
                let body = AttachmentBody {
                    attachment_id: q.attachment_id,
                    question_id: q.question_id,
                    name: q.name,
                    mime: q.mime,
                    data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
                    at: Some(q.at),
                };
                self.client.attachment(attempt, &body).await?;
                let _ = std::fs::remove_file(&q.path);
                Ok(None)
            }
            other => Err(AppError::Other(format!("jenis antrean tidak dikenal: {other}"))),
        }
    }

    /// Status koneksi untuk layar login PC peserta.
    pub async fn status(&self) -> LinkStatus {
        let report = self.flush().await;
        let error = match report.blocked {
            Some(e) => Some(e.to_string()),
            None => self.client.me().await.err().map(|e| e.to_string()),
        };
        LinkStatus {
            connected: error.is_none(),
            pending: self.core.outbox_count(None).unwrap_or(0),
            error,
        }
    }
}
