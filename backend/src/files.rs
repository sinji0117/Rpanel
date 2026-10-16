//! 사이트 파일 접근 — `sudo -n -u <사이트유저> rpanel-files <op> ...`
//! 패널(rpanel)은 사이트 파일에 직접 접근하지 않고, 항상 사이트 유저 권한으로만 다룹니다.

use std::process::Stdio;

use tokio::{io::AsyncWriteExt, process::Command};

use crate::{
    config::ExecMode,
    deploy::{
        executor::{run_ok, CmdSpec},
        logsink::LogSink,
    },
    error::{AppError, AppResult},
    models::{App, EnvVar},
    state::AppState,
};

/// 사이트 유저 (파일 기반 사이트만)
pub fn site_user(app: &App) -> AppResult<&str> {
    app.site_user.as_deref().ok_or_else(|| {
        AppError::bad("파일 관리자는 사이트(Rust·Python·HTML 파일) 앱에서만 쓸 수 있습니다")
    })
}

/// /home/<user>/htdocs/<도메인> → "htdocs/<도메인>"
pub fn rel_root(app: &App) -> String {
    match (app.site_user.as_deref(), app.site_root.as_deref()) {
        (Some(u), Some(r)) => r
            .strip_prefix(&format!("/home/{u}/"))
            .unwrap_or("htdocs")
            .to_string(),
        _ => String::new(),
    }
}

pub fn command(state: &AppState, app: &App, args: &[&str]) -> AppResult<Command> {
    let user = site_user(app)?;
    if state.cfg.exec_mode == ExecMode::DryRun {
        return Err(AppError::bad(
            "DRY-RUN 모드에서는 파일 관리자를 쓸 수 없습니다 (RPANEL_EXEC_MODE=real)",
        ));
    }
    let mut c = Command::new("sudo");
    c.arg("-n")
        .arg("-u")
        .arg(user)
        .arg(&state.cfg.files_bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    Ok(c)
}

fn error_from(stderr: &[u8]) -> AppError {
    let msg = String::from_utf8_lossy(stderr);
    let msg = msg
        .lines()
        .last()
        .unwrap_or("파일 작업 실패")
        .trim_start_matches("rpanel-files: ")
        .trim()
        .to_string();
    if msg.contains("a password is required") || msg.contains("not allowed to execute") {
        return AppError::Internal(anyhow::anyhow!(
            "sudo 권한이 없습니다 — /etc/sudoers.d/rpanel 에 rpanel-files 줄이 있는지 확인하세요 ({msg})"
        ));
    }
    AppError::BadRequest(if msg.is_empty() {
        "파일 작업 실패".into()
    } else {
        msg
    })
}

/// 실행해서 stdout 반환 (stdin 은 선택)
pub async fn output(
    state: &AppState,
    app: &App,
    args: &[&str],
    stdin: Option<&[u8]>,
) -> AppResult<Vec<u8>> {
    let mut c = command(state, app, args)?;
    if stdin.is_some() {
        c.stdin(Stdio::piped());
    }
    let mut child = c.spawn().map_err(|e| AppError::Internal(e.into()))?;
    if let (Some(data), Some(mut sin)) = (stdin, child.stdin.take()) {
        sin.write_all(data)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        drop(sin);
    }
    let out = tokio::time::timeout(
        std::time::Duration::from_secs(600),
        child.wait_with_output(),
    )
    .await
    .map_err(|_| AppError::bad("파일 작업 시간 초과"))?
    .map_err(|e| AppError::Internal(e.into()))?;
    if !out.status.success() {
        return Err(error_from(&out.stderr));
    }
    Ok(out.stdout)
}

pub async fn json(state: &AppState, app: &App, args: &[&str]) -> AppResult<serde_json::Value> {
    let out = output(state, app, args, None).await?;
    serde_json::from_slice(&out).map_err(|e| AppError::Internal(e.into()))
}

/// 사이트 유저 권한으로 명령 실행 (빌드 명령). 출력은 배포 로그로.
pub async fn run_command(
    state: &AppState,
    app: &App,
    cmd: &str,
    vars: &[EnvVar],
    log: &LogSink,
) -> anyhow::Result<()> {
    let user = app
        .site_user
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("사이트 유저가 없습니다"))?;
    let env: serde_json::Map<String, serde_json::Value> = vars
        .iter()
        .map(|v| (v.key.clone(), serde_json::Value::String(v.value.clone())))
        .collect();
    let files_bin = state.cfg.files_bin.display().to_string();
    let root = rel_root(app);
    run_ok(
        state.exec.as_ref(),
        CmdSpec::new(
            "sudo",
            &["-n", "-u", user, &files_bin, "run", &root, cmd, "--env"],
        )
        .display(format!("({user}) $ {cmd}"))
        .stdin_data(serde_json::to_vec(&env)?)
        .timeout(1800),
        log,
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stderr_messages() {
        match error_from(b"rpanel-files: \xec\x97\x86\xeb\x8a\x94 \xea\xb2\xbd\xeb\xa1\x9c\xec\x9e\x85\xeb\x8b\x88\xeb\x8b\xa4: x\n") {
            AppError::BadRequest(m) => assert_eq!(m, "없는 경로입니다: x"),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            error_from(b"sudo: a password is required\n"),
            AppError::Internal(_)
        ));
    }
}
