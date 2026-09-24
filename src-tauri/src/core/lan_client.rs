//! Klien HTTP PC peserta ke API LAN server lokal (lihat `lan.rs`).

use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::{Client, RequestBuilder, Response};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use super::api::APP_VERSION;
use super::error::{AppError, AppResult};
use super::exam::{AttemptState, LoginRequest, SavedAttachment};
use super::lan::{
    AnswerBody, AttachmentBody, DeviceStatus, EventBody, LanInfo, LanSchedule, LanSession, PairRequest, ProctorVerifyBody,
    SubmitBody, API_PREFIX, SERVICE_NAME,
};
use super::proctor::{PairResult, Proctor};

pub struct LanClient {
    http: Client,
    base: String,
    token: String,
    /// Selisih jam server lokal terhadap jam komputer ini (milidetik).
    offset_ms: AtomicI64,
}

impl LanClient {
    pub fn new(base_url: &str, token: &str) -> AppResult<Self> {
        // LAN selalu dihubungi langsung, tanpa proxy sistem.
        let http = Client::builder()
            .no_proxy()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(60))
            .user_agent(format!("cbt-pc-peserta/{APP_VERSION}"))
            .build()?;
        Ok(Self {
            http,
            base: format!("{}{API_PREFIX}", base_url.trim_end_matches('/')),
            token: token.to_string(),
            offset_ms: AtomicI64::new(0),
        })
    }

    /// Jam server lokal menurut perkiraan terakhir.
    pub fn now(&self) -> DateTime<Utc> {
        Utc::now() + chrono::Duration::milliseconds(self.offset_ms.load(Ordering::Relaxed))
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    async fn send(&self, req: RequestBuilder) -> AppResult<Response> {
        let res = req.bearer_auth(&self.token).send().await?;
        if let Some(t) = res
            .headers()
            .get("x-server-time")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| DateTime::parse_from_rfc3339(v).ok())
        {
            let offset = (t.with_timezone(&Utc) - Utc::now()).num_milliseconds();
            self.offset_ms.store(offset, Ordering::Relaxed);
        }
        let status = res.status();
        if status.is_success() {
            return Ok(res);
        }
        let body: Value = res.json().await.unwrap_or(Value::Null);
        let text = |k: &str| body.get(k).and_then(Value::as_str).map(String::from);
        Err(AppError::Lan {
            status: status.as_u16(),
            code: text("code").unwrap_or_else(|| "http".into()),
            message: text("message").unwrap_or_else(|| format!("Server lokal menjawab {status}")),
        })
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> AppResult<T> {
        Ok(self.send(self.http.get(self.url(path))).await?.json().await?)
    }

    async fn post<B: Serialize, T: DeserializeOwned>(&self, path: &str, body: &B) -> AppResult<T> {
        Ok(self.send(self.http.post(self.url(path)).json(body)).await?.json().await?)
    }

    /// Info server lokal; sekaligus memastikan alamat benar-benar server lokal CBT.
    pub async fn info(&self) -> AppResult<LanInfo> {
        let info: LanInfo = self.get("/info").await?;
        if info.service != SERVICE_NAME {
            return Err(AppError::user("Alamat tersebut bukan server lokal CBT"));
        }
        Ok(info)
    }

    pub async fn pair(&self, device_id: &str, name: &str) -> AppResult<PairResult> {
        let body = PairRequest {
            device_id: device_id.to_string(),
            name: name.to_string(),
            token: self.token.clone(),
            app_version: Some(APP_VERSION.into()),
        };
        self.post("/pair", &body).await
    }

    pub async fn me(&self) -> AppResult<DeviceStatus> {
        self.get("/me").await
    }

    pub async fn schedules(&self) -> AppResult<Vec<LanSchedule>> {
        self.get("/schedules").await
    }

    pub async fn asset(&self, id: &str) -> AppResult<Vec<u8>> {
        let res = self.send(self.http.get(self.url(&format!("/assets/{id}")))).await?;
        Ok(res.bytes().await?.to_vec())
    }

    pub async fn login(&self, req: &LoginRequest) -> AppResult<LanSession> {
        self.post("/login", req).await
    }

    pub async fn session(&self, attempt_id: &str) -> AppResult<LanSession> {
        self.get(&format!("/attempts/{attempt_id}")).await
    }

    pub async fn state(&self, attempt_id: &str) -> AppResult<AttemptState> {
        self.get(&format!("/attempts/{attempt_id}/state")).await
    }

    pub async fn answer(&self, attempt_id: &str, body: &AnswerBody) -> AppResult<AttemptState> {
        self.post(&format!("/attempts/{attempt_id}/answer"), body).await
    }

    pub async fn event(&self, attempt_id: &str, body: &EventBody) -> AppResult<AttemptState> {
        self.post(&format!("/attempts/{attempt_id}/event"), body).await
    }

    pub async fn submit(&self, attempt_id: &str, body: &SubmitBody) -> AppResult<AttemptState> {
        self.post(&format!("/attempts/{attempt_id}/submit"), body).await
    }

    pub async fn attachment(&self, attempt_id: &str, body: &AttachmentBody) -> AppResult<SavedAttachment> {
        self.post(&format!("/attempts/{attempt_id}/attachment"), body).await
    }

    pub async fn proctor_verify(&self, username: &str, password: &str) -> AppResult<Proctor> {
        let body = ProctorVerifyBody {
            username: username.to_string(),
            password: password.to_string(),
        };
        self.post("/proctor/verify", &body).await
    }
}
