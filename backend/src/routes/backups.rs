use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::Response,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    audit,
    auth::{AuthUser, ClientIp},
    backup,
    deploy::{helper, logsink::LogSink},
    error::{AppError, AppResult},
    models::{Backup, BACKUP_COLUMNS},
    routes::apps,
    state::AppState,
};

#[derive(Serialize, sqlx::FromRow)]
pub struct BackupView {
    pub id: u64,
    pub app_id: u64,
    pub app_name: String,
    pub app_slug: String,
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
    pub limit: Option<u32>,
}

/// GET /api/backups?app_id=
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<Vec<BackupView>>> {
    user.require_admin()?;
    let limit = q.limit.unwrap_or(200).clamp(1, 1000);
    let base =
        "SELECT b.id, b.app_id, a.name AS app_name, a.slug AS app_slug, b.kind, b.target, b.file, \
                b.size_bytes, b.status, b.error, b.trigger_by, b.created_at, b.finished_at \
                FROM backups b JOIN apps a ON a.id = b.app_id";
    let rows = match q.app_id {
        Some(id) => {
            sqlx::query_as::<_, BackupView>(&format!(
                "{base} WHERE b.app_id = ? ORDER BY b.id DESC LIMIT ?"
            ))
            .bind(id)
            .bind(limit)
            .fetch_all(&state.db)
            .await?
        }
        None => {
            sqlx::query_as::<_, BackupView>(&format!("{base} ORDER BY b.id DESC LIMIT ?"))
                .bind(limit)
                .fetch_all(&state.db)
                .await?
        }
    };
    Ok(Json(rows))
}

/// POST /api/apps/{id}/backups — 지금 백업
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
) -> AppResult<(StatusCode, Json<serde_json::Value>)> {
    user.require_admin()?;
    let app = apps::fetch(&state, id).await?;
    let ids = backup::start(&state, &app, "manual").await?;
    audit::log(&state, Some(user.id), "backup.create", &app.slug, &ip).await;
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
    let app = apps::fetch(&state, b.app_id).await?;
    if b.kind == "db" {
        let exists: Option<(u64,)> =
            sqlx::query_as("SELECT id FROM app_databases WHERE app_id = ? AND name = ?")
                .bind(app.id)
                .bind(&b.target)
                .fetch_optional(&state.db)
                .await?;
        if exists.is_none() {
            return Err(AppError::bad(format!(
                "데이터베이스 {} 가 더 이상 이 앱에 없습니다. 같은 이름으로 다시 만든 뒤 복원하세요",
                b.target
            )));
        }
    }
    let log = LogSink::capture();
    let out = if b.kind == "db" {
        helper::call_long(
            &state,
            &["restore-db", &app.slug, &b.target, &b.file],
            &log,
            3600,
        )
        .await
    } else {
        helper::call_long(&state, &["restore-data", &app.slug, &b.file], &log, 3600).await
    }
    .map_err(|e| AppError::BadRequest(format!("{e:#}")))?;
    audit::log(
        &state,
        Some(user.id),
        "backup.restore",
        &format!("{} ← {}", app.slug, b.file),
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
    let app = apps::fetch(&state, b.app_id).await?;
    backup::delete_row(&state, &app.slug, &b).await?;
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
    let app = apps::fetch(&state, b.app_id).await?;
    let path = backup::backup_dir(&state, &app.slug).join(&b.file);
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
            format!("attachment; filename=\"{}-{}\"", app.slug, b.file),
        )
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from_stream(stream))
        .map_err(|e| AppError::Internal(e.into()))
}

#[derive(Deserialize)]
pub struct Settings {
    pub schedule: String,
    pub keep: u32,
}

/// PUT /api/apps/{id}/backup-settings
pub async fn update_settings(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
    Json(s): Json<Settings>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let app = apps::fetch(&state, id).await?;
    if !matches!(s.schedule.as_str(), "off" | "daily" | "weekly") {
        return Err(AppError::bad("schedule 은 off / daily / weekly"));
    }
    if !(1..=100).contains(&s.keep) {
        return Err(AppError::bad("보관 개수는 1~100"));
    }
    sqlx::query("UPDATE apps SET backup_schedule = ?, backup_keep = ? WHERE id = ?")
        .bind(&s.schedule)
        .bind(s.keep)
        .bind(id)
        .execute(&state.db)
        .await?;
    audit::log(
        &state,
        Some(user.id),
        "backup.settings",
        &format!("{} {} keep={}", app.slug, s.schedule, s.keep),
        &ip,
    )
    .await;
    Ok(Json(
        json!({ "schedule": s.schedule, "keep": s.keep, "hour": state.cfg.backup_hour }),
    ))
}
