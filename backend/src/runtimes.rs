//! 언어 런타임 버전 관리 — Rust 툴체인(rustup) · Python · Node.js
//!
//! - Rust: 패널 사용자(rpanel)의 rustup 으로 툴체인을 설치하고, 빌드할 때 `RUSTUP_TOOLCHAIN` 으로 고릅니다.
//!   (`rustup default` 는 건드리지 않음 → 패널 자체 업데이트 빌드에 영향 없음)
//! - Python · Node.js: helper(root)가 `/opt/rpanel/runtimes` 에 설치합니다 (모든 사이트 유저가 읽기 가능).
//!   - Python: uv 로 standalone 빌드 설치 → `/opt/rpanel/runtimes/bin/python3.X`
//!   - Node.js: nodejs.org 공식 tarball(SHA-256 확인) → `/opt/rpanel/runtimes/node/<major>/bin`
//! - `system` 은 서버에 원래 있던 python3 / node 입니다.
//!
//! 언어마다 기본 버전이 하나 있고, 앱은 따로 고르지 않으면 기본 버전을 씁니다.

use anyhow::{anyhow, bail};
use chrono::{NaiveDate, Utc};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;

use crate::{
    deploy::{
        executor::{run_ok, CmdSpec},
        helper,
        logsink::LogSink,
    },
    models::{App, AppKind, Source},
    state::AppState,
};

pub const RUNTIMES_DIR: &str = "/opt/rpanel/runtimes";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    Rust,
    Python,
    Node,
}

impl Lang {
    pub const ALL: [Lang; 3] = [Lang::Rust, Lang::Python, Lang::Node];

    pub fn as_str(self) -> &'static str {
        match self {
            Lang::Rust => "rust",
            Lang::Python => "python",
            Lang::Node => "node",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "rust" => Some(Lang::Rust),
            "python" => Some(Lang::Python),
            "node" => Some(Lang::Node),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Lang::Rust => "Rust",
            Lang::Python => "Python",
            Lang::Node => "Node.js",
        }
    }

    /// 처음 쓸 때 기본으로 잡히는 버전 (설치 스크립트가 깔아 둔 것)
    fn builtin(self) -> &'static str {
        match self {
            Lang::Rust => "stable",
            Lang::Python | Lang::Node => "system",
        }
    }
}

static RUST_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(stable|beta|nightly|1\.[0-9]{1,3}(\.[0-9]{1,3})?)$").unwrap());
static PY_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(system|3\.[0-9]{1,2})$").unwrap());
static NODE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(system|[1-9][0-9])$").unwrap());

/// 버전 문자열 검사 (helper 도 같은 규칙으로 다시 검사)
pub fn validate(lang: Lang, version: &str) -> Result<(), String> {
    let ok = match lang {
        Lang::Rust => RUST_RE.is_match(version),
        Lang::Python => {
            PY_RE.is_match(version)
                && (version == "system" || minor(version).is_some_and(|m| (8..=20).contains(&m)))
        }
        Lang::Node => {
            NODE_RE.is_match(version)
                && (version == "system"
                    || version.parse::<u32>().is_ok_and(|m| (18..=40).contains(&m)))
        }
    };
    if ok {
        Ok(())
    } else {
        Err(match lang {
            Lang::Rust => {
                "Rust 버전은 stable · beta · nightly 또는 1.85 · 1.85.0 형식입니다".into()
            }
            Lang::Python => "Python 버전은 3.8 ~ 3.20 형식입니다 (예: 3.12)".into(),
            Lang::Node => "Node.js 버전은 주 버전 숫자입니다 (예: 22)".into(),
        })
    }
}

fn minor(v: &str) -> Option<u32> {
    v.split('.').nth(1)?.parse().ok()
}

/// 고를 수 있는 버전 목록 (새 버전은 직접 입력도 가능)
pub fn candidates(lang: Lang) -> Vec<String> {
    match lang {
        Lang::Rust => {
            let latest = latest_rust_minor(Utc::now().date_naive());
            let mut v = vec!["stable".to_string(), "beta".into(), "nightly".into()];
            v.extend((0..8).map(|i| format!("1.{}", latest - i)));
            v
        }
        Lang::Python => ["system", "3.14", "3.13", "3.12", "3.11", "3.10"]
            .map(String::from)
            .to_vec(),
        Lang::Node => ["system", "24", "22", "20"].map(String::from).to_vec(),
    }
}

