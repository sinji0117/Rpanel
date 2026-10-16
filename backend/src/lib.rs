//! Rpanel — 셀프호스팅 웹 호스팅 패널 (Ubuntu 24.04)
//!
//! Rust(Axum/Tokio) + SQLx(MySQL/MariaDB) + Redis + JWT(액세스) & Redis 리프레시 세션 + WebSocket

pub mod audit;
pub mod auth;
pub mod backup;
pub mod config;
pub mod dbengines;
pub mod deploy;
pub mod error;
pub mod files;
pub mod logstore;
pub mod metrics;
pub mod models;
pub mod mongo;
pub mod notify;
pub mod routes;
pub mod runtimes;
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

/// 주석만 고친 마이그레이션의 예전 체크섬(SHA-384). 이미 설치된 서버에서 "변경됨" 오류가 나지 않도록
/// 시작할 때 현재 체크섬으로 바꿔 줍니다. SQL 내용이 바뀐 경우에는 절대 여기에 넣지 마세요.
const LEGACY_CHECKSUMS: &[(i64, &str)] = &[
    (
        2,
        "05ad04c45bda19254b9adb7b57137ff245ed00aaab565bb31c802a5e2b9b80cf43ff5085353be733d91a916ea092f780",
    ),
    (
        3,
        "d7e31ead4553121ff45d6a064e3963ad96f52603b33a00e9bd938af93e1d4bfb1e25e0999b4ba7ff5cb379adf7800eb4",
    ),
];

async fn migrate_compat(db: &sqlx::MySqlPool) {
    for (version, old) in LEGACY_CHECKSUMS {
        let Some(m) = MIGRATOR.iter().find(|m| m.version == *version) else {
            continue;
        };
        let Ok(old) = hex::decode(old) else { continue };
        // 테이블이 아직 없으면(새 설치) 실패해도 무시
        let _ = sqlx::query(
            "UPDATE _sqlx_migrations SET checksum = ? WHERE version = ? AND checksum = ?",
        )
        .bind(m.checksum.as_ref())
        .bind(version)
        .bind(old)
        .execute(db)
        .await;
    }
}

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
    migrate_compat(&db).await;
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

    // MongoDB: 연결은 처음 쓸 때 맺어짐 (서버가 없어도 패널은 뜸)
    let mongo = match cfg.mongodb_url.as_deref() {
        Some(url) => {
            let mut opts = mongodb::options::ClientOptions::parse(url)
                .await
                .context("MONGODB_URL 형식 오류")?;
            opts.app_name = Some("rpanel".into());
            opts.server_selection_timeout = Some(Duration::from_secs(3));
            opts.connect_timeout = Some(Duration::from_secs(3));
            Some(mongodb::Client::with_options(opts).context("MongoDB 클라이언트 생성 실패")?)
        }
        None => None,
    };

    Ok(AppState {
        db,
        redis,
        redis_client,
        mongo,
        build_slots: Arc::new(Semaphore::new(cfg.max_concurrent_builds.max(1))),
        cancels: Arc::new(Mutex::new(HashMap::new())),
        exec,
        cfg: Arc::new(cfg),
    })
}
