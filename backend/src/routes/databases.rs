//! MariaDB 데이터베이스·계정 / Redis DB 번호·계정 관리
//!
//! - DB·계정 생성/삭제/비밀번호 교체는 `rpanel-helper db-*` 가 root(unix_socket)로 수행합니다.
//!   helper는 자신이 만든 DB·계정(/etc/rpanel/managed-dbs, managed-db-users)만 지우거나 덤프합니다.
//! - 데이터베이스는 앱에 연결하거나(배포 시 `DATABASE_URL` 등 자동 주입) 독립적으로 둘 수 있습니다.
//! - Redis 계정은 ACL 사용자입니다 (`rpanel-helper redis-user-*`, 키 패턴·읽기 전용 제한).

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
    models::{
        App, AppDatabase, DbUser, RedisUser, DB_COLUMNS, DB_USER_COLUMNS, REDIS_USER_COLUMNS,
    },
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

/// 같은 서버의 MariaDB 에 다른 계정으로 접속하는 주소
pub fn database_url_as(state: &AppState, db: &str, user: &str, pw: &str) -> String {
    format!("mysql://{user}:{pw}@{}/{db}", state.cfg.app_db_host)
}

/// Redis 계정용 주소 (host:port 는 패널 설정의 앱용 주소에서 가져옴)
pub fn redis_user_url(state: &AppState, u: &RedisUser) -> String {
    let base = state.cfg.app_redis_base.trim_end_matches('/');
    let host = base.rsplit('@').next().unwrap_or(base);
    let host = host.trim_start_matches("redis://");
    format!(
        "redis://{}:{}@{host}/{}",
        u.username, u.password, u.db_index
    )
}

/// 사용자가 정한 비밀번호 검사 (URL 에 그대로 넣을 수 있는 문자만) — 없으면 새로 만듦
pub fn password_or_generate(pw: Option<String>) -> AppResult<String> {
    match pw.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        Some(p) => {
            let ok = (10..=64).contains(&p.len())
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '~' | '-'));
            if !ok {
                return Err(AppError::bad(
                    "비밀번호는 영문·숫자·_.~- 10~64자로 입력하세요 (비워 두면 자동 생성)",
                ));
            }
            Ok(p)
        }
        None => Ok(random_token(16)),
    }
}

