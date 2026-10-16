//! 데이터베이스 웹 관리 (MariaDB) — 테이블 보기, 행 추가·수정·삭제, SQL 실행
//!
//! 두 가지 입구가 있습니다.
//! - 패널 관리자: `/api/dbweb/admin/{db_id}/…` — 패널이 만든 DB 를 그 DB 의 기본 계정으로 접속
//! - DB 계정 로그인: `/api/dbweb/login` 로 DB 계정 이름·비밀번호를 확인하고 세션 토큰을 받아
//!   `/api/dbweb/s/{database}/…` (헤더 `X-DB-Token`) — 그 계정이 볼 수 있는 DB 만
//!
//! 어느 쪽이든 실제 질의는 그 DB 계정 권한으로만 실행됩니다 (root·패널 계정은 쓰지 않음).

use std::time::{Duration, Instant};

use axum::{
    extract::{FromRequestParts, Path, Query, State},
    http::{request::Parts, StatusCode},
    Json,
};
use futures::TryStreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sqlx::{
    mysql::{MySqlConnectOptions, MySqlRow},
    Column, ConnectOptions, Connection, Either, Executor, MySqlConnection, Row, TypeInfo, ValueRef,
};

use crate::{
    audit,
    auth::{session, AuthUser, ClientIp},
    error::{AppError, AppResult},
    models::{AppDatabase, DB_COLUMNS},
    state::AppState,
};

const SYSTEM_DBS: &[&str] = &["information_schema", "performance_schema", "mysql", "sys"];
/// 패널 자신과 root 계정은 이 화면으로 로그인할 수 없음
const BLOCKED_USERS: &[&str] = &["root", "rpanel", "mariadb.sys", "mysql", "debian-sys-maint"];
const MAX_ROWS: usize = 1000;
const SESSION_SECS: u64 = 2 * 3600;

/// 접속 정보 (DB 계정)
#[derive(Clone, Serialize, Deserialize)]
pub struct Creds {
    pub username: String,
    pub password: String,
    /// 볼 수 있는 DB
    pub databases: Vec<String>,
}

fn host_port(state: &AppState) -> (String, u16) {
    let h = &state.cfg.app_db_host;
    match h.rsplit_once(':') {
        Some((host, port)) => (host.to_string(), port.parse().unwrap_or(3306)),
        None => (h.clone(), 3306),
    }
}

async fn connect(state: &AppState, c: &Creds, db: Option<&str>) -> AppResult<MySqlConnection> {
    let (host, port) = host_port(state);
    let mut opts = MySqlConnectOptions::new()
        .host(&host)
        .port(port)
        .username(&c.username)
        .password(&c.password)
        .charset("utf8mb4")
        .timezone(None)
        .disable_statement_logging();
    if let Some(d) = db {
        opts = opts.database(d);
    }
    let mut conn = tokio::time::timeout(Duration::from_secs(10), opts.connect())
        .await
        .map_err(|_| AppError::bad("MariaDB 에 접속하지 못했습니다 (시간 초과)"))?
        .map_err(|e| {
            let m = e.to_string();
            if m.contains("1045") || m.contains("Access denied") {
                AppError::bad("계정 이름 또는 비밀번호가 올바르지 않습니다")
            } else {
                AppError::bad(format!("MariaDB 접속 실패: {m}"))
            }
        })?;
    // 한 질의는 최대 30초
    let _ = conn.execute("SET SESSION max_statement_time = 30").await;
    Ok(conn)
}

/// `식별자` (백틱 이스케이프)
fn ident(s: &str) -> String {
    format!("`{}`", s.replace('`', "``"))
}

