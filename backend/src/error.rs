use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    BadRequest(String),
    #[error("인증이 필요합니다")]
    Unauthorized,
    #[error("{0}")]
    Forbidden(String),
    #[error("찾을 수 없습니다")]
    NotFound,
    #[error("{0}")]
    Conflict(String),
    #[error("요청이 너무 많습니다. 잠시 후 다시 시도하세요")]
    TooManyRequests,
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Redis(#[from] redis::RedisError),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    pub fn bad(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            AppError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            AppError::Forbidden(_) => (StatusCode::FORBIDDEN, "forbidden"),
            AppError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            AppError::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            AppError::TooManyRequests => (StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
            AppError::Db(sqlx::Error::RowNotFound) => (StatusCode::NOT_FOUND, "not_found"),
            AppError::Db(e) if is_unique_violation(e) => (StatusCode::CONFLICT, "conflict"),
            AppError::Db(_) | AppError::Redis(_) | AppError::Internal(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "internal")
            }
        };

        let message = match &self {
            AppError::Db(sqlx::Error::RowNotFound) => "찾을 수 없습니다".to_string(),
            AppError::Db(e) if is_unique_violation(e) => "이미 존재하는 값입니다".to_string(),
            AppError::Db(_) | AppError::Redis(_) | AppError::Internal(_) => {
                tracing::error!(error = ?self, "internal error");
                "서버 내부 오류".to_string()
            }
            other => other.to_string(),
        };

        (status, Json(json!({ "error": code, "message": message }))).into_response()
    }
}

pub fn is_unique_violation(e: &sqlx::Error) -> bool {
    match e {
        sqlx::Error::Database(db) => {
            db.code().as_deref() == Some("23000") || db.message().contains("Duplicate entry")
        }
        _ => false,
    }
}
