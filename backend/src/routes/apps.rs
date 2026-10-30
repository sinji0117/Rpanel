use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    audit,
    auth::{session::random_token, AuthUser, ClientIp},
    deploy::{
        executor::{run_ok, CmdSpec},
        helper,
        logsink::LogSink,
        runner,
        templates::AppPaths,
    },
    error::{AppError, AppResult},
    models::{App, AppKind, EnvVar, Source, APP_COLUMNS},
    routes::projects,
    state::AppState,
    validate,
};

#[derive(Debug, Deserialize)]
pub struct AppInput {
    pub name: String,
    #[serde(default)]
    pub slug: Option<String>,
    pub kind: Option<String>,
    #[serde(default)]
    pub git_url: Option<String>,
    #[serde(default)]
    pub frontend_dir: Option<String>,
    /// Static 앱 용도: front | admin
    #[serde(default)]
    pub app_role: Option<String>,
    #[serde(default)]
    pub git_branch: Option<String>,
    #[serde(default)]
    pub root_dir: Option<String>,
    #[serde(default)]
    pub build_cmd: Option<String>,
    #[serde(default)]
    pub start_cmd: Option<String>,
    #[serde(default)]
    pub binary_name: Option<String>,
    #[serde(default)]
    pub output_dir: Option<String>,
    #[serde(default)]
    pub domains: Option<String>,
    #[serde(default)]
    pub api_proxy_port: Option<u32>,
    #[serde(default)]
    pub health_path: Option<String>,
    // ── 파일 기반 사이트 · 프록시 ──
    #[serde(default)]
    pub web_root: Option<String>,
    #[serde(default)]
    pub proxy_target: Option<String>,
    #[serde(default)]
    pub client_max_body_mb: Option<u32>,
}

/// 검증·정규화된 입력
#[derive(Debug)]
struct Clean {
    name: String,
    git_url: String,
    git_branch: String,
    root_dir: String,
    build_cmd: Option<String>,
    start_cmd: Option<String>,
    binary_name: Option<String>,
    output_dir: String,
    frontend_dir: Option<String>,
    app_role: Option<String>,
    domains: String,
    api_proxy_port: Option<u32>,
    health_path: String,
}

