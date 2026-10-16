//! 배포 파이프라인
//!
//! git clone/fetch → 빌드(cargo / npm) → 릴리스 디렉토리 → current 심볼릭 링크 교체
//! → (서비스) env 파일 + systemd 유닛 + 재시작 + 헬스체크 → Nginx 사이트 → 오래된 릴리스 정리
//! 헬스체크 실패 시 이전 릴리스로 자동 롤백합니다.

use std::{path::PathBuf, time::Duration};

use anyhow::{anyhow, bail, Context};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use super::{
    executor::{run_ok, CmdSpec},
    helper,
    logsink::LogSink,
    templates::{self, AppPaths},
};
use crate::{
    models::{App, AppKind, EnvVar, Source, APP_COLUMNS},
    state::AppState,
};

pub const RUN_USER: &str = "rpanel-app";
pub const KEEP_RELEASES: usize = 3;

pub async fn load_app(state: &AppState, app_id: u64) -> anyhow::Result<App> {
    Ok(
        sqlx::query_as::<_, App>(&format!("SELECT {APP_COLUMNS} FROM apps WHERE id = ?"))
            .bind(app_id)
            .fetch_one(&state.db)
            .await?,
    )
}

pub async fn load_env(state: &AppState, app_id: u64) -> anyhow::Result<Vec<EnvVar>> {
    Ok(sqlx::query_as::<_, EnvVar>(
        "SELECT `key`, value FROM app_env WHERE app_id = ? ORDER BY `key`",
    )
    .bind(app_id)
    .fetch_all(&state.db)
    .await?)
}

/// 배포에 쓰는 최종 환경변수: 패널이 관리하는 값(DB·Redis) + 사용자 값 (같은 키는 사용자 값 우선)
pub async fn effective_env(state: &AppState, app: &App) -> anyhow::Result<Vec<EnvVar>> {
    let user = load_env(state, app.id).await?;
    let mut out: Vec<EnvVar> = crate::routes::databases::managed_env(state, app)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .into_iter()
        .filter(|m| !user.iter().any(|u| u.key == m.key))
        .map(|m| EnvVar {
            key: m.key,
            value: m.value,
        })
        .collect();
    out.extend(user);
    Ok(out)
}

/// 배포를 백그라운드로 시작
pub fn spawn(state: AppState, deploy_id: u64, app_id: u64) {
    let (tx, rx) = tokio::sync::watch::channel(false);
    state.cancels.lock().unwrap().insert(deploy_id, tx);
    tokio::spawn(async move {
        let log =
            LogSink::for_deployment(state.redis.clone(), &state.cfg.redis_prefix, deploy_id, rx);
        let result = run(&state, deploy_id, app_id, &log).await;
        finish(&state, deploy_id, app_id, &log, result).await;
        state.cancels.lock().unwrap().remove(&deploy_id);
    });
}

/// 배포 취소 요청. 실행 중인 배포면 true
pub fn cancel(state: &AppState, deploy_id: u64) -> bool {
    match state.cancels.lock().unwrap().get(&deploy_id) {
        Some(tx) => tx.send(true).is_ok(),
        None => false,
    }
}

async fn finish(
    state: &AppState,
    deploy_id: u64,
    app_id: u64,
    log: &LogSink,
    result: anyhow::Result<String>,
) {
    let (status, error) = match &result {
        Ok(_) => ("success", None),
        Err(e) if log.is_cancelled() => ("cancelled", Some(e.to_string())),
        Err(e) => ("failed", Some(format!("{e:#}"))),
    };
    match &result {
        Ok(sha) if sha.is_empty() => log.line("✔ 적용 완료").await,
        Ok(sha) => log.line(format!("✔ 배포 완료 ({})", short(sha))).await,
        Err(e) => log.line(format!("✘ {status}: {e:#}")).await,
    }
    let db = &state.db;
    let _ = sqlx::query(
        "UPDATE deployments SET status = ?, error = ?, log = ?, finished_at = CURRENT_TIMESTAMP(3), \
         commit_sha = COALESCE(commit_sha, ?) WHERE id = ?",
    )
    .bind(status)
    .bind(error.as_deref().map(|e| e.chars().take(1000).collect::<String>()))
    .bind(log.contents())
    .bind(result.as_ref().ok().filter(|s| !s.is_empty()))
    .bind(deploy_id)
    .execute(db)
    .await
    .map_err(|e| tracing::error!(error = %e, "failed to save deployment result"));

    if status != "success" {
        // 이전 릴리스가 살아 있으면 running 유지, 아니면 failed
        let _ = sqlx::query("UPDATE apps SET status = IF(current_release IS NULL, 'failed', 'running') WHERE id = ?")
            .bind(app_id)
            .execute(db)
            .await;
    }
    log.status(status).await;

    if status == "success" || status == "failed" {
        let name: String = sqlx::query_scalar("SELECT name FROM apps WHERE id = ?")
            .bind(app_id)
            .fetch_optional(db)
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| format!("#{app_id}"));
        let params = serde_json::json!({ "app": name, "id": deploy_id });
        let ev = if status == "success" {
            crate::notify::Event::new(
                "deploy_success",
                "success",
                "배포 성공: {app} #{id}",
                params,
            )
        } else {
            crate::notify::Event::new("deploy_failed", "error", "배포 실패: {app} #{id}", params)
                .body(error.clone().unwrap_or_default())
        };
        crate::notify::emit(state, ev.link(format!("/deployments/{deploy_id}"))).await;
    }
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(8)]
}

