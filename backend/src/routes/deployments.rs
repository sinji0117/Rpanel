use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{
    audit,
    auth::{AuthUser, ClientIp},
    deploy::runner,
    error::{AppError, AppResult},
    models::{Deployment, APP_COLUMNS, DEPLOY_COLUMNS},
    routes::apps,
    state::AppState,
};

#[derive(Serialize)]
pub struct DeploymentDetail {
    #[serde(flatten)]
    pub deployment: Deployment,
    pub log: Option<String>,
    pub app_name: String,
    pub app_slug: String,
}

/// 새 배포 생성 + 백그라운드 실행. 같은 앱의 진행 중 배포가 있으면 409.
pub async fn enqueue(state: &AppState, app_id: u64, user_id: Option<u64>) -> AppResult<u64> {
    let mut tx = state.db.begin().await?;
    // 앱 행 잠금으로 동시 트리거 직렬화
    let _: (u64,) = sqlx::query_as("SELECT id FROM apps WHERE id = ? FOR UPDATE")
        .bind(app_id)
        .fetch_one(&mut *tx)
        .await?;
    let active: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM deployments WHERE app_id = ? AND status IN ('queued','running')",
    )
    .bind(app_id)
    .fetch_one(&mut *tx)
    .await?;
    if active > 0 {
        return Err(AppError::Conflict("이미 진행 중인 배포가 있습니다".into()));
    }
    let res = sqlx::query(
        "INSERT INTO deployments (app_id, status, triggered_by) VALUES (?, 'queued', ?)",
    )
    .bind(app_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    let id = res.last_insert_id();
    runner::spawn(state.clone(), id, app_id);
    Ok(id)
}

pub async fn deploy(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(app_id): Path<u64>,
) -> AppResult<(StatusCode, Json<serde_json::Value>)> {
    user.require_admin()?;
    let app = apps::fetch(&state, app_id).await?;
    let id = enqueue(&state, app_id, Some(user.id)).await?;
    audit::log(
        &state,
        Some(user.id),
        "app.deploy",
        &format!("{} #{id}", app.slug),
        &ip,
    )
    .await;
    Ok((StatusCode::ACCEPTED, Json(json!({ "deployment_id": id }))))
}

#[derive(Deserialize)]
pub struct Paging {
    pub limit: Option<u32>,
}

