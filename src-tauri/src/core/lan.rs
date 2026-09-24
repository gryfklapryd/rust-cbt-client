//! API LAN server lokal untuk PC peserta (`/lan/v1/*`).
//!
//! Semua endpoint kecuali `/info` dan `/pair` membutuhkan `Authorization: Bearer <token perangkat>`
//! dari PC yang sudah disetujui proktor. Setiap respons membawa header `x-server-time` supaya
//! PC peserta bisa menyamakan jam untuk jawaban yang diantrekan saat terputus.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::error::AppError;
use super::exam::{AttemptState, ExamSession, LoginRequest, SaveAnswerRequest, SavedAttachment, STATUS_IN_PROGRESS};
use super::package::AssetInfo;
use super::proctor::{LogTarget, PairResult, Proctor, DEVICE_APPROVED, DEVICE_PENDING};
use super::Core;

pub const API_PREFIX: &str = "/lan/v1";
pub const SERVICE_NAME: &str = "cbt-server-lokal";

/// Batas umur waktu jawaban yang diantrekan PC peserta saat terputus.
const MAX_QUEUED_AGE_HOURS: i64 = 12;
/// Batas ukuran body (lampiran jawaban dikirim sebagai base64).
const MAX_BODY_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanInfo {
    pub service: String,
    pub site_code: Option<String>,
    pub server_name: Option<String>,
    pub app_version: String,
    pub server_time: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanSchedule {
    pub schedule_id: String,
    pub name: String,
    pub exam_code: String,
    pub exam_title: String,
    pub duration_minutes: i64,
    pub start_at: DateTime<Utc>,
    pub end_at: DateTime<Utc>,
    pub requires_token: bool,
    /// Paket dan semua media sudah ada di server lokal.
    pub ready: bool,
}

/// Sesi ujian untuk PC peserta, beserta daftar media paket untuk disimpan di PC.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanSession {
    pub session: ExamSession,
    pub assets: Vec<AssetInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairRequest {
    pub device_id: String,
    pub name: String,
    pub token: String,
    pub app_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceStatus {
    pub device_id: String,
    pub name: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerBody {
    pub question_id: String,
    pub response: Value,
    pub flagged: bool,
    #[serde(default)]
    pub time_spent_delta: i64,
    #[serde(default)]
    pub current_index: Option<i64>,
    /// Waktu jawaban dibuat di PC peserta (jam server), untuk jawaban yang diantrekan.
    #[serde(default)]
    pub at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventBody {
    pub kind: String,
    #[serde(default)]
    pub data: Option<Value>,
    #[serde(default)]
    pub at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitBody {
    pub manual: bool,
    #[serde(default)]
    pub at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentBody {
    pub attachment_id: String,
    pub question_id: String,
    pub name: String,
    pub mime: String,
    pub data_base64: String,
    #[serde(default)]
    pub at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProctorVerifyBody {
    pub username: String,
    pub password: String,
}

// ---------------------------------------------------------------------- error

/// Error API LAN: `{ "message": "...", "code": "..." }`.
pub struct ApiError {
    status: StatusCode,
    message: String,
    code: &'static str,
}

impl ApiError {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        ApiError {
            status,
            message: message.into(),
            code,
        }
    }
}

impl From<AppError> for ApiError {
    fn from(e: AppError) -> Self {
        match e {
            AppError::User(m) => ApiError::new(StatusCode::BAD_REQUEST, "user", m),
            AppError::NotConfigured(_) => ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "not_configured", e.to_string()),
            other => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", other.to_string()),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "message": self.message, "code": self.code }))).into_response()
    }
}

type ApiResult<T> = Result<Json<T>, ApiError>;

// ---------------------------------------------------------------------- helper

/// Jalankan operasi core (SQLite, argon2) di thread blocking.
async fn blocking<T, F>(core: &Arc<Core>, f: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce(&Core) -> Result<T, AppError> + Send + 'static,
{
    let core = core.clone();
    tokio::task::spawn_blocking(move || f(&core))
        .await
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", e.to_string()))?
        .map_err(ApiError::from)
}

/// Waktu efektif sebuah aksi: waktu yang dikirim PC peserta bila wajar, selain itu jam server.
pub fn effective_time(at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> DateTime<Utc> {
    match at {
        Some(t) if t <= now && t >= now - Duration::hours(MAX_QUEUED_AGE_HOURS) => t,
        _ => now,
    }
}

fn bearer(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(|t| t.trim().to_string())
}

/// PC peserta yang sudah disetujui, dari token Bearer.
async fn device(core: &Arc<Core>, headers: &HeaderMap, ip: SocketAddr) -> Result<String, ApiError> {
    let token = bearer(headers).ok_or_else(|| {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "unpaired",
            "Komputer ini belum terdaftar di server lokal",
        )
    })?;
    let found = blocking(core, move |c| c.device_by_token(&token)).await?;
    match found {
        None => Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "unpaired",
            "Komputer ini belum terdaftar di server lokal",
        )),
        Some((_, status)) if status == DEVICE_PENDING => Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "pending",
            "Komputer ini menunggu persetujuan proktor",
        )),
        Some((_, status)) if status != DEVICE_APPROVED => Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "revoked",
            "Pendaftaran komputer ini dicabut proktor. Daftarkan ulang.",
        )),
        Some((id, _)) => {
            let dev = id.clone();
            blocking(core, move |c| c.touch_device(&dev, Some(&ip.ip().to_string()), Utc::now())).await?;
            Ok(id)
        }
    }
}

/// Pastikan attempt milik PC yang meminta.
async fn owned_attempt(core: &Arc<Core>, attempt_id: &str, device_id: &str) -> Result<(), ApiError> {
    let id = attempt_id.to_string();
    let owner = blocking(core, move |c| c.attempt_device(&id)).await?;
    match owner {
        Some(d) if d == device_id => Ok(()),
        None => Err(ApiError::new(
            StatusCode::CONFLICT,
            "moved",
            "Proktor memindahkan ujian ini ke komputer lain. Silakan login ulang.",
        )),
        Some(_) => Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "other_device",
            "Ujian ini sudah dilanjutkan di komputer lain",
        )),
    }
}

