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
        "boot_time": metrics::boot_time(),
        "panel_started_at": *PANEL_STARTED,
        "log_store": crate::logstore::backend_name(&state),
        "app_port_range": [state.cfg.app_port_min, state.cfg.app_port_max],
    }))
}

/// 패널 프로세스가 시작된 시각 (초)
pub static PANEL_STARTED: once_cell::sync::Lazy<i64> =
    once_cell::sync::Lazy::new(|| chrono::Utc::now().timestamp());

/// 1분마다 지표를 Redis 에 쌓음 (최근 7일)
pub fn spawn_sampler(state: AppState) {
    once_cell::sync::Lazy::force(&PANEL_STARTED);
    tokio::spawn(async move {
        let mut collector = metrics::Collector::new();
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(metrics::SAMPLE_SECS));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        loop {
            tick.tick().await;
            let Ok((c, snap)) = tokio::task::spawn_blocking(move || {
                let s = collector.snapshot();
                (collector, s)
            })
            .await
            else {
                break;
            };
            collector = c;
            if let Err(e) = push_sample(&state, &metrics::Sample::from(&snap)).await {
                tracing::debug!(error = %e, "metrics sample not stored");
            }
            crate::notify::check_resources(&state, &snap).await;
        }
    });
}

pub async fn push_sample(state: &AppState, sample: &metrics::Sample) -> anyhow::Result<()> {
    crate::logstore::metrics_push(state, sample).await
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    pub range: Option<String>,
}

/// GET /api/system/metrics?range=30m|1h|6h|24h|7d — 그래프용 지표 기록
pub async fn metrics_history(
    State(state): State<AppState>,
    _u: AuthUser,
    Query(q): Query<HistoryQuery>,
) -> AppResult<Json<serde_json::Value>> {
    let range = q.range.unwrap_or_else(|| "30m".into());
    let secs = metrics::range_secs(&range).ok_or_else(|| {
        crate::error::AppError::bad("range 는 30m · 1h · 6h · 24h · 7d 중 하나입니다")
    })?;
    let now = chrono::Utc::now().timestamp();
    let samples = crate::logstore::metrics_since(&state, now - secs).await?;
    let points = metrics::downsample(samples, now - secs, 120);
    Ok(Json(json!({
        "range": range,
        "from": now - secs,
        "to": now,
        "interval": metrics::SAMPLE_SECS,
        "cpu_count": tokio::task::spawn_blocking(|| metrics::host_info().cpu_count).await.unwrap_or(0),
        "points": points,
    })))
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
    let rows = crate::logstore::audit_list(&state, q.limit.unwrap_or(100).clamp(1, 500)).await?;
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
pub async fn fetch_latest(repo: &str) -> anyhow::Result<String> {
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

#[derive(Deserialize)]
pub struct SystemLogQuery {
    pub kind: String,
    pub lines: Option<u32>,
}

pub const SYSTEM_LOG_KINDS: [&str; 5] = ["panel", "nginx", "ssh", "mariadb", "redis"];

/// GET /api/logs/system?kind=panel|nginx|ssh|mariadb|redis&lines=200
pub async fn system_log(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<SystemLogQuery>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    if !SYSTEM_LOG_KINDS.contains(&q.kind.as_str()) {
        return Err(crate::error::AppError::bad("로그 종류가 올바르지 않습니다"));
    }
    let n = q.lines.unwrap_or(200).clamp(10, 2000).to_string();
    let out = crate::deploy::helper::call_quiet(&state, &["system-log", &q.kind, &n]).await?;
    Ok(Json(
        json!({ "kind": q.kind, "lines": out.stdout.lines().collect::<Vec<_>>() }),
    ))
}

#[derive(Deserialize)]
pub struct LimitQuery {
    pub limit: Option<u32>,
}

/// GET /api/logs/deployments?limit=100 — 모든 앱의 배포·적용 기록
pub async fn deployment_log(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<LimitQuery>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    Ok(Json(json!(
        deployments::recent(&state, q.limit.unwrap_or(100).clamp(1, 500)).await?
    )))
}

/// GET /api/system/events?limit=150 — 패널·nginx·DB·Redis·cron 과 시스템 오류 기록 (journald + nginx error.log)
pub async fn events(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<LimitQuery>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let limit = q.limit.unwrap_or(150).clamp(1, 1000);
    let n = limit.to_string();
    let ja = ["system-events", n.as_str()];
    let na = ["system-log", "nginx", n.as_str()];
    let (journal, nginx) = tokio::join!(
        crate::deploy::helper::call_quiet(&state, &ja),
        crate::deploy::helper::call_quiet(&state, &na),
    );
    let journal = journal.map(|o| o.stdout).unwrap_or_default();
    let nginx = nginx.map(|o| o.stdout).unwrap_or_default();
    let events = crate::sysevents::merge(&journal, &nginx, limit as usize);
    let summary = crate::sysevents::summary(&events);
    Ok(Json(json!({ "events": events, "summary": summary })))
}

/// GET /api/system/processes — CPU·메모리 상위 프로세스
pub async fn processes(user: AuthUser) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let (list, summary) = tokio::task::spawn_blocking(|| crate::sysevents::top_processes(40))
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    Ok(Json(json!({ "processes": list, "summary": summary })))
}
