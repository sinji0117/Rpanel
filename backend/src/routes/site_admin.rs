//! 사이트: 만들기(Rust 실행 파일·Python·HTML·리버스 프록시), 사이트 유저, SSL

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::json;

use crate::{
    audit,
    auth::{session::random_token, AuthUser, ClientIp},
    config::EdgeMode,
    deploy::{helper, logsink::LogSink, runner},
    error::{AppError, AppResult},
    models::{App, AppKind, Source},
    routes::{apps, deployments, projects},
    state::AppState,
    validate,
};

const RESERVED_USERS: &[&str] = &[
    "root",
    "daemon",
    "bin",
    "sys",
    "sync",
    "games",
    "man",
    "lp",
    "mail",
    "news",
    "uucp",
    "proxy",
    "www-data",
    "backup",
    "list",
    "irc",
    "nobody",
    "sshd",
    "syslog",
    "ubuntu",
    "admin",
    "administrator",
    "sysadmin",
    "mysql",
    "redis",
    "nginx",
    "rpanel",
    "rpanel-app",
    "postfix",
];

pub fn valid_site_user(u: &str) -> Result<(), String> {
    let ok = u.len() >= 3
        && u.len() <= 32
        && u.starts_with(|c: char| c.is_ascii_lowercase())
        && u.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !ok {
        return Err("사이트 유저는 소문자로 시작하는 소문자·숫자·- 3~32자입니다".into());
    }
    if RESERVED_USERS.contains(&u) {
        return Err(format!("{u} 은(는) 쓸 수 없는 이름입니다"));
    }
    Ok(())
}

/// 도메인 → 사이트 유저 후보 (www.beripick.co.kr → beripick)
pub fn user_from_domain(domain: &str) -> String {
    let d = domain.strip_prefix("www.").unwrap_or(domain);
    let first = d.split('.').next().unwrap_or("site");
    let mut u: String = first
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    u = u.trim_matches('-').to_string();
    if !u.starts_with(|c: char| c.is_ascii_lowercase()) {
        u = format!("site-{u}");
    }
    if u.len() < 3 {
        u = format!("{u}-site");
    }
    u.chars()
        .take(28)
        .collect::<String>()
        .trim_end_matches('-')
        .to_string()
}

fn system_users() -> Vec<String> {
    std::fs::read_to_string("/etc/passwd")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split(':').next().map(str::to_string))
        .collect()
}

async fn pick_site_user(
    state: &AppState,
    wanted: Option<String>,
    domain: &str,
) -> AppResult<String> {
    let taken_db: Vec<String> =
        sqlx::query_scalar("SELECT site_user FROM apps WHERE site_user IS NOT NULL")
            .fetch_all(&state.db)
            .await?;
    let sys = system_users();
    let taken = |u: &str| taken_db.iter().any(|t| t == u) || sys.iter().any(|t| t == u);
    if let Some(u) = wanted {
        valid_site_user(&u).map_err(AppError::BadRequest)?;
        if taken(&u) {
            return Err(AppError::Conflict(format!("사용자 {u} 는 이미 있습니다")));
        }
        return Ok(u);
    }
    let base = user_from_domain(domain);
    for i in 1..100 {
        let u = if i == 1 {
            base.clone()
        } else {
            format!("{base}{i}")
        };
        if valid_site_user(&u).is_ok() && !taken(&u) {
            return Ok(u);
        }
    }
    Err(AppError::Conflict(
        "사이트 유저 이름을 정할 수 없습니다. 직접 입력하세요".into(),
    ))
}

async fn unique_slug(state: &AppState, base: &str) -> AppResult<String> {
    let base: String = base
        .chars()
        .take(36)
        .collect::<String>()
        .trim_end_matches('-')
        .to_string();
    for i in 1..100 {
        let s = if i == 1 {
            base.clone()
        } else {
            format!("{base}-{i}")
        };
        let exists: Option<(u64,)> = sqlx::query_as("SELECT id FROM apps WHERE slug = ?")
            .bind(&s)
            .fetch_optional(&state.db)
            .await?;
        if exists.is_none() && validate::slug(&s).is_ok() {
            return Ok(s);
        }
    }
    Err(AppError::Conflict("slug 를 만들 수 없습니다".into()))
}