/// 결과 값 하나 → JSON (형식을 모르면 글자로)
fn cell(row: &MySqlRow, i: usize) -> Value {
    let Ok(raw) = row.try_get_raw(i) else {
        return Value::Null;
    };
    if raw.is_null() {
        return Value::Null;
    }
    let ty = row.column(i).type_info().name().to_ascii_uppercase();
    let bytes_to_value = |b: Vec<u8>| match String::from_utf8(b) {
        Ok(s) => Value::String(s),
        Err(e) => Value::String(format!("(binary {} bytes)", e.as_bytes().len())),
    };
    if ty.contains("UNSIGNED") && ty.contains("INT") {
        return row
            .try_get::<u64, _>(i)
            .map(Value::from)
            .unwrap_or(Value::Null);
    }
    match ty.as_str() {
        "BOOLEAN" => row
            .try_get::<bool, _>(i)
            .map(Value::from)
            .or_else(|_| row.try_get::<i8, _>(i).map(Value::from))
            .unwrap_or(Value::Null),
        "TINYINT" | "SMALLINT" | "MEDIUMINT" | "INT" | "BIGINT" => row
            .try_get::<i64, _>(i)
            .map(Value::from)
            .unwrap_or(Value::Null),
        "FLOAT" => row
            .try_get::<f32, _>(i)
            .map(|v| json!(v))
            .unwrap_or(Value::Null),
        "DOUBLE" => row
            .try_get::<f64, _>(i)
            .map(|v| json!(v))
            .unwrap_or(Value::Null),
        "DATE" => row
            .try_get::<chrono::NaiveDate, _>(i)
            .map(|v| Value::String(v.to_string()))
            .unwrap_or(Value::Null),
        "DATETIME" | "TIMESTAMP" => row
            .try_get::<chrono::NaiveDateTime, _>(i)
            .map(|v| {
                Value::String(
                    v.format("%Y-%m-%d %H:%M:%S%.f")
                        .to_string()
                        .trim_end_matches(".")
                        .to_string(),
                )
            })
            .unwrap_or(Value::Null),
        "TIME" => row
            .try_get::<chrono::NaiveTime, _>(i)
            .map(|v| Value::String(v.to_string()))
            .or_else(|_| row.try_get_unchecked::<Vec<u8>, _>(i).map(bytes_to_value))
            .unwrap_or(Value::Null),
        _ => row
            .try_get_unchecked::<Vec<u8>, _>(i)
            .map(bytes_to_value)
            .unwrap_or(Value::Null),
    }
}

fn columns_of(row: &MySqlRow) -> Vec<String> {
    row.columns().iter().map(|c| c.name().to_string()).collect()
}

async fn table_names(conn: &mut MySqlConnection) -> AppResult<Vec<String>> {
    let rows: Vec<MySqlRow> = sqlx::query("SHOW FULL TABLES")
        .fetch_all(&mut *conn)
        .await?;
    Ok(rows
        .iter()
        .filter_map(|r| r.try_get_unchecked::<String, _>(0).ok())
        .collect())
}

async fn ensure_table(conn: &mut MySqlConnection, table: &str) -> AppResult<()> {
    if table_names(conn).await?.iter().any(|t| t == table) {
        Ok(())
    } else {
        Err(AppError::NotFound)
    }
}

// ───────────── 공통 동작 ─────────────

async fn do_tables(state: &AppState, c: &Creds, db: &str) -> AppResult<Value> {
    let mut conn = connect(state, c, Some(db)).await?;
    let rows: Vec<MySqlRow> = sqlx::query(
        "SELECT TABLE_NAME, TABLE_TYPE, ENGINE, TABLE_ROWS, DATA_LENGTH + INDEX_LENGTH, TABLE_COMMENT \
         FROM information_schema.TABLES WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME",
    )
    .bind(db)
    .fetch_all(&mut conn)
    .await?;
    let tables: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "name": cell(r, 0), "type": cell(r, 1), "engine": cell(r, 2),
                "rows": cell(r, 3), "size": cell(r, 4), "comment": cell(r, 5),
            })
        })
        .collect();
    let _ = conn.close().await;
    Ok(json!({ "database": db, "user": c.username, "tables": tables }))
}

#[derive(Deserialize)]
pub struct BrowseQuery {
    pub page: Option<u64>,
    pub size: Option<u64>,
    pub order: Option<String>,
    pub desc: Option<bool>,
}