pub async fn users_of(state: &AppState, db_id: u64) -> AppResult<Vec<DbUser>> {
    Ok(sqlx::query_as::<_, DbUser>(&format!(
        "SELECT {DB_USER_COLUMNS} FROM db_users WHERE database_id = ? ORDER BY id"
    ))
    .bind(db_id)
    .fetch_all(&state.db)
    .await?)
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
    pub app_name: Option<String>,
    pub app_slug: Option<String>,
    /// 기본 계정 + 추가 계정 (접속 주소 포함)
    pub users: Vec<serde_json::Value>,
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
    let find = |id: Option<u64>| {
        apps_rows
            .iter()
            .find(|a| Some(a.0) == id)
            .map(|a| (Some(a.1.clone()), Some(a.2.clone())))
            .unwrap_or_default()
    };
    let mut databases: Vec<DbView> = Vec::new();
    for db in dbs {
        let (app_name, app_slug) = find(db.app_id);
        let mut users = vec![json!({
            "id": null, "username": db.username, "privileges": "all", "primary": true,
            "created_at": db.created_at, "url": database_url(&state, &db),
        })];
        for u in users_of(&state, db.id).await? {
            users.push(json!({
                "id": u.id, "username": u.username, "privileges": u.privileges, "primary": false,
                "created_at": u.created_at, "url": database_url_as(&state, &db.name, &u.username, &u.password),
            }));
        }
        databases.push(DbView {
            db,
            app_name,
            app_slug,
            users,
        });
    }
    let redis_users: Vec<serde_json::Value> = sqlx::query_as::<_, RedisUser>(&format!(
        "SELECT {REDIS_USER_COLUMNS} FROM redis_users ORDER BY username"
    ))
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .map(|u| {
        let url = redis_user_url(&state, &u);
        let mut v = serde_json::to_value(&u).unwrap_or_default();
        v["url"] = json!(url);
        v
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
        "redis_users": redis_users,
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
    /// POST /api/databases 에서만: 연결할 앱 (없으면 독립 DB)
    pub app_id: Option<u64>,
}

/// DB + 기본 계정 만들기 (app 이 있으면 그 앱에 연결)
async fn create_db(
    state: &AppState,
    user: &AuthUser,
    ip: &str,
    app: Option<&App>,
    body: CreateDb,
) -> AppResult<serde_json::Value> {
    let name = body
        .name
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .or_else(|| app.map(|a| default_db_name(&a.slug)))
        .ok_or_else(|| AppError::bad("데이터베이스 이름을 입력하세요"))?;
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
    let app_id = app.map(|a| a.id);
    let exists: Option<(u64,)> = sqlx::query_as(
        "SELECT id FROM app_databases WHERE name = ? OR username = ? OR (app_id = ? AND env_key = ?) \
         UNION SELECT id FROM db_users WHERE username = ?",
    )
    .bind(&name)
    .bind(&username)
    .bind(app_id)
    .bind(&env_key)
    .bind(&username)
    .fetch_optional(&state.db)
    .await?;
    if exists.is_some() {
        return Err(AppError::Conflict(format!(
            "이미 사용 중인 DB 이름/사용자 이름이거나, 이 앱에 {env_key} 가 이미 연결되어 있습니다"
        )));
    }

    let log = LogSink::capture();
    let out = helper::call(state, &["db-create", &name, &username], &log)
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
    .bind(app_id)
    .bind(&name)
    .bind(&username)
    .bind(&password)
    .bind(&env_key)
    .execute(&state.db)
    .await?;
    audit::log(
        state,
        Some(user.id),
        "db.create",
        &match app {
            Some(a) => format!("{name} → {}", a.slug),
            None => name.clone(),
        },
        ip,
    )
    .await;
    Ok(json!({
        "id": res.last_insert_id(), "name": name, "username": username, "env_key": env_key,
        "app_id": app_id,
        "url": database_url_as(state, &name, &username, &password),
        "note": if app.is_some() { "다음 배포(적용)부터 환경변수로 주입됩니다" } else { "앱에 연결하지 않은 데이터베이스입니다" },
    }))
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
    let v = create_db(&state, &user, &ip, Some(&app), body).await?;
    Ok((StatusCode::CREATED, Json(v)))
}

/// POST /api/databases — 데이터베이스 만들기 (app_id 를 주면 그 앱에 연결)
pub async fn create_global(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Json(body): Json<CreateDb>,
) -> AppResult<(StatusCode, Json<serde_json::Value>)> {
    user.require_admin()?;
    let app = match body.app_id {
        Some(id) => Some(apps::fetch(&state, id).await?),
        None => None,
    };
    let v = create_db(&state, &user, &ip, app.as_ref(), body).await?;
    Ok((StatusCode::CREATED, Json(v)))
}

async fn fetch_any(state: &AppState, db_id: u64) -> AppResult<AppDatabase> {
    sqlx::query_as::<_, AppDatabase>(&format!(
        "SELECT {DB_COLUMNS} FROM app_databases WHERE id = ?"
    ))
    .bind(db_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)
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
    drop_db(&state, &user, &ip, &db).await
}

async fn drop_db(
    state: &AppState,
    user: &AuthUser,
    ip: &str,
    db: &AppDatabase,
) -> AppResult<StatusCode> {
    let log = LogSink::capture();
    helper::call(state, &["db-drop", &db.name, &db.username], &log)
        .await
        .map_err(|e| AppError::BadRequest(format!("{e:#}")))?;
    // 추가 계정(db_users)과 이 DB 백업 예약은 외래 키로 함께 지워집니다
    sqlx::query("DELETE FROM app_databases WHERE id = ?")
        .bind(db.id)
        .execute(&state.db)
        .await?;
    audit::log(state, Some(user.id), "db.drop", &db.name, ip).await;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/databases/{id}
pub async fn drop_global(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(db_id): Path<u64>,
) -> AppResult<StatusCode> {
    user.require_admin()?;
    let db = fetch_any(&state, db_id).await?;
    drop_db(&state, &user, &ip, &db).await
}

#[derive(Deserialize)]
pub struct LinkDb {
    /// null 이면 앱 연결 해제
    pub app_id: Option<u64>,
    pub env_key: Option<String>,
}

/// PUT /api/databases/{id} — 앱 연결 / 해제
pub async fn link(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(db_id): Path<u64>,
    Json(body): Json<LinkDb>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let db = fetch_any(&state, db_id).await?;
    let env_key = body
        .env_key
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| db.env_key.clone());
    validate::env_key(&env_key).map_err(AppError::BadRequest)?;
    let target = match body.app_id {
        Some(id) => Some(apps::fetch(&state, id).await?),
        None => None,
    };
    if let Some(app) = &target {
        let clash: Option<(u64,)> = sqlx::query_as(
            "SELECT id FROM app_databases WHERE app_id = ? AND env_key = ? AND id <> ?",
        )
        .bind(app.id)
        .bind(&env_key)
        .bind(db_id)
        .fetch_optional(&state.db)
        .await?;
        if clash.is_some() {
            return Err(AppError::Conflict(format!(
                "{} 앱에 {env_key} 가 이미 다른 데이터베이스로 연결되어 있습니다. 다른 환경변수 이름을 쓰세요",
                app.name
            )));
        }
    }
    sqlx::query("UPDATE app_databases SET app_id = ?, env_key = ? WHERE id = ?")
        .bind(target.as_ref().map(|a| a.id))
        .bind(&env_key)
        .bind(db_id)
        .execute(&state.db)
        .await?;
    audit::log(
        &state,
        Some(user.id),
        "db.link",
        &format!(
            "{} → {}",
            db.name,
            target.as_ref().map(|a| a.slug.as_str()).unwrap_or("(없음)")
        ),
        &ip,
    )
    .await;
    Ok(Json(json!({
        "ok": true,
        "note": "앱 쪽은 다음 배포(적용)부터 반영됩니다. 기존 백업 파일은 원래 폴더에 남아 있습니다",
    })))
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
    rotate_primary(&state, &user, &ip, &db).await
}

/// POST /api/databases/{id}/password — 기본 계정 비밀번호 새로 만들기
pub async fn rotate_password_global(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(db_id): Path<u64>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let db = fetch_any(&state, db_id).await?;
    rotate_primary(&state, &user, &ip, &db).await
}

async fn rotate_primary(
    state: &AppState,
    user: &AuthUser,
    ip: &str,
    db: &AppDatabase,
) -> AppResult<Json<serde_json::Value>> {
    let db_id = db.id;
    let log = LogSink::capture();
    let out = helper::call(state, &["db-password", &db.name, &db.username], &log)
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
    audit::log(state, Some(user.id), "db.password", &db.name, ip).await;
    Ok(Json(json!({
        "ok": true,
        "url": database_url_as(state, &db.name, &db.username, &pw),
        "note": "새 비밀번호는 재배포(적용)해야 앱에 반영됩니다",
    })))
}

// ───────────── 데이터베이스 추가 계정 ─────────────

#[derive(Deserialize)]
pub struct CreateDbUser {
    pub username: String,
    /// all | read
    pub privileges: Option<String>,
    pub password: Option<String>,
}

/// POST /api/databases/{id}/users
pub async fn user_create(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(db_id): Path<u64>,
    Json(body): Json<CreateDbUser>,
) -> AppResult<(StatusCode, Json<serde_json::Value>)> {
    user.require_admin()?;
    let db = fetch_any(&state, db_id).await?;
    let username = body.username.trim().to_ascii_lowercase();
    check_db_ident(&username, 32, "사용자 이름").map_err(AppError::BadRequest)?;
    let privileges = body.privileges.unwrap_or_else(|| "all".into());
    if privileges != "all" && privileges != "read" {
        return Err(AppError::bad("권한은 all(읽기·쓰기) 또는 read(읽기 전용)"));
    }
    let taken: Option<(u64,)> = sqlx::query_as(
        "SELECT id FROM app_databases WHERE username = ? UNION SELECT id FROM db_users WHERE username = ?",
    )
    .bind(&username)
    .bind(&username)
    .fetch_optional(&state.db)
    .await?;
    if taken.is_some() {
        return Err(AppError::Conflict(format!(
            "사용자 이름 {username} 은(는) 이미 있습니다"
        )));
    }
    let pw = password_or_generate(body.password)?;
    let log = LogSink::capture();
    helper::call_stdin(
        &state,
        &["db-user-create", &db.name, &username, &privileges],
        &format!("{pw}\n"),
        &log,
    )
    .await
    .map_err(|e| AppError::BadRequest(format!("{e:#}")))?;
    let res = sqlx::query(
        "INSERT INTO db_users (database_id, username, password, privileges) VALUES (?, ?, ?, ?)",
    )
    .bind(db_id)
    .bind(&username)
    .bind(&pw)
    .bind(&privileges)
    .execute(&state.db)
    .await?;
    audit::log(
        &state,
        Some(user.id),
        "db.user.create",
        &format!("{username}@{} ({privileges})", db.name),
        &ip,
    )
    .await;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "id": res.last_insert_id(), "username": username, "privileges": privileges,
            "password": pw, "url": database_url_as(&state, &db.name, &username, &pw),
        })),
    ))
}

