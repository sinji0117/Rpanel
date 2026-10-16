use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct User {
    pub id: u64,
    pub email: String,
    pub password_hash: String,
    pub role: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct UserView {
    pub id: u64,
    pub email: String,
    pub role: String,
    pub created_at: DateTime<Utc>,
}

impl From<User> for UserView {
    fn from(u: User) -> Self {
        Self {
            id: u.id,
            email: u.email,
            role: u.role,
            created_at: u.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Project {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AppKind {
    Rust,
    Static,
    Proxy,
}

impl AppKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            AppKind::Rust => "rust",
            AppKind::Static => "static",
            AppKind::Proxy => "proxy",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "rust" => Some(Self::Rust),
            "static" => Some(Self::Static),
            "proxy" => Some(Self::Proxy),
            _ => None,
        }
    }
    /// systemd 서비스로 상주하는 앱인가 (포트 필요)
    pub fn is_service(&self) -> bool {
        matches!(self, AppKind::Rust)
    }
}

/// 앱 소스
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// Git 저장소에서 빌드
    Git,
    /// 사이트 유저 홈(/home/<유저>/htdocs/<도메인>)의 파일을 그대로 서비스
    Files,
    /// 파일 없음 (리버스 프록시)
    None,
}

impl Source {
    pub fn as_str(&self) -> &'static str {
        match self {
            Source::Git => "git",
            Source::Files => "files",
            Source::None => "none",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "git" => Some(Self::Git),
            "files" => Some(Self::Files),
            "none" => Some(Self::None),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct App {
    pub id: u64,
    pub project_id: Option<u64>,
    pub name: String,
    pub slug: String,
    pub kind: String,
    pub git_url: Option<String>,
    pub git_branch: String,
    pub root_dir: String,
    pub build_cmd: Option<String>,
    pub start_cmd: Option<String>,
    pub binary_name: Option<String>,
    pub output_dir: String,
    pub port: Option<u32>,
    pub api_proxy_port: Option<u32>,
    pub domains: String,
    pub health_path: String,
    pub status: String,
    pub current_commit: Option<String>,
    /// 현재 활성 릴리스(배포 id)
    pub current_release: Option<u64>,
    #[serde(skip_serializing)]
    pub webhook_token: String,
    /// 배정된 Redis DB 번호
    pub redis_db: Option<u32>,
    /// off | daily | weekly
    pub backup_schedule: String,
    pub backup_keep: u32,
    pub backup_last_scheduled: Option<DateTime<Utc>>,
    pub source: String,
    pub site_user: Option<String>,
    pub site_root: Option<String>,
    pub web_root: String,
    pub proxy_target: Option<String>,
    /// none | letsencrypt | custom
    pub ssl_mode: String,
    pub force_https: bool,
    pub client_max_body_mb: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl App {
    pub fn kind(&self) -> AppKind {
        AppKind::parse(&self.kind).unwrap_or(AppKind::Static)
    }
    pub fn source(&self) -> Source {
        Source::parse(&self.source).unwrap_or(Source::Git)
    }
    pub fn git_url(&self) -> &str {
        self.git_url.as_deref().unwrap_or("")
    }
    /// Nginx 가 서비스하는 디렉토리 (파일 기반 사이트)
    pub fn document_root(&self) -> Option<String> {
        let root = self.site_root.as_deref()?;
        Some(if self.web_root.is_empty() {
            root.to_string()
        } else {
            format!("{root}/{}", self.web_root)
        })
    }
    pub fn has_ssl(&self) -> bool {
        self.ssl_mode == "letsencrypt" || self.ssl_mode == "custom"
    }
    pub fn domain_list(&self) -> Vec<&str> {
        self.domains.split_whitespace().collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct EnvVar {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Deployment {
    pub id: u64,
    pub app_id: u64,
    pub status: String,
    pub triggered_by: Option<u64>,
    pub commit_sha: Option<String>,
    pub error: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct AuditLog {
    pub id: u64,
    pub user_id: Option<u64>,
    pub email: Option<String>,
    pub action: String,
    pub target: String,
    pub ip: String,
    pub created_at: DateTime<Utc>,
}

pub const APP_COLUMNS: &str = "id, project_id, name, slug, kind, git_url, git_branch, root_dir, build_cmd, start_cmd, \
     binary_name, output_dir, port, api_proxy_port, domains, health_path, status, current_commit, current_release, webhook_token, redis_db, backup_schedule, backup_keep, backup_last_scheduled, source, site_user, site_root, web_root, \
     proxy_target, ssl_mode, force_https, client_max_body_mb, \
     created_at, updated_at";

pub const DEPLOY_COLUMNS: &str =
    "id, app_id, status, triggered_by, commit_sha, error, started_at, finished_at, created_at";

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct AppDatabase {
    pub id: u64,
    pub app_id: u64,
    pub name: String,
    pub username: String,
    #[serde(skip_serializing)]
    pub password: String,
    pub env_key: String,
    pub created_at: DateTime<Utc>,
}

pub const DB_COLUMNS: &str = "id, app_id, name, username, password, env_key, created_at";

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Backup {
    pub id: u64,
    pub app_id: u64,
    pub kind: String,
    pub target: String,
    pub file: String,
    pub size_bytes: u64,
    pub status: String,
    pub error: Option<String>,
    pub trigger_by: String,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

pub const BACKUP_COLUMNS: &str =
    "id, app_id, kind, target, file, size_bytes, status, error, trigger_by, created_at, finished_at";

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct CronJob {
    pub id: u64,
    pub app_id: u64,
    pub name: String,
    pub schedule: String,
    pub command: String,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub const CRON_COLUMNS: &str =
    "id, app_id, name, schedule, command, enabled, created_at, updated_at";
