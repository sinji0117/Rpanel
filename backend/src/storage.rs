//! 저장 위치 (백업 · 프로젝트 파일)
//!
//! 패널·앱은 늘 `/var/lib/rpanel/{backups,apps,www,builds}` 경로를 씁니다. 위치를 바꾸면 helper
//! `storage-set` 이 데이터를 새 위치로 옮긴 뒤 원래 경로에 bind mount 하므로(systemd .mount 유닛)
//! 다른 설정은 바뀌지 않습니다. 지금 위치는 helper 가 쓰는 `/etc/rpanel/storage` 에서 읽습니다.

use std::{
    collections::HashMap,
    path::Path,
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
pub enum Kind {
    Backups,
    Projects,
}

impl Kind {
    pub const ALL: [Kind; 2] = [Kind::Backups, Kind::Projects];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Backups => "backups",
            Kind::Projects => "projects",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Backups => "백업",
            Kind::Projects => "프로젝트 파일",
        }
    }

    /// 데이터 폴더 아래에서 옮겨지는 폴더들
    pub fn subdirs(self) -> &'static [&'static str] {
        match self {
            Kind::Backups => &["backups"],
            Kind::Projects => &["apps", "www", "builds"],
        }
    }
}

/// helper 가 기록한 지금 위치 (기본 위치면 없음)
pub fn registry(state: &AppState) -> HashMap<String, String> {
    std::fs::read_to_string(&state.cfg.storage_reg)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            Some((it.next()?.to_string(), it.next()?.to_string()))
        })
        .collect()
}

const FORBIDDEN: &[&str] = &[
    "/bin",
    "/boot",
    "/dev",
    "/etc",
    "/lib",
    "/lib32",
    "/lib64",
    "/libx32",
    "/proc",
    "/root",
    "/run",
    "/sbin",
    "/sys",
    "/usr",
    "/tmp",
    "/home",
    "/snap",
    "/lost+found",
    "/var",
];

fn inside(p: &str, base: &str) -> bool {
    p == base || p.starts_with(&format!("{base}/"))
}

/// 새 위치 검사 (helper 와 같은 규칙 — 빠른 안내용, 최종 검사는 helper)
pub fn check_path(kind: Kind, path: &str, reg: &HashMap<String, String>) -> Result<(), String> {
    static RE: Lazy<regex::Regex> =
        Lazy::new(|| regex::Regex::new(r"^/[A-Za-z0-9._/-]{1,200}$").unwrap());
    if !RE.is_match(path) {
        return Err("경로는 / 로 시작하고 영문·숫자·. _ - / 만 쓸 수 있습니다".into());
    }
    if path.ends_with('/') || path.contains("//") || path.split('/').any(|c| c == "." || c == "..")
    {
        return Err("경로가 올바르지 않습니다".into());
    }
    if let Some(f) = FORBIDDEN.iter().find(|f| inside(path, f)) {
        return Err(format!(
            "{f} 안은 쓸 수 없습니다. /srv, /data, /mnt 처럼 시스템 폴더가 아닌 곳을 고르세요"
        ));
    }
    if reg.get(kind.as_str()).map(String::as_str) == Some(path) {
        return Err("이미 그 위치입니다".into());
    }
    for other in Kind::ALL.into_iter().filter(|k| *k != kind) {
        if let Some(o) = reg.get(other.as_str()) {
            if inside(path, o) || inside(o, path) {
                return Err(format!("{} 위치({o})와 겹칩니다", other.label()));
            }
        }
    }
    Ok(())
}

