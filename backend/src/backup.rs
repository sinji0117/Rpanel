//! 앱 백업: 연결된 MariaDB 덤프(.sql.gz) + 앱 data 디렉토리(.tar.gz)
//! 파일은 /var/lib/rpanel/backups/<slug>/ 에 저장되며(NAS가 rsync로 가져가기 좋은 위치),
//! 앱별 보관 개수를 넘으면 오래된 것부터 지웁니다. 일/주 단위 예약 실행을 지원합니다.

use std::{path::PathBuf, time::Duration};

use chrono::{DateTime, Local, Timelike, Utc};
use redis::AsyncCommands;

use crate::{
    deploy::{helper, logsink::LogSink},
    error::{AppError, AppResult},
    models::{App, Backup, APP_COLUMNS, BACKUP_COLUMNS},
    routes::databases::list_for_app,
    state::AppState,
};

pub fn backup_dir(state: &AppState, slug: &str) -> PathBuf {
    state.cfg.data_dir.join("backups").join(slug)
}

pub fn stamp_now() -> String {
    Local::now().format("%Y%m%d-%H%M%S").to_string()
}

/// 파일 이름 검증 (helper 와 같은 규칙)
pub fn valid_file_name(f: &str) -> bool {
    static RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
        regex::Regex::new(r"^[0-9]{8}-[0-9]{6}-(db-[a-z][a-z0-9_]+\.sql|data\.tar)\.gz$").unwrap()
    });
    RE.is_match(f)
}

struct Job {
    id: u64,
    kind: String,
    target: String,
}

/// 백업 행을 만들고 백그라운드로 실행. 생성된 백업 id 목록 반환.
pub async fn start(state: &AppState, app: &App, trigger: &str) -> AppResult<Vec<u64>> {
    let mut r = state.redis.clone();
    let lock = state.key(&["lock", "backup", &app.id.to_string()]);
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
            "이 앱의 백업이 이미 진행 중입니다".into(),
        ));
    }

    let stamp = stamp_now();
    let mut jobs = Vec::new();
    for db in list_for_app(state, app.id).await? {
        let file = format!("{stamp}-db-{}.sql.gz", db.name);
        jobs.push(("db", db.name.clone(), file));
    }
    if app.kind().is_service() {
        jobs.push(("data", "data".to_string(), format!("{stamp}-data.tar.gz")));
    }
    if jobs.is_empty() {
        let _: () = r.del(&lock).await?;
        return Err(AppError::bad(
            "백업할 대상이 없습니다 (연결된 데이터베이스가 없고, 정적 앱은 data 디렉토리가 없습니다)",
        ));
    }
    let mut created = Vec::new();
    for (kind, target, file) in jobs {
        let res = sqlx::query(
            "INSERT INTO backups (app_id, kind, target, file, status, trigger_by) VALUES (?, ?, ?, ?, 'running', ?)",
        )
        .bind(app.id)
        .bind(kind)
        .bind(&target)
        .bind(&file)
        .bind(trigger)
        .execute(&state.db)
        .await?;
        created.push(Job {
            id: res.last_insert_id(),
            kind: kind.into(),
            target,
        });
    }
    let ids = created.iter().map(|j| j.id).collect();

    let state = state.clone();
    let app = app.clone();
    tokio::spawn(async move {
        for job in created {
            let log = LogSink::capture();
            let result = match job.kind.as_str() {
                "db" => {
                    helper::call_long(
                        &state,
                        &["backup-db", &app.slug, &job.target, &stamp],
                        &log,
                        3600,
                    )
                    .await
                }
                _ => {
                    helper::call_long(&state, &["backup-data", &app.slug, &stamp], &log, 3600).await
                }
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
            let _ = sqlx::query(
                "UPDATE backups SET status = ?, size_bytes = ?, error = ?, finished_at = CURRENT_TIMESTAMP(3) WHERE id = ?",
            )
            .bind(status)
            .bind(size)
            .bind(err)
            .bind(job.id)
            .execute(&state.db)
            .await;
        }
        if let Err(e) = prune(&state, &app).await {
            tracing::warn!(error = %e, app = %app.slug, "backup prune failed");
        }
        let mut r = state.redis.clone();
        let _: Result<(), _> = r.del(&lock).await;
    });
    Ok(ids)
}