pub async fn list_for_app(
    State(state): State<AppState>,
    _u: AuthUser,
    Path(app_id): Path<u64>,
    Query(p): Query<Paging>,
) -> AppResult<Json<Vec<Deployment>>> {
    apps::fetch(&state, app_id).await?;
    let limit = p.limit.unwrap_or(30).clamp(1, 200);
    let rows = sqlx::query_as::<_, Deployment>(&format!(
        "SELECT {DEPLOY_COLUMNS} FROM deployments WHERE app_id = ? ORDER BY id DESC LIMIT ?"
    ))
    .bind(app_id)
    .bind(limit)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

pub async fn get(
    State(state): State<AppState>,
    _u: AuthUser,
    Path(id): Path<u64>,
) -> AppResult<Json<DeploymentDetail>> {
    Ok(Json(detail(&state, id).await?))
}

pub async fn detail(state: &AppState, id: u64) -> AppResult<DeploymentDetail> {
    let d = sqlx::query_as::<_, Deployment>(&format!(
        "SELECT {DEPLOY_COLUMNS} FROM deployments WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    let (log, app_name, app_slug): (Option<String>, String, String) =
        sqlx::query_as("SELECT d.log, a.name, a.slug FROM deployments d JOIN apps a ON a.id = d.app_id WHERE d.id = ?")
            .bind(id)
            .fetch_one(&state.db)
            .await?;
    Ok(DeploymentDetail {
        deployment: d,
        log,
        app_name,
        app_slug,
    })
}

pub async fn cancel(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let d = detail(&state, id).await?;
    if !matches!(d.deployment.status.as_str(), "queued" | "running") {
        return Err(AppError::Conflict("이미 끝난 배포입니다".into()));
    }
    let signalled = runner::cancel(&state, id);
    if !signalled {
        // 프로세스에 없는 고아 배포 → 바로 취소 처리
        sqlx::query("UPDATE deployments SET status = 'cancelled', finished_at = CURRENT_TIMESTAMP(3) WHERE id = ?")
            .bind(id)
            .execute(&state.db)
            .await?;
    }
    audit::log(
        &state,
        Some(user.id),
        "deploy.cancel",
        &format!("{} #{id}", d.app_slug),
        &ip,
    )
    .await;
    Ok(Json(json!({ "ok": true })))
}

pub async fn rollback(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let d = detail(&state, id).await?;
    if d.deployment.status != "success" {
        return Err(AppError::bad("성공한 배포로만 되돌릴 수 있습니다"));
    }
    let app = apps::fetch(&state, d.deployment.app_id).await?;
    if app.current_release == Some(id) {
        return Err(AppError::bad("이미 현재 릴리스입니다"));
    }
    if app.status == "deploying" {
        return Err(AppError::Conflict(
            "배포 진행 중에는 되돌릴 수 없습니다".into(),
        ));
    }
    let out = runner::rollback(&state, &app, id, d.deployment.commit_sha.clone())
        .await
        .map_err(|e| AppError::BadRequest(format!("{e:#}")))?;
    audit::log(
        &state,
        Some(user.id),
        "deploy.rollback",
        &format!("{} → #{id}", app.slug),
        &ip,
    )
    .await;
    Ok(Json(json!({ "ok": true, "log": out })))
}

#[derive(Serialize, sqlx::FromRow)]
pub struct RecentDeployment {
    pub id: u64,
    pub app_id: u64,
    pub app_name: String,
    pub app_slug: String,
    pub status: String,
    pub commit_sha: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub async fn recent(state: &AppState, limit: u32) -> AppResult<Vec<RecentDeployment>> {
    Ok(sqlx::query_as::<_, RecentDeployment>(
        "SELECT d.id, d.app_id, a.name AS app_name, a.slug AS app_slug, d.status, d.commit_sha, d.created_at, d.finished_at \
         FROM deployments d JOIN apps a ON a.id = d.app_id ORDER BY d.id DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(&state.db)
    .await?)
}

/// 패널 재시작 시, 끝나지 못한 배포를 실패 처리
pub async fn recover_orphans(state: &AppState) -> anyhow::Result<()> {
    let n = sqlx::query(
        "UPDATE deployments SET status = 'failed', error = '패널 재시작으로 중단되었습니다', finished_at = CURRENT_TIMESTAMP(3) \
         WHERE status IN ('queued','running')",
    )
    .execute(&state.db)
    .await?
    .rows_affected();
    sqlx::query("UPDATE apps SET status = IF(current_release IS NULL, 'failed', 'running') WHERE status = 'deploying'")
        .execute(&state.db)
        .await?;
    if n > 0 {
        tracing::warn!(count = n, "marked orphaned deployments as failed");
    }
    Ok(())
}

// ───────────── Webhook (GitLab / GitHub / 일반) ─────────────

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    // 길이를 숨기기 위해 해시끼리 비교
    let ha = Sha256::digest(a);
    let hb = Sha256::digest(b);
    ha.iter()
        .zip(hb.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// HMAC-SHA256 (GitHub 웹훅 서명 확인용)
pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; BLOCK];
    let mut opad = [0x5cu8; BLOCK];
    for i in 0..BLOCK {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }
    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(msg);
    let ih = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(ih);
    outer.finalize().into()
}

/// GitHub: `X-Hub-Signature-256: sha256=<hex>` 를 webhook 토큰(Secret)으로 확인
fn github_signature_ok(headers: &HeaderMap, body: &[u8], secret: &str) -> bool {
    let Some(sig) = headers
        .get("x-hub-signature-256")
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    let Some(hex_sig) = sig.strip_prefix("sha256=") else {
        return false;
    };
    let expected = hex::encode(hmac_sha256(secret.as_bytes(), body));
    ct_eq(expected.as_bytes(), hex_sig.to_ascii_lowercase().as_bytes())
}

/// POST /api/hooks/{slug}
/// 인증: GitHub `X-Hub-Signature-256`(Secret = 웹훅 토큰), 또는 `X-Gitlab-Token` / `X-Rpanel-Token` 헤더 (또는 ?token=)
/// 본문에 `ref`(refs/heads/<branch>)가 있으면 앱 브랜치와 같을 때만 배포
pub async fn webhook(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Path(slug): Path<String>,
    Query(q): Query<std::collections::HashMap<String, String>>,
    headers: HeaderMap,
    body: Bytes,
) -> AppResult<(StatusCode, Json<serde_json::Value>)> {
    if crate::validate::slug(&slug).is_err() {
        return Err(AppError::NotFound);
    }
    if !crate::auth::session::rate_limit(&state, &format!("hook:{ip}"), 30, 60).await? {
        return Err(AppError::TooManyRequests);
    }
    let app = sqlx::query_as::<_, crate::models::App>(&format!(
        "SELECT {APP_COLUMNS} FROM apps WHERE slug = ?"
    ))
    .bind(&slug)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    let provided = headers
        .get("x-gitlab-token")
        .or_else(|| headers.get("x-rpanel-token"))
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .or_else(|| q.get("token").cloned())
        .unwrap_or_default();
    let github = github_signature_ok(&headers, &body, &app.webhook_token);
    if !github && (provided.is_empty() || !ct_eq(provided.as_bytes(), app.webhook_token.as_bytes()))
    {
        audit::log(&state, None, "hook.denied", &slug, &ip).await;
        return Err(AppError::Unauthorized);
    }
    // GitHub 이 웹훅을 만들 때 보내는 ping, push 가 아닌 이벤트는 배포하지 않음
    match headers.get("x-github-event").and_then(|v| v.to_str().ok()) {
        Some("ping") => return Ok((StatusCode::OK, Json(json!({ "pong": true })))),
        Some(ev) if ev != "push" => {
            return Ok((
                StatusCode::OK,
                Json(json!({ "skipped": true, "reason": format!("event {ev}") })),
            ))
        }
        _ => {}
    }
    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&body) {
        if let Some(r) = v.get("ref").and_then(|r| r.as_str()) {
            let branch = r.strip_prefix("refs/heads/").unwrap_or(r);
            if branch != app.git_branch {
                return Ok((
                    StatusCode::OK,
                    Json(
                        json!({ "skipped": true, "reason": format!("branch {branch} != {}", app.git_branch) }),
                    ),
                ));
            }
        }
    }
    let id = enqueue(&state, app.id, None).await?;
    audit::log(&state, None, "hook.deploy", &format!("{slug} #{id}"), &ip).await;
    Ok((StatusCode::ACCEPTED, Json(json!({ "deployment_id": id }))))
}

#[cfg(test)]
mod tests {
    use super::{ct_eq, hmac_sha256};

    #[test]
    fn hmac_matches_rfc4231() {
        // RFC 4231 test case 2
        assert_eq!(
            hex::encode(hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        // GitHub 문서 예시
        assert_eq!(
            hex::encode(hmac_sha256(b"It's a Secret to Everybody", b"Hello, World!")),
            "757107ea0eb2509fc211221cce984b8a37570b6d7586c22c46f4379c8b043e17"
        );
        let long_key = [0xaau8; 131];
        assert_eq!(
            hex::encode(hmac_sha256(
                &long_key,
                b"Test Using Larger Than Block-Size Key - Hash Key First"
            )),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    #[test]
    fn constant_time_eq() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"abcd"));
        assert!(!ct_eq(b"", b"x"));
    }
}
