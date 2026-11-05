//! 웹 터미널 통합 테스트 (실제 MariaDB + Redis, PTY 는 sudo helper 대신 /bin/sh 로).
//!
//!   RPANEL_TEST_DATABASE_URL=... RPANEL_TEST_REDIS_URL=... cargo test --test terminal
//!
//! 환경변수가 없으면 건너뜁니다. ⚠ 테스트 DB의 데이터를 지웁니다.

use std::{net::SocketAddr, time::Duration};

use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
    Router,
};
use futures::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use rpanel::{auth::totp, config::Config, routes, state::AppState};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;
use tower::ServiceExt;

struct Ctx {
    app: Router,
}

impl Ctx {
    async fn req(
        &self,
        method: Method,
        uri: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut b = Request::builder()
            .method(method)
            .uri(uri)
            .header("x-requested-with", "rpanel");
        if let Some(t) = token {
            b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        let req = match body {
            Some(v) => b
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(v.to_string()))
                .unwrap(),
            None => b.body(Body::empty()).unwrap(),
        };
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }
    async fn get(&self, uri: &str, t: &str) -> (StatusCode, Value) {
        self.req(Method::GET, uri, Some(t), None).await
    }
    async fn post(&self, uri: &str, t: &str, body: Value) -> (StatusCode, Value) {
        self.req(Method::POST, uri, Some(t), Some(body)).await
    }
}

async fn serve(state: AppState) -> SocketAddr {
    let app = routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    addr
}

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// 화면 출력(바이너리)을 모아 needle 이 나올 때까지. 끝 메시지(텍스트)도 돌려줌
async fn read_until(ws: &mut Ws, needle: &str) -> (String, Option<Value>) {
    let mut out = String::new();
    let mut ctrl = None;
    let _ = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(Ok(m)) = ws.next().await {
            match m {
                Message::Binary(b) => out.push_str(&String::from_utf8_lossy(&b)),
                Message::Text(t) => {
                    ctrl = serde_json::from_str(&t).ok();
                    break;
                }
                Message::Close(_) => break,
                _ => {}
            }
            if !needle.is_empty() && out.contains(needle) {
                break;
            }
        }
    })
    .await;
    (out, ctrl)
}

async fn fresh_state(cmd: Vec<&str>) -> Option<AppState> {
    let (Ok(db_url), Ok(redis_url)) = (
        std::env::var("RPANEL_TEST_DATABASE_URL"),
        std::env::var("RPANEL_TEST_REDIS_URL"),
    ) else {
        eprintln!(
            "RPANEL_TEST_DATABASE_URL / RPANEL_TEST_REDIS_URL 미설정 — 터미널 통합 테스트 건너뜀"
        );
        return None;
    };
    let mut cfg = Config::for_tests(db_url, redis_url);
    cfg.terminal_cmd = Some(cmd.into_iter().map(String::from).collect());
    Some(rpanel::build_state(cfg).await.expect("state"))
}

