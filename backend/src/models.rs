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
    Node,
}

impl AppKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            AppKind::Rust => "rust",
            AppKind::Static => "static",
            AppKind::Node => "node",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "rust" => Some(Self::Rust),
            "static" => Some(Self::Static),
            "node" => Some(Self::Node),
            _ => None,
        }
    }
    /// systemd 서비스로 상주하는 앱인가 (포트 필요)
    pub fn is_service(&self) -> bool {
        !matches!(self, AppKind::Static)
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct App {
    pub id: u64,
    pub project_id: u64,
    pub name: String,
    pub slug: String,
    pub kind: String,
    pub git_url: String,
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
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl App {
    pub fn kind(&self) -> AppKind {
        AppKind::parse(&self.kind).unwrap_or(AppKind::Static)
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
     binary_name, output_dir, port, api_proxy_port, domains, health_path, status, current_commit, current_release, webhook_token, created_at, updated_at";

pub const DEPLOY_COLUMNS: &str =
    "id, app_id, status, triggered_by, commit_sha, error, started_at, finished_at, created_at";
