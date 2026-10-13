//! Rpanel — VM102(Ubuntu 24.04)용 셀프호스팅 배포 패널
//!
//! Rust(Axum/Tokio) + SQLx(MySQL/MariaDB) + Redis + JWT(액세스) & Redis 리프레시 세션 + WebSocket

pub mod audit;
pub mod auth;
pub mod config;
pub mod deploy;
pub mod error;
pub mod metrics;
pub mod models;
pub mod routes;
pub mod state;
pub mod validate;

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::Context;
use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions};
use tokio::sync::Semaphore;

use crate::{
    config::{Config, ExecMode},
    deploy::executor::{DryRunExecutor, Executor, RealExecutor},
    state::AppState,
};

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

pub async fn build_state(cfg: Config) -> anyhow::Result<AppState> {
    let opts: MySqlConnectOptions = cfg.database_url.parse().context("DATABASE_URL 형식 오류")?;
    let opts = opts
        .timezone(Some(String::from("+00:00")))
        .charset("utf8mb4");
    let db = MySqlPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(opts)
        .await
        .context("MariaDB 접속 실패 (DATABASE_URL 확인)")?;
    MIGRATOR.run(&db).await.context("DB 마이그레이션 실패")?;

    let redis_client =
        redis::Client::open(cfg.redis_url.as_str()).context("REDIS_URL 형식 오류")?;
    let redis = redis::aio::ConnectionManager::new(redis_client.clone())
        .await
        .context("Redis 접속 실패 (REDIS_URL / requirepass 확인)")?;

    let exec: Arc<dyn Executor> = match cfg.exec_mode {
        ExecMode::Real => Arc::new(RealExecutor {
            base_path: cfg.build_path.clone(),
            home: std::env::var("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| cfg.data_dir.clone()),
            pass_env: cfg
                .pass_env
                .iter()
                .filter_map(|k| std::env::var(k).ok().map(|v| (k.clone(), v)))
                .collect(),
        }),
        ExecMode::DryRun => Arc::new(DryRunExecutor),
    };

    Ok(AppState {
        db,
        redis,
        redis_client,
        build_slots: Arc::new(Semaphore::new(cfg.max_concurrent_builds.max(1))),
        cancels: Arc::new(Mutex::new(HashMap::new())),
        exec,
        cfg: Arc::new(cfg),
    })
}
