use serde::{Serialize, Serializer};

/// Error aplikasi. Pesan ditujukan untuk ditampilkan ke operator / peserta.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// Kesalahan yang disebabkan input / keadaan (bukan bug), pesannya aman ditampilkan.
    #[error("{0}")]
    User(String),
    #[error("Belum dikonfigurasi: {0}")]
    NotConfigured(String),
    #[error("Tidak dapat terhubung ke server pusat: {0}")]
    Network(String),
    #[error("Server menolak permintaan ({status}): {message}")]
    Server { status: u16, message: String },
    #[error("Kesalahan database lokal: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("Kesalahan berkas: {0}")]
    Io(#[from] std::io::Error),
    #[error("Data tidak valid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Other(String),
}

impl AppError {
    pub fn user(msg: impl Into<String>) -> Self {
        AppError::User(msg.into())
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        AppError::Network(describe_reqwest(e))
    }
}

/// Pesan jaringan beserta penyebab aslinya (mis. koneksi ditolak, timeout, proxy),
/// karena `to_string()` reqwest hanya berisi "error sending request".
fn describe_reqwest(e: reqwest::Error) -> String {
    let e = e.without_url();
    let hint = if e.is_timeout() {
        Some("waktu habis")
    } else if e.is_connect() {
        Some("gagal membuka koneksi")
    } else {
        None
    };
    let mut parts = vec![e.to_string()];
    let mut source = std::error::Error::source(&e);
    while let Some(s) = source {
        let msg = s.to_string();
        if !parts.iter().any(|p| p.contains(&msg)) {
            parts.push(msg);
        }
        source = s.source();
    }
    let detail = parts.join(": ");
    match hint {
        Some(h) => format!("{h} ({detail})"),
        None => detail,
    }
}

/// Dikirim ke frontend sebagai string pesan.
impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn network_error_includes_cause() {
        let err = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get("http://127.0.0.1:1/")
            .send()
            .await
            .unwrap_err();
        let msg = super::AppError::from(err).to_string();
        eprintln!("{msg}");
        assert!(msg.contains("gagal membuka koneksi"), "{msg}");
        assert!(
            msg.len() > "Tidak dapat terhubung ke server pusat: error sending request".len() + 25,
            "{msg}"
        );
    }
}