/// 경로(또는 가장 가까운 상위 폴더)가 있는 파일시스템의 (전체, 남은) 바이트
pub fn fs_space(path: &Path) -> Option<(u64, u64)> {
    let mut p = path;
    while !p.exists() {
        p = p.parent()?;
    }
    let c = std::ffi::CString::new(p.as_os_str().as_encoded_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    let f = st.f_frsize as u64;
    Some((st.f_blocks as u64 * f, st.f_bavail as u64 * f))
}

// ───────── 옮기기 작업 ─────────

#[derive(Debug, Clone, Serialize)]
pub struct Job {
    pub kind: &'static str,
    /// 새 위치 (빈 문자열 = 기본 위치)
    pub target: String,
    /// running | done | failed
    pub status: String,
    pub log: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

static JOB: Lazy<Mutex<Option<Arc<Mutex<Job>>>>> = Lazy::new(Default::default);

pub fn job() -> Option<Job> {
    JOB.lock()
        .unwrap()
        .as_ref()
        .map(|j| j.lock().unwrap().clone())
}

pub fn running() -> bool {
    job().is_some_and(|j| j.status == "running")
}

/// 옮기는 동안 겹치면 안 되는 작업
pub async fn busy_reason(state: &AppState) -> Option<String> {
    if running() {
        return Some("저장 위치를 옮기는 중입니다".into());
    }
    if let Some(e) = crate::dbengines::running() {
        return Some(format!("{e} 버전을 바꾸는 중입니다"));
    }
    let deploys: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM deployments WHERE status IN ('queued','running')")
            .fetch_one(&state.db)
            .await
            .unwrap_or(0);
    if deploys > 0 {
        return Some(format!("배포 {deploys}개가 진행 중입니다"));
    }
    let backups: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM backups WHERE status = 'running'")
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);
    if backups > 0 {
        return Some(format!("백업 {backups}개가 진행 중입니다"));
    }
    None
}

pub async fn last_result(state: &AppState) -> Option<serde_json::Value> {
    let v: Option<String> =
        sqlx::query_scalar("SELECT v FROM panel_settings WHERE k = 'storage_job'")
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
    v.and_then(|s| serde_json::from_str(&s).ok())
}

/// 백그라운드로 helper `storage-set` 실행
pub fn start(state: AppState, kind: Kind, target: String) {
    let j = Arc::new(Mutex::new(Job {
        kind: kind.as_str(),
        target: target.clone(),
        status: "running".into(),
        log: String::new(),
        started_at: Utc::now(),
        finished_at: None,
    }));
    *JOB.lock().unwrap() = Some(j.clone());
    tokio::spawn(async move {
        let log = LogSink::capture();
        let jj = j.clone();
        let lg = log.clone();
        let copier = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                jj.lock().unwrap().log = lg.contents();
            }
        });
        let helper_path = state.cfg.helper_path.display().to_string();
        let arg = if target.is_empty() {
            "default"
        } else {
            target.as_str()
        };
        let res = run_ok(
            state.exec.as_ref(),
            CmdSpec::new(
                "sudo",
                &["-n", &helper_path, "storage-set", kind.as_str(), arg],
            )
            .timeout(6 * 3600),
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
        let status = if ok { "done" } else { "failed" };
        {
            let mut g = j.lock().unwrap();
            g.status = status.into();
            g.log = text.clone();
            g.finished_at = Some(Utc::now());
        }
        let tail: Vec<&str> = text.lines().collect();
        let tail = tail[tail.len().saturating_sub(60)..].join("\n");
        let result = json!({ "kind": kind.as_str(), "target": target, "status": status, "finished_at": Utc::now(), "log": tail });
        let _ = sqlx::query("INSERT INTO panel_settings (k, v) VALUES ('storage_job', ?) ON DUPLICATE KEY UPDATE v = VALUES(v)")
            .bind(result.to_string())
            .execute(&state.db)
            .await;
        let shown = if target.is_empty() {
            format!("{} (기본)", state.cfg.data_dir.display())
        } else {
            target.clone()
        };
        let params = json!({ "what": kind.label(), "path": shown });
        let ev = if ok {
            crate::notify::Event::new(
                "storage",
                "success",
                "{what} 저장 위치를 {path} 로 옮겼습니다",
                params,
            )
        } else {
            crate::notify::Event::new(
                "storage",
                "error",
                "{what} 저장 위치를 {path} 로 옮기지 못했습니다",
                params,
            )
            .body(
                text.lines()
                    .rev()
                    .take(3)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        };
        crate::notify::emit(&state, ev.link("/settings")).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_rules() {
        let mut reg = HashMap::new();
        for ok in [
            "/data/backups",
            "/srv/rpanel",
            "/mnt/hdd1/rp_files",
            "/backup",
        ] {
            assert!(check_path(Kind::Backups, ok, &reg).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "/",
            "data",
            "/data/",
            "/data//x",
            "/data/../etc",
            "/data/./x",
            "/etc/x",
            "/var/lib/rpanel/x",
            "/home/me/b",
            "/tmp/x",
            "/usr",
            "/data/a b",
            "/data/$(id)",
            "/root",
        ] {
            assert!(check_path(Kind::Backups, bad, &reg).is_err(), "{bad}");
        }
        reg.insert("projects".to_string(), "/data/projects".to_string());
        assert!(check_path(Kind::Backups, "/data/projects/b", &reg).is_err());
        assert!(check_path(Kind::Backups, "/data", &reg).is_err());
        assert!(check_path(Kind::Backups, "/data/projects2", &reg).is_ok());
        assert!(check_path(Kind::Projects, "/data/projects", &reg)
            .unwrap_err()
            .contains("이미"));
    }

    #[test]
    fn space_of_missing_path_uses_parent() {
        let (total, avail) = fs_space(Path::new("/definitely/not/here")).unwrap();
        assert!(total > 0 && avail <= total);
    }
}
