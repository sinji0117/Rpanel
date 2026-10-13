//! 실제 MariaDB + Redis 를 붙여 돌리는 통합 테스트 (DRY-RUN 실행 모드).
//!
//!   RPANEL_TEST_DATABASE_URL=mysql://rpanel:pw@127.0.0.1:3306/rpanel_test \
//!   RPANEL_TEST_REDIS_URL=redis://:pw@127.0.0.1:6379/15 \
//!   cargo test --test api -- --nocapture
//!
//! 환경변수가 없으면 건너뜁니다. ⚠ 테스트 DB의 모든 데이터를 지웁니다.

use std::{net::SocketAddr, time::Duration};

use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
    Router,
};
use futures::StreamExt;
use http_body_util::BodyExt;
use rpanel::{config::Config, routes};
use serde_json::{json, Value};
use tower::ServiceExt;

struct Ctx {
    app: Router,
}

struct Resp {
    status: StatusCode,
    body: Value,
    set_cookie: Option<String>,
}

impl Ctx {
    async fn req(
        &self,
        method: Method,
        uri: &str,
        token: Option<&str>,
        cookie: Option<&str>,
        body: Option<Value>,
    ) -> Resp {
        let mut b = Request::builder()
            .method(method)
            .uri(uri)
            .header("x-requested-with", "rpanel");
        if let Some(t) = token {
            b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        if let Some(c) = cookie {
            b = b.header(header::COOKIE, format!("rpanel_rt={c}"));
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
        let set_cookie = res
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split(';').next())
            .and_then(|kv| kv.strip_prefix("rpanel_rt="))
            .map(str::to_string);
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Resp {
            status,
            body,
            set_cookie,
        }
    }
    async fn get(&self, uri: &str, t: &str) -> Resp {
        self.req(Method::GET, uri, Some(t), None, None).await
    }
    async fn post(&self, uri: &str, t: &str, body: Value) -> Resp {
        self.req(Method::POST, uri, Some(t), None, Some(body)).await
    }
}

async fn wait_deploy(ctx: &Ctx, token: &str, id: u64) -> Value {
    for _ in 0..200 {
        let r = ctx.get(&format!("/api/deployments/{id}"), token).await;
        let s = r.body["status"].as_str().unwrap_or("").to_string();
        if !matches!(s.as_str(), "queued" | "running") {
            return r.body;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("deployment {id} did not finish");
}

#[tokio::test]
async fn full_flow() {
    let (Ok(db_url), Ok(redis_url)) = (
        std::env::var("RPANEL_TEST_DATABASE_URL"),
        std::env::var("RPANEL_TEST_REDIS_URL"),
    ) else {
        eprintln!("RPANEL_TEST_DATABASE_URL / RPANEL_TEST_REDIS_URL 미설정 — 통합 테스트 건너뜀");
        return;
    };
    let cfg = Config::for_tests(db_url, redis_url);
    let state = rpanel::build_state(cfg).await.expect("state");
    for t in [
        "app_env",
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
    let ctx = Ctx {
        app: routes::router(state.clone()),
    };

    // ── health / setup ──
    let r = ctx.req(Method::GET, "/api/health", None, None, None).await;
    assert_eq!(r.body["status"], "ok");
    let r = ctx
        .req(Method::GET, "/api/auth/status", None, None, None)
        .await;
    assert_eq!(r.body["needs_setup"], true);

    let r = ctx
        .req(
            Method::POST,
            "/api/auth/setup",
            None,
            None,
            Some(json!({"email":"x","password":"short"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);

    let creds = json!({"email":"Admin@Example.com","password":"correct-horse-1"});
    let r = ctx
        .req(
            Method::POST,
            "/api/auth/setup",
            None,
            None,
            Some(creds.clone()),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["user"]["email"], "admin@example.com");
    let token1 = r.body["access_token"].as_str().unwrap().to_string();
    let rt1 = r.set_cookie.clone().expect("refresh cookie");

    // setup 락이 풀린 뒤에도 두 번째 setup은 거부
    sqlx::query("SELECT 1").execute(&state.db).await.unwrap();
    let mut rc = state.redis.clone();
    let _: () = redis::AsyncCommands::del(&mut rc, state.key(&["lock", "setup"]))
        .await
        .unwrap();
    let r = ctx
        .req(
            Method::POST,
            "/api/auth/setup",
            None,
            None,
            Some(creds.clone()),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    // ── me / 인증 실패 ──
    assert_eq!(
        ctx.get("/api/auth/me", &token1).await.status,
        StatusCode::OK
    );
    assert_eq!(
        ctx.get("/api/auth/me", "garbage").await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        ctx.req(Method::GET, "/api/projects", None, None, None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let r = ctx
        .req(
            Method::POST,
            "/api/auth/login",
            None,
            None,
            Some(json!({"email":"admin@example.com","password":"wrong-password"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);

    // ── 리프레시 회전 & 재사용 탐지 ──
    let no_csrf = Request::builder()
        .method(Method::POST)
        .uri("/api/auth/refresh")
        .header(header::COOKIE, format!("rpanel_rt={rt1}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        ctx.app.clone().oneshot(no_csrf).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );

    let r = ctx
        .req(Method::POST, "/api/auth/refresh", None, Some(&rt1), None)
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let token2 = r.body["access_token"].as_str().unwrap().to_string();
    let rt2 = r.set_cookie.clone().unwrap();
    assert_ne!(rt1, rt2);
    assert_eq!(
        ctx.get("/api/auth/me", &token2).await.status,
        StatusCode::OK
    );

    // 이미 쓴 rt1 재사용 → 세션 패밀리 전체 폐기
    let r = ctx
        .req(Method::POST, "/api/auth/refresh", None, Some(&rt1), None)
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        ctx.get("/api/auth/me", &token2).await.status,
        StatusCode::UNAUTHORIZED,
        "access token of revoked family"
    );
    assert_eq!(
        ctx.req(Method::POST, "/api/auth/refresh", None, Some(&rt2), None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );

    // 새 로그인
    let r = ctx
        .req(
            Method::POST,
            "/api/auth/login",
            None,
            None,
            Some(creds.clone()),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let t = r.body["access_token"].as_str().unwrap().to_string();
    let rt = r.set_cookie.clone().unwrap();

    // ── 프로젝트 ──
    let r = ctx
        .post(
            "/api/projects",
            &t,
            json!({"name":"Beripick","description":"메인 서비스"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let pid = r.body["id"].as_u64().unwrap();
    assert_eq!(
        ctx.post("/api/projects", &t, json!({"name":"Beripick"}))
            .await
            .status,
        StatusCode::CONFLICT
    );

    // ── 앱 ──
    let r = ctx
        .post(
            &format!("/api/projects/{pid}/apps"),
            &t,
            json!({"name":"Beripick API","kind":"rust","git_url":"ssh://git@100.100.199.123:2222/beri/backend.git",
                   "binary_name":"hello-api","domains":"api.beripick.com","health_path":"/api/health"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let api_id = r.body["id"].as_u64().unwrap();
    assert_eq!(r.body["slug"], "beripick-api");
    assert_eq!(r.body["port"], 8100);
    assert!(
        r.body.get("webhook_token").is_none(),
        "token must not be serialized"
    );

    let r = ctx
        .post(&format!("/api/projects/{pid}/apps"), &t, json!({"name":"Dup","kind":"static","git_url":"https://github.com/a/b.git","domains":"API.beripick.com"}))
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT, "domain conflict");

    let r = ctx
        .post(
            &format!("/api/projects/{pid}/apps"),
            &t,
            json!({"name":"Beripick Front","kind":"static","git_url":"https://github.com/a/front.git","domains":"beripick.com www.beripick.com","api_proxy_port":8100}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let front_id = r.body["id"].as_u64().unwrap();
    assert!(r.body["port"].is_null());

    let r = ctx
        .post(
            &format!("/api/projects/{pid}/apps"),
            &t,
            json!({"name":"bad","kind":"rust","git_url":"file:///etc/passwd"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);

    let r = ctx.get(&format!("/api/apps?project_id={pid}"), &t).await;
    assert_eq!(r.body.as_array().unwrap().len(), 2);

    // ── env ──
    let r = ctx
        .req(Method::PUT, &format!("/api/apps/{api_id}/env"), Some(&t), None, Some(json!({"vars":[{"key":"DATABASE_URL","value":"mysql://u:p@127.0.0.1/db"},{"key":"RUST_LOG","value":"info"}]})))
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["vars"].as_array().unwrap().len(), 2);
    let r = ctx
        .req(
            Method::PUT,
            &format!("/api/apps/{api_id}/env"),
            Some(&t),
            None,
            Some(json!({"vars":[{"key":"BAD-KEY","value":"x"}]})),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);

    // ── 배포 (dry-run) + WebSocket 로그 스트림 ──
    let server = {
        let app = routes::router(state.clone());
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
    };

    let r = ctx
        .post(&format!("/api/apps/{api_id}/deploy"), &t, json!({}))
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{:?}", r.body);
    let d1 = r.body["deployment_id"].as_u64().unwrap();
    assert_eq!(
        ctx.post(&format!("/api/apps/{api_id}/deploy"), &t, json!({}))
            .await
            .status,
        StatusCode::CONFLICT
    );

    let ticket = ctx.post("/api/auth/ws-ticket", &t, json!({})).await.body["ticket"]
        .as_str()
        .unwrap()
        .to_string();
    let (mut ws, _) = tokio_tungstenite::connect_async(format!(
        "ws://{server}/api/ws/deployments/{d1}?ticket={ticket}"
    ))
    .await
    .expect("ws connect");
    let mut lines = Vec::new();
    let mut final_status = None;
    while let Some(Ok(m)) = tokio::time::timeout(Duration::from_secs(20), ws.next())
        .await
        .expect("ws timeout")
    {
        if let tokio_tungstenite::tungstenite::Message::Text(txt) = m {
            let v: Value = serde_json::from_str(&txt).unwrap();
            match v["type"].as_str() {
                Some("line") => lines.push((
                    v["seq"].as_u64().unwrap(),
                    v["text"].as_str().unwrap().to_string(),
                )),
                Some("status") => {
                    final_status = Some(v["status"].as_str().unwrap().to_string());
                    break;
                }
                _ => {}
            }
        }
    }
    assert_eq!(
        final_status.as_deref(),
        Some("success"),
        "lines: {lines:#?}"
    );
    let seqs: Vec<u64> = lines.iter().map(|(s, _)| *s).collect();
    let mut sorted = seqs.clone();
    sorted.dedup();
    assert_eq!(seqs, sorted, "no duplicate lines over websocket");
    assert!(lines
        .iter()
        .any(|(_, l)| l.contains("git clone --depth 1 --branch main --")));
    assert!(lines
        .iter()
        .any(|(_, l)| l.contains("install-unit beripick-api")));
    assert!(lines
        .iter()
        .any(|(_, l)| l.contains("install-site beripick-api")));

    // 티켓은 1회용
    assert!(tokio_tungstenite::connect_async(format!(
        "ws://{server}/api/ws/deployments/{d1}?ticket={ticket}"
    ))
    .await
    .is_err());

    let d = wait_deploy(&ctx, &t, d1).await;
    assert_eq!(d["status"], "success");
    assert!(d["log"].as_str().unwrap().contains("[dry-run]"));
    let app = ctx.get(&format!("/api/apps/{api_id}"), &t).await.body;
    assert_eq!(app["status"], "running");
    assert_eq!(app["current_release"], d1);

    // 두 번째 배포 → 첫 번째로 롤백
    let d2 = ctx
        .post(&format!("/api/apps/{api_id}/deploy"), &t, json!({}))
        .await
        .body["deployment_id"]
        .as_u64()
        .unwrap();
    assert_eq!(wait_deploy(&ctx, &t, d2).await["status"], "success");
    let r = ctx
        .post(&format!("/api/deployments/{d1}/rollback"), &t, json!({}))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        ctx.get(&format!("/api/apps/{api_id}"), &t).await.body["current_release"],
        d1
    );
    assert_eq!(
        ctx.get(&format!("/api/apps/{api_id}/deployments"), &t)
            .await
            .body
            .as_array()
            .unwrap()
            .len(),
        2
    );

    // ── 취소 ──
    let d3 = ctx
        .post(&format!("/api/apps/{front_id}/deploy"), &t, json!({}))
        .await
        .body["deployment_id"]
        .as_u64()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        ctx.post(&format!("/api/deployments/{d3}/cancel"), &t, json!({}))
            .await
            .status,
        StatusCode::OK
    );
    let d = wait_deploy(&ctx, &t, d3).await;
    assert!(
        matches!(d["status"].as_str(), Some("cancelled") | Some("success")),
        "{d:?}"
    );

    // ── 웹훅 ──
    let hook = ctx
        .get(&format!("/api/apps/{front_id}/webhook"), &t)
        .await
        .body;
    let token = hook["token"].as_str().unwrap();
    let r = ctx
        .req(
            Method::POST,
            "/api/hooks/beripick-front",
            None,
            None,
            Some(json!({"ref":"refs/heads/main"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let hook_req = |tok: &str, body: Value| {
        Request::builder()
            .method(Method::POST)
            .uri("/api/hooks/beripick-front")
            .header("x-gitlab-token", tok)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    let res = ctx
        .app
        .clone()
        .oneshot(hook_req(token, json!({"ref":"refs/heads/dev"})))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK, "other branch is skipped");
    let res = ctx
        .app
        .clone()
        .oneshot(hook_req(token, json!({"ref":"refs/heads/main"})))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let hid = serde_json::from_slice::<Value>(&bytes).unwrap()["deployment_id"]
        .as_u64()
        .unwrap();
    assert_eq!(wait_deploy(&ctx, &t, hid).await["status"], "success");

    // ── 서비스 / 저널 / 키 ──
    assert_eq!(
        ctx.get(&format!("/api/apps/{api_id}/service"), &t)
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        ctx.post(
            &format!("/api/apps/{api_id}/service"),
            &t,
            json!({"action":"restart"})
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        ctx.post(
            &format!("/api/apps/{api_id}/service"),
            &t,
            json!({"action":"rm -rf"})
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        ctx.post(
            &format!("/api/apps/{front_id}/service"),
            &t,
            json!({"action":"restart"})
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    let r = ctx
        .post(&format!("/api/apps/{api_id}/deploy-key"), &t, json!({}))
        .await;
    assert!(r.body["public_key"]
        .as_str()
        .unwrap()
        .starts_with("ssh-ed25519"));

    // ── 개요 / 감사 / 지표 WS ──
    let r = ctx.get("/api/system/overview", &t).await;
    assert_eq!(r.body["projects"], 1);
    assert_eq!(r.body["apps"]["total"], 2);
    assert!(r.body["recent_deployments"].as_array().unwrap().len() >= 3);
    let r = ctx.get("/api/audit", &t).await;
    assert!(r
        .body
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["action"] == "app.deploy"));
    assert!(
        r.body
            .as_array()
            .unwrap()
            .iter()
            .all(|e| !e["target"].as_str().unwrap().contains("mysql://")),
        "no env values in audit"
    );

    let ticket = ctx.post("/api/auth/ws-ticket", &t, json!({})).await.body["ticket"]
        .as_str()
        .unwrap()
        .to_string();
    let (mut ws, _) =
        tokio_tungstenite::connect_async(format!("ws://{server}/api/ws/metrics?ticket={ticket}"))
            .await
            .unwrap();
    let msg = tokio::time::timeout(Duration::from_secs(10), ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let v: Value = serde_json::from_str(msg.to_text().unwrap()).unwrap();
    assert_eq!(v["type"], "metrics");
    assert!(v["data"]["mem_total"].as_u64().unwrap() > 0);
    let _ = ws.close(None).await;
    assert!(tokio_tungstenite::connect_async(format!(
        "ws://{server}/api/ws/metrics?ticket=deadbeef"
    ))
    .await
    .is_err());

    // ── 삭제 ──
    assert_eq!(
        ctx.req(
            Method::DELETE,
            &format!("/api/projects/{pid}"),
            Some(&t),
            None,
            None
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        ctx.req(
            Method::DELETE,
            &format!("/api/apps/{api_id}"),
            Some(&t),
            None,
            None
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        ctx.req(
            Method::DELETE,
            &format!("/api/apps/{front_id}"),
            Some(&t),
            None,
            None
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        ctx.req(
            Method::DELETE,
            &format!("/api/projects/{pid}"),
            Some(&t),
            None,
            None
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );

    // ── 비밀번호 변경 → 모든 세션 폐기 ──
    let r = ctx
        .post(
            "/api/auth/password",
            &t,
            json!({"current_password":"correct-horse-1","new_password":"new-password-22"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(
        ctx.get("/api/auth/me", &t).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        ctx.req(Method::POST, "/api/auth/refresh", None, Some(&rt), None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let r = ctx
        .req(
            Method::POST,
            "/api/auth/login",
            None,
            None,
            Some(json!({"email":"admin@example.com","password":"new-password-22"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let t = r.body["access_token"].as_str().unwrap().to_string();
    let rt = r.set_cookie.unwrap();

    // ── 로그아웃 ──
    assert_eq!(
        ctx.req(Method::POST, "/api/auth/logout", None, Some(&rt), None)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        ctx.get("/api/auth/me", &t).await.status,
        StatusCode::UNAUTHORIZED
    );

    // ── 로그인 rate limit ──
    let mut last = StatusCode::OK;
    for _ in 0..12 {
        last = ctx
            .req(
                Method::POST,
                "/api/auth/login",
                None,
                None,
                Some(json!({"email":"nobody@example.com","password":"wrong-password"})),
            )
            .await
            .status;
    }
    assert_eq!(last, StatusCode::TOO_MANY_REQUESTS);

    // 정리
    let mut r = state.redis.clone();
    let keys: Vec<String> =
        redis::AsyncCommands::keys(&mut r, format!("{}:*", state.cfg.redis_prefix))
            .await
            .unwrap();
    for k in keys {
        let _: () = redis::AsyncCommands::del(&mut r, k).await.unwrap();
    }
}
