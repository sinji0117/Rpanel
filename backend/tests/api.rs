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

/// 조건에 맞는 백업이 모두 끝날 때까지 기다린 뒤 목록 반환
async fn wait_backups(ctx: &Ctx, token: &str, query: &str) -> Vec<Value> {
    for _ in 0..100 {
        let list = ctx.get(&format!("/api/backups?{query}"), token).await.body;
        let rows = list.as_array().cloned().unwrap_or_default();
        if !rows.is_empty() && rows.iter().all(|b| b["status"] != "running") {
            return rows;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("backups ({query}) did not finish");
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
    let mut cfg = Config::for_tests(db_url, redis_url);
    // MongoDB 가 있으면 패널 로그를 MongoDB 에 저장하는 경로로 전체 흐름을 검사
    cfg.mongodb_url = std::env::var("RPANEL_TEST_MONGODB_URL")
        .ok()
        .filter(|v| !v.is_empty());
    let mongo_users = std::env::var("RPANEL_TEST_MONGO_USERS").is_ok_and(|v| v == "1");
    let state = rpanel::build_state(cfg).await.expect("state");
    if let Some(m) = &state.mongo {
        m.database(rpanel::logstore::LOG_DB)
            .drop()
            .await
            .expect("drop log db");
        for name in ["shop_mongo"] {
            let _ = m
                .database(name)
                .run_command(mongodb::bson::doc! { "dropAllUsersFromDatabase": 1 })
                .await;
            let _ = m.database(name).drop().await;
        }
    }
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
    rpanel::logstore::init(&state)
        .await
        .expect("log store init");
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
            json!({"name":"Example","description":"메인 서비스"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let pid = r.body["id"].as_u64().unwrap();
    assert_eq!(
        ctx.post("/api/projects", &t, json!({"name":"Example"}))
            .await
            .status,
        StatusCode::CONFLICT
    );

    // ── 앱 ──
    let r = ctx
        .post(
            &format!("/api/projects/{pid}/apps"),
            &t,
            json!({"name":"Example API","kind":"rust","git_url":"ssh://git@192.0.2.10:2222/team/backend.git",
                   "binary_name":"hello-api","domains":"api.example.com","health_path":"/api/health"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let api_id = r.body["id"].as_u64().unwrap();
    assert_eq!(r.body["slug"], "example-api");
    assert_eq!(r.body["port"], 8100);
    assert!(
        r.body.get("webhook_token").is_none(),
        "token must not be serialized"
    );

    let r = ctx
        .post(&format!("/api/projects/{pid}/apps"), &t, json!({"name":"Dup","kind":"static","git_url":"https://github.com/a/b.git","domains":"API.example.com"}))
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT, "domain conflict");

    // 패널 전용 도메인
    let pd = "/api/system/panel-domain";
    for (body, code) in [
        (
            json!({"domain":"panel","email":"a@b.co"}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"domain":"panel.example.com","email":"x"}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"domain":"api.example.com","email":"a@b.co"}),
            StatusCode::CONFLICT,
        ),
    ] {
        let r = ctx
            .req(Method::PUT, pd, Some(&t), None, Some(body.clone()))
            .await;
        assert_eq!(r.status, code, "{body} {:?}", r.body);
    }
    let r = ctx
        .req(
            Method::PUT,
            pd,
            Some(&t),
            None,
            Some(json!({"domain":"Panel.Example.com","email":"a@b.co"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        ctx.get(pd, &t).await.body["url"],
        "https://panel.example.com"
    );
    // 같은 도메인으로 다시 설정(인증서 갱신)은 허용, 앱 도메인으로는 거부
    let r = ctx
        .req(
            Method::PUT,
            pd,
            Some(&t),
            None,
            Some(json!({"domain":"panel.example.com","email":"a@b.co"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = ctx.post(&format!("/api/projects/{pid}/apps"), &t, json!({"name":"PD","kind":"static","git_url":"https://github.com/a/b.git","domains":"panel.example.com"})).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{:?}", r.body);
    assert_eq!(
        ctx.req(Method::DELETE, pd, Some(&t), None, None)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert!(ctx.get(pd, &t).await.body["domain"].is_null());

    let r = ctx
        .post(
            &format!("/api/projects/{pid}/apps"),
            &t,
            json!({"name":"Example Front","kind":"static","git_url":"https://github.com/a/front.git","domains":"example.com www.example.com","api_proxy_port":8100}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let front_id = r.body["id"].as_u64().unwrap();
    assert!(r.body["port"].is_null());
    assert_eq!(r.body["app_role"], "front", "static default role");
    let r = ctx
        .post(
            &format!("/api/projects/{pid}/apps"),
            &t,
            json!({"name":"Back Office","kind":"static","app_role":"admin","git_url":"https://github.com/a/admin.git","api_proxy_port":8100}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["app_role"], "admin");
    let admin_id = r.body["id"].as_u64().unwrap();
    let r = ctx
        .req(Method::PUT, &format!("/api/apps/{admin_id}"), Some(&t), None, Some(json!({"name":"Back Office","git_url":"https://github.com/a/admin.git","app_role":"front"})))
        .await;
    assert_eq!(r.body["app_role"], "front", "{:?}", r.body);
    assert!(ctx
        .req(
            Method::DELETE,
            &format!("/api/apps/{admin_id}"),
            Some(&t),
            None,
            None
        )
        .await
        .status
        .is_success());

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
        .any(|(_, l)| l.contains("install-unit example-api")));
    assert!(lines
        .iter()
        .any(|(_, l)| l.contains("install-site example-api")));

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
            "/api/hooks/example-front",
            None,
            None,
            Some(json!({"ref":"refs/heads/main"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let hook_req = |tok: &str, body: Value| {
        Request::builder()
            .method(Method::POST)
            .uri("/api/hooks/example-front")
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

    // GitHub 웹훅: X-Hub-Signature-256 (Secret = 웹훅 토큰), ping, 잘못된 서명
    let gh_req = |sig: String, event: &str, body: &str| {
        Request::builder()
            .method(Method::POST)
            .uri("/api/hooks/example-front")
            .header("x-hub-signature-256", sig)
            .header("x-github-event", event)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    let sign = |body: &str| {
        format!(
            "sha256={}",
            hex::encode(rpanel::routes::deployments::hmac_sha256(
                token.as_bytes(),
                body.as_bytes()
            ))
        )
    };
    let ping = r#"{"zen":"Keep it logically awesome."}"#;
    let res = ctx
        .app
        .clone()
        .oneshot(gh_req(sign(ping), "ping", ping))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK, "ping");
    let push = r#"{"ref":"refs/heads/main","after":"abc"}"#;
    let res = ctx
        .app
        .clone()
        .oneshot(gh_req("sha256=00".into(), "push", push))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "bad signature");
    let res = ctx
        .app
        .clone()
        .oneshot(gh_req(sign(push), "push", &push.replace("main", "dev")))
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        StatusCode::UNAUTHORIZED,
        "signature over different body"
    );
    let res = ctx
        .app
        .clone()
        .oneshot(gh_req(sign(push), "push", push))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::ACCEPTED, "signed github push");
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let gid = serde_json::from_slice::<Value>(&bytes).unwrap()["deployment_id"]
        .as_u64()
        .unwrap();
    assert_eq!(wait_deploy(&ctx, &t, gid).await["status"], "success");

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

    // ── DB · Redis ──
    let r = ctx
        .post(&format!("/api/apps/{api_id}/databases"), &t, json!({}))
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["name"], "example_api");
    let db_id = r.body["id"].as_u64().unwrap();
    assert_eq!(
        ctx.post(&format!("/api/apps/{api_id}/databases"), &t, json!({}))
            .await
            .status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        ctx.post(
            &format!("/api/apps/{api_id}/databases"),
            &t,
            json!({"name":"mysql"})
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        ctx.post(
            &format!("/api/apps/{api_id}/databases"),
            &t,
            json!({"name":"x`; DROP"})
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    let r = ctx
        .post(&format!("/api/apps/{api_id}/redis"), &t, json!({}))
        .await;
    assert_eq!(r.body["db"], 4, "{:?}", r.body);
    let r = ctx
        .post(&format!("/api/apps/{front_id}/redis"), &t, json!({}))
        .await;
    assert_eq!(r.body["db"], 5);
    // DB 웹 관리 (Redis): 앱에 배정한 DB 만
    let r = ctx.get("/api/dbweb/redis", &t).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        r.body["databases"].as_array().unwrap().len(),
        2,
        "{:?}",
        r.body
    );
    for k in ["web:a", "web:b"] {
        let r = ctx
            .post(
                "/api/dbweb/redis/4/set",
                &t,
                json!({"key": k, "value": "값", "ttl": 600}),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    }
    let r = ctx.get("/api/dbweb/redis/4/keys?pattern=web:*", &t).await;
    assert_eq!(r.body["keys"].as_array().unwrap().len(), 2, "{:?}", r.body);
    assert_eq!(r.body["keys"][0]["type"], "string");
    assert!(r.body["keys"][0]["ttl"].as_i64().unwrap() > 0);
    let r = ctx.get("/api/dbweb/redis/4/key?k=web:a", &t).await;
    assert_eq!(r.body["value"], "값", "{:?}", r.body);
    let r = ctx
        .post("/api/dbweb/redis/4/delete", &t, json!({"key":"web:a"}))
        .await;
    assert_eq!(r.body["deleted"], 1);
    ctx.post("/api/dbweb/redis/4/delete", &t, json!({"key":"web:b"}))
        .await;
    assert_eq!(
        ctx.get("/api/dbweb/redis/4/key?k=web:a", &t).await.status,
        StatusCode::NOT_FOUND
    );
    for db in [15, 9] {
        assert_eq!(
            ctx.get(&format!("/api/dbweb/redis/{db}/keys"), &t)
                .await
                .status,
            StatusCode::FORBIDDEN,
            "db {db}"
        );
    }
    let env = ctx.get(&format!("/api/apps/{api_id}/env"), &t).await.body;
    let managed = env["managed"].as_array().unwrap();
    let keys: Vec<&str> = managed.iter().map(|m| m["key"].as_str().unwrap()).collect();
    assert_eq!(keys, ["DATABASE_URL", "REDIS_URL"]);
    // 사용자가 직접 넣은 DATABASE_URL 이 우선 (배포 시)
    assert!(managed[1]["value"].as_str().unwrap().ends_with("/4"));
    let r = ctx.get("/api/databases", &t).await;
    assert_eq!(r.body["databases"].as_array().unwrap().len(), 1);
    assert_eq!(r.body["redis"].as_array().unwrap().len(), 2);
    assert!(
        r.body["databases"][0].get("password").is_none(),
        "password must not be serialized"
    );
    assert_eq!(
        ctx.post(
            &format!("/api/apps/{api_id}/databases/{db_id}/password"),
            &t,
            json!({})
        )
        .await
        .status,
        StatusCode::OK
    );
    let d = ctx
        .post(&format!("/api/apps/{api_id}/deploy"), &t, json!({}))
        .await
        .body["deployment_id"]
        .as_u64()
        .unwrap();
    let d = wait_deploy(&ctx, &t, d).await;
    assert!(
        d["log"].as_str().unwrap().contains("(3개 변수)"),
        "managed REDIS_URL + user 2 vars (DATABASE_URL overridden)"
    );
    assert_eq!(
        ctx.req(
            Method::DELETE,
            &format!("/api/apps/{front_id}/redis"),
            Some(&t),
            None,
            Some(json!({"flush":true}))
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );

    // ── 독립 데이터베이스 · 추가 계정 ──
    let r = ctx
        .post("/api/databases", &t, json!({"name":"analytics"}))
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert!(r.body["app_id"].is_null());
    assert!(r.body["url"]
        .as_str()
        .unwrap()
        .contains("@127.0.0.1:3306/analytics"));
    let an_id = r.body["id"].as_u64().unwrap();
    assert_eq!(
        ctx.post("/api/databases", &t, json!({})).await.status,
        StatusCode::BAD_REQUEST,
        "name required without app"
    );
    let r = ctx
        .post(
            &format!("/api/databases/{an_id}/users"),
            &t,
            json!({"username":"analytics_ro","privileges":"read"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert!(r.body["password"].as_str().unwrap().len() >= 32);
    let ro_id = r.body["id"].as_u64().unwrap();
    for (body, code) in [
        (json!({"username":"analytics_ro"}), StatusCode::CONFLICT),
        (json!({"username":"example_api"}), StatusCode::CONFLICT),
        (
            json!({"username":"x2","privileges":"admin"}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"username":"x3","password":"has space 123"}),
            StatusCode::BAD_REQUEST,
        ),
        (json!({"username":"root"}), StatusCode::BAD_REQUEST),
    ] {
        assert_eq!(
            ctx.post(&format!("/api/databases/{an_id}/users"), &t, body.clone())
                .await
                .status,
            code,
            "{body}"
        );
    }
    let r = ctx
        .post(
            &format!("/api/databases/{an_id}/users/{ro_id}/password"),
            &t,
            json!({"password":"Custom_pass_123"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(r.body["url"]
        .as_str()
        .unwrap()
        .contains("analytics_ro:Custom_pass_123@"));
    let r = ctx.get("/api/databases", &t).await;
    let an = r.body["databases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["name"] == "analytics")
        .unwrap()
        .clone();
    assert!(an["app_id"].is_null());
    assert_eq!(an["users"].as_array().unwrap().len(), 2);
    assert_eq!(an["users"][1]["privileges"], "read");
    // 앱에 연결 → 그 앱 환경변수로 주입, 같은 키 중복은 거부
    assert_eq!(
        ctx.req(
            Method::PUT,
            &format!("/api/databases/{an_id}"),
            Some(&t),
            None,
            Some(json!({"app_id": api_id, "env_key": "DATABASE_URL"}))
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        ctx.req(
            Method::PUT,
            &format!("/api/databases/{an_id}"),
            Some(&t),
            None,
            Some(json!({"app_id": api_id, "env_key": "ANALYTICS_URL"}))
        )
        .await
        .status,
        StatusCode::OK
    );
    let env = ctx.get(&format!("/api/apps/{api_id}/env"), &t).await.body;
    assert!(env["managed"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["key"] == "ANALYTICS_URL"));
    assert_eq!(
        ctx.req(
            Method::PUT,
            &format!("/api/databases/{an_id}"),
            Some(&t),
            None,
            Some(json!({"app_id": null}))
        )
        .await
        .status,
        StatusCode::OK
    );
    let r = ctx
        .post(&format!("/api/databases/{an_id}/backups"), &t, json!({}))
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{:?}", r.body);
    let rows = wait_backups(&ctx, &t, &format!("database_id={an_id}")).await;
    assert_eq!(rows.len(), 1);
    assert!(rows[0]["app_id"].is_null());
    assert_eq!(rows[0]["status"], "success");

    // ── Redis 계정 ──
    let r = ctx
        .post(
            "/api/redis-users",
            &t,
            json!({"username":"cache_ro","key_pattern":"cache:*","readonly":true,"db_index":3}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert!(r.body["url"]
        .as_str()
        .unwrap()
        .starts_with("redis://cache_ro:"));
    assert!(r.body["url"].as_str().unwrap().ends_with("/3"));
    let ru_id = r.body["id"].as_u64().unwrap();
    for (body, code) in [
        (json!({"username":"cache_ro"}), StatusCode::CONFLICT),
        (json!({"username":"default"}), StatusCode::BAD_REQUEST),
        (
            json!({"username":"ok_user","key_pattern":"a b"}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"username":"ok_user","db_index":99}),
            StatusCode::BAD_REQUEST,
        ),
    ] {
        assert_eq!(
            ctx.post("/api/redis-users", &t, body.clone()).await.status,
            code,
            "{body}"
        );
    }
    let r = ctx
        .req(
            Method::PUT,
            &format!("/api/redis-users/{ru_id}"),
            Some(&t),
            None,
            Some(json!({"key_pattern":"cache2:*","readonly":false,"db_index":3})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = ctx
        .post(&format!("/api/redis-users/{ru_id}/password"), &t, json!({}))
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let r = ctx.get("/api/databases", &t).await;
    let ru = &r.body["redis_users"][0];
    assert_eq!(ru["key_pattern"], "cache2:*");
    assert_eq!(ru["readonly"], false);
    assert!(
        ru.get("password").is_none(),
        "password must not be serialized"
    );

    // ── 사이트 만들기 ──
    let r = ctx
        .post(
            "/api/sites",
            &t,
            json!({"type":"html","domains":"Static-Site.example.com www.static-site.example.com"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["site_user"], "static-site");
    assert_eq!(r.body["app"]["source"], "files");
    assert_eq!(r.body["app"]["kind"], "static");
    assert_eq!(
        r.body["app"]["site_root"],
        "/home/static-site/htdocs/static-site.example.com"
    );
    assert!(r.body["generated_password"].as_str().unwrap().len() >= 16);
    let html_id = r.body["app"]["id"].as_u64().unwrap();
    let d = wait_deploy(&ctx, &t, r.body["deployment_id"].as_u64().unwrap()).await;
    assert_eq!(d["status"], "success", "{d:?}");
    let log = d["log"].as_str().unwrap();
    assert!(
        log.contains("install-site static-site-example-com"),
        "{log}"
    );
    assert!(log.contains("✔ 적용 완료"));
    assert!(d["commit_sha"].is_null());

    // 업로드형 Rust 사이트 (실행 파일을 올려 사이트 유저로 실행)
    let r = ctx
        .post(
            "/api/sites",
            &t,
            json!({"type":"rust","domains":"shop.example.com","password":"site-password-1"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert!(
        r.body["generated_password"].is_null(),
        "given password is not echoed"
    );
    assert_eq!(r.body["app"]["kind"], "rust");
    assert_eq!(r.body["app"]["source"], "files");
    assert_eq!(r.body["app"]["start_cmd"], "./app");
    assert!(r.body["app"]["port"].as_u64().is_some());
    let rust_id = r.body["app"]["id"].as_u64().unwrap();
    let d = wait_deploy(&ctx, &t, r.body["deployment_id"].as_u64().unwrap()).await;
    assert_eq!(d["status"], "success", "{d:?}");
    let log = d["log"].as_str().unwrap();
    assert!(log.contains("chmod u+x -- ./app"), "{log}");
    assert!(log.contains("install-unit shop-example-com"), "{log}");
    assert!(log.contains("install-site shop-example-com"), "{log}");

    // PHP 는 지원하지 않음
    assert_eq!(
        ctx.post(
            "/api/sites",
            &t,
            json!({"type":"php","domains":"x.example.com"})
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        ctx.post(
            "/api/sites",
            &t,
            json!({"type":"proxy","domains":"p.example.com"})
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        ctx.post(
            "/api/sites",
            &t,
            json!({"type":"html","domains":"shop.example.com"})
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        ctx.post(
            "/api/sites",
            &t,
            json!({"type":"html","domains":"y.example.com","site_user":"root"})
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        ctx.post(
            "/api/sites",
            &t,
            json!({"type":"shell","domains":"y.example.com"})
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );

    let r = ctx.post("/api/sites", &t, json!({"type":"proxy","domains":"grafana.example.com","proxy_target":"http://10.0.0.20:3000"})).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert!(r.body["app"]["site_user"].is_null());
    let proxy_id = r.body["app"]["id"].as_u64().unwrap();
    // 지원하지 않는 종류(PHP 등)는 만들 수 없음
    let r = ctx
        .post(
            "/api/sites",
            &t,
            json!({"type":"php","domains":"php.example.com"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    // ── 사이트 계정 (SSH · SFTP) ──
    let r = ctx.get(&format!("/api/apps/{html_id}/accounts"), &t).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["accounts"].as_array().unwrap().len(), 1);
    assert_eq!(r.body["accounts"][0]["username"], "static-site");
    assert_eq!(r.body["accounts"][0]["is_primary"], true);
    let primary_id = r.body["accounts"][0]["id"].as_u64().unwrap();
    let r = ctx
        .post(
            &format!("/api/apps/{html_id}/accounts"),
            &t,
            json!({"username":"shop-dev","access":"ssh","note":"외주 개발자"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["password"].as_str().unwrap().len(), 20, "generated");
    let dev_id = r.body["account"]["id"].as_u64().unwrap();
    let key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGqci ci@build";
    let r = ctx
        .post(
            &format!("/api/apps/{html_id}/accounts"),
            &t,
            json!({"username":"shop-ci","ssh_keys": key}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert!(r.body["password"].is_null(), "key only");
    assert_eq!(r.body["account"]["access"], "sftp", "default sftp");
    let ci_id = r.body["account"]["id"].as_u64().unwrap();
    for (body, code) in [
        (json!({"username":"static-site"}), StatusCode::CONFLICT),
        (json!({"username":"shop-dev"}), StatusCode::CONFLICT),
        (json!({"username":"root"}), StatusCode::BAD_REQUEST),
        (
            json!({"username":"x2-y","access":"ftp"}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"username":"x3-y","ssh_keys":"command=\"x\" ssh-ed25519 AAAA"}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"username":"x4-y","password":"has:colon-1234"}),
            StatusCode::BAD_REQUEST,
        ),
    ] {
        assert_eq!(
            ctx.post(&format!("/api/apps/{html_id}/accounts"), &t, body.clone())
                .await
                .status,
            code,
            "{body}"
        );
    }
    let r = ctx
        .req(
            Method::PUT,
            &format!("/api/apps/{html_id}/accounts/{dev_id}"),
            Some(&t),
            None,
            Some(json!({"enabled": false, "access": "sftp", "ssh_keys": key, "note": "잠시 중지"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["enabled"], false);
    assert_eq!(r.body["access"], "sftp");
    let r = ctx
        .post(
            &format!("/api/apps/{html_id}/accounts/{primary_id}/password"),
            &t,
            json!({"password":"Primary_pass_1"}),
        )
        .await;
    assert_eq!(r.body["password"], "Primary_pass_1");
    let r = ctx.get(&format!("/api/apps/{html_id}/accounts"), &t).await;
    assert_eq!(r.body["accounts"].as_array().unwrap().len(), 3);
    assert_eq!(r.body["accounts"][1]["key_count"], 1);
    assert_eq!(
        ctx.req(
            Method::DELETE,
            &format!("/api/apps/{html_id}/accounts/{primary_id}"),
            Some(&t),
            None,
            None
        )
        .await
        .status,
        StatusCode::BAD_REQUEST,
        "primary cannot be deleted"
    );
    for aid in [dev_id, ci_id] {
        assert_eq!(
            ctx.req(
                Method::DELETE,
                &format!("/api/apps/{html_id}/accounts/{aid}"),
                Some(&t),
                None,
                None
            )
            .await
            .status,
            StatusCode::NO_CONTENT
        );
    }
    assert_eq!(
        ctx.get(&format!("/api/apps/{proxy_id}/accounts"), &t)
            .await
            .status,
        StatusCode::BAD_REQUEST,
        "proxy site has no accounts"
    );

    // Python 사이트: 기본 시작 명령 python app.py, 포트 배정, 바로 적용
    let r = ctx
        .post(
            "/api/sites",
            &t,
            json!({"type":"python","domains":"py.example.com"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["app"]["kind"], "python");
    assert_eq!(r.body["app"]["start_cmd"], "python app.py");
    assert!(r.body["app"]["port"].as_u64().is_some());
    let py_id = r.body["app"]["id"].as_u64().unwrap();
    let d = wait_deploy(&ctx, &t, r.body["deployment_id"].as_u64().unwrap()).await;
    assert_eq!(d["status"], "success", "{}", d["log"]);
    assert!(d["log"].as_str().unwrap().contains("Python 가상환경"));
    assert!(
        d["log"]
            .as_str()
            .unwrap()
            .contains("런타임: Python 서버 기본"),
        "{}",
        d["log"]
    );

    // Node.js · Java 사이트: 기본 시작 명령, 포트 배정, 바로 적용
    let mut site_ids = vec![];
    for (ty, start, step) in [
        ("node", "node server.js", "Node.js 패키지"),
        ("java", "java $JAVA_OPTS App.java", "서비스"),
    ] {
        let r = ctx
            .post(
                "/api/sites",
                &t,
                json!({"type": ty, "domains": format!("{ty}.example.com")}),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        assert_eq!(r.body["app"]["kind"], ty);
        assert_eq!(r.body["app"]["start_cmd"], start);
        assert!(r.body["app"]["port"].as_u64().is_some());
        site_ids.push(r.body["app"]["id"].as_u64().unwrap());
        let d = wait_deploy(&ctx, &t, r.body["deployment_id"].as_u64().unwrap()).await;
        assert_eq!(d["status"], "success", "{}", d["log"]);
        assert!(d["log"].as_str().unwrap().contains(step), "{}", d["log"]);
    }
    let java_site = site_ids[1];

    // Git Node.js · Java 앱: 기본 시작 명령, npm / Maven·Gradle 빌드
    for (kind, start, build) in [
        ("node", "npm start", "npm run build --if-present"),
        ("java", "java $JAVA_OPTS -jar app.jar", "mvnw"),
    ] {
        let r = ctx
            .post(
                &format!("/api/projects/{pid}/apps"),
                &t,
                json!({"name": format!("Git {kind}"), "kind": kind, "git_url": "https://github.com/a/b.git",
                       "domains": format!("git-{kind}.example.com"), "frontend_dir": "web"}),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        assert_eq!(r.body["start_cmd"], start);
        assert_eq!(r.body["frontend_dir"], "web");
        assert!(r.body["port"].as_u64().is_some());
        let id = r.body["id"].as_u64().unwrap();
        let r = ctx
            .post(&format!("/api/apps/{id}/deploy"), &t, json!({}))
            .await;
        assert_eq!(r.status, StatusCode::ACCEPTED, "{:?}", r.body);
        let d = wait_deploy(&ctx, &t, r.body["deployment_id"].as_u64().unwrap()).await;
        assert_eq!(d["status"], "success", "{}", d["log"]);
        let log = d["log"].as_str().unwrap();
        assert!(log.contains(build), "{log}");
        assert!(log.contains("프런트엔드 빌드"), "{log}");
        let r = ctx
            .req(
                Method::DELETE,
                &format!("/api/apps/{id}"),
                Some(&t),
                None,
                None,
            )
            .await;
        assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    }
    // Java jar 경로는 .jar 로 끝나는 상대 경로만
    for bad in ["../x.jar", "target/app.war", "/abs/app.jar"] {
        let r = ctx
            .post(
                &format!("/api/projects/{pid}/apps"),
                &t,
                json!({"name":"Bad jar","kind":"java","git_url":"https://github.com/a/b.git","binary_name": bad}),
            )
            .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{bad}");
    }

    // ── 런타임 버전 ──
    let r = ctx.get("/api/runtimes", &t).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let rows = r.body["runtimes"].as_array().unwrap().clone();
    assert_eq!(rows.len(), 6);
    let default_of = |rows: &[Value], lang: &str| -> String {
        rows.iter()
            .find(|x| x["lang"] == lang && x["is_default"] == true)
            .map(|x| x["version"].as_str().unwrap().to_string())
            .unwrap()
    };
    assert_eq!(default_of(&rows, "rust"), "stable");
    assert_eq!(default_of(&rows, "python"), "system");
    assert_eq!(default_of(&rows, "node"), "system");
    assert_eq!(default_of(&rows, "java"), "system");
    assert_eq!(default_of(&rows, "maven"), "system");
    assert_eq!(default_of(&rows, "gradle"), "system");
    assert!(r.body["candidates"]["java"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "21"));
    assert!(r.body["candidates"]["rust"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "stable"));
    assert!(r.body["candidates"]["python"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "3.12"));
    assert_eq!(r.body["usage"]["python:system"], 1);
    for body in [
        json!({"lang":"ruby","version":"3.3"}),
        json!({"lang":"python","version":"2.7"}),
        json!({"lang":"node","version":"22; rm -rf /"}),
        json!({"lang":"rust","version":"1.85.0.1"}),
        json!({"lang":"node","version":"16"}),
        json!({"lang":"java","version":"9"}),
        json!({"lang":"java","version":"21.0"}),
        json!({"lang":"maven","version":"3.9.11"}),
        json!({"lang":"gradle","version":"6"}),
    ] {
        assert_eq!(
            ctx.post("/api/runtimes", &t, body.clone()).await.status,
            StatusCode::BAD_REQUEST,
            "{body}"
        );
    }
    let r = ctx
        .post(
            "/api/runtimes",
            &t,
            json!({"lang":"python","version":"3.12","make_default":true}),
        )
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{:?}", r.body);
    assert_eq!(r.body["status"], "installing");
    let py312 = r.body["id"].as_u64().unwrap();
    let mut rows = vec![];
    for _ in 0..50 {
        rows = ctx.get("/api/runtimes", &t).await.body["runtimes"]
            .as_array()
            .unwrap()
            .clone();
        if rows
            .iter()
            .any(|x| x["id"] == py312 && x["status"] != "installing")
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let row = rows.iter().find(|x| x["id"] == py312).unwrap();
    assert_eq!(row["status"], "ready", "{}", row["log"]);
    assert!(row["log"]
        .as_str()
        .unwrap()
        .contains("runtime-install python 3.12"));
    assert_eq!(default_of(&rows, "python"), "3.12");
    // 이미 설치된 버전 → 바로 200
    let r = ctx
        .post(
            "/api/runtimes",
            &t,
            json!({"lang":"python","version":"3.12"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    // Java 21 설치 → Java 사이트에 고르고 적용
    let r = ctx
        .post("/api/runtimes", &t, json!({"lang":"java","version":"21"}))
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{:?}", r.body);
    let j21 = r.body["id"].as_u64().unwrap();
    for _ in 0..50 {
        let rows = ctx.get("/api/runtimes", &t).await.body["runtimes"].clone();
        if rows
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["id"] == j21 && x["status"] == "ready")
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let r = ctx
        .req(
            Method::PUT,
            &format!("/api/apps/{java_site}/runtime"),
            Some(&t),
            None,
            Some(json!({ "version": "21" })),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = ctx
        .req(
            Method::DELETE,
            &format!("/api/runtimes/{j21}"),
            Some(&t),
            None,
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT, "java 21 in use");

    // Rust 툴체인
    let r = ctx
        .post("/api/runtimes", &t, json!({"lang":"rust","version":"1.85"}))
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{:?}", r.body);
    let rust185 = r.body["id"].as_u64().unwrap();

    // 앱별 버전: 설치 안 된 버전 거부, 업로드형 Rust 사이트는 고를 수 없음
    let put_rt = |id: u64, v: Value| {
        let ctx = &ctx;
        let t = t.clone();
        async move {
            ctx.req(
                Method::PUT,
                &format!("/api/apps/{id}/runtime"),
                Some(&t),
                None,
                Some(json!({ "version": v })),
            )
            .await
        }
    };
    assert_eq!(
        put_rt(py_id, json!("3.11")).await.status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        put_rt(py_id, json!("3.12; id")).await.status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        put_rt(rust_id, json!("stable")).await.status,
        StatusCode::BAD_REQUEST
    );
    let r = put_rt(py_id, json!("3.12")).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["runtime_version"], "3.12");
    let r = ctx
        .post(&format!("/api/apps/{py_id}/deploy"), &t, json!({}))
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{:?}", r.body);
    let d = wait_deploy(&ctx, &t, r.body["deployment_id"].as_u64().unwrap()).await;
    assert_eq!(d["status"], "success", "{}", d["log"]);
    assert!(
        d["log"]
            .as_str()
            .unwrap()
            .contains("/opt/rpanel/runtimes/bin/python3.12 -m venv"),
        "{}",
        d["log"]
    );

    // 지우기: 서버 기본·기본 버전·쓰는 중인 버전은 거부
    let sys_py = rows
        .iter()
        .find(|x| x["lang"] == "python" && x["version"] == "system")
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    let del_rt = |id: u64| {
        let ctx = &ctx;
        let t = t.clone();
        async move {
            ctx.req(
                Method::DELETE,
                &format!("/api/runtimes/{id}"),
                Some(&t),
                None,
                None,
            )
            .await
            .status
        }
    };
    assert_eq!(del_rt(sys_py).await, StatusCode::BAD_REQUEST);
    assert_eq!(del_rt(py312).await, StatusCode::BAD_REQUEST, "default");
    let r = ctx
        .req(
            Method::PUT,
            &format!("/api/runtimes/{sys_py}/default"),
            Some(&t),
            None,
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["is_default"], true);
    assert_eq!(del_rt(py312).await, StatusCode::CONFLICT, "used by app");
    assert_eq!(
        put_rt(py_id, Value::Null).await.body["runtime_version"],
        Value::Null
    );
    assert_eq!(del_rt(py312).await, StatusCode::NO_CONTENT);
    for _ in 0..50 {
        let rows = ctx.get("/api/runtimes", &t).await.body["runtimes"]
            .as_array()
            .unwrap()
            .clone();
        if rows
            .iter()
            .any(|x| x["id"] == rust185 && x["status"] == "ready")
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(del_rt(rust185).await, StatusCode::NO_CONTENT);
    // 서버 기본 6개 + Java 사이트가 쓰는 Java 21
    assert_eq!(
        ctx.get("/api/runtimes", &t).await.body["runtimes"]
            .as_array()
            .unwrap()
            .len(),
        7
    );

    // 지표 기록 (그래프)
    let now = chrono::Utc::now().timestamp();
    for i in 0..40 {
        let sample = rpanel::metrics::Sample {
            t: now - (39 - i) * 60,
            c: i as f32,
            m: 1 << 30,
            mt: 4 << 30,
            l: [0.5, 0.4, 0.3],
            d: vec![("/".into(), 10 << 30, 100 << 30)],
        };
        rpanel::routes::system::push_sample(&state, &sample)
            .await
            .unwrap();
    }
    let r = ctx.get("/api/system/metrics?range=30m", &t).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let pts = r.body["points"].as_array().unwrap();
    assert!((29..=31).contains(&pts.len()), "{}", pts.len());
    assert!(pts
        .windows(2)
        .all(|w| w[0]["t"].as_i64() < w[1]["t"].as_i64()));
    assert_eq!(pts.last().unwrap()["c"], 39.0);
    assert!(r.body["cpu_count"].as_u64().unwrap() > 0);
    assert_eq!(
        ctx.get("/api/system/metrics?range=1y", &t).await.status,
        StatusCode::BAD_REQUEST
    );
    let r = ctx.get("/api/system/info", &t).await;
    assert!(r.body["boot_time"].as_i64().unwrap() > 0);
    // 로그 메뉴
    let r = ctx.get("/api/logs/system?kind=panel&lines=50", &t).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(r.body["lines"].is_array());
    assert_eq!(
        ctx.get("/api/logs/system?kind=shadow", &t).await.status,
        StatusCode::BAD_REQUEST
    );
    // 대시보드 시스템 이벤트·프로세스
    let r = ctx.get("/api/system/events?limit=20", &t).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(r.body["events"].is_array());
    assert!(r.body["summary"]["total"].is_u64());
    let r = ctx.get("/api/system/processes", &t).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(!r.body["processes"].as_array().unwrap().is_empty());
    assert!(r.body["processes"][0]["pid"].is_u64());
    let r = ctx.get("/api/logs/deployments?limit=5", &t).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(!r.body.as_array().unwrap().is_empty());

    // ── 알림 ──
    let r = ctx.get("/api/notifications/settings", &t).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["settings"]["events"]["deploy_failed"], true);
    assert_eq!(r.body["settings"]["events"]["deploy_success"], false);
    assert_eq!(r.body["settings"]["disk_threshold"], 90);
    assert!(r.body["events"].as_array().unwrap().len() >= 8);
    let mut settings = r.body["settings"].clone();
    for (field, bad) in [
        ("discord_url", "https://evil.example/hook"),
        ("slack_url", "http://hooks.slack.com/x"),
        ("webhook_url", "file:///etc/passwd"),
        ("telegram_token", "123:short"),
    ] {
        let mut s = settings.clone();
        s["channels"][field] = json!(bad);
        let r = ctx
            .req(
                Method::PUT,
                "/api/notifications/settings",
                Some(&t),
                None,
                Some(s),
            )
            .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{field}={bad}");
    }
    // 로컬 웹훅 수신기로 실제 전송 확인
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let hook_port = listener.local_addr().unwrap().port();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            let tx = tx.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 65536];
                let mut got = Vec::new();
                loop {
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    got.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&got);
                    if let Some(i) = text.find("\r\n\r\n") {
                        let len = text[..i]
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        if got.len() >= i + 4 + len {
                            break;
                        }
                    }
                }
                let _ = sock
                    .write_all(b"HTTP/1.1 204 No Content\r\ncontent-length: 0\r\n\r\n")
                    .await;
                let _ = tx.send(String::from_utf8_lossy(&got).into_owned());
            });
        }
    });
    settings["events"]["deploy_success"] = json!(true);
    settings["disk_threshold"] = json!(85);
    settings["channels"]["webhook_url"] = json!(format!("  http://127.0.0.1:{hook_port}/hook  "));
    let r = ctx
        .req(
            Method::PUT,
            "/api/notifications/settings",
            Some(&t),
            None,
            Some(settings.clone()),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        r.body["settings"]["channels"]["webhook_url"],
        format!("http://127.0.0.1:{hook_port}/hook")
    );
    assert_eq!(r.body["settings"]["disk_threshold"], 85);
    let r = ctx.post("/api/notifications/test", &t, json!({})).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["results"][0]["channel"], "webhook");
    assert_eq!(r.body["results"][0]["ok"], true, "{:?}", r.body);
    let got = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(got.starts_with("POST /hook"), "{got}");
    assert!(got.contains("\"kind\":\"test\""), "{got}");
    // 켠 이벤트(배포 성공)는 목록에 남고 웹훅으로도 감
    let r = ctx
        .post(&format!("/api/apps/{py_id}/deploy"), &t, json!({}))
        .await;
    let did = r.body["deployment_id"].as_u64().unwrap();
    wait_deploy(&ctx, &t, did).await;
    let got = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(got.contains("deploy_success"), "{got}");
    let r = ctx.get("/api/notifications?limit=10", &t).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let items = r.body["items"].as_array().unwrap();
    let dep = items
        .iter()
        .find(|n| n["kind"] == "deploy_success")
        .expect("deploy notification");
    assert_eq!(dep["title"], "배포 성공: {app} #{id}");
    assert_eq!(dep["params"]["id"], did);
    assert_eq!(dep["link"], format!("/deployments/{did}"));
    assert!(r.body["unread"].as_i64().unwrap() >= 2);
    let r = ctx
        .post("/api/notifications/read", &t, json!({ "ids": [dep["id"]] }))
        .await;
    let left = r.body["unread"].as_i64().unwrap();
    let r = ctx.get("/api/notifications?unread=true", &t).await;
    assert_eq!(r.body["items"].as_array().unwrap().len() as i64, left);
    let r = ctx
        .post("/api/notifications/read", &t, json!({ "all": true }))
        .await;
    assert_eq!(r.body["unread"], 0);
    // 끈 이벤트는 기록하지 않음
    settings["events"]["deploy_success"] = json!(false);
    settings["channels"]["webhook_url"] = json!("");
    ctx.req(
        Method::PUT,
        "/api/notifications/settings",
        Some(&t),
        None,
        Some(settings),
    )
    .await;
    let before = ctx.get("/api/notifications?limit=500", &t).await.body["items"]
        .as_array()
        .unwrap()
        .len();
    let r = ctx
        .post(&format!("/api/apps/{py_id}/deploy"), &t, json!({}))
        .await;
    wait_deploy(&ctx, &t, r.body["deployment_id"].as_u64().unwrap()).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        ctx.get("/api/notifications?limit=500", &t).await.body["items"]
            .as_array()
            .unwrap()
            .len(),
        before
    );
    let r = ctx
        .req(Method::DELETE, "/api/notifications", Some(&t), None, None)
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(
        ctx.get("/api/notifications", &t).await.body["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    // ── Rust 앱 + 프런트엔드(npm) 한 저장소 ──
    let r = ctx
        .post(
            &format!("/api/projects/{pid}/apps"),
            &t,
            json!({"name":"Full Stack","kind":"rust","git_url":"git@github.com:me/myapp.git","binary_name":"myapp",
                   "frontend_dir":"../web","domains":"full.example.com"}),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::BAD_REQUEST,
        "frontend_dir must stay inside the repo"
    );
    let r = ctx
        .post(
            &format!("/api/projects/{pid}/apps"),
            &t,
            json!({"name":"Full Stack","kind":"rust","git_url":"git@github.com:me/myapp.git","binary_name":"myapp",
                   "frontend_dir":"/frontend/","output_dir":"dist","domains":"full.example.com"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["frontend_dir"], "frontend");
    let full_id = r.body["id"].as_u64().unwrap();
    let r = ctx
        .post(&format!("/api/apps/{full_id}/deploy"), &t, json!({}))
        .await;
    let d = wait_deploy(&ctx, &t, r.body["deployment_id"].as_u64().unwrap()).await;
    assert_eq!(d["status"], "success", "{}", d["log"]);
    let log = d["log"].as_str().unwrap();
    let pos = |needle: &str| {
        log.find(needle)
            .unwrap_or_else(|| panic!("{needle} not in log:\n{log}"))
    };
    assert!(
        pos("프런트엔드 빌드 (npm · frontend)") < pos("cargo build --release"),
        "frontend first"
    );
    pos("npm ci --no-audit --no-fund");
    pos("npm run build");
    pos("/frontend/dist/");
    // static 앱에는 frontend_dir 를 저장하지 않음
    let r = ctx
        .req(Method::PUT, &format!("/api/apps/{front_id}"), Some(&t), None, Some(json!({"name":"Example Front","git_url":"https://github.com/a/front.git","domains":"example.com www.example.com","api_proxy_port":8100,"frontend_dir":"web"})))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(r.body["frontend_dir"].is_null());
    ctx.req(
        Method::DELETE,
        &format!("/api/apps/{full_id}"),
        Some(&t),
        None,
        None,
    )
    .await;

    // ── 로그 저장소 / MongoDB ──
    let r = ctx.get("/api/system/info", &t).await;
    assert_eq!(
        r.body["log_store"],
        if state.mongo.is_some() {
            "mongodb"
        } else {
            "mariadb"
        }
    );
    let r = ctx.get("/api/mongo", &t).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["configured"], state.mongo.is_some());
    if let Some(m) = &state.mongo {
        assert_eq!(r.body["available"], true, "{:?}", r.body);
        // 로그가 MongoDB 에 실제로 들어갔는지 (활동 기록·배포 로그·알림·지표)
        let ldb = m.database(rpanel::logstore::LOG_DB);
        for coll in ["audit", "deployment_logs", "notifications"] {
            let n = ldb
                .collection::<mongodb::bson::Document>(coll)
                .count_documents(mongodb::bson::doc! {})
                .await
                .unwrap();
            assert!(n > 0 || coll == "notifications", "{coll} empty");
        }
        let audit = ctx.get("/api/audit?limit=5", &t).await;
        assert!(audit.body.as_array().unwrap().len() == 5);
        assert!(audit.body[0]["id"].as_u64().unwrap() > audit.body[1]["id"].as_u64().unwrap());
    } else {
        let r = ctx
            .post("/api/mongo", &t, json!({"name":"shop_mongo"}))
            .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "no mongo → 400");
    }
    for bad in ["admin", "rpanel_logs", "Shop", "a-b", "x"] {
        if state.mongo.is_some() {
            let r = ctx.post("/api/mongo", &t, json!({"name": bad})).await;
            assert_eq!(r.status, StatusCode::BAD_REQUEST, "{bad}");
        }
    }
    if mongo_users {
        let r = ctx
            .post(
                "/api/mongo",
                &t,
                json!({"name":"shop_mongo","app_id": py_id}),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        let mid = r.body["database"]["id"].as_u64().unwrap();
        let url = r.body["url"].as_str().unwrap().to_string();
        assert!(
            url.starts_with("mongodb://shop_mongo:")
                && url.ends_with("/shop_mongo?authSource=shop_mongo"),
            "{url}"
        );
        assert!(r.body["database"].get("password").is_none());
        assert_eq!(
            ctx.post("/api/mongo", &t, json!({"name":"shop_mongo"}))
                .await
                .status,
            StatusCode::CONFLICT
        );
        // 앱 환경변수에 자동 주입
        let env = ctx.get(&format!("/api/apps/{py_id}/env"), &t).await;
        assert!(env.body["managed"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["key"] == "MONGODB_URL" && m["value"] == url));
        // DB 웹 관리: 컬렉션 만들기 → 문서 추가·찾기·수정·삭제 (그 DB 계정으로)
        let w = format!("/api/dbweb/mongo/{mid}");
        let r = ctx
            .post(&format!("{w}/collections"), &t, json!({"name":"webdocs"}))
            .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        let r = ctx
            .post(
                &format!("{w}/c/webdocs/insert"),
                &t,
                json!({"doc":{"item":"사과","qty":3}}),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        let oid = r.body["inserted_id"].clone();
        assert!(oid["$oid"].is_string(), "{oid}");
        ctx.post(
            &format!("{w}/c/webdocs/insert"),
            &t,
            json!({"doc":{"item":"배","qty":1}}),
        )
        .await;
        let r = ctx.get(&w, &t).await;
        assert_eq!(r.body["collections"][0]["name"], "webdocs", "{:?}", r.body);
        let r = ctx
            .get(&format!("{w}/c/webdocs?filter=%7B%22qty%22%3A3%7D"), &t)
            .await;
        assert_eq!(r.body["total"], 1, "{:?}", r.body);
        assert_eq!(r.body["docs"][0]["item"], "사과");
        let r = ctx
            .post(
                &format!("{w}/c/webdocs/update"),
                &t,
                json!({"id": oid, "doc":{"item":"청사과","qty":5}}),
            )
            .await;
        assert_eq!(r.body["modified"], 1, "{:?}", r.body);
        let r = ctx
            .post(&format!("{w}/c/webdocs/delete"), &t, json!({"id": oid}))
            .await;
        assert_eq!(r.body["deleted"], 1, "{:?}", r.body);
        assert_eq!(
            ctx.get(&format!("{w}/c/webdocs?filter=nope"), &t)
                .await
                .status,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            ctx.get(&format!("{w}/c/system.users"), &t).await.status,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            ctx.get(&format!("{w}/c/webdocs"), &t).await.body["total"],
            1
        );

        // 실제로 그 계정으로 쓰기·읽기 (TEST 서버 주소로 바꿔서)
        let host = std::env::var("RPANEL_TEST_MONGODB_URL").unwrap();
        let host = host
            .split('@')
            .nth(1)
            .unwrap()
            .split('/')
            .next()
            .unwrap()
            .to_string();
        let as_user = |u: &str| u.replace("127.0.0.1:27017", &host);
        let c = mongodb::Client::with_uri_str(as_user(&url)).await.unwrap();
        c.database("shop_mongo")
            .collection::<mongodb::bson::Document>("orders")
            .insert_one(mongodb::bson::doc! { "n": 1 })
            .await
            .expect("app user can write");
        // 읽기 전용 계정
        let r = ctx
            .post(
                &format!("/api/mongo/{mid}/users"),
                &t,
                json!({"username":"shop_reader","role":"read"}),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        let uid = r.body["id"].as_u64().unwrap();
        let ro = mongodb::Client::with_uri_str(as_user(r.body["url"].as_str().unwrap()))
            .await
            .unwrap();
        let coll = ro
            .database("shop_mongo")
            .collection::<mongodb::bson::Document>("orders");
        assert_eq!(
            coll.count_documents(mongodb::bson::doc! {})
                .await
                .expect("reader can read"),
            1
        );
        assert!(
            coll.insert_one(mongodb::bson::doc! { "n": 2 })
                .await
                .is_err(),
            "reader must not write"
        );
        assert_eq!(
            ctx.post(
                &format!("/api/mongo/{mid}/users"),
                &t,
                json!({"username":"shop_reader"})
            )
            .await
            .status,
            StatusCode::CONFLICT
        );
        assert_eq!(
            ctx.post(
                &format!("/api/mongo/{mid}/users"),
                &t,
                json!({"username":"x2","role":"root"})
            )
            .await
            .status,
            StatusCode::BAD_REQUEST
        );
        // 비밀번호 바꾸면 옛 주소로는 안 됨
        let r = ctx
            .post(
                &format!("/api/mongo/{mid}/users/{uid}/password"),
                &t,
                json!({}),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK);
        let old =
            mongodb::Client::with_uri_str(as_user(&url.replace("shop_mongo:", "shop_reader:")))
                .await
                .unwrap();
        assert!(old
            .database("shop_mongo")
            .collection::<mongodb::bson::Document>("orders")
            .count_documents(mongodb::bson::doc! {})
            .await
            .is_err());
        let r = ctx.get("/api/mongo", &t).await;
        let d = &r.body["databases"][0];
        assert_eq!(d["users"].as_array().unwrap().len(), 1);
        assert_eq!(d["app_id"], py_id);
        // 연결 해제 · 삭제
        assert_eq!(
            ctx.req(
                Method::PUT,
                &format!("/api/mongo/{mid}"),
                Some(&t),
                None,
                Some(json!({"app_id": null}))
            )
            .await
            .status,
            StatusCode::OK
        );
        assert_eq!(
            ctx.req(
                Method::DELETE,
                &format!("/api/mongo/{mid}/users/{uid}"),
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
                &format!("/api/mongo/{mid}"),
                Some(&t),
                None,
                None
            )
            .await
            .status,
            StatusCode::NO_CONTENT
        );
        assert!(
            c.database("shop_mongo")
                .collection::<mongodb::bson::Document>("orders")
                .count_documents(mongodb::bson::doc! {})
                .await
                .is_err(),
            "user dropped"
        );
        assert_eq!(
            ctx.get("/api/mongo", &t).await.body["databases"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }

    // 버전 확인 (테스트 설정은 GitHub 확인 끔)
    let r = ctx.get("/api/system/version", &t).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["current"], env!("CARGO_PKG_VERSION"));
    assert_eq!(r.body["latest"], Value::Null);
    assert_eq!(r.body["update_available"], false);

    // 사이트 설정 변경 → 바로 적용
    let r = ctx
        .req(Method::PUT, &format!("/api/apps/{rust_id}"), Some(&t), None, Some(json!({"name":"Shop","domains":"shop.example.com www.shop.example.com","start_cmd":"./shop-server --port $PORT"})))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["start_cmd"], "./shop-server --port $PORT");
    assert_eq!(
        ctx.req(
            Method::PUT,
            &format!("/api/apps/{rust_id}"),
            Some(&t),
            None,
            Some(json!({"name":"Shop","domains":"shop.example.com"}))
        )
        .await
        .status,
        StatusCode::BAD_REQUEST,
        "start_cmd is required for rust sites"
    );
    let r = ctx
        .req(
            Method::PUT,
            &format!("/api/apps/{html_id}"),
            Some(&t),
            None,
            Some(json!({"name":"Static","domains":"static-site.example.com","web_root":"public"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["web_root"], "public");
    let r = ctx
        .req(
            Method::PUT,
            &format!("/api/apps/{html_id}"),
            Some(&t),
            None,
            Some(json!({"name":"Static","domains":"static-site.example.com","web_root":"../etc"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    for id in [rust_id, html_id] {
        for _ in 0..50 {
            if ctx.get(&format!("/api/apps/{id}"), &t).await.body["status"] != "deploying" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    // 사이트 유저 · SSL · 파일 (dry-run 에서는 파일 관리자 비활성)
    assert_eq!(
        ctx.req(
            Method::PUT,
            &format!("/api/apps/{html_id}/site-user/password"),
            Some(&t),
            None,
            Some(json!({"password":"short"}))
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        ctx.req(
            Method::PUT,
            &format!("/api/apps/{html_id}/site-user/password"),
            Some(&t),
            None,
            Some(json!({"password":"another-password-9"}))
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        ctx.req(
            Method::PUT,
            &format!("/api/apps/{proxy_id}/site-user/password"),
            Some(&t),
            None,
            Some(json!({"password":"another-password-9"}))
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    let r = ctx.get(&format!("/api/apps/{html_id}/ssl"), &t).await;
    assert_eq!(r.body["mode"], "none");
    assert_eq!(r.body["edge_mode"], "direct");
    assert_eq!(
        ctx.req(
            Method::PUT,
            &format!("/api/apps/{html_id}/ssl/force"),
            Some(&t),
            None,
            Some(json!({"force_https":true}))
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    let r = ctx
        .post(
            &format!("/api/apps/{html_id}/ssl/letsencrypt"),
            &t,
            json!({"email":"admin@example.com"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        ctx.get(&format!("/api/apps/{html_id}"), &t).await.body["ssl_mode"],
        "letsencrypt"
    );
    // 화면 보안 헤더 (CSP)
    let r = ctx.get(&format!("/api/apps/{html_id}"), &t).await;
    assert_eq!(r.body["csp_mode"], "off", "default off");
    for bad in [
        json!({"csp_mode":"report"}),
        json!({"csp_mode":"maybe","csp_policy":"default-src 'self'"}),
        json!({"csp_mode":"enforce","csp_policy":"default-src 'self'\"; include /etc/passwd; #"}),
        json!({"csp_mode":"enforce","csp_policy":"default-src $host"}),
    ] {
        let r = ctx
            .req(
                Method::PUT,
                &format!("/api/apps/{html_id}/security"),
                Some(&t),
                None,
                Some(bad.clone()),
            )
            .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{bad} {:?}", r.body);
    }
    let r = ctx
        .req(
            Method::PUT,
            &format!("/api/apps/{html_id}/security"),
            Some(&t),
            None,
            Some(json!({"csp_mode":"report","csp_policy":"default-src 'self';\n  frame-ancestors 'none'"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["csp_mode"], "report");
    assert_eq!(
        r.body["csp_policy"],
        "default-src 'self'; frame-ancestors 'none'"
    );
    // 끄더라도 정책은 남겨 둠
    let r = ctx
        .req(
            Method::PUT,
            &format!("/api/apps/{html_id}/security"),
            Some(&t),
            None,
            Some(
                json!({"csp_mode":"off","csp_policy":"default-src 'self'; frame-ancestors 'none'"}),
            ),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["csp_mode"], "off");
    assert_eq!(
        r.body["csp_policy"],
        "default-src 'self'; frame-ancestors 'none'"
    );
    assert_eq!(
        ctx.post(
            &format!("/api/apps/{html_id}/ssl/custom"),
            &t,
            json!({"certificate":"x","private_key":"y"})
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        ctx.req(
            Method::DELETE,
            &format!("/api/apps/{html_id}/ssl"),
            Some(&t),
            None,
            None
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        ctx.get(&format!("/api/apps/{html_id}/files?path=htdocs"), &t)
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        ctx.get(&format!("/api/apps/{proxy_id}/files"), &t)
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        ctx.get("/api/system/php", &t).await.status,
        StatusCode::NOT_FOUND
    );
    // 파일 사이트 백업은 htdocs
    let r = ctx
        .post(&format!("/api/apps/{html_id}/backups"), &t, json!({}))
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{:?}", r.body);
    for _ in 0..50 {
        let l = ctx
            .get(&format!("/api/backups?app_id={html_id}"), &t)
            .await
            .body;
        if l.as_array()
            .unwrap()
            .iter()
            .all(|b| b["status"] != "running")
        {
            assert_eq!(l[0]["kind"], "files");
            assert!(l[0]["file"].as_str().unwrap().ends_with("-files.tar.gz"));
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // 파일 복원 뒤에는 바로 적용 (패키지 다시 설치 · 서비스 재시작)
    let l = ctx
        .get(&format!("/api/backups?app_id={html_id}"), &t)
        .await
        .body;
    assert_eq!(l[0]["status"], "success", "{l}");
    let r = ctx
        .post(
            &format!("/api/backups/{}/restore", l[0]["id"]),
            &t,
            json!({}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let d = wait_deploy(&ctx, &t, r.body["deployment_id"].as_u64().unwrap()).await;
    assert_eq!(d["status"], "success", "{}", d["log"]);
    // 파일 사이트 크론 허용
    let r = ctx
        .post(
            &format!("/api/apps/{rust_id}/cron"),
            &t,
            json!({"name":"cleanup","schedule":"*:0/5","command":"./app --cleanup"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    for id in [html_id, rust_id, proxy_id, py_id] {
        let r = ctx
            .req(
                Method::DELETE,
                &format!("/api/apps/{id}"),
                Some(&t),
                None,
                None,
            )
            .await;
        assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    }

    // ── 사이트 ──
    let r = ctx.get("/api/sites", &t).await;
    let domains: Vec<&str> = r
        .body
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["domain"].as_str().unwrap())
        .collect();
    assert!(
        domains.contains(&"api.example.com") && domains.contains(&"www.example.com"),
        "{domains:?}"
    );
    assert_eq!(
        ctx.get(&format!("/api/apps/{api_id}/nginx-logs?kind=access"), &t)
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        ctx.get(&format!("/api/apps/{api_id}/nginx-logs?kind=../x"), &t)
            .await
            .status,
        StatusCode::BAD_REQUEST
    );

    // ── 백업 ──
    let r = ctx
        .post(&format!("/api/apps/{api_id}/backups"), &t, json!({}))
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{:?}", r.body);
    assert_eq!(
        r.body["backup_ids"].as_array().unwrap().len(),
        2,
        "db + data"
    );
    let mut done = false;
    for _ in 0..100 {
        let list = ctx
            .get(&format!("/api/backups?app_id={api_id}"), &t)
            .await
            .body;
        if list
            .as_array()
            .unwrap()
            .iter()
            .all(|b| b["status"] == "success")
        {
            done = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(done, "backups did not finish");
    let list = ctx
        .get(&format!("/api/backups?app_id={api_id}"), &t)
        .await
        .body;
    let db_backup = list
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["kind"] == "db")
        .unwrap()
        .clone();
    assert!(db_backup["file"]
        .as_str()
        .unwrap()
        .ends_with("-db-example_api.sql.gz"));
    let bid = db_backup["id"].as_u64().unwrap();
    assert_eq!(
        ctx.post(&format!("/api/backups/{bid}/restore"), &t, json!({}))
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        ctx.get(&format!("/api/backups/{bid}/download"), &t)
            .await
            .status,
        StatusCode::NOT_FOUND,
        "dry-run has no file"
    );
    assert_eq!(
        ctx.post(&format!("/api/apps/{front_id}/backups"), &t, json!({}))
            .await
            .status,
        StatusCode::BAD_REQUEST,
        "static app without DB has nothing to back up"
    );
    // ── 백업 예약 ──
    let r = ctx
        .post(
            "/api/backup-jobs",
            &t,
            json!({"target":"app","app_id":api_id,"schedule":"daily","hour":3,"minute":30,"keep_count":3}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["target_name"], "Example API");
    assert!(r.body["next_run_at"].is_string());
    let job_id = r.body["id"].as_u64().unwrap();
    for body in [
        json!({"target":"app","app_id":api_id,"schedule":"monthly","keep_count":3}),
        json!({"target":"app","app_id":api_id,"schedule":"daily","hour":25,"keep_count":3}),
        json!({"target":"app","schedule":"daily","keep_count":3}),
        json!({"target":"app","app_id":api_id,"schedule":"daily","keep_count":0}),
        json!({"target":"nope","schedule":"daily","keep_count":3}),
    ] {
        assert_eq!(
            ctx.post("/api/backup-jobs", &t, body.clone()).await.status,
            StatusCode::BAD_REQUEST,
            "{body}"
        );
    }
    let r = ctx
        .req(Method::PUT, &format!("/api/backup-jobs/{job_id}"), Some(&t), None,
            Some(json!({"name":"API 매주","target":"app","app_id":api_id,"schedule":"weekly","weekday":0,"hour":4,"keep_count":1,"enabled":false})))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["name"], "API 매주");
    assert!(
        r.body["next_run_at"].is_null(),
        "disabled job has no next run"
    );
    let r = ctx
        .post(&format!("/api/backup-jobs/{job_id}/run"), &t, json!({}))
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{:?}", r.body);
    assert_eq!(r.body["started"], 2, "db + data");
    let rows = wait_backups(&ctx, &t, &format!("job_id={job_id}")).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["job_name"], "API 매주");
    // 한 번 더 실행 → 보관 1개라 대상별로 최신 것만 남음
    ctx.post(&format!("/api/backup-jobs/{job_id}/run"), &t, json!({}))
        .await;
    let mut kept = Vec::new();
    for _ in 0..50 {
        kept = wait_backups(&ctx, &t, &format!("job_id={job_id}")).await;
        if kept.len() == 2
            && kept
                .iter()
                .all(|b| b["id"].as_u64() > rows[0]["id"].as_u64())
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(kept.len(), 2, "pruned to keep_count per target: {kept:?}");
    let r = ctx
        .post(
            "/api/backup-jobs",
            &t,
            json!({"target":"all","schedule":"hourly","minute":5,"keep_count":2}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let all_id = r.body["id"].as_u64().unwrap();
    let r = ctx
        .get(&format!("/api/backup-jobs?app_id={api_id}"), &t)
        .await;
    assert_eq!(r.body["jobs"].as_array().unwrap().len(), 2);
    assert_eq!(
        ctx.req(
            Method::DELETE,
            &format!("/api/backup-jobs/{job_id}"),
            Some(&t),
            None,
            Some(json!({"delete_files": true}))
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert!(ctx
        .get(&format!("/api/backups?job_id={job_id}"), &t)
        .await
        .body
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        ctx.req(
            Method::DELETE,
            &format!("/api/backup-jobs/{all_id}"),
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
            &format!("/api/backups/{bid}"),
            Some(&t),
            None,
            None
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );

    // ── 크론 ──
    let cron_body =
        json!({"name":"정리","schedule":"*-*-* 03:00:00","command":"./hello-api --cleanup"});
    assert_eq!(
        ctx.post(&format!("/api/apps/{front_id}/cron"), &t, cron_body.clone())
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        ctx.post(
            &format!("/api/apps/{api_id}/cron"),
            &t,
            json!({"name":"x","schedule":"daily; id","command":"true"})
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    let r = ctx
        .post(&format!("/api/apps/{api_id}/cron"), &t, cron_body)
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let cid = r.body["id"].as_u64().unwrap();
    assert_eq!(
        ctx.get(&format!("/api/apps/{api_id}/cron"), &t)
            .await
            .body
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        ctx.post(&format!("/api/cron/{cid}/run"), &t, json!({}))
            .await
            .status,
        StatusCode::OK
    );
    let r = ctx
        .req(
            Method::PUT,
            &format!("/api/cron/{cid}"),
            Some(&t),
            None,
            Some(json!({"name":"정리","schedule":"hourly","command":"true","enabled":false})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        ctx.post(&format!("/api/cron/{cid}/run"), &t, json!({}))
            .await
            .status,
        StatusCode::BAD_REQUEST,
        "disabled"
    );
    let r = ctx
        .post("/api/cron/preview", &t, json!({"schedule":"*:0/5"}))
        .await;
    assert_eq!(r.body["valid"], true);
    assert_eq!(
        ctx.req(
            Method::DELETE,
            &format!("/api/cron/{cid}"),
            Some(&t),
            None,
            None
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );

    // DB 가 남아 있으면 앱 삭제 불가 → DB 삭제 후 진행
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
        StatusCode::CONFLICT
    );
    assert_eq!(
        ctx.req(
            Method::DELETE,
            &format!("/api/apps/{api_id}/databases/{db_id}"),
            Some(&t),
            None,
            None
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );

    // ── 관리자 관리 ──
    let r = ctx.get("/api/users", &t).await;
    assert_eq!(r.body["users"].as_array().unwrap().len(), 1);
    let me = r.body["me"].as_u64().unwrap();
    assert!(r.body["users"][0]["last_login_at"].is_string());
    assert!(r.body["users"][0].get("password_hash").is_none());
    for (body, code) in [
        (
            json!({"email":"bad","password":"long-enough-1"}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"email":"ops@example.com","password":"short"}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"email":"Admin@Example.com","password":"long-enough-1"}),
            StatusCode::CONFLICT,
        ),
    ] {
        assert_eq!(
            ctx.post("/api/users", &t, body.clone()).await.status,
            code,
            "{body}"
        );
    }
    let r = ctx
        .post(
            "/api/users",
            &t,
            json!({"email":"Ops@Example.com","password":"ops-password-1"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["email"], "ops@example.com");
    let ops = r.body["id"].as_u64().unwrap();
    let r = ctx
        .req(
            Method::POST,
            "/api/auth/login",
            None,
            None,
            Some(json!({"email":"ops@example.com","password":"ops-password-1"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let t_ops = r.body["access_token"].as_str().unwrap().to_string();
    assert_eq!(ctx.get("/api/auth/me", &t_ops).await.status, StatusCode::OK);
    // 비밀번호를 바꾸면 그 관리자의 세션이 끊김
    let r = ctx
        .req(
            Method::PUT,
            &format!("/api/users/{ops}"),
            Some(&t),
            None,
            Some(json!({"email":"ops2@example.com","password":"ops-password-2"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["email"], "ops2@example.com");
    assert_eq!(
        ctx.get("/api/auth/me", &t_ops).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        ctx.req(
            Method::DELETE,
            &format!("/api/users/{me}"),
            Some(&t),
            None,
            None
        )
        .await
        .status,
        StatusCode::BAD_REQUEST,
        "cannot delete self"
    );
    assert_eq!(
        ctx.req(
            Method::DELETE,
            &format!("/api/users/{ops}"),
            Some(&t),
            None,
            None
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        ctx.get("/api/users", &t).await.body["users"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    // ── 독립 DB · 계정 정리 ──
    for (m, uri) in [
        (
            Method::DELETE,
            format!("/api/databases/{an_id}/users/{ro_id}"),
        ),
        (Method::DELETE, format!("/api/databases/{an_id}")),
        (Method::DELETE, format!("/api/redis-users/{ru_id}")),
    ] {
        assert_eq!(
            ctx.req(m, &uri, Some(&t), None, None).await.status,
            StatusCode::NO_CONTENT,
            "{uri}"
        );
    }
    assert_eq!(
        ctx.req(
            Method::DELETE,
            &format!("/api/databases/{an_id}"),
            Some(&t),
            None,
            None
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

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

    // ── DB 서버 버전 · 패널 접속 IP ──
    {
        let r = ctx
            .req(
                Method::POST,
                "/api/auth/login",
                None,
                None,
                Some(json!({"email":"admin@example.com","password":"new-password-22"})),
            )
            .await;
        let t = r.body["access_token"].as_str().unwrap().to_string();
        let r = ctx.get("/api/db-engines", &t).await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        let engines = r.body["engines"].as_array().unwrap();
        assert_eq!(engines.len(), 3);
        let maria = engines.iter().find(|e| e["engine"] == "mariadb").unwrap();
        assert_eq!(maria["installed"], true, "{maria}");
        assert!(maria["series"].as_str().is_some());
        assert!(
            engines.iter().find(|e| e["engine"] == "redis").unwrap()["version"]
                .as_str()
                .is_some()
        );
        for (eng, v, code) in [
            ("mariadb", "9.9", StatusCode::BAD_REQUEST),
            ("mariadb", "11.4;id", StatusCode::BAD_REQUEST),
            ("oracle", "1", StatusCode::NOT_FOUND),
        ] {
            let r = ctx
                .post(&format!("/api/db-engines/{eng}"), &t, json!({"version": v}))
                .await;
            assert_eq!(r.status, code, "{eng} {v} {:?}", r.body);
        }
        // 이미 그 버전이면 거부
        let cur = maria["series"].as_str().unwrap().to_string();
        if maria["versions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["version"] == cur.as_str())
        {
            let r = ctx
                .post("/api/db-engines/mariadb", &t, json!({"version": cur}))
                .await;
            assert_eq!(r.status, StatusCode::BAD_REQUEST, "{:?}", r.body);
        }
        // 만든 DB 가 있으면 어느 쪽으로도 바꾸지 않음
        let tmp_db: u64 = sqlx::query("INSERT INTO app_databases (name, username, password, env_key) VALUES ('engine_guard', 'engine_guard', 'x', 'X_URL')")
            .execute(&state.db)
            .await
            .unwrap()
            .last_insert_id();
        let r = ctx.get("/api/db-engines", &t).await;
        let maria = r.body["engines"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["engine"] == "mariadb")
            .unwrap()
            .clone();
        assert!(maria["user_dbs"].as_i64().unwrap() >= 1);
        for v in maria["versions"].as_array().unwrap() {
            assert_eq!(v["available"], false, "{v}");
        }
        let other = maria["versions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["current"] == false)
            .unwrap()["version"]
            .as_str()
            .unwrap()
            .to_string();
        let r = ctx
            .post("/api/db-engines/mariadb", &t, json!({"version": other}))
            .await;
        assert_eq!(r.status, StatusCode::CONFLICT, "{:?}", r.body);
        sqlx::query("DELETE FROM app_databases WHERE id = ?")
            .bind(tmp_db)
            .execute(&state.db)
            .await
            .unwrap();

        // 저장 위치 (백업 · 프로젝트)
        let r = ctx.get("/api/system/storage", &t).await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        let items = r.body["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["kind"], "backups");
        assert_eq!(items[0]["custom"], false);
        assert!(items[0]["path"].as_str().unwrap().ends_with("/backups"));
        assert!(items[1]["free_bytes"].as_u64().is_some());
        for (kind, path, code) in [
            ("backups", "/etc/rpanel-b", StatusCode::BAD_REQUEST),
            ("backups", "/data/../etc", StatusCode::BAD_REQUEST),
            ("backups", "relative", StatusCode::BAD_REQUEST),
            ("backups", "", StatusCode::BAD_REQUEST),
            ("nope", "/data/x", StatusCode::NOT_FOUND),
        ] {
            let r = ctx
                .req(
                    Method::PUT,
                    &format!("/api/system/storage/{kind}"),
                    Some(&t),
                    None,
                    Some(json!({ "path": path })),
                )
                .await;
            assert_eq!(r.status, code, "{kind} {path} {:?}", r.body);
        }
        let r = ctx
            .req(
                Method::PUT,
                "/api/system/storage/backups",
                Some(&t),
                None,
                Some(json!({ "path": "/data/rpanel-backups" })),
            )
            .await;
        assert_eq!(r.status, StatusCode::ACCEPTED, "{:?}", r.body);
        let mut done = false;
        for _ in 0..50 {
            let r = ctx.get("/api/system/storage", &t).await;
            if r.body["job"]["status"] == "done" {
                assert!(
                    r.body["job"]["log"]
                        .as_str()
                        .unwrap()
                        .contains("storage-set backups /data/rpanel-backups"),
                    "{:?}",
                    r.body
                );
                done = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(done, "storage job did not finish");

        let pa = "/api/system/panel-access";
        assert_eq!(ctx.get(pa, &t).await.body["restricted"], false);
        let r = ctx
            .req(
                Method::PUT,
                pa,
                Some(&t),
                None,
                Some(json!({"allow":["10.0.0.0/24","not-an-ip"]})),
            )
            .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST);
        let r = ctx
            .req(
                Method::PUT,
                pa,
                Some(&t),
                None,
                Some(json!({"allow":[" 10.0.0.0/24 ","203.0.113.5/32","10.0.0.0/24",""]})),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        assert_eq!(r.body["allow"], json!(["10.0.0.0/24", "203.0.113.5"]));
        assert_eq!(ctx.get(pa, &t).await.body["restricted"], true);
        let r = ctx
            .req(Method::PUT, pa, Some(&t), None, Some(json!({"allow":[]})))
            .await;
        assert_eq!(r.body["restricted"], false);
    }

    // ── DB 웹 관리 (테스트 DB 계정으로 직접 접속) ──
    {
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
        let url = state.cfg.database_url.clone();
        let rest = url.strip_prefix("mysql://").unwrap();
        let (userpass, hostdb) = rest.split_once('@').unwrap();
        let (dbuser, dbpass) = userpass.split_once(':').unwrap();
        let dbname = hostdb
            .split_once('/')
            .unwrap()
            .1
            .split('?')
            .next()
            .unwrap()
            .to_string();
        sqlx::query("DROP TABLE IF EXISTS dbweb_t")
            .execute(&state.db)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE dbweb_t (id INT UNSIGNED AUTO_INCREMENT PRIMARY KEY, name VARCHAR(40) NOT NULL, price DECIMAL(10,2) NULL, made_at DATETIME NULL)")
            .execute(&state.db)
            .await
            .unwrap();
        let wid: u64 = sqlx::query("INSERT INTO app_databases (name, username, password, env_key) VALUES (?, ?, ?, 'DBWEB_URL')")
            .bind(&dbname)
            .bind(dbuser)
            .bind(dbpass)
            .execute(&state.db)
            .await
            .unwrap()
            .last_insert_id();
        let base = format!("/api/dbweb/admin/{wid}");
        let r = ctx.get(&base, &t).await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        assert!(r.body["tables"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["name"] == "dbweb_t"));
        let r = ctx
            .post(
                &format!("{base}/table/dbweb_t/insert"),
                &t,
                json!({"values":{"name":"사과","price":"1200.50","made_at":"2026-01-02 03:04:05"}}),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        assert_eq!(r.body["affected"], 1);
        let rid = r.body["last_insert_id"].as_u64().unwrap();
        ctx.post(
            &format!("{base}/table/dbweb_t/insert"),
            &t,
            json!({"values":{"name":"배"}}),
        )
        .await;
        let r = ctx
            .get(&format!("{base}/table/dbweb_t?size=1&page=1"), &t)
            .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        assert_eq!(r.body["total"], 2);
        assert_eq!(r.body["primary_key"], json!(["id"]));
        assert_eq!(r.body["rows"].as_array().unwrap().len(), 1);
        assert_eq!(r.body["rows"][0][0], json!(rid));
        assert_eq!(r.body["rows"][0][1], "사과");
        assert_eq!(r.body["rows"][0][2], "1200.50");
        assert_eq!(r.body["rows"][0][3], "2026-01-02 03:04:05");
        let r = ctx
            .post(
                &format!("{base}/table/dbweb_t/update"),
                &t,
                json!({"key":{"id":rid},"values":{"name":"청사과","price":null}}),
            )
            .await;
        assert_eq!(r.body["affected"], 1, "{:?}", r.body);
        for (op, body) in [
            ("update", json!({"key":{"id":rid},"values":{"nope":1}})),
            ("update", json!({"key":{},"values":{"name":"x"}})),
            ("delete", json!({"key":{}})),
        ] {
            let r = ctx
                .post(&format!("{base}/table/dbweb_t/{op}"), &t, body.clone())
                .await;
            assert_eq!(r.status, StatusCode::BAD_REQUEST, "{op} {body}");
        }
        assert_eq!(
            ctx.get(&format!("{base}/table/no_such"), &t).await.status,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            ctx.get(&format!("{base}/table/a%60b"), &t).await.status,
            StatusCode::NOT_FOUND
        );
        let r = ctx
            .post(
                &format!("{base}/query"),
                &t,
                json!({"sql":"SELECT name, price FROM dbweb_t ORDER BY id"}),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK);
        assert_eq!(r.body["columns"], json!(["name", "price"]));
        assert_eq!(r.body["rows"][0], json!(["청사과", null]));
        let r = ctx
            .post(&format!("{base}/query"), &t, json!({"sql":"SELEC oops"}))
            .await;
        assert_eq!(r.status, StatusCode::OK);
        assert!(
            r.body["error"].as_str().unwrap().contains("syntax"),
            "{:?}",
            r.body
        );
        // prepared 로 안 되는 문장도 실행
        let r = ctx
            .post(
                &format!("{base}/query"),
                &t,
                json!({"sql":"SHOW CREATE TABLE dbweb_t"}),
            )
            .await;
        assert!(r.body["error"].is_null(), "{:?}", r.body);
        let r = ctx
            .post(
                &format!("{base}/query"),
                &t,
                json!({"sql":"DELETE FROM dbweb_t WHERE name = '배'"}),
            )
            .await;
        assert_eq!(r.body["affected"], 1, "{:?}", r.body);
        let r = ctx
            .post(
                &format!("{base}/table/dbweb_t/delete"),
                &t,
                json!({"key":{"id":rid}}),
            )
            .await;
        assert_eq!(r.body["affected"], 1, "{:?}", r.body);
        // 관리자가 아니면 거부, 없는 id 는 404
        assert_eq!(
            ctx.get("/api/dbweb/admin/999999", &t).await.status,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            ctx.req(Method::GET, &base, None, None, None).await.status,
            StatusCode::UNAUTHORIZED
        );

        // DB 계정 로그인 입구
        let login = |u: &str, p: &str| json!({"username": u, "password": p});
        for (body, code) in [
            (login(dbuser, "wrong-password"), StatusCode::BAD_REQUEST),
            (login("root", "x"), StatusCode::BAD_REQUEST),
            (login("", "x"), StatusCode::BAD_REQUEST),
        ] {
            let r = ctx
                .req(
                    Method::POST,
                    "/api/dbweb/login",
                    None,
                    None,
                    Some(body.clone()),
                )
                .await;
            assert_eq!(r.status, code, "{body} {:?}", r.body);
        }
        // 패널 자신의 DB 는 목록에서 빠짐
        let r = ctx
            .req(
                Method::POST,
                "/api/dbweb/login",
                None,
                None,
                Some(login(dbuser, dbpass)),
            )
            .await;
        if r.status == StatusCode::OK {
            assert!(!r.body["databases"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d == dbname.as_str()));
        } else {
            assert_eq!(r.status, StatusCode::BAD_REQUEST, "{:?}", r.body);
        }
        // 세션이 있으면 그 계정이 볼 수 있는 DB 만
        let token = "ab".repeat(24);
        let mut rc = state.redis.clone();
        let _: () = redis::cmd("SET")
            .arg(state.key(&["dbweb", "session", &token]))
            .arg(json!({"username": dbuser, "password": dbpass, "databases": [dbname]}).to_string())
            .arg("EX")
            .arg(60)
            .query_async(&mut rc)
            .await
            .unwrap();
        let sreq = |m: Method, uri: String, body: Option<Value>| {
            let mut b = Request::builder()
                .method(m)
                .uri(uri)
                .header("x-requested-with", "rpanel")
                .header("x-db-token", token.clone());
            if body.is_some() {
                b = b.header(header::CONTENT_TYPE, "application/json");
            }
            b.body(
                body.map(|v| Body::from(v.to_string()))
                    .unwrap_or_else(Body::empty),
            )
            .unwrap()
        };
        let res = ctx
            .app
            .clone()
            .oneshot(sreq(Method::GET, "/api/dbweb/me".into(), None))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let res = ctx
            .app
            .clone()
            .oneshot(sreq(Method::GET, format!("/api/dbweb/s/{dbname}"), None))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let res = ctx
            .app
            .clone()
            .oneshot(sreq(Method::GET, "/api/dbweb/s/mysql".into(), None))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        let res = ctx
            .app
            .clone()
            .oneshot(sreq(
                Method::POST,
                format!("/api/dbweb/s/{dbname}/query"),
                Some(json!({"sql":"SELECT COUNT(*) AS n FROM dbweb_t"})),
            ))
            .await
            .unwrap();
        let v: Value =
            serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(v["rows"][0][0], 0, "{v}");
        let res = ctx
            .app
            .clone()
            .oneshot(sreq(Method::POST, "/api/dbweb/logout".into(), None))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        let res = ctx
            .app
            .clone()
            .oneshot(sreq(Method::GET, "/api/dbweb/me".into(), None))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        sqlx::query("DELETE FROM app_databases WHERE id = ?")
            .bind(wid)
            .execute(&state.db)
            .await
            .unwrap();
        sqlx::query("DROP TABLE dbweb_t")
            .execute(&state.db)
            .await
            .unwrap();
    }

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