async fn do_browse(
    state: &AppState,
    c: &Creds,
    db: &str,
    table: &str,
    q: &BrowseQuery,
) -> AppResult<Value> {
    let mut conn = connect(state, c, Some(db)).await?;
    ensure_table(&mut conn, table).await?;
    let cols: Vec<MySqlRow> = sqlx::query(&format!("SHOW FULL COLUMNS FROM {}", ident(table)))
        .fetch_all(&mut conn)
        .await?;
    let columns: Vec<Value> = cols
        .iter()
        .map(|r| {
            json!({
                "name": cell(r, 0), "type": cell(r, 1), "nullable": cell(r, 3) == "YES",
                "key": cell(r, 4), "default": cell(r, 5), "extra": cell(r, 6), "comment": cell(r, 8),
            })
        })
        .collect();
    let names: Vec<String> = columns
        .iter()
        .filter_map(|c| c["name"].as_str().map(String::from))
        .collect();
    let pk: Vec<String> = columns
        .iter()
        .filter(|c| c["key"] == "PRI")
        .filter_map(|c| c["name"].as_str().map(String::from))
        .collect();
    let size = q.size.unwrap_or(50).clamp(1, 500);
    let page = q.page.unwrap_or(1).max(1);
    let order = q
        .order
        .as_deref()
        .filter(|o| names.iter().any(|n| n == o))
        .map(String::from)
        .or_else(|| pk.first().cloned());
    let order_sql = match &order {
        Some(o) => format!(
            " ORDER BY {} {}",
            ident(o),
            if q.desc.unwrap_or(false) {
                "DESC"
            } else {
                "ASC"
            }
        ),
        None => String::new(),
    };
    let total: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {}", ident(table)))
        .fetch_one(&mut conn)
        .await?;
    let rows: Vec<MySqlRow> = sqlx::query(&format!(
        "SELECT * FROM {}{order_sql} LIMIT {size} OFFSET {}",
        ident(table),
        (page - 1) * size
    ))
    .fetch_all(&mut conn)
    .await?;
    let data: Vec<Vec<Value>> = rows
        .iter()
        .map(|r| (0..r.columns().len()).map(|i| cell(r, i)).collect())
        .collect();
    let _ = conn.close().await;
    Ok(json!({
        "table": table, "columns": columns, "primary_key": pk, "rows": data,
        "total": total, "page": page, "size": size, "order": order, "desc": q.desc.unwrap_or(false),
    }))
}

#[derive(Deserialize)]
pub struct RowInput {
    /// 수정·삭제할 행의 기본 키 값
    #[serde(default)]
    pub key: Map<String, Value>,
    /// 넣거나 바꿀 값 (null = NULL)
    #[serde(default)]
    pub values: Map<String, Value>,
}

fn as_param(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(if *b { "1".into() } else { "0".into() }),
        other => Some(other.to_string()),
    }
}

async fn known_columns(
    conn: &mut MySqlConnection,
    table: &str,
) -> AppResult<(Vec<String>, Vec<String>)> {
    let cols: Vec<MySqlRow> = sqlx::query(&format!("SHOW COLUMNS FROM {}", ident(table)))
        .fetch_all(&mut *conn)
        .await?;
    let names = cols
        .iter()
        .filter_map(|r| r.try_get_unchecked::<String, _>(0).ok())
        .collect();
    let pk = cols
        .iter()
        .filter(|r| r.try_get_unchecked::<String, _>(3).ok().as_deref() == Some("PRI"))
        .filter_map(|r| r.try_get_unchecked::<String, _>(0).ok())
        .collect();
    Ok((names, pk))
}

fn check_cols<'a>(
    names: &[String],
    m: &'a Map<String, Value>,
) -> AppResult<Vec<(&'a String, &'a Value)>> {
    let mut out = Vec::new();
    for (k, v) in m {
        if !names.contains(k) {
            return Err(AppError::bad(format!("없는 컬럼입니다: {k}")));
        }
        out.push((k, v));
    }
    Ok(out)
}

/// 기본 키로 정확히 한 행을 가리키는 WHERE
fn where_pk<'a>(pk: &[String], key: &'a Map<String, Value>) -> AppResult<(String, Vec<&'a Value>)> {
    if pk.is_empty() {
        return Err(AppError::bad("기본 키(PRIMARY KEY)가 없는 테이블은 화면에서 수정·삭제할 수 없습니다 — SQL 실행을 쓰세요"));
    }
    let mut parts = Vec::new();
    let mut vals = Vec::new();
    for k in pk {
        let v = key
            .get(k)
            .ok_or_else(|| AppError::bad(format!("기본 키 값이 없습니다: {k}")))?;
        parts.push(format!("{} <=> ?", ident(k)));
        vals.push(v);
    }
    Ok((parts.join(" AND "), vals))
}

