//! 사이트(도메인) 중심 화면 — 사이트 목록과 응답 확인
//! 도메인마다 이 서버의 Nginx(:80)에 Host 헤더로 요청해 실제 응답을 확인하고, Nginx 로그를 보여줍니다.

use std::{collections::HashMap, time::Instant};

use axum::{
    extract::{Path, Query, State},
    Json,
};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    auth::AuthUser,
    deploy::{helper, runner},
    error::{AppError, AppResult},
    models::{App, APP_COLUMNS},
    routes::apps,
    state::AppState,
    validate,
};

#[derive(Serialize)]
pub struct SiteCheck {
    pub domain: String,
    pub app_id: u64,
    pub app_name: String,
    pub app_slug: String,
    pub kind: String,
    pub source: String,
    pub https: bool,
    pub app_status: String,
    pub project_id: Option<u64>,
    pub http_status: Option<u16>,
    pub latency_ms: Option<u64>,
    pub error: Option<String>,
    pub last_deploy_status: Option<String>,
    pub last_deploy_at: Option<chrono::DateTime<chrono::Utc>>,
}

async fn check(domain: &str) -> (Option<u16>, Option<u64>, Option<String>) {
    let t = Instant::now();
    match runner::http_status_host(80, domain, "/").await {
        Ok(code) => (Some(code), Some(t.elapsed().as_millis() as u64), None),
        Err(e) => (None, None, Some(format!("{e:#}"))),
    }
}

/// GET /api/sites
pub async fn list(State(state): State<AppState>, _u: AuthUser) -> AppResult<Json<Vec<SiteCheck>>> {
    let apps_list = sqlx::query_as::<_, App>(&format!(
        "SELECT {APP_COLUMNS} FROM apps WHERE domains <> '' ORDER BY name"
    ))
    .fetch_all(&state.db)
    .await?;
    let last: Vec<(u64, String, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT d.app_id, d.status, d.created_at FROM deployments d \
         JOIN (SELECT app_id, MAX(id) AS id FROM deployments GROUP BY app_id) m ON m.id = d.id",
    )
    .fetch_all(&state.db)
    .await?;
    let last: HashMap<u64, (String, chrono::DateTime<chrono::Utc>)> =
        last.into_iter().map(|(a, s, t)| (a, (s, t))).collect();

    let mut targets = Vec::new();
    for app in &apps_list {
        for d in app.domain_list() {
            targets.push((d.to_string(), app));
        }
    }
    let dry = state.exec.is_dry_run();
    let checks = futures::future::join_all(targets.iter().map(|(d, _)| async move {
        if dry {
            (
                None,
                None,
                Some("dry-run 모드에서는 확인하지 않습니다".to_string()),
            )
        } else {
            check(d).await
        }
    }))
    .await;

    let out = targets
        .into_iter()
        .zip(checks)
        .map(|((domain, app), (code, ms, err))| SiteCheck {
            domain,
            app_id: app.id,
            app_name: app.name.clone(),
            app_slug: app.slug.clone(),
            kind: app.kind.clone(),
            source: app.source.clone(),
            https: app.has_ssl(),
            app_status: app.status.clone(),
            project_id: app.project_id,
            http_status: code,
            latency_ms: ms,
            error: err,
            last_deploy_status: last.get(&app.id).map(|l| l.0.clone()),
            last_deploy_at: last.get(&app.id).map(|l| l.1),
        })
        .collect();
    Ok(Json(out))
}

#[derive(Deserialize)]
pub struct LogQuery {
    pub kind: Option<String>,
    pub lines: Option<u32>,
}

#[derive(Debug, Default, Serialize, PartialEq)]
pub struct AccessStats {
    pub total: u64,
    pub by_class: HashMap<String, u64>,
    pub top_paths: Vec<(String, u64)>,
    pub top_ips: Vec<(String, u64)>,
    pub bytes: u64,
}

// combined: 1.2.3.4 - - [13/Oct/2026:11:00:00 +0900] "GET /path HTTP/1.1" 200 123 "ref" "ua"
static ACCESS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"^(\S+) \S+ \S+ \[[^\]]+\] "(?:[A-Z]+) (\S+)[^"]*" (\d{3}) (\d+|-)"#).unwrap()
});

pub fn access_stats(lines: &[&str]) -> AccessStats {
    let mut st = AccessStats::default();
    let mut paths: HashMap<String, u64> = HashMap::new();
    let mut ips: HashMap<String, u64> = HashMap::new();
    for l in lines {
        let Some(c) = ACCESS.captures(l) else {
            continue;
        };
        st.total += 1;
        let class = format!("{}xx", &c[3][..1]);
        *st.by_class.entry(class).or_default() += 1;
        let path = c[2].split('?').next().unwrap_or("").to_string();
        *paths.entry(path).or_default() += 1;
        *ips.entry(c[1].to_string()).or_default() += 1;
        st.bytes += c[4].parse::<u64>().unwrap_or(0);
    }
    let top = |m: HashMap<String, u64>| {
        let mut v: Vec<(String, u64)> = m.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v.truncate(8);
        v
    };
    st.top_paths = top(paths);
    st.top_ips = top(ips);
    st
}

/// GET /api/apps/{id}/nginx-logs?kind=access|error&lines=500
pub async fn logs(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<u64>,
    Query(q): Query<LogQuery>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let app = apps::fetch(&state, id).await?;
    validate::slug(&app.slug).map_err(AppError::BadRequest)?;
    let kind = match q.kind.as_deref().unwrap_or("access") {
        k @ ("access" | "error") => k,
        _ => return Err(AppError::bad("kind 는 access 또는 error")),
    };
    let n = q.lines.unwrap_or(500).clamp(10, 5000).to_string();
    let out = helper::call_quiet(&state, &["nginx-log", &app.slug, kind, &n]).await?;
    let lines: Vec<&str> = out.stdout.lines().collect();
    let stats = if kind == "access" {
        Some(access_stats(&lines))
    } else {
        None
    };
    Ok(Json(
        json!({ "kind": kind, "lines": lines, "stats": stats }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_combined_log() {
        let lines = [
            r#"203.0.113.5 - - [13/Oct/2026:11:00:00 +0900] "GET /api/items?page=2 HTTP/1.1" 200 512 "-" "Mozilla""#,
            r#"203.0.113.5 - - [13/Oct/2026:11:00:01 +0900] "GET /api/items HTTP/1.1" 200 100 "-" "Mozilla""#,
            r#"198.51.100.7 - - [13/Oct/2026:11:00:02 +0900] "POST /login HTTP/1.1" 401 20 "-" "curl""#,
            r#"198.51.100.7 - - [13/Oct/2026:11:00:03 +0900] "GET / HTTP/1.1" 502 - "-" "curl""#,
            "garbage line",
        ];
        let s = access_stats(&lines);
        assert_eq!(s.total, 4);
        assert_eq!(s.by_class.get("2xx"), Some(&2));
        assert_eq!(s.by_class.get("4xx"), Some(&1));
        assert_eq!(s.by_class.get("5xx"), Some(&1));
        assert_eq!(s.top_paths[0], ("/api/items".to_string(), 2));
        assert_eq!(s.bytes, 632);
        assert_eq!(s.top_ips.len(), 2);
    }
}
