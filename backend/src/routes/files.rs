//! 파일 관리자 API — 경로는 사이트 유저 홈(/home/<user>) 기준 상대 경로

use std::process::Stdio;

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::Response,
    Json,
};
use futures::StreamExt;
use serde::Deserialize;
use serde_json::json;
use tokio::io::AsyncWriteExt;

use crate::{
    audit,
    auth::{AuthUser, ClientIp},
    error::{AppError, AppResult},
    files,
    routes::apps,
    state::AppState,
};

/// 편집기로 여는 파일 크기 상한
pub const MAX_EDIT_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Deserialize)]
pub struct PathQuery {
    #[serde(default)]
    pub path: String,
}

fn check_path(p: &str) -> AppResult<()> {
    if p.len() > 1024 || p.contains('\0') || p.starts_with('/') || p.split('/').any(|s| s == "..") {
        return Err(AppError::bad("잘못된 경로입니다"));
    }
    Ok(())
}

/// GET /api/apps/{id}/files?path=
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<u64>,
    Query(q): Query<PathQuery>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let app = apps::fetch(&state, id).await?;
    check_path(&q.path)?;
    let mut v = files::json(&state, &app, &["list", &q.path]).await?;
    v["root"] = json!(files::rel_root(&app));
    v["user"] = json!(app.site_user);
    Ok(Json(v))
}

/// GET /api/apps/{id}/files/content?path=  (텍스트 편집용, 2MB 이하)
pub async fn read(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<u64>,
    Query(q): Query<PathQuery>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let app = apps::fetch(&state, id).await?;
    check_path(&q.path)?;
    let st = files::json(&state, &app, &["stat", &q.path]).await?;
    if st["kind"] == "dir" {
        return Err(AppError::bad("폴더입니다"));
    }
    if st["size"].as_u64().unwrap_or(0) > MAX_EDIT_BYTES {
        return Err(AppError::bad(
            "2MB 가 넘는 파일은 편집기로 열 수 없습니다 — 다운로드하세요",
        ));
    }
    let bytes = files::output(&state, &app, &["read", &q.path], None).await?;
    if bytes.contains(&0) {
        return Err(AppError::bad("텍스트 파일이 아닙니다 — 다운로드하세요"));
    }
    let content = String::from_utf8(bytes)
        .map_err(|_| AppError::bad("UTF-8 텍스트가 아닙니다 — 다운로드하세요"))?;
    Ok(Json(
        json!({ "path": q.path, "content": content, "mode": st["mode"] }),
    ))
}

#[derive(Deserialize)]
pub struct WriteBody {
    pub path: String,
    pub content: String,
}

/// PUT /api/apps/{id}/files/content
pub async fn write(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
    Json(b): Json<WriteBody>,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let app = apps::fetch(&state, id).await?;
    check_path(&b.path)?;
    if b.content.len() as u64 > MAX_EDIT_BYTES {
        return Err(AppError::bad(
            "2MB 이하만 저장할 수 있습니다 — 업로드를 쓰세요",
        ));
    }
    let out = files::output(
        &state,
        &app,
        &["write", &b.path],
        Some(b.content.as_bytes()),
    )
    .await?;
    audit::log(
        &state,
        Some(user.id),
        "files.write",
        &format!("{}:{}", app.slug, b.path),
        &ip,
    )
    .await;
    Ok(Json(
        json!({ "ok": true, "bytes": String::from_utf8_lossy(&out).trim() }),
    ))
}

