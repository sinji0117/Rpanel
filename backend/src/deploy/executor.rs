//! 부수효과(명령 실행·파일 쓰기)를 한 곳으로 모은 추상화.
//! - `RealExecutor`: 실제 실행 (VM102 운영)
//! - `DryRunExecutor`: 무엇을 할지 로그만 남김 (개발·테스트·UI 시연)
//!
//! 명령은 항상 프로그램 + 인자 배열로 실행합니다. 사용자 정의 빌드/시작 명령만
//! `bash -lc`로 실행되며, 이는 관리자가 직접 입력한 값입니다.

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use anyhow::{anyhow, bail, Context};
use futures::future::BoxFuture;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

use super::logsink::LogSink;

#[derive(Debug, Clone)]
pub struct CmdSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub timeout: Duration,
    /// 출력을 로그에 남길지 (journal 조회처럼 결과만 필요한 경우 false)
    pub stream: bool,
    /// 로그에 표시할 명령 (민감정보 가리기용). None이면 program+args.
    pub display: Option<String>,
}

impl CmdSpec {
    pub fn new(program: impl Into<String>, args: &[&str]) -> Self {
        Self {
            program: program.into(),
            args: args.iter().map(|s| s.to_string()).collect(),
            cwd: None,
            env: vec![],
            timeout: Duration::from_secs(600),
            stream: true,
            display: None,
        }
    }
    pub fn cwd(mut self, p: impl Into<PathBuf>) -> Self {
        self.cwd = Some(p.into());
        self
    }
    pub fn env(mut self, k: &str, v: impl Into<String>) -> Self {
        self.env.push((k.to_string(), v.into()));
        self
    }
    pub fn timeout(mut self, secs: u64) -> Self {
        self.timeout = Duration::from_secs(secs);
        self
    }
    pub fn quiet(mut self) -> Self {
        self.stream = false;
        self
    }
    pub fn display(mut self, d: impl Into<String>) -> Self {
        self.display = Some(d.into());
        self
    }
    pub fn render(&self) -> String {
        if let Some(d) = &self.display {
            return d.clone();
        }
        let mut s = self.program.clone();
        for a in &self.args {
            s.push(' ');
            if a.is_empty()
                || a.chars()
                    .any(|c| c.is_whitespace() || "'\"$&;|<>()`\\".contains(c))
            {
                s.push('\'');
                s.push_str(&a.replace('\'', "'\\''"));
                s.push('\'');
            } else {
                s.push_str(a);
            }
        }
        s
    }
}

#[derive(Debug, Clone, Default)]
pub struct CmdOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CmdOutput {
    pub fn ok(&self) -> bool {
        self.code == 0
    }
}

pub trait Executor: Send + Sync {
    fn is_dry_run(&self) -> bool;
    fn run<'a>(
        &'a self,
        spec: CmdSpec,
        log: &'a LogSink,
    ) -> BoxFuture<'a, anyhow::Result<CmdOutput>>;
    fn write_file<'a>(
        &'a self,
        path: &'a Path,
        content: &'a str,
        mode: u32,
        log: &'a LogSink,
    ) -> BoxFuture<'a, anyhow::Result<()>>;
    fn mkdirs<'a>(&'a self, path: &'a Path, log: &'a LogSink) -> BoxFuture<'a, anyhow::Result<()>>;
    /// link → target 심볼릭 링크를 원자적으로 교체
    fn swap_symlink<'a>(
        &'a self,
        target: &'a Path,
        link: &'a Path,
        log: &'a LogSink,
    ) -> BoxFuture<'a, anyhow::Result<()>>;
    fn remove_dir_all<'a>(
        &'a self,
        path: &'a Path,
        log: &'a LogSink,
    ) -> BoxFuture<'a, anyhow::Result<()>>;
    fn exists(&self, path: &Path) -> bool;
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, anyhow::Result<String>>;
}

