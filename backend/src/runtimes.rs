//! 언어 런타임 버전 관리 — Rust 툴체인(rustup) · Python · Node.js · Java
//!
//! - Rust: 패널 사용자(rpanel)의 rustup 으로 툴체인을 설치하고, 빌드할 때 `RUSTUP_TOOLCHAIN` 으로 고릅니다.
//!   (`rustup default` 는 건드리지 않음 → 패널 자체 업데이트 빌드에 영향 없음)
//! - Python · Node.js: helper(root)가 `/opt/rpanel/runtimes` 에 설치합니다 (모든 사이트 유저가 읽기 가능).
//!   - Python: uv 로 standalone 빌드 설치 → `/opt/rpanel/runtimes/bin/python3.X`
//!   - Node.js: nodejs.org 공식 tarball(SHA-256 확인) → `/opt/rpanel/runtimes/node/<major>/bin`
//!   - Java: Eclipse Temurin JDK(Adoptium, SHA-256 확인) → `/opt/rpanel/runtimes/java/<major>/bin`
//!   - Maven · Gradle(Java 빌드 도구): 공식 배포 파일(SHA-512 · SHA-256 확인) → `/opt/rpanel/runtimes/{maven,gradle}/<버전>/bin`
//!     Java 앱을 빌드할 때 기본 버전의 bin 을 PATH 앞에 붙입니다 (앱별로 고르지는 않음)
//! - `system` 은 서버에 원래 있던 python3 / node / java 입니다.
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
    Java,
    Maven,
    Gradle,
}

impl Lang {
    pub const ALL: [Lang; 6] = [
        Lang::Rust,
        Lang::Python,
        Lang::Node,
        Lang::Java,
        Lang::Maven,
        Lang::Gradle,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Lang::Rust => "rust",
            Lang::Python => "python",
            Lang::Node => "node",
            Lang::Java => "java",
            Lang::Maven => "maven",
            Lang::Gradle => "gradle",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "rust" => Some(Lang::Rust),
            "python" => Some(Lang::Python),
            "node" => Some(Lang::Node),
            "java" => Some(Lang::Java),
            "maven" => Some(Lang::Maven),
            "gradle" => Some(Lang::Gradle),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Lang::Rust => "Rust",
            Lang::Python => "Python",
            Lang::Node => "Node.js",
            Lang::Java => "Java",
            Lang::Maven => "Maven",
            Lang::Gradle => "Gradle",
        }
    }

    /// 처음 쓸 때 기본으로 잡히는 버전 (설치 스크립트가 깔아 둔 것)
    fn builtin(self) -> &'static str {
        match self {
            Lang::Rust => "stable",
            Lang::Python | Lang::Node | Lang::Java | Lang::Maven | Lang::Gradle => "system",
        }
    }
}

static RUST_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(stable|beta|nightly|1\.[0-9]{1,3}(\.[0-9]{1,3})?)$").unwrap());
static PY_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(system|3\.[0-9]{1,2})$").unwrap());
static NODE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(system|[1-9][0-9])$").unwrap());
static JAVA_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(system|8|[1-9][0-9])$").unwrap());
/// Maven · Gradle: 주 버전(그 안의 최신) 또는 주.부 버전
static TOOL_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(system|[1-9][0-9]?(\.[0-9]{1,2})?)$").unwrap());

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
        Lang::Java => {
            JAVA_RE.is_match(version)
                && (version == "system"
                    || version
                        .parse::<u32>()
                        .is_ok_and(|m| m == 8 || (11..=40).contains(&m)))
        }
        Lang::Maven => {
            TOOL_RE.is_match(version) && (version == "system" || major_in(version, 3..=9))
        }
        Lang::Gradle => {
            TOOL_RE.is_match(version) && (version == "system" || major_in(version, 7..=30))
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
            Lang::Java => "Java 버전은 주 버전 숫자입니다 (8 또는 11 이상, 예: 21)".into(),
            Lang::Maven => "Maven 버전은 3 · 3.9 형식입니다 (주 버전만 쓰면 그 안의 최신)".into(),
            Lang::Gradle => {
                "Gradle 버전은 9 · 8.14 형식입니다 (주 버전만 쓰면 그 안의 최신)".into()
            }
        })
    }
}

fn major_in(v: &str, r: std::ops::RangeInclusive<u32>) -> bool {
    v.split('.')
        .next()
        .and_then(|m| m.parse::<u32>().ok())
        .is_some_and(|m| r.contains(&m))
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
        Lang::Java => ["system", "25", "21", "17", "11"]
            .map(String::from)
            .to_vec(),
        Lang::Maven => ["system", "3"].map(String::from).to_vec(),
        Lang::Gradle => ["system", "9", "8"].map(String::from).to_vec(),
    }
}

/// Rust 는 1.0(2015-05-15) 이후 6주마다 새 버전 → 날짜로 최신 안정 버전 번호를 계산 (네트워크 불필요)
pub fn latest_rust_minor(today: NaiveDate) -> i64 {
    let start = NaiveDate::from_ymd_opt(2015, 5, 15).unwrap();
    ((today - start).num_days() / 42).max(0)
}

