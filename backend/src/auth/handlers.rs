use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{jwt, password, session, twofa, AuthUser, ClientIp, UserAgent};
use crate::{
    audit,
    error::{AppError, AppResult},
    models::{User, UserView},
    state::AppState,
    validate,
};

pub const REFRESH_COOKIE: &str = "rpanel_rt";
const REFRESH_PATH: &str = "/api/auth";

#[derive(Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: &'static str,
    pub expires_at: i64,
    pub user: UserView,
}

#[derive(Deserialize)]
pub struct Credentials {
    pub email: String,
    pub password: String,
}

fn refresh_cookie(state: &AppState, value: String) -> Cookie<'static> {
    Cookie::build((REFRESH_COOKIE, value))
        .http_only(true)
        .secure(state.cfg.cookie_secure)
        .same_site(SameSite::Strict)
        .path(REFRESH_PATH)
        .max_age(time_dur(state.cfg.refresh_ttl_secs))
        .build()
}

fn clear_cookie() -> Cookie<'static> {
    Cookie::build((REFRESH_COOKIE, ""))
        .path(REFRESH_PATH)
        .max_age(time_dur(0))
        .build()
}

fn time_dur(secs: i64) -> time::Duration {
    time::Duration::seconds(secs)
}

/// 쿠키 기반 엔드포인트의 CSRF 방어: 단순 폼 전송으로는 붙일 수 없는 커스텀 헤더 요구
fn require_csrf_header(headers: &HeaderMap) -> AppResult<()> {
    match headers
        .get("x-requested-with")
        .and_then(|v| v.to_str().ok())
    {
        Some("rpanel") => Ok(()),
        _ => Err(AppError::Forbidden("CSRF 헤더가 없습니다".into())),
    }
}

async fn user_count(state: &AppState) -> AppResult<i64> {
    Ok(sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db)
        .await?)
}

