//! 패널 접속 허용 IP — 이 IP·대역에서만 패널(:8443 · 패널 도메인)을 열 수 있게
//!
//! 실제 제한은 Nginx allow/deny (helper `panel-access`). Git 웹훅(/api/hooks/)은 예외입니다.
//! 지금 접속한 IP 가 빠진 목록은 거부합니다 (스스로 잠기는 것 방지).

use std::net::IpAddr;

use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    audit,
    auth::{AuthUser, ClientIp},
    deploy::{helper, logsink::LogSink},
    error::{AppError, AppResult},
    state::AppState,
};

const KEY: &str = "panel_access";

/// "10.0.0.0/24" · "1.2.3.4" · "2001:db8::/32" → (주소, 비트)
pub fn parse_cidr(s: &str) -> Option<(IpAddr, u8)> {
    let (ip, bits) = match s.split_once('/') {
        Some((a, b)) => (a, Some(b)),
        None => (s, None),
    };
    let ip: IpAddr = ip.parse().ok()?;
    let max = if ip.is_ipv4() { 32 } else { 128 };
    let bits = match bits {
        Some(b) => b.parse::<u8>().ok().filter(|b| *b <= max)?,
        None => max,
    };
    Some((ip, bits))
}

pub fn contains(cidr: &str, ip: IpAddr) -> bool {
    let Some((net, bits)) = parse_cidr(cidr) else {
        return false;
    };
    // IPv4-mapped IPv6 → IPv4
    let ip = match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(ip),
        v4 => v4,
    };
    match (net, ip) {
        (IpAddr::V4(n), IpAddr::V4(a)) => {
            let mask = if bits == 0 {
                0
            } else {
                u32::MAX << (32 - bits)
            };
            u32::from(n) & mask == u32::from(a) & mask
        }
        (IpAddr::V6(n), IpAddr::V6(a)) => {
            let mask = if bits == 0 {
                0
            } else {
                u128::MAX << (128 - bits)
            };
            u128::from(n) & mask == u128::from(a) & mask
        }
        _ => false,
    }
}

async fn load(state: &AppState) -> AppResult<Vec<String>> {
    let v: Option<String> = sqlx::query_scalar("SELECT v FROM panel_settings WHERE k = ?")
        .bind(KEY)
        .fetch_optional(&state.db)
        .await?;
    Ok(v.and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v["allow"].as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|x| x.as_str().map(String::from))
        .collect())
}

/// GET /api/system/panel-access
pub async fn get(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
) -> AppResult<Json<Value>> {
    user.require_admin()?;
    let allow = load(&state).await?;
    Ok(Json(
        json!({ "allow": allow, "my_ip": ip, "restricted": !allow.is_empty() }),
    ))
}

#[derive(Deserialize)]
pub struct Input {
    /// 비우면 제한 없음
    #[serde(default)]
    pub allow: Vec<String>,
}

/// PUT /api/system/panel-access
pub async fn set(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Json(input): Json<Input>,
) -> AppResult<Json<Value>> {
    user.require_admin()?;
    let mut allow: Vec<String> = Vec::new();
    for raw in &input.allow {
        let s = raw.trim();
        if s.is_empty() {
            continue;
        }
        let (addr, bits) = parse_cidr(s).ok_or_else(|| {
            AppError::bad(format!(
                "IP 형식이 올바르지 않습니다: {s} (예: 203.0.113.5, 10.0.0.0/24)"
            ))
        })?;
        let norm = if (addr.is_ipv4() && bits == 32) || (addr.is_ipv6() && bits == 128) {
            addr.to_string()
        } else {
            format!("{addr}/{bits}")
        };
        if !allow.contains(&norm) {
            allow.push(norm);
        }
    }
    if allow.len() > 50 {
        return Err(AppError::bad("허용 IP 는 50개까지입니다"));
    }
    if !allow.is_empty() {
        if let Ok(me) = ip.parse::<IpAddr>() {
            if !me.is_loopback() && !allow.iter().any(|c| contains(c, me)) {
                return Err(AppError::bad(format!(
                    "지금 접속한 IP {me} 가 목록에 없습니다 — 저장하면 패널에 접속할 수 없게 됩니다. 이 IP 를 먼저 추가하세요"
                )));
            }
        }
    }
    let arg = if allow.is_empty() {
        "any".to_string()
    } else {
        allow.join(",")
    };
    let log = LogSink::capture();
    helper::call(&state, &["panel-access", &arg], &log)
        .await
        .map_err(|e| AppError::bad(format!("접속 IP 적용 실패: {e}")))?;
    sqlx::query(
        "INSERT INTO panel_settings (k, v) VALUES (?, ?) ON DUPLICATE KEY UPDATE v = VALUES(v)",
    )
    .bind(KEY)
    .bind(json!({ "allow": allow }).to_string())
    .execute(&state.db)
    .await?;
    audit::log(&state, Some(user.id), "panel.access", &arg, &ip).await;
    Ok(Json(
        json!({ "allow": allow, "my_ip": ip, "restricted": !allow.is_empty() }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cidr() {
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        assert!(contains("10.0.0.0/24", ip("10.0.0.77")));
        assert!(!contains("10.0.0.0/24", ip("10.0.1.1")));
        assert!(contains("203.0.113.5", ip("203.0.113.5")));
        assert!(!contains("203.0.113.5", ip("203.0.113.6")));
        assert!(contains("0.0.0.0/0", ip("8.8.8.8")));
        assert!(contains("10.0.0.0/8", ip("::ffff:10.1.2.3")));
        assert!(contains("2001:db8::/32", ip("2001:db8:1::5")));
        assert!(!contains("2001:db8::/32", ip("2001:db9::1")));
        for bad in [
            "10.0.0.0/33",
            "300.1.1.1",
            "abc",
            "1.2.3.4/x",
            "1.2.3.4; deny",
        ] {
            assert!(parse_cidr(bad).is_none(), "{bad}");
        }
    }
}