/// 앱 종류 → 쓰는 런타임 (업로드형 Rust 사이트·HTML 사이트·프록시는 없음)
pub fn lang_for(app: &App) -> Option<Lang> {
    lang_for_kind(app.kind(), app.source())
}

pub fn lang_for_kind(kind: AppKind, source: Source) -> Option<Lang> {
    match (kind, source) {
        (AppKind::Rust, Source::Git) => Some(Lang::Rust),
        (AppKind::Python, _) => Some(Lang::Python),
        (AppKind::Node, _) => Some(Lang::Node),
        (AppKind::Java, _) => Some(Lang::Java),
        (AppKind::Static, Source::Git) => Some(Lang::Node),
        _ => None,
    }
}

/// 이 런타임 버전을 쓰는 앱 종류 (지우기 전 사용 중인지 확인용)
pub fn kinds_for(lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::Rust => &["rust"],
        Lang::Python => &["python"],
        Lang::Node => &["node", "static"],
        Lang::Java => &["java"],
        // 빌드 도구는 앱별로 고르지 않음
        Lang::Maven | Lang::Gradle => &[],
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

    /// 설치한 Node.js · Java 의 bin 폴더 (서버 기본 버전이면 None)
    pub fn bin_dir(&self) -> Option<String> {
        bin_dir(self.lang, &self.version)
    }

    /// PATH 앞에 런타임 bin 을 붙임
    pub fn path_with(&self, base_path: &str) -> String {
        match self.bin_dir() {
            Some(b) => format!("{b}:{base_path}"),
            None => base_path.to_string(),
        }
    }

    /// 빌드 명령에 넣을 환경변수
    pub fn build_env(&self, base_path: &str) -> Vec<(String, String)> {
        match self.lang {
            Lang::Rust => vec![("RUSTUP_TOOLCHAIN".into(), self.version.clone())],
            Lang::Node if self.version != "system" => {
                vec![("PATH".into(), self.path_with(base_path))]
            }
            Lang::Java if self.version != "system" => vec![
                ("PATH".into(), self.path_with(base_path)),
                (
                    "JAVA_HOME".into(),
                    format!("{RUNTIMES_DIR}/java/{}", self.version),
                ),
            ],
            _ => vec![],
        }
    }
}

fn bin_dir(lang: Lang, version: &str) -> Option<String> {
    match lang {
        Lang::Node | Lang::Java | Lang::Maven | Lang::Gradle if version != "system" => {
            Some(format!("{RUNTIMES_DIR}/{}/{version}/bin", lang.as_str()))
        }
        _ => None,
    }
}

