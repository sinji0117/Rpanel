//! 관리자 2단계 인증 (TOTP) — 내 계정에서 켜기 · 끄기 · 복구 코드
//!
//! - 켜기: setup(비밀키 발급, 10분 보관) → enable(인증 앱의 6자리 코드로 확인) → 복구 코드 10개
//! - 로그인: 비밀번호가 맞으면 mfa_token 을 주고, /auth/login/otp 에서 코드까지 맞아야 로그인
//! - 복구 코드: 휴대폰을 잃었을 때 6자리 코드 대신 한 번씩 쓸 수 있음 (DB 에는 해시만 저장)
//! - 같은 30초 코드는 두 번 쓸 수 없음 (Redis 에 쓴 시간 단계 기록)
//! - 비밀키는 패널 비밀키(JWT_SECRET)로 암호화해 저장 (secretbox). 예전 평문 값은 다음 로그인 때 암호화

use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use rand::RngCore;
use redis::AsyncCommands;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::{password, secretbox, session, totp, AuthUser, ClientIp};
use crate::{
    audit,
    error::{AppError, AppResult},
    state::AppState,
};

const ISSUER: &str = "Rpanel";
/// 등록 중인 비밀키 보관 시간
const SETUP_TTL: u64 = 600;
const RECOVERY_COUNT: usize = 10;

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 2단계 인증 키를 암호화하는 키 (패널 비밀키에서 만듦)
fn master(state: &AppState) -> &[u8] {
    state.cfg.jwt_secret.as_bytes()
}

fn seal_secret(state: &AppState, secret: &str) -> String {
    let mut nonce = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut nonce);
    secretbox::seal(master(state), nonce, secret)
}

/// 보안 알림 (보안 변경)
async fn notify(state: &AppState, level: &'static str, title: &'static str, email: &str, ip: &str) {
    crate::notify::emit(
        state,
        crate::notify::Event::new("security", level, title, json!({ "email": email }))
            .body(format!("IP {ip}"))
            .link("/security?tab=2fa"),
    )
    .await;
}

fn normalize(code: &str) -> String {
    code.trim().to_ascii_lowercase().replace([' ', '-'], "")
}

fn hash_code(code: &str) -> String {
    hex::encode(Sha256::digest(normalize(code).as_bytes()))
}

/// "a1b2-c3d4" 모양 복구 코드 10개
fn new_recovery_codes() -> Vec<String> {
    (0..RECOVERY_COUNT)
        .map(|_| {
            let t = session::random_token(4);
            format!("{}-{}", &t[..4], &t[4..])
        })
        .collect()
}

#[derive(sqlx::FromRow)]
pub(crate) struct TotpRow {
    pub email: String,
    pub password_hash: String,
    pub totp_secret: Option<String>,
    pub totp_enabled: bool,
    pub totp_recovery: Option<String>,
}

pub(crate) async fn load(state: &AppState, uid: u64) -> AppResult<TotpRow> {
    Ok(sqlx::query_as::<_, TotpRow>(
        "SELECT email, password_hash, totp_secret, totp_enabled, totp_recovery FROM users WHERE id = ?",
    )
    .bind(uid)
    .fetch_one(&state.db)
    .await?)
}

fn recovery_list(row: &TotpRow) -> Vec<String> {
    row.totp_recovery
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default()
}

/// 6자리 코드 또는 복구 코드 확인. 맞으면 Some("totp" | "recovery").
/// 복구 코드는 쓰는 즉시 지우고, 6자리 코드는 같은 시간 단계를 다시 쓰지 못하게 기록합니다.
pub(crate) async fn check_code(
    state: &AppState,
    uid: u64,
    row: &TotpRow,
    code: &str,
) -> AppResult<Option<&'static str>> {
    let Some(stored) = row.totp_secret.as_deref().filter(|_| row.totp_enabled) else {
        return Ok(None);
    };
    let c = code.trim().replace(' ', "");
    if c.len() == totp::DIGITS as usize && c.bytes().all(|b| b.is_ascii_digit()) {
        let legacy = !secretbox::is_sealed(stored);
        let secret = if legacy {
            stored.to_string()
        } else {
            match secretbox::open(master(state), stored) {
                Some(s) => s,
                None => {
                    // JWT_SECRET 이 바뀌면 풀 수 없음 → 복구 코드나 관리자 초기화로
                    tracing::warn!(
                        uid,
                        "2FA secret could not be decrypted (JWT_SECRET changed?)"
                    );
                    return Ok(None);
                }
            }
        };
        let Some(step) = totp::verify(&secret, &c, now_unix()) else {
            return Ok(None);
        };
        if legacy {
            // 예전(평문) 저장값 → 암호화해서 다시 저장
            let _ =
                sqlx::query("UPDATE users SET totp_secret = ? WHERE id = ? AND totp_secret = ?")
                    .bind(seal_secret(state, &secret))
                    .bind(uid)
                    .bind(stored)
                    .execute(&state.db)
                    .await;
        }
        let mut r = state.redis.clone();
        let fresh = redis::cmd("SET")
            .arg(state.key(&["totp_used", &uid.to_string(), &step.to_string()]))
            .arg(1)
            .arg("NX")
            .arg("EX")
            .arg(totp::STEP * 4)
            .query_async::<Option<String>>(&mut r)
            .await?
            .is_some();
        return Ok(fresh.then_some("totp"));
    }
    let h = hash_code(&c);
    let mut list = recovery_list(row);
    let Some(pos) = list.iter().position(|x| *x == h) else {
        return Ok(None);
    };
    list.remove(pos);
    // 동시에 같은 코드를 두 번 쓰지 못하게, 저장된 값이 그대로일 때만 바꿈
    let res = sqlx::query("UPDATE users SET totp_recovery = ? WHERE id = ? AND totp_recovery = ?")
        .bind(serde_json::to_string(&list).unwrap_or_else(|_| "[]".into()))
        .bind(uid)
        .bind(row.totp_recovery.as_deref().unwrap_or(""))
        .execute(&state.db)
        .await?;
    Ok((res.rows_affected() == 1).then_some("recovery"))
}

