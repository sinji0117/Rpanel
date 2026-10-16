//! MongoDB 데이터베이스·계정 API (MariaDB 화면과 같은 방식)

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    audit,
    auth::{AuthUser, ClientIp},
    error::{AppError, AppResult},
    mongo,
    routes::{apps, databases::password_or_generate},
    state::AppState,
    validate,
};

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct MongoDb {
    pub id: u64,
    pub app_id: Option<u64>,
    pub name: String,
    pub username: String,
    #[serde(skip_serializing)]
    pub password: String,
    pub env_key: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct MongoUser {
    pub id: u64,
    pub database_id: u64,
    pub username: String,
    #[serde(skip_serializing)]
    pub password: String,
    pub role: String,
    pub created_at: DateTime<Utc>,
}

const COLS: &str = "id, app_id, name, username, password, env_key, created_at";
const USER_COLS: &str = "id, database_id, username, password, role, created_at";

pub async fn list_for_app(state: &AppState, app_id: u64) -> AppResult<Vec<MongoDb>> {
    Ok(sqlx::query_as::<_, MongoDb>(&format!(
        "SELECT {COLS} FROM mongo_databases WHERE app_id = ? ORDER BY id"
    ))
    .bind(app_id)
    .fetch_all(&state.db)
    .await?)
}

pub fn url(state: &AppState, d: &MongoDb) -> String {
    mongo::app_url(state, &d.name, &d.username, &d.password)
}

async fn fetch(state: &AppState, id: u64) -> AppResult<MongoDb> {
    sqlx::query_as::<_, MongoDb>(&format!("SELECT {COLS} FROM mongo_databases WHERE id = ?"))
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)
}

async fn users_of(state: &AppState, id: u64) -> AppResult<Vec<MongoUser>> {
    Ok(sqlx::query_as::<_, MongoUser>(&format!(
        "SELECT {USER_COLS} FROM mongo_users WHERE database_id = ? ORDER BY id"
    ))
    .bind(id)
    .fetch_all(&state.db)
    .await?)
}

fn mongo_err(e: anyhow::Error) -> AppError {
    AppError::BadRequest(format!("{e:#}"))
}

/// 같은 이름의 계정이 다른 DB 에서 쓰이고 있으면 헷갈리므로 막음 (MongoDB 는 DB 마다 따로지만 화면에서 구분이 어려움)
async fn username_taken(state: &AppState, name: &str) -> AppResult<bool> {
    let (n,): (i64,) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM mongo_databases WHERE username = ?) + (SELECT COUNT(*) FROM mongo_users WHERE username = ?)",
    )
    .bind(name)
    .bind(name)
    .fetch_one(&state.db)
    .await?;
    Ok(n > 0)
}

/// GET /api/mongo
pub async fn list(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<Value>> {
    user.require_admin()?;
    let info = mongo::server_info(&state).await;
    let dbs =
        sqlx::query_as::<_, MongoDb>(&format!("SELECT {COLS} FROM mongo_databases ORDER BY name"))
            .fetch_all(&state.db)
            .await?;
    let mut out = Vec::new();
    for d in &dbs {
        let users: Vec<Value> = users_of(&state, d.id)
            .await?
            .into_iter()
            .map(|u| {
                let url = mongo::app_url(&state, &d.name, &u.username, &u.password);
                json!({ "id": u.id, "username": u.username, "role": u.role, "created_at": u.created_at, "url": url })
            })
            .collect();
        let app_name: Option<String> = match d.app_id {
            Some(id) => {
                sqlx::query_scalar("SELECT name FROM apps WHERE id = ?")
                    .bind(id)
                    .fetch_optional(&state.db)
                    .await?
            }
            None => None,
        };
        out.push(json!({
            "id": d.id, "app_id": d.app_id, "app_name": app_name, "name": d.name, "username": d.username,
            "env_key": d.env_key, "created_at": d.created_at, "url": url(&state, d),
            "size_bytes": info.sizes.get(&d.name).copied(), "users": users,
        }));
    }
    Ok(Json(json!({
        "configured": state.mongo.is_some(),
        "available": info.available,
        "version": info.version,
        "error": info.error,
        "databases": out,
    })))
}

#[derive(Deserialize)]
pub struct CreateInput {
    pub name: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub app_id: Option<u64>,
    pub env_key: Option<String>,
}

fn env_key(k: Option<String>) -> AppResult<String> {
    let k = k
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "MONGODB_URL".into());
    validate::env_key(&k).map_err(AppError::BadRequest)?;
    Ok(k)
}