/// 실패 시 에러로 바꿔주는 헬퍼
pub async fn run_ok(
    exec: &dyn Executor,
    spec: CmdSpec,
    log: &LogSink,
) -> anyhow::Result<CmdOutput> {
    let shown = spec.render();
    let out = exec.run(spec, log).await?;
    if !out.ok() {
        let tail: String = out
            .stderr
            .lines()
            .rev()
            .take(5)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join(" / ");
        bail!(
            "명령 실패 (exit {}): {}{}",
            out.code,
            shown,
            if tail.is_empty() {
                String::new()
            } else {
                format!(" — {tail}")
            }
        );
    }
    Ok(out)
}

// ───────────────────────────── Real ─────────────────────────────

pub struct RealExecutor {
    pub base_path: String,
    pub home: PathBuf,
}

impl Executor for RealExecutor {
    fn is_dry_run(&self) -> bool {
        false
    }

    fn run<'a>(
        &'a self,
        spec: CmdSpec,
        log: &'a LogSink,
    ) -> BoxFuture<'a, anyhow::Result<CmdOutput>> {
        Box::pin(async move {
            if spec.stream {
                log.line(format!("$ {}", spec.render())).await;
            }
            let mut cmd = Command::new(&spec.program);
            cmd.args(&spec.args)
                .env_clear()
                .env("PATH", &self.base_path)
                .env("HOME", &self.home)
                .env("LANG", "C.UTF-8")
                .env("TERM", "dumb")
                .env("CARGO_TERM_COLOR", "never")
                .env("NO_COLOR", "1")
                .env("CI", "1")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            for (k, v) in &spec.env {
                cmd.env(k, v);
            }
            if let Some(c) = &spec.cwd {
                cmd.current_dir(c);
            }
            let mut child = cmd
                .spawn()
                .with_context(|| format!("실행 실패: {}", spec.program))?;
            let stdout = child.stdout.take().unwrap();
            let stderr = child.stderr.take().unwrap();
            let stream = spec.stream;

            let out_task = {
                let log = log.clone();
                tokio::spawn(async move {
                    let mut acc = String::new();
                    let mut lines = BufReader::new(stdout).lines();
                    while let Ok(Some(l)) = lines.next_line().await {
                        if stream {
                            log.line(&l).await;
                        }
                        if acc.len() < 1_000_000 {
                            acc.push_str(&l);
                            acc.push('\n');
                        }
                    }
                    acc
                })
            };
            let err_task = {
                let log = log.clone();
                tokio::spawn(async move {
                    let mut acc = String::new();
                    let mut lines = BufReader::new(stderr).lines();
                    while let Ok(Some(l)) = lines.next_line().await {
                        if stream {
                            log.line(&l).await;
                        }
                        if acc.len() < 200_000 {
                            acc.push_str(&l);
                            acc.push('\n');
                        }
                    }
                    acc
                })
            };

            let mut cancel = log.cancel_receiver();
            let status = tokio::select! {
                s = child.wait() => s?,
                _ = tokio::time::sleep(spec.timeout) => {
                    let _ = child.kill().await;
                    bail!("시간 초과({}초): {}", spec.timeout.as_secs(), spec.render());
                }
                _ = async {
                    if cancel.wait_for(|v| *v).await.is_err() {
                        // 취소 채널이 없는 단발 작업: 영원히 대기
                        futures::future::pending::<()>().await;
                    }
                } => {
                    let _ = child.kill().await;
                    bail!("사용자가 배포를 취소했습니다");
                }
            };
            let stdout = out_task.await.unwrap_or_default();
            let stderr = err_task.await.unwrap_or_default();
            Ok(CmdOutput {
                code: status.code().unwrap_or(-1),
                stdout,
                stderr,
            })
        })
    }

    fn write_file<'a>(
        &'a self,
        path: &'a Path,
        content: &'a str,
        mode: u32,
        _log: &'a LogSink,
    ) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            // 임시 파일에 쓰고 rename → 원자적 교체, 권한은 쓰기 전에 설정
            let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
            {
                use tokio::io::AsyncWriteExt;
                let mut f = tokio::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .mode(mode)
                    .open(&tmp)
                    .await
                    .with_context(|| format!("파일 쓰기 실패: {}", tmp.display()))?;
                f.write_all(content.as_bytes()).await?;
                f.flush().await?;
            }
            tokio::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode)).await?;
            tokio::fs::rename(&tmp, path).await?;
            Ok(())
        })
    }

    fn mkdirs<'a>(
        &'a self,
        path: &'a Path,
        _log: &'a LogSink,
    ) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            tokio::fs::create_dir_all(path)
                .await
                .with_context(|| format!("디렉토리 생성 실패: {}", path.display()))
        })
    }

    fn swap_symlink<'a>(
        &'a self,
        target: &'a Path,
        link: &'a Path,
        log: &'a LogSink,
    ) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            let tmp = link.with_file_name(format!(
                ".{}.tmp",
                link.file_name()
                    .and_then(|s| s.to_str())
                    .ok_or_else(|| anyhow!("bad link"))?
            ));
            let _ = tokio::fs::remove_file(&tmp).await;
            tokio::fs::symlink(target, &tmp).await?;
            tokio::fs::rename(&tmp, link).await?;
            log.line(format!("→ {} ⇒ {}", link.display(), target.display()))
                .await;
            Ok(())
        })
    }

    fn remove_dir_all<'a>(
        &'a self,
        path: &'a Path,
        _log: &'a LogSink,
    ) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            match tokio::fs::remove_dir_all(path).await {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.into()),
            }
        })
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, anyhow::Result<String>> {
        Box::pin(async move { Ok(tokio::fs::read_to_string(path).await?) })
    }
}

