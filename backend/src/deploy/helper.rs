//! 권한이 필요한 작업은 전부 `sudo -n rpanel-helper <subcommand> <slug>` 로만 수행합니다.
//! helper 스크립트(deploy/rpanel-helper)가 slug를 다시 검증하고, 정해진 경로의 파일만 다룹니다.
//! 패널 프로세스 자체는 root 권한이 없습니다.

use crate::{
    deploy::{
        executor::{run_ok, CmdOutput, CmdSpec},
        logsink::LogSink,
    },
    state::AppState,
};

pub async fn call(state: &AppState, args: &[&str], log: &LogSink) -> anyhow::Result<CmdOutput> {
    let helper = state.cfg.helper_path.display().to_string();
    let mut full: Vec<&str> = vec!["-n", &helper];
    full.extend_from_slice(args);
    run_ok(
        state.exec.as_ref(),
        CmdSpec::new("sudo", &full).timeout(120),
        log,
    )
    .await
}

pub async fn call_quiet(state: &AppState, args: &[&str]) -> anyhow::Result<CmdOutput> {
    let helper = state.cfg.helper_path.display().to_string();
    let mut full: Vec<&str> = vec!["-n", &helper];
    full.extend_from_slice(args);
    let log = LogSink::capture();
    state
        .exec
        .run(CmdSpec::new("sudo", &full).timeout(60).quiet(), &log)
        .await
}

/// 오래 걸릴 수 있는 작업(덤프·복원)용
pub async fn call_long(
    state: &AppState,
    args: &[&str],
    log: &LogSink,
    timeout_secs: u64,
) -> anyhow::Result<CmdOutput> {
    let helper = state.cfg.helper_path.display().to_string();
    let mut full: Vec<&str> = vec!["-n", &helper];
    full.extend_from_slice(args);
    run_ok(
        state.exec.as_ref(),
        CmdSpec::new("sudo", &full).timeout(timeout_secs).quiet(),
        log,
    )
    .await
}

/// 표준입력으로 비밀값을 넘기는 호출 (명령줄·로그에 남지 않음)
pub async fn call_stdin(
    state: &AppState,
    args: &[&str],
    stdin: &str,
    log: &LogSink,
) -> anyhow::Result<CmdOutput> {
    let helper = state.cfg.helper_path.display().to_string();
    let mut full: Vec<&str> = vec!["-n", &helper];
    full.extend_from_slice(args);
    run_ok(
        state.exec.as_ref(),
        CmdSpec::new("sudo", &full)
            .timeout(60)
            .stdin_data(format!("{stdin}\n")),
        log,
    )
    .await
}
