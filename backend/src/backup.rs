//! 백업: MariaDB 덤프(.sql.gz) + 사이트 파일 / 앱 data 디렉토리(.tar.gz)
//!
//! - 파일은 /var/lib/rpanel/backups/<앱 slug>/ 에 저장합니다. 앱에 연결하지 않은 데이터베이스는
//!   /var/lib/rpanel/backups/_databases/ 에 저장합니다.
//! - 백업 예약(backup_jobs)은 여러 개 만들 수 있고, 예약마다 대상(앱 / 데이터베이스 / 전체)·주기·보관 개수를 정합니다.
//!   보관 개수는 그 예약이 만든 백업에만 적용됩니다 (직접 만든 백업은 지우기 전까지 남음).

use std::{path::PathBuf, time::Duration};

use chrono::{DateTime, Datelike, Local, NaiveDate, Timelike, Utc};
use redis::AsyncCommands;

use crate::{
    deploy::{helper, logsink::LogSink},
    error::{AppError, AppResult},
    models::{
        App, AppDatabase, Backup, BackupJob, APP_COLUMNS, BACKUP_COLUMNS, BACKUP_JOB_COLUMNS,
        DB_COLUMNS,
    },
    routes::databases::list_for_app,
    state::AppState,
};

/// 앱에 연결하지 않은 데이터베이스의 백업 폴더 이름
pub const DB_SCOPE: &str = "_databases";

pub fn backup_dir(state: &AppState, scope: &str) -> PathBuf {
    state.cfg.data_dir.join("backups").join(scope)
}

pub fn stamp_now() -> String {
    Local::now().format("%Y%m%d-%H%M%S").to_string()
}

/// 파일 이름 검증 (helper 와 같은 규칙)
pub fn valid_file_name(f: &str) -> bool {
    static RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
        regex::Regex::new(r"^[0-9]{8}-[0-9]{6}-(db-[a-z][a-z0-9_]+\.sql|mongo-[a-z][a-z0-9_]+\.archive|data\.tar|files\.tar)\.gz$")
            .unwrap()
    });
    RE.is_match(f)
}

/// 백업 행이 저장된 폴더 (앱 slug 또는 _databases)
pub async fn scope_of(state: &AppState, b: &Backup) -> AppResult<String> {
    match b.app_id {
        Some(id) => {
            let slug: Option<String> = sqlx::query_scalar("SELECT slug FROM apps WHERE id = ?")
                .bind(id)
                .fetch_optional(&state.db)
                .await?;
            slug.ok_or(AppError::NotFound)
        }
        None => Ok(DB_SCOPE.to_string()),
    }
}

/// 예약에서 실행될 때의 정보: (예약 id, 보관 개수)
pub type JobRef = Option<(u64, u32)>;

struct Item {
    kind: &'static str,
    target: String,
    file: String,
    database_id: Option<u64>,
    mongo_id: Option<u64>,
}

struct Running {
    id: u64,
    kind: &'static str,
    target: String,
}

async fn take_lock(state: &AppState, key: &str) -> AppResult<String> {
    if crate::storage::running() {
        return Err(AppError::Conflict(
            "저장 위치를 옮기는 중입니다. 끝난 뒤 백업하세요".into(),
        ));
    }
    let mut r = state.redis.clone();
    let lock = state.key(&["lock", "backup", key]);
    let got: Option<String> = redis::cmd("SET")
        .arg(&lock)
        .arg(1)
        .arg("NX")
        .arg("EX")
        .arg(3600)
        .query_async(&mut r)
        .await?;
    if got.is_none() {
        return Err(AppError::Conflict(
            "이 대상의 백업이 이미 진행 중입니다".into(),
        ));
    }
    Ok(lock)
}