/// Rust 는 1.0(2015-05-15) 이후 6주마다 새 버전 → 날짜로 최신 안정 버전 번호를 계산 (네트워크 불필요)
pub fn latest_rust_minor(today: NaiveDate) -> i64 {
    let start = NaiveDate::from_ymd_opt(2015, 5, 15).unwrap();
    ((today - start).num_days() / 42).max(0)
}

/// 앱 종류 → 쓰는 런타임 (업로드형 Rust 사이트·HTML 사이트·프록시는 없음)
pub fn lang_for(app: &App) -> Option<Lang> {
    match (app.kind(), app.source()) {
        (AppKind::Rust, Source::Git) => Some(Lang::Rust),
        (AppKind::Python, _) => Some(Lang::Python),
        (AppKind::Static, Source::Git) => Some(Lang::Node),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Runtime {
    pub id: u64,
    pub lang: String,
    pub version: String,
    pub status: String,
    pub is_default: bool,
    pub is_system: bool,
    pub detail: String,
    pub log: Option<String>,
    pub created_at: chrono::DateTime<Utc>,
    pub updated_at: chrono::DateTime<Utc>,
}

pub const RUNTIME_COLUMNS: &str =
    "id, lang, version, status, is_default, is_system, detail, log, created_at, updated_at";

/// 빌드·실행에 실제로 쓸 런타임
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub lang: Lang,
    pub version: String,
    pub detail: String,
}

impl Resolved {
    /// 화면·로그 표시용
    pub fn describe(&self) -> String {
        let v = if self.version == "system" {
            "서버 기본".to_string()
        } else {
            self.version.clone()
        };
        if self.detail.is_empty() || self.detail == self.version {
            format!("{} {v}", self.lang.label())
        } else {
            format!("{} {v} ({})", self.lang.label(), self.detail)
        }
    }

    /// 가상환경을 만들 Python 실행 파일
    pub fn python(&self) -> String {
        if self.version == "system" {
            "python3".into()
        } else {
            format!("{RUNTIMES_DIR}/bin/python{}", self.version)
        }
    }

    /// 빌드 명령에 넣을 환경변수
    pub fn build_env(&self, base_path: &str) -> Vec<(String, String)> {
        match self.lang {
            Lang::Rust => vec![("RUSTUP_TOOLCHAIN".into(), self.version.clone())],
            Lang::Node if self.version != "system" => vec![(
                "PATH".into(),
                format!("{RUNTIMES_DIR}/node/{}/bin:{base_path}", self.version),
            )],
            _ => vec![],
        }
    }
}

/// 처음 조회할 때 서버에 있던 버전을 기본으로 등록
pub async fn ensure_seeded(state: &AppState) -> anyhow::Result<()> {
    for lang in Lang::ALL {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM runtimes WHERE lang = ?")
            .bind(lang.as_str())
            .fetch_one(&state.db)
            .await?;
        if n > 0 {
            continue;
        }
        let version = lang.builtin();
        let detail = detect(state, lang, version).await;
        let status = if detail.is_some() || state.exec.is_dry_run() {
            "ready"
        } else {
            "failed"
        };
        sqlx::query(
            "INSERT IGNORE INTO runtimes (lang, version, status, is_default, is_system, detail) VALUES (?, ?, ?, 1, 1, ?)",
        )
        .bind(lang.as_str())
        .bind(version)
        .bind(status)
        .bind(detail.unwrap_or_default())
        .execute(&state.db)
        .await?;
    }
    Ok(())
}

/// 실제 버전 읽기 (없으면 None)
async fn detect(state: &AppState, lang: Lang, version: &str) -> Option<String> {
    let r = Resolved {
        lang,
        version: version.to_string(),
        detail: String::new(),
    };
    let spec = match lang {
        Lang::Rust => CmdSpec::new("rustc", &["--version"]).env("RUSTUP_TOOLCHAIN", version),
        Lang::Python => CmdSpec::new(r.python(), &["--version"]),
        Lang::Node if version == "system" => CmdSpec::new("node", &["--version"]),
        Lang::Node => CmdSpec::new(
            format!("{RUNTIMES_DIR}/node/{version}/bin/node"),
            &["--version"],
        ),
    };
    let out = state
        .exec
        .run(spec.timeout(30).quiet(), &LogSink::capture())
        .await
        .ok()?;
    if !out.ok() {
        return None;
    }
    parse_version(&format!("{}{}", out.stdout, out.stderr))
}

/// "rustc 1.90.0 (…)" · "Python 3.12.3" · "v22.11.0" → 버전 숫자
pub fn parse_version(s: &str) -> Option<String> {
    static RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"v?([0-9]+\.[0-9]+(\.[0-9]+)?(-[a-z]+)?)").unwrap());
    RE.captures(s).map(|c| c[1].to_string())
}