fn opt(s: &Option<String>) -> Option<String> {
    s.as_ref()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn clean(input: &AppInput, kind: AppKind) -> AppResult<Clean> {
    let bad = AppError::BadRequest;
    validate::name(&input.name).map_err(bad)?;
    let git_url = input.git_url.as_deref().unwrap_or("").trim().to_string();
    validate::git_url(&git_url).map_err(bad)?;
    let git_branch = opt(&input.git_branch).unwrap_or_else(|| "main".into());
    validate::branch(&git_branch).map_err(bad)?;
    let root_dir = opt(&input.root_dir)
        .unwrap_or_default()
        .trim_matches('/')
        .to_string();
    validate::rel_path(&root_dir).map_err(bad)?;
    let build_cmd = opt(&input.build_cmd);
    if let Some(c) = &build_cmd {
        validate::command(c).map_err(bad)?;
    }
    let start_cmd = opt(&input.start_cmd);
    if let Some(c) = &start_cmd {
        validate::command(c).map_err(bad)?;
    }
    let binary_name = opt(&input.binary_name);
    if let Some(b) = &binary_name {
        if kind == AppKind::Java {
            // Java: 빌드한 jar 경로 (앱 폴더 기준, 비우면 자동으로 찾음)
            validate::rel_path(b).map_err(bad)?;
            if !b.ends_with(".jar") {
                return Err(AppError::bad("jar 파일 경로는 .jar 로 끝나야 합니다"));
            }
        } else {
            validate::binary_name(b).map_err(bad)?;
        }
    }
    let output_dir = opt(&input.output_dir)
        .unwrap_or_else(|| "dist".into())
        .trim_matches('/')
        .to_string();
    validate::rel_path(&output_dir).map_err(bad)?;
    let frontend_dir = match kind {
        AppKind::Rust | AppKind::Python | AppKind::Node | AppKind::Java => opt(&input.frontend_dir)
            .map(|d| d.trim_matches('/').to_string())
            .filter(|d| !d.is_empty()),
        _ => None,
    };
    if let Some(d) = &frontend_dir {
        validate::rel_path(d).map_err(bad)?;
    }
    let app_role = match (kind, opt(&input.app_role).as_deref()) {
        (AppKind::Static, None | Some("front")) => Some("front".to_string()),
        (AppKind::Static, Some("admin")) => Some("admin".to_string()),
        (AppKind::Static, Some(_)) => {
            return Err(AppError::bad(
                "화면 용도는 front(사용자) 또는 admin(관리자) 입니다",
            ))
        }
        _ => None,
    };
    let domains = validate::domains(input.domains.as_deref().unwrap_or("")).map_err(bad)?;
    let api_proxy_port = match (kind, input.api_proxy_port) {
        (AppKind::Static, Some(p)) if (1..=65535).contains(&p) => Some(p),
        (AppKind::Static, Some(_)) => {
            return Err(AppError::bad("API 프록시 포트는 1~65535 이어야 합니다"))
        }
        _ => None,
    };
    let health_path = opt(&input.health_path).unwrap_or_else(|| "/".into());
    validate::url_path(&health_path).map_err(bad)?;
    Ok(Clean {
        name: input.name.trim().to_string(),
        git_url,
        git_branch,
        root_dir,
        build_cmd,
        // Git Python · Node.js · Java 앱은 시작 명령을 씀 (Rust 는 빌드한 바이너리를 바로 실행)
        start_cmd: if matches!(kind, AppKind::Python | AppKind::Node | AppKind::Java) {
            Some(start_cmd.unwrap_or_else(|| kind.default_start(Source::Git).into()))
        } else {
            None
        },
        binary_name: if matches!(kind, AppKind::Rust | AppKind::Java) {
            binary_name
        } else {
            None
        },
        output_dir,
        frontend_dir,
        app_role,
        domains,
        api_proxy_port,
        health_path,
    })
}

pub async fn fetch(state: &AppState, id: u64) -> AppResult<App> {
    sqlx::query_as::<_, App>(&format!("SELECT {APP_COLUMNS} FROM apps WHERE id = ?"))
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)
}

/// 다른 앱이나 패널 전용 도메인이 이미 쓰는 도메인 거부
pub async fn check_domain_conflicts(
    state: &AppState,
    domains: &str,
    exclude_id: Option<u64>,
) -> AppResult<()> {
    if domains.is_empty() {
        return Ok(());
    }
    crate::routes::panel_domain::conflicts(state, domains).await?;
    check_app_domains(state, domains, exclude_id).await
}

/// 다른 앱이 이미 쓰는 도메인 거부
pub async fn check_app_domains(
    state: &AppState,
    domains: &str,
    exclude_id: Option<u64>,
) -> AppResult<()> {
    let rows: Vec<(u64, String, String)> =
        sqlx::query_as("SELECT id, slug, domains FROM apps WHERE domains <> ''")
            .fetch_all(&state.db)
            .await?;
    for d in domains.split_whitespace() {
        for (id, slug, ds) in &rows {
            if Some(*id) != exclude_id && ds.split_whitespace().any(|x| x == d) {
                return Err(AppError::Conflict(format!(
                    "도메인 {d} 은(는) 이미 앱 '{slug}'에서 사용 중입니다"
                )));
            }
        }
    }
    Ok(())
}

/// 앱 포트 자동 할당: DB 미사용 + 실제로 bind 가능한 가장 낮은 포트
pub async fn allocate_port(state: &AppState) -> AppResult<u32> {
    let used: Vec<u32> = sqlx::query_scalar("SELECT port FROM apps WHERE port IS NOT NULL")
        .fetch_all(&state.db)
        .await?;
    pick_port(state.cfg.app_port_min, state.cfg.app_port_max, &used, |p| {
        std::net::TcpListener::bind(("127.0.0.1", p)).is_ok()
    })
    .ok_or_else(|| AppError::Conflict("할당 가능한 포트가 없습니다".into()))
}

pub fn pick_port(min: u16, max: u16, used: &[u32], is_free: impl Fn(u16) -> bool) -> Option<u32> {
    (min..=max)
        .find(|p| !used.contains(&(*p as u32)) && is_free(*p))
        .map(|p| p as u32)
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub project_id: Option<u64>,
}