async fn find_user(state: &AppState, id: u64) -> AppResult<User> {
    Ok(sqlx::query_as::<_, User>(
        "SELECT id, email, password_hash, role, created_at FROM users WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?)
}

async fn issue_tokens(
    state: &AppState,
    jar: CookieJar,
    user: User,
    ip: &str,
    ua: &str,
) -> AppResult<(CookieJar, Json<TokenResponse>)> {
    let issued = session::create(state, user.id, ip, ua).await?;
    let (access, exp) = jwt::issue(
        &state.cfg.jwt_secret,
        user.id,
        &user.role,
        &issued.family,
        state.cfg.access_ttl_secs,
    )?;
    let jar = jar.add(refresh_cookie(state, issued.refresh_token));
    Ok((
        jar,
        Json(TokenResponse {
            access_token: access,
            token_type: "Bearer",
            expires_at: exp,
            user: user.into(),
        }),
    ))
}

/// GET /api/auth/status — 최초 관리자 생성이 필요한지
pub async fn status(State(state): State<AppState>) -> AppResult<Json<serde_json::Value>> {
    Ok(Json(
        json!({ "needs_setup": user_count(&state).await? == 0 }),
    ))
}

/// POST /api/auth/setup — 사용자가 한 명도 없을 때만 최초 관리자 생성
pub async fn setup(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    UserAgent(ua): UserAgent,
    jar: CookieJar,
    Json(body): Json<Credentials>,
) -> AppResult<impl IntoResponse> {
    let email = body.email.trim().to_ascii_lowercase();
    validate::email(&email).map_err(AppError::BadRequest)?;
    validate::password(&body.password).map_err(AppError::BadRequest)?;

    // 동시 setup 요청 경쟁 방지: Redis 락
    let mut r = state.redis.clone();
    let lock: bool = redis::cmd("SET")
        .arg(state.key(&["lock", "setup"]))
        .arg(1)
        .arg("NX")
        .arg("EX")
        .arg(10)
        .query_async::<Option<String>>(&mut r)
        .await?
        .is_some();
    if !lock {
        return Err(AppError::Conflict("설정이 진행 중입니다".into()));
    }
    if user_count(&state).await? > 0 {
        return Err(AppError::Forbidden("이미 설정이 완료되었습니다".into()));
    }
    let hash = password::hash(&body.password)?;
    let res = sqlx::query("INSERT INTO users (email, password_hash, role) VALUES (?, ?, 'admin')")
        .bind(&email)
        .bind(&hash)
        .execute(&state.db)
        .await?;
    let user = find_user(&state, res.last_insert_id()).await?;
    audit::log(&state, Some(user.id), "auth.setup", &email, &ip).await;
    let (jar, body) = issue_tokens(&state, jar, user, &ip, &ua).await?;
    Ok((StatusCode::CREATED, jar, body))
}

/// POST /api/auth/login
pub async fn login(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    UserAgent(ua): UserAgent,
    jar: CookieJar,
    Json(body): Json<Credentials>,
) -> AppResult<impl IntoResponse> {
    let email = body.email.trim().to_ascii_lowercase();
    // IP당, 계정당 5분에 10회
    let ip_bucket = format!("login:ip:{ip}");
    let acct_bucket = format!("login:acct:{email}");
    if !session::rate_limit(&state, &ip_bucket, 10, 300).await? {
        notify_blocked(&state, &ip).await;
        return Err(AppError::TooManyRequests);
    }
    if !session::rate_limit(&state, &acct_bucket, 10, 300).await? {
        notify_blocked(&state, &email).await;
        return Err(AppError::TooManyRequests);
    }

    let user = sqlx::query_as::<_, User>(
        "SELECT id, email, password_hash, role, created_at FROM users WHERE email = ?",
    )
    .bind(&email)
    .fetch_optional(&state.db)
    .await?;
    let Some(user) = user else {
        password::dummy_verify(&body.password);
        audit::log(&state, None, "auth.login_failed", &email, &ip).await;
        return Err(AppError::BadRequest(
            "이메일 또는 비밀번호가 올바르지 않습니다".into(),
        ));
    };
    if !password::verify(&body.password, &user.password_hash) {
        audit::log(&state, Some(user.id), "auth.login_failed", &email, &ip).await;
        return Err(AppError::BadRequest(
            "이메일 또는 비밀번호가 올바르지 않습니다".into(),
        ));
    }
    // 2단계 인증을 켠 계정: 코드까지 확인한 뒤 로그인 (/auth/login/otp)
    let mfa: bool = sqlx::query_scalar("SELECT totp_enabled FROM users WHERE id = ?")
        .bind(user.id)
        .fetch_one(&state.db)
        .await?;
    if mfa {
        let token = session::random_token(24);
        let mut r = state.redis.clone();
        let _: () =
            redis::AsyncCommands::set_ex(&mut r, state.key(&["mfa", &token]), user.id, MFA_TTL)
                .await?;
        return Ok(Json(
            json!({ "mfa_required": true, "mfa_token": token, "expires_in": MFA_TTL }),
        )
        .into_response());
    }
    Ok(complete_login(&state, jar, user, &email, &ip, &ua)
        .await?
        .into_response())
}

/// 2단계 인증 코드 입력 시간 (초)
const MFA_TTL: u64 = 300;

/// 로그인 시도 제한에 걸리면 알림 (같은 IP·계정은 5분에 한 번)
async fn notify_blocked(state: &AppState, who: &str) {
    if crate::notify::cooldown(state, &format!("login_blocked:{who}"), 300).await {
        crate::notify::emit(
            state,
            crate::notify::Event::new(
                "login_blocked",
                "warning",
                "로그인 시도 제한: {who}",
                json!({ "who": who }),
            )
            .link("/security?tab=logins"),
        )
        .await;
    }
}

/// 비밀번호(와 2단계 코드)까지 확인된 뒤의 로그인 마무리
async fn complete_login(
    state: &AppState,
    jar: CookieJar,
    user: User,
    email: &str,
    ip: &str,
    ua: &str,
) -> AppResult<(CookieJar, Json<TokenResponse>)> {
    session::reset_rate_limit(state, &format!("login:acct:{email}")).await?;
    if twofa::required(state).await {
        let enabled: bool = sqlx::query_scalar("SELECT totp_enabled FROM users WHERE id = ?")
            .bind(user.id)
            .fetch_one(&state.db)
            .await?;
        session::set_needs_2fa(state, user.id, !enabled).await?;
    }
    let _ = sqlx::query("UPDATE users SET last_login_at = CURRENT_TIMESTAMP(3) WHERE id = ?")
        .bind(user.id)
        .execute(&state.db)
        .await;
    audit::log(state, Some(user.id), "auth.login", email, ip).await;
    crate::notify::emit(
        state,
        crate::notify::Event::new(
            "admin_login",
            "info",
            "관리자 로그인: {email}",
            serde_json::json!({ "email": email }),
        )
        .body(format!("IP {ip}"))
        .link("/logs?tab=audit"),
    )
    .await;
    issue_tokens(state, jar, user, ip, ua).await
}

#[derive(Deserialize)]
pub struct OtpInput {
    pub mfa_token: String,
    pub code: String,
}

/// POST /api/auth/login/otp — 로그인 두 번째 단계: 인증 앱의 6자리 코드 또는 복구 코드
pub async fn login_otp(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    UserAgent(ua): UserAgent,
    jar: CookieJar,
    Json(b): Json<OtpInput>,
) -> AppResult<impl IntoResponse> {
    if b.mfa_token.len() != 48 || !b.mfa_token.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(AppError::bad("잘못된 요청입니다"));
    }
    if !session::rate_limit(&state, &format!("login:ip:{ip}"), 10, 300).await? {
        notify_blocked(&state, &ip).await;
        return Err(AppError::TooManyRequests);
    }
    let key = state.key(&["mfa", &b.mfa_token]);
    let mut r = state.redis.clone();
    let uid: Option<u64> = redis::AsyncCommands::get(&mut r, &key).await?;
    let Some(uid) = uid else {
        return Err(AppError::bad(
            "로그인 시간이 지났습니다. 처음부터 다시 로그인하세요",
        ));
    };
    // 계정당 5분에 10회, 같은 로그인 시도에서 5회까지
    if !session::rate_limit(&state, &format!("login:otp:{uid}"), 10, 300).await? {
        if let Ok(u) = find_user(&state, uid).await {
            notify_blocked(&state, &u.email).await;
        }
        return Err(AppError::TooManyRequests);
    }
    let tries: u64 =
        redis::AsyncCommands::incr(&mut r, state.key(&["mfa_tries", &b.mfa_token]), 1).await?;
    let _: () = redis::AsyncCommands::expire(
        &mut r,
        state.key(&["mfa_tries", &b.mfa_token]),
        MFA_TTL as i64,
    )
    .await?;
    if tries > 5 {
        let _: () = redis::AsyncCommands::del(&mut r, &key).await?;
        return Err(AppError::bad(
            "시도가 너무 많습니다. 처음부터 다시 로그인하세요",
        ));
    }
    let user = find_user(&state, uid).await?;
    let row = twofa::load(&state, uid).await?;
    match twofa::check_code(&state, uid, &row, &b.code).await? {
        Some(kind) => {
            let _: () = redis::AsyncCommands::del(&mut r, &key).await?;
            let email = user.email.clone();
            if kind == "recovery" {
                audit::log(&state, Some(uid), "auth.2fa_recovery_used", &email, &ip).await;
                crate::notify::emit(
                    &state,
                    crate::notify::Event::new(
                        "security",
                        "warning",
                        "복구 코드로 로그인했습니다: {email}",
                        json!({ "email": email }),
                    )
                    .body(format!("IP {ip}"))
                    .link("/security?tab=2fa"),
                )
                .await;
            }
            complete_login(&state, jar, user, &email, &ip, &ua).await
        }
        None => {
            audit::log(&state, Some(uid), "auth.otp_failed", &user.email, &ip).await;
            Err(AppError::bad("인증 코드가 맞지 않습니다"))
        }
    }
}

/// POST /api/auth/refresh — 리프레시 쿠키로 새 액세스 토큰 (토큰 회전)
pub async fn refresh(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    jar: CookieJar,
) -> AppResult<impl IntoResponse> {
    require_csrf_header(&headers)?;
    let token = jar
        .get(REFRESH_COOKIE)
        .map(|c| c.value().to_string())
        .ok_or(AppError::Unauthorized)?;
    match session::rotate(&state, &token).await? {
        session::RotateResult::Ok { uid, issued } => {
            let _ = session::touch(&state, &issued.family, &ip).await;
            let user = match find_user(&state, uid).await {
                Ok(u) => u,
                Err(_) => return Err(AppError::Unauthorized),
            };
            let (access, exp) = jwt::issue(
                &state.cfg.jwt_secret,
                user.id,
                &user.role,
                &issued.family,
                state.cfg.access_ttl_secs,
            )?;
            let jar = jar.add(refresh_cookie(&state, issued.refresh_token));
            Ok((
                jar,
                Json(TokenResponse {
                    access_token: access,
                    token_type: "Bearer",
                    expires_at: exp,
                    user: user.into(),
                }),
            )
                .into_response())
        }
        _ => Ok((
            StatusCode::UNAUTHORIZED,
            jar.add(clear_cookie()),
            Json(json!({"error":"unauthorized","message":"세션이 만료되었습니다"})),
        )
            .into_response()),
    }
}

/// POST /api/auth/logout
pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
) -> AppResult<impl IntoResponse> {
    require_csrf_header(&headers)?;
    if let Some(c) = jar.get(REFRESH_COOKIE) {
        session::revoke_by_token(&state, c.value()).await?;
    }
    Ok((StatusCode::NO_CONTENT, jar.add(clear_cookie())))
}