async fn do_row(
    state: &AppState,
    c: &Creds,
    db: &str,
    table: &str,
    op: &str,
    input: &RowInput,
) -> AppResult<Value> {
    let mut conn = connect(state, c, Some(db)).await?;
    ensure_table(&mut conn, table).await?;
    let (names, pk) = known_columns(&mut conn, table).await?;
    let res = match op {
        "insert" => {
            let vals = check_cols(&names, &input.values)?;
            if vals.is_empty() {
                sqlx::query(&format!("INSERT INTO {} () VALUES ()", ident(table)))
                    .execute(&mut conn)
                    .await?
            } else {
                let cols = vals
                    .iter()
                    .map(|(k, _)| ident(k))
                    .collect::<Vec<_>>()
                    .join(", ");
                let marks = vec!["?"; vals.len()].join(", ");
                let sql = format!("INSERT INTO {} ({cols}) VALUES ({marks})", ident(table));
                let mut q = sqlx::query(&sql);
                for (_, v) in &vals {
                    q = q.bind(as_param(v));
                }
                q.execute(&mut conn).await?
            }
        }
        "update" => {
            let vals = check_cols(&names, &input.values)?;
            if vals.is_empty() {
                return Err(AppError::bad("바꿀 값이 없습니다"));
            }
            let (w, kv) = where_pk(&pk, &input.key)?;
            let set = vals
                .iter()
                .map(|(k, _)| format!("{} = ?", ident(k)))
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!("UPDATE {} SET {set} WHERE {w} LIMIT 1", ident(table));
            let mut q = sqlx::query(&sql);
            for (_, v) in &vals {
                q = q.bind(as_param(v));
            }
            for v in kv {
                q = q.bind(as_param(v));
            }
            q.execute(&mut conn).await?
        }
        "delete" => {
            let (w, kv) = where_pk(&pk, &input.key)?;
            let sql = format!("DELETE FROM {} WHERE {w} LIMIT 1", ident(table));
            let mut q = sqlx::query(&sql);
            for v in kv {
                q = q.bind(as_param(v));
            }
            q.execute(&mut conn).await?
        }
        _ => return Err(AppError::NotFound),
    };
    let _ = conn.close().await;
    Ok(json!({ "affected": res.rows_affected(), "last_insert_id": res.last_insert_id() }))
}

#[derive(Deserialize)]
pub struct QueryInput {
    pub sql: String,
}

#[allow(deprecated)]
async fn do_query(state: &AppState, c: &Creds, db: &str, sql: &str) -> AppResult<Value> {
    let sql = sql.trim();
    if sql.is_empty() {
        return Err(AppError::bad("SQL 을 입력하세요"));
    }
    if sql.len() > 100_000 {
        return Err(AppError::bad("SQL 이 너무 깁니다"));
    }
    let mut conn = connect(state, c, Some(db)).await?;
    let started = Instant::now();
    let mut columns: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<Value>> = Vec::new();
    let mut affected = 0u64;
    let mut truncated = false;
    let run = async {
        // 대부분은 prepared 로, 지원하지 않는 문장(1295)은 텍스트 프로토콜로
        let first = {
            let mut s = sqlx::query(sql).fetch_many(&mut conn);
            let mut err = None;
            loop {
                match s.try_next().await {
                    Ok(Some(Either::Left(r))) => affected += r.rows_affected(),
                    Ok(Some(Either::Right(row))) => {
                        if columns.is_empty() {
                            columns = columns_of(&row);
                        }
                        if rows.len() < MAX_ROWS {
                            rows.push((0..row.columns().len()).map(|i| cell(&row, i)).collect());
                        } else {
                            truncated = true;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        err = Some(e);
                        break;
                    }
                }
            }
            err
        };
        match first {
            Some(e) if e.to_string().contains("1295") => {
                let mut s = sqlx::raw_sql(sql).fetch_many(&mut conn);
                while let Some(item) = s.try_next().await? {
                    match item {
                        Either::Left(r) => affected += r.rows_affected(),
                        Either::Right(row) => {
                            if columns.is_empty() {
                                columns = columns_of(&row);
                            }
                            if rows.len() < MAX_ROWS {
                                rows.push(
                                    (0..row.columns().len()).map(|i| cell(&row, i)).collect(),
                                );
                            } else {
                                truncated = true;
                            }
                        }
                    }
                }
                Ok::<(), sqlx::Error>(())
            }
            Some(e) => Err(e),
            None => Ok(()),
        }
    };
    let res = tokio::time::timeout(Duration::from_secs(35), run).await;
    let elapsed = started.elapsed().as_millis() as u64;
    match res {
        Err(_) => return Err(AppError::bad("30초 안에 끝나지 않아 멈췄습니다")),
        Ok(Err(e)) => {
            let msg = match &e {
                sqlx::Error::Database(d) => d.message().to_string(),
                other => other.to_string(),
            };
            return Ok(Json(json!({ "error": msg, "elapsed_ms": elapsed })).0);
        }
        Ok(Ok(())) => {}
    }
    let _ = conn.close().await;
    Ok(
        json!({ "columns": columns, "rows": rows, "affected": affected, "truncated": truncated, "elapsed_ms": elapsed }),
    )
}

// ───────────── 패널 관리자 입구 ─────────────

async fn admin_creds(state: &AppState, db_id: u64) -> AppResult<(Creds, String)> {
    let db = sqlx::query_as::<_, AppDatabase>(&format!(
        "SELECT {DB_COLUMNS} FROM app_databases WHERE id = ?"
    ))
    .bind(db_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok((
        Creds {
            username: db.username,
            password: db.password,
            databases: vec![db.name.clone()],
        },
        db.name,
    ))
}

pub async fn admin_tables(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<u64>,
) -> AppResult<Json<Value>> {
    user.require_admin()?;
    let (c, db) = admin_creds(&state, id).await?;
    Ok(Json(do_tables(&state, &c, &db).await?))
}

pub async fn admin_browse(
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, table)): Path<(u64, String)>,
    Query(q): Query<BrowseQuery>,
) -> AppResult<Json<Value>> {
    user.require_admin()?;
    let (c, db) = admin_creds(&state, id).await?;
    Ok(Json(do_browse(&state, &c, &db, &table, &q).await?))
}

