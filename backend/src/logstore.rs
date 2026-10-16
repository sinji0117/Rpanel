//! 패널 로그 저장소 — MongoDB 가 있으면 MongoDB, 없으면 MariaDB/Redis
//!
//! MongoDB(`MONGODB_URL`)가 설정되면 패널 로그는 `rpanel_logs` 데이터베이스에 저장합니다.
//!   - `audit`          활동 기록
//!   - `deployment_logs` 배포 로그 본문 (배포 목록·상태는 MariaDB 에 그대로)
//!   - `notifications`  알림
//!   - `metrics`        모니터링 기록 (7일 지나면 TTL 인덱스로 자동 삭제)
//!
//! 숫자 id 는 `counters` 컬렉션으로 이어 붙여 화면·API 는 그대로 숫자 id 를 씁니다.
//! 처음 켜질 때 MariaDB/Redis 에 있던 기존 로그를 한 번 옮깁니다.

use std::time::Duration;

use anyhow::Context;
use chrono::{DateTime, Utc};
use futures::TryStreamExt;
use mongodb::{
    bson::{doc, Bson, Document},
    options::{FindOptions, IndexOptions, ReturnDocument},
    Collection, Database, IndexModel,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{metrics::Sample, models::AuditLog, state::AppState};

pub const LOG_DB: &str = "rpanel_logs";
const NOTIFY_KEEP: i64 = 1000;

/// 지금 어디에 저장하는지 (화면 표시용)
pub fn backend_name(state: &AppState) -> &'static str {
    if state.mongo.is_some() {
        "mongodb"
    } else {
        "mariadb"
    }
}

fn db(state: &AppState) -> Option<Database> {
    state.mongo.as_ref().map(|c| c.database(LOG_DB))
}

fn to_bson_dt(t: DateTime<Utc>) -> mongodb::bson::DateTime {
    mongodb::bson::DateTime::from_millis(t.timestamp_millis())
}

fn from_bson_dt(t: &mongodb::bson::DateTime) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(t.timestamp_millis()).unwrap_or_default()
}

/// 이어지는 숫자 id
async fn next_id(d: &Database, name: &str) -> anyhow::Result<u64> {
    let c: Collection<Document> = d.collection("counters");
    let r = c
        .find_one_and_update(doc! { "_id": name }, doc! { "$inc": { "seq": 1_i64 } })
        .upsert(true)
        .return_document(ReturnDocument::After)
        .await?
        .context("counter")?;
    Ok(match r.get("seq") {
        Some(Bson::Int64(v)) => *v as u64,
        Some(Bson::Int32(v)) => *v as u64,
        _ => 1,
    })
}

/// 인덱스 준비 + 기존 로그 옮기기 (패널 시작 때 한 번)
pub async fn init(state: &AppState) -> anyhow::Result<()> {
    let Some(d) = db(state) else { return Ok(()) };
    let idx = |keys: Document| IndexModel::builder().keys(keys).build();
    d.collection::<Document>("audit")
        .create_index(idx(doc! { "_id": -1 }))
        .await
        .ok();
    d.collection::<Document>("notifications")
        .create_index(idx(doc! { "read_at": 1 }))
        .await?;
    d.collection::<Document>("deployment_logs")
        .create_index(idx(doc! { "app_id": 1 }))
        .await?;
    // 7일 지난 지표 자동 삭제 (TTL 인덱스를 못 쓰는 호환 서버면 metrics_push 가 한 시간마다 지움)
    if let Err(e) = d
        .collection::<Document>("metrics")
        .create_index(
            IndexModel::builder()
                .keys(doc! { "at": 1 })
                .options(
                    IndexOptions::builder()
                        .expire_after(Duration::from_secs(7 * 24 * 3600))
                        .build(),
                )
                .build(),
        )
        .await
    {
        tracing::debug!(error = %e, "metrics TTL index not created");
    }
    d.collection::<Document>("metrics")
        .create_index(idx(doc! { "t": 1 }))
        .await
        .ok();
    migrate(state, &d).await
}

