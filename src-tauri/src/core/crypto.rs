//! Verifikasi hash argon2id format PHC (dibuat server pusat dengan @node-rs/argon2)
//! dan hash PIN operator lokal.

use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
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

pub fn hash_pin(pin: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(pin.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| e.to_string())
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
    fn pin_roundtrip() {
        let h = hash_pin("2468").unwrap();
        assert!(verify_phc(&h, "2468"));
        assert!(!verify_phc(&h, "1357"));
    }

    #[test]
    fn sha256_known() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
