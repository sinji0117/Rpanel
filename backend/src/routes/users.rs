//! 관리자 계정 관리 — 패널에 로그인하는 사람들
//!
//! 지금은 모든 계정이 관리자(admin) 권한입니다. 자기 자신과 마지막 관리자는 지울 수 없습니다.
//! 비밀번호를 바꾸거나 계정을 지우면 그 계정의 모든 로그인 세션이 바로 끊어집니다.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    audit,
    auth::{password, session, AuthUser, ClientIp},
    error::{AppError, AppResult},
    state::AppState,
    validate,
};

#[derive(Serialize, sqlx::FromRow)]
pub struct AdminView {
    pub id: u64,
    pub email: String,
    pub role: String,
    pub created_at: DateTime<Utc>,
    pub last_login_at: Option<DateTime<Utc>>,
}

async fn fetch(state: &AppState, id: u64) -> AppResult<AdminView> {
    sqlx::query_as::<_, AdminView>(
        "SELECT id, email, role, created_at, last_login_at FROM users WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)
}

/// GET /api/users
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let rows = sqlx::query_as::<_, AdminView>(
        "SELECT id, email, role, created_at, last_login_at FROM users ORDER BY id",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(json!({ "users": rows, "me": user.id })))
}

#[derive(Deserialize)]
pub struct CreateAdmin {
    pub email: String,
    pub password: String,
}

/// POST /api/users — 관리자 추가
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Json(b): Json<CreateAdmin>,
) -> AppResult<(StatusCode, Json<AdminView>)> {
    user.require_admin()?;
    let email = b.email.trim().to_ascii_lowercase();
    validate::email(&email).map_err(AppError::BadRequest)?;
    validate::password(&b.password).map_err(AppError::BadRequest)?;
    let exists: Option<(u64,)> = sqlx::query_as("SELECT id FROM users WHERE email = ?")
        .bind(&email)
        .fetch_optional(&state.db)
        .await?;
    if exists.is_some() {
        return Err(AppError::Conflict(format!(
            "{email} 은(는) 이미 있는 관리자입니다"
        )));
    }
    let hash = password::hash(&b.password)?;
    let res = sqlx::query("INSERT INTO users (email, password_hash, role) VALUES (?, ?, 'admin')")
        .bind(&email)
        .bind(&hash)
        .execute(&state.db)
        .await?;
    audit::log(&state, Some(user.id), "admin.create", &email, &ip).await;
    Ok((
        StatusCode::CREATED,
        Json(fetch(&state, res.last_insert_id()).await?),
    ))
}

#[derive(Deserialize)]
pub struct UpdateAdmin {
    pub email: Option<String>,
    /// 새 비밀번호 (비우면 그대로)
    pub password: Option<String>,
}

/// PUT /api/users/{id} — 이메일 · 비밀번호 변경 (비밀번호를 바꾸면 그 계정의 세션 모두 종료)
pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
    Json(b): Json<UpdateAdmin>,
) -> AppResult<Json<AdminView>> {
    user.require_admin()?;
    let cur = fetch(&state, id).await?;
    if let Some(email) = b.email.map(|e| e.trim().to_ascii_lowercase()) {
        if email != cur.email {
            validate::email(&email).map_err(AppError::BadRequest)?;
            let clash: Option<(u64,)> =
                sqlx::query_as("SELECT id FROM users WHERE email = ? AND id <> ?")
                    .bind(&email)
                    .bind(id)
                    .fetch_optional(&state.db)
                    .await?;
            if clash.is_some() {
                return Err(AppError::Conflict(format!(
                    "{email} 은(는) 이미 있는 관리자입니다"
                )));
            }
            sqlx::query("UPDATE users SET email = ? WHERE id = ?")
                .bind(&email)
                .bind(id)
                .execute(&state.db)
                .await?;
            audit::log(
                &state,
                Some(user.id),
                "admin.email",
                &format!("{} → {email}", cur.email),
                &ip,
            )
            .await;
        }
    }
    if let Some(pw) = b.password.filter(|p| !p.is_empty()) {
        validate::password(&pw).map_err(AppError::BadRequest)?;
        let hash = password::hash(&pw)?;
        sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
            .bind(&hash)
            .bind(id)
            .execute(&state.db)
            .await?;
        session::revoke_all_for_user(&state, id).await?;
        audit::log(&state, Some(user.id), "admin.password", &cur.email, &ip).await;
    }
    Ok(Json(fetch(&state, id).await?))
}

/// DELETE /api/users/{id}
pub async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
) -> AppResult<StatusCode> {
    user.require_admin()?;
    let cur = fetch(&state, id).await?;
    if id == user.id {
        return Err(AppError::bad(
            "자기 자신은 지울 수 없습니다. 다른 관리자로 로그인해서 지우세요",
        ));
    }
    let admins: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role = 'admin'")
        .fetch_one(&state.db)
        .await?;
    if admins <= 1 {
        return Err(AppError::bad("마지막 관리자는 지울 수 없습니다"));
    }
    session::revoke_all_for_user(&state, id).await?;
    // 감사 로그의 user_id 는 기록으로 남김 (외래 키가 있으면 NULL 로)
    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    audit::log(&state, Some(user.id), "admin.delete", &cur.email, &ip).await;
    Ok(StatusCode::NO_CONTENT)
}
