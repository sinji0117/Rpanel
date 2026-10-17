pub mod apps;
pub mod backups;
pub mod cron;
pub mod databases;
pub mod dbengines;
pub mod dbweb;
pub mod dbweb_nosql;
pub mod deployments;
pub mod files;
pub mod mongo;
pub mod notifications;
pub mod panel_access;
pub mod panel_domain;
pub mod projects;
pub mod runtimes;
pub mod site_accounts;
pub mod site_admin;
pub mod sites;
pub mod storage;
pub mod system;
pub mod users;
pub mod ws;

use axum::{
    extract::DefaultBodyLimit,
    routing::{delete, get, post, put},
    Router,
};
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

use crate::{auth::handlers as auth, state::AppState};

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/health", get(system::health))
        // 인증
        .route("/auth/status", get(auth::status))
        .route("/auth/setup", post(auth::setup))
        .route("/auth/login", post(auth::login))
        .route("/auth/refresh", post(auth::refresh))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/me", get(auth::me))
        .route("/auth/password", post(auth::change_password))
        .route("/auth/ws-ticket", post(auth::ws_ticket))
        // 시스템
        .route("/system/info", get(system::info))
        .route("/system/overview", get(system::overview))
        .route("/system/version", get(system::version))
        .route("/system/metrics", get(system::metrics_history))
        .route("/logs/system", get(system::system_log))
        .route("/system/events", get(system::events))
        .route("/system/processes", get(system::processes))
        // 알림
        .route(
            "/notifications",
            get(notifications::list).delete(notifications::clear),
        )
        .route("/notifications/read", post(notifications::mark_read))
        .route(
            "/notifications/settings",
            get(notifications::get_settings).put(notifications::put_settings),
        )
        .route("/notifications/test", post(notifications::test))
        .route("/logs/deployments", get(system::deployment_log))
        // 언어 런타임 버전
        .route("/runtimes", get(runtimes::list).post(runtimes::install))
        .route("/runtimes/{id}", delete(runtimes::delete))
        .route("/runtimes/{id}/default", put(runtimes::make_default))
        .route("/apps/{id}/runtime", put(runtimes::set_for_app))
        .route("/audit", get(system::audit))
        // 프로젝트
        .route("/projects", get(projects::list).post(projects::create))
        .route(
            "/projects/{id}",
            get(projects::get)
                .put(projects::update)
                .delete(projects::delete),
        )
        .route("/projects/{id}/apps", post(apps::create))
        // 앱
        .route("/apps", get(apps::list))
        .route(
            "/apps/{id}",
            get(apps::get).put(apps::update).delete(apps::delete),
        )
        .route("/apps/{id}/env", get(apps::get_env).put(apps::put_env))
        .route(
            "/apps/{id}/service",
            get(apps::service_status).post(apps::service_control),
        )
        .route("/apps/{id}/journal", get(apps::journal))
        .route(
            "/apps/{id}/deploy-key",
            get(apps::get_deploy_key).post(apps::create_deploy_key),
        )
        .route(
            "/apps/{id}/webhook",
            get(apps::get_webhook).post(apps::rotate_webhook),
        )
        // DB · Redis
        .route(
            "/databases",
            get(databases::list_all).post(databases::create_global),
        )
        .route(
            "/databases/{db}",
            put(databases::link).delete(databases::drop_global),
        )
        .route(
            "/databases/{db}/password",
            post(databases::rotate_password_global),
        )
        .route("/databases/{db}/users", post(databases::user_create))
        .route("/databases/{db}/users/{uid}", delete(databases::user_drop))
        .route(
            "/databases/{db}/users/{uid}/password",
            post(databases::user_password),
        )
        .route(
            "/databases/{db}/backups",
            post(backups::create_for_database),
        )
        .route(
            "/system/panel-domain",
            get(panel_domain::get)
                .put(panel_domain::set)
                .delete(panel_domain::remove),
        )
        .route(
            "/system/panel-access",
            get(panel_access::get).put(panel_access::set),
        )
        .route("/system/storage", get(storage::get))
        .route("/system/storage/{kind}", put(storage::set))
        .route("/db-engines", get(dbengines::list))
        .route("/db-engines/{engine}", post(dbengines::upgrade))
        // DB 웹 관리 (MariaDB)
        .route("/dbweb/admin/{id}", get(dbweb::admin_tables))
        .route("/dbweb/admin/{id}/query", post(dbweb::admin_query))
        .route("/dbweb/admin/{id}/table/{table}", get(dbweb::admin_browse))
        .route(
            "/dbweb/admin/{id}/table/{table}/{op}",
            post(dbweb::admin_row),
        )
        .route("/dbweb/mongo/{id}", get(dbweb_nosql::mongo_collections))
        .route(
            "/dbweb/mongo/{id}/collections",
            post(dbweb_nosql::mongo_create_collection),
        )
        .route("/dbweb/mongo/{id}/c/{coll}", get(dbweb_nosql::mongo_find))
        .route(
            "/dbweb/mongo/{id}/c/{coll}/{op}",
            post(dbweb_nosql::mongo_write),
        )
        .route("/dbweb/redis", get(dbweb_nosql::redis_dbs))
        .route("/dbweb/redis/{db}/keys", get(dbweb_nosql::redis_keys))
        .route("/dbweb/redis/{db}/key", get(dbweb_nosql::redis_get))
        .route("/dbweb/redis/{db}/{op}", post(dbweb_nosql::redis_write))
        .route("/dbweb/login", post(dbweb::login))
        .route("/dbweb/logout", post(dbweb::logout))
        .route("/dbweb/me", get(dbweb::me))
        .route("/dbweb/s/{db}", get(dbweb::s_tables))
        .route("/dbweb/s/{db}/query", post(dbweb::s_query))
        .route("/dbweb/s/{db}/table/{table}", get(dbweb::s_browse))
        .route("/dbweb/s/{db}/table/{table}/{op}", post(dbweb::s_row))
        // MongoDB
        .route("/mongo", get(mongo::list).post(mongo::create))
        .route("/mongo/{id}", put(mongo::link).delete(mongo::delete))
        .route("/mongo/{id}/password", post(mongo::password))
        .route("/mongo/{id}/backups", post(mongo::backup_now))
        .route("/mongo/{id}/users", post(mongo::user_create))
        .route("/mongo/{id}/users/{uid}", delete(mongo::user_delete))
        .route(
            "/mongo/{id}/users/{uid}/password",
            post(mongo::user_password),
        )
        .route("/redis-users", post(databases::redis_user_create))
        .route(
            "/redis-users/{id}",
            put(databases::redis_user_update).delete(databases::redis_user_drop),
        )
        .route(
            "/redis-users/{id}/password",
            post(databases::redis_user_password),
        )
        .route(
            "/apps/{id}/databases",
            get(databases::get_for_app).post(databases::create),
        )
        .route("/apps/{id}/databases/{db}", delete(databases::drop))
        .route(
            "/apps/{id}/databases/{db}/password",
            post(databases::rotate_password),
        )
        .route(
            "/apps/{id}/redis",
            post(databases::redis_assign).delete(databases::redis_release),
        )
        .route("/apps/{id}/redis/flush", post(databases::redis_flush))
        // 사이트 (사이트 유저 · SSL · 파일)
        .route("/sites", get(sites::list).post(site_admin::create))
        .route("/system/edge", get(site_admin::edge))
        .route(
            "/apps/{id}/site-user/password",
            put(site_admin::set_password),
        )
        .route(
            "/apps/{id}/accounts",
            get(site_accounts::list).post(site_accounts::create),
        )
        .route(
            "/apps/{id}/accounts/{aid}",
            put(site_accounts::update).delete(site_accounts::delete),
        )
        .route(
            "/apps/{id}/accounts/{aid}/password",
            post(site_accounts::password),
        )
        // 관리자 계정
        .route("/users", get(users::list).post(users::create))
        .route("/users/{id}", put(users::update).delete(users::delete))
        .route(
            "/apps/{id}/ssl",
            get(site_admin::ssl_get).delete(site_admin::ssl_delete),
        )
        .route(
            "/apps/{id}/ssl/letsencrypt",
            post(site_admin::ssl_letsencrypt),
        )
        .route("/apps/{id}/ssl/custom", post(site_admin::ssl_custom))
        .route("/apps/{id}/ssl/force", put(site_admin::ssl_force))
        // 파일 관리자
        .route("/apps/{id}/files", get(files::list))
        .route(
            "/apps/{id}/files/content",
            get(files::read).put(files::write),
        )
        .route(
            "/apps/{id}/files/upload",
            post(files::upload).layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024)),
        )
        .route("/apps/{id}/files/download", get(files::download))
        .route("/apps/{id}/files/op", post(files::op))
        .route("/apps/{id}/nginx-logs", get(sites::logs))
        // 백업
        .route("/backups", get(backups::list))
        .route("/apps/{id}/backups", post(backups::create))
        .route(
            "/backup-jobs",
            get(backups::list_jobs).post(backups::create_job),
        )
        .route(
            "/backup-jobs/{id}",
            put(backups::update_job).delete(backups::delete_job),
        )
        .route("/backup-jobs/{id}/run", post(backups::run_job))
        .route("/backups/{id}", delete(backups::delete))
        .route("/backups/{id}/restore", post(backups::restore))
        .route("/backups/{id}/download", get(backups::download))
        // 크론
        .route("/apps/{id}/cron", get(cron::list).post(cron::create))
        .route("/cron/preview", post(cron::preview_route))
        .route("/cron/{id}", put(cron::update).delete(cron::delete))
        .route("/cron/{id}/run", post(cron::run_now))
        .route("/cron/{id}/log", get(cron::log))
        .route("/apps/{id}/deploy", post(deployments::deploy))
        .route("/apps/{id}/deployments", get(deployments::list_for_app))
        // 배포
        .route("/deployments/{id}", get(deployments::get))
        .route("/deployments/{id}/cancel", post(deployments::cancel))
        .route("/deployments/{id}/rollback", post(deployments::rollback))
        // 외부 트리거
        .route("/hooks/{slug}", post(deployments::webhook))
        // WebSocket
        .route("/ws/metrics", get(ws::metrics))
        .route("/ws/deployments/{id}", get(ws::deployment_log))
        .fallback(|| async { crate::error::AppError::NotFound });

    let mut app = Router::new().nest("/api", api);

    if let Some(dir) = state.cfg.static_dir.clone() {
        let index = dir.join("index.html");
        app = app.fallback_service(ServeDir::new(dir).fallback(ServeFile::new(index)));
    }

    app.layer(axum::middleware::map_response(static_cache_headers))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// 화면 파일 캐시: index.html 등은 매번 확인(no-cache) → 업데이트 후 옛 화면이 남지 않음.
