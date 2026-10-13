pub mod apps;
pub mod deployments;
pub mod projects;
pub mod system;
pub mod ws;

use axum::{
    routing::{get, post},
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
