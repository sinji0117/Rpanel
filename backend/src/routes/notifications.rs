//! 알림 API — 목록·읽음 처리·삭제, 알림 설정, 테스트 전송

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    audit,
    auth::{AuthUser, ClientIp},
    error::{AppError, AppResult},
    notify::{self, Event, Settings, EVENTS},
    state::AppState,
};

#[derive(Deserialize)]
pub struct ListQuery {
    pub limit: Option<u32>,
    #[serde(default)]
    pub unread: bool,
}

/// GET /api/notifications?limit=50&unread=true
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<Value>> {
    user.require_admin()?;
    let (items, unread) =
        crate::logstore::notify_list(&state, q.limit.unwrap_or(50).clamp(1, 500), q.unread).await?;
    Ok(Json(json!({ "items": items, "unread": unread })))
}

#[derive(Deserialize)]
pub struct ReadInput {
    #[serde(default)]
    pub ids: Vec<u64>,
    #[serde(default)]
    pub all: bool,
}

/// POST /api/notifications/read — { ids: [..] } 또는 { all: true }
pub async fn mark_read(
    State(state): State<AppState>,
    user: AuthUser,
    Json(input): Json<ReadInput>,
) -> AppResult<Json<Value>> {
    user.require_admin()?;
    crate::logstore::notify_mark_read(&state, &input.ids, input.all).await?;
    let unread = crate::logstore::notify_unread(&state).await?;
    Ok(Json(json!({ "unread": unread })))
}

/// DELETE /api/notifications — 모두 지우기
pub async fn clear(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
) -> AppResult<StatusCode> {
    user.require_admin()?;
    crate::logstore::notify_clear(&state).await?;
    audit::log(&state, Some(user.id), "notifications.clear", "", &ip).await;
    Ok(StatusCode::NO_CONTENT)
}

fn events_catalog() -> Value {
    json!(EVENTS
        .iter()
        .map(|(k, label, d)| json!({ "key": k, "label": label, "default": d }))
        .collect::<Vec<_>>())
}

/// GET /api/notifications/settings
pub async fn get_settings(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<Value>> {
    user.require_admin()?;
    Ok(Json(
        json!({ "settings": notify::load_settings(&state).await, "events": events_catalog() }),
    ))
}

/// PUT /api/notifications/settings
pub async fn put_settings(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Json(mut input): Json<Settings>,
) -> AppResult<Json<Value>> {
    user.require_admin()?;
    let c = &mut input.channels;
    for v in [
        &mut c.discord_url,
        &mut c.slack_url,
        &mut c.telegram_token,
        &mut c.telegram_chat_id,
        &mut c.webhook_url,
    ] {
        *v = v.trim().to_string();
    }
    input.validate().map_err(AppError::BadRequest)?;
    let s = input.normalized();
    notify::save_settings(&state, &s).await?;
    audit::log(&state, Some(user.id), "notifications.settings", "", &ip).await;
    Ok(Json(json!({ "settings": s, "events": events_catalog() })))
}

/// POST /api/notifications/test — 설정된 채널로 시험 알림 (패널 안 목록에도 남김)
pub async fn test(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<Value>> {
    user.require_admin()?;
    let s = notify::load_settings(&state).await;
    let ev = Event::new("test", "info", "시험 알림입니다", json!({}))
        .body("Rpanel 알림이 잘 도착했습니다.");
    crate::logstore::notify_insert(
        &state,
        crate::logstore::NewNotification {
            kind: "test",
            level: "info",
            title: ev.title,
            params: &ev.params,
            body: ev.body.as_deref(),
            link: None,
        },
    )
    .await?;
    if !notify::has_channel(&s.channels) {
        return Ok(Json(json!({ "results": [] })));
    }
    let host = crate::metrics::host_info()
        .hostname
        .unwrap_or_else(|| "rpanel".into());
    let results: Vec<Value> = notify::send_all(&s.channels, &host, &ev)
        .await
        .into_iter()
        .map(|(name, r)| json!({ "channel": name, "ok": r.is_ok(), "error": r.err().map(|e| e.to_string()) }))
        .collect();
    Ok(Json(json!({ "results": results })))
}
