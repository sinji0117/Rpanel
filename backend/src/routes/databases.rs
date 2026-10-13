//! MariaDB 데이터베이스 / Redis DB 번호 관리 (CloudPanel의 Databases)
//!
//! - DB·계정 생성/삭제/비밀번호 교체는 `rpanel-helper db-*` 가 root(unix_socket)로 수행합니다.
//!   helper는 자신이 만든 DB 목록(/etc/rpanel/managed-dbs)에 있는 것만 지우거나 덤프합니다.
//! - 앱 배포 시 `DATABASE_URL`(또는 지정 키) / `REDIS_URL` 이 자동 주입됩니다.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    audit,
    auth::{session::random_token, AuthUser, ClientIp},
    config::redis_db_index,
    deploy::{helper, logsink::LogSink},
    error::{AppError, AppResult},
    models::{App, AppDatabase, DB_COLUMNS},
    routes::apps,
    state::AppState,
    validate,
};

const RESERVED: &[&str] = &[
    "mysql",
    "information_schema",
    "performance_schema",
    "sys",
    "test",
    "root",
    "rpanel",
    "mariadb.sys",
];

/// 앱에 자동 주입되는 환경변수
#[derive(Debug, Clone, Serialize)]
pub struct ManagedVar {
    pub key: String,
    pub value: String,
    pub source: String,
}

pub fn database_url(state: &AppState, db: &AppDatabase) -> String {
    format!(
        "mysql://{}:{}@{}/{}",
        db.username, db.password, state.cfg.app_db_host, db.name
    )
}

pub fn redis_url(state: &AppState, n: u32) -> String {
    format!("{}/{}", state.cfg.app_redis_base.trim_end_matches('/'), n)
}

pub async fn list_for_app(state: &AppState, app_id: u64) -> AppResult<Vec<AppDatabase>> {
    Ok(sqlx::query_as::<_, AppDatabase>(&format!(
        "SELECT {DB_COLUMNS} FROM app_databases WHERE app_id = ? ORDER BY id"
    ))
    .bind(app_id)
    .fetch_all(&state.db)
    .await?)
}

pub async fn managed_env(state: &AppState, app: &App) -> AppResult<Vec<ManagedVar>> {
    let mut out = Vec::new();
    for db in list_for_app(state, app.id).await? {
        out.push(ManagedVar {
            key: db.env_key.clone(),
            value: database_url(state, &db),
            source: format!("MariaDB · {}", db.name),
        });
    }
    if let Some(n) = app.redis_db {
        out.push(ManagedVar {
            key: "REDIS_URL".into(),
            value: redis_url(state, n),
            source: format!("Redis · DB {n}"),
        });
    }
    Ok(out)
}

/// 패널 자신의 DB 이름 (DATABASE_URL 경로)
fn panel_db_name(url: &str) -> String {
    url.rsplit('/')
        .next()
        .unwrap_or("")
        .split('?')
        .next()
        .unwrap_or("")
        .to_string()
}

pub fn check_db_ident(s: &str, max: usize, what: &str) -> Result<(), String> {
    let ok = s.len() >= 2
        && s.len() <= max
        && s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !ok {
        return Err(format!(
            "{what}은(는) 소문자로 시작하는 소문자·숫자·_ 2~{max}자여야 합니다"
        ));
    }
    if RESERVED.contains(&s) {
        return Err(format!("{s} 은(는) 예약된 이름입니다"));
    }
    Ok(())
}

/// slug → 기본 DB 이름 (beripick-api → beripick_api)
pub fn default_db_name(slug: &str) -> String {
    let mut s: String = slug.replace('-', "_");
    if !s.starts_with(|c: char| c.is_ascii_lowercase()) {
        s = format!("app_{s}");
    }
    s.chars()
        .take(32)
        .collect::<String>()
        .trim_end_matches('_')
        .to_string()
}

#[derive(Serialize)]
pub struct DbView {
    #[serde(flatten)]
    pub db: AppDatabase,
    pub app_name: String,
    pub app_slug: String,
}

#[derive(Serialize)]
pub struct RedisView {
    pub app_id: u64,
    pub app_name: String,
    pub app_slug: String,
    pub db: u32,
    pub keys: Option<u64>,
}