/// 앱 백업: 연결된 DB 전부 + 사이트 파일(파일 사이트) 또는 data 디렉토리(Git 서비스 앱)
pub async fn start_app(
    state: &AppState,
    app: &App,
    trigger: &str,
    job: JobRef,
) -> AppResult<Vec<u64>> {
    let stamp = stamp_now();
    let mut items = Vec::new();
    for db in list_for_app(state, app.id).await? {
        items.push(Item {
            kind: "db",
            target: db.name.clone(),
            file: format!("{stamp}-db-{}.sql.gz", db.name),
            database_id: Some(db.id),
            mongo_id: None,
        });
    }
    if state.mongo.is_some() {
        for m in crate::routes::mongo::list_for_app(state, app.id).await? {
            items.push(Item {
                kind: "mongo",
                target: m.name.clone(),
                file: format!("{stamp}-mongo-{}.archive.gz", m.name),
                database_id: None,
                mongo_id: Some(m.id),
            });
        }
    }
    if app.site_user.is_some() {
        items.push(Item {
            kind: "files",
            target: "htdocs".into(),
            file: format!("{stamp}-files.tar.gz"),
            database_id: None,
            mongo_id: None,
        });
    } else if app.kind().is_service() {
        items.push(Item {
            kind: "data",
            target: "data".into(),
            file: format!("{stamp}-data.tar.gz"),
            database_id: None,
            mongo_id: None,
        });
    }
    if items.is_empty() {
        return Err(AppError::bad(
            "백업할 대상이 없습니다 (연결된 데이터베이스가 없고, 이 앱은 파일·data 디렉토리가 없습니다)",
        ));
    }
    let lock = take_lock(state, &format!("app-{}", app.id)).await?;
    launch(
        state,
        lock,
        app.slug.clone(),
        Some(app.id),
        items,
        stamp,
        trigger,
        job,
    )
    .await
}

/// 데이터베이스 하나만 백업
pub async fn start_database(
    state: &AppState,
    db: &AppDatabase,
    trigger: &str,
    job: JobRef,
) -> AppResult<Vec<u64>> {
    let scope = match db.app_id {
        Some(id) => {
            let slug: Option<String> = sqlx::query_scalar("SELECT slug FROM apps WHERE id = ?")
                .bind(id)
                .fetch_optional(&state.db)
                .await?;
            slug.ok_or(AppError::NotFound)?
        }
        None => DB_SCOPE.to_string(),
    };
    let stamp = stamp_now();
    let lock = take_lock(state, &format!("db-{}", db.id)).await?;
    let items = vec![Item {
        kind: "db",
        target: db.name.clone(),
        file: format!("{stamp}-db-{}.sql.gz", db.name),
        database_id: Some(db.id),
        mongo_id: None,
    }];
    launch(state, lock, scope, db.app_id, items, stamp, trigger, job).await
}

