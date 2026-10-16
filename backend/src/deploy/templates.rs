//! systemd 유닛 / Nginx 사이트 / env 파일 템플릿.
//! 모든 입력값은 `validate`를 통과한 값이라는 전제지만, 한 번 더 방어적으로 이스케이프합니다.

use std::path::{Path, PathBuf};

use crate::models::{App, AppKind, CronJob, EnvVar, Source};

pub const MANAGED_MARKER: &str = "# managed by rpanel";

/// 앱별 경로 규칙
#[derive(Debug, Clone)]
pub struct AppPaths {
    pub build_dir: PathBuf,
    pub repo_dir: PathBuf,
    pub app_dir: PathBuf,
    pub releases_dir: PathBuf,
    pub current: PathBuf,
    pub env_file: PathBuf,
    pub data_dir: PathBuf,
    pub www_dir: PathBuf,
    pub www_releases: PathBuf,
    pub www_current: PathBuf,
    pub staging_unit: PathBuf,
    pub staging_site: PathBuf,
    pub key_file: PathBuf,
    pub known_hosts: PathBuf,
}

impl AppPaths {
    pub fn new(data_dir: &Path, slug: &str) -> Self {
        let build_dir = data_dir.join("builds").join(slug);
        let app_dir = data_dir.join("apps").join(slug);
        let www_dir = data_dir.join("www").join(slug);
        Self {
            repo_dir: build_dir.join("repo"),
            build_dir,
            releases_dir: app_dir.join("releases"),
            current: app_dir.join("current"),
            env_file: app_dir.join("app.env"),
            data_dir: app_dir.join("data"),
            app_dir,
            www_releases: www_dir.join("releases"),
            www_current: www_dir.join("current"),
            www_dir,
            staging_unit: data_dir.join("staging").join(format!("{slug}.service")),
            staging_site: data_dir.join("staging").join(format!("{slug}.conf")),
            key_file: data_dir.join("keys").join(slug),
            known_hosts: data_dir.join("keys").join("known_hosts"),
        }
    }

    /// 빌드가 실행될 디렉토리 (repo + root_dir)
    pub fn work_dir(&self, root_dir: &str) -> PathBuf {
        if root_dir.is_empty() {
            self.repo_dir.clone()
        } else {
            self.repo_dir.join(root_dir)
        }
    }
}

pub fn unit_name(slug: &str) -> String {
    format!("rpanel-app-{slug}.service")
}

/// systemd ExecStart 인자 인용: "..." 안에서 \\, \", $, % 이스케이프
pub fn systemd_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '$' => out.push_str("$$"),
            '%' => out.push_str("%%"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// systemd EnvironmentFile 형식. 값은 큰따옴표로 감싸고 \\ " $ ` 이스케이프.
pub fn env_file(vars: &[EnvVar], port: Option<u32>) -> String {
    let mut out = String::from("# managed by rpanel — 패널의 Environment 탭에서 수정하세요\n");
    let mut has_port = false;
    let mut has_bind = false;
    for v in vars {
        has_port |= v.key == "PORT";
        has_bind |= v.key == "BIND_ADDR";
        out.push_str(&v.key);
        out.push_str("=\"");
        for c in v.value.chars() {
            match c {
                '\\' => out.push_str("\\\\"),
                '"' => out.push_str("\\\""),
                '$' => out.push_str("\\$"),
                '`' => out.push_str("\\`"),
                '\n' | '\r' => {}
                c => out.push(c),
            }
        }
        out.push_str("\"\n");
    }
    if let Some(p) = port {
        if !has_port {
            out.push_str(&format!("PORT=\"{p}\"\n"));
        }
        if !has_bind {
            out.push_str(&format!("BIND_ADDR=\"127.0.0.1:{p}\"\n"));
        }
    }
    out
}

pub struct UnitInput<'a> {
    pub app: &'a App,
    pub paths: &'a AppPaths,
    pub run_user: &'a str,
    pub path_env: &'a str,
}

const HARDENING: &str = "NoNewPrivileges=true
PrivateTmp=true
PrivateDevices=true
ProtectSystem=strict
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
RestrictNamespaces=true
LockPersonality=true
";

