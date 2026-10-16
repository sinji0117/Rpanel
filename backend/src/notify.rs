//! 알림 — 패널 안 알림 목록 + (선택) Discord · Slack · Telegram · 웹훅으로 보내기
//!
//! 이벤트마다 켜기/끄기를 설정에서 정합니다. 꺼진 이벤트는 기록도, 전송도 하지 않습니다.
//! 제목은 한국어 원문 템플릿과 params 로 저장해서 화면에서 번역합니다.

use std::collections::BTreeMap;

use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::state::AppState;

/// 이벤트 목록: (키, 한국어 이름, 기본값)
pub const EVENTS: &[(&str, &str, bool)] = &[
    ("deploy_failed", "배포·적용 실패", true),
    ("deploy_success", "배포·적용 성공", false),
    ("backup_failed", "백업 실패", true),
    ("backup_success", "백업 완료", false),
    ("site_down", "사이트 응답 없음 · 복구", true),
    ("disk_high", "디스크 사용량 높음", true),
    ("mem_high", "메모리 사용량 높음", true),
    ("update_available", "새 버전 알림", true),
    ("runtime_install", "런타임 설치 결과", true),
    ("admin_login", "관리자 로그인", false),
];

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Channels {
    #[serde(default)]
    pub discord_url: String,
    #[serde(default)]
    pub slack_url: String,
    #[serde(default)]
    pub telegram_token: String,
    #[serde(default)]
    pub telegram_chat_id: String,
    #[serde(default)]
    pub webhook_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    #[serde(default)]
    pub events: BTreeMap<String, bool>,
    #[serde(default = "default_threshold")]
    pub disk_threshold: u8,
    #[serde(default = "default_threshold")]
    pub mem_threshold: u8,
    #[serde(default)]
    pub channels: Channels,
}

fn default_threshold() -> u8 {
    90
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            events: BTreeMap::new(),
            disk_threshold: 90,
            mem_threshold: 90,
            channels: Channels::default(),
        }
    }
}

impl Settings {
    pub fn enabled(&self, kind: &str) -> bool {
        self.events.get(kind).copied().unwrap_or_else(|| {
            EVENTS
                .iter()
                .find(|e| e.0 == kind)
                .map(|e| e.2)
                .unwrap_or(false)
        })
    }

    /// 모든 이벤트 키를 채운 형태 (화면 표시용)
    pub fn normalized(mut self) -> Self {
        for (k, _, d) in EVENTS {
            self.events.entry((*k).to_string()).or_insert(*d);
        }
        self.events.retain(|k, _| EVENTS.iter().any(|e| e.0 == k));
        self
    }

    pub fn validate(&self) -> Result<(), String> {
        static TG_TOKEN: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"^[0-9]{5,15}:[A-Za-z0-9_-]{20,64}$").unwrap());
        static TG_CHAT: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"^(-?[0-9]{3,20}|@[A-Za-z0-9_]{4,64})$").unwrap());
        let c = &self.channels;
        if !(50..=99).contains(&self.disk_threshold) || !(50..=99).contains(&self.mem_threshold) {
            return Err("기준 사용량은 50 ~ 99% 사이로 정하세요".into());
        }
        if !c.discord_url.is_empty()
            && !(c
                .discord_url
                .starts_with("https://discord.com/api/webhooks/")
                || c.discord_url
                    .starts_with("https://discordapp.com/api/webhooks/"))
        {
            return Err(
                "Discord 웹훅 주소는 https://discord.com/api/webhooks/ 로 시작합니다".into(),
            );
        }
        if !c.slack_url.is_empty() && !c.slack_url.starts_with("https://hooks.slack.com/") {
            return Err("Slack 웹훅 주소는 https://hooks.slack.com/ 로 시작합니다".into());
        }
        if !c.webhook_url.is_empty()
            && !(c.webhook_url.starts_with("https://") || c.webhook_url.starts_with("http://"))
        {
            return Err("웹훅 주소는 http:// 또는 https:// 로 시작해야 합니다".into());
        }
        for u in [&c.discord_url, &c.slack_url, &c.webhook_url] {
            if u.len() > 500
                || u.chars()
                    .any(|ch| ch.is_whitespace() || ch.is_control() || ch == '"' || ch == '\\')
            {
                return Err("웹훅 주소 형식이 올바르지 않습니다".into());
            }
        }
        match (c.telegram_token.is_empty(), c.telegram_chat_id.is_empty()) {
            (true, true) => {}
            (false, false) => {
                if !TG_TOKEN.is_match(&c.telegram_token) {
                    return Err(
                        "Telegram 봇 토큰 형식이 올바르지 않습니다 (예: 123456789:AA…)".into(),
                    );
                }
                if !TG_CHAT.is_match(&c.telegram_chat_id) {
                    return Err(
                        "Telegram chat id 형식이 올바르지 않습니다 (숫자 또는 @채널이름)".into(),
                    );
                }
            }
            _ => return Err("Telegram 은 봇 토큰과 chat id 를 함께 넣어야 합니다".into()),
        }
        Ok(())
    }
}