pub async fn list(
    State(state): State<AppState>,
    _u: AuthUser,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<Vec<App>>> {
    let rows = match q.project_id {
        Some(pid) => {
            sqlx::query_as::<_, App>(&format!(
                "SELECT {APP_COLUMNS} FROM apps WHERE project_id = ? ORDER BY name"
            ))
            .bind(pid)
            .fetch_all(&state.db)
            .await?
        }
        None => {
            sqlx::query_as::<_, App>(&format!("SELECT {APP_COLUMNS} FROM apps ORDER BY name"))
                .fetch_all(&state.db)
                .await?
        }
    };
    Ok(Json(rows))
}

pub async fn get(
    State(state): State<AppState>,
    _u: AuthUser,
    Path(id): Path<u64>,
) -> AppResult<Json<App>> {
    Ok(Json(fetch(&state, id).await?))
}

pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(project_id): Path<u64>,
    Json(input): Json<AppInput>,
) -> AppResult<(StatusCode, Json<App>)> {
    user.require_admin()?;
    projects::fetch(&state, project_id).await?;
    let kind = input
        .kind
        .as_deref()
        .and_then(AppKind::parse)
        .filter(|k| {
            matches!(
                k,
                AppKind::Rust | AppKind::Python | AppKind::Node | AppKind::Java | AppKind::Static
            )
        })
        .ok_or_else(|| {
            AppError::bad("Git 앱의 kind 는 rust / python / node / java / static 중 하나입니다 (업로드형 사이트·프록시는 사이트 만들기에서)")
        })?;
    let c = clean(&input, kind)?;
    let slug = match opt(&input.slug) {
        Some(s) => s.to_ascii_lowercase(),
        None => validate::slugify(&c.name),
    };
    validate::slug(&slug).map_err(|e| {
        AppError::BadRequest(if slug.is_empty() {
            "이름으로 slug를 만들 수 없습니다. slug를 직접 입력하세요".into()
        } else {
            e
        })
    })?;
    check_domain_conflicts(&state, &c.domains, None).await?;

    // 포트 할당은 경쟁 조건이 있으므로 Redis 락으로 직렬화
    let mut r = state.redis.clone();
    let lock_key = state.key(&["lock", "port"]);
    let got: Option<String> = redis::cmd("SET")
        .arg(&lock_key)
        .arg(1)
        .arg("NX")
        .arg("EX")
        .arg(10)
        .query_async(&mut r)
        .await?;
    if got.is_none() {
        return Err(AppError::Conflict(
            "다른 앱이 생성 중입니다. 잠시 후 다시 시도하세요".into(),
        ));
    }
    let result = async {
        let port = if kind.is_service() { Some(allocate_port(&state).await?) } else { None };
        let res = sqlx::query(
            "INSERT INTO apps (project_id, name, slug, kind, git_url, git_branch, root_dir, build_cmd, start_cmd, binary_name, \
             output_dir, port, api_proxy_port, domains, health_path, webhook_token, frontend_dir, app_role) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(project_id)
        .bind(&c.name)
        .bind(&slug)
        .bind(kind.as_str())
        .bind(&c.git_url)
        .bind(&c.git_branch)
        .bind(&c.root_dir)
        .bind(&c.build_cmd)
        .bind(&c.start_cmd)
        .bind(&c.binary_name)
        .bind(&c.output_dir)
        .bind(port)
        .bind(c.api_proxy_port)
        .bind(&c.domains)
        .bind(&c.health_path)
        .bind(random_token(24))
        .bind(&c.frontend_dir)
        .bind(&c.app_role)
        .execute(&state.db)
        .await
        .map_err(|e| {
            if crate::error::is_unique_violation(&e) {
                AppError::Conflict(format!("slug '{slug}' 는 이미 사용 중입니다"))
            } else {
                e.into()
            }
        })?;
        AppResult::Ok(res.last_insert_id())
    }
    .await;
    let _: () = redis::AsyncCommands::del(&mut r, &lock_key).await?;
    let id = result?;
    audit::log(&state, Some(user.id), "app.create", &slug, &ip).await;
    Ok((StatusCode::CREATED, Json(fetch(&state, id).await?)))
}

pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
    Json(input): Json<AppInput>,
) -> AppResult<Json<App>> {
    user.require_admin()?;
    let app = fetch(&state, id).await?;
    if let Some(k) = &input.kind {
        if k != &app.kind {
            return Err(AppError::bad("앱 종류(kind)는 생성 후 바꿀 수 없습니다"));
        }
    }
    if app.source() != Source::Git {
        return update_site(state, user, ip, app, input).await;
    }
    let c = clean(&input, app.kind())?;
    check_domain_conflicts(&state, &c.domains, Some(id)).await?;
    sqlx::query(
        "UPDATE apps SET name = ?, git_url = ?, git_branch = ?, root_dir = ?, build_cmd = ?, start_cmd = ?, binary_name = ?, \
         output_dir = ?, api_proxy_port = ?, domains = ?, health_path = ?, frontend_dir = ?, app_role = ? WHERE id = ?",
    )
    .bind(&c.name)
    .bind(&c.git_url)
    .bind(&c.git_branch)
    .bind(&c.root_dir)
    .bind(&c.build_cmd)
    .bind(&c.start_cmd)
    .bind(&c.binary_name)
    .bind(&c.output_dir)
    .bind(c.api_proxy_port)
    .bind(&c.domains)
    .bind(&c.health_path)
    .bind(&c.frontend_dir)
    .bind(&c.app_role)
    .bind(id)
    .execute(&state.db)
    .await?;
    audit::log(&state, Some(user.id), "app.update", &app.slug, &ip).await;
    Ok(Json(fetch(&state, id).await?))
}

