//! systemd 유닛 / Nginx 사이트 / env 파일 템플릿.
//! 모든 입력값은 `validate`를 통과한 값이라는 전제지만, 한 번 더 방어적으로 이스케이프합니다.

use std::path::{Path, PathBuf};

use crate::models::{App, AppKind, CronJob, EnvVar};

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

pub fn systemd_unit(i: &UnitInput) -> anyhow::Result<String> {
    let app = i.app;
    let kind = app.kind();
    let exec_start = match kind {
        AppKind::Rust => {
            let bin = app.binary_name.as_deref().unwrap_or(&app.slug);
            i.paths.current.join(bin).display().to_string()
        }
        AppKind::Node => {
            let cmd = app
                .start_cmd
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("npm start");
            format!("/bin/bash -c {}", systemd_quote(cmd))
        }
        AppKind::Static => anyhow::bail!("static 앱은 systemd 유닛이 없습니다"),
    };
    // Node(V8 JIT)는 W^X 제한과 충돌
    let mdwe = if kind == AppKind::Rust {
        "MemoryDenyWriteExecute=true\n"
    } else {
        ""
    };
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

NoNewPrivileges=true
PrivateTmp=true
PrivateDevices=true
ProtectSystem=strict
ProtectHome=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
RestrictNamespaces=true
LockPersonality=true
{mdwe}ReadWritePaths={data}
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
",
        slug = app.slug,
        name = app.name.replace(['\n', '\r'], " "),
        user = i.run_user,
        cwd = i.paths.current.display(),
        env = i.paths.env_file.display(),
        path = i.path_env,
        exec = exec_start,
        data = i.paths.data_dir.display(),
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
NoNewPrivileges=true
PrivateTmp=true
PrivateDevices=true
ProtectSystem=strict
ProtectHome=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
RestrictNamespaces=true
LockPersonality=true
ReadWritePaths={data}
",
        slug = app.slug,
        id = job.id,
        name = job.name.replace(['\n', '\r', '%'], " "),
        user = run_user,
        cwd = paths.current.display(),
        env = paths.env_file.display(),
        path = path_env,
        cmd = systemd_quote(&job.command),
        data = paths.data_dir.display(),
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
    pub trusted_proxy: &'a str,
}

const PROXY_HEADERS: &str = "        proxy_http_version 1.1;
        proxy_set_header Host              $host;
        proxy_set_header X-Real-IP         $remote_addr;
        proxy_set_header X-Forwarded-For   $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $http_x_forwarded_proto;
        proxy_set_header Upgrade           $http_upgrade;
        proxy_set_header Connection        \"upgrade\";
        proxy_connect_timeout 10s;
        proxy_read_timeout    60s;
        proxy_send_timeout    60s;
        proxy_buffering       off;
";

/// Nginx server 블록 (VM102: CT100 → :80 → 이 블록). 도메인이 없으면 None.
pub fn nginx_site(i: &SiteInput) -> Option<String> {
    let app = i.app;
    let domains = app.domain_list();
    if domains.is_empty() {
        return None;
    }
    let slug = &app.slug;
    let mut s = format!(
        "{MANAGED_MARKER} (app {slug}) — 직접 수정하지 마세요. 패널에서 재배포 시 덮어씁니다.
server {{
    listen 80;
    server_name {names};

    set_real_ip_from {proxy};
    real_ip_header X-Forwarded-For;
    real_ip_recursive on;

    add_header X-Content-Type-Options \"nosniff\" always;
    add_header X-Frame-Options \"SAMEORIGIN\" always;
    add_header Referrer-Policy \"strict-origin-when-cross-origin\" always;

    access_log /var/log/nginx/rpanel-{slug}-access.log;
    error_log  /var/log/nginx/rpanel-{slug}-error.log;

    client_max_body_size 20m;
",
        names = domains.join(" "),
        proxy = i.trusted_proxy,
    );

    match app.kind() {
        AppKind::Rust | AppKind::Node => {
            let port = app.port.unwrap_or(0);
            s.push_str(&format!(
                "
    location / {{
        proxy_pass http://127.0.0.1:{port};
{PROXY_HEADERS}    }}
"
            ));
        }
        AppKind::Static => {
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
            s.push_str(
                "
    location ~* \\.(?:js|css|woff2?|ttf|eot|svg|png|jpe?g|gif|webp|avif|ico)$ {
        expires 1y;
        add_header Cache-Control \"public, immutable\";
        access_log off;
        try_files $uri =404;
    }

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
    }

    s.push_str(
        "
    gzip on;
    gzip_vary on;
    gzip_min_length 1024;
    gzip_types text/plain text/css application/json application/javascript
               text/xml application/xml image/svg+xml;
}
",
    );
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn app(kind: &str) -> App {
        App {
            id: 1,
            project_id: 1,
            name: "Beripick API".into(),
            slug: "beripick-api".into(),
            kind: kind.into(),
            git_url: "ssh://git@100.100.199.123:2222/beri/backend.git".into(),
            git_branch: "main".into(),
            root_dir: "".into(),
            build_cmd: None,
            start_cmd: Some("node server.js --name \"x\" $HOME 100%".into()),
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
    fn node_unit_quotes_command() {
        let a = app("node");
        let p = AppPaths::new(Path::new("/var/lib/rpanel"), &a.slug);
        let u = systemd_unit(&UnitInput {
            app: &a,
            paths: &p,
            run_user: "rpanel-app",
            path_env: "/usr/bin",
        })
        .unwrap();
        assert!(u.contains(r#"ExecStart=/bin/bash -c "node server.js --name \"x\" $$HOME 100%%""#));
        assert!(!u.contains("MemoryDenyWriteExecute"));
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
        })
        .unwrap();
        assert!(s.contains("server_name beripick.com www.beripick.com;"));
        assert!(s.contains("proxy_pass http://127.0.0.1:8101;"));
        assert!(s.contains("set_real_ip_from 10.0.0.10;"));
        assert!(s.contains("listen 80;"));
        assert!(!s.contains("default_server"));
        assert!(!s.contains("try_files"));
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
        })
        .unwrap();
        assert!(!s.contains("location /api/"));

        a.domains = "".into();
        assert!(nginx_site(&SiteInput {
            app: &a,
            paths: &p,
            trusted_proxy: "10.0.0.10"
        })
        .is_none());
    }
}