/// 실행 사용자 · 작업 디렉토리 · 쓰기 가능 경로 (Git 앱: rpanel-app, 파일 사이트: 사이트 유저)
fn unit_identity(
    app: &App,
    paths: &AppPaths,
    run_user: &str,
) -> (String, String, String, &'static str) {
    match (
        app.source(),
        app.site_user.as_deref(),
        app.site_root.as_deref(),
    ) {
        (Source::Files, Some(u), Some(root)) => {
            (u.to_string(), root.to_string(), format!("/home/{u}"), "")
        }
        _ => (
            run_user.to_string(),
            paths.current.display().to_string(),
            paths.data_dir.display().to_string(),
            "ProtectHome=true\n",
        ),
    }
}

/// Python 가상환경 위치 (파일 사이트: 문서 루트/.venv, Git 앱: current/.venv)
pub fn python_venv(app: &App, paths: &AppPaths) -> PathBuf {
    match (app.source(), app.site_root.as_deref()) {
        (Source::Files, Some(root)) => Path::new(root).join(".venv"),
        _ => paths.current.join(".venv"),
    }
}

/// 서비스·크론의 PATH: Python 앱은 가상환경의 bin 을 맨 앞에 (python·pip·gunicorn 등이 venv 것으로)
pub fn service_path(app: &App, paths: &AppPaths, base: &str) -> String {
    if app.kind() == AppKind::Python {
        format!("{}/bin:{base}", python_venv(app, paths).display())
    } else {
        base.to_string()
    }
}

pub const PYTHON_DEFAULT_START: &str = "python app.py";

pub fn systemd_unit(i: &UnitInput) -> anyhow::Result<String> {
    let app = i.app;
    let kind = app.kind();
    let exec_start = match (kind, app.source()) {
        (AppKind::Rust, Source::Git) => {
            let bin = app.binary_name.as_deref().unwrap_or(&app.slug);
            i.paths.current.join(bin).display().to_string()
        }
        (AppKind::Rust | AppKind::Python, _) => {
            let default = if kind == AppKind::Python {
                PYTHON_DEFAULT_START
            } else {
                "./app"
            };
            let cmd = app
                .start_cmd
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(default);
            format!("/bin/bash -c {}", systemd_quote(cmd))
        }
        _ => anyhow::bail!("{} 앱은 systemd 유닛이 없습니다", kind.as_str()),
    };
    let (user, cwd, rw, protect_home) = unit_identity(app, i.paths, i.run_user);
    // Python 확장 모듈(cffi 등)은 실행 가능한 메모리를 만들 수 있어 W^X 제한은 Rust 에만
    let mdwe = if kind == AppKind::Rust {
        "MemoryDenyWriteExecute=true\n"
    } else {
        ""
    };
    let path = service_path(app, i.paths, i.path_env);
    Ok(format!(
        "{MANAGED_MARKER} (app {slug}) — 직접 수정하지 마세요. 패널에서 재배포 시 덮어씁니다.
[Unit]
Description=Rpanel app {slug} ({name})
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User={user}
Group={user}
WorkingDirectory={cwd}
EnvironmentFile={env}
Environment=PATH={path}
ExecStart={exec}
Restart=always
RestartSec=5
TimeoutStopSec=20

{HARDENING}{protect_home}{mdwe}ReadWritePaths={rw}
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
",
        slug = app.slug,
        name = app.name.replace(['\n', '\r'], " "),
        env = i.paths.env_file.display(),
        exec = exec_start,
    ))
}

pub fn cron_unit_name(slug: &str, id: u64) -> String {
    format!("rpanel-cron-{slug}-{id}")
}

pub fn cron_staging(data_dir: &Path, slug: &str, id: u64) -> (PathBuf, PathBuf) {
    let base = data_dir.join("staging");
    (
        base.join(format!("cron-{slug}-{id}.service")),
        base.join(format!("cron-{slug}-{id}.timer")),
    )
}