#[tokio::test]
async fn web_terminal() {
    // 테스트용 "셸": TERM · 화면 크기를 보여 주고, 한 줄 받아 되돌려 준 뒤 끝남
    let script = "echo \"term=$TERM\"; stty size; read line; echo \"got:$line\"; exit 7";
    let Some(state) = fresh_state(vec!["/bin/sh", "-c", script]).await else {
        return;
    };
    for t in [
        "mongo_users",
        "mongo_databases",
        "notifications",
        "panel_settings",
        "runtimes",
        "redis_users",
        "db_users",
        "backup_jobs",
        "site_accounts",
        "app_env",
        "cron_jobs",
        "backups",
        "app_databases",
        "deployments",
        "apps",
        "projects",
        "audit_logs",
        "users",
    ] {
        sqlx::query(&format!("DELETE FROM {t}"))
            .execute(&state.db)
            .await
            .unwrap();
    }
    rpanel::logstore::init(&state).await.expect("log store");
    let ctx = Ctx {
        app: routes::router(state.clone()),
    };
    let server = serve(state.clone()).await;

    let (s, b) = ctx
        .req(
            Method::POST,
            "/api/auth/setup",
            None,
            Some(json!({"email":"admin@example.com","password":"correct-horse-1"})),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{b:?}");
    let t = b["access_token"].as_str().unwrap().to_string();

    // 사이트 하나
    let (s, b) = ctx
        .post(
            "/api/sites",
            &t,
            json!({"type":"html","domains":"term-site.example.com"}),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{b:?}");
    let site_id = b["app"]["id"].as_u64().unwrap();

    // 상태: 사이트 목록, 서버 터미널 꺼짐
    let (s, b) = ctx.get("/api/terminal", &t).await;
    assert_eq!(s, StatusCode::OK, "{b:?}");
    assert_eq!(b["available"], true);
    assert_eq!(b["root"]["enabled"], false);
    assert_eq!(b["root"]["twofa"], false);
    let sites = b["sites"].as_array().unwrap();
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0]["user"], "term-site");
    assert_eq!(sites[0]["domain"], "term-site.example.com");

    // 서버 터미널은 꺼져 있으면 티켓도 안 나옴
    let (s, _) = ctx
        .post(
            "/api/terminal/ticket",
            &t,
            json!({"kind":"root","code":"000000"}),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    // 없는 사이트 / 잘못된 종류
    let (s, _) = ctx
        .post(
            "/api/terminal/ticket",
            &t,
            json!({"kind":"site","app_id": 999_999}),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = ctx
        .post("/api/terminal/ticket", &t, json!({"kind":"bash"}))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // 보기 전용 계정은 터미널 못 씀
    let (s, _) = ctx
        .post(
            "/api/users",
            &t,
            json!({"email":"viewer@example.com","password":"viewer-pass-123","role":"viewer"}),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED);
    let (_, b) = ctx
        .req(
            Method::POST,
            "/api/auth/login",
            None,
            Some(json!({"email":"viewer@example.com","password":"viewer-pass-123"})),
        )
        .await;
    let vt = b["access_token"].as_str().unwrap().to_string();
    assert_eq!(ctx.get("/api/terminal", &vt).await.0, StatusCode::FORBIDDEN);
    assert_eq!(
        ctx.post(
            "/api/terminal/ticket",
            &vt,
            json!({"kind":"site","app_id": site_id})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );

    // 사이트 터미널: 티켓 → WebSocket → PTY
    let (s, b) = ctx
        .post(
            "/api/terminal/ticket",
            &t,
            json!({"kind":"site","app_id": site_id}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{b:?}");
    let ticket = b["ticket"].as_str().unwrap().to_string();
    let (mut ws, _) = tokio_tungstenite::connect_async(format!(
        "ws://{server}/api/ws/terminal?ticket={ticket}&cols=100&rows=30"
    ))
    .await
    .expect("ws");
    let (out, _) = read_until(&mut ws, "30 100").await;
    assert!(out.contains("term=xterm-256color"), "{out:?}");
    assert!(out.contains("30 100"), "{out:?}");
    ws.send(Message::Binary(b"hello\r".to_vec().into()))
        .await
        .unwrap();
    let (out, _) = read_until(&mut ws, "got:hello").await;
    assert!(out.contains("got:hello"), "{out:?}");
    // 끝나면 종료 코드가 옴
    let (_, ctrl) = read_until(&mut ws, "").await;
    let ctrl = ctrl.expect("exit message");
    assert_eq!(ctrl["type"], "exit");
    assert_eq!(ctrl["code"], 7);

    // 티켓은 한 번만, 엉터리 티켓은 거부
    for bad in [ticket.as_str(), "deadbeef"] {
        assert!(
            tokio_tungstenite::connect_async(format!("ws://{server}/api/ws/terminal?ticket={bad}"))
                .await
                .is_err(),
            "ticket {bad} accepted twice"
        );
    }

    // 감사 기록: 열기 · 닫기
    let mut audit = Value::Null;
    for _ in 0..50 {
        audit = ctx.get("/api/audit", &t).await.1;
        if audit
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["action"] == "terminal.close")
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let acts: Vec<(String, String)> = audit
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["action"].as_str().unwrap_or("").to_string(),
                e["target"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect();
    assert!(
        acts.iter()
            .any(|(a, tg)| a == "terminal.open" && tg == "site:term-site-example-com"),
        "{acts:?}"
    );
    assert!(
        acts.iter()
            .any(|(a, tg)| a == "terminal.close" && tg.starts_with("site:term-site-example-com")),
        "{acts:?}"
    );

    // ── 서버 터미널 (켜진 것으로 가정한 두 번째 상태) ──
    let Some(state2) = fresh_state(vec![
        "/bin/sh",
        "-c",
        "id -u >/dev/null; echo ROOT-SHELL; read x",
        "root-on",
    ])
    .await
    else {
        return;
    };
    let ctx2 = Ctx {
        app: routes::router(state2.clone()),
    };
    let server2 = serve(state2.clone()).await;
    let (_, b) = ctx2.get("/api/terminal", &t).await;
    assert_eq!(b["root"]["enabled"], true);
    // 2단계 인증이 꺼진 계정은 안 됨
    let (s, b) = ctx2
        .post(
            "/api/terminal/ticket",
            &t,
            json!({"kind":"root","code":"123456"}),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{b:?}");
    assert!(b.to_string().contains("2단계 인증"), "{b:?}");

    // 2단계 인증 켜기
    let now = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    };
    let (_, b) = ctx2.post("/api/auth/2fa/setup", &t, json!({})).await;
    let key = totp::base32_decode(b["secret"].as_str().unwrap()).unwrap();
    let code_at = |step: u64| format!("{:06}", totp::hotp(&key, step, 6));
    let step = now() / totp::STEP;
    let (s, b) = ctx2
        .post("/api/auth/2fa/enable", &t, json!({"code": code_at(step)}))
        .await;
    assert_eq!(s, StatusCode::OK, "{b:?}");
    assert_eq!(ctx2.get("/api/terminal", &t).await.1["root"]["twofa"], true);

    // 틀린 코드 → 거부 + 기록
    let wrong = format!(
        "{:06}",
        (code_at(step + 1).parse::<u32>().unwrap() + 500_000) % 1_000_000
    );
    let (s, _) = ctx2
        .post(
            "/api/terminal/ticket",
            &t,
            json!({"kind":"root","code": wrong}),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // 맞는 새 코드 → 열림
    let (s, b) = ctx2
        .post(
            "/api/terminal/ticket",
            &t,
            json!({"kind":"root","code": code_at(step + 1)}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{b:?}");
    let ticket = b["ticket"].as_str().unwrap().to_string();
    let (mut ws, _) =
        tokio_tungstenite::connect_async(format!("ws://{server2}/api/ws/terminal?ticket={ticket}"))
            .await
            .expect("root ws");
    let (out, _) = read_until(&mut ws, "ROOT-SHELL").await;
    assert!(out.contains("ROOT-SHELL"), "{out:?}");
    // 브라우저가 닫으면 셸도 닫힘
    ws.close(None).await.unwrap();

    // 열 때 알림, 감사 기록
    let mut ok = false;
    for _ in 0..50 {
        let n = ctx2.get("/api/notifications", &t).await.1;
        let a = ctx2.get("/api/audit", &t).await.1;
        let notified = n["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["title"] == "서버 터미널(root)을 열었습니다: {email}");
        let audited = a
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["action"] == "terminal.root_denied")
            && a.as_array().unwrap().iter().any(|e| {
                e["action"] == "terminal.close"
                    && e["target"].as_str().unwrap_or("").starts_with("root")
            });
        if notified && audited {
            ok = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(ok, "root terminal notification / audit missing");
}