async fn migrated(state: &AppState) -> bool {
    sqlx::query_scalar::<_, String>("SELECT v FROM panel_settings WHERE k = 'logs_migrated_mongo'")
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .is_some()
}

/// MariaDB 의 활동 기록·알림·배포 로그를 MongoDB 로 (id 그대로)
#[allow(clippy::type_complexity)]
async fn migrate(state: &AppState, d: &Database) -> anyhow::Result<()> {
    if migrated(state).await {
        return Ok(());
    }
    let audits: Vec<(u64, Option<u64>, String, String, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, user_id, action, target, ip, created_at FROM audit_logs ORDER BY id",
    )
    .fetch_all(&state.db)
    .await?;
    let mut max_audit = 0;
    let docs: Vec<Document> = audits
        .into_iter()
        .map(|(id, uid, action, target, ip, at)| {
            max_audit = max_audit.max(id);
            doc! { "_id": id as i64, "user_id": uid.map(|u| u as i64), "action": action, "target": target, "ip": ip, "created_at": to_bson_dt(at) }
        })
        .collect();
    for chunk in docs.chunks(1000) {
        d.collection::<Document>("audit")
            .insert_many(chunk.to_vec())
            .ordered(false)
            .await
            .ok();
    }
    let notes: Vec<(u64, String, String, String, String, Option<String>, Option<String>, Option<DateTime<Utc>>, DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, kind, level, title, CAST(COALESCE(params, '{}') AS CHAR), body, link, read_at, created_at FROM notifications ORDER BY id",
    )
    .fetch_all(&state.db)
    .await?;
    let mut max_note = 0;
    let docs: Vec<Document> = notes
        .into_iter()
        .map(|(id, kind, level, title, params, body, link, read_at, at)| {
            max_note = max_note.max(id);
            let params: Value = serde_json::from_str(&params).unwrap_or(Value::Null);
            doc! {
                "_id": id as i64, "kind": kind, "level": level, "title": title,
                "params": mongodb::bson::to_bson(&params).unwrap_or(Bson::Null),
                "body": body, "link": link, "read_at": read_at.map(to_bson_dt), "created_at": to_bson_dt(at),
            }
        })
        .collect();
    for chunk in docs.chunks(1000) {
        d.collection::<Document>("notifications")
            .insert_many(chunk.to_vec())
            .ordered(false)
            .await
            .ok();
    }
    let counters: Collection<Document> = d.collection("counters");
    for (name, max) in [("audit", max_audit), ("notifications", max_note)] {
        counters
            .update_one(doc! { "_id": name }, doc! { "$max": { "seq": max as i64 } })
            .upsert(true)
            .await?;
    }
    // 배포 로그 본문
    let logs: Vec<(u64, u64, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, app_id, log, created_at FROM deployments WHERE log IS NOT NULL ORDER BY id",
    )
    .fetch_all(&state.db)
    .await?;
    for (id, app_id, log, at) in logs {
        d.collection::<Document>("deployment_logs")
            .update_one(
                doc! { "_id": id as i64 },
                doc! { "$setOnInsert": { "app_id": app_id as i64, "log": log, "created_at": to_bson_dt(at) } },
            )
            .upsert(true)
            .await?;
    }
    // 모니터링 기록 (Redis → MongoDB)
    let samples = redis_samples(state, crate::metrics::HISTORY_MAX)
        .await
        .unwrap_or_default();
    let docs: Vec<Document> = samples.iter().map(sample_doc).collect();
    for chunk in docs.chunks(1000) {
        d.collection::<Document>("metrics")
            .insert_many(chunk.to_vec())
            .ordered(false)
            .await
            .ok();
    }
    sqlx::query("INSERT INTO panel_settings (k, v) VALUES ('logs_migrated_mongo', ?) ON DUPLICATE KEY UPDATE v = VALUES(v)")
        .bind(Utc::now().to_rfc3339())
        .execute(&state.db)
        .await?;
    tracing::info!("panel logs migrated to MongoDB");
    Ok(())
}

