use std::{env, net::SocketAddr, path::PathBuf};

use anyhow::{bail, Context};

/// 실행 모드. `real`은 실제로 git/cargo/npm/sudo helper를 실행하고,
/// `dry-run`은 명령을 로그에만 남깁니다 (개발·테스트용).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecMode {
    Real,
    DryRun,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub redis_url: String,
    pub bind_addr: SocketAddr,
    pub jwt_secret: String,
    pub access_ttl_secs: i64,
    pub refresh_ttl_secs: i64,
    /// 리프레시 쿠키에 Secure 플래그. CT100(NPM)에서 HTTPS 종료 시 true.
    /// Tailscale로 http://10.0.0.12:9000 에 직접 접속한다면 false.
    pub cookie_secure: bool,
    pub exec_mode: ExecMode,
    /// 빌드 작업 디렉토리 루트 (/var/lib/rpanel)
    pub data_dir: PathBuf,
    /// 권한 작업용 helper (sudo -n 으로 실행)
    pub helper_path: PathBuf,
    /// 프론트 빌드 결과 (패널 자체 UI). 지정 시 백엔드가 직접 정적 서비스
    pub static_dir: Option<PathBuf>,
    pub app_port_min: u16,
    pub app_port_max: u16,
    pub redis_prefix: String,
    /// 앱 Nginx 설정의 set_real_ip_from (CT100 프록시 IP)
    pub trusted_proxy: String,
    /// 빌드 명령 실행 시 PATH
    pub build_path: String,
    /// 동시 빌드 수
    pub max_concurrent_builds: usize,
    /// 빌드 명령에 전달할 환경변수 이름 (기본: 없음). 예: HTTPS_PROXY,NO_PROXY,SSL_CERT_FILE
    pub pass_env: Vec<String>,
    /// 앱에 주입할 Redis 접속 기본 URL (DB 번호 제외). 기본: REDIS_URL 에서 /<n> 을 뗀 값
    pub app_redis_base: String,
    /// 앱에 자동 배정할 Redis DB 번호 범위 (Vol.6 수동 앱이 0~3 사용)
    pub redis_db_min: u32,
    pub redis_db_max: u32,
    /// 앱 DATABASE_URL 의 호스트:포트
    pub app_db_host: String,
    /// 예약 백업 실행 시각 (서버 로컬 시각, 0~23)
    pub backup_hour: u32,
}