/// GET /api/auth/me
pub async fn me(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<UserView>> {
    Ok(Json(find_user(&state, user.id).await?.into()))
}

#[derive(Deserialize)]
pub struct ChangePassword {
    pub current_password: String,
    pub new_password: String,
}

/// POST /api/auth/password — 변경 후 모든 세션 폐기
pub async fn change_password(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    user: AuthUser,
    jar: CookieJar,
    Json(body): Json<ChangePassword>,
) -> AppResult<impl IntoResponse> {
    validate::password(&body.new_password).map_err(AppError::BadRequest)?;
    let u = find_user(&state, user.id).await?;
    if !password::verify(&body.current_password, &u.password_hash) {
        return Err(AppError::BadRequest(
            "현재 비밀번호가 올바르지 않습니다".into(),
        ));
    }
    let hash = password::hash(&body.new_password)?;
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(hash)
        .bind(user.id)
        .execute(&state.db)
        .await?;
    session::revoke_all_for_user(&state, user.id).await?;
    audit::log(
        &state,
        Some(user.id),
        "auth.password_changed",
        &u.email,
        &ip,
    )
    .await;
    Ok((StatusCode::NO_CONTENT, jar.add(clear_cookie())))
}

/// GET /api/auth/sessions — 내 로그인 세션 (기기 · IP · 마지막 사용)
pub async fn sessions(
    State(state): State<AppState>,
    user: AuthUser,
) -> AppResult<Json<serde_json::Value>> {
    Ok(Json(json!({
        "sessions": session::list_for_user(&state, user.id).await?,
        "current": user.sid,
    })))
}

/// DELETE /api/auth/sessions/{id} — 다른 기기의 로그인 끝내기
pub async fn revoke_session(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    if id == user.sid {
        return Err(AppError::bad("지금 쓰는 세션은 로그아웃으로 끝내세요"));
    }
    if !session::revoke_for_user(&state, user.id, &id).await? {
        return Err(AppError::NotFound);
    }
    let u = find_user(&state, user.id).await?;
    audit::log(&state, Some(user.id), "auth.session_revoked", &u.email, &ip).await;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/auth/sessions/revoke-others — 지금 기기만 남기고 모두 로그아웃
pub async fn revoke_other_sessions(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
) -> AppResult<Json<serde_json::Value>> {
    let mut n = 0;
    for s in session::list_for_user(&state, user.id).await? {
        if s.id != user.sid && session::revoke_for_user(&state, user.id, &s.id).await? {
            n += 1;
        }
    }
    let u = find_user(&state, user.id).await?;
    audit::log(
        &state,
        Some(user.id),
        "auth.sessions_revoked",
        &format!("{} ({n})", u.email),
        &ip,
    )
    .await;
    Ok(Json(json!({ "revoked": n })))
}

/// POST /api/auth/ws-ticket — WebSocket 접속용 30초 1회성 티켓
pub async fn ws_ticket(
    State(state): State<AppState>,
    user: AuthUser,
) -> AppResult<Json<serde_json::Value>> {
    let t = session::create_ws_ticket(&state, user.id, &user.sid).await?;
    Ok(Json(json!({ "ticket": t, "expires_in": 30 })))
}
