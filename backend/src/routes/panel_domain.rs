//! 패널 전용 도메인 — https://panel.example.com 으로 패널을 열 수 있게 (Let's Encrypt 자동 발급)
//!
//! 기존 `https://서버:8443` 접속은 그대로 두고, 443 에 도메인 하나를 더 붙입니다.

use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    audit,
    auth::{AuthUser, ClientIp},
    deploy::{helper, logsink::LogSink},
    error::{AppError, AppResult},
    routes::apps,
    state::AppState,
};

const KEY: &str = "panel_domain";

async fn load(state: &AppState) -> AppResult<Option<Value>> {
    let v: Option<String> = sqlx::query_scalar("SELECT v FROM panel_settings WHERE k = ?")
        .bind(KEY)
        .fetch_optional(&state.db)
        .await?;
    Ok(v.and_then(|s| serde_json::from_str(&s).ok()))
}

pub fn valid_domain(d: &str) -> bool {
    let re = regex::Regex::new(r"^([a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]{2,63}$").unwrap();
    d.len() <= 253 && re.is_match(d)
}

/// GET /api/system/panel-domain
pub async fn get(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<Value>> {
    user.require_admin()?;
    let cur = load(&state).await?;
    Ok(Json(json!({
        "domain": cur.as_ref().and_then(|v| v["domain"].as_str()),
        "email": cur.as_ref().and_then(|v| v["email"].as_str()),
        "url": cur.as_ref().and_then(|v| v["domain"].as_str()).map(|d| format!("https://{d}")),
    })))
}

#[derive(Deserialize)]
pub struct Input {
    pub domain: String,
    pub email: String,
}

/// PUT /api/system/panel-domain — 도메인 연결 + 인증서 발급
pub async fn set(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Json(input): Json<Input>,
) -> AppResult<Json<Value>> {
    user.require_admin()?;
    let domain = input
        .domain
        .trim()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let email = input.email.trim().to_string();
    if !valid_domain(&domain) {
        return Err(AppError::bad(
            "도메인 형식이 올바르지 않습니다 (예: panel.example.com)",
        ));
    }
    if email.len() > 190 || !email.contains('@') || email.contains(char::is_whitespace) {
        return Err(AppError::bad("인증서 알림을 받을 이메일을 넣으세요"));
    }
    apps::check_app_domains(&state, &domain, None).await?;
    let log = LogSink::capture();
    helper::call(&state, &["panel-domain-set", &domain, &email], &log)
        .await
        .map_err(|e| AppError::bad(format!("패널 도메인 연결 실패: {e}")))?;
    sqlx::query(
        "INSERT INTO panel_settings (k, v) VALUES (?, ?) ON DUPLICATE KEY UPDATE v = VALUES(v)",
    )
    .bind(KEY)
    .bind(json!({ "domain": domain, "email": email }).to_string())
    .execute(&state.db)
    .await?;
    audit::log(&state, Some(user.id), "panel.domain_set", &domain, &ip).await;
    Ok(Json(
        json!({ "domain": domain, "email": email, "url": format!("https://{domain}") }),
    ))
}

/// DELETE /api/system/panel-domain
pub async fn remove(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
) -> AppResult<StatusCode> {
    user.require_admin()?;
    let log = LogSink::capture();
    helper::call(&state, &["panel-domain-remove"], &log).await?;
    sqlx::query("DELETE FROM panel_settings WHERE k = ?")
        .bind(KEY)
        .execute(&state.db)
        .await?;
    audit::log(&state, Some(user.id), "panel.domain_remove", "", &ip).await;
    Ok(StatusCode::NO_CONTENT)
}

/// 앱 도메인으로 패널 도메인을 쓰지 못하게
pub async fn conflicts(state: &AppState, domains: &str) -> AppResult<()> {
    if let Some(v) = load(state).await? {
        if let Some(d) = v["domain"].as_str() {
            if domains.split_whitespace().any(|x| x == d) {
                return Err(AppError::Conflict(format!(
                    "도메인 {d} 은(는) 패널 전용 도메인입니다"
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn domains() {
        assert!(super::valid_domain("panel.example.com"));
        for bad in [
            "panel",
            "Panel.example.com",
            "a..b.com",
            "-a.com",
            "a.com;id",
            "a.com/x",
        ] {
            assert!(!super::valid_domain(bad), "{bad}");
        }
    }
}