fn lan_session(core: &Core, attempt_id: &str, now: DateTime<Utc>) -> Result<LanSession, AppError> {
    let session = core.session(attempt_id, now)?;
    let assets = core.package_assets_of_attempt(attempt_id)?;
    Ok(LanSession { session, assets })
}

async fn server_time_header(req: axum::extract::Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    if let Ok(v) = HeaderValue::from_str(&Utc::now().to_rfc3339()) {
        res.headers_mut().insert("x-server-time", v);
    }
    res
}

// ---------------------------------------------------------------------- router

pub fn router(core: Arc<Core>) -> Router {
    let api = Router::new()
        .route("/info", get(info))
        .route("/pair", post(pair))
        .route("/me", get(me))
        .route("/schedules", get(schedules))
        .route("/assets/{id}", get(asset))
        .route("/login", post(login))
        .route("/attempts/{id}", get(session))
        .route("/attempts/{id}/state", get(state))
        .route("/attempts/{id}/answer", post(answer))
        .route("/attempts/{id}/event", post(event))
        .route("/attempts/{id}/submit", post(submit))
        .route("/attempts/{id}/attachment", post(attachment))
        .route("/proctor/verify", post(proctor_verify))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::from_fn(server_time_header))
        .with_state(core);
    Router::new().nest(API_PREFIX, api)
}

/// Jalankan API LAN di `0.0.0.0:port` sampai proses berhenti.
pub async fn serve(core: Arc<Core>, port: u16) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    serve_on(core, listener).await
}

pub async fn serve_on(core: Arc<Core>, listener: tokio::net::TcpListener) -> std::io::Result<()> {
    axum::serve(listener, router(core).into_make_service_with_connect_info::<SocketAddr>()).await
}

// ---------------------------------------------------------------------- handler

async fn info(State(core): State<Arc<Core>>) -> ApiResult<LanInfo> {
    let cfg = blocking(&core, |c| c.config()).await?;
    Ok(Json(LanInfo {
        service: SERVICE_NAME.into(),
        site_code: cfg.site_code,
        server_name: cfg.device_name,
        app_version: super::api::APP_VERSION.into(),
        server_time: Utc::now(),
    }))
}

async fn pair(
    State(core): State<Arc<Core>>,
    ConnectInfo(ip): ConnectInfo<SocketAddr>,
    Json(body): Json<PairRequest>,
) -> ApiResult<PairResult> {
    let res = blocking(&core, move |c| {
        c.pair_device(
            &body.device_id,
            &body.name,
            &body.token,
            body.app_version.as_deref(),
            Some(&ip.ip().to_string()),
            Utc::now(),
        )
    })
    .await?;
    Ok(Json(res))
}

async fn me(
    State(core): State<Arc<Core>>,
    ConnectInfo(ip): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> ApiResult<DeviceStatus> {
    let id = device(&core, &headers, ip).await?;
    let dev = id.clone();
    let name = blocking(&core, move |c| Ok(c.device_name(&dev).unwrap_or_default())).await?;
    Ok(Json(DeviceStatus {
        device_id: id,
        name,
        status: DEVICE_APPROVED.into(),
    }))
}

async fn schedules(
    State(core): State<Arc<Core>>,
    ConnectInfo(ip): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> ApiResult<Vec<LanSchedule>> {
    device(&core, &headers, ip).await?;
    let list = blocking(&core, |c| c.local_schedules()).await?;
    Ok(Json(
        list.into_iter()
            .map(|s| LanSchedule {
                ready: s.assets_missing == 0,
                schedule_id: s.schedule_id,
                name: s.name,
                exam_code: s.exam_code,
                exam_title: s.exam_title,
                duration_minutes: s.duration_minutes,
                start_at: s.start_at,
                end_at: s.end_at,
                requires_token: s.requires_token,
            })
            .collect(),
    ))
}

async fn asset(
    State(core): State<Arc<Core>>,
    ConnectInfo(ip): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    device(&core, &headers, ip).await?;
    let found = blocking(&core, move |c| {
        let Some(mime) = c.asset_mime(&id) else { return Ok(None) };
        match std::fs::read(c.asset_path(&id)) {
            Ok(bytes) => Ok(Some((mime, bytes))),
            Err(_) => Ok(None),
        }
    })
    .await?;
    match found {
        Some((mime, bytes)) => Ok(([(header::CONTENT_TYPE, mime)], bytes).into_response()),
        None => Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "Media tidak ditemukan di server lokal",
        )),
    }
}

