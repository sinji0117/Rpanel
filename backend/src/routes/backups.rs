use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::Response,
    Json,
};
use chrono::{Local, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    audit,
    auth::{AuthUser, ClientIp},
    backup,
    deploy::{helper, logsink::LogSink},
    error::{AppError, AppResult},
    models::{AppDatabase, Backup, BackupJob, BACKUP_COLUMNS, BACKUP_JOB_COLUMNS, DB_COLUMNS},
    routes::apps,
    state::AppState,
};

#[derive(Serialize, sqlx::FromRow)]
pub struct BackupView {
    pub id: u64,
    pub app_id: Option<u64>,
    pub app_name: Option<String>,
    pub app_slug: Option<String>,
    pub database_id: Option<u64>,
    pub job_id: Option<u64>,
    pub job_name: Option<String>,
    pub kind: String,
    pub target: String,
    pub file: String,
    pub size_bytes: u64,
    pub status: String,
    pub error: Option<String>,
    pub trigger_by: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub app_id: Option<u64>,
    pub database_id: Option<u64>,
    pub job_id: Option<u64>,
    pub limit: Option<u32>,
}

/// GET /api/backups?app_id=&database_id=&job_id=
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<Vec<BackupView>>> {
    user.require_admin()?;
    let limit = q.limit.unwrap_or(200).clamp(1, 1000);
    let mut sql = String::from(
        "SELECT b.id, b.app_id, a.name AS app_name, a.slug AS app_slug, b.database_id, b.job_id, \
                j.name AS job_name, b.kind, b.target, b.file, b.size_bytes, b.status, b.error, \
                b.trigger_by, b.created_at, b.finished_at \
         FROM backups b LEFT JOIN apps a ON a.id = b.app_id LEFT JOIN backup_jobs j ON j.id = b.job_id \
         WHERE 1 = 1",
    );
    if q.app_id.is_some() {
        sql.push_str(" AND b.app_id = ?");
    }
    if q.database_id.is_some() {
        sql.push_str(" AND b.database_id = ?");
    }
    if q.job_id.is_some() {
        sql.push_str(" AND b.job_id = ?");
    }
    sql.push_str(" ORDER BY b.id DESC LIMIT ?");
    let mut query = sqlx::query_as::<_, BackupView>(&sql);
    for v in [q.app_id, q.database_id, q.job_id].into_iter().flatten() {
        query = query.bind(v);
    }
    let rows = query.bind(limit).fetch_all(&state.db).await?;
    Ok(Json(rows))
}

/// POST /api/apps/{id}/backups — 앱 지금 백업
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
) -> AppResult<(StatusCode, Json<serde_json::Value>)> {
    user.require_admin()?;
    let app = apps::fetch(&state, id).await?;
    let ids = backup::start_app(&state, &app, "manual", None).await?;
    audit::log(&state, Some(user.id), "backup.create", &app.slug, &ip).await;
    Ok((StatusCode::ACCEPTED, Json(json!({ "backup_ids": ids }))))
}