// ───────────── 활동 기록 ─────────────

pub async fn audit_write(
    state: &AppState,
    user_id: Option<u64>,
    action: &str,
    target: &str,
    ip: &str,
) -> anyhow::Result<()> {
    if let Some(d) = db(state) {
        let id = next_id(&d, "audit").await?;
        d.collection::<Document>("audit")
            .insert_one(doc! {
                "_id": id as i64, "user_id": user_id.map(|u| u as i64), "action": action, "target": target, "ip": ip,
                "created_at": to_bson_dt(Utc::now()),
            })
            .await?;
        return Ok(());
    }
    sqlx::query("INSERT INTO audit_logs (user_id, action, target, ip) VALUES (?, ?, ?, ?)")
        .bind(user_id)
        .bind(action)
        .bind(target)
        .bind(ip)
        .execute(&state.db)
        .await?;
    Ok(())
}

pub async fn audit_list(state: &AppState, limit: u32) -> anyhow::Result<Vec<AuditLog>> {
    let Some(d) = db(state) else {
        return Ok(sqlx::query_as::<_, AuditLog>(
            "SELECT l.id, l.user_id, u.email, l.action, l.target, l.ip, l.created_at \
             FROM audit_logs l LEFT JOIN users u ON u.id = l.user_id ORDER BY l.id DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&state.db)
        .await?);
    };
    let docs: Vec<Document> = d
        .collection::<Document>("audit")
        .find(doc! {})
        .with_options(
            FindOptions::builder()
                .sort(doc! { "_id": -1 })
                .limit(limit as i64)
                .build(),
        )
        .await?
        .try_collect()
        .await?;
    let emails: std::collections::HashMap<u64, String> =
        sqlx::query_as::<_, (u64, String)>("SELECT id, email FROM users")
            .fetch_all(&state.db)
            .await?
            .into_iter()
            .collect();
    Ok(docs
        .into_iter()
        .map(|x| {
            let user_id = int(&x, "user_id");
            AuditLog {
                id: int(&x, "_id").unwrap_or(0),
                user_id,
                email: user_id.and_then(|u| emails.get(&u).cloned()),
                action: string(&x, "action"),
                target: string(&x, "target"),
                ip: string(&x, "ip"),
                created_at: x
                    .get_datetime("created_at")
                    .map(from_bson_dt)
                    .unwrap_or_default(),
            }
        })
        .collect())
}

fn int(d: &Document, k: &str) -> Option<u64> {
    match d.get(k)? {
        Bson::Int64(v) => Some(*v as u64),
        Bson::Int32(v) => Some(*v as u64),
        Bson::Double(v) => Some(*v as u64),
        _ => None,
    }
}

fn string(d: &Document, k: &str) -> String {
    d.get_str(k).unwrap_or_default().to_string()
}

fn opt_string(d: &Document, k: &str) -> Option<String> {
    d.get_str(k).ok().map(String::from)
}

// ───────────── 배포 로그 ─────────────

/// 배포 로그 본문 저장 (MongoDB 면 거기에, 아니면 deployments.log)
pub async fn deploy_log_save(
    state: &AppState,
    deploy_id: u64,
    app_id: u64,
    text: &str,
) -> anyhow::Result<()> {
    if let Some(d) = db(state) {
        d.collection::<Document>("deployment_logs")
            .update_one(
                doc! { "_id": deploy_id as i64 },
                doc! { "$set": { "app_id": app_id as i64, "log": text, "created_at": to_bson_dt(Utc::now()) } },
            )
            .upsert(true)
            .await?;
        return Ok(());
    }
    sqlx::query("UPDATE deployments SET log = ? WHERE id = ?")
        .bind(text)
        .bind(deploy_id)
        .execute(&state.db)
        .await?;
    Ok(())
}

/// 배포 로그 본문 (MongoDB 에 없으면 예전 MariaDB 값)
pub async fn deploy_log_get(state: &AppState, deploy_id: u64) -> anyhow::Result<Option<String>> {
    if let Some(d) = db(state) {
        if let Some(x) = d
            .collection::<Document>("deployment_logs")
            .find_one(doc! { "_id": deploy_id as i64 })
            .await?
        {
            return Ok(opt_string(&x, "log"));
        }
    }
    Ok(
        sqlx::query_scalar::<_, Option<String>>("SELECT log FROM deployments WHERE id = ?")
            .bind(deploy_id)
            .fetch_optional(&state.db)
            .await?
            .flatten(),
    )
}

/// 앱을 지울 때 그 앱의 배포 로그도
pub async fn deploy_logs_delete_app(state: &AppState, app_id: u64) {
    if let Some(d) = db(state) {
        let _ = d
            .collection::<Document>("deployment_logs")
            .delete_many(doc! { "app_id": app_id as i64 })
            .await;
    }
}

// ───────────── 알림 ─────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Notification {
    pub id: u64,
    pub kind: String,
    pub level: String,
    pub title: String,
    pub params: Value,
    pub body: Option<String>,
    pub link: Option<String>,
    pub read_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

pub struct NewNotification<'a> {
    pub kind: &'a str,
    pub level: &'a str,
    pub title: &'a str,
    pub params: &'a Value,
    pub body: Option<&'a str>,
    pub link: Option<&'a str>,
}