// ───────────────────────────── Dry run ─────────────────────────────

pub struct DryRunExecutor;

impl Executor for DryRunExecutor {
    fn is_dry_run(&self) -> bool {
        true
    }

    fn run<'a>(
        &'a self,
        spec: CmdSpec,
        log: &'a LogSink,
    ) -> BoxFuture<'a, anyhow::Result<CmdOutput>> {
        Box::pin(async move {
            if spec.stream {
                log.line(format!("$ {}   [dry-run]", spec.render())).await;
            }
            tokio::time::sleep(Duration::from_millis(60)).await;
            if log.is_cancelled() {
                bail!("사용자가 배포를 취소했습니다");
            }
            let stdout = if spec.program == "git" && spec.args.iter().any(|a| a == "rev-parse") {
                "0000000dryrun0000000000000000000000000000\n".to_string()
            } else if spec.program == "sudo" && spec.args.iter().any(|a| a == "status") {
                "inactive\n".to_string()
            } else {
                String::new()
            };
            Ok(CmdOutput {
                code: 0,
                stdout,
                stderr: String::new(),
            })
        })
    }

    fn write_file<'a>(
        &'a self,
        path: &'a Path,
        content: &'a str,
        mode: u32,
        log: &'a LogSink,
    ) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            log.line(format!(
                "write {} (mode {:o}, {} bytes)   [dry-run]",
                path.display(),
                mode,
                content.len()
            ))
            .await;
            Ok(())
        })
    }

    fn mkdirs<'a>(&'a self, path: &'a Path, log: &'a LogSink) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            log.line(format!("mkdir -p {}   [dry-run]", path.display()))
                .await;
            Ok(())
        })
    }

    fn swap_symlink<'a>(
        &'a self,
        target: &'a Path,
        link: &'a Path,
        log: &'a LogSink,
    ) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            log.line(format!(
                "ln -sfn {} {}   [dry-run]",
                target.display(),
                link.display()
            ))
            .await;
            Ok(())
        })
    }

    fn remove_dir_all<'a>(
        &'a self,
        path: &'a Path,
        log: &'a LogSink,
    ) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            log.line(format!("rm -rf {}   [dry-run]", path.display()))
                .await;
            Ok(())
        })
    }

    fn exists(&self, _path: &Path) -> bool {
        false
    }

    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, anyhow::Result<String>> {
        Box::pin(async move { bail!("dry-run: {} 없음", path.display()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_quotes_args() {
        let s = CmdSpec::new("git", &["commit", "-m", "hello world", "it's"]);
        assert_eq!(s.render(), "git commit -m 'hello world' 'it'\\''s'");
        assert_eq!(CmdSpec::new("x", &[]).display("masked").render(), "masked");
    }

    #[tokio::test]
    async fn real_executor_runs_and_captures() {
        let exec = RealExecutor {
            base_path: "/usr/bin:/bin".into(),
            home: std::env::temp_dir(),
        };
        let log = LogSink::capture();
        let out = exec
            .run(
                CmdSpec::new("sh", &["-c", "echo out; echo err >&2; exit 3"]),
                &log,
            )
            .await
            .unwrap();
        assert_eq!(out.code, 3);
        assert_eq!(out.stdout.trim(), "out");
        assert_eq!(out.stderr.trim(), "err");
        let logged = log.contents();
        assert!(logged.contains("$ sh -c"));
        assert!(logged.contains("out"));
        assert!(run_ok(&exec, CmdSpec::new("false", &[]), &log)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn real_executor_env_is_cleared() {
        std::env::set_var("RPANEL_SHOULD_NOT_LEAK", "secret");
        let exec = RealExecutor {
            base_path: "/usr/bin:/bin".into(),
            home: std::env::temp_dir(),
        };
        let log = LogSink::capture();
        let out = exec
            .run(
                CmdSpec::new("sh", &["-c", "echo ${RPANEL_SHOULD_NOT_LEAK:-none} $FOO"])
                    .env("FOO", "bar")
                    .quiet(),
                &log,
            )
            .await
            .unwrap();
        assert_eq!(out.stdout.trim(), "none bar");
        assert!(
            log.contents().is_empty(),
            "quiet commands must not be logged"
        );
    }

    #[tokio::test]
    async fn real_executor_timeout() {
        let exec = RealExecutor {
            base_path: "/usr/bin:/bin".into(),
            home: std::env::temp_dir(),
        };
        let log = LogSink::capture();
        let mut spec = CmdSpec::new("sleep", &["5"]);
        spec.timeout = Duration::from_millis(200);
        let err = exec.run(spec, &log).await.unwrap_err();
        assert!(err.to_string().contains("시간 초과"));
    }

    #[tokio::test]
    async fn real_file_ops() {
        let exec = RealExecutor {
            base_path: "/usr/bin:/bin".into(),
            home: std::env::temp_dir(),
        };
        let log = LogSink::capture();
        let dir = std::env::temp_dir().join(format!("rpanel-exec-{}", std::process::id()));
        let f = dir.join("sub/a.env");
        exec.write_file(&f, "A=1\n", 0o600, &log).await.unwrap();
        let meta = std::fs::metadata(&f).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        assert_eq!(exec.read_to_string(&f).await.unwrap(), "A=1\n");

        let r1 = dir.join("r1");
        let r2 = dir.join("r2");
        exec.mkdirs(&r1, &log).await.unwrap();
        exec.mkdirs(&r2, &log).await.unwrap();
        let link = dir.join("current");
        exec.swap_symlink(&r1, &link, &log).await.unwrap();
        exec.swap_symlink(&r2, &link, &log).await.unwrap();
        assert_eq!(std::fs::read_link(&link).unwrap(), r2);
        exec.remove_dir_all(&dir, &log).await.unwrap();
        assert!(!dir.exists());
        exec.remove_dir_all(&dir, &log).await.unwrap();
    }
}