/// 이름에 해시가 붙은 /assets/* 는 오래 캐시.
async fn static_cache_headers(
    uri: axum::http::Uri,
    mut res: axum::response::Response,
) -> axum::response::Response {
    use axum::http::{header, HeaderValue};
    let path = uri.path();
    if path.starts_with("/api/") || res.headers().contains_key(header::CACHE_CONTROL) {
        return res;
    }
    let v = if path.starts_with("/assets/") && res.status().is_success() {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(v));
    res
}

#[cfg(test)]
mod cache_tests {
    use axum::{body::Body, http::Request, Router};
    use tower::ServiceExt;
    use tower_http::services::{ServeDir, ServeFile};

    #[tokio::test]
    async fn static_cache_headers() {
        let dir = std::env::temp_dir().join(format!("rpanel-static-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("index.html"), "<html></html>").unwrap();
        std::fs::write(dir.join("assets/app-abc123.js"), "x").unwrap();
        let app = Router::new()
            .fallback_service(ServeDir::new(&dir).fallback(ServeFile::new(dir.join("index.html"))))
            .layer(axum::middleware::map_response(super::static_cache_headers));
        let get = |p: &'static str| {
            let app = app.clone();
            async move {
                let res = app
                    .oneshot(Request::get(p).body(Body::empty()).unwrap())
                    .await
                    .unwrap();
                res.headers()
                    .get("cache-control")
                    .map(|v| v.to_str().unwrap().to_string())
            }
        };
        assert_eq!(get("/").await.as_deref(), Some("no-cache"));
        assert_eq!(
            get("/apps/3").await.as_deref(),
            Some("no-cache"),
            "SPA route → index.html"
        );
        assert_eq!(
            get("/assets/app-abc123.js").await.as_deref(),
            Some("public, max-age=31536000, immutable")
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
