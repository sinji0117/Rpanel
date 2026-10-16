//! 사이트 계정 (SSH · SFTP)
//!
//! 사이트마다 기본 계정(사이트 유저) 하나와 추가 계정 여러 개를 둘 수 있습니다.
//! - 접속 방식: ssh(셸) / sftp(파일 전송만)
//! - 잠금: 비밀번호·키 모두 거부 (사이트 서비스는 그대로 동작)
//! - SSH 공개키: 계정별 authorized_keys
//!
//! 실제 리눅스 계정 작업은 `rpanel-helper site-account-*` 가 수행합니다.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    audit,
    auth::{session::random_token, AuthUser, ClientIp},
    deploy::{helper, logsink::LogSink},
    error::{AppError, AppResult},
    models::App,
    routes::{
        apps,
        site_admin::{system_users, valid_site_user},
    },
    state::AppState,
};

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct SiteAccount {
    pub id: u64,
    pub app_id: u64,
    pub username: String,
    pub is_primary: bool,
    /// ssh | sftp
    pub access: String,
    pub enabled: bool,
    pub ssh_keys: Option<String>,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

const COLUMNS: &str =
    "id, app_id, username, is_primary, access, enabled, ssh_keys, note, created_at, updated_at";

/// SSH 공개키 목록 검사 (helper 와 같은 규칙). 빈 줄·# 주석은 무시
pub fn check_ssh_keys(text: &str) -> Result<String, String> {
    static RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
        regex::Regex::new(
            r"^(ssh-ed25519|ssh-rsa|ecdsa-sha2-nistp(256|384|521)|sk-ssh-ed25519@openssh\.com|sk-ecdsa-sha2-nistp256@openssh\.com) [A-Za-z0-9+/]+={0,3}( [^\x00-\x1f\x7f]{0,200})?$",
        )
        .unwrap()
    });
    let mut out = Vec::new();
    for line in text.lines() {
        let l = line.trim_end_matches('\r').trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        if !RE.is_match(l) {
            let head: String = l.chars().take(40).collect();
            return Err(format!(
                "SSH 공개키 형식이 아닙니다: {head}… (ssh-ed25519 AAAA… 형태의 .pub 파일 내용을 한 줄에 하나씩)"
            ));
        }
        out.push(l.to_string());
    }
    if out.len() > 50 {
        return Err("SSH 키는 50개까지 등록할 수 있습니다".into());
    }
    Ok(out.join("\n"))
}

pub fn check_password(p: &str) -> Result<(), String> {
    let n = p.chars().count();
    if !(10..=128).contains(&n) {
        return Err("비밀번호는 10~128자여야 합니다".into());
    }
    if p.contains(':') || p.contains('\n') || p.contains('\r') {
        return Err("비밀번호에 ':' 나 줄바꿈을 쓸 수 없습니다".into());
    }
    Ok(())
}

fn check_access(a: &str) -> AppResult<()> {
    if a == "ssh" || a == "sftp" {
        Ok(())
    } else {
        Err(AppError::bad("접속 방식은 ssh 또는 sftp 입니다"))
    }
}

fn check_note(n: Option<String>) -> AppResult<Option<String>> {
    let n = n.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    if let Some(v) = &n {
        if v.chars().count() > 200 || v.chars().any(|c| c.is_control()) {
            return Err(AppError::bad("메모는 200자 이하로 입력하세요"));
        }
    }
    Ok(n)
}

async fn site_app(state: &AppState, id: u64) -> AppResult<App> {
    let app = apps::fetch(state, id).await?;
    if app.site_user.is_none() {
        return Err(AppError::bad(
            "사이트 계정은 파일 사이트(Rust·Python·HTML)에만 있습니다",
        ));
    }
    Ok(app)
}

async fn fetch(state: &AppState, app_id: u64, aid: u64) -> AppResult<SiteAccount> {
    sqlx::query_as::<_, SiteAccount>(&format!(
        "SELECT {COLUMNS} FROM site_accounts WHERE id = ? AND app_id = ?"
    ))
    .bind(aid)
    .bind(app_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)
}

fn helper_err(e: anyhow::Error) -> AppError {
    AppError::BadRequest(format!("{e:#}"))
}