fn git_env(state: &AppState, paths: &AppPaths, spec: CmdSpec) -> CmdSpec {
    let spec = spec.env("GIT_TERMINAL_PROMPT", "0");
    if state.exec.is_dry_run() || paths.key_file.exists() {
        spec.env(
            "GIT_SSH_COMMAND",
            format!(
                "ssh -i {} -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile={} -o BatchMode=yes",
                paths.key_file.display(),
                paths.known_hosts.display()
            ),
        )
    } else {
        spec.env(
            "GIT_SSH_COMMAND",
            format!(
                "ssh -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile={} -o BatchMode=yes",
                paths.known_hosts.display()
            ),
        )
    }
}

/// git clone/fetch 실패 메시지에 고치는 방법을 덧붙임
fn git_hint(e: anyhow::Error, url: &str, branch: &str) -> anyhow::Error {
    let msg = format!("{e:#}");
    let lower = msg.to_ascii_lowercase();
    let https = url.starts_with("https://") || url.starts_with("http://");
    let hint = if https
        && (lower.contains("could not read username")
            || lower.contains("authentication failed")
            || lower.contains("repository not found")
            || lower.contains("terminal prompts disabled"))
    {
        let ssh = ssh_url(url).unwrap_or_else(|| "git@github.com:계정/저장소.git".into());
        Some(format!(
            "저장소가 비공개이거나 주소가 틀렸습니다. 비공개 저장소는 Git URL 을 SSH 주소({ssh})로 바꾸고, \
             앱의 'Git · 웹훅' 탭에서 Deploy key 를 만들어 GitHub 저장소 Settings → Deploy keys 에 등록한 뒤 다시 배포하세요."
        ))
    } else if lower.contains("permission denied (publickey)")
        || lower.contains("could not read from remote repository")
    {
        Some(
            "저장소에 접근할 수 없습니다. 앱의 'Git · 웹훅' 탭에서 Deploy key 를 만들고, 그 공개키를 저장소의 Deploy keys \
             (GitHub: Settings → Deploy keys, GitLab: Settings → Repository → Deploy keys)에 등록했는지 확인하세요."
                .to_string(),
        )
    } else if lower.contains("remote branch") && lower.contains("not found")
        || lower.contains("couldn't find remote ref")
    {
        Some(format!(
            "브랜치 '{branch}' 가 저장소에 없습니다. 앱 설정의 브랜치를 확인하세요 (예: main / master)."
        ))
    } else if lower.contains("could not resolve host") || lower.contains("connection timed out") {
        Some("서버에서 Git 서버에 접속하지 못했습니다. 주소와 서버의 인터넷(방화벽) 연결을 확인하세요.".to_string())
    } else {
        None
    };
    match hint {
        Some(h) => anyhow::anyhow!("{h}\n원래 오류: {msg}"),
        None => e,
    }
}

/// https://github.com/owner/repo(.git) → git@github.com:owner/repo.git
pub fn ssh_url(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let rest = rest.split_once('@').map(|(_, r)| r).unwrap_or(rest);
    let (host, path) = rest.split_once('/')?;
    let path = path.trim_end_matches('/');
    if host.contains(':') || path.split('/').count() < 2 || path.is_empty() {
        return None;
    }
    let path = path.strip_suffix(".git").unwrap_or(path);
    Some(format!("git@{host}:{path}.git"))
}

#[cfg(test)]
mod git_hint_tests {
    use super::*;

    #[test]
    fn ssh_urls() {
        assert_eq!(
            ssh_url("https://github.com/sinji0117/bandiuri").as_deref(),
            Some("git@github.com:sinji0117/bandiuri.git")
        );
        assert_eq!(
            ssh_url("https://github.com/a/b.git").as_deref(),
            Some("git@github.com:a/b.git")
        );
        assert_eq!(
            ssh_url("https://gitlab.com/g/sub/r/").as_deref(),
            Some("git@gitlab.com:g/sub/r.git")
        );
        assert_eq!(
            ssh_url("https://user:tok@github.com/a/b").as_deref(),
            Some("git@github.com:a/b.git")
        );
        assert_eq!(ssh_url("https://git.example.com:8443/a/b"), None);
        assert_eq!(ssh_url("git@github.com:a/b.git"), None);
        assert_eq!(ssh_url("https://github.com/onlyowner"), None);
    }

    #[test]
    fn hints() {
        let e = anyhow::anyhow!("명령 실패 (exit 128): git clone … — fatal: could not read Username for 'https://github.com': terminal prompts disabled");
        let m = format!(
            "{:#}",
            git_hint(e, "https://github.com/sinji0117/bandiuri", "main")
        );
        assert!(m.contains("git@github.com:sinji0117/bandiuri.git"), "{m}");
        assert!(m.contains("Deploy keys"));
        assert!(m.contains("원래 오류"));
        let e = anyhow::anyhow!("git@github.com: Permission denied (publickey).");
        assert!(
            format!("{:#}", git_hint(e, "git@github.com:a/b.git", "main")).contains("Deploy key")
        );
        let e = anyhow::anyhow!("warning: Could not find remote branch dev to clone. fatal: Remote branch dev not found in upstream origin");
        assert!(
            format!("{:#}", git_hint(e, "git@github.com:a/b.git", "dev")).contains("브랜치 'dev'")
        );
        let e = anyhow::anyhow!("something else");
        assert_eq!(
            format!("{:#}", git_hint(e, "https://x/a/b", "main")),
            "something else"
        );
    }
}