async fn login(
    State(core): State<Arc<Core>>,
    ConnectInfo(ip): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<LoginRequest>,
) -> ApiResult<LanSession> {
    let dev = device(&core, &headers, ip).await?;
    let res = blocking(&core, move |c| {
        let now = Utc::now();
        let id = c.login(&req, Some(&dev), now)?;
        lan_session(c, &id, now)
    })
    .await?;
    Ok(Json(res))
}

async fn session(
    State(core): State<Arc<Core>>,
    ConnectInfo(ip): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<LanSession> {
    let dev = device(&core, &headers, ip).await?;
    owned_attempt(&core, &id, &dev).await?;
    Ok(Json(blocking(&core, move |c| lan_session(c, &id, Utc::now())).await?))
}

async fn state(
    State(core): State<Arc<Core>>,
    ConnectInfo(ip): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<AttemptState> {
    let dev = device(&core, &headers, ip).await?;
    owned_attempt(&core, &id, &dev).await?;
    Ok(Json(blocking(&core, move |c| c.attempt_state(&id, Utc::now())).await?))
}

async fn answer(
    State(core): State<Arc<Core>>,
    ConnectInfo(ip): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<AnswerBody>,
) -> ApiResult<AttemptState> {
    let dev = device(&core, &headers, ip).await?;
    owned_attempt(&core, &id, &dev).await?;
    let res = blocking(&core, move |c| {
        let now = Utc::now();
        let at = effective_time(body.at, now);
        let req = SaveAnswerRequest {
            attempt_id: id.clone(),
            question_id: body.question_id,
            response: body.response,
            flagged: body.flagged,
            time_spent_delta: body.time_spent_delta,
            current_index: body.current_index,
        };
        c.save_answer(&req, at)?;
        // Sisa waktu selalu dihitung dengan jam server saat ini.
        c.attempt_state(&id, now)
    })
    .await?;
    Ok(Json(res))
}

async fn event(
    State(core): State<Arc<Core>>,
    ConnectInfo(ip): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<EventBody>,
) -> ApiResult<AttemptState> {
    let dev = device(&core, &headers, ip).await?;
    owned_attempt(&core, &id, &dev).await?;
    let res = blocking(&core, move |c| {
        let now = Utc::now();
        let st = c.log_event(&id, &body.kind, body.data, effective_time(body.at, now))?;
        if st.status == STATUS_IN_PROGRESS {
            c.attempt_state(&id, now)
        } else {
            Ok(st)
        }
    })
    .await?;
    Ok(Json(res))
}

async fn submit(
    State(core): State<Arc<Core>>,
    ConnectInfo(ip): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<SubmitBody>,
) -> ApiResult<AttemptState> {
    let dev = device(&core, &headers, ip).await?;
    owned_attempt(&core, &id, &dev).await?;
    let res = blocking(&core, move |c| {
        c.submit(&id, body.manual, effective_time(body.at, Utc::now()))
    })
    .await?;
    Ok(Json(res))
}

async fn attachment(
    State(core): State<Arc<Core>>,
    ConnectInfo(ip): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<AttachmentBody>,
) -> ApiResult<SavedAttachment> {
    let dev = device(&core, &headers, ip).await?;
    owned_attempt(&core, &id, &dev).await?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(body.data_base64.as_bytes())
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, "user", format!("berkas tidak valid: {e}")))?;
    let res = blocking(&core, move |c| {
        c.save_attachment(
            &id,
            &body.question_id,
            Some(&body.attachment_id),
            &body.name,
            &body.mime,
            &bytes,
            effective_time(body.at, Utc::now()),
        )
    })
    .await?;
    Ok(Json(res))
}

/// Verifikasi login proktor dari PC peserta (untuk membuka menu proktor di PC tersebut).
async fn proctor_verify(
    State(core): State<Arc<Core>>,
    ConnectInfo(ip): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<ProctorVerifyBody>,
) -> ApiResult<Proctor> {
    // PC yang belum disetujui pun boleh (proktor perlu mengubah alamat server di PC itu).
    let token_device = match bearer(&headers) {
        Some(t) => blocking(&core, move |c| c.device_by_token(&t)).await?.map(|d| d.0),
        None => None,
    };
    let res = blocking(&core, move |c| {
        let p = c.verify_proctor(&body.username, &body.password)?;
        c.log_proctor(
            &p,
            "login",
            LogTarget::default(),
            Some(json!({ "from": "pc_peserta", "deviceId": token_device, "ip": ip.ip().to_string() })),
            Utc::now(),
        )?;
        Ok(p)
    })
    .await;
    if res.is_err() {
        // Perlambat tebakan password.
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    }
    Ok(Json(res?))
}
