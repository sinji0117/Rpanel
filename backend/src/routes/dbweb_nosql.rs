//! DB 웹 관리 — MongoDB(문서 보기·추가·수정·삭제) · Redis(키 보기·문자열 값 수정·삭제)
//!
//! - MongoDB: 패널이 만든 DB 를 **그 DB 의 기본 계정**으로 접속 (다른 DB 는 못 봄)
//! - Redis: 앱에 배정한 DB 번호만 (패널이 쓰는 DB 는 열지 않음)

use std::time::Duration;

use axum::{
    extract::{Path, Query, State},
    Json,
};
use futures::TryStreamExt;
use mongodb::{
    bson::{doc, Bson, Document},
    options::{ClientOptions, FindOptions},
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    audit,
    auth::{AuthUser, ClientIp},
    error::{AppError, AppResult},
    state::AppState,
};

const MAX_DOC_BYTES: usize = 1024 * 1024;

fn bad(e: impl std::fmt::Display) -> AppError {
    AppError::bad(e.to_string())
}

// ───────────── MongoDB ─────────────

struct MongoTarget {
    name: String,
    db: mongodb::Database,
}

async fn mongo_target(state: &AppState, id: u64) -> AppResult<MongoTarget> {
    let row: Option<(String, String, String)> =
        sqlx::query_as("SELECT name, username, password FROM mongo_databases WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    let (name, user, pw) = row.ok_or(AppError::NotFound)?;
    if state.mongo.is_none() {
        return Err(AppError::bad("MongoDB 가 설정되지 않았습니다"));
    }
    let url = crate::mongo::app_url(state, &name, &user, &pw);
    let mut opts = ClientOptions::parse(&url).await.map_err(bad)?;
    opts.server_selection_timeout = Some(Duration::from_secs(5));
    opts.connect_timeout = Some(Duration::from_secs(5));
    let client = mongodb::Client::with_options(opts).map_err(bad)?;
    Ok(MongoTarget {
        db: client.database(&name),
        name,
    })
}

fn valid_collection(c: &str) -> AppResult<()> {
    if c.is_empty()
        || c.len() > 120
        || c.starts_with("system.")
        || c.contains('$')
        || c.contains('\0')
    {
        return Err(AppError::bad("컬렉션 이름이 올바르지 않습니다"));
    }
    Ok(())
}

fn to_json(d: Document) -> Value {
    Bson::Document(d).into_relaxed_extjson()
}

fn from_json(v: Value) -> AppResult<Document> {
    if v.to_string().len() > MAX_DOC_BYTES {
        return Err(AppError::bad("문서가 너무 큽니다 (1MB 까지)"));
    }
    match Bson::try_from(v)
        .map_err(|e| AppError::bad(format!("JSON 을 BSON 으로 바꾸지 못했습니다: {e}")))?
    {
        Bson::Document(d) => Ok(d),
        _ => Err(AppError::bad("문서는 { … } 형태의 JSON 이어야 합니다")),
    }
}

fn mongo_err(e: mongodb::error::Error) -> AppError {
    AppError::bad(format!("MongoDB: {e}"))
}

/// GET /api/dbweb/mongo/{id}
pub async fn mongo_collections(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<u64>,
) -> AppResult<Json<Value>> {
    user.require_admin()?;
    let t = mongo_target(&state, id).await?;
    let mut names = t.db.list_collection_names().await.map_err(mongo_err)?;
    names.retain(|n| !n.starts_with("system."));
    names.sort();
    let mut out = Vec::new();
    for n in names {
        let count =
            t.db.collection::<Document>(&n)
                .estimated_document_count()
                .await
                .unwrap_or(0);
        out.push(json!({ "name": n, "count": count }));
    }
    Ok(Json(json!({ "database": t.name, "collections": out })))
}

#[derive(Deserialize)]
pub struct FindQuery {
    pub filter: Option<String>,
    pub page: Option<u64>,
    pub size: Option<u64>,
}

/// GET /api/dbweb/mongo/{id}/c/{coll}?filter={json}&page=&size=
pub async fn mongo_find(
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, coll)): Path<(u64, String)>,
    Query(q): Query<FindQuery>,
) -> AppResult<Json<Value>> {
    user.require_admin()?;
    valid_collection(&coll)?;
    let t = mongo_target(&state, id).await?;
    let filter = match q.filter.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(f) => from_json(
            serde_json::from_str(f).map_err(|e| AppError::bad(format!("필터 JSON 오류: {e}")))?,
        )?,
        None => doc! {},
    };
    let size = q.size.unwrap_or(20).clamp(1, 200);
    let page = q.page.unwrap_or(1).max(1);
    let c = t.db.collection::<Document>(&coll);
    let total = c.count_documents(filter.clone()).await.map_err(mongo_err)?;
    let docs: Vec<Document> = c
        .find(filter)
        .with_options(
            FindOptions::builder()
                .sort(doc! { "_id": -1 })
                .skip((page - 1) * size)
                .limit(size as i64)
                .build(),
        )
        .await
        .map_err(mongo_err)?
        .try_collect()
        .await
        .map_err(mongo_err)?;
    Ok(Json(json!({
        "collection": coll, "total": total, "page": page, "size": size,
        "docs": docs.into_iter().map(to_json).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct DocInput {
    /// 수정·삭제할 문서의 _id (확장 JSON, 예: {"$oid": "…"})
    #[serde(default)]
    pub id: Option<Value>,
    #[serde(default)]
    pub doc: Option<Value>,
}

/// POST /api/dbweb/mongo/{id}/c/{coll}/{insert|update|delete}
pub async fn mongo_write(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path((id, coll, op)): Path<(u64, String, String)>,
    Json(input): Json<DocInput>,
) -> AppResult<Json<Value>> {
    user.require_admin()?;
    valid_collection(&coll)?;
    let t = mongo_target(&state, id).await?;
    let c = t.db.collection::<Document>(&coll);
    let key = || -> AppResult<Bson> {
        let v = input
            .id
            .clone()
            .ok_or_else(|| AppError::bad("_id 가 필요합니다"))?;
        Bson::try_from(v).map_err(|e| AppError::bad(format!("_id 오류: {e}")))
    };
    let out = match op.as_str() {
        "insert" => {
            let d = from_json(
                input
                    .doc
                    .clone()
                    .ok_or_else(|| AppError::bad("문서가 필요합니다"))?,
            )?;
            let r = c.insert_one(d).await.map_err(mongo_err)?;
            json!({ "inserted_id": r.inserted_id.into_relaxed_extjson() })
        }
        "update" => {
            let mut d = from_json(
                input
                    .doc
                    .clone()
                    .ok_or_else(|| AppError::bad("문서가 필요합니다"))?,
            )?;
            let k = key()?;
            d.remove("_id");
            let r = c
                .replace_one(doc! { "_id": k }, d)
                .await
                .map_err(mongo_err)?;
            json!({ "matched": r.matched_count, "modified": r.modified_count })
        }
        "delete" => {
            let r = c
                .delete_one(doc! { "_id": key()? })
                .await
                .map_err(mongo_err)?;
            json!({ "deleted": r.deleted_count })
        }
        _ => return Err(AppError::NotFound),
    };
    audit::log(
        &state,
        Some(user.id),
        &format!("dbweb.mongo.{op}"),
        &format!("{}.{coll}", t.name),
        &ip,
    )
    .await;
    Ok(Json(out))
}

#[derive(Deserialize)]
pub struct NewCollection {
    pub name: String,
}

/// POST /api/dbweb/mongo/{id}/collections
pub async fn mongo_create_collection(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<u64>,
    Json(input): Json<NewCollection>,
) -> AppResult<Json<Value>> {
    user.require_admin()?;
    let name = input.name.trim().to_string();
    valid_collection(&name)?;
    let t = mongo_target(&state, id).await?;
    t.db.create_collection(&name).await.map_err(mongo_err)?;
    Ok(Json(json!({ "name": name })))
}

// ───────────── Redis ─────────────

/// 웹 관리로 열 수 있는 DB 번호 (앱에 배정한 것만)
async fn redis_allowed(state: &AppState) -> AppResult<Vec<(u32, String)>> {
    let rows: Vec<(u32, String)> = sqlx::query_as(
        "SELECT redis_db, name FROM apps WHERE redis_db IS NOT NULL ORDER BY redis_db",
    )
    .fetch_all(&state.db)
    .await?;
    let panel_db = crate::config::redis_db_index(&state.cfg.redis_url);
    Ok(rows.into_iter().filter(|(d, _)| *d != panel_db).collect())
}

async fn redis_conn(state: &AppState, db: u32) -> AppResult<redis::aio::MultiplexedConnection> {
    if !redis_allowed(state).await?.iter().any(|(d, _)| *d == db) {
        return Err(AppError::Forbidden(
            "앱에 배정한 Redis DB 만 열 수 있습니다".into(),
        ));
    }
    let client = redis::Client::open(state.cfg.redis_url.as_str()).map_err(bad)?;
    let mut c = client.get_multiplexed_async_connection().await?;
    let _: () = redis::cmd("SELECT").arg(db).query_async(&mut c).await?;
    Ok(c)
}

/// GET /api/dbweb/redis
pub async fn redis_dbs(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<Value>> {
    user.require_admin()?;
    let mut out = Vec::new();
    for (db, app) in redis_allowed(&state).await? {
        let mut c = redis_conn(&state, db).await?;
        let keys: i64 = redis::cmd("DBSIZE").query_async(&mut c).await.unwrap_or(0);
        out.push(json!({ "db": db, "app": app, "keys": keys }));
    }
    Ok(Json(json!({ "databases": out })))
}

#[derive(Deserialize)]
pub struct ScanQuery {
    pub pattern: Option<String>,
    pub cursor: Option<u64>,
}

/// GET /api/dbweb/redis/{db}/keys?pattern=&cursor=
pub async fn redis_keys(
    State(state): State<AppState>,
    user: AuthUser,
    Path(db): Path<u32>,
    Query(q): Query<ScanQuery>,
) -> AppResult<Json<Value>> {
    user.require_admin()?;
    let mut c = redis_conn(&state, db).await?;
    let pattern = q
        .pattern
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| "*".into());
    if pattern.len() > 200 {
        return Err(AppError::bad("패턴이 너무 깁니다"));
    }
    // 한 번에 최대 약 200개 (SCAN 은 COUNT 보다 적게 줄 수 있어 몇 번 이어서)
    let mut cursor = q.cursor.unwrap_or(0);
    let mut keys: Vec<String> = Vec::new();
    for _ in 0..10 {
        let (next, mut batch): (u64, Vec<String>) = redis::cmd("SCAN")
            .arg(cursor)
            .arg("MATCH")
            .arg(&pattern)
            .arg("COUNT")
            .arg(200)
            .query_async(&mut c)
            .await?;
        keys.append(&mut batch);
        cursor = next;
        if cursor == 0 || keys.len() >= 200 {
            break;
        }
    }
    let mut pipe = redis::pipe();
    for k in &keys {
        pipe.cmd("TYPE").arg(k).cmd("PTTL").arg(k);
    }
    let meta: Vec<redis::Value> = if keys.is_empty() {
        vec![]
    } else {
        pipe.query_async(&mut c).await?
    };
    let items: Vec<Value> = keys
        .iter()
        .enumerate()
        .map(|(i, k)| {
            let ty = match meta.get(i * 2) {
                Some(redis::Value::SimpleString(s)) => s.clone(),
                Some(redis::Value::BulkString(b)) => String::from_utf8_lossy(b).into_owned(),
                _ => "?".into(),
            };
            let ttl = match meta.get(i * 2 + 1) {
                Some(redis::Value::Int(n)) if *n >= 0 => json!(n / 1000),
                _ => Value::Null,
            };
            json!({ "key": k, "type": ty, "ttl": ttl })
        })
        .collect();
    Ok(Json(json!({ "db": db, "keys": items, "cursor": cursor })))
}

#[derive(Deserialize)]
pub struct KeyQuery {
    pub k: String,
}

/// GET /api/dbweb/redis/{db}/key?k=
pub async fn redis_get(
    State(state): State<AppState>,
    user: AuthUser,
    Path(db): Path<u32>,
    Query(q): Query<KeyQuery>,
) -> AppResult<Json<Value>> {
    user.require_admin()?;
    let mut c = redis_conn(&state, db).await?;
    let k = q.k;
    let ty: String = redis::cmd("TYPE").arg(&k).query_async(&mut c).await?;
    let ttl: i64 = redis::cmd("TTL").arg(&k).query_async(&mut c).await?;
    let value = match ty.as_str() {
        "string" => {
            let v: Vec<u8> = redis::cmd("GETRANGE")
                .arg(&k)
                .arg(0)
                .arg(256 * 1024)
                .query_async(&mut c)
                .await?;
            json!(String::from_utf8_lossy(&v))
        }
        "list" => {
            let v: Vec<String> = redis::cmd("LRANGE")
                .arg(&k)
                .arg(0)
                .arg(199)
                .query_async(&mut c)
                .await?;
            json!(v)
        }
        "set" => {
            let v: Vec<String> = redis::cmd("SRANDMEMBER")
                .arg(&k)
                .arg(200)
                .query_async(&mut c)
                .await?;
            json!(v)
        }
        "zset" => {
            let v: Vec<(String, f64)> = redis::cmd("ZRANGE")
                .arg(&k)
                .arg(0)
                .arg(199)
                .arg("WITHSCORES")
                .query_async(&mut c)
                .await?;
            json!(v
                .into_iter()
                .map(|(m, s)| json!({ "member": m, "score": s }))
                .collect::<Vec<_>>())
        }
        "hash" => {
            let (_, v): (u64, Vec<String>) = redis::cmd("HSCAN")
                .arg(&k)
                .arg(0)
                .arg("COUNT")
                .arg(200)
                .query_async(&mut c)
                .await?;
            let mut m = serde_json::Map::new();
            for pair in v.chunks(2) {
                if let [f, val] = pair {
                    m.insert(f.clone(), json!(val));
                }
            }
            Value::Object(m)
        }
        "none" => return Err(AppError::NotFound),
        other => json!(format!("({other} 형식은 화면에서 볼 수 없습니다)")),
    };
    Ok(Json(
        json!({ "key": k, "type": ty, "ttl": if ttl >= 0 { json!(ttl) } else { Value::Null }, "value": value }),
    ))
}

#[derive(Deserialize)]
pub struct SetInput {
    pub key: String,
    #[serde(default)]
    pub value: Option<String>,
    /// 초 (없으면 만료 없음)
    #[serde(default)]
    pub ttl: Option<u64>,
}

/// POST /api/dbweb/redis/{db}/{set|delete}
pub async fn redis_write(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path((db, op)): Path<(u32, String)>,
    Json(input): Json<SetInput>,
) -> AppResult<Json<Value>> {
    user.require_admin()?;
    if input.key.is_empty() || input.key.len() > 1024 {
        return Err(AppError::bad("키 이름이 올바르지 않습니다"));
    }
    let mut c = redis_conn(&state, db).await?;
    let out = match op.as_str() {
        "set" => {
            let v = input.value.clone().unwrap_or_default();
            if v.len() > MAX_DOC_BYTES {
                return Err(AppError::bad("값이 너무 큽니다 (1MB 까지)"));
            }
            let ty: String = redis::cmd("TYPE")
                .arg(&input.key)
                .query_async(&mut c)
                .await?;
            if ty != "string" && ty != "none" {
                return Err(AppError::bad(format!(
                    "{ty} 형식은 화면에서 바꿀 수 없습니다 (문자열만)"
                )));
            }
            let mut cmd = redis::cmd("SET");
            cmd.arg(&input.key).arg(&v);
            if let Some(t) = input.ttl.filter(|t| *t > 0) {
                cmd.arg("EX").arg(t);
            }
            let _: () = cmd.query_async(&mut c).await?;
            json!({ "ok": true })
        }
        "delete" => {
            let n: i64 = redis::cmd("DEL")
                .arg(&input.key)
                .query_async(&mut c)
                .await?;
            json!({ "deleted": n })
        }
        _ => return Err(AppError::NotFound),
    };
    audit::log(
        &state,
        Some(user.id),
        &format!("dbweb.redis.{op}"),
        &format!("db{db}"),
        &ip,
    )
    .await;
    Ok(Json(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_bson_roundtrip() {
        let d =
            from_json(json!({"name": "x", "n": 3, "_id": {"$oid": "65a000000000000000000001"}}))
                .unwrap();
        assert!(matches!(d.get("_id"), Some(Bson::ObjectId(_))));
        let back = to_json(d);
        assert_eq!(back["_id"]["$oid"], "65a000000000000000000001");
        assert!(from_json(json!([1, 2])).is_err());
        assert!(valid_collection("orders").is_ok());
        for bad in ["", "system.users", "a$b"] {
            assert!(valid_collection(bad).is_err());
        }
    }
}