const SETTINGS_KEY: &str = "notifications";

pub async fn load_settings(state: &AppState) -> Settings {
    let v: Option<String> = sqlx::query_scalar("SELECT v FROM panel_settings WHERE k = ?")
        .bind(SETTINGS_KEY)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();
    v.and_then(|s| serde_json::from_str::<Settings>(&s).ok())
        .unwrap_or_default()
        .normalized()
}

pub async fn save_settings(state: &AppState, s: &Settings) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO panel_settings (k, v) VALUES (?, ?) ON DUPLICATE KEY UPDATE v = VALUES(v)",
    )
    .bind(SETTINGS_KEY)
    .bind(serde_json::to_string(s)?)
    .execute(&state.db)
    .await?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Event {
    pub kind: &'static str,
    /// info | success | warning | error
    pub level: &'static str,
    /// 한국어 원문 템플릿 (예: "배포 실패: {app}")
    pub title: &'static str,
    pub params: Value,
    pub body: Option<String>,
    pub link: Option<String>,
}

impl Event {
    pub fn new(
        kind: &'static str,
        level: &'static str,
        title: &'static str,
        params: Value,
    ) -> Self {
        Self {
            kind,
            level,
            title,
            params,
            body: None,
            link: None,
        }
    }
    pub fn body(mut self, b: impl Into<String>) -> Self {
        let b: String = b.into();
        self.body = Some(b.chars().take(2000).collect());
        self
    }
    pub fn link(mut self, l: impl Into<String>) -> Self {
        self.link = Some(l.into());
        self
    }

    /// "{app}" 자리표시를 채운 한국어 문장 (외부 전송용)
    pub fn rendered_title(&self) -> String {
        render(self.title, &self.params)
    }
}

pub fn render(template: &str, params: &Value) -> String {
    let mut s = template.to_string();
    if let Some(obj) = params.as_object() {
        for (k, v) in obj {
            let val = match v {
                Value::String(x) => x.clone(),
                other => other.to_string(),
            };
            s = s.replace(&format!("{{{k}}}"), &val);
        }
    }
    s
}

/// 알림 남기기 (꺼진 이벤트면 아무것도 안 함). 외부 채널 전송은 백그라운드로.
pub async fn emit(state: &AppState, ev: Event) {
    let settings = load_settings(state).await;
    if !settings.enabled(ev.kind) {
        return;
    }
    let res = crate::logstore::notify_insert(
        state,
        crate::logstore::NewNotification {
            kind: ev.kind,
            level: ev.level,
            title: ev.title,
            params: &ev.params,
            body: ev.body.as_deref(),
            link: ev.link.as_deref(),
        },
    )
    .await;
    if let Err(e) = res {
        tracing::warn!(error = %e, "notification not stored");
        return;
    }
    if has_channel(&settings.channels) {
        let channels = settings.channels.clone();
        let host = crate::metrics::host_info()
            .hostname
            .unwrap_or_else(|| "rpanel".into());
        tokio::spawn(async move {
            for (name, r) in send_all(&channels, &host, &ev).await {
                if let Err(e) = r {
                    tracing::warn!(channel = name, error = %e, "notification send failed");
                }
            }
        });
    }
}

