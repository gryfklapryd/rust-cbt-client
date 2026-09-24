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

/// Pesan jaringan beserta alamat server dan penyebab aslinya (mis. koneksi ditolak,
/// timeout, proxy), karena `to_string()` reqwest hanya berisi "error sending request".
fn describe_reqwest(e: reqwest::Error) -> String {
    let origin = e.url().map(|u| u.origin().ascii_serialization());
    let https = e.url().is_some_and(|u| u.scheme() == "https");
    let e = e.without_url();
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
    let target = origin.map(|o| format!(" ke {o}")).unwrap_or_default();
    let mut msg = if e.is_timeout() {
        format!("waktu habis{target} ({detail})")
    } else if e.is_connect() {
        format!("gagal membuka koneksi{target} ({detail})")
    } else {
        format!("{detail}{target}")
    };
    // Klien memulai TLS tetapi lawan bicara menjawab HTTP biasa.
    if detail.contains("InvalidContentType") {
        msg.push_str(if https {
            ". Server di alamat ini tidak memakai HTTPS: ganti awalan alamat menjadi http:// lalu Simpan."
        } else {
            ". Koneksi dibelokkan ke proxy HTTPS yang tidak valid: periksa pengaturan proxy Windows."
        });
    }
    msg
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
        assert!(msg.contains("gagal membuka koneksi ke http://127.0.0.1:1"), "{msg}");
        assert!(
            msg.len() > "Tidak dapat terhubung ke server pusat: error sending request".len() + 25,
            "{msg}"
        );
    }

    #[tokio::test]
    async fn https_to_plain_http_server_gets_hint() {
        // Server HTTP biasa: TLS handshake klien dijawab teks HTTP.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf);
            let _ = sock.write_all(b"HTTP/1.1 400 Bad Request\r\ncontent-length: 0\r\n\r\n");
        });
        let err = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("https://127.0.0.1:{port}/"))
            .send()
            .await
            .unwrap_err();
        let msg = super::AppError::from(err).to_string();
        eprintln!("{msg}");
        assert!(msg.contains("ganti awalan alamat menjadi http://"), "{msg}");
    }
}
