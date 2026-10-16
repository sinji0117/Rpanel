//! WebSocket: 실시간 서버 지표 / 배포 로그 스트림
//! 인증은 `/api/auth/ws-ticket` 으로 받은 30초 1회성 티켓(?ticket=)으로 합니다.

use std::time::Duration;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    response::Response,
};
use futures::StreamExt;
use redis::AsyncCommands;
use serde::Deserialize;

use crate::{
    auth::session,
    deploy::logsink::{self, LogEvent},
    error::{AppError, AppResult},
    metrics::Collector,
    state::AppState,
};

#[derive(Deserialize)]
pub struct TicketQuery {
    pub ticket: String,
}

async fn authorize(state: &AppState, ticket: &str) -> AppResult<u64> {
    session::consume_ws_ticket(state, ticket)
        .await?
        .ok_or(AppError::Unauthorized)
}

async fn send_json(socket: &mut WebSocket, v: &impl serde::Serialize) -> bool {
    socket
        .send(Message::Text(serde_json::to_string(v).unwrap().into()))
        .await
        .is_ok()
}

// ───────────── 지표 ─────────────

pub async fn metrics(
    State(state): State<AppState>,
    Query(q): Query<TicketQuery>,
    ws: WebSocketUpgrade,
) -> AppResult<Response> {
    authorize(&state, &q.ticket).await?;
    Ok(ws.on_upgrade(metrics_loop))
}

async fn metrics_loop(mut socket: WebSocket) {
    let mut collector = Collector::new();
    // 첫 값은 CPU 사용률 기준점 확보 후
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut tick = tokio::time::interval(Duration::from_secs(2));
    loop {
        tokio::select! {
            _ = tick.tick() => {
                let (c, snap) = match tokio::task::spawn_blocking(move || { let s = collector.snapshot(); (collector, s) }).await {
                    Ok(v) => v,
                    Err(_) => break,
                };
                collector = c;
                if !send_json(&mut socket, &serde_json::json!({ "type": "metrics", "data": snap })).await {
                    break;
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }
}

// ───────────── 배포 로그 ─────────────

pub async fn deployment_log(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    Query(q): Query<TicketQuery>,
    ws: WebSocketUpgrade,
) -> AppResult<Response> {
    authorize(&state, &q.ticket).await?;
    let exists: Option<(u64,)> = sqlx::query_as("SELECT id FROM deployments WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    if exists.is_none() {
        return Err(AppError::NotFound);
    }
    Ok(ws.on_upgrade(move |socket| async move {
        if let Err(e) = deployment_log_loop(state, id, socket).await {
            tracing::debug!(error = %e, "deployment log socket ended");
        }
    }))
}

async fn deploy_status(state: &AppState, id: u64) -> anyhow::Result<(String, Option<String>)> {
    let status: String = sqlx::query_scalar("SELECT status FROM deployments WHERE id = ?")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    // 진행 중이면 로그는 Redis 에서 오므로 본문은 끝난 뒤에만 읽음
    let log = if is_done(&status) {
        crate::logstore::deploy_log_get(state, id).await?
    } else {
        None
    };
    Ok((status, log))
}

fn is_done(status: &str) -> bool {
    !matches!(status, "queued" | "running")
}

async fn deployment_log_loop(
    state: AppState,
    id: u64,
    mut socket: WebSocket,
) -> anyhow::Result<()> {
    let prefix = state.cfg.redis_prefix.clone();
    // 1) 먼저 구독 → 2) 백로그 전송 → 3) seq 로 중복 제거하며 실시간 전달 (빈틈 없음)
    let mut pubsub = state.redis_client.get_async_pubsub().await?;
    pubsub.subscribe(logsink::channel(&prefix, id)).await?;

    let mut r = state.redis.clone();
    let backlog: Vec<String> = r.lrange(logsink::list_key(&prefix, id), 0, -1).await?;
    let mut last_seq = 0u64;
    for raw in &backlog {
        if let Ok(LogEvent::Line { seq, .. }) = serde_json::from_str::<LogEvent>(raw) {
            last_seq = last_seq.max(seq);
        }
        if socket
            .send(Message::Text(raw.clone().into()))
            .await
            .is_err()
        {
            return Ok(());
        }
    }

    let (status, db_log) = deploy_status(&state, id).await?;
    if is_done(&status) {
        // 끝난 배포: 백로그가 만료됐으면 DB 로그로 대체
        if backlog.is_empty() {
            for (i, line) in db_log.unwrap_or_default().lines().enumerate() {
                if !send_json(
                    &mut socket,
                    &LogEvent::Line {
                        seq: i as u64 + 1,
                        text: line.to_string(),
                    },
                )
                .await
                {
                    return Ok(());
                }
            }
        }
        send_json(&mut socket, &LogEvent::Status { status }).await;
        let _ = socket.send(Message::Close(None)).await;
        return Ok(());
    }

    let mut stream = pubsub.on_message();
    let mut ping = tokio::time::interval(Duration::from_secs(20));
    // 상태 메시지를 놓쳤을 때를 대비한 주기적 DB 확인
    let mut poll = tokio::time::interval(Duration::from_secs(5));
    loop {
        tokio::select! {
            msg = stream.next() => {
                let Some(msg) = msg else { break };
                let payload: String = msg.get_payload()?;
                match serde_json::from_str::<LogEvent>(&payload) {
                    Ok(LogEvent::Line { seq, .. }) if seq <= last_seq => continue,
                    Ok(LogEvent::Line { seq, .. }) => last_seq = seq,
                    Ok(LogEvent::Status { .. }) => {
                        let _ = socket.send(Message::Text(payload.into())).await;
                        let _ = socket.send(Message::Close(None)).await;
                        return Ok(());
                    }
                    Err(_) => continue,
                }
                if socket.send(Message::Text(payload.into())).await.is_err() {
                    break;
                }
            }
            _ = ping.tick() => {
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() { break; }
            }
            _ = poll.tick() => {
                if let Ok((status, _)) = deploy_status(&state, id).await {
                    if is_done(&status) {
                        // 아직 전달되지 않은 줄을 백로그에서 보충
                        let rest: Vec<String> = r.lrange(logsink::list_key(&prefix, id), 0, -1).await.unwrap_or_default();
                        for raw in rest {
                            if let Ok(LogEvent::Line { seq, .. }) = serde_json::from_str::<LogEvent>(&raw) {
                                if seq > last_seq {
                                    last_seq = seq;
                                    let _ = socket.send(Message::Text(raw.into())).await;
                                }
                            }
                        }
                        send_json(&mut socket, &LogEvent::Status { status }).await;
                        let _ = socket.send(Message::Close(None)).await;
                        break;
                    }
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }
    Ok(())
}
