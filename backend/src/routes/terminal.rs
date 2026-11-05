//! 웹 터미널
//!
//! - 사이트 터미널: 그 사이트의 리눅스 계정으로 열림 (SSH 로 접속한 것과 같은 권한). 관리자만.
//! - 서버 터미널(root): 기본 꺼짐. 서버에서 `sudo rpanel-terminal on` 으로만 켤 수 있고(패널은 못 켬),
//!   관리자 + 2단계 인증 계정이 매번 새 인증 코드를 넣어야 열립니다. 열 때마다 알림 · 감사 기록.
//!
//! 흐름: POST /api/terminal/ticket → 30초 1회용 티켓 → GET /api/ws/terminal?ticket=&cols=&rows=
//! WebSocket: 브라우저 → 서버  바이너리 = 키 입력,  텍스트 {"type":"resize","cols","rows"}
//!            서버 → 브라우저  바이너리 = 화면 출력, 텍스트 {"type":"exit","code"} / {"type":"error","message"}

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use axum::{
    extract::{
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::Response,
    Json,
};
use once_cell::sync::Lazy;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    audit,
    auth::{session, twofa, AuthUser, ClientIp},
    config::ExecMode,
    deploy::helper,
    error::{AppError, AppResult},
    state::AppState,
    terminal::Pty,
};

/// 한 사람이 동시에 열 수 있는 터미널 수 / 서버 전체
const PER_USER: usize = 4;
const TOTAL: usize = 12;
/// 입력이 없으면 닫는 시간 / 최대 사용 시간
const IDLE: Duration = Duration::from_secs(30 * 60);
const MAX_LIFE: Duration = Duration::from_secs(12 * 3600);

static OPEN: Lazy<Mutex<HashMap<u64, usize>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// 열린 터미널 수를 세는 표 (끝나면 Drop 에서 줄임)
struct Slot(u64);

