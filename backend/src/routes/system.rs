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

#[derive(Deserialize)]
pub struct VersionQuery {
    #[serde(default)]
    pub refresh: bool,
}

const VERSION_CACHE_SECS: u64 = 6 * 3600;

/// GET /api/system/version — 지금 버전과 GitHub 의 최신 버전 (6시간 캐시)
pub async fn version(
    State(state): State<AppState>,
    _u: AuthUser,
    Query(q): Query<VersionQuery>,
) -> AppResult<Json<serde_json::Value>> {
    let current = env!("CARGO_PKG_VERSION");
    let repo = state.cfg.update_repo.trim().to_string();
    if repo.is_empty() {
        return Ok(Json(
            json!({ "current": current, "latest": null, "update_available": false, "repo_url": null, "checked_at": null, "error": null }),
        ));
    }
    let key = state.key(&["update", "latest"]);
    let mut r = state.redis.clone();
    let cached: Option<String> = if q.refresh {
        None
    } else {
        redis::cmd("GET")
            .arg(&key)
            .query_async(&mut r)
            .await
            .ok()
            .flatten()
    };
    let (latest, checked_at, error) =
        match cached.and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok()) {
            Some(v) => (
                v["latest"].as_str().map(String::from),
                v["checked_at"].as_str().map(String::from),
                None::<String>,
            ),
            None => match fetch_latest(&repo).await {
                Ok(v) => {
                    let now = chrono::Utc::now().to_rfc3339();
                    let _: Result<(), _> = redis::cmd("SET")
                        .arg(&key)
                        .arg(json!({ "latest": v, "checked_at": now }).to_string())
                        .arg("EX")
                        .arg(VERSION_CACHE_SECS)
                        .query_async(&mut r)
                        .await;
                    (Some(v), Some(now), None)
                }
                Err(e) => (
                    None,
                    None,
                    Some(format!("GitHub 에서 버전을 확인하지 못했습니다: {e}")),
                ),
            },
        };
    let update_available = latest.as_deref().is_some_and(|l| is_newer(l, current));
    Ok(Json(json!({
        "current": current,
        "latest": latest,
        "update_available": update_available,
        "repo_url": format!("https://github.com/{repo}"),
        "checked_at": checked_at,
        "error": error,
    })))
}

/// GitHub 최신 릴리스 태그 → 없으면 main 브랜치 backend/Cargo.toml 의 version
async fn fetch_latest(repo: &str) -> anyhow::Result<String> {
    if !regex::Regex::new(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$")?.is_match(repo) {
        anyhow::bail!("RPANEL_UPDATE_REPO 형식 오류");
    }
    let release = curl(&format!(
        "https://api.github.com/repos/{repo}/releases/latest"
    ))
    .await;
    if let Ok(body) = release {
        if let Some(tag) = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| {
                v["tag_name"]
                    .as_str()
                    .map(|s| s.trim_start_matches('v').to_string())
            })
        {
            return Ok(tag);
        }
    }
    let toml = curl(&format!(
        "https://raw.githubusercontent.com/{repo}/main/backend/Cargo.toml"
    ))
    .await?;
    cargo_version(&toml)
        .ok_or_else(|| anyhow::anyhow!("Cargo.toml 에서 version 을 찾지 못했습니다"))
}

async fn curl(url: &str) -> anyhow::Result<String> {
    let out = tokio::process::Command::new("curl")
        .args([
            "-fsSL",
            "--max-time",
            "8",
            "-H",
            "Accept: application/vnd.github+json",
            "-A",
            "rpanel",
            url,
        ])
        .kill_on_drop(true)
        .output()
        .await?;
    if !out.status.success() {
        anyhow::bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// [package] 의 version = "x.y.z"
pub fn cargo_version(toml: &str) -> Option<String> {
    let mut in_pkg = false;
    for line in toml.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_pkg = l == "[package]";
        } else if in_pkg {
            if let Some(v) = l
                .strip_prefix("version")
                .map(str::trim)
                .and_then(|r| r.strip_prefix('='))
            {
                return Some(v.trim().trim_matches('"').to_string());
            }
        }
    }
    None
}

/// a 가 b 보다 새 버전인가 (x.y.z 숫자 비교)
pub fn is_newer(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.trim_start_matches('v')
            .split(['.', '-'])
            .take(3)
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    parse(a) > parse(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare() {
        assert!(is_newer("0.4.0", "0.3.0"));
        assert!(is_newer("v0.3.1", "0.3.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(is_newer("0.10.0", "0.9.0"));
        assert!(!is_newer("0.3.0", "0.3.0"));
        assert!(!is_newer("0.2.9", "0.3.0"));
    }

    #[test]
    fn cargo_toml_version() {
        let t = "[package]\nname = \"rpanel\"\nversion = \"0.4.0\"\n\n[dependencies]\nversion = \"9\"\n";
        assert_eq!(cargo_version(t).as_deref(), Some("0.4.0"));
        assert_eq!(cargo_version("[dependencies]\nversion = \"1\""), None);
    }
}
