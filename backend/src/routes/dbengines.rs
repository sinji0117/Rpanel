//! 데이터베이스 서버 버전 API

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    audit,
    auth::{AuthUser, ClientIp},
    dbengines::{self, Engine},
    error::{AppError, AppResult},
    state::AppState,
};

/// GET /api/db-engines
pub async fn list(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<Value>> {
    user.require_admin()?;
    let mut out = Vec::new();
    for e in Engine::ALL {
        let version = dbengines::current_version(&state, e).await;
        let series = version.as_deref().and_then(|v| dbengines::series(e, v));
        let user_dbs = dbengines::user_db_count(&state, e).await;
        let versions: Vec<Value> = e
            .versions()
            .iter()
            .map(|(v, note)| {
                let check = dbengines::check_target(e, series.as_deref(), v).and_then(|d| {
                    if user_dbs > 0 {
                        Err(format!("만든 데이터베이스가 {user_dbs}개 있어 버전을 바꿀 수 없습니다"))
                    } else {
                        Ok(d)
                    }
                });
                json!({
                    "version": v, "note": note,
                    "current": series.as_deref() == Some(*v),
                    "available": check.is_ok(),
                    "direction": check.as_ref().ok().map(|d| if *d == dbengines::Direction::Down { "down" } else { "up" }),
                    "reason": check.err(),
                })
            })
            .collect();
        out.push(json!({
            "engine": e.as_str(),
            "label": e.label(),
            "installed": version.is_some(),
            "version": version,
            "series": series,
            "versions": versions,
            "user_dbs": user_dbs,
            "job": dbengines::job(e),
            "last": dbengines::last_result(&state, e).await,
        }));
    }
    Ok(Json(json!({ "engines": out })))
}

#[derive(Deserialize)]
pub struct Input {
    pub version: String,
}

/// POST /api/db-engines/{engine} — 더 높은 버전으로 바꾸기 (먼저 전체 백업)
pub async fn upgrade(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(engine): Path<String>,
    Json(input): Json<Input>,
) -> AppResult<(StatusCode, Json<Value>)> {
    user.require_admin()?;
    let e = Engine::parse(&engine).ok_or(AppError::NotFound)?;
    if let Some(r) = dbengines::running() {
        return Err(AppError::Conflict(format!(
            "{r} 버전을 바꾸는 중입니다. 끝난 뒤 다시 시도하세요"
        )));
    }
    let cur = dbengines::current_version(&state, e).await;
    let series = cur.as_deref().and_then(|v| dbengines::series(e, v));
    let dir = dbengines::check_target(e, series.as_deref(), &input.version)
        .map_err(AppError::BadRequest)?;
    let n = dbengines::user_db_count(&state, e).await;
    if n > 0 {
        return Err(AppError::Conflict(format!(
            "{} 에 만든 데이터베이스가 {n}개 있어 버전을 바꿀 수 없습니다. 데이터베이스를 백업·삭제한 뒤 바꾸세요",
            e.label()
        )));
    }
    audit::log(
        &state,
        Some(user.id),
        "db_engine.upgrade",
        &format!(
            "{} {} → {}",
            e.as_str(),
            series.unwrap_or_default(),
            input.version
        ),
        &ip,
    )
    .await;
    dbengines::start(state.clone(), e, input.version.clone(), dir);
    Ok((
        StatusCode::ACCEPTED,
        Json(
            json!({ "engine": e.as_str(), "target": input.version, "status": "running", "direction": if dir == dbengines::Direction::Down { "down" } else { "up" } }),
        ),
    ))
}
