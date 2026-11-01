pub mod handlers;
pub mod jwt;
pub mod password;
pub mod secretbox;
pub mod session;
pub mod totp;
pub mod twofa;

use std::net::{IpAddr, SocketAddr};

use axum::{
    extract::{ConnectInfo, FromRequestParts},
    http::{header::AUTHORIZATION, request::Parts},
};

use crate::{error::AppError, state::AppState};

/// 인증된 사용자 (Authorization: Bearer <access JWT>)
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: u64,
    pub role: String,
    pub sid: String,
}

impl AuthUser {
    pub fn require_admin(&self) -> Result<(), AppError> {
        if self.role == "admin" {
            Ok(())
        } else {
            Err(AppError::Forbidden("관리자 권한이 필요합니다".into()))
        }
    }
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or(AppError::Unauthorized)?;
        let token = header
            .strip_prefix("Bearer ")
            .or_else(|| header.strip_prefix("bearer "))
            .ok_or(AppError::Unauthorized)?;
        let claims =
            jwt::verify(&state.cfg.jwt_secret, token.trim()).ok_or(AppError::Unauthorized)?;
        if session::is_revoked(state, &claims.sid).await? {
            return Err(AppError::Unauthorized);
        }
        // 2단계 인증 필수: 등록 전에는 로그인 관련(/auth/*) 요청만
        let path = parts.uri.path();
        let auth_path = path.starts_with("/auth/") || path.starts_with("/api/auth/");
        if !auth_path && session::needs_2fa(state, claims.sub).await? {
            return Err(AppError::Forbidden(
                "2단계 인증을 먼저 등록하세요 (보안 → 2단계 인증)".into(),
            ));
        }
        Ok(AuthUser {
            id: claims.sub,
            role: claims.role,
            sid: claims.sid,
        })
    }
}

/// 브라우저 User-Agent (로그인 세션 목록에 기기 이름으로 보여 줌)
#[derive(Debug, Clone)]
pub struct UserAgent(pub String);

impl<S: Send + Sync> FromRequestParts<S> for UserAgent {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(UserAgent(
            parts
                .headers
                .get(axum::http::header::USER_AGENT)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .chars()
                .take(300)
                .collect(),
        ))
    }
}

/// 클라이언트 IP. 패널 앞단 Nginx(127.0.0.1)가 넣어준 X-Real-IP만 신뢰합니다.
#[derive(Debug, Clone)]
pub struct ClientIp(pub String);

impl<S: Send + Sync> FromRequestParts<S> for ClientIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let peer: Option<IpAddr> = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(a)| a.ip());
        let trusted_proxy = peer.map(|ip| ip.is_loopback()).unwrap_or(true);
        if trusted_proxy {
            if let Some(ip) = parts
                .headers
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.trim().parse::<IpAddr>().ok())
            {
                return Ok(ClientIp(ip.to_string()));
            }
        }
        Ok(ClientIp(
            peer.map(|p| p.to_string())
                .unwrap_or_else(|| "unknown".into()),
        ))
    }
}
