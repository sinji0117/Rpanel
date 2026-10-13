use crate::state::AppState;

/// 감사 로그 기록. 실패해도 요청 자체는 막지 않습니다.
pub async fn log(state: &AppState, user_id: Option<u64>, action: &str, target: &str, ip: &str) {
    let res =
        sqlx::query("INSERT INTO audit_logs (user_id, action, target, ip) VALUES (?, ?, ?, ?)")
            .bind(user_id)
            .bind(action)
            .bind(truncate(target, 255))
            .bind(truncate(ip, 64))
            .execute(&state.db)
            .await;
    if let Err(e) = res {
        tracing::warn!(error = %e, action, "audit log insert failed");
    }
}

fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}