pub async fn notify_insert(state: &AppState, n: NewNotification<'_>) -> anyhow::Result<()> {
    if let Some(d) = db(state) {
        let id = next_id(&d, "notifications").await?;
        let c = d.collection::<Document>("notifications");
        c.insert_one(doc! {
            "_id": id as i64, "kind": n.kind, "level": n.level, "title": n.title,
            "params": mongodb::bson::to_bson(n.params).unwrap_or(Bson::Null),
            "body": n.body, "link": n.link, "read_at": Bson::Null, "created_at": to_bson_dt(Utc::now()),
        })
        .await?;
        // 최근 1000개만
        let cut = id as i64 - NOTIFY_KEEP;
        if cut > 0 {
            c.delete_many(doc! { "_id": { "$lte": cut } }).await.ok();
        }
        return Ok(());
    }
    sqlx::query("INSERT INTO notifications (kind, level, title, params, body, link) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(n.kind)
        .bind(n.level)
        .bind(n.title)
        .bind(n.params.to_string())
        .bind(n.body)
        .bind(n.link)
        .execute(&state.db)
        .await?;
    let _ = sqlx::query(
        "DELETE FROM notifications WHERE id < (SELECT id FROM (SELECT id FROM notifications ORDER BY id DESC LIMIT 1 OFFSET 999) x)",
    )
    .execute(&state.db)
    .await;
    Ok(())
}

pub async fn notify_list(
    state: &AppState,
    limit: u32,
    unread_only: bool,
) -> anyhow::Result<(Vec<Notification>, i64)> {
    if let Some(d) = db(state) {
        let c = d.collection::<Document>("notifications");
        let filter = if unread_only {
            doc! { "read_at": Bson::Null }
        } else {
            doc! {}
        };
        let docs: Vec<Document> = c
            .find(filter)
            .with_options(
                FindOptions::builder()
                    .sort(doc! { "_id": -1 })
                    .limit(limit as i64)
                    .build(),
            )
            .await?
            .try_collect()
            .await?;
        let unread = c.count_documents(doc! { "read_at": Bson::Null }).await? as i64;
        let items = docs
            .into_iter()
            .map(|x| Notification {
                id: int(&x, "_id").unwrap_or(0),
                kind: string(&x, "kind"),
                level: string(&x, "level"),
                title: string(&x, "title"),
                params: x
                    .get("params")
                    .and_then(|p| serde_json::to_value(p.clone().into_relaxed_extjson()).ok())
                    .filter(|v| v.is_object())
                    .unwrap_or_else(|| serde_json::json!({})),
                body: opt_string(&x, "body"),
                link: opt_string(&x, "link"),
                read_at: x.get_datetime("read_at").ok().map(from_bson_dt),
                created_at: x
                    .get_datetime("created_at")
                    .map(from_bson_dt)
                    .unwrap_or_default(),
            })
            .collect();
        return Ok((items, unread));
    }
    #[allow(clippy::type_complexity)]
    let rows: Vec<(u64, String, String, String, String, Option<String>, Option<String>, Option<DateTime<Utc>>, DateTime<Utc>)> = sqlx::query_as(&format!(
        "SELECT id, kind, level, title, CAST(COALESCE(params, '{{}}') AS CHAR), body, link, read_at, created_at \
         FROM notifications {} ORDER BY id DESC LIMIT ?",
        if unread_only { "WHERE read_at IS NULL" } else { "" }
    ))
    .bind(limit)
    .fetch_all(&state.db)
    .await?;
    let items = rows
        .into_iter()
        .map(
            |(id, kind, level, title, params, body, link, read_at, created_at)| Notification {
                id,
                kind,
                level,
                title,
                params: serde_json::from_str(&params).unwrap_or_else(|_| serde_json::json!({})),
                body,
                link,
                read_at,
                created_at,
            },
        )
        .collect();
    Ok((items, notify_unread(state).await?))
}

pub async fn notify_unread(state: &AppState) -> anyhow::Result<i64> {
    if let Some(d) = db(state) {
        return Ok(d
            .collection::<Document>("notifications")
            .count_documents(doc! { "read_at": Bson::Null })
            .await? as i64);
    }
    Ok(
        sqlx::query_scalar("SELECT COUNT(*) FROM notifications WHERE read_at IS NULL")
            .fetch_one(&state.db)
            .await?,
    )
}

/// ids 가 비어 있고 all=true 면 전부
pub async fn notify_mark_read(state: &AppState, ids: &[u64], all: bool) -> anyhow::Result<()> {
    if let Some(d) = db(state) {
        let c = d.collection::<Document>("notifications");
        let now = to_bson_dt(Utc::now());
        let filter = if all {
            doc! { "read_at": Bson::Null }
        } else {
            let ids: Vec<i64> = ids.iter().take(500).map(|i| *i as i64).collect();
            doc! { "_id": { "$in": ids }, "read_at": Bson::Null }
        };
        c.update_many(filter, doc! { "$set": { "read_at": now } })
            .await?;
        return Ok(());
    }
    if all {
        sqlx::query(
            "UPDATE notifications SET read_at = CURRENT_TIMESTAMP(3) WHERE read_at IS NULL",
        )
        .execute(&state.db)
        .await?;
    } else {
        for id in ids.iter().take(500) {
            sqlx::query("UPDATE notifications SET read_at = CURRENT_TIMESTAMP(3) WHERE id = ? AND read_at IS NULL")
                .bind(id)
                .execute(&state.db)
                .await?;
        }
    }
    Ok(())
}

pub async fn notify_clear(state: &AppState) -> anyhow::Result<()> {
    if let Some(d) = db(state) {
        d.collection::<Document>("notifications")
            .delete_many(doc! {})
            .await?;
        return Ok(());
    }
    sqlx::query("DELETE FROM notifications")
        .execute(&state.db)
        .await?;
    Ok(())
}

// ───────────── 모니터링 기록 ─────────────

fn redis_key(state: &AppState) -> String {
    state.key(&["metrics", "history"])
}

fn sample_doc(s: &Sample) -> Document {
    doc! {
        "at": mongodb::bson::DateTime::from_millis(s.t * 1000),
        "t": s.t, "c": s.c as f64, "m": s.m as i64, "mt": s.mt as i64,
        "l": [s.l[0], s.l[1], s.l[2]],
        "d": s.d.iter().map(|(m, u, t)| doc! { "m": m, "u": *u as i64, "t": *t as i64 }).collect::<Vec<_>>(),
    }
}

fn doc_sample(x: &Document) -> Option<Sample> {
    let f = |k: &str| match x.get(k)? {
        Bson::Double(v) => Some(*v),
        Bson::Int64(v) => Some(*v as f64),
        Bson::Int32(v) => Some(*v as f64),
        _ => None,
    };
    let l = x.get_array("l").ok()?;
    let lf = |i: usize| l.get(i).and_then(|b| b.as_f64()).unwrap_or(0.0);
    let d = x
        .get_array("d")
        .ok()?
        .iter()
        .filter_map(|b| b.as_document())
        .map(|dd| {
            let g = |k: &str| match dd.get(k) {
                Some(Bson::Int64(v)) => *v as u64,
                Some(Bson::Int32(v)) => *v as u64,
                Some(Bson::Double(v)) => *v as u64,
                _ => 0,
            };
            (
                dd.get_str("m").unwrap_or_default().to_string(),
                g("u"),
                g("t"),
            )
        })
        .collect();
    Some(Sample {
        t: f("t")? as i64,
        c: f("c")? as f32,
        m: f("m")? as u64,
        mt: f("mt")? as u64,
        l: [lf(0), lf(1), lf(2)],
        d,
    })
}

async fn redis_samples(state: &AppState, count: isize) -> anyhow::Result<Vec<Sample>> {
    let mut r = state.redis.clone();
    let raw: Vec<String> = redis::cmd("LRANGE")
        .arg(redis_key(state))
        .arg(0)
        .arg(count - 1)
        .query_async(&mut r)
        .await?;
    Ok(raw
        .iter()
        .filter_map(|s| serde_json::from_str(s).ok())
        .collect())
}

pub async fn metrics_push(state: &AppState, s: &Sample) -> anyhow::Result<()> {
    if let Some(d) = db(state) {
        d.collection::<Document>("metrics")
            .insert_one(sample_doc(s))
            .await?;
        return Ok(());
    }
    let key = redis_key(state);
    let mut r = state.redis.clone();
    redis::pipe()
        .cmd("LPUSH")
        .arg(&key)
        .arg(serde_json::to_string(s)?)
        .ignore()
        .cmd("LTRIM")
        .arg(&key)
        .arg(0)
        .arg(crate::metrics::HISTORY_MAX - 1)
        .ignore()
        .query_async::<()>(&mut r)
        .await?;
    Ok(())
}

/// since(초) 이후 기록
pub async fn metrics_since(state: &AppState, since: i64) -> anyhow::Result<Vec<Sample>> {
    if let Some(d) = db(state) {
        let docs: Vec<Document> = d
            .collection::<Document>("metrics")
            .find(doc! { "t": { "$gte": since } })
            .with_options(FindOptions::builder().sort(doc! { "t": 1 }).build())
            .await?
            .try_collect()
            .await?;
        return Ok(docs.iter().filter_map(doc_sample).collect());
    }
    let count = ((chrono::Utc::now().timestamp() - since) / crate::metrics::SAMPLE_SECS as i64 + 5)
        as isize;
    redis_samples(state, count).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_roundtrip() {
        let s = Sample {
            t: 1_792_000_000,
            c: 12.5,
            m: 1 << 30,
            mt: 4 << 30,
            l: [0.5, 0.25, 0.125],
            d: vec![("/".into(), 10, 100)],
        };
        assert_eq!(doc_sample(&sample_doc(&s)), Some(s));
    }
}