async fn fetch_user(state: &AppState, db_id: u64, uid: u64) -> AppResult<DbUser> {
    sqlx::query_as::<_, DbUser>(&format!(
        "SELECT {DB_USER_COLUMNS} FROM db_users WHERE id = ? AND database_id = ?"
    ))
    .bind(uid)
    .bind(db_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)
}

/// DELETE /api/databases/{id}/users/{uid}
pub async fn user_drop(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path((db_id, uid)): Path<(u64, u64)>,
) -> AppResult<StatusCode> {
    user.require_admin()?;
    let db = fetch_any(&state, db_id).await?;
    let u = fetch_user(&state, db_id, uid).await?;
    let log = LogSink::capture();
    helper::call(&state, &["db-user-drop", &db.name, &u.username], &log)
        .await
        .map_err(|e| AppError::BadRequest(format!("{e:#}")))?;
    sqlx::query("DELETE FROM db_users WHERE id = ?")
        .bind(uid)
        .execute(&state.db)
        .await?;
    audit::log(
        &state,
        Some(user.id),
        "db.user.drop",
        &format!("{}@{}", u.username, db.name),
        &ip,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, Default)]
pub struct NewPassword {
    pub password: Option<String>,
}

/// POST /api/databases/{id}/users/{uid}/password  (body: {"password": "..."} 없으면 자동 생성)
pub async fn user_password(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path((db_id, uid)): Path<(u64, u64)>,
    body: Option<Json<NewPassword>>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let db = fetch_any(&state, db_id).await?;
    let u = fetch_user(&state, db_id, uid).await?;
    let pw = password_or_generate(body.and_then(|b| b.0.password))?;
    let log = LogSink::capture();
    helper::call_stdin(
        &state,
        &["db-user-password", &db.name, &u.username],
        &format!("{pw}\n"),
        &log,
    )
    .await
    .map_err(|e| AppError::BadRequest(format!("{e:#}")))?;
    sqlx::query("UPDATE db_users SET password = ? WHERE id = ?")
        .bind(&pw)
        .bind(uid)
        .execute(&state.db)
        .await?;
    audit::log(
        &state,
        Some(user.id),
        "db.user.password",
        &format!("{}@{}", u.username, db.name),
        &ip,
    )
    .await;
    Ok(Json(json!({
        "ok": true, "password": pw,
        "url": database_url_as(&state, &db.name, &u.username, &pw),
    })))
}

