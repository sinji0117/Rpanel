//! 보안 메뉴 — 보안 점검 · 방화벽(ufw) · 로그인 보안(실패 기록 · 차단 해제)
//!
//! 방화벽과 서버 점검은 rpanel-helper(root) 로만 합니다. helper 가 다시 검사해서
//! SSH · 패널 · 80 · 443 포트의 허용 규칙은 지우지 못하게 막습니다 (스스로 잠기는 것 방지).

use std::net::IpAddr;

use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    audit,
    auth::{session, twofa, AuthUser, ClientIp},
    config::EdgeMode,
    deploy::{helper, logsink::LogSink},
    error::{AppError, AppResult},
    models::App,
    routes::panel_access,
    security::{cert_days_left, parse_kv, parse_ufw, valid_port_spec, FwStatus},
    state::AppState,
};

/// 로그인 시도 제한 (auth::handlers::login 과 같은 값): 5분에 10회
const LOGIN_LIMIT: u64 = 10;

/// helper 오류에서 사람이 읽을 부분만 ("rpanel-helper: …")
fn helper_error(e: &anyhow::Error) -> String {
    let s = format!("{e:#}");
    match s.rfind("rpanel-helper: ") {
        Some(i) => s[i + "rpanel-helper: ".len()..]
            .split(" / ")
            .next()
            .unwrap_or("")
            .to_string(),
        None => s,
    }
}

async fn run(state: &AppState, args: &[&str]) -> AppResult<()> {
    let log = LogSink::capture();
    helper::call(state, args, &log)
        .await
        .map(|_| ())
        .map_err(|e| AppError::bad(helper_error(&e)))
}

/// 주소 하나 또는 대역 → 표준 표기
fn norm_cidr(s: &str) -> AppResult<String> {
    let s = s.trim();
    let (addr, bits) = panel_access::parse_cidr(s).ok_or_else(|| {
        AppError::bad(format!(
            "IP 형식이 올바르지 않습니다: {s} (예: 203.0.113.5, 10.0.0.0/24)"
        ))
    })?;
    Ok(
        if (addr.is_ipv4() && bits == 32) || (addr.is_ipv6() && bits == 128) {
            addr.to_string()
        } else {
            format!("{addr}/{bits}")
        },
    )
}

// ───────────────────────── 방화벽 ─────────────────────────

struct Firewall {
    st: FwStatus,
    ssh_port: u16,
    panel_port: u16,
}

async fn firewall_state(state: &AppState) -> AppResult<Firewall> {
    let out = helper::call_quiet(state, &["firewall-status"])
        .await
        .map_err(|e| AppError::bad(helper_error(&e)))?;
    if out.code != 0 {
        return Err(AppError::bad(format!(
            "방화벽 상태를 읽지 못했습니다: {}",
            out.stderr.trim()
        )));
    }
    let kv = parse_kv(&out.stdout);
    let port = |k: &str, d: u16| kv.get(k).and_then(|v| v.parse().ok()).unwrap_or(d);
    Ok(Firewall {
        st: parse_ufw(&out.stdout),
        ssh_port: port("ssh_port", 22),
        panel_port: port("panel_port", 8443),
    })
}

fn firewall_json(fw: &Firewall) -> Value {
    let protected = [fw.ssh_port, fw.panel_port, 80, 443];
    let rules: Vec<Value> = fw
        .st
        .rules
        .iter()
        .map(|r| {
            json!({
                "num": r.num,
                "to": r.to,
                "action": r.action,
                "from": r.from,
                "v6": r.v6,
                "comment": r.comment,
                "protected": r.is_allow() && protected.iter().any(|p| r.covers(*p)),
            })
        })
        .collect();
    json!({
        "installed": fw.st.installed,
        "active": fw.st.active,
        "ssh_port": fw.ssh_port,
        "panel_port": fw.panel_port,
        "protected": protected,
        "rules": rules,
        "added": fw.st.added,
    })
}

