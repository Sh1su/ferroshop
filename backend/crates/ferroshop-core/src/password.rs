//! Argon2id password hashing.

use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};

pub const MIN_LENGTH: usize = 8;

pub fn hash(password: &str) -> String {
    Argon2::default()
        .hash_password(password.as_bytes())
        .expect("argon2 hashing with default params cannot fail")
        .to_string()
}

pub fn verify(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|parsed| Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
}

#[cfg(test)]
mod tests {
    #[test]
    fn hash_and_verify() {
        let h = super::hash("correct horse");
        assert!(super::verify("correct horse", &h));
        assert!(!super::verify("wrong", &h));
        assert!(!super::verify("x", "not a hash"));
    }
}