/// 파일 기반 사이트 · 프록시 설정 변경 → 저장 후 바로 적용
async fn update_site(
    state: AppState,
    user: AuthUser,
    ip: String,
    app: App,
    input: AppInput,
) -> AppResult<Json<App>> {
    let bad = AppError::BadRequest;
    validate::name(&input.name).map_err(bad)?;
    let domains = validate::domains(input.domains.as_deref().unwrap_or("")).map_err(bad)?;
    if domains.is_empty() {
        return Err(AppError::bad("사이트에는 도메인이 하나 이상 필요합니다"));
    }
    check_domain_conflicts(&state, &domains, Some(app.id)).await?;
    let web_root = opt(&input.web_root)
        .unwrap_or_default()
        .trim_matches('/')
        .to_string();
    validate::rel_path(&web_root).map_err(bad)?;
    let build_cmd = opt(&input.build_cmd);
    if let Some(c) = &build_cmd {
        validate::command(c).map_err(bad)?;
    }
    let start_cmd = match app.kind() {
        AppKind::Rust | AppKind::Python | AppKind::Node | AppKind::Java => {
            let c = opt(&input.start_cmd).ok_or_else(|| AppError::bad("시작 명령이 필요합니다"))?;
            validate::command(&c).map_err(bad)?;
            Some(c)
        }
        _ => None,
    };
    let proxy_target = match app.kind() {
        AppKind::Proxy => {
            let t = opt(&input.proxy_target)
                .ok_or_else(|| AppError::bad("프록시 대상 URL 이 필요합니다"))?;
            crate::routes::site_admin::valid_proxy_target(&t).map_err(bad)?;
            Some(t)
        }
        _ => None,
    };
    let health_path = opt(&input.health_path).unwrap_or_else(|| "/".into());
    validate::url_path(&health_path).map_err(bad)?;
    let body_mb = input.client_max_body_mb.unwrap_or(app.client_max_body_mb);
    if !(1..=10240).contains(&body_mb) {
        return Err(AppError::bad("업로드 크기 제한은 1~10240MB"));
    }
    sqlx::query(
        "UPDATE apps SET name = ?, domains = ?, web_root = ?, build_cmd = ?, start_cmd = ?, proxy_target = ?, \
         health_path = ?, client_max_body_mb = ? WHERE id = ?",
    )
    .bind(input.name.trim())
    .bind(&domains)
    .bind(&web_root)
    .bind(&build_cmd)
    .bind(&start_cmd)
    .bind(&proxy_target)
    .bind(&health_path)
    .bind(body_mb)
    .bind(app.id)
    .execute(&state.db)
    .await?;
    audit::log(&state, Some(user.id), "site.update", &app.slug, &ip).await;
    // 설정 즉시 적용 (진행 중인 적용이 있으면 건너뜀)
    let _ = crate::routes::deployments::enqueue(&state, app.id, Some(user.id)).await;
    Ok(Json(fetch(&state, app.id).await?))
}