/// POST /api/mongo — 데이터베이스 + 전용 계정(readWrite)
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Json(input): Json<CreateInput>,
) -> AppResult<(StatusCode, Json<Value>)> {
    user.require_admin()?;
    let name = input.name.trim().to_lowercase();
    mongo::valid_name("DB 이름", &name).map_err(AppError::BadRequest)?;
    let username = input
        .username
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| name.clone());
    mongo::valid_name("계정 이름", &username).map_err(AppError::BadRequest)?;
    let env_key = env_key(input.env_key)?;
    if let Some(app_id) = input.app_id {
        apps::fetch(&state, app_id).await?;
    }
    let exists: Option<u64> = sqlx::query_scalar("SELECT id FROM mongo_databases WHERE name = ?")
        .bind(&name)
        .fetch_optional(&state.db)
        .await?;
    if exists.is_some() {
        return Err(AppError::Conflict(format!(
            "MongoDB 데이터베이스 {name} 이(가) 이미 있습니다"
        )));
    }
    if username_taken(&state, &username).await? {
        return Err(AppError::Conflict(format!(
            "계정 이름 {username} 은(는) 이미 쓰고 있습니다"
        )));
    }
    let password = password_or_generate(input.password)?;
    mongo::client(&state).map_err(mongo_err)?;
    mongo::create_user(&state, &name, &username, &password, "readWrite")
        .await
        .map_err(mongo_err)?;
    let id = sqlx::query("INSERT INTO mongo_databases (app_id, name, username, password, env_key) VALUES (?, ?, ?, ?, ?)")
        .bind(input.app_id)
        .bind(&name)
        .bind(&username)
        .bind(&password)
        .bind(&env_key)
        .execute(&state.db)
        .await?
        .last_insert_id();
    audit::log(&state, Some(user.id), "mongo.create", &name, &ip).await;
    let d = fetch(&state, id).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "database": d, "url": url(&state, &d), "password": password })),
    ))
}