/// MongoDB 데이터베이스 하나만 백업 (mongodump)
pub async fn start_mongo(
    state: &AppState,
    m: &crate::routes::mongo::MongoDb,
    trigger: &str,
    job: JobRef,
) -> AppResult<Vec<u64>> {
    let scope = match m.app_id {
        Some(id) => sqlx::query_scalar::<_, String>("SELECT slug FROM apps WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.db)
            .await?
            .ok_or(AppError::NotFound)?,
        None => DB_SCOPE.to_string(),
    };
    let stamp = stamp_now();
    let lock = take_lock(state, &format!("mongo-{}", m.id)).await?;
    let items = vec![Item {
        kind: "mongo",
        target: m.name.clone(),
        file: format!("{stamp}-mongo-{}.archive.gz", m.name),
        database_id: None,
        mongo_id: Some(m.id),
    }];
    launch(state, lock, scope, m.app_id, items, stamp, trigger, job).await
}

#[allow(clippy::too_many_arguments)]
async fn launch(
    state: &AppState,
    lock: String,
    scope: String,
    app_id: Option<u64>,
    items: Vec<Item>,
    stamp: String,
    trigger: &str,
    job: JobRef,
) -> AppResult<Vec<u64>> {
    let mut running = Vec::new();
    for it in items {
        let res = sqlx::query(
            "INSERT INTO backups (app_id, database_id, mongo_id, job_id, kind, target, file, status, trigger_by) \
             VALUES (?, ?, ?, ?, ?, ?, ?, 'running', ?)",
        )
        .bind(app_id)
        .bind(it.database_id)
        .bind(it.mongo_id)
        .bind(job.map(|j| j.0))
        .bind(it.kind)
        .bind(&it.target)
        .bind(&it.file)
        .bind(trigger)
        .execute(&state.db)
        .await?;
        running.push(Running {
            id: res.last_insert_id(),
            kind: it.kind,
            target: it.target,
        });
    }
    let ids = running.iter().map(|j| j.id).collect();

    let state = state.clone();
    tokio::spawn(async move {
        let mut failed: Vec<String> = Vec::new();
        let mut done = 0usize;
        for job_item in running {
            let log = LogSink::capture();
            let result = match job_item.kind {
                "mongo" => {
                    helper::call_long(
                        &state,
                        &["backup-mongo", &scope, &job_item.target, &stamp],
                        &log,
                        3600,
                    )
                    .await
                }
                "db" => {
                    helper::call_long(
                        &state,
                        &["backup-db", &scope, &job_item.target, &stamp],
                        &log,
                        3600,
                    )
                    .await
                }
                "files" => {
                    helper::call_long(&state, &["backup-files", &scope, &stamp], &log, 3600).await
                }
                _ => helper::call_long(&state, &["backup-data", &scope, &stamp], &log, 3600).await,
            };
            let (status, size, err) = match result {
                Ok(out) => (
                    "success",
                    out.stdout.trim().parse::<u64>().unwrap_or(0),
                    None,
                ),
                Err(e) => (
                    "failed",
                    0,
                    Some(format!("{e:#}").chars().take(1000).collect::<String>()),
                ),
            };
            if status == "success" {
                done += 1;
            } else {
                failed.push(format!(
                    "{} {}: {}",
                    job_item.kind,
                    job_item.target,
                    err.as_deref().unwrap_or("")
                ));
            }
            let _ = sqlx::query(
                "UPDATE backups SET status = ?, size_bytes = ?, error = ?, finished_at = CURRENT_TIMESTAMP(3) WHERE id = ?",
            )
            .bind(status)
            .bind(size)
            .bind(err)
            .bind(job_item.id)
            .execute(&state.db)
            .await;
        }
        let params = serde_json::json!({ "scope": scope, "n": done, "failed": failed.len() });
        let ev = if failed.is_empty() {
            crate::notify::Event::new(
                "backup_success",
                "success",
                "백업 완료: {scope} ({n}개)",
                params,
            )
        } else {
            crate::notify::Event::new(
                "backup_failed",
                "error",
                "백업 실패: {scope} ({failed}개 실패)",
                params,
            )
            .body(failed.join("\n"))
        };
        crate::notify::emit(&state, ev.link("/backups")).await;
        if let Some((job_id, keep)) = job {
            if let Err(e) = prune_job(&state, job_id, keep).await {
                tracing::warn!(error = %e, job_id, "backup prune failed");
            }
        }
        let mut r = state.redis.clone();
        let _: Result<(), _> = r.del(&lock).await;
    });
    Ok(ids)
}

/// 예약 하나가 만든 백업을 대상(앱·DB·종류)별로 최근 keep 개만 남기고 삭제
pub async fn prune_job(state: &AppState, job_id: u64, keep: u32) -> AppResult<()> {
    let keep = keep.max(1) as usize;
    let rows = sqlx::query_as::<_, Backup>(&format!(
        "SELECT {BACKUP_COLUMNS} FROM backups WHERE job_id = ? AND status <> 'running' ORDER BY id DESC"
    ))
    .bind(job_id)
    .fetch_all(&state.db)
    .await?;
    let mut seen: std::collections::HashMap<(Option<u64>, String, String), usize> =
        Default::default();
    for b in rows {
        let n = seen
            .entry((b.app_id, b.kind.clone(), b.target.clone()))
            .or_default();
        *n += 1;
        if *n > keep {
            let scope = scope_of(state, &b).await?;
            delete_row(state, &scope, &b).await?;
        }
    }
    Ok(())
}

pub async fn delete_row(state: &AppState, scope: &str, b: &Backup) -> AppResult<()> {
    if !state.exec.is_dry_run() && valid_file_name(&b.file) {
        let p = backup_dir(state, scope).join(&b.file);
        match tokio::fs::remove_file(&p).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(AppError::Internal(anyhow::anyhow!(
                    "백업 파일 삭제 실패 {}: {e}",
                    p.display()
                )))
            }
        }
    }
    sqlx::query("DELETE FROM backups WHERE id = ?")
        .bind(b.id)
        .execute(&state.db)
        .await?;
    Ok(())
}

