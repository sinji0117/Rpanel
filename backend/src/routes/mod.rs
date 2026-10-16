pub mod apps;
pub mod backups;
pub mod cron;
pub mod databases;
pub mod deployments;
pub mod files;
pub mod projects;
pub mod site_admin;
pub mod sites;
pub mod system;
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

    app.layer(TraceLayer::new_for_http()).with_state(state)
}