pub fn has_channel(c: &Channels) -> bool {
    !c.discord_url.is_empty()
        || !c.slack_url.is_empty()
        || !c.telegram_token.is_empty()
        || !c.webhook_url.is_empty()
}

fn icon(level: &str) -> &'static str {
    match level {
        "error" => "🔴",
        "warning" => "🟠",
        "success" => "🟢",
        _ => "🔵",
    }
}

/// 설정된 모든 채널로 보내고 결과를 돌려줌
pub async fn send_all(
    c: &Channels,
    host: &str,
    ev: &Event,
) -> Vec<(&'static str, anyhow::Result<()>)> {
    let title = ev.rendered_title();
    let text = match &ev.body {
        Some(b) if !b.is_empty() => format!("{} [{host}] {title}\n{b}", icon(ev.level)),
        _ => format!("{} [{host}] {title}", icon(ev.level)),
    };
    let mut out = Vec::new();
    if !c.discord_url.is_empty() {
        let content: String = text.chars().take(1900).collect();
        out.push((
            "discord",
            post_json(&c.discord_url, &json!({ "content": content })).await,
        ));
    }
    if !c.slack_url.is_empty() {
        out.push((
            "slack",
            post_json(&c.slack_url, &json!({ "text": text })).await,
        ));
    }
    if !c.telegram_token.is_empty() {
        let url = format!(
            "https://api.telegram.org/bot{}/sendMessage",
            c.telegram_token
        );
        let msg: String = text.chars().take(4000).collect();
        out.push((
            "telegram",
            post_json(&url, &json!({ "chat_id": c.telegram_chat_id, "text": msg, "disable_web_page_preview": true })).await,
        ));
    }
    if !c.webhook_url.is_empty() {
        let payload = json!({
            "source": "rpanel",
            "host": host,
            "kind": ev.kind,
            "level": ev.level,
            "title": title,
            "body": ev.body,
            "link": ev.link,
            "at": chrono::Utc::now().to_rfc3339(),
        });
        out.push(("webhook", post_json(&c.webhook_url, &payload).await));
    }
    out
}

/// curl 로 JSON POST — 주소(비밀값 포함)는 명령줄 대신 stdin 설정으로 넘김
async fn post_json(url: &str, body: &Value) -> anyhow::Result<()> {
    use tokio::io::AsyncWriteExt;
    let mut child = tokio::process::Command::new("curl")
        .args([
            "-fsS",
            "--max-time",
            "10",
            "-o",
            "/dev/null",
            "-H",
            "Content-Type: application/json",
            "--data-binary",
            &body.to_string(),
            "-K",
            "-",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(format!("url = \"{url}\"\n").as_bytes())
            .await?;
    }
    let out = child.wait_with_output().await?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        // 토큰이 오류 메시지에 섞여 나오지 않도록
        anyhow::bail!("{}", redact(&err));
    }
    Ok(())
}

fn redact(s: &str) -> String {
    static TOKEN: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"(bot[0-9]+:)[A-Za-z0-9_-]+|(webhooks/[0-9]+/)[A-Za-z0-9_-]+").unwrap()
    });
    TOKEN.replace_all(s, "$1$2***").into_owned()
}