// ───────────── 백업 예약 ─────────────

fn at(d: NaiveDate, h: u32, m: u32) -> Option<DateTime<Local>> {
    d.and_hms_opt(h, m, 0)?.and_local_timezone(Local).earliest()
}

/// 지금(now) 이전의 가장 최근 예약 시각
pub fn prev_fire(
    schedule: &str,
    hour: u32,
    minute: u32,
    weekday: u32,
    now: DateTime<Local>,
) -> Option<DateTime<Local>> {
    let today = now.date_naive();
    match schedule {
        "hourly" => {
            let c = at(today, now.hour(), minute)?;
            Some(if c > now {
                c - chrono::Duration::hours(1)
            } else {
                c
            })
        }
        "daily" => {
            let c = at(today, hour, minute)?;
            if c > now {
                at(today.pred_opt()?, hour, minute)
            } else {
                Some(c)
            }
        }
        "weekly" => {
            let back = (now.weekday().num_days_from_sunday() + 7 - weekday % 7) % 7;
            let d = today - chrono::Days::new(back as u64);
            let c = at(d, hour, minute)?;
            if c > now {
                at(d - chrono::Days::new(7), hour, minute)
            } else {
                Some(c)
            }
        }
        _ => None,
    }
}

/// 다음 예약 시각 (화면 표시용)
pub fn next_fire(
    schedule: &str,
    hour: u32,
    minute: u32,
    weekday: u32,
    now: DateTime<Local>,
) -> Option<DateTime<Local>> {
    let prev = prev_fire(schedule, hour, minute, weekday, now)?;
    let step = match schedule {
        "hourly" => chrono::Duration::hours(1),
        "daily" => chrono::Duration::days(1),
        _ => chrono::Duration::days(7),
    };
    Some(prev + step)
}

/// 이 예약이 지금 실행되어야 하는가
/// (마지막 실행 — 없으면 만든 시각 — 이후에 예약 시각이 지났고, 그 시각에서 6시간이 넘지 않았을 때)
pub fn is_due(job: &BackupJob, now: DateTime<Local>) -> bool {
    if !job.enabled {
        return false;
    }
    let Some(prev) = prev_fire(
        &job.schedule,
        job.hour as u32,
        job.minute as u32,
        job.weekday as u32,
        now,
    ) else {
        return false;
    };
    let since = job.last_run_at.unwrap_or(job.created_at);
    prev.with_timezone(&Utc) > since && now - prev < chrono::Duration::hours(6)
}

