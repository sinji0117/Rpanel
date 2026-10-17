//! 대시보드의 "서버 시스템 이벤트" 와 "프로세스" 목록
//!
//! - 이벤트: helper `system-events` 가 내주는 journald JSON 줄 + nginx error.log 를 합쳐
//!   `{ timestamp, level, source, message }` 형태로 정리합니다.
//! - 프로세스: sysinfo 로 CPU·메모리 상위 프로세스를 모읍니다.

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use serde::Serialize;
use serde_json::Value;

const MAX_MSG: usize = 2000;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Event {
    pub id: u64,
    pub timestamp: String,
    pub level: &'static str,
    pub source: &'static str,
    pub message: String,
    #[serde(skip)]
    pub ts_us: i64,
}

fn level_of(priority: u8) -> &'static str {
    match priority {
        0..=3 => "error",
        4 => "warn",
        _ => "info",
    }
}

fn source_of(unit: &str, ident: &str) -> &'static str {
    let u = if unit.is_empty() { ident } else { unit };
    if u.starts_with("rpanel") {
        "rpanel"
    } else if u.starts_with("nginx") {
        "nginx"
    } else if u.starts_with("mariadb") || u.starts_with("mysql") {
        "mariadb"
    } else if u.starts_with("redis") {
        "redis"
    } else if u.starts_with("cron") || u.starts_with("CRON") {
        "cron"
    } else {
        "system"
    }
}

fn clip(mut s: String) -> String {
    if s.len() > MAX_MSG {
        let mut end = MAX_MSG;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
        s.push('…');
    }
    s
}

/// journald 의 MESSAGE 는 글자열이거나(UTF-8 이 아니면) 바이트 배열입니다
fn message_of(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Array(a) => {
            let bytes: Vec<u8> = a
                .iter()
                .filter_map(|b| b.as_u64().map(|b| b as u8))
                .collect();
            Some(String::from_utf8_lossy(&bytes).into_owned())
        }
        _ => None,
    }
}

fn iso(ts_us: i64) -> String {
    DateTime::<Utc>::from_timestamp_micros(ts_us)
        .unwrap_or_default()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// `journalctl -o json` 한 줄 → 이벤트 (디버그 수준·빈 줄은 버림)
pub fn parse_journal_line(line: &str) -> Option<Event> {
    let v: Value = serde_json::from_str(line.trim()).ok()?;
    let priority: u8 = v
        .get("PRIORITY")
        .and_then(|p| p.as_str())
        .and_then(|p| p.parse().ok())
        .unwrap_or(6);
    if priority > 6 {
        return None;
    }
    let ts_us: i64 = v
        .get("__REALTIME_TIMESTAMP")
        .and_then(|t| t.as_str())
        .and_then(|t| t.parse().ok())?;
    let message = message_of(v.get("MESSAGE")?)?;
    let message = message.trim().to_string();
    if message.is_empty() {
        return None;
    }
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    Some(Event {
        id: 0,
        timestamp: iso(ts_us),
        level: level_of(priority),
        source: source_of(&s("_SYSTEMD_UNIT"), &s("SYSLOG_IDENTIFIER")),
        message: clip(message),
        ts_us,
    })
}

/// nginx error.log 한 줄: `2026/10/17 12:00:00 [error] 123#123: *1 메시지`
/// (시각은 서버 지역 시간)
pub fn parse_nginx_line(line: &str) -> Option<Event> {
    let line = line.trim();
    let dt = NaiveDateTime::parse_from_str(line.get(..19)?, "%Y/%m/%d %H:%M:%S").ok()?;
    let rest = line.get(19..)?.trim_start();
    let close = rest.find(']')?;
    let lvl = rest.get(1..close)?;
    let level = match lvl {
        "emerg" | "alert" | "crit" | "error" => "error",
        "warn" => "warn",
        "notice" | "info" => "info",
        _ => return None,
    };
    let mut msg = rest[close + 1..].trim_start();
    // "123#123: " 프로세스 번호는 떼어 냄
    if let Some(i) = msg.find(": ") {
        if msg[..i].chars().all(|c| c.is_ascii_digit() || c == '#') {
            msg = &msg[i + 2..];
        }
    }
    let ts = chrono::Local
        .from_local_datetime(&dt)
        .earliest()
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|| Utc.from_utc_datetime(&dt));
    let ts_us = ts.timestamp_micros();
    Some(Event {
        id: 0,
        timestamp: iso(ts_us),
        level,
        source: "nginx",
        message: clip(msg.to_string()),
        ts_us,
    })
}

/// 두 출처를 합쳐 시간순으로 정렬하고 마지막 `limit` 개만 남깁니다
pub fn merge(journal: &str, nginx: &str, limit: usize) -> Vec<Event> {
    let mut all: Vec<Event> = journal
        .lines()
        .filter_map(parse_journal_line)
        .chain(nginx.lines().filter_map(parse_nginx_line))
        .collect();
    all.sort_by_key(|e| e.ts_us);
    let skip = all.len().saturating_sub(limit);
    let mut out: Vec<Event> = all.into_iter().skip(skip).collect();
    for (i, e) in out.iter_mut().enumerate() {
        e.id = i as u64 + 1;
    }
    out
}

pub fn summary(events: &[Event]) -> Value {
    let n = |l: &str| events.iter().filter(|e| e.level == l).count();
    serde_json::json!({
        "total": events.len(),
        "errors": n("error"),
        "warnings": n("warn"),
        "info": n("info"),
    })
}

#[derive(Debug, Serialize)]
pub struct Proc {
    pub pid: u32,
    pub name: String,
    pub user: String,
    pub cpu: f32,
    pub mem_mb: f64,
    pub status: String,
    pub command: String,
}

