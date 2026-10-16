//! 데이터베이스 서버 버전 (MariaDB · MongoDB · Redis) — 지금 버전 확인, 더 높은 버전으로 바꾸기
//!
//! 런타임과 달리 DB 서버는 한 서버에 한 버전만 돌 수 있고, 데이터 형식 때문에 낮은 버전으로는
//! 되돌릴 수 없습니다. 그래서 "같은 계열 안에서 더 높은 버전으로 올리기"만 허용하고,
//! helper 가 바꾸기 전에 전체 백업(`/var/lib/rpanel/backups/_engines`)을 먼저 만듭니다.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use chrono::{DateTime, Utc};
use once_cell::sync::Lazy;
use serde::Serialize;
use serde_json::json;

use crate::{
    deploy::{
        executor::{run_ok, CmdSpec},
        logsink::LogSink,
    },
    state::AppState,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    MariaDb,
    MongoDb,
    Redis,
}

impl Engine {
    pub const ALL: [Engine; 3] = [Engine::MariaDb, Engine::MongoDb, Engine::Redis];

    pub fn as_str(self) -> &'static str {
        match self {
            Engine::MariaDb => "mariadb",
            Engine::MongoDb => "mongodb",
            Engine::Redis => "redis",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.as_str() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            Engine::MariaDb => "MariaDB",
            Engine::MongoDb => "MongoDB",
            Engine::Redis => "Redis",
        }
    }

    /// 고를 수 있는 버전 계열 (오래된 것 → 새 것). helper 도 같은 목록만 받습니다.
    pub fn versions(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Engine::MariaDb => &[
                ("10.11", "LTS · Ubuntu 24.04 기본"),
                ("11.4", "LTS"),
                ("11.8", "LTS"),
            ],
            Engine::MongoDb => &[("8.0", "기본"), ("8.2", "")],
            Engine::Redis => &[
                ("7.0", "Ubuntu 24.04 기본"),
                ("8", "packages.redis.io 최신 8.x"),
            ],
        }
    }
}

/// "10.11.13-MariaDB-0ubuntu…" → "10.11", "8.0.4" → "8.0", Redis "8.2.1" → "8"
pub fn series(engine: Engine, version: &str) -> Option<String> {
    let mut parts = version.split(|c: char| !c.is_ascii_digit());
    let major = parts.next().filter(|s| !s.is_empty())?;
    let minor = parts.next().filter(|s| !s.is_empty());
    let s = match (engine, minor) {
        (Engine::Redis, _) if major != "7" => major.to_string(),
        (_, Some(m)) => format!("{major}.{m}"),
        (_, None) => major.to_string(),
    };
    Some(s)
}

fn rank(engine: Engine, series: &str) -> Option<usize> {
    engine.versions().iter().position(|(v, _)| *v == series)
}

/// 바꿀 수 있는지 (같은 버전·낮은 버전·목록에 없는 버전 거부)
pub fn check_target(engine: Engine, current: Option<&str>, target: &str) -> Result<(), String> {
    let t = rank(engine, target)
        .ok_or_else(|| format!("{} {target} 은(는) 고를 수 없는 버전입니다", engine.label()))?;
    let Some(cur) = current else {
        return Err(format!(
            "{} 가 설치되어 있지 않거나 접속하지 못했습니다",
            engine.label()
        ));
    };
    match rank(engine, cur) {
        Some(c) if c == t => Err(format!("이미 {} {target} 입니다", engine.label())),
        Some(c) if c > t => Err(format!(
            "{} 는 낮은 버전({target})으로 되돌릴 수 없습니다 — 데이터 형식이 바뀌기 때문입니다",
            engine.label()
        )),
        Some(_) => Ok(()),
        // 목록보다 새 버전이 이미 깔려 있으면 내릴 수 없음, 목록보다 옛 버전이면 올리기 허용
        None => {
            let newest = engine.versions().last().map(|v| v.0).unwrap_or("");
            if version_lt(cur, newest) {
                Ok(())
            } else {
                Err(format!(
                    "지금 버전({cur})이 목록보다 새 버전이라 바꿀 수 없습니다"
                ))
            }
        }
    }
}