pub async fn fetch_database(state: &AppState, id: u64) -> AppResult<AppDatabase> {
    sqlx::query_as::<_, AppDatabase>(&format!(
        "SELECT {DB_COLUMNS} FROM app_databases WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)
}

/// POST /api/databases/{id}/backups — 데이터베이스 하나만 지금 백업
pub async fn create_for_database(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
) -> AppResult<(StatusCode, Json<serde_json::Value>)> {
    user.require_admin()?;
    let db = fetch_database(&state, id).await?;
    let ids = backup::start_database(&state, &db, "manual", None).await?;
    audit::log(&state, Some(user.id), "backup.create", &db.name, &ip).await;
    Ok((StatusCode::ACCEPTED, Json(json!({ "backup_ids": ids }))))
}

async fn fetch(state: &AppState, id: u64) -> AppResult<Backup> {
    sqlx::query_as::<_, Backup>(&format!(
        "SELECT {BACKUP_COLUMNS} FROM backups WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)
}

/// POST /api/backups/{id}/restore
pub async fn restore(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let b = fetch(&state, id).await?;
    if b.status != "success" {
        return Err(AppError::bad("성공한 백업만 복원할 수 있습니다"));
    }
    let scope = backup::scope_of(&state, &b).await?;
    if b.kind == "db" {
        let exists: Option<(u64,)> = sqlx::query_as("SELECT id FROM app_databases WHERE name = ?")
            .bind(&b.target)
            .fetch_optional(&state.db)
            .await?;
        if exists.is_none() {
            return Err(AppError::bad(format!(
                "데이터베이스 {} 가 더 이상 없습니다. 같은 이름으로 다시 만든 뒤 복원하세요",
                b.target
            )));
        }
    } else if b.kind == "mongo" {
        let exists: Option<u64> =
            sqlx::query_scalar("SELECT id FROM mongo_databases WHERE name = ?")
                .bind(&b.target)
                .fetch_optional(&state.db)
                .await?;
        if exists.is_none() {
            return Err(AppError::bad(format!(
                "MongoDB 데이터베이스 {} 가 더 이상 없습니다. 같은 이름으로 다시 만든 뒤 복원하세요",
                b.target
            )));
        }
    } else if b.app_id.is_none() {
        return Err(AppError::bad("앱이 없는 파일 백업입니다"));
    }
    let log = LogSink::capture();
    let out = if b.kind == "db" {
        helper::call_long(
            &state,
            &["restore-db", &scope, &b.target, &b.file],
            &log,
            3600,
        )
        .await
    } else if b.kind == "mongo" {
        helper::call_long(
            &state,
            &["restore-mongo", &scope, &b.target, &b.file],
            &log,
            3600,
        )
        .await
    } else if b.kind == "files" {
        helper::call_long(&state, &["restore-files", &scope, &b.file], &log, 3600).await
    } else {
        helper::call_long(&state, &["restore-data", &scope, &b.file], &log, 3600).await
    }
    .map_err(|e| AppError::BadRequest(format!("{e:#}")))?;
    audit::log(
        &state,
        Some(user.id),
        "backup.restore",
        &format!("{scope} ← {}", b.file),
        &ip,
    )
    .await;
    Ok(Json(json!({ "ok": true, "output": out.stdout.trim() })))
}

/// DELETE /api/backups/{id}
pub async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
) -> AppResult<StatusCode> {
    user.require_admin()?;
    let b = fetch(&state, id).await?;
    if b.status == "running" {
        return Err(AppError::Conflict("진행 중인 백업입니다".into()));
    }
    let scope = backup::scope_of(&state, &b).await?;
    backup::delete_row(&state, &scope, &b).await?;
    audit::log(&state, Some(user.id), "backup.delete", &b.file, &ip).await;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/backups/{id}/download
pub async fn download(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
) -> AppResult<Response> {
    user.require_admin()?;
    let b = fetch(&state, id).await?;
    if b.status != "success" || !backup::valid_file_name(&b.file) {
        return Err(AppError::NotFound);
    }
    let scope = backup::scope_of(&state, &b).await?;
    let path = backup::backup_dir(&state, &scope).join(&b.file);
    let file = tokio::fs::File::open(&path)
        .await
        .map_err(|_| AppError::NotFound)?;
    let len = file.metadata().await.map(|m| m.len()).unwrap_or(0);
    audit::log(&state, Some(user.id), "backup.download", &b.file, &ip).await;
    let stream = tokio_util::io::ReaderStream::new(file);
    Response::builder()
        .header(header::CONTENT_TYPE, "application/gzip")
        .header(header::CONTENT_LENGTH, len)
        .header(
            header::CONTENT_DISPOSITION,
            format!(
                "attachment; filename=\"{}-{}\"",
                scope.trim_start_matches('_'),
                b.file
            ),
        )
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from_stream(stream))
        .map_err(|e| AppError::Internal(e.into()))
}

// ───────────── 백업 예약 ─────────────

#[derive(Serialize)]
pub struct JobView {
    #[serde(flatten)]
    pub job: BackupJob,
    /// 대상 이름 (앱 이름 / DB 이름 / 전체)
    pub target_name: String,
    pub next_run_at: Option<chrono::DateTime<Utc>>,
    pub last_status: Option<String>,
    pub backup_count: i64,
}

async fn job_view(state: &AppState, job: BackupJob) -> AppResult<JobView> {
    let target_name = match job.target.as_str() {
        "app" => sqlx::query_scalar::<_, String>("SELECT name FROM apps WHERE id = ?")
            .bind(job.app_id)
            .fetch_optional(&state.db)
            .await?
            .unwrap_or_else(|| "(삭제된 앱)".into()),
        "database" => {
            sqlx::query_scalar::<_, String>("SELECT name FROM app_databases WHERE id = ?")
                .bind(job.database_id)
                .fetch_optional(&state.db)
                .await?
                .unwrap_or_else(|| "(삭제된 DB)".into())
        }
        _ => "모든 앱·데이터베이스".into(),
    };
    let next_run_at = if job.enabled {
        backup::next_fire(
            &job.schedule,
            job.hour as u32,
            job.minute as u32,
            job.weekday as u32,
            Local::now(),
        )
        .map(|t| t.with_timezone(&Utc))
    } else {
        None
    };
    let last_status: Option<String> =
        sqlx::query_scalar("SELECT status FROM backups WHERE job_id = ? ORDER BY id DESC LIMIT 1")
            .bind(job.id)
            .fetch_optional(&state.db)
            .await?;
    let backup_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM backups WHERE job_id = ?")
        .bind(job.id)
        .fetch_one(&state.db)
        .await?;
    Ok(JobView {
        job,
        target_name,
        next_run_at,
        last_status,
        backup_count,
    })
}

async fn fetch_job(state: &AppState, id: u64) -> AppResult<BackupJob> {
    sqlx::query_as::<_, BackupJob>(&format!(
        "SELECT {BACKUP_JOB_COLUMNS} FROM backup_jobs WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)
}

#[derive(Deserialize)]
pub struct JobQuery {
    pub app_id: Option<u64>,
}

/// GET /api/backup-jobs?app_id=  (app_id 를 주면 그 앱이 대상인 예약 + 전체 예약)
pub async fn list_jobs(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<JobQuery>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let jobs = match q.app_id {
        Some(id) => {
            sqlx::query_as::<_, BackupJob>(&format!(
                "SELECT {BACKUP_JOB_COLUMNS} FROM backup_jobs \
                 WHERE app_id = ? OR target = 'all' \
                    OR database_id IN (SELECT id FROM app_databases WHERE app_id = ?) ORDER BY id"
            ))
            .bind(id)
            .bind(id)
            .fetch_all(&state.db)
            .await?
        }
        None => {
            sqlx::query_as::<_, BackupJob>(&format!(
                "SELECT {BACKUP_JOB_COLUMNS} FROM backup_jobs ORDER BY id"
            ))
            .fetch_all(&state.db)
            .await?
        }
    };
    let mut out = Vec::new();
    for j in jobs {
        out.push(job_view(&state, j).await?);
    }
    Ok(Json(json!({
        "jobs": out,
        "timezone": Local::now().format("%:z").to_string(),
    })))
}

#[derive(Deserialize)]
pub struct JobInput {
    pub name: Option<String>,
    pub target: String,
    pub app_id: Option<u64>,
    pub database_id: Option<u64>,
    pub schedule: String,
    #[serde(default)]
    pub hour: u8,
    #[serde(default)]
    pub minute: u8,
    #[serde(default)]
    pub weekday: u8,
    pub keep_count: u32,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

struct CleanJob {
    name: String,
    target: String,
    app_id: Option<u64>,
    database_id: Option<u64>,
    schedule: String,
    hour: u8,
    minute: u8,
    weekday: u8,
    keep: u32,
    enabled: bool,
}

async fn clean_job(state: &AppState, i: JobInput) -> AppResult<CleanJob> {
    if !matches!(i.schedule.as_str(), "hourly" | "daily" | "weekly") {
        return Err(AppError::bad(
            "주기는 hourly / daily / weekly 중 하나입니다",
        ));
    }
    if i.hour > 23 || i.minute > 59 || i.weekday > 6 {
        return Err(AppError::bad(
            "시각은 0~23시 0~59분, 요일은 0(일)~6(토)입니다",
        ));
    }
    if !(1..=365).contains(&i.keep_count) {
        return Err(AppError::bad("보관 개수는 1~365 입니다"));
    }
    let (app_id, database_id, default_name) = match i.target.as_str() {
        "app" => {
            let id = i
                .app_id
                .ok_or_else(|| AppError::bad("백업할 앱을 고르세요"))?;
            let app = apps::fetch(state, id).await?;
            (Some(id), None, format!("{} 백업", app.name))
        }
        "database" => {
            let id = i
                .database_id
                .ok_or_else(|| AppError::bad("백업할 데이터베이스를 고르세요"))?;
            let db = fetch_database(state, id).await?;
            (None, Some(id), format!("{} DB 백업", db.name))
        }
        "all" => (None, None, "전체 백업".to_string()),
        _ => return Err(AppError::bad("대상은 app / database / all 중 하나입니다")),
    };
    let name = i
        .name
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or(default_name);
    if name.chars().count() > 100 || name.chars().any(|c| c.is_control()) {
        return Err(AppError::bad("이름은 100자 이하로 입력하세요"));
    }
    Ok(CleanJob {
        name,
        target: i.target,
        app_id,
        database_id,
        schedule: i.schedule,
        hour: i.hour,
        minute: i.minute,
        weekday: i.weekday,
        keep: i.keep_count,
        enabled: i.enabled,
    })
}

/// POST /api/backup-jobs
pub async fn create_job(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Json(input): Json<JobInput>,
) -> AppResult<(StatusCode, Json<JobView>)> {
    user.require_admin()?;
    let c = clean_job(&state, input).await?;
    let res = sqlx::query(
        "INSERT INTO backup_jobs (name, target, app_id, database_id, schedule, hour, minute, weekday, keep_count, enabled) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&c.name)
    .bind(&c.target)
    .bind(c.app_id)
    .bind(c.database_id)
    .bind(&c.schedule)
    .bind(c.hour)
    .bind(c.minute)
    .bind(c.weekday)
    .bind(c.keep)
    .bind(c.enabled)
    .execute(&state.db)
    .await?;
    audit::log(&state, Some(user.id), "backup.job.create", &c.name, &ip).await;
    let job = fetch_job(&state, res.last_insert_id()).await?;
    Ok((StatusCode::CREATED, Json(job_view(&state, job).await?)))
}

/// PUT /api/backup-jobs/{id}
pub async fn update_job(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
    Json(input): Json<JobInput>,
) -> AppResult<Json<JobView>> {
    user.require_admin()?;
    fetch_job(&state, id).await?;
    let c = clean_job(&state, input).await?;
    sqlx::query(
        "UPDATE backup_jobs SET name = ?, target = ?, app_id = ?, database_id = ?, schedule = ?, hour = ?, \
         minute = ?, weekday = ?, keep_count = ?, enabled = ? WHERE id = ?",
    )
    .bind(&c.name)
    .bind(&c.target)
    .bind(c.app_id)
    .bind(c.database_id)
    .bind(&c.schedule)
    .bind(c.hour)
    .bind(c.minute)
    .bind(c.weekday)
    .bind(c.keep)
    .bind(c.enabled)
    .bind(id)
    .execute(&state.db)
    .await?;
    audit::log(&state, Some(user.id), "backup.job.update", &c.name, &ip).await;
    Ok(Json(job_view(&state, fetch_job(&state, id).await?).await?))
}

#[derive(Deserialize, Default)]
pub struct DeleteJob {
    /// true 면 이 예약이 만든 백업 파일도 삭제
    #[serde(default)]
    pub delete_files: bool,
}

/// DELETE /api/backup-jobs/{id}  (body: {"delete_files": true} 이면 백업 파일도 삭제)
pub async fn delete_job(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
    body: Option<Json<DeleteJob>>,
) -> AppResult<StatusCode> {
    user.require_admin()?;
    let job = fetch_job(&state, id).await?;
    let mut removed = 0;
    if body.map(|b| b.delete_files).unwrap_or(false) {
        let rows = sqlx::query_as::<_, Backup>(&format!(
            "SELECT {BACKUP_COLUMNS} FROM backups WHERE job_id = ? AND status <> 'running'"
        ))
        .bind(id)
        .fetch_all(&state.db)
        .await?;
        for b in rows {
            let scope = backup::scope_of(&state, &b).await?;
            backup::delete_row(&state, &scope, &b).await?;
            removed += 1;
        }
    }
    sqlx::query("DELETE FROM backup_jobs WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    audit::log(
        &state,
        Some(user.id),
        "backup.job.delete",
        &format!("{} (파일 {removed}개 삭제)", job.name),
        &ip,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/backup-jobs/{id}/run — 지금 실행
pub async fn run_job(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
) -> AppResult<(StatusCode, Json<serde_json::Value>)> {
    user.require_admin()?;
    let job = fetch_job(&state, id).await?;
    let n = backup::run_job(&state, &job, "manual").await?;
    audit::log(&state, Some(user.id), "backup.job.run", &job.name, &ip).await;
    Ok((StatusCode::ACCEPTED, Json(json!({ "started": n }))))
}