/// GET /api/apps/{id}/accounts
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<u64>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let app = site_app(&state, id).await?;
    let rows = sqlx::query_as::<_, SiteAccount>(&format!(
        "SELECT {COLUMNS} FROM site_accounts WHERE app_id = ? ORDER BY is_primary DESC, id"
    ))
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    let accounts: Vec<serde_json::Value> = rows
        .iter()
        .map(|a| {
            let keys = a
                .ssh_keys
                .as_deref()
                .map(|k| k.lines().filter(|l| !l.trim().is_empty()).count())
                .unwrap_or(0);
            let mut v = serde_json::to_value(a).unwrap_or_default();
            v["key_count"] = json!(keys);
            v["home"] = json!(format!("/home/{}", a.username));
            v
        })
        .collect();
    Ok(Json(json!({
        "accounts": accounts,
        "site_root": app.site_root,
        "port": 22,
    })))
}

#[derive(Deserialize)]
pub struct CreateAccount {
    pub username: String,
    pub password: Option<String>,
    pub access: Option<String>,
    pub ssh_keys: Option<String>,
    pub note: Option<String>,
}

/// POST /api/apps/{id}/accounts — 추가 계정 만들기
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
    Json(b): Json<CreateAccount>,
) -> AppResult<(StatusCode, Json<serde_json::Value>)> {
    user.require_admin()?;
    let app = site_app(&state, id).await?;
    let username = b.username.trim().to_ascii_lowercase();
    valid_site_user(&username).map_err(AppError::BadRequest)?;
    let access = b.access.unwrap_or_else(|| "sftp".into());
    check_access(&access)?;
    let keys = match b.ssh_keys.as_deref() {
        Some(k) => check_ssh_keys(k).map_err(AppError::BadRequest)?,
        None => String::new(),
    };
    let note = check_note(b.note)?;
    // 비밀번호: 직접 입력 / 비우고 키만 / 둘 다 없으면 자동 생성해서 한 번 보여줌
    let password = match b.password.filter(|p| !p.is_empty()) {
        Some(p) => {
            check_password(&p).map_err(AppError::BadRequest)?;
            Some(p)
        }
        None if !keys.is_empty() => None,
        None => Some(random_token(10)),
    };
    let taken: Option<(u64,)> = sqlx::query_as(
        "SELECT id FROM site_accounts WHERE username = ? UNION SELECT id FROM apps WHERE site_user = ?",
    )
    .bind(&username)
    .bind(&username)
    .fetch_optional(&state.db)
    .await?;
    if taken.is_some() || system_users().iter().any(|u| u == &username) {
        return Err(AppError::Conflict(format!(
            "계정 이름 {username} 은(는) 이미 있습니다"
        )));
    }

    let log = LogSink::capture();
    helper::call_stdin(
        &state,
        &["site-account-create", &app.slug, &username, &access],
        &format!("{}\n", password.as_deref().unwrap_or("")),
        &log,
    )
    .await
    .map_err(helper_err)?;
    if !keys.is_empty() {
        helper::call_stdin(
            &state,
            &["site-account-keys", &app.slug, &username],
            &format!("{keys}\n"),
            &log,
        )
        .await
        .map_err(helper_err)?;
    }
    let res = sqlx::query(
        "INSERT INTO site_accounts (app_id, username, is_primary, access, ssh_keys, note) VALUES (?, ?, 0, ?, ?, ?)",
    )
    .bind(id)
    .bind(&username)
    .bind(&access)
    .bind(if keys.is_empty() { None } else { Some(&keys) })
    .bind(&note)
    .execute(&state.db)
    .await?;
    audit::log(
        &state,
        Some(user.id),
        "site.account.create",
        &format!("{username} ({access}) → {}", app.slug),
        &ip,
    )
    .await;
    let acc = fetch(&state, id, res.last_insert_id()).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "account": acc, "password": password })),
    ))
}

#[derive(Deserialize)]
pub struct UpdateAccount {
    pub access: Option<String>,
    pub enabled: Option<bool>,
    pub ssh_keys: Option<String>,
    pub note: Option<String>,
}