async fn limit(state: &AppState, uid: u64) -> AppResult<()> {
    if !session::rate_limit(state, &format!("2fa:{uid}"), 10, 300).await? {
        return Err(AppError::TooManyRequests);
    }
    Ok(())
}

fn setup_key(state: &AppState, uid: u64) -> String {
    state.key(&["totp_setup", &uid.to_string()])
}

const POLICY_KEY: &str = "require_2fa";

/// 모든 관리자에게 2단계 인증 필수인지 (패널 설정)
pub async fn required(state: &AppState) -> bool {
    sqlx::query_scalar::<_, String>("SELECT v FROM panel_settings WHERE k = ?")
        .bind(POLICY_KEY)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .is_some_and(|v| v == "1")
}

pub async fn set_required(state: &AppState, on: bool) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO panel_settings (k, v) VALUES (?, ?) ON DUPLICATE KEY UPDATE v = VALUES(v)",
    )
    .bind(POLICY_KEY)
    .bind(if on { "1" } else { "0" })
    .execute(&state.db)
    .await?;
    // 아직 등록하지 않은 계정은 바로 등록 화면으로 (끄면 모두 풀림)
    let users: Vec<(u64, bool)> = sqlx::query_as("SELECT id, totp_enabled FROM users")
        .fetch_all(&state.db)
        .await?;
    for (uid, enabled) in users {
        session::set_needs_2fa(state, uid, on && !enabled).await?;
    }
    Ok(())
}

