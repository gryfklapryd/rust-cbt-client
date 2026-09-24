//! Verifikasi hash argon2id format PHC (dibuat server pusat dengan @node-rs/argon2),
//! token perangkat acak, dan SHA-256.

use argon2::password_hash::rand_core::{OsRng, RngCore};
use argon2::password_hash::{PasswordHash, PasswordVerifier};
use argon2::Argon2;
use sha2::{Digest, Sha256};

/// Verifikasi password terhadap hash PHC (`$argon2id$v=19$m=…,t=…,p=…$salt$hash`).
/// Parameter (m, t, p) dibaca dari string hash.
pub fn verify_phc(phc: &str, password: &str) -> bool {
    match PasswordHash::new(phc) {
        Ok(parsed) => Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok(),
        Err(_) => false,
    }
}

/// Token acak 256-bit (hex) untuk identitas PC peserta di server lokal.
pub fn random_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Kode pendek 4 digit yang ditampilkan di PC peserta dan di dasbor proktor saat pendaftaran.
pub fn pairing_code() -> String {
    format!("{:04}", OsRng.next_u32() % 10_000)
}

/// Perbandingan waktu-konstan untuk hash token.
pub fn constant_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifies_hash_from_server() {
        // Dibuat oleh server pusat (Node @node-rs/argon2, m=4096 t=2 p=1) untuk password "123456".
        let phc = "$argon2id$v=19$m=4096,t=2,p=1$fw5ywR5q8+oN1nP3QQoscw$/4DtnEqi/R5S/ryGoFOfsb8UYNblaTjciKNI9D/3MP8";
        assert!(verify_phc(phc, "123456"));
        assert!(!verify_phc(phc, "654321"));
        assert!(!verify_phc("bukan-hash", "123456"));
    }

    #[test]
    fn tokens_are_random_hex() {
        let a = random_token();
        assert_eq!(a.len(), 64);
        assert_ne!(a, random_token());
        assert!(constant_eq(&a, &a.clone()));
        assert!(!constant_eq(&a, &random_token()));
        assert_eq!(pairing_code().len(), 4);
    }

    #[test]
    fn sha256_known() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