fn version_lt(a: &str, b: &str) -> bool {
    let p = |s: &str| {
        s.split('.')
            .map(|x| x.parse::<u32>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    p(a) < p(b)
}

// ───────── 지금 버전 ─────────

pub async fn current_version(state: &AppState, engine: Engine) -> Option<String> {
    match engine {
        Engine::MariaDb => sqlx::query_scalar::<_, String>("SELECT VERSION()")
            .fetch_one(&state.db)
            .await
            .ok(),
        Engine::Redis => {
            let mut r = state.redis.clone();
            let info: String = redis::cmd("INFO")
                .arg("server")
                .query_async(&mut r)
                .await
                .ok()?;
            info.lines()
                .find_map(|l| l.strip_prefix("redis_version:"))
                .map(|v| v.trim().to_string())
        }
        Engine::MongoDb => {
            state.mongo.as_ref()?;
            crate::mongo::server_info(state).await.version
        }
    }
}

// ───────── 바꾸기 작업 (패널 메모리에 진행 상황, 끝나면 panel_settings 에 결과) ─────────

#[derive(Debug, Clone, Serialize)]
pub struct Job {
    pub engine: &'static str,
    pub target: String,
    /// running | done | failed
    pub status: String,
    pub log: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

type Jobs = HashMap<&'static str, Arc<Mutex<Job>>>;
static JOBS: Lazy<Mutex<Jobs>> = Lazy::new(Default::default);

pub fn job(engine: Engine) -> Option<Job> {
    JOBS.lock()
        .unwrap()
        .get(engine.as_str())
        .map(|j| j.lock().unwrap().clone())
}

pub fn running() -> Option<&'static str> {
    JOBS.lock()
        .unwrap()
        .iter()
        .find(|(_, j)| j.lock().unwrap().status == "running")
        .map(|(k, _)| *k)
}

/// 마지막 결과 (패널을 다시 켠 뒤에도 보이도록)
pub async fn last_result(state: &AppState, engine: Engine) -> Option<serde_json::Value> {
    let v: Option<String> = sqlx::query_scalar("SELECT v FROM panel_settings WHERE k = ?")
        .bind(format!("db_engine_{}", engine.as_str()))
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();
    v.and_then(|s| serde_json::from_str(&s).ok())
}

/// 백그라운드로 helper `db-engine-upgrade` 실행
pub fn start(state: AppState, engine: Engine, target: String) {
    let j = Arc::new(Mutex::new(Job {
        engine: engine.as_str(),
        target: target.clone(),
        status: "running".into(),
        log: String::new(),
        started_at: Utc::now(),
        finished_at: None,
    }));
    JOBS.lock().unwrap().insert(engine.as_str(), j.clone());
    tokio::spawn(async move {
        let log = LogSink::capture();
        // 진행 중 로그를 화면에 보여 주기 위해 주기적으로 복사
        let jj = j.clone();
        let lg = log.clone();
        let copier = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                jj.lock().unwrap().log = lg.contents();
            }
        });
        let helper_path = state.cfg.helper_path.display().to_string();
        let res = run_ok(
            state.exec.as_ref(),
            CmdSpec::new(
                "sudo",
                &[
                    "-n",
                    &helper_path,
                    "db-engine-upgrade",
                    engine.as_str(),
                    &target,
                ],
            )
            .timeout(3600),
            &log,
        )
        .await;
        copier.abort();
        let mut text = log.contents();
        let ok = match &res {
            Ok(_) => true,
            Err(e) => {
                text.push_str(&format!("\n✖ {e:#}\n"));
                false
            }
        };
        // MongoDB 는 새 버전 기능을 켜 줌 (setFeatureCompatibilityVersion)
        if ok && engine == Engine::MongoDb {
            if let Some(c) = state.mongo.as_ref() {
                let r = c
                    .database("admin")
                    .run_command(mongodb::bson::doc! { "setFeatureCompatibilityVersion": target.as_str(), "confirm": true })
                    .await;
                text.push_str(&match r {
                    Ok(_) => format!("featureCompatibilityVersion = {target}\n"),
                    Err(e) => format!("(featureCompatibilityVersion 설정 실패: {e} — 나중에 다시 시도해도 됩니다)\n"),
                });
            }
        }
        let status = if ok { "done" } else { "failed" };
        {
            let mut g = j.lock().unwrap();
            g.status = status.into();
            g.log = text.clone();
            g.finished_at = Some(Utc::now());
        }
        let result = json!({ "target": target, "status": status, "finished_at": Utc::now(), "log": tail(&text, 60) });
        // DB 가 막 다시 켜졌을 수 있으니 몇 번 시도
        for _ in 0..10 {
            let w = sqlx::query("INSERT INTO panel_settings (k, v) VALUES (?, ?) ON DUPLICATE KEY UPDATE v = VALUES(v)")
                .bind(format!("db_engine_{}", engine.as_str()))
                .bind(result.to_string())
                .execute(&state.db)
                .await;
            if w.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
        let params = json!({ "engine": engine.label(), "version": target });
        let ev = if ok {
            crate::notify::Event::new(
                "db_engine",
                "success",
                "{engine} {version} 으로 바꿨습니다",
                params,
            )
        } else {
            crate::notify::Event::new(
                "db_engine",
                "error",
                "{engine} {version} 으로 바꾸지 못했습니다",
                params,
            )
            .body(tail(&text, 3))
        };
        crate::notify::emit(&state, ev.link("/runtimes?tab=db")).await;
    });
}

fn tail(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn series_parse() {
        assert_eq!(
            series(Engine::MariaDb, "10.11.13-MariaDB-0ubuntu0.24.04.1").as_deref(),
            Some("10.11")
        );
        assert_eq!(
            series(Engine::MariaDb, "11.4.5-MariaDB").as_deref(),
            Some("11.4")
        );
        assert_eq!(series(Engine::MongoDb, "8.0.4").as_deref(), Some("8.0"));
        assert_eq!(series(Engine::Redis, "7.0.15").as_deref(), Some("7.0"));
        assert_eq!(series(Engine::Redis, "8.2.1").as_deref(), Some("8"));
        assert_eq!(series(Engine::Redis, ""), None);
    }

    #[test]
    fn only_upgrades() {
        let m = Engine::MariaDb;
        assert!(check_target(m, Some("10.11"), "11.4").is_ok());
        assert!(check_target(m, Some("10.11"), "11.8").is_ok());
        assert!(check_target(m, Some("11.4"), "10.11")
            .unwrap_err()
            .contains("되돌릴 수 없습니다"));
        assert!(check_target(m, Some("11.4"), "11.4")
            .unwrap_err()
            .contains("이미"));
        assert!(
            check_target(m, Some("10.6"), "10.11").is_ok(),
            "older than list → upgrade ok"
        );
        assert!(
            check_target(m, Some("12.0"), "11.8").is_err(),
            "newer than list → no"
        );
        assert!(check_target(m, Some("10.11"), "9.9").is_err());
        assert!(check_target(m, Some("10.11"), "11.4; rm -rf /").is_err());
        assert!(check_target(m, None, "11.4").is_err());
        assert!(check_target(Engine::Redis, Some("7.0"), "8").is_ok());
        assert!(check_target(Engine::Redis, Some("8"), "7.0").is_err());
        assert!(check_target(Engine::MongoDb, Some("8.0"), "8.2").is_ok());
    }
}
