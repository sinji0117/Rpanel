use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use tokio::sync::{watch, Semaphore};

use redis::aio::ConnectionManager;
use sqlx::MySqlPool;

use crate::{config::Config, deploy::executor::Executor};

#[derive(Clone)]
pub struct AppState {
    pub db: MySqlPool,
    pub redis: ConnectionManager,
    pub redis_client: redis::Client,
    pub cfg: Arc<Config>,
    pub exec: Arc<dyn Executor>,
    /// 동시 빌드 제한
    pub build_slots: Arc<Semaphore>,
    /// 실행 중 배포의 취소 신호
    pub cancels: Arc<Mutex<HashMap<u64, watch::Sender<bool>>>>,
}

impl AppState {
    /// 네임스페이스가 붙은 Redis 키
    pub fn key(&self, parts: &[&str]) -> String {
        let mut k = self.cfg.redis_prefix.clone();
        for p in parts {
            k.push(':');
            k.push_str(p);
        }
        k
    }
}
