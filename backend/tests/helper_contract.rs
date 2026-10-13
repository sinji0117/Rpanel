//! 백엔드가 만드는 systemd 유닛 / Nginx 사이트가 rpanel-helper 의 화이트리스트 검증을
//! 통과하는지, 그리고 위험한 변형은 거부되는지 확인합니다. (bash 필요)

use std::{path::Path, process::Command};

use chrono::Utc;
use rpanel::{
    deploy::templates::{nginx_site, systemd_unit, AppPaths, SiteInput, UnitInput},
    models::App,
};

const DATA: &str = "/var/lib/rpanel";

fn helper() -> String {
    format!("{}/../deploy/rpanel-helper", env!("CARGO_MANIFEST_DIR"))
}

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
        start_cmd: Some("node dist/server.js".into()),
        binary_name: Some("hello-api".into()),
        output_dir: "dist".into(),
        port: Some(8101),
        api_proxy_port: Some(8101),
        domains: "beripick.com www.beripick.com".into(),
        health_path: "/".into(),
        status: "idle".into(),
        current_commit: None,
        current_release: None,
        webhook_token: "x".into(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

/// helper 함수를 source 해서 검증 함수만 실행
fn validate(func: &str, content: &str, slug: &str) -> (bool, String) {
    let dir = std::env::temp_dir().join(format!(
        "rpanel-helper-test-{}-{}",
        std::process::id(),
        rand_id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("file");
    std::fs::write(&f, content).unwrap();
    let script = format!(
        "set -euo pipefail; source '{}'; {func} '{}' '{slug}'",
        helper(),
        f.display()
    );
    let out = Command::new("bash")
        .arg("-c")
        .arg(script)
        .output()
        .expect("bash");
    let _ = std::fs::remove_dir_all(&dir);
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn rand_id() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

fn unit(kind: &str) -> String {
    let a = app(kind);
    let p = AppPaths::new(Path::new(DATA), &a.slug);
    systemd_unit(&UnitInput {
        app: &a,
        paths: &p,
        run_user: "rpanel-app",
        path_env: "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
    })
    .unwrap()
}

fn site(kind: &str) -> String {
    let a = app(kind);
    let p = AppPaths::new(Path::new(DATA), &a.slug);
    nginx_site(&SiteInput {
        app: &a,
        paths: &p,
        trusted_proxy: "10.0.0.10",
    })
    .unwrap()
}

#[test]
fn generated_units_pass() {
    for kind in ["rust", "node"] {
        let (ok, err) = validate("validate_unit", &unit(kind), "beripick-api");
        assert!(ok, "{kind} unit rejected: {err}");
    }
}

#[test]
fn generated_sites_pass() {
    for kind in ["rust", "node", "static"] {
        let (ok, err) = validate("validate_site", &site(kind), "beripick-api");
        assert!(ok, "{kind} site rejected: {err}\n{}", site(kind));
    }
}

#[test]
fn dangerous_units_rejected() {
    let base = unit("rust");
    let cases = [
        base.replace("User=rpanel-app", "User=root"),
        base.replace("NoNewPrivileges=true", "NoNewPrivileges=false"),
        base.replace("[Service]\n", "[Service]\nExecStartPre=+/bin/sh -c id\n"),
        base.replace(
            "ExecStart=/var/lib/rpanel/apps/beripick-api/current/hello-api",
            "ExecStart=/bin/sh -c id",
        ),
        base.replace(
            "ExecStart=/var/lib/rpanel/apps/beripick-api/current/hello-api",
            "ExecStart=/var/lib/rpanel/apps/other/current/x",
        ),
        base.replace(
            "ReadWritePaths=/var/lib/rpanel/apps/beripick-api/data",
            "ReadWritePaths=/etc",
        ),
        base.replace(
            "EnvironmentFile=/var/lib/rpanel/apps/beripick-api/app.env",
            "EnvironmentFile=/etc/shadow",
        ),
        base.replace(
            "Environment=PATH=",
            "Environment=LD_PRELOAD=/tmp/x.so PATH=",
        ),
        format!("{base}\n[Service]\nAmbientCapabilities=CAP_NET_ADMIN\n"),
    ];
    for (i, c) in cases.iter().enumerate() {
        assert_ne!(c, &base, "case {i} did not change the unit");
        let (ok, _) = validate("validate_unit", c, "beripick-api");
        assert!(!ok, "dangerous unit case {i} was accepted:\n{c}");
    }
    // 다른 slug 경로로 검증하면 거부
    assert!(!validate("validate_unit", &base, "other-app").0);
}

#[test]
fn dangerous_sites_rejected() {
    let base = site("static");
    let cases = [
        base.replace("listen 80;", "listen 80 default_server;"),
        base.replace(
            "root /var/lib/rpanel/www/beripick-api/current;",
            "root /etc;",
        ),
        base.replace(
            "    index index.html;\n",
            "    index index.html;\n    include /etc/nginx/secret.conf;\n",
        ),
        base.replace(
            "    index index.html;\n",
            "    location /x { alias /etc/; }\n",
        ),
        base.replace(
            "proxy_pass http://127.0.0.1:8101;",
            "proxy_pass http://169.254.169.254;",
        ),
        base.replace(
            "access_log /var/log/nginx/rpanel-beripick-api-access.log;",
            "access_log /var/www/html/leak.log;",
        ),
        base.replace(
            "server_name beripick.com www.beripick.com;",
            "server_name beripick.com; return 301 http://evil;",
        ),
        base.replace(
            "client_max_body_size 20m;",
            "client_max_body_size 20m;\n    load_module /tmp/x.so;",
        ),
    ];
    for (i, c) in cases.iter().enumerate() {
        assert_ne!(c, &base, "case {i} did not change the site");
        let (ok, _) = validate("validate_site", c, "beripick-api");
        assert!(!ok, "dangerous site case {i} was accepted:\n{c}");
    }
}

#[test]
fn slug_validation_in_helper() {
    for bad in ["../etc", "Bad", "-x", "a b", "", "a;id"] {
        let out = Command::new("bash")
            .arg(helper())
            .arg("status")
            .arg(bad)
            .output()
            .unwrap();
        assert!(!out.status.success(), "slug {bad:?} accepted");
    }
    let out = Command::new("bash")
        .arg(helper())
        .arg("svc")
        .arg("reboot")
        .arg("ok-slug")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let out = Command::new("bash")
        .arg(helper())
        .arg("journal")
        .arg("ok-slug")
        .arg("99999")
        .output()
        .unwrap();
    assert!(!out.status.success());
}