/// 크론 작업 실행용 oneshot 서비스 (앱과 같은 사용자·env·작업 디렉토리)
pub fn cron_service(
    app: &App,
    paths: &AppPaths,
    job: &CronJob,
    run_user: &str,
    path_env: &str,
) -> String {
    let (user, cwd, rw, protect_home) = unit_identity(app, paths, run_user);
    format!(
        "{MANAGED_MARKER} (cron {slug}#{id}) — 패널의 크론 탭에서 관리합니다.
[Unit]
Description=Rpanel cron {slug} #{id} ({name})

[Service]
Type=oneshot
User={user}
Group={user}
WorkingDirectory={cwd}
EnvironmentFile={env}
Environment=PATH={path}
ExecStart=/bin/bash -c {cmd}
TimeoutStartSec=3600
{HARDENING}{protect_home}ReadWritePaths={rw}
",
        slug = app.slug,
        id = job.id,
        name = job.name.replace(['\n', '\r', '%'], " "),
        env = paths.env_file.display(),
        path = service_path(app, paths, path_env),
        cmd = systemd_quote(&job.command),
    )
}

pub fn cron_timer(app: &App, job: &CronJob) -> String {
    format!(
        "{MANAGED_MARKER} (cron {slug}#{id})
[Unit]
Description=Rpanel cron timer {slug} #{id}

[Timer]
OnCalendar={cal}
Persistent=true
Unit={unit}.service

[Install]
WantedBy=timers.target
",
        slug = app.slug,
        id = job.id,
        cal = job.schedule,
        unit = cron_unit_name(&app.slug, job.id),
    )
}

pub struct SiteInput<'a> {
    pub app: &'a App,
    pub paths: &'a AppPaths,
    /// 앞단 프록시 IP (비어 있으면 real_ip 설정 안 함)
    pub trusted_proxy: &'a str,
    /// IPv6 리스닝
    pub ipv6: bool,
}

pub const ACME_WEBROOT: &str = "/var/lib/rpanel/acme";

/// (인증서, 개인키) 경로
pub fn ssl_paths(app: &App) -> Option<(String, String)> {
    match app.ssl_mode.as_str() {
        "letsencrypt" => Some((
            format!("/etc/letsencrypt/live/rpanel-{}/fullchain.pem", app.slug),
            format!("/etc/letsencrypt/live/rpanel-{}/privkey.pem", app.slug),
        )),
        "custom" => Some((
            format!("/etc/rpanel/certs/{}/fullchain.pem", app.slug),
            format!("/etc/rpanel/certs/{}/key.pem", app.slug),
        )),
        _ => None,
    }
}

const PROXY_HEADERS: &str = "        proxy_http_version 1.1;
        proxy_set_header Host              $host;
        proxy_set_header X-Real-IP         $remote_addr;
        proxy_set_header X-Forwarded-For   $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $rpanel_proto;
        proxy_set_header Upgrade           $http_upgrade;
        proxy_set_header Connection        \"upgrade\";
        proxy_connect_timeout 10s;
        proxy_read_timeout    60s;
        proxy_send_timeout    60s;
        proxy_buffering       off;
";

const STATIC_ASSETS: &str = "
    location ~* \\.(?:js|css|woff2?|ttf|eot|svg|png|jpe?g|gif|webp|avif|ico)$ {
        expires 1y;
        add_header Cache-Control \"public, immutable\";
        access_log off;
        try_files $uri =404;
    }
";

const DENY_HIDDEN: &str = "
    location ~ /\\.(?!well-known).* {
        deny all;
    }
";

fn site_head(i: &SiteInput) -> String {
    let app = i.app;
    let mut s = String::new();
    if !i.trusted_proxy.is_empty() {
        s.push_str(&format!(
            "
    set_real_ip_from {};
    real_ip_header X-Forwarded-For;
    real_ip_recursive on;
",
            i.trusted_proxy
        ));
    }
    s.push_str(&format!(
        "
    add_header X-Content-Type-Options \"nosniff\" always;
    add_header X-Frame-Options \"SAMEORIGIN\" always;
    add_header Referrer-Policy \"strict-origin-when-cross-origin\" always;

    access_log /var/log/nginx/rpanel-{slug}-access.log;
    error_log  /var/log/nginx/rpanel-{slug}-error.log;

    client_max_body_size {body}m;
",
        slug = app.slug,
        body = app.client_max_body_mb.clamp(1, 10240),
    ));
    s
}

