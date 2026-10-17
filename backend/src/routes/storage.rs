//! 저장 위치 API (백업 · 프로젝트 파일)

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
    error::{AppError, AppResult},
    state::AppState,
    storage::{self, Kind},
};

/// GET /api/system/storage
pub async fn get(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<Value>> {
    user.require_admin()?;
    let reg = storage::registry(&state);
    let items: Vec<Value> = Kind::ALL
        .into_iter()
        .map(|k| {
            let custom = reg.get(k.as_str()).cloned();
            let default_path = match k {
                Kind::Backups => state.cfg.data_dir.join("backups"),
                Kind::Projects => state.cfg.data_dir.clone(),
            };
            let shown = custom
                .as_ref()
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| default_path.clone());
            let space = storage::fs_space(&shown);
            json!({
                "kind": k.as_str(),
                "label": k.label(),
                "path": shown,
                "custom": custom.is_some(),
                "default_path": default_path,
                "subdirs": k.subdirs(),
                "total_bytes": space.map(|s| s.0),
                "free_bytes": space.map(|s| s.1),
            })
        })
        .collect();
    Ok(Json(json!({
        "data_dir": state.cfg.data_dir,
        "items": items,
        "job": storage::job(),
        "last": storage::last_result(&state).await,
        "busy": storage::busy_reason(&state).await,
    })))
}

#[derive(Deserialize)]
pub struct Input {
    /// 새 위치 (비우면 기본 위치로 되돌림)
    #[serde(default)]
    pub path: String,
}

/// PUT /api/system/storage/{kind} — 데이터를 옮기고 원래 경로에 연결
pub async fn set(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(kind): Path<String>,
    Json(input): Json<Input>,
) -> AppResult<(StatusCode, Json<Value>)> {
    user.require_admin()?;
    let k = Kind::parse(&kind).ok_or(AppError::NotFound)?;
    let path = input.path.trim().to_string();
    let reg = storage::registry(&state);
    if path.is_empty() {
        if !reg.contains_key(k.as_str()) {
            return Err(AppError::bad("이미 기본 위치입니다"));
        }
    } else {
        storage::check_path(k, &path, &reg).map_err(AppError::BadRequest)?;
    }
    if let Some(r) = storage::busy_reason(&state).await {
        return Err(AppError::Conflict(format!("{r}. 끝난 뒤 다시 시도하세요")));
    }
    audit::log(
        &state,
        Some(user.id),
        "storage.set",
        &format!(
            "{} {} → {}",
            k.as_str(),
            reg.get(k.as_str()).map(String::as_str).unwrap_or("default"),
            if path.is_empty() { "default" } else { &path }
        ),
        &ip,
    )
    .await;
    storage::start(state.clone(), k, path.clone());
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "kind": k.as_str(), "target": path, "status": "running" })),
    ))
}