pub async fn list(state: &AppState) -> anyhow::Result<Vec<Runtime>> {
    ensure_seeded(state).await?;
    Ok(sqlx::query_as::<_, Runtime>(&format!(
        "SELECT {RUNTIME_COLUMNS} FROM runtimes ORDER BY lang, is_system DESC, version"
    ))
    .fetch_all(&state.db)
    .await?)
}

/// 앱이 쓸 런타임 — 앱에 고른 버전이 없으면 기본 버전
pub async fn resolve(state: &AppState, app: &App) -> anyhow::Result<Option<Resolved>> {
    let Some(lang) = lang_for(app) else {
        return Ok(None);
    };
    ensure_seeded(state).await?;
    let row: Option<Runtime> = match app.runtime_version.as_deref() {
        Some(v) => {
            sqlx::query_as(&format!(
                "SELECT {RUNTIME_COLUMNS} FROM runtimes WHERE lang = ? AND version = ?"
            ))
            .bind(lang.as_str())
            .bind(v)
            .fetch_optional(&state.db)
            .await?
        }
        None => {
            sqlx::query_as(&format!(
                "SELECT {RUNTIME_COLUMNS} FROM runtimes WHERE lang = ? AND is_default = 1"
            ))
            .bind(lang.as_str())
            .fetch_optional(&state.db)
            .await?
        }
    };
    let version = app
        .runtime_version
        .clone()
        .unwrap_or_else(|| lang.builtin().to_string());
    match row {
        Some(r) if r.status == "ready" => Ok(Some(Resolved {
            lang,
            version: r.version,
            detail: r.detail,
        })),
        Some(r) => bail!(
            "{} {} 이(가) 아직 준비되지 않았습니다 (상태: {}) — 런타임 메뉴에서 확인하세요",
            lang.label(),
            r.version,
            r.status
        ),
        None if app.runtime_version.is_some() => bail!(
            "{} {version} 이(가) 설치되어 있지 않습니다 — 런타임 메뉴에서 설치하세요",
            lang.label()
        ),
        None => Ok(Some(Resolved {
            lang,
            version,
            detail: String::new(),
        })),
    }
}

/// 설치 작업 (백그라운드). 성공하면 ready, 실패하면 failed + 로그
pub async fn install(state: AppState, id: u64, lang: Lang, version: String, make_default: bool) {
    let log = LogSink::capture();
    log.line(format!("{} {version} 설치 시작", lang.label()))
        .await;
    let result = do_install(&state, lang, &version, &log).await;
    let mut text = log.contents();
    let (status, detail) = match result {
        Ok(detail) => ("ready", detail),
        Err(e) => {
            text.push_str(&format!("\n오류: {e:#}\n"));
            ("failed", String::new())
        }
    };
    if text.len() > 16_000 {
        let cut = text.len() - 16_000;
        let cut = (cut..text.len())
            .find(|&i| text.is_char_boundary(i))
            .unwrap_or(cut);
        text = format!("…\n{}", &text[cut..]);
    }
    let _ = sqlx::query("UPDATE runtimes SET status = ?, detail = ?, log = ? WHERE id = ?")
        .bind(status)
        .bind(&detail)
        .bind(&text)
        .bind(id)
        .execute(&state.db)
        .await;
    if status == "ready" && make_default {
        let _ = set_default(&state, id).await;
    }
}

async fn do_install(
    state: &AppState,
    lang: Lang,
    version: &str,
    log: &LogSink,
) -> anyhow::Result<String> {
    validate(lang, version).map_err(|e| anyhow!(e))?;
    match lang {
        Lang::Rust => {
            run_ok(
                state.exec.as_ref(),
                CmdSpec::new(
                    "rustup",
                    &[
                        "toolchain",
                        "install",
                        version,
                        "--profile",
                        "minimal",
                        "--no-self-update",
                    ],
                )
                .timeout(3600),
                log,
            )
            .await?;
        }
        Lang::Python | Lang::Node => {
            if version == "system" {
                bail!("서버 기본 버전은 설치할 수 없습니다");
            }
            log.line(format!(
                "$ rpanel-helper runtime-install {} {version}",
                lang.as_str()
            ))
            .await;
            let out = helper::call_long(
                state,
                &["runtime-install", lang.as_str(), version],
                log,
                1800,
            )
            .await?;
            for l in out.stdout.lines().chain(out.stderr.lines()) {
                log.line(l).await;
            }
        }
    }
    let detail = detect(state, lang, version).await;
    if detail.is_none() && !state.exec.is_dry_run() {
        bail!("설치는 끝났지만 실행 파일을 찾지 못했습니다");
    }
    let detail = detail.unwrap_or_default();
    log.line(format!("완료: {} {}", lang.label(), detail)).await;
    Ok(detail)
}