/// GET /api/databases — 전체 DB·Redis 배정 현황
pub async fn list_all(
    State(state): State<AppState>,
    user: AuthUser,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let dbs = sqlx::query_as::<_, AppDatabase>(&format!(
        "SELECT {DB_COLUMNS} FROM app_databases ORDER BY name"
    ))
    .fetch_all(&state.db)
    .await?;
    let apps_rows: Vec<(u64, String, String, Option<u32>)> =
        sqlx::query_as("SELECT id, name, slug, redis_db FROM apps ORDER BY name")
            .fetch_all(&state.db)
            .await?;
    let find = |id: u64| {
        apps_rows
            .iter()
            .find(|a| a.0 == id)
            .map(|a| (a.1.clone(), a.2.clone()))
            .unwrap_or_default()
    };
    let databases: Vec<DbView> = dbs
        .into_iter()
        .map(|db| {
            let (app_name, app_slug) = find(db.app_id);
            DbView {
                db,
                app_name,
                app_slug,
            }
        })
        .collect();
    let mut redis = Vec::new();
    for (id, name, slug, n) in &apps_rows {
        if let Some(n) = n {
            redis.push(RedisView {
                app_id: *id,
                app_name: name.clone(),
                app_slug: slug.clone(),
                db: *n,
                keys: redis_dbsize(&state, *n).await.ok(),
            });
        }
    }
    Ok(Json(json!({
        "databases": databases,
        "redis": redis,
        "redis_range": [state.cfg.redis_db_min, state.cfg.redis_db_max],
        "panel_redis_db": redis_db_index(&state.cfg.redis_url),
    })))
}

/// GET /api/apps/{id}/databases
pub async fn get_for_app(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<u64>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let app = apps::fetch(&state, id).await?;
    let dbs = list_for_app(&state, id).await?;
    let databases: Vec<serde_json::Value> = dbs
        .iter()
        .map(|db| {
            json!({
                "id": db.id, "name": db.name, "username": db.username, "env_key": db.env_key,
                "created_at": db.created_at, "url": database_url(&state, db),
            })
        })
        .collect();
    let redis = match app.redis_db {
        Some(n) => Some(json!({
            "db": n,
            "url": redis_url(&state, n),
            "keys": redis_dbsize(&state, n).await.ok(),
        })),
        None => None,
    };
    Ok(Json(json!({
        "databases": databases,
        "redis": redis,
        "suggested_name": default_db_name(&app.slug),
    })))
}

#[derive(Deserialize)]
pub struct CreateDb {
    pub name: Option<String>,
    pub username: Option<String>,
    pub env_key: Option<String>,
}

/// POST /api/apps/{id}/databases
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
    Json(body): Json<CreateDb>,
) -> AppResult<(StatusCode, Json<serde_json::Value>)> {
    user.require_admin()?;
    let app = apps::fetch(&state, id).await?;
    let name = body
        .name
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_db_name(&app.slug));
    let username = body
        .username
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| name.chars().take(32).collect());
    let env_key = body
        .env_key
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "DATABASE_URL".into());
    check_db_ident(&name, 64, "DB 이름").map_err(AppError::BadRequest)?;
    check_db_ident(&username, 32, "사용자 이름").map_err(AppError::BadRequest)?;
    validate::env_key(&env_key).map_err(AppError::BadRequest)?;
    if name == panel_db_name(&state.cfg.database_url) {
        return Err(AppError::bad("패널 자신의 DB 이름은 쓸 수 없습니다"));
    }
    let exists: Option<(u64,)> = sqlx::query_as(
        "SELECT id FROM app_databases WHERE name = ? OR username = ? OR (app_id = ? AND env_key = ?)",
    )
    .bind(&name)
    .bind(&username)
    .bind(id)
    .bind(&env_key)
    .fetch_optional(&state.db)
    .await?;
    if exists.is_some() {
        return Err(AppError::Conflict(format!(
            "이미 사용 중인 DB 이름/사용자 이름이거나, 이 앱에 {env_key} 가 이미 연결되어 있습니다"
        )));
    }

    let log = LogSink::capture();
    let out = helper::call(&state, &["db-create", &name, &username], &log)
        .await
        .map_err(|e| AppError::BadRequest(format!("{e:#}")))?;
    let mut password = out.stdout.trim().to_string();
    if password.is_empty() && state.exec.is_dry_run() {
        password = random_token(24);
    }
    if password.len() < 16 || !password.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(AppError::Internal(anyhow::anyhow!(
            "helper가 올바른 비밀번호를 반환하지 않았습니다"
        )));
    }
    let res = sqlx::query(
        "INSERT INTO app_databases (app_id, name, username, password, env_key) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(&name)
    .bind(&username)
    .bind(&password)
    .bind(&env_key)
    .execute(&state.db)
    .await?;
    audit::log(
        &state,
        Some(user.id),
        "db.create",
        &format!("{name} → {}", app.slug),
        &ip,
    )
    .await;
    Ok((
        StatusCode::CREATED,
        Json(
            json!({ "id": res.last_insert_id(), "name": name, "username": username, "env_key": env_key,
                     "note": "다음 배포부터 환경변수로 주입됩니다" }),
        ),
    ))
}