pub async fn admin_row(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path((id, table, op)): Path<(u64, String, String)>,
    Json(input): Json<RowInput>,
) -> AppResult<Json<Value>> {
    user.require_admin()?;
    let (c, db) = admin_creds(&state, id).await?;
    let r = do_row(&state, &c, &db, &table, &op, &input).await?;
    audit::log(
        &state,
        Some(user.id),
        &format!("dbweb.{op}"),
        &format!("{db}.{table}"),
        &ip,
    )
    .await;
    Ok(Json(r))
}

pub async fn admin_query(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
    Json(input): Json<QueryInput>,
) -> AppResult<Json<Value>> {
    user.require_admin()?;
    let (c, db) = admin_creds(&state, id).await?;
    audit::log(&state, Some(user.id), "dbweb.query", &db, &ip).await;
    Ok(Json(do_query(&state, &c, &db, &input.sql).await?))
}

// ───────────── DB 계정 로그인 입구 ─────────────

fn session_key(state: &AppState, token: &str) -> String {
    state.key(&["dbweb", "session", token])
}

/// 헤더 `X-DB-Token` 으로 찾은 DB 계정 세션
pub struct DbSession {
    pub token: String,
    pub creds: Creds,
}

impl FromRequestParts<AppState> for DbSession {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get("x-db-token")
            .and_then(|v| v.to_str().ok())
            .filter(|t| t.len() >= 32 && t.chars().all(|c| c.is_ascii_hexdigit()))
            .ok_or(AppError::Unauthorized)?
            .to_string();
        let mut r = state.redis.clone();
        let key = session_key(state, &token);
        let raw: Option<String> = redis::cmd("GET").arg(&key).query_async(&mut r).await?;
        let creds: Creds = raw
            .and_then(|s| serde_json::from_str(&s).ok())
            .ok_or(AppError::Unauthorized)?;
        let _: () = redis::cmd("EXPIRE")
            .arg(&key)
            .arg(SESSION_SECS)
            .query_async(&mut r)
            .await?;
        Ok(Self { token, creds })
    }
}

fn session_db(s: &DbSession, db: &str) -> AppResult<()> {
    if s.creds.databases.iter().any(|d| d == db) {
        Ok(())
    } else {
        Err(AppError::Forbidden(
            "이 계정으로 볼 수 없는 데이터베이스입니다".into(),
        ))
    }
}

#[derive(Deserialize)]
pub struct LoginInput {
    pub username: String,
    pub password: String,
}