pub async fn set_default(state: &AppState, id: u64) -> anyhow::Result<()> {
    let lang: String = sqlx::query_scalar("SELECT lang FROM runtimes WHERE id = ?")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    sqlx::query("UPDATE runtimes SET is_default = (id = ?) WHERE lang = ?")
        .bind(id)
        .bind(&lang)
        .execute(&state.db)
        .await?;
    Ok(())
}

pub async fn uninstall(state: &AppState, lang: Lang, version: &str) -> anyhow::Result<()> {
    let log = LogSink::capture();
    match lang {
        Lang::Rust => {
            run_ok(
                state.exec.as_ref(),
                CmdSpec::new("rustup", &["toolchain", "uninstall", version]).timeout(600),
                &log,
            )
            .await?;
        }
        Lang::Python | Lang::Node => {
            helper::call(state, &["runtime-remove", lang.as_str(), version], &log).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_validated() {
        for ok in ["stable", "nightly", "1.85", "1.85.0"] {
            assert!(validate(Lang::Rust, ok).is_ok(), "{ok}");
        }
        for bad in ["", "1.85;rm", "2.0", "stable-x86_64", "../x", "1.85.0.1"] {
            assert!(validate(Lang::Rust, bad).is_err(), "{bad}");
        }
        for ok in ["system", "3.12", "3.8", "3.14"] {
            assert!(validate(Lang::Python, ok).is_ok(), "{ok}");
        }
        for bad in ["3", "3.12.1", "2.7", "3.7", "py3", "3.12 "] {
            assert!(validate(Lang::Python, bad).is_err(), "{bad}");
        }
        for ok in ["system", "22", "18", "24"] {
            assert!(validate(Lang::Node, ok).is_ok(), "{ok}");
        }
        for bad in ["v22", "22.1", "16", "8", "lts"] {
            assert!(validate(Lang::Node, bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn rust_minor_from_date() {
        let d = |y, m, dd| NaiveDate::from_ymd_opt(y, m, dd).unwrap();
        // 실제 출시일 다음 날 기준
        assert_eq!(latest_rust_minor(d(2024, 7, 26)), 80);
        assert_eq!(latest_rust_minor(d(2025, 1, 10)), 84);
        assert_eq!(latest_rust_minor(d(2015, 5, 16)), 0);
    }

    #[test]
    fn parse_versions() {
        assert_eq!(
            parse_version("rustc 1.90.0 (1159e78c4 2025-09-14)").as_deref(),
            Some("1.90.0")
        );
        assert_eq!(parse_version("Python 3.12.3").as_deref(), Some("3.12.3"));
        assert_eq!(parse_version("v22.11.0\n").as_deref(), Some("22.11.0"));
        assert_eq!(
            parse_version("rustc 1.92.0-nightly (abc 2025-10-01)").as_deref(),
            Some("1.92.0-nightly")
        );
        assert_eq!(parse_version("command not found"), None);
    }

    #[test]
    fn resolved_env() {
        let r = Resolved {
            lang: Lang::Node,
            version: "22".into(),
            detail: "22.11.0".into(),
        };
        assert_eq!(
            r.build_env("/usr/bin"),
            vec![(
                "PATH".into(),
                "/opt/rpanel/runtimes/node/22/bin:/usr/bin".into()
            )]
        );
        assert_eq!(r.describe(), "Node.js 22 (22.11.0)");
        let p = Resolved {
            lang: Lang::Python,
            version: "3.12".into(),
            detail: String::new(),
        };
        assert_eq!(p.python(), "/opt/rpanel/runtimes/bin/python3.12");
        assert!(p.build_env("/usr/bin").is_empty());
        let s = Resolved {
            lang: Lang::Python,
            version: "system".into(),
            detail: "3.12.3".into(),
        };
        assert_eq!(s.python(), "python3");
        assert_eq!(s.describe(), "Python 서버 기본 (3.12.3)");
        let rs = Resolved {
            lang: Lang::Rust,
            version: "1.85".into(),
            detail: "1.85.1".into(),
        };
        assert_eq!(
            rs.build_env("/x"),
            vec![("RUSTUP_TOOLCHAIN".into(), "1.85".into())]
        );
    }
}