/// CPU·메모리 상위 프로세스 (CPU 값을 얻으려고 잠깐 두 번 측정합니다)
pub fn top_processes(limit: usize) -> (Vec<Proc>, Value) {
    use sysinfo::{ProcessStatus, ProcessesToUpdate, System, Users};
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    std::thread::sleep(
        sysinfo::MINIMUM_CPU_UPDATE_INTERVAL.max(std::time::Duration::from_millis(250)),
    );
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let users = Users::new_with_refreshed_list();
    let (mut running, mut sleeping, mut zombie) = (0, 0, 0);
    let mut list: Vec<Proc> = sys
        .processes()
        .values()
        // 스레드는 빼고 프로세스만
        .filter(|p| p.thread_kind().is_none())
        .map(|p| {
            match p.status() {
                ProcessStatus::Run => running += 1,
                ProcessStatus::Zombie => zombie += 1,
                _ => sleeping += 1,
            }
            let cmd = p
                .cmd()
                .iter()
                .map(|s| s.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ");
            Proc {
                pid: p.pid().as_u32(),
                name: p.name().to_string_lossy().into_owned(),
                user: p
                    .user_id()
                    .and_then(|u| users.get_user_by_id(u))
                    .map(|u| u.name().to_string())
                    .unwrap_or_else(|| "-".into()),
                cpu: (p.cpu_usage() * 10.0).round() / 10.0,
                mem_mb: (p.memory() as f64 / 1024.0 / 1024.0 * 10.0).round() / 10.0,
                status: p.status().to_string().to_lowercase(),
                command: clip(if cmd.is_empty() {
                    p.name().to_string_lossy().into_owned()
                } else {
                    cmd
                }),
            }
        })
        .collect();
    let total = list.len();
    list.sort_by(|a, b| {
        b.cpu
            .partial_cmp(&a.cpu)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                b.mem_mb
                    .partial_cmp(&a.mem_mb)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });
    list.truncate(limit);
    (
        list,
        serde_json::json!({ "total": total, "running": running, "sleeping": sleeping, "zombie": zombie }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_lines() {
        let l = r#"{"__REALTIME_TIMESTAMP":"1760000000000000","PRIORITY":"3","_SYSTEMD_UNIT":"mariadb.service","MESSAGE":"Too many connections"}"#;
        let e = parse_journal_line(l).unwrap();
        assert_eq!(e.level, "error");
        assert_eq!(e.source, "mariadb");
        assert_eq!(e.message, "Too many connections");
        assert!(e.timestamp.starts_with("2025-10-09T"));

        // 바이트 배열 메시지, 커널(유닛 없음)
        let l = r#"{"__REALTIME_TIMESTAMP":"1","PRIORITY":"4","SYSLOG_IDENTIFIER":"kernel","MESSAGE":[104,105]}"#;
        let e = parse_journal_line(l).unwrap();
        assert_eq!(
            (e.level, e.source, e.message.as_str()),
            ("warn", "system", "hi")
        );

        // 디버그·깨진 줄은 버림
        assert!(
            parse_journal_line(r#"{"__REALTIME_TIMESTAMP":"1","PRIORITY":"7","MESSAGE":"x"}"#)
                .is_none()
        );
        assert!(parse_journal_line("not json").is_none());
        assert!(parse_journal_line(r#"{"PRIORITY":"6","MESSAGE":"no time"}"#).is_none());
    }

    #[test]
    fn nginx_lines() {
        let e = parse_nginx_line(
            "2026/10/17 12:00:00 [error] 812#812: *41 connect() failed (111: Connection refused) while connecting to upstream",
        )
        .unwrap();
        assert_eq!((e.level, e.source), ("error", "nginx"));
        assert!(e.message.starts_with("*41 connect() failed"));
        assert_eq!(
            parse_nginx_line("2026/10/17 12:00:00 [warn] 1#1: low")
                .unwrap()
                .level,
            "warn"
        );
        assert!(parse_nginx_line("garbage").is_none());
        assert!(parse_nginx_line("2026/10/17 12:00:00 [debug] 1#1: x").is_none());
    }

    #[test]
    fn merge_sorts_and_limits() {
        let j = [
            r#"{"__REALTIME_TIMESTAMP":"3000000","PRIORITY":"6","_SYSTEMD_UNIT":"rpanel.service","MESSAGE":"c"}"#,
            r#"{"__REALTIME_TIMESTAMP":"1000000","PRIORITY":"6","_SYSTEMD_UNIT":"cron.service","MESSAGE":"a"}"#,
            r#"{"__REALTIME_TIMESTAMP":"2000000","PRIORITY":"4","_SYSTEMD_UNIT":"redis-server.service","MESSAGE":"b"}"#,
        ]
        .join("\n");
        let out = merge(&j, "", 2);
        assert_eq!(
            out.iter().map(|e| e.message.as_str()).collect::<Vec<_>>(),
            ["b", "c"]
        );
        assert_eq!(out[0].id, 1);
        assert_eq!(out[0].source, "redis");
        let s = summary(&out);
        assert_eq!(s["warnings"], 1);
        assert_eq!(s["info"], 1);
    }

    #[test]
    fn long_messages_are_clipped() {
        let long = "가".repeat(MAX_MSG);
        let c = clip(long);
        assert!(c.len() <= MAX_MSG + '…'.len_utf8());
        assert!(c.ends_with('…'));
    }

    #[test]
    fn processes_list() {
        let (list, s) = top_processes(5);
        assert!(!list.is_empty() && list.len() <= 5);
        assert!(s["total"].as_u64().unwrap() >= list.len() as u64);
    }
}
