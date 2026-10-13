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
    models::{App, AppKind, EnvVar, APP_COLUMNS},
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
    .bind(result.as_ref().ok())
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
    let vars = load_env(state, app_id).await?;
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
                    &["-C", &repo, "remote", "set-url", "origin", &app.git_url],
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
        .await?;
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
                        &app.git_url,
                        &repo,
                    ],
                )
                .timeout(900),
            ),
            log,
        )
        .await?;
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
    let bash = |cmd: &str| with_env(CmdSpec::new("bash", &["-c", cmd]).cwd(&work), &vars);
    let release_id = deploy_id.to_string();
    let release_dir: PathBuf;

    match kind {
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
        AppKind::Node | AppKind::Static => {
            let has_lock = exec.is_dry_run() || work.join("package-lock.json").is_file();
            let install = if has_lock {
                "npm ci --no-audit --no-fund"
            } else {
                "npm install --no-audit --no-fund"
            };
            run_ok(exec, bash(install).timeout(1800), log).await?;
            let default_build = if kind == AppKind::Static {
                Some("npm run build".to_string())
            } else {
                None
            };
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
        log.step("Nginx 사이트").await;
        match templates::nginx_site(&templates::SiteInput {
            app: &app,
            paths: &paths,
            trusted_proxy: &state.cfg.trusted_proxy,
        }) {
            Some(conf) => {
                exec.write_file(&paths.staging_site, &conf, 0o644, log)
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
    let fut = async {
        let mut s = TcpStream::connect(("127.0.0.1", port))
            .await
            .context("연결 실패")?;
        s.write_all(
            format!("GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nUser-Agent: rpanel-health\r\n\r\n")
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