/// 예약 실행: 대상에 맞는 백업을 시작하고 마지막 실행 시각을 기록. 시작한 백업 수 반환
pub async fn run_job(state: &AppState, job: &BackupJob, trigger: &str) -> AppResult<usize> {
    sqlx::query("UPDATE backup_jobs SET last_run_at = CURRENT_TIMESTAMP(3) WHERE id = ?")
        .bind(job.id)
        .execute(&state.db)
        .await?;
    let jr: JobRef = Some((job.id, job.keep_count));
    match job.target.as_str() {
        "app" => {
            let id = job.app_id.ok_or(AppError::NotFound)?;
            let app =
                sqlx::query_as::<_, App>(&format!("SELECT {APP_COLUMNS} FROM apps WHERE id = ?"))
                    .bind(id)
                    .fetch_optional(&state.db)
                    .await?
                    .ok_or(AppError::NotFound)?;
            Ok(start_app(state, &app, trigger, jr).await?.len())
        }
        "database" => {
            let id = job.database_id.ok_or(AppError::NotFound)?;
            let db = sqlx::query_as::<_, AppDatabase>(&format!(
                "SELECT {DB_COLUMNS} FROM app_databases WHERE id = ?"
            ))
            .bind(id)
            .fetch_optional(&state.db)
            .await?
            .ok_or(AppError::NotFound)?;
            Ok(start_database(state, &db, trigger, jr).await?.len())
        }
        _ => {
            // 전체: 모든 앱 + 앱에 연결하지 않은 데이터베이스
            let mut n = 0;
            let apps =
                sqlx::query_as::<_, App>(&format!("SELECT {APP_COLUMNS} FROM apps ORDER BY id"))
                    .fetch_all(&state.db)
                    .await?;
            for app in apps {
                match start_app(state, &app, trigger, jr).await {
                    Ok(ids) => n += ids.len(),
                    Err(AppError::BadRequest(_)) => {} // 백업할 것이 없는 앱 (프록시 등)
                    Err(e) => tracing::warn!(app = %app.slug, error = %e, "backup skipped"),
                }
            }
            let dbs = sqlx::query_as::<_, AppDatabase>(&format!(
                "SELECT {DB_COLUMNS} FROM app_databases WHERE app_id IS NULL ORDER BY id"
            ))
            .fetch_all(&state.db)
            .await?;
            for db in dbs {
                match start_database(state, &db, trigger, jr).await {
                    Ok(ids) => n += ids.len(),
                    Err(e) => tracing::warn!(db = %db.name, error = %e, "backup skipped"),
                }
            }
            if state.mongo.is_some() {
                let ms = sqlx::query_as::<_, crate::routes::mongo::MongoDb>(
                    "SELECT id, app_id, name, username, password, env_key, created_at FROM mongo_databases WHERE app_id IS NULL ORDER BY id",
                )
                .fetch_all(&state.db)
                .await?;
                for m in ms {
                    match start_mongo(state, &m, trigger, jr).await {
                        Ok(ids) => n += ids.len(),
                        Err(e) => tracing::warn!(db = %m.name, error = %e, "mongo backup skipped"),
                    }
                }
            }
            Ok(n)
        }
    }
}

/// 1분마다 예약 확인 (패널 프로세스 안에서 실행)
pub fn spawn_scheduler(state: AppState) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        loop {
            tick.tick().await;
            if let Err(e) = scheduler_tick(&state).await {
                tracing::warn!(error = %e, "backup scheduler tick failed");
            }
        }
    });
}

