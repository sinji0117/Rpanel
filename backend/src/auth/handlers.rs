use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{jwt, password, session, AuthUser, ClientIp};
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
) -> AppResult<(CookieJar, Json<TokenResponse>)> {
    let issued = session::create(state, user.id).await?;
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
    let (jar, body) = issue_tokens(&state, jar, user).await?;
    Ok((StatusCode::CREATED, jar, body))
}

/// POST /api/auth/login
pub async fn login(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    jar: CookieJar,
    Json(body): Json<Credentials>,
) -> AppResult<impl IntoResponse> {
    let email = body.email.trim().to_ascii_lowercase();
    // IP당, 계정당 5분에 10회
    let ip_bucket = format!("login:ip:{ip}");
    let acct_bucket = format!("login:acct:{email}");
    if !session::rate_limit(&state, &ip_bucket, 10, 300).await?
        || !session::rate_limit(&state, &acct_bucket, 10, 300).await?
    {
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
    session::reset_rate_limit(&state, &acct_bucket).await?;
    let _ = sqlx::query("UPDATE users SET last_login_at = CURRENT_TIMESTAMP(3) WHERE id = ?")
        .bind(user.id)
        .execute(&state.db)
        .await;
    audit::log(&state, Some(user.id), "auth.login", &email, &ip).await;
    crate::notify::emit(
        &state,
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
    issue_tokens(&state, jar, user).await
}

/// POST /api/auth/refresh — 리프레시 쿠키로 새 액세스 토큰 (토큰 회전)
pub async fn refresh(
    State(state): State<AppState>,
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

/// POST /api/auth/ws-ticket — WebSocket 접속용 30초 1회성 티켓
pub async fn ws_ticket(
    State(state): State<AppState>,
    user: AuthUser,
) -> AppResult<Json<serde_json::Value>> {
    let t = session::create_ws_ticket(&state, user.id, &user.sid).await?;
    Ok(Json(json!({ "ticket": t, "expires_in": 30 })))
}