/// PUT /api/apps/{id}/accounts/{aid} — 접속 방식·잠금·SSH 키·메모
pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path((id, aid)): Path<(u64, u64)>,
    Json(b): Json<UpdateAccount>,
) -> AppResult<Json<SiteAccount>> {
    user.require_admin()?;
    let app = site_app(&state, id).await?;
    let acc = fetch(&state, id, aid).await?;
    let access = b.access.unwrap_or_else(|| acc.access.clone());
    check_access(&access)?;
    let enabled = b.enabled.unwrap_or(acc.enabled);
    let keys = match b.ssh_keys.as_deref() {
        Some(k) => Some(check_ssh_keys(k).map_err(AppError::BadRequest)?),
        None => None,
    };
    let note = check_note(b.note)?;
    let log = LogSink::capture();
    if access != acc.access || enabled != acc.enabled {
        helper::call(
            &state,
            &[
                "site-account-update",
                &app.slug,
                &acc.username,
                &access,
                if enabled { "on" } else { "off" },
            ],
            &log,
        )
        .await
        .map_err(helper_err)?;
    }
    if let Some(k) = &keys {
        if Some(k.as_str()) != acc.ssh_keys.as_deref().or(Some("")) {
            helper::call_stdin(
                &state,
                &["site-account-keys", &app.slug, &acc.username],
                &format!("{k}\n"),
                &log,
            )
            .await
            .map_err(helper_err)?;
        }
    }
    let keys_value = match keys {
        Some(k) if k.is_empty() => None,
        Some(k) => Some(k),
        None => acc.ssh_keys.clone(),
    };
    sqlx::query(
        "UPDATE site_accounts SET access = ?, enabled = ?, ssh_keys = ?, note = ? WHERE id = ?",
    )
    .bind(&access)
    .bind(enabled)
    .bind(&keys_value)
    .bind(&note)
    .bind(aid)
    .execute(&state.db)
    .await?;
    audit::log(
        &state,
        Some(user.id),
        "site.account.update",
        &format!(
            "{} ({access}, {})",
            acc.username,
            if enabled { "사용" } else { "잠금" }
        ),
        &ip,
    )
    .await;
    Ok(Json(fetch(&state, id, aid).await?))
}

#[derive(Deserialize, Default)]
pub struct NewPassword {
    pub password: Option<String>,
}

/// POST /api/apps/{id}/accounts/{aid}/password  (비우면 자동 생성해서 돌려줌)
pub async fn password(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path((id, aid)): Path<(u64, u64)>,
    body: Option<Json<NewPassword>>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let app = site_app(&state, id).await?;
    let acc = fetch(&state, id, aid).await?;
    let pw = match body.and_then(|b| b.0.password).filter(|p| !p.is_empty()) {
        Some(p) => {
            check_password(&p).map_err(AppError::BadRequest)?;
            p
        }
        None => random_token(10),
    };
    let log = LogSink::capture();
    helper::call_stdin(
        &state,
        &["site-account-password", &app.slug, &acc.username],
        &format!("{pw}\n"),
        &log,
    )
    .await
    .map_err(helper_err)?;
    audit::log(
        &state,
        Some(user.id),
        "site.account.password",
        &acc.username,
        &ip,
    )
    .await;
    Ok(Json(json!({ "ok": true, "password": pw })))
}

/// DELETE /api/apps/{id}/accounts/{aid} — 추가 계정만
pub async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path((id, aid)): Path<(u64, u64)>,
) -> AppResult<StatusCode> {
    user.require_admin()?;
    let app = site_app(&state, id).await?;
    let acc = fetch(&state, id, aid).await?;
    if acc.is_primary {
        return Err(AppError::bad(
            "기본 계정은 지울 수 없습니다 (사이트를 지우면 함께 지워집니다). 잠그려면 '잠금'을 쓰세요",
        ));
    }
    let log = LogSink::capture();
    helper::call(
        &state,
        &["site-account-delete", &app.slug, &acc.username],
        &log,
    )
    .await
    .map_err(helper_err)?;
    sqlx::query("DELETE FROM site_accounts WHERE id = ?")
        .bind(aid)
        .execute(&state.db)
        .await?;
    audit::log(
        &state,
        Some(user.id),
        "site.account.delete",
        &acc.username,
        &ip,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_passwords() {
        let k = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGq me@laptop";
        assert_eq!(check_ssh_keys(&format!("# 주석\n\n{k}\r\n")).unwrap(), k);
        assert!(check_ssh_keys("ssh-rsa AAAAB3Nza==").is_ok());
        assert!(check_ssh_keys("command=\"rm -rf /\" ssh-ed25519 AAAA x").is_err());
        assert!(check_ssh_keys("ssh-ed25519 AAAA x\u{1b}[2J").is_err());
        assert!(check_ssh_keys("not a key").is_err());
        assert_eq!(check_ssh_keys("").unwrap(), "");
        assert!(check_password("long-enough-pw").is_ok());
        assert!(check_password("short").is_err());
        assert!(check_password("has:colon-123").is_err());
        assert!(check_password("line\nbreak-1234").is_err());
    }
}