async fn fetch_db(state: &AppState, app_id: u64, db_id: u64) -> AppResult<AppDatabase> {
    sqlx::query_as::<_, AppDatabase>(&format!(
        "SELECT {DB_COLUMNS} FROM app_databases WHERE id = ? AND app_id = ?"
    ))
    .bind(db_id)
    .bind(app_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)
}

/// DELETE /api/apps/{id}/databases/{db}
pub async fn drop(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path((id, db_id)): Path<(u64, u64)>,
) -> AppResult<StatusCode> {
    user.require_admin()?;
    let db = fetch_db(&state, id, db_id).await?;
    let log = LogSink::capture();
    helper::call(&state, &["db-drop", &db.name, &db.username], &log)
        .await
        .map_err(|e| AppError::BadRequest(format!("{e:#}")))?;
    sqlx::query("DELETE FROM app_databases WHERE id = ?")
        .bind(db_id)
        .execute(&state.db)
        .await?;
    audit::log(&state, Some(user.id), "db.drop", &db.name, &ip).await;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/apps/{id}/databases/{db}/password — 비밀번호 교체 (재배포 필요)
pub async fn rotate_password(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path((id, db_id)): Path<(u64, u64)>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let db = fetch_db(&state, id, db_id).await?;
    let log = LogSink::capture();
    let out = helper::call(&state, &["db-password", &db.name, &db.username], &log)
        .await
        .map_err(|e| AppError::BadRequest(format!("{e:#}")))?;
    let mut pw = out.stdout.trim().to_string();
    if pw.is_empty() && state.exec.is_dry_run() {
        pw = random_token(24);
    }
    if pw.len() < 16 || !pw.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(AppError::Internal(anyhow::anyhow!("helper 응답 오류")));
    }
    sqlx::query("UPDATE app_databases SET password = ? WHERE id = ?")
        .bind(&pw)
        .bind(db_id)
        .execute(&state.db)
        .await?;
    audit::log(&state, Some(user.id), "db.password", &db.name, &ip).await;
    Ok(Json(
        json!({ "ok": true, "note": "새 비밀번호는 재배포해야 앱에 반영됩니다" }),
    ))
}

// ───────────── Redis ─────────────

pub fn pick_redis_db(min: u32, max: u32, used: &[u32], panel: u32) -> Option<u32> {
    (min..=max).find(|n| *n != panel && !used.contains(n))
}

async fn redis_conn(state: &AppState, n: u32) -> AppResult<redis::aio::MultiplexedConnection> {
    let client = redis::Client::open(redis_url(state, n))?;
    Ok(client.get_multiplexed_async_connection().await?)
}

pub async fn redis_dbsize(state: &AppState, n: u32) -> AppResult<u64> {
    let mut c = redis_conn(state, n).await?;
    Ok(redis::cmd("DBSIZE").query_async(&mut c).await?)
}

