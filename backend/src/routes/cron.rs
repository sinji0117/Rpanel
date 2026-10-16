//! 크론 작업 — 앱별 예약 명령을 systemd timer 로 등록합니다.
//! 패널이 꺼져 있어도 systemd 가 실행하고, 결과는 journal 에 남습니다.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    audit,
    auth::{AuthUser, ClientIp},
    deploy::{
        executor::CmdSpec,
        helper,
        logsink::LogSink,
        runner::RUN_USER,
        templates::{self, AppPaths},
    },
    error::{AppError, AppResult},
    models::{App, CronJob, CRON_COLUMNS},
    routes::apps,
    state::AppState,
    validate,
};

const PATH_ENV: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

/// OnCalendar 문자 집합 (helper 와 같은 규칙)
pub fn valid_schedule(s: &str) -> Result<(), String> {
    let ok = !s.trim().is_empty()
        && s.len() <= 100
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "*:/,. ~-".contains(c));
    if ok {
        Ok(())
    } else {
        Err(
            "일정 형식이 올바르지 않습니다 (systemd OnCalendar 예: *-*-* 03:00:00, hourly, *:0/5)"
                .into(),
        )
    }
}

/// systemd-analyze 로 일정 해석 → 다음 실행 시각. 도구가 없으면 None.
async fn preview(state: &AppState, schedule: &str) -> Result<Option<String>, String> {
    if state.exec.is_dry_run() {
        return Ok(None);
    }
    let log = LogSink::capture();
    let out = state
        .exec
        .run(
            CmdSpec::new("systemd-analyze", &["calendar", "--iterations=1", schedule])
                .quiet()
                .timeout(5),
            &log,
        )
        .await;
    match out {
        Ok(o) if o.ok() => Ok(o.stdout.lines().find_map(|l| {
            l.trim()
                .strip_prefix("Next elapse:")
                .map(|s| s.trim().to_string())
        })),
        Ok(o) => Err(format!(
            "systemd가 해석할 수 없는 일정입니다: {}",
            o.stderr.lines().next().unwrap_or("").trim()
        )),
        Err(_) => Ok(None), // systemd-analyze 없음
    }
}

#[derive(Serialize)]
pub struct CronView {
    #[serde(flatten)]
    pub job: CronJob,
    pub unit: String,
    pub status: Option<std::collections::HashMap<String, String>>,
}

pub async fn jobs_for_app(state: &AppState, app_id: u64) -> AppResult<Vec<CronJob>> {
    Ok(sqlx::query_as::<_, CronJob>(&format!(
        "SELECT {CRON_COLUMNS} FROM cron_jobs WHERE app_id = ? ORDER BY id"
    ))
    .bind(app_id)
    .fetch_all(&state.db)
    .await?)
}

async fn status(
    state: &AppState,
    app: &App,
    job: &CronJob,
) -> Option<std::collections::HashMap<String, String>> {
    if state.exec.is_dry_run() || !job.enabled {
        return None;
    }
    let out = helper::call_quiet(state, &["cron-status", &app.slug, &job.id.to_string()])
        .await
        .ok()?;
    Some(
        out.stdout
            .lines()
            .filter_map(|l| {
                l.split_once('=')
                    .map(|(k, v)| (k.to_string(), v.to_string()))
            })
            .collect(),
    )
}

/// GET /api/apps/{id}/cron
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<u64>,
) -> AppResult<Json<Vec<CronView>>> {
    user.require_admin()?;
    let app = apps::fetch(&state, id).await?;
    let mut out = Vec::new();
    for job in jobs_for_app(&state, id).await? {
        let st = status(&state, &app, &job).await;
        out.push(CronView {
            unit: templates::cron_unit_name(&app.slug, job.id),
            status: st,
            job,
        });
    }
    Ok(Json(out))
}

