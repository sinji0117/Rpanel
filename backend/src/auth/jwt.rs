use chrono::Utc;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

pub const ISSUER: &str = "rpanel";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Claims {
    /// user id
    pub sub: u64,
    pub role: String,
    /// 세션(리프레시 토큰 패밀리) id — 로그아웃 시 즉시 무효화에 사용
    pub sid: String,
    pub iat: i64,
    pub exp: i64,
    pub iss: String,
}

pub fn issue(
    secret: &str,
    user_id: u64,
    role: &str,
    sid: &str,
    ttl_secs: i64,
) -> anyhow::Result<(String, i64)> {
    let now = Utc::now().timestamp();
    let claims = Claims {
        sub: user_id,
        role: role.to_string(),
        sid: sid.to_string(),
        iat: now,
        exp: now + ttl_secs,
        iss: ISSUER.into(),
    };
    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;
    Ok((token, claims.exp))
}

pub fn verify(secret: &str, token: &str) -> Option<Claims> {
    let mut v = Validation::new(Algorithm::HS256);
    v.set_issuer(&[ISSUER]);
    v.leeway = 5;
    decode::<Claims>(token, &DecodingKey::from_secret(secret.as_bytes()), &v)
        .ok()
        .map(|d| d.claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn roundtrip() {
        let (t, exp) = issue(S, 7, "admin", "fam1", 60).unwrap();
        let c = verify(S, &t).expect("valid");
        assert_eq!(c.sub, 7);
        assert_eq!(c.role, "admin");
        assert_eq!(c.sid, "fam1");
        assert_eq!(c.exp, exp);
    }

    #[test]
    fn rejects_wrong_secret_and_tamper() {
        let (t, _) = issue(S, 1, "admin", "f", 60).unwrap();
        assert!(verify("another-secret-another-secret-xx", &t).is_none());
        let mut parts: Vec<&str> = t.split('.').collect();
        let forged = issue(S, 999, "admin", "f", 60).unwrap().0;
        let forged_payload = forged.split('.').nth(1).unwrap().to_string();
        parts[1] = &forged_payload;
        // 원래 서명 + 다른 payload → 거부되어야 함
        let tampered = format!("{}.{}.{}", parts[0], parts[1], t.split('.').nth(2).unwrap());
        assert!(verify(S, &tampered).is_none());
    }

    #[test]
    fn rejects_expired() {
        let (t, _) = issue(S, 1, "admin", "f", -120).unwrap();
        assert!(verify(S, &t).is_none());
    }

    #[test]
    fn rejects_alg_none() {
        // {"alg":"none"} 헤더 토큰은 절대 통과하면 안 됨
        let header = "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0";
        let (t, _) = issue(S, 1, "admin", "f", 60).unwrap();
        let payload = t.split('.').nth(1).unwrap();
        assert!(verify(S, &format!("{header}.{payload}.")).is_none());
    }
}