/// 같은 알림이 너무 자주 가지 않게 (Redis SET NX EX). 보내도 되면 true
pub async fn cooldown(state: &AppState, key: &str, secs: u64) -> bool {
    let mut r = state.redis.clone();
    let k = state.key(&["notify", "cooldown", key]);
    redis::cmd("SET")
        .arg(&k)
        .arg("1")
        .arg("NX")
        .arg("EX")
        .arg(secs)
        .query_async::<Option<String>>(&mut r)
        .await
        .ok()
        .flatten()
        .is_some()
}

/// 1분마다 받은 지표로 디스크·메모리 기준 초과 확인 (같은 항목은 6시간에 한 번)
pub async fn check_resources(state: &AppState, s: &crate::metrics::Snapshot) {
    let settings = load_settings(state).await;
    if settings.enabled("disk_high") {
        for d in &s.disks {
            if d.total == 0 {
                continue;
            }
            let pct = d.used as f64 * 100.0 / d.total as f64;
            if pct >= settings.disk_threshold as f64
                && cooldown(state, &format!("disk:{}", d.mount), 6 * 3600).await
            {
                emit(
                    state,
                    Event::new(
                        "disk_high",
                        "warning",
                        "디스크 사용량 {pct}% ({mount})",
                        json!({ "pct": pct.round() as u64, "mount": d.mount }),
                    )
                    .link("/"),
                )
                .await;
            }
        }
    }
    if settings.enabled("mem_high") && s.mem_total > 0 {
        let pct = s.mem_used as f64 * 100.0 / s.mem_total as f64;
        if pct >= settings.mem_threshold as f64 && cooldown(state, "mem", 3600).await {
            emit(
                state,
                Event::new(
                    "mem_high",
                    "warning",
                    "메모리 사용량 {pct}%",
                    json!({ "pct": pct.round() as u64 }),
                )
                .link("/"),
            )
            .await;
        }
    }
}

/// 5분마다 사이트 응답 확인 → 응답 없음 / 복구 알림 (상태가 바뀔 때만)
pub fn spawn_site_monitor(state: AppState) {
    if state.exec.is_dry_run() {
        return;
    }
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(300));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        tokio::time::sleep(std::time::Duration::from_secs(90)).await;
        loop {
            tick.tick().await;
            if let Err(e) = site_tick(&state).await {
                tracing::debug!(error = %e, "site monitor tick failed");
            }
        }
    });
}

async fn site_tick(state: &AppState) -> anyhow::Result<()> {
    if !load_settings(state).await.enabled("site_down") {
        return Ok(());
    }
    let rows: Vec<(u64, String, String, String)> = sqlx::query_as(
        "SELECT id, name, domains, status FROM apps WHERE domains <> '' AND status <> 'idle'",
    )
    .fetch_all(&state.db)
    .await?;
    let mut r = state.redis.clone();
    for (id, name, domains, _status) in rows {
        let Some(domain) = domains.split_whitespace().next() else {
            continue;
        };
        let code = crate::deploy::runner::http_status_host(80, domain, "/")
            .await
            .ok();
        let up = matches!(code, Some(c) if c < 500);
        let key = state.key(&["notify", "site", domain]);
        let prev: Option<String> = redis::cmd("GET")
            .arg(&key)
            .query_async(&mut r)
            .await
            .ok()
            .flatten();
        let now = if up { "up" } else { "down" };
        let _: Result<(), _> = redis::cmd("SET")
            .arg(&key)
            .arg(now)
            .arg("EX")
            .arg(7 * 24 * 3600)
            .query_async(&mut r)
            .await;
        let what = code
            .map(|c| format!("HTTP {c}"))
            .unwrap_or_else(|| "응답 없음".into());
        match (prev.as_deref(), up) {
            (Some("up") | None, false) => {
                emit(
                    state,
                    Event::new(
                        "site_down",
                        "error",
                        "사이트 응답 없음: {domain}",
                        json!({ "domain": domain }),
                    )
                    .body(format!("{name} — {what}"))
                    .link(format!("/apps/{id}")),
                )
                .await
            }
            (Some("down"), true) => {
                emit(
                    state,
                    Event::new(
                        "site_down",
                        "success",
                        "사이트 복구: {domain}",
                        json!({ "domain": domain }),
                    )
                    .body(format!("{name} — {what}"))
                    .link(format!("/apps/{id}")),
                )
                .await
            }
            _ => {}
        }
    }
    Ok(())
}