pub async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
) -> AppResult<StatusCode> {
    user.require_admin()?;
    let app = fetch(&state, id).await?;
    let active: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM deployments WHERE app_id = ? AND status IN ('queued','running')",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    if active > 0 {
        return Err(AppError::Conflict(
            "진행 중인 배포가 있습니다. 취소하거나 끝난 뒤 삭제하세요".into(),
        ));
    }
    let dbs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM app_databases WHERE app_id = ?")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    if dbs > 0 {
        return Err(AppError::Conflict(format!(
            "이 앱에 연결된 MariaDB 데이터베이스 {dbs}개가 있습니다. 데이터베이스 탭에서 먼저 삭제(필요하면 백업)하세요"
        )));
    }
    let log = LogSink::capture();
    helper::call(&state, &["purge-app", &app.slug], &log)
        .await
        .map_err(|e| AppError::Internal(e.context("서비스/사이트/파일 정리 실패")))?;
    // 배정된 Redis DB 는 비워서 반납 (다음 앱이 남은 키를 물려받지 않도록)
    if let Some(n) = app.redis_db {
        if let Err(e) = crate::routes::databases::flush(&state, n).await {
            tracing::warn!(error = %e, db = n, "redis flush on app delete failed");
        }
    }
    crate::logstore::deploy_logs_delete_app(&state, app.id).await;
    sqlx::query("DELETE FROM apps WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    audit::log(&state, Some(user.id), "app.delete", &app.slug, &ip).await;
    Ok(StatusCode::NO_CONTENT)
}

// ───────────── 환경변수 ─────────────

#[derive(Serialize)]
pub struct EnvResponse {
    pub vars: Vec<EnvVar>,
    /// 패널이 자동 주입하는 값 (DB·Redis). 같은 키를 직접 넣으면 직접 넣은 값이 우선
    pub managed: Vec<crate::routes::databases::ManagedVar>,
}

pub async fn get_env(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<u64>,
) -> AppResult<Json<EnvResponse>> {
    user.require_admin()?;
    let app = fetch(&state, id).await?;
    Ok(Json(EnvResponse {
        vars: runner::load_env(&state, id).await?,
        managed: crate::routes::databases::managed_env(&state, &app).await?,
    }))
}

#[derive(Deserialize)]
pub struct EnvInput {
    pub vars: Vec<EnvVar>,
}