/// POST /api/apps/{id}/redis — Redis DB 번호 배정
pub async fn redis_assign(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let app = apps::fetch(&state, id).await?;
    if let Some(n) = app.redis_db {
        return Ok(Json(json!({ "db": n, "url": redis_url(&state, n) })));
    }
    let mut tx = state.db.begin().await?;
    let used: Vec<u32> =
        sqlx::query_scalar("SELECT redis_db FROM apps WHERE redis_db IS NOT NULL FOR UPDATE")
            .fetch_all(&mut *tx)
            .await?;
    let n = pick_redis_db(
        state.cfg.redis_db_min,
        state.cfg.redis_db_max,
        &used,
        redis_db_index(&state.cfg.redis_url),
    )
    .ok_or_else(|| {
        AppError::Conflict(format!(
            "남은 Redis DB 번호가 없습니다 ({}~{}). redis.conf 의 databases 값을 늘리고 RPANEL_REDIS_DB_MAX 를 조정하세요",
            state.cfg.redis_db_min, state.cfg.redis_db_max
        ))
    })?;
    sqlx::query("UPDATE apps SET redis_db = ? WHERE id = ?")
        .bind(n)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    audit::log(
        &state,
        Some(user.id),
        "redis.assign",
        &format!("{} → DB {n}", app.slug),
        &ip,
    )
    .await;
    Ok(Json(json!({ "db": n, "url": redis_url(&state, n) })))
}

#[derive(Deserialize, Default)]
pub struct RedisRelease {
    #[serde(default)]
    pub flush: bool,
}

/// DELETE /api/apps/{id}/redis — 배정 해제 (flush=true 면 키도 삭제)
pub async fn redis_release(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
    body: Option<Json<RedisRelease>>,
) -> AppResult<StatusCode> {
    user.require_admin()?;
    let app = apps::fetch(&state, id).await?;
    let Some(n) = app.redis_db else {
        return Ok(StatusCode::NO_CONTENT);
    };
    if body.map(|b| b.flush).unwrap_or(false) {
        flush(&state, n).await?;
    }
    sqlx::query("UPDATE apps SET redis_db = NULL WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    audit::log(
        &state,
        Some(user.id),
        "redis.release",
        &format!("{} DB {n}", app.slug),
        &ip,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn flush(state: &AppState, n: u32) -> AppResult<()> {
    if n == redis_db_index(&state.cfg.redis_url) {
        return Err(AppError::bad("패널이 쓰는 Redis DB는 비울 수 없습니다"));
    }
    let mut c = redis_conn(state, n).await?;
    let _: () = redis::cmd("FLUSHDB").query_async(&mut c).await?;
    Ok(())
}

/// POST /api/apps/{id}/redis/flush — 해당 DB 의 키 전부 삭제
pub async fn redis_flush(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let app = apps::fetch(&state, id).await?;
    let n = app
        .redis_db
        .ok_or_else(|| AppError::bad("배정된 Redis DB가 없습니다"))?;
    flush(&state, n).await?;
    audit::log(
        &state,
        Some(user.id),
        "redis.flush",
        &format!("{} DB {n}", app.slug),
        &ip,
    )
    .await;
    Ok(Json(
        json!({ "ok": true, "keys": redis_dbsize(&state, n).await.ok() }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idents() {
        assert!(check_db_ident("beripick_api", 64, "DB").is_ok());
        assert!(check_db_ident("Beri", 64, "DB").is_err());
        assert!(check_db_ident("1db", 64, "DB").is_err());
        assert!(check_db_ident("a`b", 64, "DB").is_err());
        assert!(check_db_ident("a'; DROP", 64, "DB").is_err());
        assert!(check_db_ident("mysql", 64, "DB").is_err());
        assert!(check_db_ident("rpanel", 64, "DB").is_err());
        assert!(check_db_ident(&"a".repeat(33), 32, "user").is_err());
        assert_eq!(default_db_name("beripick-api"), "beripick_api");
        assert_eq!(default_db_name("9shop"), "app_9shop");
        assert_eq!(
            panel_db_name("mysql://u:p@127.0.0.1:3306/rpanel_main?ssl=0"),
            "rpanel_main"
        );
    }

    #[test]
    fn redis_pick() {
        assert_eq!(pick_redis_db(4, 15, &[], 10), Some(4));
        assert_eq!(pick_redis_db(4, 15, &[4, 5], 10), Some(6));
        assert_eq!(pick_redis_db(9, 11, &[9], 10), Some(11));
        assert_eq!(pick_redis_db(10, 10, &[], 10), None);
    }
}