/// POST /api/apps/{id}/files/upload?path=<폴더>/<파일명>  (본문 = 파일 그대로, 스트리밍)
pub async fn upload(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
    Query(q): Query<PathQuery>,
    body: Body,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let app = apps::fetch(&state, id).await?;
    check_path(&q.path)?;
    if q.path.is_empty() || q.path.ends_with('/') {
        return Err(AppError::bad("파일 이름이 필요합니다"));
    }
    let mut c = files::command(&state, &app, &["write", &q.path])?;
    c.stdin(Stdio::piped());
    let mut child = c.spawn().map_err(|e| AppError::Internal(e.into()))?;
    let mut sin = child.stdin.take().unwrap();
    let mut stream = body.into_data_stream();
    let mut total: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| AppError::bad(format!("업로드가 끊겼습니다: {e}")))?;
        total += chunk.len() as u64;
        if sin.write_all(&chunk).await.is_err() {
            break; // 자식이 먼저 실패 — 아래에서 stderr 로 보고
        }
    }
    drop(sin);
    let out = child
        .wait_with_output()
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr);
        return Err(AppError::BadRequest(
            msg.lines()
                .last()
                .unwrap_or("업로드 실패")
                .trim_start_matches("rpanel-files: ")
                .into(),
        ));
    }
    audit::log(
        &state,
        Some(user.id),
        "files.upload",
        &format!("{}:{} ({total}B)", app.slug, q.path),
        &ip,
    )
    .await;
    Ok(Json(json!({ "ok": true, "bytes": total })))
}

/// GET /api/apps/{id}/files/download?path=
pub async fn download(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<u64>,
    Query(q): Query<PathQuery>,
) -> AppResult<Response> {
    user.require_admin()?;
    let app = apps::fetch(&state, id).await?;
    check_path(&q.path)?;
    let st = files::json(&state, &app, &["stat", &q.path]).await?;
    if st["kind"] == "dir" {
        return Err(AppError::bad(
            "폴더는 내려받을 수 없습니다 (압축해서 받으세요)",
        ));
    }
    let mut child = files::command(&state, &app, &["read", &q.path])?
        .spawn()
        .map_err(|e| AppError::Internal(e.into()))?;
    let stdout = child.stdout.take().unwrap();
    tokio::spawn(async move {
        let _ = child.wait().await;
    });
    let name: String = q
        .path
        .rsplit('/')
        .next()
        .unwrap_or("download")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._-".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, st["size"].as_u64().unwrap_or(0))
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{name}\""),
        )
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from_stream(tokio_util::io::ReaderStream::new(stdout)))
        .map_err(|e| AppError::Internal(e.into()))
}

#[derive(Deserialize)]
pub struct OpBody {
    pub op: String,
    pub path: String,
    #[serde(default)]
    pub to: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
}

/// POST /api/apps/{id}/files/op  — mkdir | rm | mv | chmod | extract
pub async fn op(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
    Json(b): Json<OpBody>,
) -> AppResult<(StatusCode, Json<serde_json::Value>)> {
    user.require_admin()?;
    let app = apps::fetch(&state, id).await?;
    check_path(&b.path)?;
    if let Some(t) = &b.to {
        check_path(t)?;
    }
    let to = b.to.clone().unwrap_or_default();
    let args: Vec<&str> = match b.op.as_str() {
        "mkdir" | "rm" => vec![b.op.as_str(), &b.path],
        "mv" | "extract" => {
            if to.is_empty() && b.op == "mv" {
                return Err(AppError::bad("새 이름이 필요합니다"));
            }
            vec![b.op.as_str(), &b.path, &to]
        }
        "chmod" => {
            let m = b.mode.as_deref().unwrap_or("");
            if !(m.len() == 3 && m.chars().all(|c| ('0'..='7').contains(&c))) {
                return Err(AppError::bad("권한은 644 처럼 8진수 3자리입니다"));
            }
            vec!["chmod", &b.path, m]
        }
        _ => return Err(AppError::bad("op 는 mkdir / rm / mv / chmod / extract")),
    };
    files::output(&state, &app, &args, None).await?;
    audit::log(
        &state,
        Some(user.id),
        &format!("files.{}", b.op),
        &format!("{}:{}", app.slug, b.path),
        &ip,
    )
    .await;
    Ok((StatusCode::OK, Json(json!({ "ok": true }))))
}

#[cfg(test)]
mod tests {
    use super::check_path;

    #[test]
    fn paths() {
        assert!(check_path("htdocs/a.com/app").is_ok());
        assert!(check_path("").is_ok());
        assert!(check_path("/etc/passwd").is_err());
        assert!(check_path("htdocs/../../etc").is_err());
        assert!(check_path("a\0b").is_err());
    }
}