fn with_env(mut spec: CmdSpec, vars: &[EnvVar]) -> CmdSpec {
    for v in vars {
        // 빌드 프로세스 PATH/HOME은 패널이 통제
        if v.key != "PATH" && v.key != "HOME" {
            spec = spec.env(&v.key, v.value.clone());
        }
    }
    spec
}

async fn run(
    state: &AppState,
    deploy_id: u64,
    app_id: u64,
    log: &LogSink,
) -> anyhow::Result<String> {
    let exec = state.exec.as_ref();
    let app = load_app(state, app_id).await?;
    let vars = effective_env(state, &app).await?;
    let kind = app.kind();
    let paths = AppPaths::new(&state.cfg.data_dir, &app.slug);

    log.line(format!(
        "Rpanel 배포 #{deploy_id} — {} ({}) · {} · 브랜치 {}{}",
        app.name,
        app.slug,
        kind.as_str(),
        app.git_branch,
        if exec.is_dry_run() {
            " · DRY-RUN 모드"
        } else {
            ""
        }
    ))
    .await;

    log.line("빌드 슬롯 대기 중…").await;
    let _permit = state
        .build_slots
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| anyhow!("semaphore closed"))?;
    if log.is_cancelled() {
        bail!("사용자가 배포를 취소했습니다");
    }

    sqlx::query(
        "UPDATE deployments SET status = 'running', started_at = CURRENT_TIMESTAMP(3) WHERE id = ?",
    )
    .bind(deploy_id)
    .execute(&state.db)
    .await?;
    sqlx::query("UPDATE apps SET status = 'deploying' WHERE id = ?")
        .bind(app_id)
        .execute(&state.db)
        .await?;

    if app.source() != Source::Git {
        return apply_site(state, deploy_id, &app, &vars, &paths, log).await;
    }

    // 1) 디렉토리
    log.step("작업 디렉토리 준비").await;
    for d in [
        &paths.build_dir,
        &paths.releases_dir,
        &paths.www_releases,
        &paths.staging_site.parent().unwrap().to_path_buf(),
    ] {
        exec.mkdirs(d, log).await?;
    }
    if kind.is_service() {
        helper::call(state, &["prepare-app", &app.slug], log).await?;
    }

    // 2) 소스
    log.step("소스 가져오기").await;
    let repo = paths.repo_dir.display().to_string();
    if exec.exists(&paths.repo_dir.join(".git")) {
        run_ok(
            exec,
            git_env(
                state,
                &paths,
                CmdSpec::new(
                    "git",
                    &["-C", &repo, "remote", "set-url", "origin", app.git_url()],
                ),
            ),
            log,
        )
        .await?;
        run_ok(
            exec,
            git_env(
                state,
                &paths,
                CmdSpec::new(
                    "git",
                    &[
                        "-C",
                        &repo,
                        "fetch",
                        "--depth",
                        "1",
                        "origin",
                        &app.git_branch,
                    ],
                )
                .timeout(600),
            ),
            log,
        )
        .await
        .map_err(|e| git_hint(e, app.git_url(), &app.git_branch))?;
        run_ok(
            exec,
            CmdSpec::new("git", &["-C", &repo, "reset", "--hard", "FETCH_HEAD"]),
            log,
        )
        .await?;
        run_ok(
            exec,
            CmdSpec::new(
                "git",
                &[
                    "-C",
                    &repo,
                    "clean",
                    "-fdx",
                    "-e",
                    "node_modules",
                    "-e",
                    "target",
                ],
            ),
            log,
        )
        .await?;
    } else {
        exec.remove_dir_all(&paths.repo_dir, log).await?;
        run_ok(
            exec,
            git_env(
                state,
                &paths,
                CmdSpec::new(
                    "git",
                    &[
                        "clone",
                        "--depth",
                        "1",
                        "--branch",
                        &app.git_branch,
                        "--",
                        app.git_url(),
                        &repo,
                    ],
                )
                .timeout(900),
            ),
            log,
        )
        .await
        .map_err(|e| git_hint(e, app.git_url(), &app.git_branch))?;
    }
    let sha = run_ok(
        exec,
        CmdSpec::new("git", &["-C", &repo, "rev-parse", "HEAD"]).quiet(),
        log,
    )
    .await?
    .stdout
    .trim()
    .to_string();
    log.line(format!("커밋 {}", short(&sha))).await;
    sqlx::query("UPDATE deployments SET commit_sha = ? WHERE id = ?")
        .bind(&sha)
        .bind(deploy_id)
        .execute(&state.db)
        .await?;

    let work = paths.work_dir(&app.root_dir);
    if !exec.is_dry_run() && !work.is_dir() {
        bail!("root_dir '{}' 가 저장소에 없습니다", app.root_dir);
    }
    let work_s = work.display().to_string();

    // 3) 빌드
    log.step("빌드").await;
    let runtime = crate::runtimes::resolve(state, &app).await?;
    let rt_env = runtime
        .as_ref()
        .map(|r| r.build_env(&state.cfg.build_path))
        .unwrap_or_default();
    if let Some(r) = &runtime {
        log.line(format!("런타임: {}", r.describe())).await;
    }
    let bash = |cmd: &str| {
        let mut spec = with_env(CmdSpec::new("bash", &["-c", cmd]).cwd(&work), &vars);
        for (k, v) in &rt_env {
            spec = spec.env(k, v.clone());
        }
        spec
    };
    let release_id = deploy_id.to_string();
    let release_dir: PathBuf;

    // Rust·Python 앱 안의 프런트엔드(React·Vue 등): 백엔드보다 먼저 npm 으로 빌드
    let frontend = match (kind, app.frontend_dir.as_deref()) {
        (AppKind::Rust | AppKind::Python, Some(d)) if !d.is_empty() => Some(d.to_string()),
        _ => None,
    };
    if let Some(fdir) = &frontend {
        log.step(&format!("프런트엔드 빌드 (npm · {fdir})")).await;
        let node = crate::runtimes::resolve_lang(state, crate::runtimes::Lang::Node, None).await?;
        log.line(format!("런타임: {}", node.describe())).await;
        let fwork = work.join(fdir);
        if !exec.is_dry_run() && !fwork.join("package.json").is_file() {
            bail!("{fdir}/package.json 이 없습니다. 앱 설정의 '프런트엔드 폴더'를 확인하세요 (앱 폴더 기준 경로)");
        }
        let has_lock = exec.is_dry_run() || fwork.join("package-lock.json").is_file();
        let node_env = node.build_env(&state.cfg.build_path);
        let npm = |cmd: &str| {
            let mut spec = with_env(CmdSpec::new("bash", &["-c", cmd]).cwd(&fwork), &vars);
            for (k, v) in &node_env {
                spec = spec.env(k, v.clone());
            }
            spec.timeout(1800)
        };
        run_ok(exec, npm("node --version && npm --version"), log)
            .await
            .map_err(|e| anyhow!("Node.js / npm 을 찾지 못했습니다 — 런타임 메뉴에서 Node.js 를 설치하세요 ({e})"))?;
        run_ok(
            exec,
            npm(if has_lock {
                "npm ci --no-audit --no-fund"
            } else {
                "npm install --no-audit --no-fund"
            }),
            log,
        )
        .await?;
        run_ok(exec, npm("npm run build"), log).await?;
        if !exec.is_dry_run() && !fwork.join(&app.output_dir).is_dir() {
            bail!(
                "{fdir}/{} 폴더가 만들어지지 않았습니다. 앱 설정의 '출력 폴더'(npm run build 결과, 기본 dist)를 확인하세요",
                app.output_dir
            );
        }
    }

    match kind {
        AppKind::Proxy => {
            bail!("Git 배포는 rust / python / static 만 지원합니다")
        }
        AppKind::Python => {
            // 소스를 릴리스 폴더로 복사한 뒤 그 안에 가상환경을 만듦 (릴리스마다 독립 → 롤백 안전)
            release_dir = paths.releases_dir.join(&release_id);
            exec.mkdirs(&release_dir, log).await?;
            run_ok(
                exec,
                CmdSpec::new(
                    "rsync",
                    &[
                        "-a",
                        "--delete",
                        "--exclude",
                        ".git",
                        "--exclude",
                        ".venv",
                        "--exclude",
                        "node_modules",
                        &format!("{work_s}/"),
                        &format!("{}/", release_dir.display()),
                    ],
                )
                .timeout(900),
                log,
            )
            .await?;
            let pip_cache = state.cfg.data_dir.join("cache").join("pip");
            exec.mkdirs(&pip_cache, log).await?;
            let venv_bin = release_dir.join(".venv").join("bin");
            let py_path = format!(
                "{}:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
                venv_bin.display()
            );
            let in_release = |cmd: &str| {
                with_env(CmdSpec::new("bash", &["-c", cmd]).cwd(&release_dir), &vars)
                    .env("PIP_CACHE_DIR", pip_cache.display().to_string())
                    .env("PATH", py_path.clone())
                    .env(
                        "VIRTUAL_ENV",
                        release_dir.join(".venv").display().to_string(),
                    )
            };
            let python = runtime
                .as_ref()
                .map(|r| r.python())
                .unwrap_or_else(|| "python3".into());
            run_ok(
                exec,
                in_release(&python_setup_git(&python)).timeout(1800),
                log,
            )
            .await?;
            if let Some(cmd) = app.build_cmd.clone().filter(|s| !s.trim().is_empty()) {
                run_ok(exec, in_release(&cmd).timeout(1800), log).await?;
            }
            run_ok(
                exec,
                CmdSpec::new("chmod", &["-R", "a+rX", &release_dir.display().to_string()]),
                log,
            )
            .await?;
        }
        AppKind::Rust => {
            let cmd = app
                .build_cmd
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "cargo build --release".into());
            run_ok(exec, bash(&cmd).timeout(3600), log).await?;
            let bin = app.binary_name.clone().unwrap_or_else(|| app.slug.clone());
            let built = work.join("target").join("release").join(&bin);
            if !exec.is_dry_run() && !built.is_file() {
                bail!(
                    "빌드 결과물 {} 이 없습니다. 앱 설정의 '바이너리 이름'이 Cargo.toml의 [package] name / [[bin]] name 과 같은지 확인하세요",
                    built.display()
                );
            }
            release_dir = paths.releases_dir.join(&release_id);
            exec.mkdirs(&release_dir, log).await?;
            let dst = release_dir.join(&bin).display().to_string();
            run_ok(
                exec,
                CmdSpec::new(
                    "install",
                    &["-m", "0755", &built.display().to_string(), &dst],
                ),
                log,
            )
            .await?;
        }
        AppKind::Static => {
            let has_lock = exec.is_dry_run() || work.join("package-lock.json").is_file();
            let install = if has_lock {
                "npm ci --no-audit --no-fund"
            } else {
                "npm install --no-audit --no-fund"
            };
            run_ok(exec, bash(install).timeout(1800), log).await?;
            let default_build = Some("npm run build".to_string());
            if let Some(cmd) = app
                .build_cmd
                .clone()
                .filter(|s| !s.trim().is_empty())
                .or(default_build)
            {
                run_ok(exec, bash(&cmd).timeout(1800), log).await?;
            }
            if kind == AppKind::Static {
                let out = work.join(&app.output_dir);
                if !exec.is_dry_run() && !out.join("index.html").is_file() {
                    bail!(
                        "{}/index.html 이 없습니다. 출력 디렉토리 설정(기본 dist)을 확인하세요",
                        out.display()
                    );
                }
                release_dir = paths.www_releases.join(&release_id);
                exec.mkdirs(&release_dir, log).await?;
                run_ok(
                    exec,
                    CmdSpec::new(
                        "rsync",
                        &[
                            "-a",
                            "--delete",
                            &format!("{}/", out.display()),
                            &format!("{}/", release_dir.display()),
                        ],
                    ),
                    log,
                )
                .await?;
                // Nginx(www-data)가 읽을 수 있도록
                run_ok(
                    exec,
                    CmdSpec::new("chmod", &["-R", "a+rX", &release_dir.display().to_string()]),
                    log,
                )
                .await?;
            } else {
                release_dir = paths.releases_dir.join(&release_id);
                exec.mkdirs(&release_dir, log).await?;
                run_ok(
                    exec,
                    CmdSpec::new(
                        "rsync",
                        &[
                            "-a",
                            "--delete",
                            "--exclude",
                            ".git",
                            &format!("{work_s}/"),
                            &format!("{}/", release_dir.display()),
                        ],
                    )
                    .timeout(900),
                    log,
                )
                .await?;
                run_ok(
                    exec,
                    CmdSpec::new("chmod", &["-R", "a+rX", &release_dir.display().to_string()]),
                    log,
                )
                .await?;
            }
        }
    }

    // 프런트엔드 빌드 결과를 릴리스 안 같은 위치(<프런트엔드 폴더>/<출력 폴더>)로 복사 — 서비스 작업 폴더 기준 경로가 그대로 맞음
    if let Some(fdir) = &frontend {
        let src = work.join(fdir).join(&app.output_dir);
        let dst = release_dir.join(fdir).join(&app.output_dir);
        exec.mkdirs(&dst, log).await?;
        run_ok(
            exec,
            CmdSpec::new(
                "rsync",
                &[
                    "-a",
                    "--delete",
                    &format!("{}/", src.display()),
                    &format!("{}/", dst.display()),
                ],
            ),
            log,
        )
        .await?;
        run_ok(
            exec,
            CmdSpec::new(
                "chmod",
                &["-R", "a+rX", &release_dir.join(fdir).display().to_string()],
            ),
            log,
        )
        .await?;
        log.line(format!(
            "프런트엔드 → {} (프로그램에서는 \"{fdir}/{}\" 경로로 읽으면 됩니다)",
            dst.display(),
            app.output_dir
        ))
        .await;
    }

    // 4) 활성화 — 이 지점부터 실패/취소 시 이전 릴리스·env 로 되돌립니다
    log.step("릴리스 활성화").await;
    let current_link = if kind == AppKind::Static {
        paths.www_current.clone()
    } else {
        paths.current.clone()
    };
    let previous = if exec.is_dry_run() {
        None
    } else {
        tokio::fs::read_link(&current_link).await.ok()
    };
    let previous_env = if kind.is_service() && !exec.is_dry_run() {
        tokio::fs::read_to_string(&paths.env_file).await.ok()
    } else {
        None
    };

    if kind.is_service() {
        exec.write_file(
            &paths.env_file,
            &templates::env_file(&vars, app.port),
            0o600,
            log,
        )
        .await?;
        log.line(format!(
            "env 파일 작성: {} ({}개 변수)",
            paths.env_file.display(),
            vars.len()
        ))
        .await;
        let unit = templates::systemd_unit(&templates::UnitInput {
            app: &app,
            paths: &paths,
            run_user: RUN_USER,
            path_env: "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        })?;
        exec.write_file(&paths.staging_unit, &unit, 0o644, log)
            .await?;
    }
    exec.swap_symlink(&release_dir, &current_link, log).await?;

    let activated: anyhow::Result<()> = async {
        if kind.is_service() {
            helper::call(state, &["install-unit", &app.slug], log).await?;
            helper::call(state, &["svc", "restart", &app.slug], log).await?;
            log.step("헬스체크").await;
            if let Err(e) = health_check(state, &app, log).await {
                log.line(format!("헬스체크 실패: {e:#}")).await;
                if let Ok(j) = helper::call_quiet(state, &["journal", &app.slug, "30"]).await {
                    log.line("── 최근 서비스 로그 (journalctl) ──").await;
                    for l in j.stdout.lines() {
                        log.line(l).await;
                    }
                }
                return Err(e.context("새 릴리스가 정상 응답하지 않아 배포를 중단했습니다"));
            }
        }

        // 5) Nginx
        apply_nginx(state, &app, log).await?;
        Ok(())
    }
    .await;

    if let Err(e) = activated {
        revert(
            state,
            &app,
            &paths,
            &current_link,
            previous,
            previous_env,
            log,
        )
        .await;
        return Err(e);
    }

    // 6) 정리
    prune_releases(state, &app, log).await;

    sqlx::query(
        "UPDATE apps SET status = 'running', current_commit = ?, current_release = ? WHERE id = ?",
    )
    .bind(&sha)
    .bind(deploy_id)
    .bind(app_id)
    .execute(&state.db)
    .await?;
    Ok(sha)
}

/// 활성화 단계 실패/취소 시: current 링크와 env 파일을 이전 상태로 되돌리고 서비스 재시작.
/// 취소 신호와 무관하게 끝까지 수행합니다.
async fn revert(
    state: &AppState,
    app: &App,
    paths: &AppPaths,
    current_link: &std::path::Path,
    previous: Option<PathBuf>,
    previous_env: Option<String>,
    log: &LogSink,
) {
    let log = log.uncancellable();
    let exec = state.exec.as_ref();
    let Some(prev) = previous else {
        log.line("이전 릴리스가 없어 되돌릴 대상이 없습니다").await;
        if app.kind().is_service() {
            let _ = helper::call(state, &["svc", "stop", &app.slug], &log).await;
        }
        return;
    };
    log.line(format!("↩ 이전 릴리스로 되돌림: {}", prev.display()))
        .await;
    if let Err(e) = exec.swap_symlink(&prev, current_link, &log).await {
        log.line(format!("되돌리기 실패: {e:#}")).await;
        return;
    }
    if app.kind().is_service() {
        if let Some(env) = previous_env {
            if exec
                .write_file(&paths.env_file, &env, 0o600, &log)
                .await
                .is_ok()
            {
                log.line("env 파일도 이전 내용으로 되돌렸습니다 (패널의 환경변수 값은 그대로)")
                    .await;
            }
        }
        if helper::call(state, &["svc", "restart", &app.slug], &log)
            .await
            .is_ok()
            && health_check(state, app, &log).await.is_ok()
        {
            log.line("이전 릴리스가 정상 동작 중입니다").await;
        }
    }
}

/// Nginx 사이트 설정 생성 → helper 가 검증·설치 (도메인 없으면 제거)
pub async fn apply_nginx(state: &AppState, app: &App, log: &LogSink) -> anyhow::Result<()> {
    let paths = AppPaths::new(&state.cfg.data_dir, &app.slug);
    log.step("Nginx 사이트").await;
    match templates::nginx_site(&templates::SiteInput {
        app,
        paths: &paths,
        trusted_proxy: &state.cfg.trusted_proxy,
        ipv6: state.cfg.ipv6,
    }) {
        Some(conf) => {
            state
                .exec
                .write_file(&paths.staging_site, &conf, 0o644, log)
                .await?;
            helper::call(state, &["install-site", &app.slug], log).await?;
        }
        None => {
            log.line("도메인이 없어 Nginx 사이트를 만들지 않습니다 (내부 포트로만 접근 가능)")
                .await;
            helper::call(state, &["remove-site", &app.slug], log).await?;
        }
    }
    Ok(())
}

/// 파일 기반 사이트 · 리버스 프록시: 빌드 없이 설정만 적용
async fn apply_site(
    state: &AppState,
    deploy_id: u64,
    app: &App,
    vars: &[EnvVar],
    paths: &AppPaths,
    log: &LogSink,
) -> anyhow::Result<String> {
    let exec = state.exec.as_ref();
    let kind = app.kind();
    exec.mkdirs(&state.cfg.data_dir.join("staging"), log)
        .await?;
    exec.mkdirs(&paths.app_dir, log).await?;

    if app.source() == Source::Files {
        let user = app
            .site_user
            .as_deref()
            .ok_or_else(|| anyhow!("사이트 유저가 없습니다"))?;
        log.line(format!(
            "사이트 유저 {user} · 문서 루트 {}",
            app.document_root().unwrap_or_default()
        ))
        .await;
        // 크론·서비스가 함께 쓰는 env 파일
        exec.write_file(
            &paths.env_file,
            &templates::env_file(vars, app.port),
            0o600,
            log,
        )
        .await?;
        if kind == AppKind::Python {
            log.step("Python 가상환경 (.venv · requirements.txt)").await;
            let runtime = crate::runtimes::resolve(state, app).await?;
            let python = runtime
                .as_ref()
                .map(|r| r.python())
                .unwrap_or_else(|| "python3".into());
            if let Some(r) = &runtime {
                log.line(format!("런타임: {}", r.describe())).await;
            }
            crate::files::run_command(state, app, &python_setup_site(&python), vars, log).await?;
        }
        if let Some(cmd) = app.build_cmd.as_deref().filter(|c| !c.trim().is_empty()) {
            log.step("빌드 명령 (사이트 유저 권한)").await;
            let cmd = if kind == AppKind::Python {
                format!("{PYTHON_ACTIVATE} {cmd}")
            } else {
                cmd.to_string()
            };
            crate::files::run_command(state, app, &cmd, vars, log).await?;
        }
    }

    // 업로드형 Rust 사이트: 실행 파일이 아직 없으면 서비스는 띄우지 않고 기다림
    if kind == AppKind::Rust && app.source() == Source::Files {
        match uploaded_binary(state, app).await? {
            BinaryState::Missing(bin) => {
                // 이전 실행 파일로 돌던 서비스가 있으면 멈춤 (상태와 실제가 어긋나지 않게)
                let _ = helper::call_quiet(state, &["svc", "stop", &app.slug]).await;
                apply_nginx(state, app, log).await?;
                log.line(format!(
                    "※ 실행 파일 {bin} 이 아직 없습니다. 파일 탭에서 빌드한 실행 파일을 올린 뒤 '적용'을 누르면 시작됩니다"
                ))
                .await;
                sqlx::query("UPDATE apps SET status = 'idle', current_release = NULL WHERE id = ?")
                    .bind(app.id)
                    .execute(&state.db)
                    .await?;
                return Ok(String::new());
            }
            BinaryState::Present(bin) => {
                // 업로드한 파일은 실행 권한이 없을 수 있음
                crate::files::run_command(state, app, &format!("chmod u+x -- {bin}"), &[], log)
                    .await?;
            }
            BinaryState::Unknown => {}
        }
    }

    if kind.is_service() {
        log.step("서비스").await;
        let unit = templates::systemd_unit(&templates::UnitInput {
            app,
            paths,
            run_user: RUN_USER,
            path_env: "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        })?;
        exec.write_file(&paths.staging_unit, &unit, 0o644, log)
            .await?;
        helper::call(state, &["install-unit", &app.slug], log).await?;
        helper::call(state, &["svc", "restart", &app.slug], log).await?;
        log.step("헬스체크").await;
        if let Err(e) = health_check(state, app, log).await {
            if let Ok(j) = helper::call_quiet(state, &["journal", &app.slug, "30"]).await {
                log.line("── 최근 서비스 로그 (journalctl) ──").await;
                for l in j.stdout.lines() {
                    log.line(l).await;
                }
            }
            return Err(e.context("서비스가 응답하지 않습니다. 시작 명령과 PORT 사용을 확인하세요"));
        }
    }

    apply_nginx(state, app, log).await?;

    sqlx::query(
        "UPDATE apps SET status = 'running', current_commit = NULL, current_release = ? WHERE id = ?",
    )
    .bind(deploy_id)
    .bind(app.id)
    .execute(&state.db)
    .await?;
    Ok(String::new())
}

/// Git Python 앱: 릴리스 폴더 안에 가상환경을 만들고 requirements.txt 설치
/// (`python` 은 runtimes::Resolved::python() — "python3" 또는 /opt/rpanel/runtimes/bin/python3.X)
fn python_setup_git(python: &str) -> String {
    format!(
        "{python} -m venv .venv && \
         if [ -f requirements.txt ]; then pip install --no-input --disable-pip-version-check -r requirements.txt; \
         else echo 'requirements.txt 없음 — 패키지 설치 생략'; fi"
    )
}

/// 파일 Python 사이트: 문서 루트의 .venv + requirements.txt 설치.
/// 가상환경의 Python 버전이 고른 버전과 다르면 새로 만듦 (버전 변경)
fn python_setup_site(python: &str) -> String {
    format!(
        "PYV='import sys; print(\"%d.%d\" % sys.version_info[:2])'; \
         want=\"$({python} -c \"$PYV\")\" || {{ echo 'Python 실행 파일을 찾지 못했습니다: {python}' >&2; exit 1; }}; \
         have=\"$(.venv/bin/python -c \"$PYV\" 2>/dev/null || true)\"; \
         if [ \"$want\" != \"$have\" ]; then \
           if [ -n \"$have\" ]; then echo \"Python $have → $want: 가상환경을 새로 만듭니다\"; rm -rf .venv; fi; \
           {python} -m venv .venv; \
         fi; \
         if [ -f requirements.txt ]; then .venv/bin/pip install --no-input --disable-pip-version-check -q -r requirements.txt; \
         else echo 'requirements.txt 없음 — 패키지 설치 생략'; fi"
    )
}

/// 사이트 유저로 실행하는 명령 앞에 붙여 가상환경을 켬
const PYTHON_ACTIVATE: &str = "export VIRTUAL_ENV=\"$PWD/.venv\" PATH=\"$PWD/.venv/bin:$PATH\";";

enum BinaryState {
    Missing(String),
    Present(String),
    /// 시작 명령이 ./파일 형태가 아님 (확인하지 않음)
    Unknown,
}

/// 시작 명령의 첫 단어가 `./이름` 이면 그 파일이 문서 루트에 있는지 확인
fn start_binary(app: &App) -> Option<String> {
    let cmd = app.start_cmd.as_deref().unwrap_or("./app");
    let first = cmd.split_whitespace().next()?;
    let name = first.strip_prefix("./")?;
    let ok = !name.is_empty()
        && !name.contains("..")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'));
    ok.then(|| first.to_string())
}

async fn uploaded_binary(state: &AppState, app: &App) -> anyhow::Result<BinaryState> {
    let Some(bin) = start_binary(app) else {
        return Ok(BinaryState::Unknown);
    };
    if state.exec.is_dry_run() {
        return Ok(BinaryState::Present(bin));
    }
    let rel = format!("{}/{}", crate::files::rel_root(app), &bin[2..]);
    Ok(
        match crate::files::json(state, app, &["stat", &rel]).await {
            Ok(_) => BinaryState::Present(bin),
            Err(_) => BinaryState::Missing(bin),
        },
    )
}

async fn health_check(state: &AppState, app: &App, log: &LogSink) -> anyhow::Result<()> {
    let port = app.port.ok_or_else(|| anyhow!("포트가 없습니다"))? as u16;
    if state.exec.is_dry_run() {
        log.line(format!(
            "GET http://127.0.0.1:{port}{}   [dry-run]",
            app.health_path
        ))
        .await;
        return Ok(());
    }
    let mut last_err = anyhow!("응답 없음");
    for attempt in 1..=30 {
        if log.is_cancelled() {
            bail!("사용자가 배포를 취소했습니다");
        }
        match http_status(port, &app.health_path).await {
            Ok(code) if code < 500 => {
                log.line(format!("GET {} → {code} ({attempt}회차)", app.health_path))
                    .await;
                return Ok(());
            }
            Ok(code) => last_err = anyhow!("HTTP {code}"),
            Err(e) => last_err = e,
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Err(last_err.context(format!(
        "127.0.0.1:{port}{} 가 30초 안에 응답하지 않았습니다",
        app.health_path
    )))
}

/// 의존성 없이 HTTP/1.0 GET 상태코드만 확인
pub async fn http_status(port: u16, path: &str) -> anyhow::Result<u16> {
    http_status_host(port, "127.0.0.1", path).await
}

/// Host 헤더를 지정한 GET (Nginx 가상 호스트 확인용)
pub async fn http_status_host(port: u16, host: &str, path: &str) -> anyhow::Result<u16> {
    let fut = async {
        let mut s = TcpStream::connect(("127.0.0.1", port))
            .await
            .context("연결 실패")?;
        s.write_all(
            format!("GET {path} HTTP/1.0\r\nHost: {host}\r\nUser-Agent: rpanel-health\r\nX-Forwarded-Proto: https\r\n\r\n")
                .as_bytes(),
        )
        .await?;
        let mut buf = vec![0u8; 64];
        let n = s.read(&mut buf).await?;
        let head = String::from_utf8_lossy(&buf[..n]);
        let code = head
            .split_whitespace()
            .nth(1)
            .and_then(|c| c.parse::<u16>().ok())
            .ok_or_else(|| anyhow!("HTTP 응답이 아닙니다"))?;
        Ok::<u16, anyhow::Error>(code)
    };
    tokio::time::timeout(Duration::from_secs(3), fut)
        .await
        .map_err(|_| anyhow!("3초 시간 초과"))?
}

async fn prune_releases(state: &AppState, app: &App, log: &LogSink) {
    if state.exec.is_dry_run() {
        return;
    }
    let paths = AppPaths::new(&state.cfg.data_dir, &app.slug);
    for (dir, link) in [
        (&paths.releases_dir, &paths.current),
        (&paths.www_releases, &paths.www_current),
    ] {
        let current = tokio::fs::read_link(link).await.ok();
        let mut ids: Vec<u64> = match std::fs::read_dir(dir) {
            Ok(rd) => rd
                .filter_map(|e| e.ok()?.file_name().to_str()?.parse().ok())
                .collect(),
            Err(_) => continue,
        };
        ids.sort_unstable_by(|a, b| b.cmp(a));
        for id in ids.into_iter().skip(KEEP_RELEASES) {
            let p = dir.join(id.to_string());
            if Some(&p) == current.as_ref() {
                continue;
            }
            if state.exec.remove_dir_all(&p, log).await.is_ok() {
                log.line(format!("오래된 릴리스 삭제: {}", p.display()))
                    .await;
            }
        }
    }
}

/// 성공했던 배포의 릴리스로 되돌리기
pub async fn rollback(
    state: &AppState,
    app: &App,
    deploy_id: u64,
    commit: Option<String>,
) -> anyhow::Result<String> {
    let exec = state.exec.as_ref();
    let log = LogSink::capture();
    let paths = AppPaths::new(&state.cfg.data_dir, &app.slug);
    let (dir, link) = if app.kind() == AppKind::Static {
        (
            paths.www_releases.join(deploy_id.to_string()),
            paths.www_current.clone(),
        )
    } else {
        (
            paths.releases_dir.join(deploy_id.to_string()),
            paths.current.clone(),
        )
    };
    if !exec.is_dry_run() && !dir.is_dir() {
        bail!("릴리스 #{deploy_id} 가 이미 정리되었습니다 (최근 {KEEP_RELEASES}개만 보관)");
    }
    exec.swap_symlink(&dir, &link, &log).await?;
    if app.kind().is_service() {
        helper::call(state, &["svc", "restart", &app.slug], &log).await?;
    }
    sqlx::query(
        "UPDATE apps SET status = 'running', current_release = ?, current_commit = ? WHERE id = ?",
    )
    .bind(deploy_id)
    .bind(commit)
    .bind(app.id)
    .execute(&state.db)
    .await?;
    Ok(log.contents())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn http_status_parses_code() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 256];
            let _ = s.read(&mut buf).await;
            s.write_all(b"HTTP/1.1 204 No Content\r\n\r\n")
                .await
                .unwrap();
        });
        assert_eq!(http_status(port, "/health").await.unwrap(), 204);
    }

    #[tokio::test]
    async fn http_status_connection_refused() {
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        assert!(http_status(port, "/").await.is_err());
    }
}
