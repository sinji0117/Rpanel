//! MongoDB — 프로젝트용 데이터베이스·계정 관리 (패널의 관리자 연결로 직접 수행)
//!
//! 설치 스크립트가 MongoDB 를 127.0.0.1 에서만, 인증 필수로 띄우고 root 권한 관리자 계정을 만들어
//! `MONGODB_URL` 로 패널에 넘겨 줍니다. 패널은 이 연결로 데이터베이스마다 전용 계정을 만들고
//! (`readWrite` 또는 `read`, 그 DB 에만), 앱에 `MONGODB_URL` 로 자동 주입합니다.

use std::time::Duration;

use anyhow::{anyhow, Context};
use mongodb::bson::{doc, Document};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;

use crate::state::AppState;

/// 패널이 쓰거나 MongoDB 가 쓰는 이름 — 사용자 DB 로 만들 수 없음
const RESERVED: &[&str] = &["admin", "local", "config", "rpanel", "rpanel_logs", "test"];

static NAME: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-z][a-z0-9_]{1,62}$").unwrap());

pub fn valid_name(kind: &str, s: &str) -> Result<(), String> {
    if !NAME.is_match(s) {
        return Err(format!(
            "{kind}은(는) 소문자로 시작하는 소문자·숫자·_ 2~63자여야 합니다"
        ));
    }
    if RESERVED.contains(&s) {
        return Err(format!("'{s}' 는 쓸 수 없는 이름입니다"));
    }
    Ok(())
}

pub fn client(state: &AppState) -> anyhow::Result<&mongodb::Client> {
    state
        .mongo
        .as_ref()
        .ok_or_else(|| anyhow!("MongoDB 가 설치·설정되지 않았습니다 (MONGODB_URL 없음). 서버에서 sudo rpanel-update 로 설치하세요"))
}

/// 앱에 넘길 주소 — 계정의 인증 DB 는 그 DB 자신
pub fn app_url(state: &AppState, db: &str, user: &str, pw: &str) -> String {
    format!(
        "mongodb://{user}:{pw}@{}/{db}?authSource={db}",
        state.cfg.app_mongo_host
    )
}

#[derive(Debug, Serialize, Default)]
pub struct ServerInfo {
    pub available: bool,
    pub version: Option<String>,
    pub error: Option<String>,
    /// DB 이름 → 디스크 사용량
    #[serde(skip)]
    pub sizes: std::collections::HashMap<String, i64>,
}

/// 서버 상태와 DB 크기 (연결 안 되면 available=false)
pub async fn server_info(state: &AppState) -> ServerInfo {
    let Some(c) = state.mongo.as_ref() else {
        return ServerInfo {
            error: Some("MongoDB 가 설치·설정되지 않았습니다".into()),
            ..Default::default()
        };
    };
    let admin = c.database("admin");
    let res = tokio::time::timeout(
        Duration::from_secs(5),
        admin.run_command(doc! { "buildInfo": 1 }),
    )
    .await;
    match res {
        Ok(Ok(info)) => {
            let mut out = ServerInfo {
                available: true,
                version: info.get_str("version").ok().map(String::from),
                ..Default::default()
            };
            if let Ok(Ok(list)) = tokio::time::timeout(
                Duration::from_secs(5),
                admin.run_command(doc! { "listDatabases": 1 }),
            )
            .await
            {
                if let Ok(dbs) = list.get_array("databases") {
                    for d in dbs.iter().filter_map(|d| d.as_document()) {
                        if let (Ok(name), Some(size)) = (d.get_str("name"), size_of(d)) {
                            out.sizes.insert(name.to_string(), size);
                        }
                    }
                }
            }
            out
        }
        Ok(Err(e)) => ServerInfo {
            error: Some(format!("MongoDB 오류: {e}")),
            ..Default::default()
        },
        Err(_) => ServerInfo {
            error: Some(
                "MongoDB 에 접속하지 못했습니다 (시간 초과) — systemctl status mongod".into(),
            ),
            ..Default::default()
        },
    }
}

fn size_of(d: &Document) -> Option<i64> {
    match d.get("sizeOnDisk")? {
        mongodb::bson::Bson::Int64(v) => Some(*v),
        mongodb::bson::Bson::Int32(v) => Some(*v as i64),
        mongodb::bson::Bson::Double(v) => Some(*v as i64),
        _ => None,
    }
}

/// role: "readWrite" | "read"
pub async fn create_user(
    state: &AppState,
    db: &str,
    user: &str,
    pw: &str,
    role: &str,
) -> anyhow::Result<()> {
    let c = client(state)?;
    c.database(db)
        .run_command(doc! {
            "createUser": user,
            "pwd": pw,
            "roles": [ { "role": role, "db": db } ],
        })
        .await
        .with_context(|| format!("MongoDB 계정 {user} 만들기 실패"))?;
    Ok(())
}

pub async fn set_password(state: &AppState, db: &str, user: &str, pw: &str) -> anyhow::Result<()> {
    let c = client(state)?;
    c.database(db)
        .run_command(doc! { "updateUser": user, "pwd": pw })
        .await
        .with_context(|| format!("MongoDB 계정 {user} 비밀번호 변경 실패"))?;
    Ok(())
}

pub async fn drop_user(state: &AppState, db: &str, user: &str) -> anyhow::Result<()> {
    let c = client(state)?;
    match c.database(db).run_command(doc! { "dropUser": user }).await {
        Ok(_) => Ok(()),
        // 이미 없는 계정(UserNotFound = 11)은 성공으로 봄
        Err(e)
            if format!("{e}").contains("UserNotFound") || format!("{e}").contains("not found") =>
        {
            Ok(())
        }
        Err(e) => Err(anyhow!("MongoDB 계정 {user} 삭제 실패: {e}")),
    }
}

pub async fn drop_database(state: &AppState, db: &str) -> anyhow::Result<()> {
    let c = client(state)?;
    c.database(db)
        .drop()
        .await
        .with_context(|| format!("MongoDB 데이터베이스 {db} 삭제 실패"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(valid_name("DB 이름", "shop").is_ok());
        assert!(valid_name("DB 이름", "shop_logs2").is_ok());
        for bad in [
            "admin", "local", "config", "rpanel", "Shop", "1shop", "s", "shop-db", "shop.db",
            "a$b", "",
        ] {
            assert!(valid_name("DB 이름", bad).is_err(), "{bad}");
        }
        assert!(valid_name("DB 이름", &"a".repeat(64)).is_err());
    }
}
