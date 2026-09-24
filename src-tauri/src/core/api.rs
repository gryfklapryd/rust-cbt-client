//! Klien HTTP ke server pusat (`/api/sync/*`). Kontrak: docs/sinkronisasi.md di repo server.

use std::time::Duration;

use reqwest::{multipart, Client, Response, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

use super::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemotePackageInfo {
    pub id: String,
    pub version: i64,
    pub checksum: Option<String>,
    pub size: Option<i64>,
    pub built_at: Option<String>,
    pub asset_count: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteExam {
    pub id: String,
    pub code: String,
    pub title: String,
    pub duration_minutes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSchedule {
    pub id: String,
    pub name: String,
    pub start_at: String,
    pub end_at: String,
    pub status: String,
    pub exam: RemoteExam,
    pub package: Option<RemotePackageInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SchedulesResponse {
    schedules: Vec<RemoteSchedule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptOutcome {
    pub attempt_id: String,
    pub accepted: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchAck {
    pub batch_id: String,
    pub status: String,
    pub attempt_count: i64,
    pub received_at: String,
    pub processed_at: Option<String>,
    pub error: Option<String>,
    #[serde(default)]
    pub attempts: Option<Vec<AttemptOutcome>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthInfo {
    pub token: String,
    pub expires_at: Option<String>,
    pub site: Value,
    pub server_time: String,
}

pub struct DownloadedPackage {
    pub bytes: Vec<u8>,
    pub checksum: String,
}

#[derive(Clone)]
pub struct Credentials {
    pub server_url: String,
    pub site_code: String,
    pub secret: String,
    pub device_id: String,
}

/// Klien sinkronisasi dengan token lokasi yang di-cache dan diperbarui otomatis saat 401.
pub struct SyncApi {
    http: Client,
    creds: Credentials,
    token: Mutex<Option<String>>,
}

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

impl SyncApi {
    pub fn new(creds: Credentials) -> AppResult<Self> {
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(300))
            .user_agent(format!("cbt-client/{APP_VERSION}"))
            .build()?;
        Ok(Self {
            http,
            creds,
            token: Mutex::new(None),
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/sync{}", self.creds.server_url.trim_end_matches('/'), path)
    }

    pub async fn auth(&self) -> AppResult<AuthInfo> {
        let res = self
            .http
            .post(self.url("/auth"))
            .json(&serde_json::json!({
                "siteCode": self.creds.site_code,
                "secret": self.creds.secret,
                "deviceId": self.creds.device_id,
                "appVersion": APP_VERSION,
            }))
            .send()
            .await?;
        let info: AuthInfo = check(res).await?.json().await?;
        *self.token.lock().await = Some(info.token.clone());
        Ok(info)
    }

    async fn token(&self) -> AppResult<String> {
        if let Some(t) = self.token.lock().await.clone() {
            return Ok(t);
        }
        Ok(self.auth().await?.token)
    }

    /// Kirim request dengan token; bila 401, login ulang sekali lalu ulangi.
    async fn send<F>(&self, build: F) -> AppResult<Response>
    where
        F: Fn(&Client, &str) -> reqwest::RequestBuilder,
    {
        let token = self.token().await?;
        let res = build(&self.http, &token).send().await?;
        if res.status() == StatusCode::UNAUTHORIZED {
            let token = self.auth().await?.token;
            return Ok(build(&self.http, &token).send().await?);
        }
        Ok(res)
    }

    pub async fn schedules(&self) -> AppResult<Vec<RemoteSchedule>> {
        let url = self.url("/schedules");
        let res = self.send(|c, t| c.get(&url).bearer_auth(t)).await?;
        Ok(check(res).await?.json::<SchedulesResponse>().await?.schedules)
    }

    /// Unduh paket. `None` bila paket lokal (checksum `etag`) sudah terbaru.
    pub async fn package(&self, schedule_id: &str, etag: Option<&str>) -> AppResult<Option<DownloadedPackage>> {
        let url = self.url(&format!("/schedules/{schedule_id}/package"));
        let res = self
            .send(|c, t| {
                let mut r = c.get(&url).bearer_auth(t);
                if let Some(e) = etag {
                    r = r.header("if-none-match", format!("\"{e}\""));
                }
                r
            })
            .await?;
        if res.status() == StatusCode::NOT_MODIFIED {
            return Ok(None);
        }
        let res = check(res).await?;
        let checksum = res
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim_matches('"').to_string())
            .ok_or_else(|| AppError::Other("respons paket tanpa ETag".into()))?;
        let bytes = res.bytes().await?.to_vec();
        Ok(Some(DownloadedPackage { bytes, checksum }))
    }

    pub async fn asset(&self, asset_id: &str) -> AppResult<Vec<u8>> {
        let url = self.url(&format!("/assets/{asset_id}"));
        let res = self.send(|c, t| c.get(&url).bearer_auth(t)).await?;
        Ok(check(res).await?.bytes().await?.to_vec())
    }

    pub async fn upload_attachment(
        &self,
        attachment_id: &str,
        attempt_id: &str,
        question_id: &str,
        name: &str,
        mime: &str,
        bytes: Vec<u8>,
    ) -> AppResult<()> {
        let url = self.url("/attachments");
        let res = self
            .send(|c, t| {
                let part = multipart::Part::bytes(bytes.clone())
                    .file_name(name.to_string())
                    .mime_str(mime)
                    .unwrap_or_else(|_| multipart::Part::bytes(bytes.clone()).file_name(name.to_string()));
                // Field teks harus dikirim sebelum field berkas.
                let form = multipart::Form::new()
                    .text("attachmentId", attachment_id.to_string())
                    .text("attemptId", attempt_id.to_string())
                    .text("questionId", question_id.to_string())
                    .part("file", part);
                c.post(&url).bearer_auth(t).multipart(form)
            })
            .await?;
        check(res).await?;
        Ok(())
    }

    pub async fn post_results(&self, batch: &Value) -> AppResult<BatchAck> {
        let url = self.url("/results");
        let res = self.send(|c, t| c.post(&url).bearer_auth(t).json(batch)).await?;
        Ok(check(res).await?.json().await?)
    }

    pub async fn batch_status(&self, batch_id: &str) -> AppResult<BatchAck> {
        let url = self.url(&format!("/results/{batch_id}"));
        let res = self.send(|c, t| c.get(&url).bearer_auth(t)).await?;
        Ok(check(res).await?.json().await?)
    }

    pub async fn heartbeat(&self, status: Value) -> AppResult<()> {
        let url = self.url("/heartbeat");
        let body = serde_json::json!({ "deviceId": self.creds.device_id, "appVersion": APP_VERSION, "status": status });
        let res = self.send(|c, t| c.post(&url).bearer_auth(t).json(&body)).await?;
        check(res).await?;
        Ok(())
    }
}

/// Ubah respons non-2xx menjadi `AppError::Server` dengan pesan dari server.
async fn check(res: Response) -> AppResult<Response> {
    let status = res.status();
    if status.is_success() || status == StatusCode::NOT_MODIFIED {
        return Ok(res);
    }
    let body: Value = res.json().await.unwrap_or(Value::Null);
    let message = body
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_else(|| status.canonical_reason().unwrap_or("error"))
        .to_string();
    // Rincian validasi dari server (maks. 3) agar mudah didiagnosis operator.
    let details: Vec<String> = body
        .get("issues")
        .and_then(Value::as_array)
        .map(|issues| {
            issues
                .iter()
                .take(3)
                .map(|i| {
                    let path = i.get("path").and_then(Value::as_str).unwrap_or("");
                    let msg = i.get("message").and_then(Value::as_str).unwrap_or("");
                    format!("{path} {msg}").trim().to_string()
                })
                .collect()
        })
        .unwrap_or_default();
    let message = if details.is_empty() {
        message
    } else {
        format!("{message}: {}", details.join("; "))
    };
    Err(AppError::Server {
        status: status.as_u16(),
        message,
    })
}