// ───────────── Redis 계정 (ACL) ─────────────

pub fn check_redis_user(s: &str) -> Result<(), String> {
    let ok = (3..=32).contains(&s.len())
        && s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
    if !ok {
        return Err("Redis 계정 이름은 소문자로 시작하는 소문자·숫자·_- 3~32자여야 합니다".into());
    }
    if ["default", "rpanel", "admin", "root"].contains(&s) {
        return Err(format!("{s} 은(는) 예약된 이름입니다"));
    }
    Ok(())
}

pub fn check_key_pattern(s: &str) -> Result<(), String> {
    let ok = (1..=100).contains(&s.len())
        && s.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '_' | ':' | '.' | '*' | '{' | '}' | '-')
        });
    if ok {
        Ok(())
    } else {
        Err("키 패턴은 영문·숫자·_:.*{}- 1~100자입니다 (예: myapp:*)".into())
    }
}

#[derive(Deserialize)]
pub struct RedisUserInput {
    pub username: Option<String>,
    pub key_pattern: Option<String>,
    #[serde(default)]
    pub readonly: bool,
    pub db_index: Option<u32>,
    pub note: Option<String>,
    pub password: Option<String>,
}

async fn fetch_redis_user(state: &AppState, id: u64) -> AppResult<RedisUser> {
    sqlx::query_as::<_, RedisUser>(&format!(
        "SELECT {REDIS_USER_COLUMNS} FROM redis_users WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)
}

async fn apply_redis_user(
    state: &AppState,
    username: &str,
    pattern: &str,
    readonly: bool,
    new_password: Option<&str>,
) -> AppResult<()> {
    let log = LogSink::capture();
    let mode = if readonly { "ro" } else { "rw" };
    helper::call_stdin(
        state,
        &["redis-user-set", username, pattern, mode],
        &format!("{}\n", new_password.unwrap_or("")),
        &log,
    )
    .await
    .map_err(|e| AppError::BadRequest(format!("{e:#}")))?;
    Ok(())
}

fn redis_note(note: Option<String>) -> AppResult<Option<String>> {
    let n = note.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    if let Some(v) = &n {
        if v.chars().count() > 200 || v.chars().any(|c| c.is_control()) {
            return Err(AppError::bad("메모는 200자 이하로 입력하세요"));
        }
    }
    Ok(n)
}

/// POST /api/redis-users
pub async fn redis_user_create(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Json(body): Json<RedisUserInput>,
) -> AppResult<(StatusCode, Json<serde_json::Value>)> {
    user.require_admin()?;
    let username = body
        .username
        .as_deref()
        .map(|s| s.trim().to_ascii_lowercase())
        .unwrap_or_default();
    check_redis_user(&username).map_err(AppError::BadRequest)?;
    let pattern = body
        .key_pattern
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{username}:*"));
    check_key_pattern(&pattern).map_err(AppError::BadRequest)?;
    let db_index = body.db_index.unwrap_or(0);
    if db_index > 15 {
        return Err(AppError::bad("DB 번호는 0~15 입니다"));
    }
    let note = redis_note(body.note)?;
    let exists: Option<(u64,)> = sqlx::query_as("SELECT id FROM redis_users WHERE username = ?")
        .bind(&username)
        .fetch_optional(&state.db)
        .await?;
    if exists.is_some() {
        return Err(AppError::Conflict(format!(
            "Redis 계정 {username} 이(가) 이미 있습니다"
        )));
    }
    let pw = password_or_generate(body.password)?;
    apply_redis_user(&state, &username, &pattern, body.readonly, Some(&pw)).await?;
    let res = sqlx::query(
        "INSERT INTO redis_users (username, password, key_pattern, readonly, db_index, note) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&username)
    .bind(&pw)
    .bind(&pattern)
    .bind(body.readonly)
    .bind(db_index)
    .bind(&note)
    .execute(&state.db)
    .await?;
    audit::log(
        &state,
        Some(user.id),
        "redis.user.create",
        &format!("{username} ~{pattern}"),
        &ip,
    )
    .await;
    let u = fetch_redis_user(&state, res.last_insert_id()).await?;
    Ok((
        StatusCode::CREATED,
        Json(
            json!({ "id": u.id, "username": u.username, "password": pw, "url": redis_user_url(&state, &u) }),
        ),
    ))
}

/// PUT /api/redis-users/{id} — 키 패턴·권한·DB 번호·메모 변경 (비밀번호는 그대로)
pub async fn redis_user_update(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
    Json(body): Json<RedisUserInput>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let u = fetch_redis_user(&state, id).await?;
    let pattern = body
        .key_pattern
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or(u.key_pattern.clone());
    check_key_pattern(&pattern).map_err(AppError::BadRequest)?;
    let db_index = body.db_index.unwrap_or(u.db_index);
    if db_index > 15 {
        return Err(AppError::bad("DB 번호는 0~15 입니다"));
    }
    let note = redis_note(body.note)?;
    apply_redis_user(&state, &u.username, &pattern, body.readonly, None).await?;
    sqlx::query(
        "UPDATE redis_users SET key_pattern = ?, readonly = ?, db_index = ?, note = ? WHERE id = ?",
    )
    .bind(&pattern)
    .bind(body.readonly)
    .bind(db_index)
    .bind(&note)
    .bind(id)
    .execute(&state.db)
    .await?;
    audit::log(
        &state,
        Some(user.id),
        "redis.user.update",
        &format!("{} ~{pattern}", u.username),
        &ip,
    )
    .await;
    let u = fetch_redis_user(&state, id).await?;
    Ok(Json(
        json!({ "ok": true, "url": redis_user_url(&state, &u) }),
    ))
}

/// POST /api/redis-users/{id}/password  (body: {"password": "..."} 없으면 자동 생성)
pub async fn redis_user_password(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
    body: Option<Json<NewPassword>>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let u = fetch_redis_user(&state, id).await?;
    let pw = password_or_generate(body.and_then(|b| b.0.password))?;
    apply_redis_user(&state, &u.username, &u.key_pattern, u.readonly, Some(&pw)).await?;
    sqlx::query("UPDATE redis_users SET password = ? WHERE id = ?")
        .bind(&pw)
        .bind(id)
        .execute(&state.db)
        .await?;
    audit::log(
        &state,
        Some(user.id),
        "redis.user.password",
        &u.username,
        &ip,
    )
    .await;
    let u = fetch_redis_user(&state, id).await?;
    Ok(Json(
        json!({ "ok": true, "password": pw, "url": redis_user_url(&state, &u) }),
    ))
}

/// DELETE /api/redis-users/{id}
pub async fn redis_user_drop(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
) -> AppResult<StatusCode> {
    user.require_admin()?;
    let u = fetch_redis_user(&state, id).await?;
    let log = LogSink::capture();
    helper::call(&state, &["redis-user-drop", &u.username], &log)
        .await
        .map_err(|e| AppError::BadRequest(format!("{e:#}")))?;
    sqlx::query("DELETE FROM redis_users WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    audit::log(&state, Some(user.id), "redis.user.drop", &u.username, &ip).await;
    Ok(StatusCode::NO_CONTENT)
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
    fn account_rules() {
        assert!(check_redis_user("cache_user").is_ok());
        assert!(check_redis_user("default").is_err());
        assert!(check_redis_user("ab").is_err());
        assert!(check_redis_user("Bad").is_err());
        assert!(check_redis_user("a b c").is_err());
        assert!(check_key_pattern("myapp:*").is_ok());
        assert!(check_key_pattern("*").is_ok());
        assert!(check_key_pattern("a b").is_err());
        assert!(check_key_pattern("x;FLUSHALL").is_err());
        assert!(password_or_generate(None).unwrap().len() >= 32);
        assert_eq!(
            password_or_generate(Some("My_pass-1234".into())).unwrap(),
            "My_pass-1234"
        );
        assert!(password_or_generate(Some("short".into())).is_err());
        assert!(password_or_generate(Some("has@at-sign-123".into())).is_err());
        assert!(password_or_generate(Some("quote'1234567".into())).is_err());
    }

    #[test]
    fn redis_pick() {
        assert_eq!(pick_redis_db(4, 15, &[], 10), Some(4));
        assert_eq!(pick_redis_db(4, 15, &[4, 5], 10), Some(6));
        assert_eq!(pick_redis_db(9, 11, &[9], 10), Some(11));
        assert_eq!(pick_redis_db(10, 10, &[], 10), None);
    }
}
