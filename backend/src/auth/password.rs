use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};

pub fn hash(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| anyhow::anyhow!("password hash failed: {e}"))
}

pub fn verify(password: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// 사용자 존재 여부를 응답 시간으로 노출하지 않기 위해, 없는 계정에도 검증 1회 수행
pub fn dummy_verify(password: &str) {
    static DUMMY: once_cell::sync::Lazy<String> =
        once_cell::sync::Lazy::new(|| hash("dummy-password-for-timing").unwrap());
    let _ = verify(password, &DUMMY);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let h = hash("correct horse battery").unwrap();
        assert!(h.starts_with("$argon2"));
        assert!(verify("correct horse battery", &h));
        assert!(!verify("wrong", &h));
        assert!(!verify("x", "not-a-hash"));
    }
}
