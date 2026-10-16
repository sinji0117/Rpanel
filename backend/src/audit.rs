use crate::state::AppState;

/// 감사 로그 기록. 실패해도 요청 자체는 막지 않습니다.
pub async fn log(state: &AppState, user_id: Option<u64>, action: &str, target: &str, ip: &str) {
    let res = crate::logstore::audit_write(
        state,
        user_id,
        action,
        &truncate(target, 255),
        &truncate(ip, 64),
    )
    .await;
    if let Err(e) = res {
        tracing::warn!(error = %e, action, "audit log insert failed");
    }
}

fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}
