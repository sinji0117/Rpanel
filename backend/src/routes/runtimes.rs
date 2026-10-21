//! 런타임 버전 API — 설치·기본 버전 변경·삭제, 앱별 버전 선택

use std::collections::BTreeMap;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::json;

use crate::{
    audit,
    auth::{AuthUser, ClientIp},
    error::{AppError, AppResult},
    models::App,
    routes::apps,
    runtimes::{self, Lang, Runtime, RUNTIME_COLUMNS},
    state::AppState,
};

async fn fetch(state: &AppState, id: u64) -> AppResult<Runtime> {
    sqlx::query_as::<_, Runtime>(&format!(
        "SELECT {RUNTIME_COLUMNS} FROM runtimes WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)
}

/// 언어·버전별로 그 버전을 쓰는 앱 수 (기본 버전을 쓰는 앱은 기본 버전에 포함)
async fn usage(state: &AppState, rows: &[Runtime]) -> AppResult<BTreeMap<String, u32>> {
    let apps =
        sqlx::query_as::<_, App>(&format!("SELECT {} FROM apps", crate::models::APP_COLUMNS))
            .fetch_all(&state.db)
            .await?;
    let mut out = BTreeMap::new();
    for a in &apps {
        let Some(lang) = runtimes::lang_for(a) else {
            continue;
        };
        let version = a.runtime_version.clone().or_else(|| {
            rows.iter()
                .find(|r| r.lang == lang.as_str() && r.is_default)
                .map(|r| r.version.clone())
        });
        if let Some(v) = version {
            *out.entry(format!("{}:{v}", lang.as_str())).or_insert(0) += 1;
        }
    }
    Ok(out)
}

/// GET /api/runtimes
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
) -> AppResult<Json<serde_json::Value>> {
    user.require_admin()?;
    let rows = runtimes::list(&state).await?;
    let used = usage(&state, &rows).await?;
    let candidates: BTreeMap<&str, Vec<String>> = Lang::ALL
        .iter()
        .map(|l| (l.as_str(), runtimes::candidates(*l)))
        .collect();
    Ok(Json(
        json!({ "runtimes": rows, "candidates": candidates, "usage": used }),
    ))
}

#[derive(Deserialize)]
pub struct InstallInput {
    pub lang: String,
    pub version: String,
    /// 설치가 끝나면 기본 버전으로
    #[serde(default)]
    pub make_default: bool,
}

/// POST /api/runtimes — 설치(백그라운드). 이미 설치돼 있으면 기본 버전만 바꿈
pub async fn install(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Json(input): Json<InstallInput>,
) -> AppResult<(StatusCode, Json<Runtime>)> {
    user.require_admin()?;
    let lang = Lang::parse(&input.lang)
        .ok_or_else(|| AppError::bad("언어는 rust · python · node · java 중 하나입니다"))?;
    let version = input.version.trim().to_string();
    runtimes::validate(lang, &version).map_err(AppError::BadRequest)?;
    runtimes::ensure_seeded(&state).await?;

    let existing: Option<Runtime> = sqlx::query_as(&format!(
        "SELECT {RUNTIME_COLUMNS} FROM runtimes WHERE lang = ? AND version = ?"
    ))
    .bind(lang.as_str())
    .bind(&version)
    .fetch_optional(&state.db)
    .await?;
    let id = match existing {
        Some(r) if r.status == "ready" => {
            if input.make_default {
                runtimes::set_default(&state, r.id).await?;
                audit::log(
                    &state,
                    Some(user.id),
                    "runtime.default",
                    &format!("{} {version}", lang.as_str()),
                    &ip,
                )
                .await;
            }
            return Ok((StatusCode::OK, Json(fetch(&state, r.id).await?)));
        }
        Some(r) if r.status == "installing" => {
            return Err(AppError::Conflict(format!(
                "{} {version} 을(를) 설치하는 중입니다",
                lang.label()
            )))
        }
        Some(r) => {
            if r.is_system {
                return Err(AppError::bad(
                    "서버 기본 버전은 다시 설치할 수 없습니다 — 서버에서 직접 설치하세요",
                ));
            }
            sqlx::query("UPDATE runtimes SET status = 'installing', log = NULL WHERE id = ?")
                .bind(r.id)
                .execute(&state.db)
                .await?;
            r.id
        }
        None => {
            if version == "system" {
                return Err(AppError::bad("서버 기본 버전은 설치할 수 없습니다"));
            }
            sqlx::query("INSERT INTO runtimes (lang, version, status) VALUES (?, ?, 'installing')")
                .bind(lang.as_str())
                .bind(&version)
                .execute(&state.db)
                .await?
                .last_insert_id()
        }
    };
    audit::log(
        &state,
        Some(user.id),
        "runtime.install",
        &format!("{} {version}", lang.as_str()),
        &ip,
    )
    .await;
    tokio::spawn(runtimes::install(
        state.clone(),
        id,
        lang,
        version,
        input.make_default,
    ));
    Ok((StatusCode::ACCEPTED, Json(fetch(&state, id).await?)))
}

/// PUT /api/runtimes/{id}/default
pub async fn make_default(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
) -> AppResult<Json<Runtime>> {
    user.require_admin()?;
    let r = fetch(&state, id).await?;
    if r.status != "ready" {
        return Err(AppError::bad(
            "설치가 끝난 버전만 기본으로 정할 수 있습니다",
        ));
    }
    runtimes::set_default(&state, id).await?;
    audit::log(
        &state,
        Some(user.id),
        "runtime.default",
        &format!("{} {}", r.lang, r.version),
        &ip,
    )
    .await;
    Ok(Json(fetch(&state, id).await?))
}

/// DELETE /api/runtimes/{id}
pub async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
) -> AppResult<StatusCode> {
    user.require_admin()?;
    let r = fetch(&state, id).await?;
    if r.is_system {
        return Err(AppError::bad("서버 기본 버전은 지울 수 없습니다"));
    }
    if r.is_default {
        return Err(AppError::bad(
            "기본 버전은 지울 수 없습니다 — 다른 버전을 기본으로 정한 뒤 지우세요",
        ));
    }
    if r.status == "installing" {
        return Err(AppError::Conflict("설치 중에는 지울 수 없습니다".into()));
    }
    let lang = Lang::parse(&r.lang).ok_or(AppError::NotFound)?;
    let kinds = runtimes::kinds_for(lang);
    let sql = format!(
        "SELECT COUNT(*) FROM apps WHERE runtime_version = ? AND kind IN ({})",
        vec!["?"; kinds.len()].join(", ")
    );
    let mut q = sqlx::query_as::<_, (i64,)>(&sql).bind(&r.version);
    for k in kinds {
        q = q.bind(*k);
    }
    let (n,) = q.fetch_one(&state.db).await?;
    if n > 0 {
        return Err(AppError::Conflict(format!(
            "앱 {n}개가 이 버전을 쓰고 있습니다 — 앱 설정에서 버전을 바꾼 뒤 지우세요"
        )));
    }
    if r.status == "ready" {
        runtimes::uninstall(&state, lang, &r.version).await?;
    }
    sqlx::query("DELETE FROM runtimes WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    audit::log(
        &state,
        Some(user.id),
        "runtime.delete",
        &format!("{} {}", r.lang, r.version),
        &ip,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct AppRuntimeInput {
    /// None = 기본 버전 사용
    pub version: Option<String>,
}

/// PUT /api/apps/{id}/runtime — 앱이 쓸 버전 (다음 배포·적용부터)
pub async fn set_for_app(
    State(state): State<AppState>,
    user: AuthUser,
    ClientIp(ip): ClientIp,
    Path(id): Path<u64>,
    Json(input): Json<AppRuntimeInput>,
) -> AppResult<Json<App>> {
    user.require_admin()?;
    let app = apps::fetch(&state, id).await?;
    let lang = runtimes::lang_for(&app).ok_or_else(|| {
        AppError::bad("이 앱은 언어 버전을 고를 수 없습니다 (업로드형·HTML·프록시)")
    })?;
    let version = input
        .version
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    if let Some(v) = &version {
        runtimes::validate(lang, v).map_err(AppError::BadRequest)?;
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM runtimes WHERE lang = ? AND version = ?")
                .bind(lang.as_str())
                .bind(v)
                .fetch_optional(&state.db)
                .await?;
        if status.as_deref() != Some("ready") {
            return Err(AppError::bad(format!(
                "{} {v} 이(가) 설치되어 있지 않습니다 — 런타임 메뉴에서 먼저 설치하세요",
                lang.label()
            )));
        }
    }
    sqlx::query("UPDATE apps SET runtime_version = ? WHERE id = ?")
        .bind(&version)
        .bind(id)
        .execute(&state.db)
        .await?;
    audit::log(
        &state,
        Some(user.id),
        "app.runtime",
        &format!("{} {}", app.slug, version.as_deref().unwrap_or("default")),
        &ip,
    )
    .await;
    Ok(Json(apps::fetch(&state, id).await?))
}