/// 서비스·크론·사이트 명령의 PATH — 앱이 쓰는 Node.js · Java 버전의 bin 을 앞에 붙임
/// (런타임을 확인하지 못하면 base 그대로)
pub async fn app_path(state: &AppState, app: &App, base: &str) -> String {
    match resolve(state, app).await {
        Ok(Some(r)) => r.path_with(base),
        _ => base.to_string(),
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
        // java -version 은 stderr 로 출력 (JAVA_TOOL_OPTIONS 안내 줄이 먼저 나올 수 있음)
        Lang::Java if version == "system" => CmdSpec::new("java", &["-version"]),
        Lang::Java => CmdSpec::new(
            format!("{RUNTIMES_DIR}/java/{version}/bin/java"),
            &["-version"],
        ),
        Lang::Maven if version == "system" => CmdSpec::new("mvn", &["-v"]),
        Lang::Gradle if version == "system" => CmdSpec::new("gradle", &["--version", "--quiet"]),
        // 설치한 빌드 도구: helper 가 남긴 버전 파일 (실행에는 Java 가 필요해 직접 실행하지 않음)
        Lang::Maven | Lang::Gradle => CmdSpec::new(
            "cat",
            &[&format!(
                "{RUNTIMES_DIR}/{}/{version}/.rpanel-version",
                lang.as_str()
            )],
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
    let text = format!("{}{}", out.stdout, out.stderr);
    match lang {
        Lang::Java => parse_java_version(&text),
        Lang::Maven if version == "system" => parse_tool_version("Apache Maven", &text),
        Lang::Gradle if version == "system" => parse_tool_version("Gradle", &text),
        _ => parse_version(&text),
    }
}

/// `Apache Maven 3.9.11 (…)` · `Gradle 8.14.3` → 버전 (Java 안내 줄 등은 건너뜀)
pub fn parse_tool_version(name: &str, s: &str) -> Option<String> {
    let re = Regex::new(&format!(
        r"(?m)^{} ([0-9]+\.[0-9]+(\.[0-9]+)?)",
        regex::escape(name)
    ))
    .ok()?;
    re.captures(s).map(|c| c[1].to_string())
}

/// `openjdk version "21.0.4" 2024-07-16` · `java version "1.8.0_412"` → "21.0.4" · "1.8.0_412"
pub fn parse_java_version(s: &str) -> Option<String> {
    static RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r#"version "([0-9][0-9A-Za-z._+-]*)""#).unwrap());
    RE.captures(s).map(|c| c[1].to_string())
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
    resolve_lang(state, lang, app.runtime_version.as_deref())
        .await
        .map(Some)
}

/// 언어의 런타임 — `chosen` 이 없으면 기본 버전
pub async fn resolve_lang(
    state: &AppState,
    lang: Lang,
    chosen: Option<&str>,
) -> anyhow::Result<Resolved> {
    ensure_seeded(state).await?;
    let row: Option<Runtime> = match chosen {
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
    let version = chosen
        .map(str::to_string)
        .unwrap_or_else(|| lang.builtin().to_string());
    match row {
        Some(r) if r.status == "ready" => Ok(Resolved {
            lang,
            version: r.version,
            detail: r.detail,
        }),
        Some(r) => bail!(
            "{} {} 이(가) 아직 준비되지 않았습니다 (상태: {}) — 런타임 메뉴에서 확인하세요",
            lang.label(),
            r.version,
            r.status
        ),
        None if chosen.is_some() => bail!(
            "{} {version} 이(가) 설치되어 있지 않습니다 — 런타임 메뉴에서 설치하세요",
            lang.label()
        ),
        None => Ok(Resolved {
            lang,
            version,
            detail: String::new(),
        }),
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
    let params = serde_json::json!({ "lang": lang.label(), "version": version });
    let ev = if status == "ready" {
        crate::notify::Event::new(
            "runtime_install",
            "success",
            "{lang} {version} 설치 완료",
            params,
        )
    } else {
        crate::notify::Event::new(
            "runtime_install",
            "error",
            "{lang} {version} 설치 실패",
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
    crate::notify::emit(&state, ev.link("/runtimes")).await;
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
        Lang::Python | Lang::Node | Lang::Java | Lang::Maven | Lang::Gradle => {
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
        Lang::Python | Lang::Node | Lang::Java | Lang::Maven | Lang::Gradle => {
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
        for ok in ["system", "8", "11", "17", "21", "25"] {
            assert!(validate(Lang::Java, ok).is_ok(), "{ok}");
        }
        for bad in ["9", "10", "1.8", "21.0", "jdk21", "7", "41", ""] {
            assert!(validate(Lang::Java, bad).is_err(), "{bad}");
        }
        for ok in ["system", "3", "3.9"] {
            assert!(validate(Lang::Maven, ok).is_ok(), "{ok}");
        }
        for bad in ["2", "3.9.11", "latest", "3.x", "3;id", ""] {
            assert!(validate(Lang::Maven, bad).is_err(), "{bad}");
        }
        for ok in ["system", "8", "9", "8.14"] {
            assert!(validate(Lang::Gradle, ok).is_ok(), "{ok}");
        }
        for bad in ["6", "8.14.3", "9.x", "../9", ""] {
            assert!(validate(Lang::Gradle, bad).is_err(), "{bad}");
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
        let j = "Picked up JAVA_TOOL_OPTIONS: -Dhttps.proxyHost=127.0.0.1\nopenjdk version \"21.0.4\" 2024-07-16 LTS\n";
        assert_eq!(parse_java_version(j).as_deref(), Some("21.0.4"));
        assert_eq!(
            parse_java_version("java version \"1.8.0_412\"").as_deref(),
            Some("1.8.0_412")
        );
        assert_eq!(parse_java_version("bash: java: not found"), None);
        let mvn = "Apache Maven 3.9.11 (3e54c93a704957b63ee3494413a2b544fd3d825b)\nMaven home: /usr/share/maven\nJava version: 21.0.4";
        assert_eq!(
            parse_tool_version("Apache Maven", mvn).as_deref(),
            Some("3.9.11")
        );
        let gr = "Picked up JAVA_TOOL_OPTIONS: -Dx=127.0.0.1\n\n------\nGradle 8.14.3\n------\nKotlin: 2.0.21";
        assert_eq!(parse_tool_version("Gradle", gr).as_deref(), Some("8.14.3"));
        assert_eq!(parse_tool_version("Gradle", "command not found"), None);
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
        let j = Resolved {
            lang: Lang::Java,
            version: "21".into(),
            detail: "21.0.4".into(),
        };
        assert_eq!(
            j.build_env("/usr/bin"),
            vec![
                (
                    "PATH".into(),
                    "/opt/rpanel/runtimes/java/21/bin:/usr/bin".into()
                ),
                ("JAVA_HOME".into(), "/opt/rpanel/runtimes/java/21".into()),
            ]
        );
        assert_eq!(j.path_with("/bin"), "/opt/rpanel/runtimes/java/21/bin:/bin");
        let js = Resolved {
            lang: Lang::Java,
            version: "system".into(),
            detail: String::new(),
        };
        assert!(js.build_env("/usr/bin").is_empty());
        assert_eq!(js.path_with("/bin"), "/bin");
    }
}
