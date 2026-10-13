use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{
    audit,
    auth::{AuthUser, ClientIp},
    error::{AppError, AppResult},
    models::Project,
    state::AppState,
    validate,
};

#[derive(Serialize, sqlx::FromRow)]
pub struct ProjectSummary {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub app_count: i64,
    pub running_count: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Deserialize)]
pub struct ProjectInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

fn check(input: &ProjectInput) -> AppResult<(String, String)> {
    validate::name(&input.name).map_err(AppError::BadRequest)?;
    let desc = input.description.trim().to_string();
    if desc.chars().count() > 500 {
        return Err(AppError::bad("설명은 500자 이하여야 합니다"));
    }
    Ok((input.name.trim().to_string(), desc))
}

pub async fn list(
    State(state): State<AppState>,
    _u: AuthUser,
) -> AppResult<Json<Vec<ProjectSummary>>> {
    let rows = sqlx::query_as::<_, ProjectSummary>(
        "SELECT p.id, p.name, p.description, \
                CAST(COUNT(a.id) AS SIGNED) AS app_count, \
                CAST(COALESCE(SUM(a.status = 'running'), 0) AS SIGNED) AS running_count, \
                p.created_at, p.updated_at \
         FROM projects p LEFT JOIN apps a ON a.project_id = p.id \
         GROUP BY p.id ORDER BY p.name",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

pub async fn get(
    State(state): State<AppState>,
    _u: AuthUser,
    Path(id): Path<u64>,
) -> AppResult<Json<Project>> {
    Ok(Json(fetch(&state, id).await?))
}

pub async fn fetch(state: &AppState, id: u64) -> AppResult<Project> {
    sqlx::query_as::<_, Project>(
        "SELECT id, name, description, created_at, updated_at FROM projects WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)
}

pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Json(input): Json<ProjectInput>,
) -> AppResult<(StatusCode, Json<Project>)> {
    user.require_admin()?;
    let (name, desc) = check(&input)?;
    let res = sqlx::query("INSERT INTO projects (name, description) VALUES (?, ?)")
        .bind(&name)
        .bind(&desc)
        .execute(&state.db)
        .await?;
    audit::log(&state, Some(user.id), "project.create", &name, &ip).await;
    Ok((
        StatusCode::CREATED,
        Json(fetch(&state, res.last_insert_id()).await?),
    ))
}

pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
    Json(input): Json<ProjectInput>,
) -> AppResult<Json<Project>> {
    user.require_admin()?;
    let (name, desc) = check(&input)?;
    fetch(&state, id).await?;
    sqlx::query("UPDATE projects SET name = ?, description = ? WHERE id = ?")
        .bind(&name)
        .bind(&desc)
        .bind(id)
        .execute(&state.db)
        .await?;
    audit::log(&state, Some(user.id), "project.update", &name, &ip).await;
    Ok(Json(fetch(&state, id).await?))
}

pub async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
) -> AppResult<StatusCode> {
    user.require_admin()?;
    let p = fetch(&state, id).await?;
    let apps: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM apps WHERE project_id = ?")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    if apps > 0 {
        return Err(AppError::Conflict(format!(
            "앱 {apps}개가 남아 있습니다. 앱을 먼저 삭제하세요"
        )));
    }
    sqlx::query("DELETE FROM projects WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    audit::log(&state, Some(user.id), "project.delete", &p.name, &ip).await;
    Ok(StatusCode::NO_CONTENT)
}