/// GET /api/security/firewall
pub async fn firewall(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<Value>> {
    user.require_admin()?;
    Ok(Json(firewall_json(&firewall_state(&state).await?)))
}

#[derive(Deserialize)]
pub struct AllowInput {
    pub port: String,
    #[serde(default = "any")]
    pub proto: String,
    /// 비우면 어디서나
    #[serde(default)]
    pub from: String,
}

fn any() -> String {
    "any".into()
}

/// POST /api/security/firewall/allow — 포트 열기
pub async fn allow(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Json(b): Json<AllowInput>,
) -> AppResult<Json<Value>> {
    user.require_admin()?;
    let (a, z) = valid_port_spec(&b.port)
        .ok_or_else(|| AppError::bad("포트는 3000 또는 3000:3010 처럼 1~65535 사이로 넣으세요"))?;
    let proto = b.proto.trim().to_ascii_lowercase();
    if !matches!(proto.as_str(), "tcp" | "udp" | "any") {
        return Err(AppError::bad("프로토콜은 tcp · udp · any 중 하나입니다"));
    }
    if a != z && proto == "any" {
        return Err(AppError::bad("포트 범위는 tcp 또는 udp 를 골라야 합니다"));
    }
    let port = if a == z {
        a.to_string()
    } else {
        format!("{a}:{z}")
    };
    let from = if b.from.trim().is_empty() || b.from.trim() == "any" {
        "any".to_string()
    } else {
        norm_cidr(&b.from)?
    };
    run(&state, &["firewall-allow", &port, &proto, &from]).await?;
    audit::log(
        &state,
        Some(user.id),
        "firewall.allow",
        &format!("{port}/{proto} from {from}"),
        &ip,
    )
    .await;
    Ok(Json(firewall_json(&firewall_state(&state).await?)))
}

#[derive(Deserialize)]
pub struct BlockInput {
    pub ip: String,
}

/// POST /api/security/firewall/block — 이 주소의 모든 접속 차단
pub async fn block(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Json(b): Json<BlockInput>,
) -> AppResult<Json<Value>> {
    user.require_admin()?;
    let cidr = norm_cidr(&b.ip)?;
    if let Ok(me) = ip.parse::<IpAddr>() {
        if panel_access::contains(&cidr, me) {
            return Err(AppError::bad(format!(
                "지금 접속한 IP {me} 가 들어 있어 막을 수 없습니다"
            )));
        }
    }
    run(&state, &["firewall-block", &cidr]).await?;
    audit::log(&state, Some(user.id), "firewall.block", &cidr, &ip).await;
    Ok(Json(firewall_json(&firewall_state(&state).await?)))
}

#[derive(Deserialize)]
pub struct DeleteInput {
    pub num: u32,
    /// 화면에 보였던 대상 — 그 사이 번호가 바뀌었으면 지우지 않음
    #[serde(default)]
    pub to: String,
}

/// POST /api/security/firewall/delete — 규칙 지우기
pub async fn delete_rule(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Json(b): Json<DeleteInput>,
) -> AppResult<Json<Value>> {
    user.require_admin()?;
    let num = b.num.to_string();
    let to = b.to.trim().to_string();
    let mut args = vec!["firewall-delete", num.as_str()];
    if !to.is_empty() {
        args.push(to.as_str());
    }
    run(&state, &args).await?;
    audit::log(
        &state,
        Some(user.id),
        "firewall.delete",
        &format!("#{num} {to}"),
        &ip,
    )
    .await;
    Ok(Json(firewall_json(&firewall_state(&state).await?)))
}

/// POST /api/security/firewall/enable
pub async fn enable(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
) -> AppResult<Json<Value>> {
    user.require_admin()?;
    run(&state, &["firewall-enable"]).await?;
    audit::log(&state, Some(user.id), "firewall.enable", "ufw", &ip).await;
    Ok(Json(firewall_json(&firewall_state(&state).await?)))
}

/// POST /api/security/firewall/disable
pub async fn disable(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
) -> AppResult<Json<Value>> {
    user.require_admin()?;
    run(&state, &["firewall-disable"]).await?;
    audit::log(&state, Some(user.id), "firewall.disable", "ufw", &ip).await;
    Ok(Json(firewall_json(&firewall_state(&state).await?)))
}

// ───────────────────────── 로그인 보안 ─────────────────────────

/// 로그인 시도 카운터 (Redis "rl:login:*") — 많은 순
async fn login_counters(state: &AppState) -> AppResult<Vec<Value>> {
    let mut r = state.redis.clone();
    let prefix = state.key(&["rl", ""]);
    let pattern = format!("{prefix}login:*");
    let mut cursor: u64 = 0;
    let mut keys: Vec<String> = Vec::new();
    loop {
        let (next, batch): (u64, Vec<String>) = redis::cmd("SCAN")
            .arg(cursor)
            .arg("MATCH")
            .arg(&pattern)
            .arg("COUNT")
            .arg(500)
            .query_async(&mut r)
            .await?;
        keys.extend(batch);
        cursor = next;
        if cursor == 0 || keys.len() >= 2000 {
            break;
        }
    }
    let mut out = Vec::new();
    for k in keys {
        let count: Option<u64> = redis::cmd("GET").arg(&k).query_async(&mut r).await?;
        let ttl: i64 = redis::cmd("TTL").arg(&k).query_async(&mut r).await?;
        let Some(count) = count else { continue };
        let bucket = k.strip_prefix(&prefix).unwrap_or(&k).to_string();
        let (kind, who) = match bucket.strip_prefix("login:") {
            Some(rest) => rest.split_once(':').unwrap_or((rest, "")),
            None => continue,
        };
        out.push(json!({
            "bucket": bucket,
            "kind": kind,
            "who": who,
            "count": count,
            "limit": LOGIN_LIMIT,
            "blocked": count >= LOGIN_LIMIT,
            "ttl": ttl.max(0),
        }));
    }
    out.sort_by(|a, b| b["count"].as_u64().cmp(&a["count"].as_u64()));
    out.truncate(200);
    Ok(out)
}

/// GET /api/security/logins — 최근 로그인 실패와 시도 제한에 걸린 IP·계정
pub async fn logins(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<Value>> {
    user.require_admin()?;
    let failures = crate::logstore::audit_query(
        &state,
        Some(&["auth.login_failed", "auth.otp_failed"][..]),
        100,
    )
    .await
    .map_err(AppError::Internal)?;
    Ok(Json(json!({
        "failures": failures,
        "counters": login_counters(&state).await?,
        "limit": LOGIN_LIMIT,
        "window_secs": 300,
    })))
}

#[derive(Deserialize)]
pub struct UnblockInput {
    pub bucket: String,
}

/// POST /api/security/unblock — 시도 제한 풀기
pub async fn unblock(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Json(b): Json<UnblockInput>,
) -> AppResult<Json<Value>> {
    user.require_admin()?;
    let bucket = b.bucket.trim();
    let ok = bucket.len() < 300
        && ["login:ip:", "login:acct:", "login:otp:"]
            .iter()
            .any(|p| bucket.starts_with(p));
    if !ok {
        return Err(AppError::bad("잘못된 항목입니다"));
    }
    session::reset_rate_limit(&state, bucket).await?;
    audit::log(&state, Some(user.id), "security.unblock", bucket, &ip).await;
    Ok(Json(json!({ "counters": login_counters(&state).await? })))
}

// ───────────────────────── 보안 점검 ─────────────────────────

fn kv_str<'a>(m: &'a std::collections::HashMap<String, String>, k: &str) -> &'a str {
    m.get(k).map(String::as_str).unwrap_or("")
}