/// URL 검증 (리버스 프록시 대상)
pub fn valid_proxy_target(s: &str) -> Result<(), String> {
    static RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
        regex::Regex::new(r"^https?://[A-Za-z0-9.-]+(:[0-9]{1,5})?(/[A-Za-z0-9._~/-]*)?$").unwrap()
    });
    if s.len() <= 300 && RE.is_match(s) {
        Ok(())
    } else {
        Err("대상 URL 형식: http://127.0.0.1:3000 또는 https://10.0.0.11:8443/path".into())
    }
}

#[derive(Deserialize)]
pub struct CreateSite {
    /// rust | html | node | proxy
    #[serde(rename = "type")]
    pub site_type: String,
    pub domains: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub site_user: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub start_cmd: Option<String>,
    #[serde(default)]
    pub proxy_target: Option<String>,
    #[serde(default)]
    pub project_id: Option<u64>,
}

fn opt(s: &Option<String>) -> Option<String> {
    s.as_ref()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// POST /api/sites — 사이트 만들기
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Json(b): Json<CreateSite>,
) -> AppResult<(StatusCode, Json<serde_json::Value>)> {
    user.require_admin()?;
    let (kind, source) = match b.site_type.as_str() {
        "html" | "static" => (AppKind::Static, Source::Files),
        "rust" => (AppKind::Rust, Source::Files),
        "python" => (AppKind::Python, Source::Files),
        "proxy" => (AppKind::Proxy, Source::None),
        _ => {
            return Err(AppError::bad(
                "type 은 rust / python / html / proxy 중 하나입니다",
            ))
        }
    };
    let domains = validate::domains(&b.domains).map_err(AppError::BadRequest)?;
    let primary = domains
        .split_whitespace()
        .next()
        .ok_or_else(|| AppError::bad("도메인을 하나 이상 입력하세요"))?
        .to_string();
    apps::check_domain_conflicts(&state, &domains, None).await?;
    if let Some(pid) = b.project_id {
        projects::fetch(&state, pid).await?;
    }
    let name = opt(&b.name).unwrap_or_else(|| primary.clone());
    validate::name(&name).map_err(AppError::BadRequest)?;

    let start_cmd = if kind.is_service() {
        let default = if kind == AppKind::Python {
            crate::deploy::templates::PYTHON_DEFAULT_START
        } else {
            "./app"
        };
        let c = opt(&b.start_cmd).unwrap_or_else(|| default.into());
        validate::command(&c).map_err(AppError::BadRequest)?;
        Some(c)
    } else {
        None
    };
    let proxy_target = if kind == AppKind::Proxy {
        let t =
            opt(&b.proxy_target).ok_or_else(|| AppError::bad("프록시 대상 URL 을 입력하세요"))?;
        valid_proxy_target(&t).map_err(AppError::BadRequest)?;
        Some(t)
    } else {
        None
    };
    let password = match opt(&b.password) {
        Some(p) => {
            validate::password(&p).map_err(AppError::BadRequest)?;
            if p.contains(':') {
                return Err(AppError::bad("비밀번호에 ':' 를 쓸 수 없습니다"));
            }
            Some(p)
        }
        None => None,
    };

    // slug·사이트 유저·포트는 동시 생성 경쟁이 있으므로 Redis 락으로 직렬화
    let mut r = state.redis.clone();
    let lock_key = state.key(&["lock", "port"]);
    let got: Option<String> = redis::cmd("SET")
        .arg(&lock_key)
        .arg(1)
        .arg("NX")
        .arg("EX")
        .arg(30)
        .query_async(&mut r)
        .await?;
    if got.is_none() {
        return Err(AppError::Conflict(
            "다른 사이트를 만드는 중입니다. 잠시 후 다시 시도하세요".into(),
        ));
    }
    let result: AppResult<(u64, Option<String>)> = async {
        let slug = unique_slug(&state, &validate::slugify(&primary)).await?;
        let site_user = if source == Source::Files {
            Some(pick_site_user(&state, opt(&b.site_user), &primary).await?)
        } else {
            None
        };
        let site_root = site_user.as_ref().map(|u| format!("/home/{u}/htdocs/{primary}"));
        let port = if kind.is_service() { Some(apps::allocate_port(&state).await?) } else { None };
        let res = sqlx::query(
            "INSERT INTO apps (project_id, name, slug, kind, source, git_url, git_branch, start_cmd, port, domains, \
             health_path, webhook_token, site_user, site_root, proxy_target) \
             VALUES (?, ?, ?, ?, ?, NULL, 'main', ?, ?, ?, '/', ?, ?, ?, ?)",
        )
        .bind(b.project_id)
        .bind(&name)
        .bind(&slug)
        .bind(kind.as_str())
        .bind(source.as_str())
        .bind(&start_cmd)
        .bind(port)
        .bind(&domains)
        .bind(random_token(24))
        .bind(&site_user)
        .bind(&site_root)
        .bind(&proxy_target)
        .execute(&state.db)
        .await?;
        let id = res.last_insert_id();

        // 사이트 유저 + 홈 + 기본 페이지
        if let Some(u) = &site_user {
            let log = LogSink::capture();
            let helper_kind = match kind {
                AppKind::Rust => "rust",
                AppKind::Python => "python",
                _ => "static",
            };
            if let Err(e) = helper::call(&state, &["site-create", &slug, u, &primary, helper_kind], &log).await {
                sqlx::query("DELETE FROM apps WHERE id = ?").bind(id).execute(&state.db).await?;
                return Err(AppError::BadRequest(format!("{e:#}")));
            }
        }
        // 비밀번호 (없으면 생성해서 한 번만 보여줌)
        let mut shown = None;
        if site_user.is_some() {
            let pw = match &password {
                Some(p) => p.clone(),
                None => {
                    let p = random_token(10);
                    shown = Some(p.clone());
                    p
                }
            };
            let log = LogSink::capture();
            if let Err(e) = helper::call_stdin(&state, &["site-password", &slug], &pw, &log).await {
                tracing::warn!(error = %e, "site password set failed");
            }
        }
        Ok((id, shown))
    }
    .await;
    let _: () = redis::AsyncCommands::del(&mut r, &lock_key).await?;
    let (id, generated_password) = result?;
    let app = apps::fetch(&state, id).await?;
    audit::log(
        &state,
        Some(user.id),
        "site.create",
        &format!("{} ({})", app.slug, b.site_type),
        &ip,
    )
    .await;
    let deployment_id = deployments::enqueue(&state, id, Some(user.id)).await.ok();
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "app": app,
            "deployment_id": deployment_id,
            "site_user": app.site_user,
            "generated_password": generated_password,
        })),
    ))
}

