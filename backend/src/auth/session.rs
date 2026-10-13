//! Redis 기반 리프레시 세션.
//!
//! - 리프레시 토큰은 32바이트 난수. Redis에는 SHA-256 해시만 저장합니다.
//! - 토큰은 "패밀리"(=로그인 세션) 단위로 묶이며, 사용할 때마다 회전(rotation)됩니다.
//! - 이미 사용된 토큰이 다시 들어오면(탈취 의심) 패밀리 전체를 폐기합니다.
//! - 액세스 JWT의 `sid`에 패밀리 id가 들어가므로, 로그아웃/폐기 즉시 액세스 토큰도 거부됩니다.

use rand::RngCore;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{error::AppResult, state::AppState};

#[derive(Debug, Serialize, Deserialize)]
struct RefreshRecord {
    uid: u64,
    fam: String,
}

pub fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

fn digest(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

pub struct Issued {
    pub refresh_token: String,
    pub family: String,
}

/// 새 로그인 세션 생성
pub async fn create(state: &AppState, uid: u64) -> AppResult<Issued> {
    let fam = random_token(16);
    let token = issue_in_family(state, uid, &fam).await?;
    let mut r = state.redis.clone();
    let ttl = state.cfg.refresh_ttl_secs;
    let ufam = state.key(&["ufam", &uid.to_string()]);
    let _: () = r.sadd(&ufam, &fam).await?;
    let _: () = r.expire(&ufam, ttl).await?;
    Ok(Issued {
        refresh_token: token,
        family: fam,
    })
}

async fn issue_in_family(state: &AppState, uid: u64, fam: &str) -> AppResult<String> {
    let token = random_token(32);
    let h = digest(&token);
    let ttl = state.cfg.refresh_ttl_secs as u64;
    let mut r = state.redis.clone();
    let rec = serde_json::to_string(&RefreshRecord {
        uid,
        fam: fam.to_string(),
    })
    .unwrap();
    let _: () = r.set_ex(state.key(&["rt", &h]), rec, ttl).await?;
    let famkey = state.key(&["rtfam", fam]);
    let _: () = r.sadd(&famkey, &h).await?;
    let _: () = r.expire(&famkey, ttl as i64).await?;
    Ok(token)
}

pub enum RotateResult {
    Ok { uid: u64, issued: Issued },
    Invalid,
    ReuseDetected,
}

/// 리프레시 토큰 회전
pub async fn rotate(state: &AppState, token: &str) -> AppResult<RotateResult> {
    let h = digest(token);
    let mut r = state.redis.clone();
    // GETDEL: 원자적으로 꺼내고 삭제 → 동시에 두 번 써도 하나만 성공
    let raw: Option<String> = redis::cmd("GETDEL")
        .arg(state.key(&["rt", &h]))
        .query_async(&mut r)
        .await?;
    let Some(raw) = raw else {
        let used: Option<String> = r.get(state.key(&["rtused", &h])).await?;
        if let Some(fam) = used {
            tracing::warn!(family = %fam, "refresh token reuse detected — revoking session family");
            revoke_family(state, &fam).await?;
            return Ok(RotateResult::ReuseDetected);
        }
        return Ok(RotateResult::Invalid);
    };
    let rec: RefreshRecord = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return Ok(RotateResult::Invalid),
    };
    if is_revoked(state, &rec.fam).await? {
        return Ok(RotateResult::Invalid);
    }
    let ttl = state.cfg.refresh_ttl_secs as u64;
    let _: () = r.set_ex(state.key(&["rtused", &h]), &rec.fam, ttl).await?;
    let _: () = r.srem(state.key(&["rtfam", &rec.fam]), &h).await?;
    let token = issue_in_family(state, rec.uid, &rec.fam).await?;
    Ok(RotateResult::Ok {
        uid: rec.uid,
        issued: Issued {
            refresh_token: token,
            family: rec.fam,
        },
    })
}

/// 패밀리(세션) 전체 폐기
pub async fn revoke_family(state: &AppState, fam: &str) -> AppResult<()> {
    let mut r = state.redis.clone();
    let famkey = state.key(&["rtfam", fam]);
    let members: Vec<String> = r.smembers(&famkey).await?;
    for h in members {
        let _: () = r.del(state.key(&["rt", &h])).await?;
    }
    let _: () = r.del(&famkey).await?;
    // 액세스 토큰 최대 수명 동안 폐기 표시 유지
    let _: () = r
        .set_ex(
            state.key(&["revoked", fam]),
            1,
            (state.cfg.access_ttl_secs + 60) as u64,
        )
        .await?;
    Ok(())
}

/// 리프레시 토큰으로 해당 세션 폐기 (로그아웃)
pub async fn revoke_by_token(state: &AppState, token: &str) -> AppResult<()> {
    let h = digest(token);
    let mut r = state.redis.clone();
    let raw: Option<String> = r.get(state.key(&["rt", &h])).await?;
    if let Some(rec) = raw.and_then(|s| serde_json::from_str::<RefreshRecord>(&s).ok()) {
        revoke_family(state, &rec.fam).await?;
    }
    Ok(())
}

/// 사용자의 모든 세션 폐기 (비밀번호 변경 시)
pub async fn revoke_all_for_user(state: &AppState, uid: u64) -> AppResult<()> {
    let mut r = state.redis.clone();
    let ufam = state.key(&["ufam", &uid.to_string()]);
    let fams: Vec<String> = r.smembers(&ufam).await?;
    for f in fams {
        revoke_family(state, &f).await?;
    }
    let _: () = r.del(&ufam).await?;
    Ok(())
}

pub async fn is_revoked(state: &AppState, fam: &str) -> AppResult<bool> {
    let mut r = state.redis.clone();
    Ok(r.exists(state.key(&["revoked", fam])).await?)
}

/// 고정 윈도 rate limit. 허용되면 true.
pub async fn rate_limit(
    state: &AppState,
    bucket: &str,
    limit: u64,
    window_secs: i64,
) -> AppResult<bool> {
    let mut r = state.redis.clone();
    let key = state.key(&["rl", bucket]);
    let n: u64 = r.incr(&key, 1).await?;
    if n == 1 {
        let _: () = r.expire(&key, window_secs).await?;
    }
    Ok(n <= limit)
}

pub async fn reset_rate_limit(state: &AppState, bucket: &str) -> AppResult<()> {
    let mut r = state.redis.clone();
    let _: () = r.del(state.key(&["rl", bucket])).await?;
    Ok(())
}

/// WebSocket용 1회성 티켓 (브라우저 WebSocket은 Authorization 헤더를 못 붙이므로)
pub async fn create_ws_ticket(state: &AppState, uid: u64, sid: &str) -> AppResult<String> {
    let t = random_token(24);
    let mut r = state.redis.clone();
    let _: () = r
        .set_ex(state.key(&["wst", &t]), format!("{uid}:{sid}"), 30)
        .await?;
    Ok(t)
}

pub async fn consume_ws_ticket(state: &AppState, ticket: &str) -> AppResult<Option<u64>> {
    if ticket.len() != 48 || !ticket.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(None);
    }
    let mut r = state.redis.clone();
    let v: Option<String> = redis::cmd("GETDEL")
        .arg(state.key(&["wst", ticket]))
        .query_async(&mut r)
        .await?;
    let Some(v) = v else { return Ok(None) };
    let Some((uid, sid)) = v.split_once(':') else {
        return Ok(None);
    };
    if is_revoked(state, sid).await? {
        return Ok(None);
    }
    Ok(uid.parse().ok())
}