fn var(name: &str) -> Option<String> {
    env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn parse_bool(v: &str) -> bool {
    matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let database_url = var("DATABASE_URL").context("DATABASE_URL is required")?;
        let redis_url = var("REDIS_URL").context("REDIS_URL is required")?;
        let jwt_secret = var("JWT_SECRET").context("JWT_SECRET is required")?;
        if jwt_secret.len() < 32 {
            bail!("JWT_SECRET must be at least 32 characters (use: openssl rand -hex 32)");
        }
        let bind_addr: SocketAddr = var("BIND_ADDR")
            .unwrap_or_else(|| "127.0.0.1:8090".into())
            .parse()
            .context("BIND_ADDR must be host:port")?;

        let exec_mode = match var("RPANEL_EXEC_MODE").as_deref() {
            Some("dry-run") | Some("dryrun") => ExecMode::DryRun,
            Some("real") | None => ExecMode::Real,
            Some(other) => bail!("RPANEL_EXEC_MODE must be 'real' or 'dry-run', got {other}"),
        };

        let app_port_min = var("RPANEL_APP_PORT_MIN")
            .map(|v| v.parse())
            .transpose()?
            .unwrap_or(8100);
        let app_port_max = var("RPANEL_APP_PORT_MAX")
            .map(|v| v.parse())
            .transpose()?
            .unwrap_or(8999);
        if app_port_min >= app_port_max {
            bail!("RPANEL_APP_PORT_MIN must be < RPANEL_APP_PORT_MAX");
        }

        Ok(Self {
            database_url,
            redis_url: redis_url.clone(),
            bind_addr,
            jwt_secret,
            access_ttl_secs: var("ACCESS_TOKEN_TTL_SECS").map(|v| v.parse()).transpose()?.unwrap_or(900),
            refresh_ttl_secs: var("REFRESH_TOKEN_TTL_SECS")
                .map(|v| v.parse())
                .transpose()?
                .unwrap_or(14 * 24 * 3600),
            cookie_secure: var("COOKIE_SECURE").map(|v| parse_bool(&v)).unwrap_or(false),
            exec_mode,
            data_dir: var("RPANEL_DATA_DIR").unwrap_or_else(|| "/var/lib/rpanel".into()).into(),
            helper_path: var("RPANEL_HELPER")
                .unwrap_or_else(|| "/usr/local/lib/rpanel/rpanel-helper".into())
                .into(),
            static_dir: var("RPANEL_STATIC_DIR").map(PathBuf::from),
            app_port_min,
            app_port_max,
            redis_prefix: var("REDIS_PREFIX").unwrap_or_else(|| "rpanel".into()),
            trusted_proxy: var("RPANEL_TRUSTED_PROXY").unwrap_or_else(|| "10.0.0.10".into()),
            build_path: var("RPANEL_BUILD_PATH").unwrap_or_else(|| {
                "/var/lib/rpanel/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".into()
            }),
            pass_env: var("RPANEL_PASS_ENV")
                .map(|v| {
                    v.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
            max_concurrent_builds: var("RPANEL_MAX_CONCURRENT_BUILDS").map(|v| v.parse()).transpose()?.unwrap_or(2),
            app_redis_base: var("RPANEL_APP_REDIS_BASE").unwrap_or_else(|| redis_base(&redis_url)),
            redis_db_min: var("RPANEL_REDIS_DB_MIN").map(|v| v.parse()).transpose()?.unwrap_or(4),
            redis_db_max: var("RPANEL_REDIS_DB_MAX").map(|v| v.parse()).transpose()?.unwrap_or(15),
            app_db_host: var("RPANEL_APP_DB_HOST").unwrap_or_else(|| "127.0.0.1:3306".into()),
            backup_hour: var("RPANEL_BACKUP_HOUR").map(|v| v.parse()).transpose()?.unwrap_or(3).min(23),
        })
    }

    /// 테스트용 기본값
    pub fn for_tests(database_url: String, redis_url: String) -> Self {
        Self {
            database_url,
            redis_url: redis_url.clone(),
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            jwt_secret: "test-secret-test-secret-test-secret-0123".into(),
            access_ttl_secs: 900,
            refresh_ttl_secs: 3600,
            cookie_secure: false,
            exec_mode: ExecMode::DryRun,
            data_dir: std::env::temp_dir().join(format!("rpanel-test-{}", std::process::id())),
            helper_path: "/bin/true".into(),
            static_dir: None,
            app_port_min: 8100,
            app_port_max: 8999,
            redis_prefix: format!("rpanel-test-{}-{}", std::process::id(), rand_suffix()),
            trusted_proxy: "10.0.0.10".into(),
            build_path: "/usr/bin:/bin".into(),
            max_concurrent_builds: 2,
            pass_env: vec![],
            app_redis_base: redis_base(&redis_url),
            redis_db_min: 4,
            redis_db_max: 15,
            app_db_host: "127.0.0.1:3306".into(),
            backup_hour: 3,
        }
    }
}

/// redis://:pw@host:6379/10 → redis://:pw@host:6379
pub fn redis_base(url: &str) -> String {
    let scheme_end = url.find("://").map(|i| i + 3).unwrap_or(0);
    match url[scheme_end..].rfind('/') {
        Some(i) => url[..scheme_end + i].to_string(),
        None => url.to_string(),
    }
}

/// REDIS_URL 의 DB 번호 (없으면 0)
pub fn redis_db_index(url: &str) -> u32 {
    let scheme_end = url.find("://").map(|i| i + 3).unwrap_or(0);
    url[scheme_end..]
        .rfind('/')
        .and_then(|i| url[scheme_end + i + 1..].parse().ok())
        .unwrap_or(0)
}

fn rand_suffix() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redis_url_parts() {
        assert_eq!(
            redis_base("redis://:pw@127.0.0.1:6379/10"),
            "redis://:pw@127.0.0.1:6379"
        );
        assert_eq!(
            redis_base("redis://127.0.0.1:6379"),
            "redis://127.0.0.1:6379"
        );
        assert_eq!(redis_db_index("redis://:pw@127.0.0.1:6379/10"), 10);
        assert_eq!(redis_db_index("redis://127.0.0.1:6379"), 0);
    }

    #[test]
    fn bool_parsing() {
        assert!(parse_bool("true"));
        assert!(parse_bool("1"));
        assert!(parse_bool("YES"));
        assert!(!parse_bool("false"));
        assert!(!parse_bool("0"));
    }
}