fn item(id: &str, level: &str, data: Value) -> Value {
    json!({ "id": id, "level": level, "data": data })
}

/// 인증서가 있는 사이트들의 남은 날 — (이름, 남은 날)
async fn cert_days(state: &AppState, apps: &[App]) -> Vec<(String, i64)> {
    let now = twofa::now_unix() as i64;
    let jobs = apps
        .iter()
        .filter(|a| a.ssl_mode != "none")
        .take(40)
        .map(|a| async move {
            let out = helper::call_quiet(state, &["ssl-info", &a.slug])
                .await
                .ok()?;
            let certs = crate::routes::site_admin::parse_ssl_info(&out.stdout);
            let days = certs
                .as_array()?
                .iter()
                .filter_map(|c| c["not_after"].as_str())
                .filter_map(|s| cert_days_left(s, now))
                .min()?;
            Some((a.name.clone(), days))
        });
    futures::future::join_all(jobs)
        .await
        .into_iter()
        .flatten()
        .collect()
}

/// GET /api/security/check — 한눈에 보는 보안 점검
pub async fn check(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
) -> AppResult<Json<Value>> {
    user.require_admin()?;
    let mut items = Vec::new();

    // 서버: 방화벽 · SSH · 자동 업데이트 · fail2ban
    let scan = helper::call_quiet(&state, &["security-scan"])
        .await
        .ok()
        .filter(|o| o.code == 0)
        .map(|o| parse_kv(&o.stdout))
        .unwrap_or_default();
    let get = |k: &str| kv_str(&scan, k);
    if scan.is_empty() {
        items.push(item("server_scan", "info", json!({})));
    } else {
        let fw = get("ufw");
        items.push(item(
            "firewall",
            if fw == "active" { "ok" } else { "fail" },
            json!({ "state": fw }),
        ));
        let root = get("ssh_root");
        items.push(item(
            "ssh_root",
            match root {
                "yes" => "warn",
                "" => "info",
                _ => "ok",
            },
            json!({ "value": root }),
        ));
        let pw = get("ssh_password");
        let f2b = get("fail2ban");
        items.push(item(
            "ssh_password",
            match (pw, f2b) {
                ("no", _) => "ok",
                ("yes", "active") => "info",
                ("yes", _) => "warn",
                _ => "info",
            },
            json!({ "value": pw, "fail2ban": f2b }),
        ));
        items.push(item(
            "fail2ban",
            match f2b {
                "active" => "ok",
                _ if pw == "yes" => "warn",
                _ => "info",
            },
            json!({ "state": f2b }),
        ));
        let port = get("ssh_port");
        items.push(item(
            "ssh_port",
            if port == "22" { "info" } else { "ok" },
            json!({ "port": port }),
        ));
        let upd = get("unattended");
        items.push(item(
            "auto_updates",
            if upd == "on" { "ok" } else { "warn" },
            json!({ "state": upd }),
        ));
    }

    // 패널 접속 허용 IP
    let allow: Option<String> =
        sqlx::query_scalar("SELECT v FROM panel_settings WHERE k = 'panel_access'")
            .fetch_optional(&state.db)
            .await?;
    let restricted = allow
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v["allow"].as_array().map(|a| !a.is_empty()))
        .unwrap_or(false);
    items.push(item(
        "panel_access",
        if restricted { "ok" } else { "warn" },
        json!({ "my_ip": ip }),
    ));

    // 관리자 2단계 인증
    let (total, with_2fa): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), CAST(COALESCE(SUM(totp_enabled), 0) AS SIGNED) FROM users",
    )
    .fetch_one(&state.db)
    .await?;
    let me_2fa: bool = sqlx::query_scalar("SELECT totp_enabled FROM users WHERE id = ?")
        .bind(user.id)
        .fetch_one(&state.db)
        .await?;
    items.push(item(
        "two_factor",
        if total > 0 && with_2fa == total {
            "ok"
        } else {
            "warn"
        },
        json!({ "total": total, "enabled": with_2fa, "me": me_2fa }),
    ));

    // 사이트 HTTPS · 인증서 만료
    let apps = sqlx::query_as::<_, App>(&format!(
        "SELECT {} FROM apps WHERE domains <> '' ORDER BY name",
        crate::models::APP_COLUMNS
    ))
    .fetch_all(&state.db)
    .await?;
    let no_ssl: Vec<&str> = apps
        .iter()
        .filter(|a| a.ssl_mode == "none")
        .map(|a| a.name.as_str())
        .collect();
    let edge = !matches!(state.cfg.edge_mode, EdgeMode::Direct);
    items.push(item(
        "https",
        if no_ssl.is_empty() {
            "ok"
        } else if edge {
            "info"
        } else {
            "warn"
        },
        json!({ "sites": no_ssl, "total": apps.len(), "edge": edge }),
    ));
    let days = cert_days(&state, &apps).await;
    let expired: Vec<&String> = days.iter().filter(|d| d.1 < 0).map(|d| &d.0).collect();
    let soon: Vec<Value> = days
        .iter()
        .filter(|d| (0..14).contains(&d.1))
        .map(|d| json!({ "name": d.0, "days": d.1 }))
        .collect();
    if !days.is_empty() {
        items.push(item(
            "certificates",
            if !expired.is_empty() {
                "fail"
            } else if !soon.is_empty() {
                "warn"
            } else {
                "ok"
            },
            json!({ "expired": expired, "soon": soon, "total": days.len() }),
        ));
    }

    // 로그인 실패 · 차단
    let fails = crate::logstore::audit_query(
        &state,
        Some(&["auth.login_failed", "auth.otp_failed"][..]),
        500,
    )
    .await
    .map_err(AppError::Internal)?;
    let day_ago = chrono::Utc::now() - chrono::Duration::hours(24);
    let recent = fails.iter().filter(|f| f.created_at > day_ago).count();
    let blocked = login_counters(&state)
        .await?
        .iter()
        .filter(|c| c["blocked"].as_bool() == Some(true))
        .count();
    items.push(item(
        "login_failures",
        if recent >= 20 || blocked > 0 {
            "warn"
        } else {
            "ok"
        },
        json!({ "count": recent, "blocked": blocked }),
    ));

    let count = |lv: &str| items.iter().filter(|i| i["level"] == lv).count();
    Ok(Json(json!({
        "items": items,
        "summary": { "ok": count("ok"), "warn": count("warn"), "fail": count("fail"), "info": count("info") },
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_error_message() {
        let e = anyhow::anyhow!(
            "명령 실패 (exit 1): sudo -n /usr/local/lib/rpanel/rpanel-helper firewall-delete 2 — rpanel-helper: SSH·패널·웹(80·443) 포트를 여는 규칙은 지울 수 없습니다"
        );
        assert_eq!(
            helper_error(&e),
            "SSH·패널·웹(80·443) 포트를 여는 규칙은 지울 수 없습니다"
        );
        assert_eq!(helper_error(&anyhow::anyhow!("boom")), "boom");
    }

    #[test]
    fn cidr_normalized() {
        assert_eq!(norm_cidr(" 203.0.113.5 ").unwrap(), "203.0.113.5");
        assert_eq!(norm_cidr("10.0.0.0/24").unwrap(), "10.0.0.0/24");
        assert_eq!(norm_cidr("10.0.0.1/32").unwrap(), "10.0.0.1");
        assert!(norm_cidr("10.0.0.0/33").is_err());
        assert!(norm_cidr("x; rm").is_err());
    }
}
