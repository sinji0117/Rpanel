//! 배포 로그 싱크.
//! 각 줄은 Redis 리스트(백로그)에 쌓이고 Pub/Sub 채널로 실시간 전파되며,
//! 배포 종료 시 전체 로그가 DB(deployments.log)에 저장됩니다.

use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};

use redis::{aio::ConnectionManager, AsyncCommands};
use serde::{Deserialize, Serialize};

/// 로그 1줄 최대 길이, DB 저장 최대 크기
const MAX_LINE: usize = 4000;
const MAX_TOTAL: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum LogEvent {
    Line { seq: u64, text: String },
    Status { status: String },
}

struct RedisTarget {
    conn: ConnectionManager,
    list_key: String,
    channel: String,
}

#[derive(Clone)]
pub struct LogSink {
    inner: Arc<Inner>,
    cancel: tokio::sync::watch::Receiver<bool>,
}

struct Inner {
    redis: Option<tokio::sync::Mutex<RedisTarget>>,
    seq: AtomicU64,
    buf: Mutex<String>,
    truncated: AtomicBool,
}

pub fn list_key(prefix: &str, deploy_id: u64) -> String {
    format!("{prefix}:deploylog:{deploy_id}")
}
pub fn channel(prefix: &str, deploy_id: u64) -> String {
    format!("{prefix}:deploychan:{deploy_id}")
}

impl LogSink {
    pub fn for_deployment(
        conn: ConnectionManager,
        prefix: &str,
        deploy_id: u64,
        cancel: tokio::sync::watch::Receiver<bool>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                redis: Some(tokio::sync::Mutex::new(RedisTarget {
                    conn,
                    list_key: list_key(prefix, deploy_id),
                    channel: channel(prefix, deploy_id),
                })),
                seq: AtomicU64::new(0),
                buf: Mutex::new(String::new()),
                truncated: AtomicBool::new(false),
            }),
            cancel,
        }
    }

    /// Redis 없이 메모리에만 모으는 싱크 (서비스 제어, 키 생성 등 단발 작업용)
    pub fn capture() -> Self {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        Self {
            inner: Arc::new(Inner {
                redis: None,
                seq: AtomicU64::new(0),
                buf: Mutex::new(String::new()),
                truncated: AtomicBool::new(false),
            }),
            cancel: rx,
        }
    }

    pub fn is_cancelled(&self) -> bool {
        *self.cancel.borrow()
    }

    pub fn cancel_receiver(&self) -> tokio::sync::watch::Receiver<bool> {
        self.cancel.clone()
    }

    /// 같은 로그에 쓰지만 취소 신호를 받지 않는 싱크 — 실패/취소 후 원상복구 작업용
    pub fn uncancellable(&self) -> Self {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        Self {
            inner: self.inner.clone(),
            cancel: rx,
        }
    }

    pub async fn line(&self, text: impl AsRef<str>) {
        let mut text: String = text.as_ref().chars().filter(|c| *c != '\r').collect();
        if text.len() > MAX_LINE {
            let mut cut = MAX_LINE;
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            text.truncate(cut);
            text.push_str(" …");
        }
        let seq = self.inner.seq.fetch_add(1, Ordering::SeqCst) + 1;
        {
            let mut buf = self.inner.buf.lock().unwrap();
            if buf.len() + text.len() < MAX_TOTAL {
                buf.push_str(&text);
                buf.push('\n');
            } else if !self.inner.truncated.swap(true, Ordering::SeqCst) {
                buf.push_str("… (로그가 너무 길어 이후 내용은 저장되지 않았습니다)\n");
            }
        }
        if let Some(r) = &self.inner.redis {
            let ev = serde_json::to_string(&LogEvent::Line { seq, text }).unwrap();
            let mut t = r.lock().await;
            let list_key = t.list_key.clone();
            let channel = t.channel.clone();
            let res: redis::RedisResult<()> = async {
                let _: () = t.conn.rpush(&list_key, &ev).await?;
                let _: () = t.conn.expire(&list_key, 86400).await?;
                let _: () = t.conn.publish(&channel, &ev).await?;
                Ok(())
            }
            .await;
            if let Err(e) = res {
                tracing::warn!(error = %e, "failed to push deploy log line");
            }
        }
    }

    pub async fn step(&self, title: &str) {
        self.line(format!("▶ {title}")).await;
    }

    pub async fn status(&self, status: &str) {
        if let Some(r) = &self.inner.redis {
            let ev = serde_json::to_string(&LogEvent::Status {
                status: status.to_string(),
            })
            .unwrap();
            let mut t = r.lock().await;
            let channel = t.channel.clone();
            let _: redis::RedisResult<()> = t.conn.publish(&channel, &ev).await;
        }
    }

    pub fn contents(&self) -> String {
        self.inner.buf.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn capture_collects_and_truncates_lines() {
        let s = LogSink::capture();
        s.line("hello").await;
        s.line("a\r").await;
        s.line("x".repeat(MAX_LINE + 50)).await;
        let c = s.contents();
        let lines: Vec<&str> = c.lines().collect();
        assert_eq!(lines[0], "hello");
        assert_eq!(lines[1], "a");
        assert!(lines[2].ends_with(" …"));
        assert!(lines[2].len() <= MAX_LINE + 4);
        assert!(!s.is_cancelled());
    }

    #[test]
    fn event_json_shape() {
        let j = serde_json::to_string(&LogEvent::Line {
            seq: 3,
            text: "hi".into(),
        })
        .unwrap();
        assert_eq!(j, r#"{"type":"line","seq":3,"text":"hi"}"#);
        let j = serde_json::to_string(&LogEvent::Status {
            status: "success".into(),
        })
        .unwrap();
        assert_eq!(j, r#"{"type":"status","status":"success"}"#);
    }
}