/// DELETE /api/mongo/{id} — 데이터와 이 DB 의 모든 계정 삭제
pub async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
) -> AppResult<StatusCode> {
    user.require_admin()?;
    let d = fetch(&state, id).await?;
    for u in users_of(&state, id).await? {
        mongo::drop_user(&state, &d.name, &u.username)
            .await
            .map_err(mongo_err)?;
    }
    mongo::drop_user(&state, &d.name, &d.username)
        .await
        .map_err(mongo_err)?;
    mongo::drop_database(&state, &d.name)
        .await
        .map_err(mongo_err)?;
    sqlx::query("DELETE FROM mongo_databases WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    audit::log(&state, Some(user.id), "mongo.drop", &d.name, &ip).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct LinkInput {
    pub app_id: Option<u64>,
    pub env_key: Option<String>,
}

/// PUT /api/mongo/{id} — 앱 연결 / 해제
pub async fn link(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
    Json(input): Json<LinkInput>,
) -> AppResult<Json<Value>> {
    user.require_admin()?;
    let d = fetch(&state, id).await?;
    if let Some(app_id) = input.app_id {
        apps::fetch(&state, app_id).await?;
    }
    let key = env_key(input.env_key)?;
    sqlx::query("UPDATE mongo_databases SET app_id = ?, env_key = ? WHERE id = ?")
        .bind(input.app_id)
        .bind(&key)
        .bind(id)
        .execute(&state.db)
        .await?;
    audit::log(&state, Some(user.id), "mongo.link", &d.name, &ip).await;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct PasswordInput {
    pub password: Option<String>,
}

/// POST /api/mongo/{id}/password — 기본 계정 비밀번호 새로 만들기
pub async fn password(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
    Json(input): Json<PasswordInput>,
) -> AppResult<Json<Value>> {
    user.require_admin()?;
    let d = fetch(&state, id).await?;
    let pw = password_or_generate(input.password)?;
    mongo::set_password(&state, &d.name, &d.username, &pw)
        .await
        .map_err(mongo_err)?;
    sqlx::query("UPDATE mongo_databases SET password = ? WHERE id = ?")
        .bind(&pw)
        .bind(id)
        .execute(&state.db)
        .await?;
    audit::log(&state, Some(user.id), "mongo.password", &d.name, &ip).await;
    let d = fetch(&state, id).await?;
    Ok(Json(json!({ "url": url(&state, &d), "password": pw })))
}

#[derive(Deserialize)]
pub struct UserInput {
    pub username: String,
    pub password: Option<String>,
    /// readWrite | read
    pub role: Option<String>,
}

/// POST /api/mongo/{id}/users — 추가 계정
pub async fn user_create(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
    Json(input): Json<UserInput>,
) -> AppResult<(StatusCode, Json<Value>)> {
    user.require_admin()?;
    let d = fetch(&state, id).await?;
    let username = input.username.trim().to_lowercase();
    mongo::valid_name("계정 이름", &username).map_err(AppError::BadRequest)?;
    let role = match input.role.as_deref().unwrap_or("readWrite") {
        r @ ("readWrite" | "read") => r.to_string(),
        _ => {
            return Err(AppError::bad(
                "권한은 readWrite(읽기·쓰기) 또는 read(읽기 전용)입니다",
            ))
        }
    };
    if username_taken(&state, &username).await? {
        return Err(AppError::Conflict(format!(
            "계정 이름 {username} 은(는) 이미 있습니다"
        )));
    }
    let pw = password_or_generate(input.password)?;
    mongo::create_user(&state, &d.name, &username, &pw, &role)
        .await
        .map_err(mongo_err)?;
    let uid = sqlx::query(
        "INSERT INTO mongo_users (database_id, username, password, role) VALUES (?, ?, ?, ?)",
    )
    .bind(id)
    .bind(&username)
    .bind(&pw)
    .bind(&role)
    .execute(&state.db)
    .await?
    .last_insert_id();
    audit::log(
        &state,
        Some(user.id),
        "mongo.user_create",
        &format!("{} {username}", d.name),
        &ip,
    )
    .await;
    Ok((
        StatusCode::CREATED,
        Json(
            json!({ "id": uid, "username": username, "role": role, "password": pw, "url": mongo::app_url(&state, &d.name, &username, &pw) }),
        ),
    ))
}

async fn fetch_user(state: &AppState, id: u64, uid: u64) -> AppResult<MongoUser> {
    sqlx::query_as::<_, MongoUser>(&format!(
        "SELECT {USER_COLS} FROM mongo_users WHERE id = ? AND database_id = ?"
    ))
    .bind(uid)
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)
}

/// DELETE /api/mongo/{id}/users/{uid}
pub async fn user_delete(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path((id, uid)): Path<(u64, u64)>,
) -> AppResult<StatusCode> {
    user.require_admin()?;
    let d = fetch(&state, id).await?;
    let u = fetch_user(&state, id, uid).await?;
    mongo::drop_user(&state, &d.name, &u.username)
        .await
        .map_err(mongo_err)?;
    sqlx::query("DELETE FROM mongo_users WHERE id = ?")
        .bind(uid)
        .execute(&state.db)
        .await?;
    audit::log(
        &state,
        Some(user.id),
        "mongo.user_drop",
        &format!("{} {}", d.name, u.username),
        &ip,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/mongo/{id}/users/{uid}/password
pub async fn user_password(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path((id, uid)): Path<(u64, u64)>,
    Json(input): Json<PasswordInput>,
) -> AppResult<Json<Value>> {
    user.require_admin()?;
    let d = fetch(&state, id).await?;
    let u = fetch_user(&state, id, uid).await?;
    let pw = password_or_generate(input.password)?;
    mongo::set_password(&state, &d.name, &u.username, &pw)
        .await
        .map_err(mongo_err)?;
    sqlx::query("UPDATE mongo_users SET password = ? WHERE id = ?")
        .bind(&pw)
        .bind(uid)
        .execute(&state.db)
        .await?;
    audit::log(
        &state,
        Some(user.id),
        "mongo.user_password",
        &format!("{} {}", d.name, u.username),
        &ip,
    )
    .await;
    Ok(Json(
        json!({ "password": pw, "url": mongo::app_url(&state, &d.name, &u.username, &pw) }),
    ))
}

/// POST /api/mongo/{id}/backups — 지금 백업 (mongodump)
pub async fn backup_now(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
) -> AppResult<(StatusCode, Json<Value>)> {
    user.require_admin()?;
    let d = fetch(&state, id).await?;
    let ids = crate::backup::start_mongo(&state, &d, "manual", None).await?;
    audit::log(&state, Some(user.id), "mongo.backup", &d.name, &ip).await;
    Ok((StatusCode::ACCEPTED, Json(json!({ "backup_ids": ids }))))
}
