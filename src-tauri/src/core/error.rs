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
        AppError::Network(e.without_url().to_string())
    }
}

/// Dikirim ke frontend sebagai string pesan.
impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