#[derive(Deserialize)]
pub struct PasswordBody {
    pub password: String,
}

/// PUT /api/apps/{id}/site-user/password
pub async fn set_password(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
    Json(b): Json<PasswordBody>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let app = apps::fetch(&state, id).await?;
    let su = app
        .site_user
        .clone()
        .ok_or_else(|| AppError::bad("사이트 유저가 없는 앱입니다"))?;
    validate::password(&b.password).map_err(AppError::BadRequest)?;
    if b.password.contains(':') || b.password.contains('\n') {
        return Err(AppError::bad("비밀번호에 ':' 나 줄바꿈을 쓸 수 없습니다"));
    }
    let log = LogSink::capture();
    helper::call_stdin(&state, &["site-password", &app.slug], &b.password, &log)
        .await
        .map_err(|e| AppError::BadRequest(format!("{e:#}")))?;
    audit::log(&state, Some(user.id), "site.password", &su, &ip).await;
    Ok(Json(json!({ "ok": true })))
}

// ───────────── SSL ─────────────

fn parse_ssl_info(out: &str) -> serde_json::Value {
    let mut certs = Vec::new();
    let mut cur: Option<serde_json::Map<String, serde_json::Value>> = None;
    for line in out.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        if k == "path" {
            if let Some(c) = cur.take() {
                certs.push(serde_json::Value::Object(c));
            }
            cur = Some(serde_json::Map::new());
        }
        if let Some(c) = cur.as_mut() {
            if k == "san" {
                let list: Vec<serde_json::Value> = v
                    .split(',')
                    .filter_map(|x| x.strip_prefix("DNS:"))
                    .map(|x| json!(x))
                    .collect();
                c.insert("domains".into(), json!(list));
            } else {
                c.insert(k.into(), json!(v.trim()));
            }
        }
    }
    if let Some(c) = cur.take() {
        certs.push(serde_json::Value::Object(c));
    }
    json!(certs)
}