async fn scheduler_tick(state: &AppState) -> AppResult<()> {
    let jobs = sqlx::query_as::<_, BackupJob>(&format!(
        "SELECT {BACKUP_JOB_COLUMNS} FROM backup_jobs WHERE enabled = 1"
    ))
    .fetch_all(&state.db)
    .await?;
    let now = Local::now();
    for job in jobs {
        if !is_due(&job, now) {
            continue;
        }
        match run_job(state, &job, "schedule").await {
            Ok(n) => tracing::info!(job = job.id, count = n, "scheduled backup started"),
            Err(e) => tracing::warn!(job = job.id, error = %e, "scheduled backup skipped"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn file_names() {
        assert!(valid_file_name("20261013-030000-db-beripick_api.sql.gz"));
        assert!(valid_file_name("20261013-030000-data.tar.gz"));
        assert!(valid_file_name("20261013-030000-files.tar.gz"));
        assert!(!valid_file_name("../etc/passwd"));
        assert!(!valid_file_name("20261013-030000-db-x.sql.gz/../../a"));
        assert!(!valid_file_name("20261013-030000-other.tar.gz"));
    }

    fn job(schedule: &str, hour: u8, minute: u8, weekday: u8) -> BackupJob {
        let created = Local
            .with_ymd_and_hms(2026, 10, 1, 0, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        BackupJob {
            id: 1,
            name: "t".into(),
            target: "all".into(),
            app_id: None,
            database_id: None,
            schedule: schedule.into(),
            hour,
            minute,
            weekday,
            keep_count: 7,
            enabled: true,
            last_run_at: None,
            created_at: created,
            updated_at: created,
        }
    }

    #[test]
    fn fire_times() {
        // 2026-10-14 는 수요일 (weekday 3)
        let now = Local.with_ymd_and_hms(2026, 10, 14, 3, 20, 0).unwrap();
        let p = |s, h, m, w| prev_fire(s, h, m, w, now).unwrap();
        assert_eq!(
            p("hourly", 0, 15, 0),
            Local.with_ymd_and_hms(2026, 10, 14, 3, 15, 0).unwrap()
        );
        assert_eq!(
            p("hourly", 0, 30, 0),
            Local.with_ymd_and_hms(2026, 10, 14, 2, 30, 0).unwrap()
        );
        assert_eq!(
            p("daily", 3, 0, 0),
            Local.with_ymd_and_hms(2026, 10, 14, 3, 0, 0).unwrap()
        );
        assert_eq!(
            p("daily", 4, 0, 0),
            Local.with_ymd_and_hms(2026, 10, 13, 4, 0, 0).unwrap()
        );
        assert_eq!(
            p("weekly", 3, 0, 3),
            Local.with_ymd_and_hms(2026, 10, 14, 3, 0, 0).unwrap()
        );
        assert_eq!(
            p("weekly", 3, 0, 0),
            Local.with_ymd_and_hms(2026, 10, 11, 3, 0, 0).unwrap()
        );
        assert_eq!(
            p("weekly", 5, 0, 3),
            Local.with_ymd_and_hms(2026, 10, 7, 5, 0, 0).unwrap()
        );
        assert_eq!(
            next_fire("daily", 4, 0, 0, now).unwrap(),
            Local.with_ymd_and_hms(2026, 10, 14, 4, 0, 0).unwrap()
        );
        assert!(prev_fire("off", 3, 0, 0, now).is_none());
    }

    #[test]
    fn due_logic() {
        let at3 = Local.with_ymd_and_hms(2026, 10, 13, 3, 5, 0).unwrap();
        let mut j = job("daily", 3, 0, 0);
        assert!(is_due(&j, at3));
        // 방금 실행했으면 다시 돌지 않음
        j.last_run_at = Some((at3 - chrono::Duration::minutes(2)).with_timezone(&Utc));
        assert!(!is_due(&j, at3));
        // 어제 실행 → 오늘 3시가 지났으니 실행
        j.last_run_at = Some((at3 - chrono::Duration::hours(24)).with_timezone(&Utc));
        assert!(is_due(&j, at3));
        // 꺼진 예약
        j.enabled = false;
        assert!(!is_due(&j, at3));
        // 예약 시각에서 6시간 넘게 지났으면 (패널이 오래 꺼져 있었던 경우) 다음 예약까지 기다림
        let j = job("daily", 3, 0, 0);
        let at10 = Local.with_ymd_and_hms(2026, 10, 13, 10, 0, 0).unwrap();
        assert!(!is_due(&j, at10));
        // 방금 만든 예약은 지난 시각으로 바로 돌지 않음
        let mut j = job("daily", 3, 0, 0);
        j.created_at = (at3 - chrono::Duration::minutes(1)).with_timezone(&Utc);
        assert!(!is_due(&j, at3));
    }
}