/// 대상(DB 이름 / data)별로 최근 keep 개만 남기고 삭제
pub async fn prune(state: &AppState, app: &App) -> AppResult<()> {
    let keep = app.backup_keep.max(1) as usize;
    let rows = sqlx::query_as::<_, Backup>(&format!(
        "SELECT {BACKUP_COLUMNS} FROM backups WHERE app_id = ? AND status <> 'running' ORDER BY id DESC"
    ))
    .bind(app.id)
    .fetch_all(&state.db)
    .await?;
    let mut seen: std::collections::HashMap<(String, String), usize> = Default::default();
    for b in rows {
        let n = seen.entry((b.kind.clone(), b.target.clone())).or_default();
        *n += 1;
        if *n > keep {
            delete_row(state, &app.slug, &b).await?;
        }
    }
    Ok(())
}

pub async fn delete_row(state: &AppState, slug: &str, b: &Backup) -> AppResult<()> {
    if !state.exec.is_dry_run() && valid_file_name(&b.file) {
        let p = backup_dir(state, slug).join(&b.file);
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

/// 예약 백업이 지금 실행되어야 하는가
pub fn is_due(
    schedule: &str,
    last: Option<DateTime<Utc>>,
    now_local: DateTime<Local>,
    hour: u32,
) -> bool {
    let period = match schedule {
        "daily" => chrono::Duration::hours(24),
        "weekly" => chrono::Duration::days(7),
        _ => return false,
    };
    if now_local.hour() != hour {
        return false;
    }
    match last {
        None => true,
        // 같은 시각대에 두 번 돌지 않도록 30분 여유
        Some(t) => now_local.with_timezone(&Utc) - t >= period - chrono::Duration::minutes(30),
    }
}

/// 1분마다 예약 백업 확인 (패널 프로세스 안에서 실행)
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
    let apps = sqlx::query_as::<_, App>(&format!(
        "SELECT {APP_COLUMNS} FROM apps WHERE backup_schedule <> 'off'"
    ))
    .fetch_all(&state.db)
    .await?;
    let now = Local::now();
    for app in apps {
        if !is_due(
            &app.backup_schedule,
            app.backup_last_scheduled,
            now,
            state.cfg.backup_hour,
        ) {
            continue;
        }
        sqlx::query("UPDATE apps SET backup_last_scheduled = CURRENT_TIMESTAMP(3) WHERE id = ?")
            .bind(app.id)
            .execute(&state.db)
            .await?;
        match start(state, &app, "schedule").await {
            Ok(ids) => {
                tracing::info!(app = %app.slug, count = ids.len(), "scheduled backup started")
            }
            Err(e) => tracing::warn!(app = %app.slug, error = %e, "scheduled backup skipped"),
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
        assert!(!valid_file_name("../etc/passwd"));
        assert!(!valid_file_name("20261013-030000-db-x.sql.gz/../../a"));
        assert!(!valid_file_name("20261013-030000-other.tar.gz"));
    }

    #[test]
    fn due_logic() {
        let at3 = Local.with_ymd_and_hms(2026, 10, 13, 3, 5, 0).unwrap();
        let at4 = Local.with_ymd_and_hms(2026, 10, 13, 4, 5, 0).unwrap();
        assert!(is_due("daily", None, at3, 3));
        assert!(!is_due("daily", None, at4, 3));
        assert!(!is_due("off", None, at3, 3));
        let yesterday = (at3 - chrono::Duration::hours(24)).with_timezone(&Utc);
        assert!(is_due("daily", Some(yesterday), at3, 3));
        let minutes_ago = (at3 - chrono::Duration::minutes(2)).with_timezone(&Utc);
        assert!(!is_due("daily", Some(minutes_ago), at3, 3));
        assert!(!is_due("weekly", Some(yesterday), at3, 3));
        let week = (at3 - chrono::Duration::days(7)).with_timezone(&Utc);
        assert!(is_due("weekly", Some(week), at3, 3));
    }
}
