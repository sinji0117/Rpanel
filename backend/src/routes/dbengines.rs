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
        let versions: Vec<Value> = e
            .versions()
            .iter()
            .map(|(v, note)| {
                let check = dbengines::check_target(e, series.as_deref(), v);
                json!({
                    "version": v, "note": note,
                    "current": series.as_deref() == Some(*v),
                    "available": check.is_ok(),
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
    dbengines::check_target(e, series.as_deref(), &input.version).map_err(AppError::BadRequest)?;
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
    dbengines::start(state.clone(), e, input.version.clone());
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "engine": e.as_str(), "target": input.version, "status": "running" })),
    ))
}
