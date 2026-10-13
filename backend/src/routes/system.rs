use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::json;

use crate::{
    auth::AuthUser, config::ExecMode, error::AppResult, metrics, models::AuditLog,
    routes::deployments, state::AppState,
};

pub async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let db = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.db)
        .await
        .is_ok();
    let mut r = state.redis.clone();
    let redis = redis::cmd("PING")
        .query_async::<String>(&mut r)
        .await
        .is_ok();
    Json(json!({ "status": if db && redis { "ok" } else { "degraded" }, "db": db, "redis": redis }))
}

pub async fn info(State(state): State<AppState>, _u: AuthUser) -> Json<serde_json::Value> {
    let host = tokio::task::spawn_blocking(metrics::host_info).await.ok();
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "exec_mode": if state.cfg.exec_mode == ExecMode::DryRun { "dry-run" } else { "real" },
        "host": host,
        "app_port_range": [state.cfg.app_port_min, state.cfg.app_port_max],
    }))
}

pub async fn overview(
    State(state): State<AppState>,
    _u: AuthUser,
) -> AppResult<Json<serde_json::Value>> {
    let (projects,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM projects")
        .fetch_one(&state.db)
        .await?;
    let rows: Vec<(String, i64)> =
        sqlx::query_as("SELECT status, COUNT(*) FROM apps GROUP BY status")
            .fetch_all(&state.db)
            .await?;
    let total: i64 = rows.iter().map(|(_, n)| n).sum();
    let by_status: serde_json::Map<String, serde_json::Value> =
        rows.into_iter().map(|(s, n)| (s, json!(n))).collect();
    let (d24, f24): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), CAST(COALESCE(SUM(status = 'failed'), 0) AS SIGNED) FROM deployments \
         WHERE created_at > UTC_TIMESTAMP(3) - INTERVAL 1 DAY",
    )
    .fetch_one(&state.db)
    .await?;
    Ok(Json(json!({
        "projects": projects,
        "apps": { "total": total, "by_status": by_status },
        "deployments_24h": { "total": d24, "failed": f24 },
        "recent_deployments": deployments::recent(&state, 10).await?,
    })))
}

#[derive(Deserialize)]
pub struct AuditQuery {
    pub limit: Option<u32>,
}

pub async fn audit(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<AuditQuery>,
) -> AppResult<Json<Vec<AuditLog>>> {
    user.require_admin()?;
    let rows = sqlx::query_as::<_, AuditLog>(
        "SELECT l.id, l.user_id, u.email, l.action, l.target, l.ip, l.created_at \
         FROM audit_logs l LEFT JOIN users u ON u.id = l.user_id ORDER BY l.id DESC LIMIT ?",
    )
    .bind(q.limit.unwrap_or(100).clamp(1, 500))
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}