/// 전체 교체. 적용은 다음 배포(또는 재시작) 시점.
pub async fn put_env(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
    Json(input): Json<EnvInput>,
) -> AppResult<Json<EnvResponse>> {
    user.require_admin()?;
    let app = fetch(&state, id).await?;
    if input.vars.len() > 200 {
        return Err(AppError::bad("환경변수는 최대 200개입니다"));
    }
    let mut seen = std::collections::HashSet::new();
    for v in &input.vars {
        validate::env_key(&v.key).map_err(AppError::BadRequest)?;
        validate::env_value(&v.value).map_err(AppError::BadRequest)?;
        if !seen.insert(v.key.clone()) {
            return Err(AppError::bad(format!("중복된 키: {}", v.key)));
        }
    }
    let mut tx = state.db.begin().await?;
    sqlx::query("DELETE FROM app_env WHERE app_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    for v in &input.vars {
        sqlx::query("INSERT INTO app_env (app_id, `key`, value) VALUES (?, ?, ?)")
            .bind(id)
            .bind(&v.key)
            .bind(&v.value)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    // 값은 감사 로그에 남기지 않음
    audit::log(
        &state,
        Some(user.id),
        "app.env_update",
        &format!("{} ({}개)", app.slug, input.vars.len()),
        &ip,
    )
    .await;
    Ok(Json(EnvResponse {
        vars: runner::load_env(&state, id).await?,
        managed: crate::routes::databases::managed_env(&state, &app).await?,
    }))
}

// ───────────── 서비스 제어 ─────────────

#[derive(Deserialize)]
pub struct ServiceAction {
    pub action: String,
}

pub async fn service_status(
    State(state): State<AppState>,
    _u: AuthUser,
    Path(id): Path<u64>,
) -> AppResult<Json<serde_json::Value>> {
    let app = fetch(&state, id).await?;
    if !app.kind().is_service() {
        return Ok(Json(json!({ "active": null, "unit": null })));
    }
    let out = helper::call_quiet(&state, &["status", &app.slug]).await?;
    let active = out.stdout.trim().to_string();
    Ok(Json(
        json!({ "active": if active.is_empty() { "unknown".to_string() } else { active }, "unit": crate::deploy::templates::unit_name(&app.slug) }),
    ))
}

pub async fn service_control(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
    Json(body): Json<ServiceAction>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let app = fetch(&state, id).await?;
    if !app.kind().is_service() {
        return Err(AppError::bad("정적(static) 앱에는 서비스가 없습니다"));
    }
    if app.current_release.is_none() {
        return Err(AppError::bad(
            "아직 배포된 릴리스가 없습니다. 먼저 배포하세요",
        ));
    }
    let action = match body.action.as_str() {
        a @ ("start" | "stop" | "restart") => a,
        _ => {
            return Err(AppError::bad(
                "action 은 start / stop / restart 중 하나입니다",
            ))
        }
    };
    let log = LogSink::capture();
    helper::call(&state, &["svc", action, &app.slug], &log)
        .await
        .map_err(AppError::Internal)?;
    let status = if action == "stop" {
        "stopped"
    } else {
        "running"
    };
    sqlx::query("UPDATE apps SET status = ? WHERE id = ?")
        .bind(status)
        .bind(id)
        .execute(&state.db)
        .await?;
    audit::log(
        &state,
        Some(user.id),
        &format!("app.{action}"),
        &app.slug,
        &ip,
    )
    .await;
    Ok(Json(json!({ "ok": true, "status": status })))
}

#[derive(Deserialize)]
pub struct JournalQuery {
    pub lines: Option<u32>,
}

pub async fn journal(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<u64>,
    Query(q): Query<JournalQuery>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let app = fetch(&state, id).await?;
    if !app.kind().is_service() {
        return Ok(Json(json!({ "lines": [] })));
    }
    let n = q.lines.unwrap_or(200).clamp(10, 2000).to_string();
    let out = helper::call_quiet(&state, &["journal", &app.slug, &n]).await?;
    let lines: Vec<&str> = out.stdout.lines().collect();
    Ok(Json(json!({ "lines": lines })))
}

// ───────────── Deploy key (git SSH) ─────────────

async fn read_pubkey(state: &AppState, paths: &AppPaths) -> Option<String> {
    if state.exec.is_dry_run() {
        return None;
    }
    tokio::fs::read_to_string(paths.key_file.with_extension("pub"))
        .await
        .ok()
        .map(|s| s.trim().to_string())
}

pub async fn get_deploy_key(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<u64>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let app = fetch(&state, id).await?;
    let paths = AppPaths::new(&state.cfg.data_dir, &app.slug);
    Ok(Json(
        json!({ "public_key": read_pubkey(&state, &paths).await }),
    ))
}

pub async fn create_deploy_key(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let app = fetch(&state, id).await?;
    let paths = AppPaths::new(&state.cfg.data_dir, &app.slug);
    let exec = state.exec.as_ref();
    let log = LogSink::capture();
    let keys_dir = paths.key_file.parent().unwrap().to_path_buf();
    exec.mkdirs(&keys_dir, &log).await?;
    run_ok(
        exec,
        CmdSpec::new("chmod", &["700", &keys_dir.display().to_string()]),
        &log,
    )
    .await?;
    // 기존 키 교체
    let key = paths.key_file.display().to_string();
    let _ = tokio::fs::remove_file(&paths.key_file).await;
    let _ = tokio::fs::remove_file(paths.key_file.with_extension("pub")).await;
    run_ok(
        exec,
        CmdSpec::new(
            "ssh-keygen",
            &[
                "-q",
                "-t",
                "ed25519",
                "-N",
                "",
                "-C",
                &format!("rpanel-{}", app.slug),
                "-f",
                &key,
            ],
        ),
        &log,
    )
    .await?;
    audit::log(&state, Some(user.id), "app.deploy_key", &app.slug, &ip).await;
    let pk = read_pubkey(&state, &paths).await.unwrap_or_else(|| {
        format!(
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIDRYRUNDRYRUNDRYRUN rpanel-{} (dry-run)",
            app.slug
        )
    });
    Ok(Json(json!({ "public_key": pk })))
}

// ───────────── Webhook ─────────────

pub async fn get_webhook(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<u64>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let app = fetch(&state, id).await?;
    Ok(Json(
        json!({ "path": format!("/api/hooks/{}", app.slug), "token": app.webhook_token, "branch": app.git_branch }),
    ))
}

pub async fn rotate_webhook(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let app = fetch(&state, id).await?;
    sqlx::query("UPDATE apps SET webhook_token = ? WHERE id = ?")
        .bind(random_token(24))
        .bind(id)
        .execute(&state.db)
        .await?;
    audit::log(&state, Some(user.id), "app.webhook_rotate", &app.slug, &ip).await;
    get_webhook(State(state), user, Path(id)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> AppInput {
        AppInput {
            name: "Example API".into(),
            slug: None,
            kind: Some("rust".into()),
            git_url: Some("ssh://git@192.0.2.10:2222/team/backend.git".into()),
            git_branch: None,
            root_dir: Some("/backend/".into()),
            build_cmd: Some("  ".into()),
            start_cmd: Some("ignored".into()),
            binary_name: Some("hello-api".into()),
            output_dir: None,
            domains: Some("API.example.com, api.example.com".into()),
            api_proxy_port: Some(8080),
            health_path: Some("/api/health".into()),
            web_root: None,
            proxy_target: None,
            client_max_body_mb: None,
            frontend_dir: None,
            app_role: None,
        }
    }

    #[test]
    fn app_role_only_for_static() {
        let mut i = input();
        i.app_role = Some("admin".into());
        assert_eq!(clean(&i, AppKind::Rust).unwrap().app_role, None);
        assert_eq!(
            clean(&i, AppKind::Static).unwrap().app_role.as_deref(),
            Some("admin")
        );
        i.app_role = None;
        assert_eq!(
            clean(&i, AppKind::Static).unwrap().app_role.as_deref(),
            Some("front")
        );
        i.app_role = Some("root".into());
        assert!(clean(&i, AppKind::Static).is_err());
    }

    #[test]
    fn clean_normalizes() {
        let c = clean(&input(), AppKind::Rust).unwrap();
        assert_eq!(c.git_branch, "main");
        assert_eq!(c.root_dir, "backend");
        assert_eq!(c.build_cmd, None);
        assert_eq!(c.start_cmd, None, "start_cmd only kept for node");
        assert_eq!(c.binary_name.as_deref(), Some("hello-api"));
        assert_eq!(c.domains, "api.example.com");
        assert_eq!(c.api_proxy_port, None, "api proxy only for static");
        assert_eq!(c.output_dir, "dist");
    }

    #[test]
    fn clean_static_rules() {
        let c = clean(&input(), AppKind::Static).unwrap();
        assert_eq!(c.api_proxy_port, Some(8080));
        assert_eq!(c.binary_name, None);
        let mut i = input();
        i.git_url = Some("file:///etc".into());
        assert!(clean(&i, AppKind::Rust).is_err());
        let mut i = input();
        i.root_dir = Some("../../etc".into());
        assert!(clean(&i, AppKind::Rust).is_err());
    }

    #[test]
    fn port_picking() {
        assert_eq!(pick_port(8100, 8105, &[], |_| true), Some(8100));
        assert_eq!(pick_port(8100, 8105, &[8100, 8101], |_| true), Some(8102));
        assert_eq!(pick_port(8100, 8105, &[8100], |p| p != 8101), Some(8102));
        assert_eq!(pick_port(8100, 8101, &[8100, 8101], |_| true), None);
    }
}