/// GET /api/apps/{id}/ssl
pub async fn ssl_get(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<u64>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let app = apps::fetch(&state, id).await?;
    let certs = match helper::call_quiet(&state, &["ssl-info", &app.slug]).await {
        Ok(o) => parse_ssl_info(&o.stdout),
        Err(_) => json!([]),
    };
    Ok(Json(json!({
        "mode": app.ssl_mode,
        "force_https": app.force_https,
        "certificates": certs,
        "edge_mode": state.cfg.edge_mode,
        "domains": app.domain_list(),
    })))
}

async fn reapply_nginx(state: &AppState, app_id: u64) -> AppResult<App> {
    let app = apps::fetch(state, app_id).await?;
    let log = LogSink::capture();
    runner::apply_nginx(state, &app, &log)
        .await
        .map_err(|e| AppError::BadRequest(format!("{e:#}")))?;
    Ok(app)
}

async fn set_ssl(state: &AppState, id: u64, mode: &str, force: bool) -> AppResult<()> {
    sqlx::query("UPDATE apps SET ssl_mode = ?, force_https = ? WHERE id = ?")
        .bind(mode)
        .bind(force)
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(())
}

#[derive(Deserialize)]
pub struct LeBody {
    pub email: String,
    #[serde(default = "yes")]
    pub force_https: bool,
}
fn yes() -> bool {
    true
}