/// GET /api/auth/2fa — 내 2단계 인증 상태 (required: 필수 정책)
pub async fn status(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<Value>> {
    let row = load(&state, user.id).await?;
    Ok(Json(json!({
        "enabled": row.totp_enabled,
        "recovery_left": if row.totp_enabled { recovery_list(&row).len() } else { 0 },
        "required": required(&state).await,
    })))
}

/// POST /api/auth/2fa/setup — 새 비밀키 (인증 앱에 등록할 QR 주소)
pub async fn setup(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<Value>> {
    limit(&state, user.id).await?;
    let row = load(&state, user.id).await?;
    if row.totp_enabled {
        return Err(AppError::Conflict(
            "이미 2단계 인증을 쓰고 있습니다. 다시 등록하려면 먼저 끄세요".into(),
        ));
    }
    let mut key = [0u8; 20];
    rand::thread_rng().fill_bytes(&mut key);
    let secret = totp::base32_encode(&key);
    let mut r = state.redis.clone();
    let _: () = r
        .set_ex(setup_key(&state, user.id), &secret, SETUP_TTL)
        .await?;
    Ok(Json(json!({
        "secret": secret,
        "uri": totp::otpauth_uri(ISSUER, &row.email, &secret),
        "expires_in": SETUP_TTL,
    })))
}

#[derive(Deserialize)]
pub struct CodeInput {
    pub code: String,
}

/// POST /api/auth/2fa/enable — 인증 앱의 코드로 확인하고 켬 → 복구 코드
pub async fn enable(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Json(b): Json<CodeInput>,
) -> AppResult<Json<Value>> {
    limit(&state, user.id).await?;
    let mut r = state.redis.clone();
    let pending: Option<String> = r.get(setup_key(&state, user.id)).await?;
    let Some(secret) = pending else {
        return Err(AppError::bad(
            "등록 시간이 지났습니다. 처음부터 다시 시작하세요",
        ));
    };
    let Some(step) = totp::verify(&secret, &b.code, now_unix()) else {
        return Err(AppError::bad(
            "코드가 맞지 않습니다. 휴대폰 시간이 자동으로 맞춰져 있는지 확인하세요",
        ));
    };
    // 방금 쓴 코드로 바로 로그인하지 못하게 기록
    let _: () = r
        .set_ex(
            state.key(&["totp_used", &user.id.to_string(), &step.to_string()]),
            1,
            totp::STEP * 4,
        )
        .await?;
    let codes = new_recovery_codes();
    let hashes: Vec<String> = codes.iter().map(|c| hash_code(c)).collect();
    sqlx::query(
        "UPDATE users SET totp_secret = ?, totp_enabled = 1, totp_recovery = ? WHERE id = ?",
    )
    .bind(seal_secret(&state, &secret))
    .bind(serde_json::to_string(&hashes).unwrap_or_else(|_| "[]".into()))
    .bind(user.id)
    .execute(&state.db)
    .await?;
    let _: () = r.del(setup_key(&state, user.id)).await?;
    session::set_needs_2fa(&state, user.id, false).await?;
    let row = load(&state, user.id).await?;
    audit::log(&state, Some(user.id), "auth.2fa_enabled", &row.email, &ip).await;
    notify(
        &state,
        "info",
        "2단계 인증을 켰습니다: {email}",
        &row.email,
        &ip,
    )
    .await;
    Ok(Json(json!({ "recovery_codes": codes })))
}

#[derive(Deserialize)]
pub struct ConfirmInput {
    pub password: String,
    pub code: String,
}

async fn confirm(state: &AppState, uid: u64, b: &ConfirmInput) -> AppResult<TotpRow> {
    limit(state, uid).await?;
    let row = load(state, uid).await?;
    if !row.totp_enabled {
        return Err(AppError::bad("2단계 인증이 꺼져 있습니다"));
    }
    if !password::verify(&b.password, &row.password_hash) {
        return Err(AppError::bad("비밀번호가 올바르지 않습니다"));
    }
    if check_code(state, uid, &row, &b.code).await?.is_none() {
        return Err(AppError::bad("인증 코드가 맞지 않습니다"));
    }
    Ok(row)
}

/// POST /api/auth/2fa/disable — 비밀번호 + 코드(또는 복구 코드)로 끔
pub async fn disable(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Json(b): Json<ConfirmInput>,
) -> AppResult<StatusCode> {
    if required(&state).await {
        return Err(AppError::bad(
            "2단계 인증이 필수로 정해져 있어 끌 수 없습니다",
        ));
    }
    let row = confirm(&state, user.id, &b).await?;
    sqlx::query(
        "UPDATE users SET totp_secret = NULL, totp_enabled = 0, totp_recovery = NULL WHERE id = ?",
    )
    .bind(user.id)
    .execute(&state.db)
    .await?;
    audit::log(&state, Some(user.id), "auth.2fa_disabled", &row.email, &ip).await;
    notify(
        &state,
        "warning",
        "2단계 인증을 껐습니다: {email}",
        &row.email,
        &ip,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/auth/2fa/recovery — 복구 코드 새로 만들기 (예전 코드는 못 씀)
pub async fn regenerate(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Json(b): Json<ConfirmInput>,
) -> AppResult<Json<Value>> {
    let row = confirm(&state, user.id, &b).await?;
    let codes = new_recovery_codes();
    let hashes: Vec<String> = codes.iter().map(|c| hash_code(c)).collect();
    sqlx::query("UPDATE users SET totp_recovery = ? WHERE id = ?")
        .bind(serde_json::to_string(&hashes).unwrap_or_else(|_| "[]".into()))
        .bind(user.id)
        .execute(&state.db)
        .await?;
    audit::log(&state, Some(user.id), "auth.2fa_recovery", &row.email, &ip).await;
    Ok(Json(json!({ "recovery_codes": codes })))
}

/// DELETE /api/users/{id}/2fa — 다른 관리자의 2단계 인증 초기화 (휴대폰을 잃었을 때)
pub async fn reset_for_user(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
) -> AppResult<StatusCode> {
    user.require_admin()?;
    if id == user.id {
        return Err(AppError::bad(
            "내 2단계 인증은 보안 → 2단계 인증에서 끄세요",
        ));
    }
    let row = load(&state, id).await?;
    sqlx::query(
        "UPDATE users SET totp_secret = NULL, totp_enabled = 0, totp_recovery = NULL WHERE id = ?",
    )
    .bind(id)
    .execute(&state.db)
    .await?;
    // 그 계정의 로그인도 모두 끊어, 다시 로그인하면서 새로 등록하게 함
    session::revoke_all_for_user(&state, id).await?;
    if required(&state).await {
        session::set_needs_2fa(&state, id, true).await?;
    }
    audit::log(&state, Some(user.id), "admin.2fa_reset", &row.email, &ip).await;
    notify(
        &state,
        "warning",
        "2단계 인증을 초기화했습니다: {email}",
        &row.email,
        &ip,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_codes_shape() {
        let c = new_recovery_codes();
        assert_eq!(c.len(), RECOVERY_COUNT);
        for x in &c {
            assert_eq!(x.len(), 9);
            assert_eq!(&x[4..5], "-");
        }
        // 대소문자 · 하이픈 · 공백과 상관없이 같은 해시
        assert_eq!(hash_code("A1B2-C3D4"), hash_code(" a1b2c3d4 "));
        assert_ne!(hash_code("a1b2-c3d4"), hash_code("a1b2-c3d5"));
    }
}