fn site_body(i: &SiteInput) -> String {
    let app = i.app;
    let mut s = String::new();
    match (app.kind(), app.source()) {
        (AppKind::Rust | AppKind::Python, _) => {
            let port = app.port.unwrap_or(0);
            s.push_str(&format!(
                "
    location / {{
        proxy_pass http://127.0.0.1:{port};
{PROXY_HEADERS}    }}
"
            ));
        }
        (AppKind::Proxy, _) => {
            let target = app.proxy_target.as_deref().unwrap_or("http://127.0.0.1:1");
            let sni = if target.starts_with("https://") {
                "        proxy_ssl_server_name on;\n"
            } else {
                ""
            };
            s.push_str(&format!(
                "
    location / {{
        proxy_pass {target};
{PROXY_HEADERS}{sni}    }}
"
            ));
        }
        (AppKind::Static, Source::Git) => {
            s.push_str(&format!(
                "
    root {root};
    index index.html;
",
                root = i.paths.www_current.display()
            ));
            if let Some(api) = app.api_proxy_port {
                s.push_str(&format!(
                    "
    location /api/ {{
        proxy_pass http://127.0.0.1:{api};
{PROXY_HEADERS}    }}
"
                ));
            }
            s.push_str(STATIC_ASSETS);
            s.push_str(
                "
    location / {
        try_files $uri $uri/ /index.html;
    }

    location = /index.html {
        add_header Cache-Control \"no-cache, no-store, must-revalidate\";
        expires 0;
    }
",
            );
        }
        (AppKind::Static, _) => {
            s.push_str(&format!(
                "
    root {root};
    index index.html index.htm;
",
                root = app.document_root().unwrap_or_default()
            ));
            s.push_str(STATIC_ASSETS);
            s.push_str(DENY_HIDDEN);
            s.push_str(
                "
    location / {
        try_files $uri $uri/ =404;
    }
",
            );
        }
    }
    s.push_str(
        "
    gzip on;
    gzip_vary on;
    gzip_min_length 1024;
    gzip_types text/plain text/css application/json application/javascript
               text/xml application/xml image/svg+xml;
",
    );
    s
}

