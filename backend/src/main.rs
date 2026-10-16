use std::net::SocketAddr;

use rpanel::{config::Config, routes, routes::deployments};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("RUST_LOG")
                .unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn,tower_http=info")),
        )
        .init();

    let cfg = Config::from_env()?;
    let bind = cfg.bind_addr;
    tracing::info!(mode = ?cfg.exec_mode, %bind, "starting rpanel {}", env!("CARGO_PKG_VERSION"));

    let state = rpanel::build_state(cfg).await?;
    // MongoDB 로그 저장소 준비 (인덱스·기존 로그 옮기기). 실패해도 패널은 계속 동작
    {
        let st = state.clone();
        tokio::spawn(async move {
            if let Err(e) = rpanel::logstore::init(&st).await {
                tracing::warn!(error = %e, "MongoDB log store init failed");
            }
        });
    }
    deployments::recover_orphans(&state).await?;
    rpanel::backup::spawn_scheduler(state.clone());
    routes::system::spawn_sampler(state.clone());
    rpanel::notify::spawn_site_monitor(state.clone());
    rpanel::notify::spawn_update_watch(state.clone());

    let app = routes::router(state);
    let listener = tokio::net::TcpListener::bind(bind).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

/// systemd stop(SIGTERM) 또는 Ctrl+C
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => futures::future::pending::<()>().await,
        }
    };
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
    tracing::info!("shutting down");
}