/// POST /api/apps/{id}/ssl/letsencrypt
pub async fn ssl_letsencrypt(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
    Json(b): Json<LeBody>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let app = apps::fetch(&state, id).await?;
    validate::email(b.email.trim()).map_err(AppError::BadRequest)?;
    let domains: Vec<String> = app.domain_list().iter().map(|s| s.to_string()).collect();
    if domains.is_empty() {
        return Err(AppError::bad("도메인이 없습니다"));
    }
    // 1) ACME 경로가 열린 :80 설정이 먼저 있어야 함
    reapply_nginx(&state, id).await?;
    // 2) 발급
    let mut args: Vec<&str> = vec!["ssl-issue", &app.slug, b.email.trim()];
    args.extend(domains.iter().map(|s| s.as_str()));
    let log = LogSink::capture();
    helper::call_long(&state, &args, &log, 300)
        .await
        .map_err(|e| AppError::BadRequest(format!("{e:#}")))?;
    // 3) 443 적용 (실패하면 원복)
    let (prev_mode, prev_force) = (app.ssl_mode.clone(), app.force_https);
    set_ssl(&state, id, "letsencrypt", b.force_https).await?;
    if let Err(e) = reapply_nginx(&state, id).await {
        set_ssl(&state, id, &prev_mode, prev_force).await?;
        let _ = reapply_nginx(&state, id).await;
        return Err(e);
    }
    audit::log(&state, Some(user.id), "ssl.letsencrypt", &app.slug, &ip).await;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct CustomBody {
    pub certificate: String,
    pub private_key: String,
    #[serde(default = "yes")]
    pub force_https: bool,
}

/// POST /api/apps/{id}/ssl/custom — 가지고 있는 인증서(PEM) 설치
pub async fn ssl_custom(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
    Json(b): Json<CustomBody>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let app = apps::fetch(&state, id).await?;
    if !b.certificate.contains("-----BEGIN CERTIFICATE-----")
        || !b.private_key.contains("PRIVATE KEY-----")
    {
        return Err(AppError::bad("PEM 형식의 인증서와 개인키를 붙여넣으세요"));
    }
    if b.certificate.len() > 60_000 || b.private_key.len() > 20_000 {
        return Err(AppError::bad("인증서가 너무 큽니다"));
    }
    let staging = state.cfg.data_dir.join("staging");
    let log = LogSink::capture();
    state.exec.mkdirs(&staging, &log).await?;
    state
        .exec
        .write_file(
            &staging.join(format!("{}.crt", app.slug)),
            &b.certificate,
            0o600,
            &log,
        )
        .await?;
    state
        .exec
        .write_file(
            &staging.join(format!("{}.key", app.slug)),
            &b.private_key,
            0o600,
            &log,
        )
        .await?;
    helper::call(&state, &["ssl-custom", &app.slug], &log)
        .await
        .map_err(|e| AppError::BadRequest(format!("{e:#}")))?;
    let (prev_mode, prev_force) = (app.ssl_mode.clone(), app.force_https);
    set_ssl(&state, id, "custom", b.force_https).await?;
    if let Err(e) = reapply_nginx(&state, id).await {
        set_ssl(&state, id, &prev_mode, prev_force).await?;
        let _ = reapply_nginx(&state, id).await;
        return Err(e);
    }
    audit::log(&state, Some(user.id), "ssl.custom", &app.slug, &ip).await;
    Ok(Json(json!({ "ok": true })))
}

/// DELETE /api/apps/{id}/ssl — HTTPS 끄고 인증서 삭제
pub async fn ssl_delete(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
) -> AppResult<StatusCode> {
    user.require_admin()?;
    let app = apps::fetch(&state, id).await?;
    set_ssl(&state, id, "none", false).await?;
    reapply_nginx(&state, id).await?;
    let log = LogSink::capture();
    let _ = helper::call(&state, &["ssl-remove", &app.slug], &log).await;
    audit::log(&state, Some(user.id), "ssl.remove", &app.slug, &ip).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct ForceBody {
    pub force_https: bool,
}

/// PUT /api/apps/{id}/ssl/force
pub async fn ssl_force(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
    Json(b): Json<ForceBody>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let app = apps::fetch(&state, id).await?;
    if b.force_https && !app.has_ssl() {
        return Err(AppError::bad("인증서가 없어 HTTPS 를 강제할 수 없습니다"));
    }
    set_ssl(&state, id, &app.ssl_mode, b.force_https).await?;
    reapply_nginx(&state, id).await?;
    audit::log(
        &state,
        Some(user.id),
        "ssl.force",
        &format!("{} {}", app.slug, b.force_https),
        &ip,
    )
    .await;
    Ok(Json(json!({ "ok": true })))
}

/// GET /api/system/edge — 설치 모드 안내용
pub async fn edge(State(state): State<AppState>, _u: AuthUser) -> Json<serde_json::Value> {
    Json(json!({
        "edge_mode": state.cfg.edge_mode,
        "trusted_proxy": state.cfg.trusted_proxy,
        "ssl_available": state.cfg.edge_mode == EdgeMode::Direct,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_users() {
        assert_eq!(user_from_domain("beripick.com"), "beripick");
        assert_eq!(user_from_domain("www.bandiuri.co.kr"), "bandiuri");
        assert_eq!(user_from_domain("9shop.com"), "site-9shop");
        assert_eq!(user_from_domain("ab.io"), "ab-site");
        assert!(valid_site_user("beripick").is_ok());
        assert!(valid_site_user("root").is_err());
        assert!(valid_site_user("www-data").is_err());
        assert!(valid_site_user("Bad").is_err());
        assert!(valid_site_user("a;b").is_err());
    }

    #[test]
    fn proxy_targets() {
        assert!(valid_proxy_target("http://127.0.0.1:3000").is_ok());
        assert!(valid_proxy_target("https://10.0.0.11:8443/app").is_ok());
        assert!(valid_proxy_target("unix:/run/x.sock").is_err());
        assert!(valid_proxy_target("http://a b").is_err());
        assert!(valid_proxy_target("http://x;y").is_err());
    }

    #[test]
    fn ssl_info_parsing() {
        let out = "path=/etc/letsencrypt/live/rpanel-x/fullchain.pem\nnot_after=Jan 13 00:00:00 2027 GMT\nissuer=C = US, O = Let's Encrypt, CN = R11\nsubject=CN = a.com\nsan=DNS:a.com,DNS:www.a.com\n";
        let v = parse_ssl_info(out);
        assert_eq!(v[0]["not_after"], "Jan 13 00:00:00 2027 GMT");
        assert_eq!(v[0]["domains"], json!(["a.com", "www.a.com"]));
    }
}