/// 6시간마다 GitHub 새 버전 확인 → 버전마다 한 번 알림
pub fn spawn_update_watch(state: AppState) {
    if state.cfg.update_repo.trim().is_empty() {
        return;
    }
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(120)).await;
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(6 * 3600));
        loop {
            tick.tick().await;
            if let Ok(latest) = crate::routes::system::fetch_latest(&state.cfg.update_repo).await {
                let current = env!("CARGO_PKG_VERSION");
                if crate::routes::system::is_newer(&latest, current)
                    && cooldown(&state, &format!("update:{latest}"), 365 * 24 * 3600).await
                {
                    emit(
                        &state,
                        Event::new(
                            "update_available",
                            "info",
                            "새 버전 v{latest} 이(가) 나왔습니다 (지금 v{current})",
                            json!({ "latest": latest, "current": current }),
                        )
                        .body("서버에서 sudo rpanel-update 를 실행하세요")
                        .link("/"),
                    )
                    .await;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_enabled() {
        let s = Settings::default().normalized();
        assert_eq!(s.events.len(), EVENTS.len());
        assert!(s.enabled("deploy_failed"));
        assert!(!s.enabled("deploy_success"));
        assert!(!s.enabled("unknown"));
        let mut s2 = s.clone();
        s2.events.insert("deploy_failed".into(), false);
        s2.events.insert("bogus".into(), true);
        let s2 = s2.normalized();
        assert!(!s2.enabled("deploy_failed"));
        assert!(!s2.events.contains_key("bogus"));
    }

    #[test]
    fn validation() {
        let ok = |f: &dyn Fn(&mut Settings)| {
            let mut s = Settings::default();
            f(&mut s);
            s.validate()
        };
        assert!(ok(&|_| {}).is_ok());
        assert!(
            ok(&|s| s.channels.discord_url = "https://discord.com/api/webhooks/1/abc".into())
                .is_ok()
        );
        assert!(
            ok(&|s| s.channels.discord_url = "https://evil.example/api/webhooks/1".into()).is_err()
        );
        assert!(
            ok(&|s| s.channels.slack_url = "https://hooks.slack.com/services/T/B/x".into()).is_ok()
        );
        assert!(ok(&|s| s.channels.webhook_url = "ftp://x".into()).is_err());
        assert!(
            ok(&|s| s.channels.webhook_url = "https://x.example/hook\" -o /etc/x".into()).is_err()
        );
        assert!(
            ok(&|s| s.channels.telegram_token =
                "123456789:AAbbccddeeffgghhiijjkkllmmnnoopp".into())
            .is_err(),
            "chat id missing"
        );
        assert!(ok(&|s| {
            s.channels.telegram_token = "123456789:AAbbccddeeffgghhiijjkkllmmnnoopp".into();
            s.channels.telegram_chat_id = "-1001234567890".into();
        })
        .is_ok());
        assert!(ok(&|s| s.disk_threshold = 20).is_err());
    }

    #[test]
    fn render_and_redact() {
        let e = Event::new(
            "deploy_failed",
            "error",
            "배포 실패: {app} #{id}",
            json!({ "app": "shop", "id": 7 }),
        );
        assert_eq!(e.rendered_title(), "배포 실패: shop #7");
        assert_eq!(
            redact("curl: (22) https://api.telegram.org/bot123:SECRET/sendMessage 404"),
            "curl: (22) https://api.telegram.org/bot123:***/sendMessage 404"
        );
        assert!(!redact("https://discord.com/api/webhooks/123/TOKENVALUE").contains("TOKENVALUE"));
    }
}