#[derive(Deserialize)]
pub struct CronInput {
    pub name: String,
    pub schedule: String,
    pub command: String,
    #[serde(default = "yes")]
    pub enabled: bool,
}
fn yes() -> bool {
    true
}

async fn check_input(state: &AppState, app: &App, i: &CronInput) -> AppResult<Option<String>> {
    if !app.kind().is_service() && app.site_user.is_none() {
        return Err(AppError::bad(
            "크론은 Rust·Python 앱과 파일 사이트에서 쓸 수 있습니다",
        ));
    }
    validate::name(&i.name).map_err(AppError::BadRequest)?;
    validate::command(&i.command).map_err(AppError::BadRequest)?;
    if i.command.trim().is_empty() {
        return Err(AppError::bad("명령을 입력하세요"));
    }
    valid_schedule(i.schedule.trim()).map_err(AppError::BadRequest)?;
    preview(state, i.schedule.trim())
        .await
        .map_err(AppError::BadRequest)
}

/// 활성 상태면 유닛 설치, 아니면 제거
async fn sync(state: &AppState, app: &App, job: &CronJob) -> AppResult<()> {
    let log = LogSink::capture();
    if !job.enabled {
        helper::call(
            state,
            &["remove-cron", &app.slug, &job.id.to_string()],
            &log,
        )
        .await
        .map_err(|e| AppError::BadRequest(format!("{e:#}")))?;
        return Ok(());
    }
    if app.current_release.is_none() {
        return Err(AppError::bad(
            "앱을 먼저 배포하세요 (크론은 배포된 릴리스와 env 파일을 사용합니다)",
        ));
    }
    let paths = AppPaths::new(&state.cfg.data_dir, &app.slug);
    let (svc, tmr) = templates::cron_staging(&state.cfg.data_dir, &app.slug, job.id);
    state
        .exec
        .write_file(
            &svc,
            &templates::cron_service(app, &paths, job, RUN_USER, PATH_ENV),
            0o644,
            &log,
        )
        .await?;
    state
        .exec
        .write_file(&tmr, &templates::cron_timer(app, job), 0o644, &log)
        .await?;
    helper::call(
        state,
        &["install-cron", &app.slug, &job.id.to_string()],
        &log,
    )
    .await
    .map_err(|e| AppError::BadRequest(format!("{e:#}")))?;
    Ok(())
}

async fn fetch_job(state: &AppState, id: u64) -> AppResult<(CronJob, App)> {
    let job = sqlx::query_as::<_, CronJob>(&format!(
        "SELECT {CRON_COLUMNS} FROM cron_jobs WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    let app = apps::fetch(state, job.app_id).await?;
    Ok((job, app))
}

/// POST /api/apps/{id}/cron
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
    Json(i): Json<CronInput>,
) -> AppResult<(StatusCode, Json<serde_json::Value>)> {
    user.require_admin()?;
    let app = apps::fetch(&state, id).await?;
    let next = check_input(&state, &app, &i).await?;
    if i.enabled && app.current_release.is_none() {
        return Err(AppError::bad(
            "앱을 먼저 배포하세요 (크론은 배포된 릴리스와 env 파일을 사용합니다)",
        ));
    }
    let res = sqlx::query(
        "INSERT INTO cron_jobs (app_id, name, schedule, command, enabled) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(i.name.trim())
    .bind(i.schedule.trim())
    .bind(i.command.trim())
    .bind(i.enabled)
    .execute(&state.db)
    .await?;
    let (job, app) = fetch_job(&state, res.last_insert_id()).await?;
    if let Err(e) = sync(&state, &app, &job).await {
        sqlx::query("DELETE FROM cron_jobs WHERE id = ?")
            .bind(job.id)
            .execute(&state.db)
            .await?;
        return Err(e);
    }
    audit::log(
        &state,
        Some(user.id),
        "cron.create",
        &format!("{} #{}", app.slug, job.id),
        &ip,
    )
    .await;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "id": job.id, "next": next })),
    ))
}