/// POST /api/dbweb/login — DB 계정 이름·비밀번호로 로그인 (패널 계정 필요 없음)
pub async fn login(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Json(input): Json<LoginInput>,
) -> AppResult<Json<Value>> {
    let username = input.username.trim().to_string();
    if !session::rate_limit(&state, &format!("dbweb:ip:{ip}"), 10, 300).await?
        || !session::rate_limit(&state, &format!("dbweb:user:{username}"), 10, 300).await?
    {
        return Err(AppError::TooManyRequests);
    }
    if username.is_empty()
        || username.len() > 80
        || input.password.is_empty()
        || BLOCKED_USERS.contains(&username.as_str())
    {
        return Err(AppError::bad("계정 이름 또는 비밀번호가 올바르지 않습니다"));
    }
    let mut creds = Creds {
        username: username.clone(),
        password: input.password,
        databases: vec![],
    };
    let mut conn = match connect(&state, &creds, None).await {
        Ok(c) => c,
        Err(e) => {
            audit::log(&state, None, "dbweb.login_failed", &username, &ip).await;
            return Err(e);
        }
    };
    let rows: Vec<MySqlRow> = sqlx::query("SHOW DATABASES").fetch_all(&mut conn).await?;
    let _ = conn.close().await;
    creds.databases = rows
        .iter()
        .filter_map(|r| r.try_get_unchecked::<String, _>(0).ok())
        .filter(|d| !SYSTEM_DBS.contains(&d.as_str()))
        .collect();
    let panel_db = state
        .cfg
        .database_url
        .rsplit('/')
        .next()
        .unwrap_or("")
        .split('?')
        .next()
        .unwrap_or("")
        .to_string();
    creds.databases.retain(|d| d != &panel_db);
    if creds.databases.is_empty() {
        return Err(AppError::bad(
            "이 계정으로 볼 수 있는 데이터베이스가 없습니다",
        ));
    }
    let token = session::random_token(24);
    let mut r = state.redis.clone();
    let _: () = redis::cmd("SET")
        .arg(session_key(&state, &token))
        .arg(serde_json::to_string(&creds).map_err(|e| AppError::Internal(e.into()))?)
        .arg("EX")
        .arg(SESSION_SECS)
        .query_async(&mut r)
        .await?;
    audit::log(&state, None, "dbweb.login", &username, &ip).await;
    Ok(Json(
        json!({ "token": token, "username": username, "databases": creds.databases }),
    ))
}

/// POST /api/dbweb/logout
pub async fn logout(State(state): State<AppState>, s: DbSession) -> AppResult<StatusCode> {
    let mut r = state.redis.clone();
    let _: () = redis::cmd("DEL")
        .arg(session_key(&state, &s.token))
        .query_async(&mut r)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/dbweb/me
pub async fn me(s: DbSession) -> Json<Value> {
    Json(json!({ "username": s.creds.username, "databases": s.creds.databases }))
}

pub async fn s_tables(
    State(state): State<AppState>,
    s: DbSession,
    Path(db): Path<String>,
) -> AppResult<Json<Value>> {
    session_db(&s, &db)?;
    Ok(Json(do_tables(&state, &s.creds, &db).await?))
}

pub async fn s_browse(
    State(state): State<AppState>,
    s: DbSession,
    Path((db, table)): Path<(String, String)>,
    Query(q): Query<BrowseQuery>,
) -> AppResult<Json<Value>> {
    session_db(&s, &db)?;
    Ok(Json(do_browse(&state, &s.creds, &db, &table, &q).await?))
}

pub async fn s_row(
    State(state): State<AppState>,
    s: DbSession,
    Path((db, table, op)): Path<(String, String, String)>,
    Json(input): Json<RowInput>,
) -> AppResult<Json<Value>> {
    session_db(&s, &db)?;
    Ok(Json(
        do_row(&state, &s.creds, &db, &table, &op, &input).await?,
    ))
}

pub async fn s_query(
    State(state): State<AppState>,
    s: DbSession,
    Path(db): Path<String>,
    Json(input): Json<QueryInput>,
) -> AppResult<Json<Value>> {
    session_db(&s, &db)?;
    Ok(Json(do_query(&state, &s.creds, &db, &input.sql).await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idents_and_params() {
        assert_eq!(ident("orders"), "`orders`");
        assert_eq!(ident("a`b"), "`a``b`");
        assert_eq!(as_param(&Value::Null), None);
        assert_eq!(as_param(&json!(true)).as_deref(), Some("1"));
        assert_eq!(as_param(&json!(12)).as_deref(), Some("12"));
        assert_eq!(as_param(&json!("x")).as_deref(), Some("x"));
    }

    #[test]
    fn pk_where() {
        let pk = vec!["id".to_string(), "k".to_string()];
        let mut key = Map::new();
        key.insert("id".into(), json!(3));
        assert!(where_pk(&pk, &key).is_err(), "all pk columns required");
        key.insert("k".into(), Value::Null);
        let (w, vals) = where_pk(&pk, &key).unwrap();
        assert_eq!(w, "`id` <=> ? AND `k` <=> ?");
        assert_eq!(vals.len(), 2);
        assert!(where_pk(&[], &key).is_err());
    }
}