impl Slot {
    fn take(uid: u64) -> AppResult<Self> {
        let mut m = OPEN.lock().unwrap_or_else(|e| e.into_inner());
        let total: usize = m.values().sum();
        let mine = m.get(&uid).copied().unwrap_or(0);
        if mine >= PER_USER || total >= TOTAL {
            return Err(AppError::Conflict(format!(
                "터미널을 너무 많이 열었습니다 (한 사람 {PER_USER}개, 전체 {TOTAL}개까지). 안 쓰는 터미널을 닫아 주세요"
            )));
        }
        *m.entry(uid).or_insert(0) += 1;
        Ok(Slot(uid))
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut m = OPEN.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(n) = m.get_mut(&self.0) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                m.remove(&self.0);
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Target {
    Site {
        app_id: u64,
        slug: String,
        name: String,
    },
    Root,
}

impl Target {
    fn label(&self) -> String {
        match self {
            Target::Site { slug, .. } => format!("site:{slug}"),
            Target::Root => "root".into(),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Ticket {
    uid: u64,
    sid: String,
    ip: String,
    target: Target,
}

#[derive(sqlx::FromRow)]
struct SiteRow {
    id: u64,
    name: String,
    slug: String,
    site_user: String,
    domains: String,
}

async fn sites(state: &AppState) -> AppResult<Vec<SiteRow>> {
    Ok(sqlx::query_as::<_, SiteRow>(
        "SELECT id, name, slug, site_user, domains FROM apps \
         WHERE site_user IS NOT NULL AND site_user <> '' ORDER BY name",
    )
    .fetch_all(&state.db)
    .await?)
}

fn dry_run(state: &AppState) -> bool {
    state.cfg.exec_mode == ExecMode::DryRun && state.cfg.terminal_cmd.is_none()
}

/// 서버 터미널이 켜져 있는지 (서버의 /etc/rpanel/terminal-root, helper 만 확인 가능)
async fn root_enabled(state: &AppState) -> bool {
    if let Some(cmd) = &state.cfg.terminal_cmd {
        // 테스트: 명령 끝에 "root-on" 표시가 있으면 켜진 것으로
        return cmd.iter().any(|a| a == "root-on");
    }
    if dry_run(state) {
        return false;
    }
    match helper::call_quiet(state, &["term-root-status"]).await {
        Ok(o) => o.code == 0 && o.stdout.trim() == "on",
        Err(_) => false,
    }
}

async fn totp_enabled(state: &AppState, uid: u64) -> AppResult<bool> {
    Ok(twofa::load(state, uid).await?.totp_enabled)
}

/// GET /api/terminal — 열 수 있는 터미널
pub async fn status(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<Value>> {
    user.require_full_admin()?;
    let list: Vec<Value> = sites(&state)
        .await?
        .into_iter()
        .map(|s| {
            json!({
                "id": s.id,
                "name": s.name,
                "slug": s.slug,
                "user": s.site_user,
                "domain": s.domains.split_whitespace().next().unwrap_or(""),
            })
        })
        .collect();
    Ok(Json(json!({
        "available": !dry_run(&state),
        "sites": list,
        "root": {
            "enabled": root_enabled(&state).await,
            "twofa": totp_enabled(&state, user.id).await?,
        },
        "limits": { "per_user": PER_USER, "idle_minutes": IDLE.as_secs() / 60 },
    })))
}

#[derive(Deserialize)]
pub struct TicketInput {
    pub kind: String,
    pub app_id: Option<u64>,
    pub code: Option<String>,
}

/// POST /api/terminal/ticket — 30초 1회용 티켓
pub async fn ticket(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Json(b): Json<TicketInput>,
) -> AppResult<Json<Value>> {
    user.require_full_admin()?;
    if dry_run(&state) {
        return Err(AppError::bad("둘러보기 모드에서는 터미널을 쓸 수 없습니다"));
    }
    let target = match b.kind.as_str() {
        "site" => {
            let id = b.app_id.ok_or_else(|| AppError::bad("사이트를 고르세요"))?;
            let s = sites(&state)
                .await?
                .into_iter()
                .find(|s| s.id == id)
                .ok_or(AppError::NotFound)?;
            Target::Site {
                app_id: s.id,
                slug: s.slug,
                name: s.name,
            }
        }
        "root" => {
            if !root_enabled(&state).await {
                return Err(AppError::Forbidden(
                    "서버 터미널이 꺼져 있습니다. 서버에서 sudo rpanel-terminal on 으로 켜세요"
                        .into(),
                ));
            }
            let row = twofa::load(&state, user.id).await?;
            if !row.totp_enabled {
                return Err(AppError::Forbidden(
                    "서버 터미널은 2단계 인증을 켠 계정만 쓸 수 있습니다".into(),
                ));
            }
            twofa::limit(&state, user.id).await?;
            // 인증 앱의 6자리 코드만 (복구 코드는 휴대폰을 잃었을 때 로그인용이라 여기선 받지 않음)
            let code = b.code.as_deref().unwrap_or("").trim().replace(' ', "");
            let six_digits = code.len() == 6 && code.bytes().all(|c| c.is_ascii_digit());
            if !six_digits
                || twofa::check_code(&state, user.id, &row, &code)
                    .await?
                    .is_none()
            {
                audit::log(&state, Some(user.id), "terminal.root_denied", "root", &ip).await;
                return Err(AppError::bad("인증 코드가 맞지 않습니다"));
            }
            Target::Root
        }
        _ => return Err(AppError::bad("kind 는 site 또는 root 입니다")),
    };
    let t = session::random_token(24);
    let v = serde_json::to_string(&Ticket {
        uid: user.id,
        sid: user.sid.clone(),
        ip,
        target,
    })
    .map_err(|e| AppError::Internal(e.into()))?;
    let mut r = state.redis.clone();
    let _: () = r.set_ex(state.key(&["termt", &t]), v, 30).await?;
    Ok(Json(json!({ "ticket": t, "expires_in": 30 })))
}

#[derive(Deserialize)]
pub struct WsQuery {
    pub ticket: String,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
}

async fn take_ticket(state: &AppState, t: &str) -> AppResult<Ticket> {
    if t.len() != 48 || !t.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(AppError::Unauthorized);
    }
    let mut r = state.redis.clone();
    let v: Option<String> = redis::cmd("GETDEL")
        .arg(state.key(&["termt", t]))
        .query_async(&mut r)
        .await?;
    let tk: Ticket = v
        .and_then(|v| serde_json::from_str(&v).ok())
        .ok_or(AppError::Unauthorized)?;
    if session::is_revoked(state, &tk.sid).await? {
        return Err(AppError::Unauthorized);
    }
    // 그사이 권한이 바뀌었을 수 있으니 다시 확인
    let role: Option<String> = sqlx::query_scalar("SELECT role FROM users WHERE id = ?")
        .bind(tk.uid)
        .fetch_optional(&state.db)
        .await?;
    if role.as_deref() != Some("admin") {
        return Err(AppError::Forbidden("관리자 권한이 필요합니다".into()));
    }
    Ok(tk)
}

/// 실행할 명령: sudo -n rpanel-helper term-site <slug> | term-root <사용자 번호>
fn command(state: &AppState, target: &Target, uid: u64) -> Vec<String> {
    if let Some(cmd) = &state.cfg.terminal_cmd {
        return cmd.iter().filter(|a| *a != "root-on").cloned().collect();
    }
    let helper = state.cfg.helper_path.display().to_string();
    let mut v = vec!["sudo".to_string(), "-n".into(), helper];
    match target {
        Target::Site { slug, .. } => v.extend(["term-site".into(), slug.clone()]),
        Target::Root => v.extend(["term-root".into(), uid.to_string()]),
    }
    v
}

/// GET /api/ws/terminal
pub async fn socket(
    State(state): State<AppState>,
    Query(q): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> AppResult<Response> {
    let tk = take_ticket(&state, &q.ticket).await?;
    let slot = Slot::take(tk.uid)?;
    let argv = command(&state, &tk.target, tk.uid);
    let (cols, rows) = (q.cols.unwrap_or(80), q.rows.unwrap_or(24));
    let pty = Pty::spawn(
        &argv,
        &[
            (
                "PATH",
                "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
            ),
            ("TERM", "xterm-256color"),
            ("LANG", "C.UTF-8"),
        ],
        cols,
        rows,
    )
    .map_err(|e| AppError::Internal(anyhow::anyhow!("터미널을 열지 못했습니다: {e}")))?;

    let label = tk.target.label();
    audit::log(&state, Some(tk.uid), "terminal.open", &label, &tk.ip).await;
    if tk.target == Target::Root {
        let email: String = sqlx::query_scalar("SELECT email FROM users WHERE id = ?")
            .bind(tk.uid)
            .fetch_one(&state.db)
            .await
            .unwrap_or_default();
        crate::notify::emit(
            &state,
            crate::notify::Event::new(
                "security",
                "warning",
                "서버 터미널(root)을 열었습니다: {email}",
                json!({ "email": email }),
            )
            .body(format!("IP {}", tk.ip))
            .link("/terminal"),
        )
        .await;
    }

    Ok(ws.on_upgrade(move |socket| async move {
        let started = Instant::now();
        let code = run(socket, pty, &state, &tk.sid).await;
        drop(slot);
        let mins = started.elapsed().as_secs() / 60;
        audit::log(
            &state,
            Some(tk.uid),
            "terminal.close",
            &format!("{label} ({mins}분)"),
            &tk.ip,
        )
        .await;
        tracing::info!(target = %label, uid = tk.uid, ?code, "terminal closed");
    }))
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Control {
    Resize { cols: u16, rows: u16 },
    Ping,
}

async fn send_text(socket: &mut WebSocket, v: Value) -> bool {
    socket
        .send(Message::Text(v.to_string().into()))
        .await
        .is_ok()
}

async fn run(mut socket: WebSocket, mut pty: Pty, state: &AppState, sid: &str) -> Option<i32> {
    let mut buf = vec![0u8; 16 * 1024];
    let started = Instant::now();
    let mut last_input = Instant::now();
    let mut tick = tokio::time::interval(Duration::from_secs(15));
    tick.tick().await;
    let reason: Option<&str> = loop {
        tokio::select! {
            n = pty.read(&mut buf) => {
                match n {
                    Ok(0) | Err(_) => break None,
                    Ok(n) => {
                        if socket.send(Message::Binary(buf[..n].to_vec().into())).await.is_err() {
                            break Some("disconnected");
                        }
                    }
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Binary(b))) => {
                        last_input = Instant::now();
                        if pty.write_all(&b).await.is_err() { break None; }
                    }
                    Some(Ok(Message::Text(t))) => match serde_json::from_str::<Control>(&t) {
                        Ok(Control::Resize { cols, rows }) => { let _ = pty.resize(cols, rows); }
                        Ok(Control::Ping) => {}
                        Err(_) => {
                            // 텍스트로 온 키 입력도 받아 줌
                            last_input = Instant::now();
                            if pty.write_all(t.as_bytes()).await.is_err() { break None; }
                        }
                    },
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break Some("disconnected"),
                    _ => {}
                }
            }
            _ = tick.tick() => {
                if last_input.elapsed() > IDLE {
                    break Some("idle");
                }
                if started.elapsed() > MAX_LIFE {
                    break Some("max");
                }
                // 로그아웃 · 세션 끊기 하면 터미널도 닫힘
                if session::is_revoked(state, sid).await.unwrap_or(false) {
                    break Some("revoked");
                }
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break Some("disconnected");
                }
            }
        }
    };
    let exited = pty.try_exit_code();
    let code = match exited {
        Some(c) => Some(c),
        None => pty.close().await,
    };
    if reason != Some("disconnected") {
        let msg = match reason {
            Some("idle") => json!({ "type": "exit", "code": code, "reason": "idle" }),
            Some("max") => json!({ "type": "exit", "code": code, "reason": "max" }),
            Some("revoked") => json!({ "type": "exit", "code": code, "reason": "revoked" }),
            _ => json!({ "type": "exit", "code": code }),
        };
        send_text(&mut socket, msg).await;
        let _ = socket
            .send(Message::Close(Some(CloseFrame {
                code: 1000,
                reason: "".into(),
            })))
            .await;
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_are_limited_per_user_and_released() {
        let uid = 987_654_321;
        let held: Vec<Slot> = (0..PER_USER).map(|_| Slot::take(uid).unwrap()).collect();
        assert!(Slot::take(uid).is_err());
        drop(held);
        assert!(Slot::take(uid).is_ok());
    }

    #[test]
    fn control_messages_parse() {
        assert!(matches!(
            serde_json::from_str::<Control>(r#"{"type":"resize","cols":120,"rows":30}"#),
            Ok(Control::Resize {
                cols: 120,
                rows: 30
            })
        ));
        assert!(serde_json::from_str::<Control>("ls -la").is_err());
    }

    #[test]
    fn target_labels() {
        assert_eq!(Target::Root.label(), "root");
        let s = Target::Site {
            app_id: 1,
            slug: "blog".into(),
            name: "Blog".into(),
        };
        assert_eq!(s.label(), "site:blog");
        let j = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Target>(&j).unwrap(), s);
    }
}