/// PUT /api/cron/{id}
pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
    Json(i): Json<CronInput>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let (_, app) = fetch_job(&state, id).await?;
    let next = check_input(&state, &app, &i).await?;
    sqlx::query(
        "UPDATE cron_jobs SET name = ?, schedule = ?, command = ?, enabled = ? WHERE id = ?",
    )
    .bind(i.name.trim())
    .bind(i.schedule.trim())
    .bind(i.command.trim())
    .bind(i.enabled)
    .bind(id)
    .execute(&state.db)
    .await?;
    let (job, app) = fetch_job(&state, id).await?;
    sync(&state, &app, &job).await?;
    audit::log(
        &state,
        Some(user.id),
        "cron.update",
        &format!("{} #{id}", app.slug),
        &ip,
    )
    .await;
    Ok(Json(json!({ "ok": true, "next": next })))
}

/// DELETE /api/cron/{id}
pub async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
) -> AppResult<StatusCode> {
    user.require_admin()?;
    let (job, app) = fetch_job(&state, id).await?;
    let log = LogSink::capture();
    helper::call(
        &state,
        &["remove-cron", &app.slug, &job.id.to_string()],
        &log,
    )
    .await
    .map_err(|e| AppError::BadRequest(format!("{e:#}")))?;
    sqlx::query("DELETE FROM cron_jobs WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    audit::log(
        &state,
        Some(user.id),
        "cron.delete",
        &format!("{} #{id}", app.slug),
        &ip,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/cron/{id}/run — 지금 한 번 실행
pub async fn run_now(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let (job, app) = fetch_job(&state, id).await?;
    if !job.enabled {
        return Err(AppError::bad("비활성화된 작업입니다. 먼저 활성화하세요"));
    }
    let log = LogSink::capture();
    helper::call(&state, &["run-cron", &app.slug, &job.id.to_string()], &log)
        .await
        .map_err(|e| AppError::BadRequest(format!("{e:#}")))?;
    audit::log(
        &state,
        Some(user.id),
        "cron.run",
        &format!("{} #{id}", app.slug),
        &ip,
    )
    .await;
    Ok(Json(json!({ "ok": true })))
}

/// GET /api/cron/{id}/log
pub async fn log(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<u64>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let (job, app) = fetch_job(&state, id).await?;
    let out =
        helper::call_quiet(&state, &["cron-log", &app.slug, &job.id.to_string(), "200"]).await?;
    Ok(Json(
        json!({ "lines": out.stdout.lines().collect::<Vec<_>>() }),
    ))
}

#[derive(Deserialize)]
pub struct PreviewInput {
    pub schedule: String,
}

/// POST /api/cron/preview — 일정 검증 + 다음 실행 시각
pub async fn preview_route(
    State(state): State<AppState>,
    _u: AuthUser,
    Json(i): Json<PreviewInput>,
) -> AppResult<Json<serde_json::Value>> {
    let s = i.schedule.trim();
    if let Err(e) = valid_schedule(s) {
        return Ok(Json(json!({ "valid": false, "error": e })));
    }
    match preview(&state, s).await {
        Ok(next) => Ok(Json(json!({ "valid": true, "next": next }))),
        Err(e) => Ok(Json(json!({ "valid": false, "error": e }))),
    }
}

/// 앱 재배포 후에도 크론은 current 심볼릭 링크를 쓰므로 다시 설치할 필요가 없습니다.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedules() {
        assert!(valid_schedule("*-*-* 03:00:00").is_ok());
        assert!(valid_schedule("hourly").is_ok());
        assert!(valid_schedule("*:0/5").is_ok());
        assert!(valid_schedule("Mon..Fri *-*-* 09:30").is_ok());
        assert!(valid_schedule("").is_err());
        assert!(valid_schedule("daily; rm -rf /").is_err());
        assert!(valid_schedule("daily\nExecStart=x").is_err());
        assert!(valid_schedule("$(id)").is_err());
    }
}