/// Nginx 사이트 설정. 도메인이 없으면 None.
///  - :80  — ACME(Let's Encrypt) 경로 + (HTTPS 강제면 301, 아니면 사이트 본문)
///  - :443 — 인증서가 있을 때만
pub fn nginx_site(i: &SiteInput) -> Option<String> {
    let app = i.app;
    let domains = app.domain_list();
    if domains.is_empty() {
        return None;
    }
    let names = domains.join(" ");
    let head = site_head(i);
    let body = site_body(i);
    let ssl = ssl_paths(app);
    let acme = format!(
        "
    location ^~ /.well-known/acme-challenge/ {{
        root {ACME_WEBROOT};
        default_type \"text/plain\";
        try_files $uri =404;
    }}
"
    );
    let (v6_80, v6_443) = if i.ipv6 {
        ("    listen [::]:80;\n", "    listen [::]:443 ssl http2;\n")
    } else {
        ("", "")
    };
    let mut s = format!(
        "{MANAGED_MARKER} (app {slug}) — 직접 수정하지 마세요. 패널에서 덮어씁니다.
server {{
    listen 80;
{v6_80}    server_name {names};
{head}{acme}",
        slug = app.slug,
    );
    if ssl.is_some() && app.force_https {
        s.push_str(
            "
    location / {
        return 301 https://$host$request_uri;
    }
}
",
        );
    } else {
        s.push_str(&body);
        s.push_str("}\n");
    }
    if let Some((cert, key)) = ssl {
        s.push_str(&format!(
            "
server {{
    listen 443 ssl http2;
{v6_443}    server_name {names};

    ssl_certificate {cert};
    ssl_certificate_key {key};
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_session_cache shared:rpanel_ssl:10m;
    ssl_session_timeout 1d;
{head}{acme}{body}}}
"
        ));
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn app(kind: &str) -> App {
        App {
            id: 1,
            project_id: Some(1),
            name: "Beripick API".into(),
            slug: "beripick-api".into(),
            kind: kind.into(),
            git_url: Some("ssh://git@100.100.199.123:2222/beri/backend.git".into()),
            git_branch: "main".into(),
            root_dir: "".into(),
            build_cmd: None,
            start_cmd: Some("./app --name \"x\" $HOME 100%".into()),
            binary_name: Some("hello-api".into()),
            output_dir: "dist".into(),
            port: Some(8101),
            api_proxy_port: None,
            domains: "beripick.com www.beripick.com".into(),
            health_path: "/api/health".into(),
            status: "idle".into(),
            current_commit: None,
            current_release: None,
            webhook_token: "t".into(),
            redis_db: None,
            backup_schedule: "off".into(),
            backup_keep: 7,
            backup_last_scheduled: None,
            source: "git".into(),
            site_user: None,
            site_root: None,
            web_root: "".into(),
            proxy_target: None,
            ssl_mode: "none".into(),
            force_https: false,
            client_max_body_mb: 64,
            runtime_version: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn paths_layout() {
        let p = AppPaths::new(Path::new("/var/lib/rpanel"), "beri");
        assert_eq!(p.repo_dir, Path::new("/var/lib/rpanel/builds/beri/repo"));
        assert_eq!(p.current, Path::new("/var/lib/rpanel/apps/beri/current"));
        assert_eq!(p.www_current, Path::new("/var/lib/rpanel/www/beri/current"));
        assert_eq!(
            p.staging_unit,
            Path::new("/var/lib/rpanel/staging/beri.service")
        );
        assert_eq!(
            p.work_dir("frontend"),
            Path::new("/var/lib/rpanel/builds/beri/repo/frontend")
        );
        assert_eq!(unit_name("beri"), "rpanel-app-beri.service");
    }

    #[test]
    fn rust_unit() {
        let a = app("rust");
        let p = AppPaths::new(Path::new("/var/lib/rpanel"), &a.slug);
        let u = systemd_unit(&UnitInput {
            app: &a,
            paths: &p,
            run_user: "rpanel-app",
            path_env: "/usr/bin:/bin",
        })
        .unwrap();
        assert!(u.starts_with(MANAGED_MARKER));
        assert!(u.contains("ExecStart=/var/lib/rpanel/apps/beripick-api/current/hello-api\n"));
        assert!(u.contains("User=rpanel-app"));
        assert!(u.contains("EnvironmentFile=/var/lib/rpanel/apps/beripick-api/app.env"));
        assert!(u.contains("MemoryDenyWriteExecute=true"));
        assert!(u.contains("ReadWritePaths=/var/lib/rpanel/apps/beripick-api/data"));
    }

    #[test]
    fn files_rust_unit_quotes_command() {
        let a = files_app("rust");
        let p = AppPaths::new(Path::new("/var/lib/rpanel"), &a.slug);
        let u = systemd_unit(&UnitInput {
            app: &a,
            paths: &p,
            run_user: "rpanel-app",
            path_env: "/usr/bin",
        })
        .unwrap();
        assert!(u.contains(r#"ExecStart=/bin/bash -c "./app --name \"x\" $$HOME 100%%""#));
        assert!(systemd_unit(&UnitInput {
            app: &app("static"),
            paths: &p,
            run_user: "x",
            path_env: ""
        })
        .is_err());
    }

    #[test]
    fn env_file_escapes() {
        let vars = vec![
            EnvVar {
                key: "DATABASE_URL".into(),
                value: "mysql://u:p@127.0.0.1:3306/db".into(),
            },
            EnvVar {
                key: "WEIRD".into(),
                value: r#"a"b$c`d\e"#.into(),
            },
        ];
        let f = env_file(&vars, Some(8101));
        assert!(f.contains("DATABASE_URL=\"mysql://u:p@127.0.0.1:3306/db\"\n"));
        assert!(f.contains(r#"WEIRD="a\"b\$c\`d\\e""#));
        assert!(f.contains("PORT=\"8101\"\n"));
        assert!(f.contains("BIND_ADDR=\"127.0.0.1:8101\"\n"));
        let f2 = env_file(
            &[EnvVar {
                key: "PORT".into(),
                value: "9".into(),
            }],
            Some(8101),
        );
        assert!(!f2.contains("PORT=\"8101\""));
        assert!(!env_file(&[], None).contains("PORT"));
    }

    #[test]
    fn service_site() {
        let a = app("rust");
        let p = AppPaths::new(Path::new("/var/lib/rpanel"), &a.slug);
        let s = nginx_site(&SiteInput {
            app: &a,
            paths: &p,
            trusted_proxy: "10.0.0.10",
            ipv6: true,
        })
        .unwrap();
        assert!(s.contains("server_name beripick.com www.beripick.com;"));
        assert!(s.contains("proxy_pass http://127.0.0.1:8101;"));
        assert!(s.contains("set_real_ip_from 10.0.0.10;"));
        assert!(s.contains("listen 80;"));
        assert!(!s.contains("default_server"));
        assert!(!s.contains("listen 443"));
        assert!(s.contains("/.well-known/acme-challenge/"));
        assert_eq!(s.matches('{').count(), s.matches('}').count());
    }

    #[test]
    fn static_site_with_api() {
        let mut a = app("static");
        a.api_proxy_port = Some(8101);
        let p = AppPaths::new(Path::new("/var/lib/rpanel"), &a.slug);
        let s = nginx_site(&SiteInput {
            app: &a,
            paths: &p,
            trusted_proxy: "10.0.0.10",
            ipv6: true,
        })
        .unwrap();
        assert!(s.contains("root /var/lib/rpanel/www/beripick-api/current;"));
        assert!(s.contains("location /api/ {\n        proxy_pass http://127.0.0.1:8101;"));
        assert!(s.contains("try_files $uri $uri/ /index.html;"));
        assert_eq!(s.matches('{').count(), s.matches('}').count());

        a.api_proxy_port = None;
        let s = nginx_site(&SiteInput {
            app: &a,
            paths: &p,
            trusted_proxy: "10.0.0.10",
            ipv6: true,
        })
        .unwrap();
        assert!(!s.contains("location /api/"));

        a.domains = "".into();
        assert!(nginx_site(&SiteInput {
            app: &a,
            paths: &p,
            trusted_proxy: "10.0.0.10",
            ipv6: true,
        })
        .is_none());
    }

    fn files_app(kind: &str) -> App {
        let mut a = app(kind);
        a.source = "files".into();
        a.git_url = None;
        a.site_user = Some("beripick".into());
        a.site_root = Some("/home/beripick/htdocs/beripick.com".into());
        a
    }

    #[test]
    fn rust_files_site_runs_uploaded_binary_as_site_user() {
        let mut a = files_app("rust");
        a.start_cmd = Some("./app --port $PORT".into());
        let p = AppPaths::new(Path::new("/var/lib/rpanel"), &a.slug);
        let unit = systemd_unit(&UnitInput {
            app: &a,
            paths: &p,
            run_user: "rpanel",
            path_env: "/usr/bin:/bin",
        })
        .unwrap();
        assert!(unit.contains("User=beripick\n"));
        assert!(unit.contains("WorkingDirectory=/home/beripick/htdocs/beripick.com\n"));
        assert!(unit.contains("ExecStart=/bin/bash -c "));
        assert!(unit.contains("./app --port $$PORT"), "{unit}");
        assert!(unit.contains("MemoryDenyWriteExecute=true"));
        let s = nginx_site(&SiteInput {
            app: &a,
            paths: &p,
            trusted_proxy: "",
            ipv6: true,
        })
        .unwrap();
        assert!(s.contains("proxy_pass http://127.0.0.1:8101;"), "{s}");
        assert!(
            !s.contains("root /home"),
            "service sites are proxied, not served from disk"
        );
        assert_eq!(s.matches('{').count(), s.matches('}').count());
    }

    #[test]
    fn ssl_and_redirect() {
        let mut a = files_app("static");
        a.ssl_mode = "letsencrypt".into();
        a.force_https = true;
        let p = AppPaths::new(Path::new("/var/lib/rpanel"), &a.slug);
        let s = nginx_site(&SiteInput {
            app: &a,
            paths: &p,
            trusted_proxy: "",
            ipv6: true,
        })
        .unwrap();
        let (http, https) = s.split_once("listen 443 ssl http2;").unwrap();
        assert!(http.contains("return 301 https://$host$request_uri;"));
        assert!(!http.contains("root /home"), "http block only redirects");
        assert!(https
            .contains("ssl_certificate /etc/letsencrypt/live/rpanel-beripick-api/fullchain.pem;"));
        assert!(https.contains("root /home/beripick/htdocs/beripick.com;"));
        assert!(https.contains("try_files $uri $uri/ =404;"));
        assert_eq!(s.matches('{').count(), s.matches('}').count());

        a.force_https = false;
        a.ssl_mode = "custom".into();
        let s = nginx_site(&SiteInput {
            app: &a,
            paths: &p,
            trusted_proxy: "",
            ipv6: true,
        })
        .unwrap();
        assert!(!s.contains("return 301"));
        assert!(s.contains("ssl_certificate_key /etc/rpanel/certs/beripick-api/key.pem;"));
        assert_eq!(
            s.matches("root /home/beripick/htdocs/beripick.com;")
                .count(),
            2
        );
    }

    #[test]
    fn proxy_site_and_files_rust_unit() {
        let mut a = app("proxy");
        a.source = "none".into();
        a.proxy_target = Some("https://10.0.0.11:8443".into());
        let p = AppPaths::new(Path::new("/var/lib/rpanel"), &a.slug);
        let s = nginx_site(&SiteInput {
            app: &a,
            paths: &p,
            trusted_proxy: "10.0.0.10",
            ipv6: true,
        })
        .unwrap();
        assert!(s.contains("proxy_pass https://10.0.0.11:8443;"));
        assert!(s.contains("proxy_ssl_server_name on;"));
        assert!(s.contains("set_real_ip_from 10.0.0.10;"));

        let n = files_app("rust");
        let u = systemd_unit(&UnitInput {
            app: &n,
            paths: &p,
            run_user: "rpanel-app",
            path_env: "/usr/bin",
        })
        .unwrap();
        assert!(u.contains("User=beripick\n"));
        assert!(u.contains("WorkingDirectory=/home/beripick/htdocs/beripick.com\n"));
        assert!(u.contains("ReadWritePaths=/home/beripick\n"));
        assert!(!u.contains("ProtectHome"));
    }

    #[test]
    fn python_units_use_venv() {
        let mut a = files_app("python");
        a.start_cmd = None;
        let p = AppPaths::new(Path::new("/var/lib/rpanel"), &a.slug);
        let u = systemd_unit(&UnitInput {
            app: &a,
            paths: &p,
            run_user: "rpanel-app",
            path_env: "/usr/bin:/bin",
        })
        .unwrap();
        assert!(u.contains(
            "Environment=PATH=/home/beripick/htdocs/beripick.com/.venv/bin:/usr/bin:/bin\n"
        ));
        assert!(u.contains(r#"ExecStart=/bin/bash -c "python app.py""#));
        assert!(!u.contains("MemoryDenyWriteExecute"));
        assert!(u.contains("User=beripick\n"));

        let mut g = app("python");
        g.start_cmd = Some("gunicorn -b 127.0.0.1:$PORT app:app".into());
        let u = systemd_unit(&UnitInput {
            app: &g,
            paths: &p,
            run_user: "rpanel-app",
            path_env: "/usr/bin",
        })
        .unwrap();
        assert!(u.contains(
            "Environment=PATH=/var/lib/rpanel/apps/beripick-api/current/.venv/bin:/usr/bin\n"
        ));
        assert!(u.contains("User=rpanel-app"));
        let job = CronJob {
            id: 7,
            app_id: 1,
            name: "정리".into(),
            schedule: "*-*-* 03:00:00".into(),
            command: "python manage.py clean".into(),
            enabled: true,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let cron = cron_service(&g, &p, &job, "rpanel-app", "/usr/bin");
        assert!(cron.contains("current/.venv/bin:/usr/bin"));
        // Rust 는 그대로
        let r = app("rust");
        assert_eq!(service_path(&r, &p, "/usr/bin"), "/usr/bin");
        let s = nginx_site(&SiteInput {
            app: &a,
            paths: &p,
            trusted_proxy: "",
            ipv6: false,
        })
        .unwrap();
        assert!(s.contains("proxy_pass http://127.0.0.1:8101;"));
    }
}
