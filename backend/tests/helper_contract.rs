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
        project_id: Some(1),
        name: "Beripick API".into(),
        slug: "beripick-api".into(),
        kind: kind.into(),
        git_url: Some("ssh://git@100.100.199.123:2222/beri/backend.git".into()),
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
        frontend_dir: None,
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
        ipv6: true,
    })
    .unwrap()
}

#[test]
fn generated_units_pass() {
    for kind in ["rust", "python"] {
        let (ok, err) = validate("validate_unit", &unit(kind), "beripick-api");
        assert!(ok, "{kind} unit rejected: {err}\n{}", unit(kind));
    }
}

#[test]
fn generated_sites_pass() {
    for kind in ["rust", "python", "static"] {
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
            "proxy_pass http://unix:/var/run/docker.sock:/;",
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
            "client_max_body_size 64m;",
            "client_max_body_size 64m;\n    load_module /tmp/x.so;",
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

fn cron_job() -> rpanel::models::CronJob {
    rpanel::models::CronJob {
        id: 42,
        app_id: 1,
        name: "정리 작업".into(),
        schedule: "*-*-* 03:00:00".into(),
        command: "./hello-api --cleanup \"old\" $HOME".into(),
        enabled: true,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn validate_args(func: &str, content: &str, args: &str) -> (bool, String) {
    let dir = std::env::temp_dir().join(format!(
        "rpanel-helper-test-{}-{}",
        std::process::id(),
        rand_id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("file");
    std::fs::write(&f, content).unwrap();
    let script = format!(
        "set -euo pipefail; source '{}'; {func} '{}' {args}",
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

#[test]
fn cron_units_pass_and_dangerous_rejected() {
    use rpanel::deploy::templates::{cron_service, cron_timer};
    let a = app("rust");
    let p = AppPaths::new(Path::new(DATA), &a.slug);
    let svc = cron_service(
        &a,
        &p,
        &cron_job(),
        "rpanel-app",
        "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
    );
    let tmr = cron_timer(&a, &cron_job());
    let (ok, err) = validate_args("validate_unit", &svc, "beripick-api cron");
    assert!(ok, "cron service rejected: {err}\n{svc}");
    let (ok, err) = validate_args("validate_timer", &tmr, "beripick-api 42");
    assert!(ok, "timer rejected: {err}\n{tmr}");

    // 서비스 유닛 검증 모드로 cron 유닛을 넣으면 거부 (Type=oneshot)
    assert!(!validate_args("validate_unit", &svc, "beripick-api service").0);
    for bad in [
        svc.replace("User=rpanel-app", "User=root"),
        svc.replace("Type=oneshot", "Type=simple"),
        format!("{svc}Restart=always\n"),
    ] {
        assert!(
            !validate_args("validate_unit", &bad, "beripick-api cron").0,
            "accepted:\n{bad}"
        );
    }
    for bad in [
        tmr.replace(
            "Unit=rpanel-cron-beripick-api-42.service",
            "Unit=ssh.service",
        ),
        tmr.replace("OnCalendar=*-*-* 03:00:00", "OnCalendar=$(id)"),
        tmr.replace("WantedBy=timers.target", "WantedBy=multi-user.target"),
        tmr.replace("Persistent=true", "Persistent=true\nOnBootSec=1"),
    ] {
        assert!(
            !validate_args("validate_timer", &bad, "beripick-api 42").0,
            "accepted:\n{bad}"
        );
    }
    // 다른 id 로 검증하면 Unit= 불일치로 거부
    assert!(!validate_args("validate_timer", &tmr, "beripick-api 43").0);
}

#[test]
fn helper_rejects_bad_db_and_backup_args() {
    let run = |args: &[&str]| {
        Command::new("bash")
            .arg(helper())
            .args(args)
            .output()
            .unwrap()
    };
    for (a, b) in [
        ("mysql", "x_user"),
        ("Bad", "u_ser"),
        ("ok_db", "root"),
        ("a`b", "u_ser"),
        ("db'x", "u_ser"),
        ("rpanel", "u_ser"),
    ] {
        let out = run(&["db-create", a, b]);
        assert!(!out.status.success(), "db-create {a} {b} accepted");
        let out = run(&["db-drop", a, b]);
        assert!(!out.status.success(), "db-drop {a} {b} accepted");
    }
    assert!(!run(&["backup-db", "app", "ok_db", "2026-10-13"])
        .status
        .success());
    assert!(!run(&["restore-db", "app", "ok_db", "../../etc/passwd"])
        .status
        .success());
    assert!(
        !run(&["restore-data", "app", "20261013-030000-db-x.sql.gz"])
            .status
            .success()
    );
    assert!(!run(&["nginx-log", "app", "secret", "10"]).status.success());
    assert!(!run(&["nginx-log", "../x", "access", "10"]).status.success());
    assert!(!run(&["run-cron", "app", "1;id"]).status.success());
}

// ───────────── 파일 사이트 · SSL ─────────────

fn files_app(kind: &str) -> App {
    let mut a = app(kind);
    a.source = "files".into();
    a.git_url = None;
    a.site_user = Some("beripick".into());
    a.site_root = Some("/home/beripick/htdocs/beripick.com".into());
    a
}

/// 레지스트리를 임시 파일로 두고 helper 함수 실행
fn validate_site_reg(func: &str, content: &str, args: &str) -> (bool, String) {
    let dir = std::env::temp_dir().join(format!(
        "rpanel-helper-reg-{}-{}",
        std::process::id(),
        rand_id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("file");
    let reg = dir.join("site-users");
    std::fs::write(&f, content).unwrap();
    std::fs::write(
        &reg,
        "beripick-api beripick /home/beripick/htdocs/beripick.com\n",
    )
    .unwrap();
    let script = format!(
        "set -euo pipefail; export RPANEL_TEST_SITES_REG='{}'; source '{}'; {func} '{}' {args}",
        reg.display(),
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

fn site_of(a: &App) -> String {
    let p = AppPaths::new(Path::new(DATA), &a.slug);
    nginx_site(&SiteInput {
        app: a,
        paths: &p,
        trusted_proxy: "",
        ipv6: true,
    })
    .unwrap()
}

#[test]
fn file_sites_pass() {
    for kind in ["rust", "python", "static"] {
        for (ssl, force) in [("none", false), ("letsencrypt", true), ("custom", false)] {
            let mut a = files_app(kind);
            a.ssl_mode = ssl.into();
            a.force_https = force;
            a.web_root = if kind == "static" {
                "public".into()
            } else {
                "".into()
            };
            let conf = site_of(&a);
            let (ok, err) = validate_site_reg("validate_site", &conf, "beripick-api");
            assert!(ok, "{kind}/{ssl} rejected: {err}\n{conf}");
        }
    }
    let mut p = app("proxy");
    p.source = "none".into();
    p.proxy_target = Some("https://10.0.0.11:8443/app".into());
    let (ok, err) = validate_site_reg("validate_site", &site_of(&p), "beripick-api");
    assert!(ok, "proxy rejected: {err}");
}

#[test]
fn dangerous_file_sites_rejected() {
    let mut a = files_app("static");
    a.ssl_mode = "letsencrypt".into();
    let base = site_of(&a);
    let cases = [
        base.replace(
            "root /home/beripick/htdocs/beripick.com;",
            "root /home/otheruser/htdocs/x;",
        ),
        base.replace("root /home/beripick/htdocs/beripick.com;", "root /etc;"),
        base.replace(
            "root /home/beripick/htdocs/beripick.com;",
            "root /home/beripick/htdocs/beripick.com/../../..;",
        ),
        base.replace(
            "try_files $uri $uri/ =404;",
            "try_files $uri $uri/ =404;\n        include /etc/passwd;",
        ),
        base.replace(
            "try_files $uri $uri/ =404;",
            "try_files $uri $uri/ =404;\n        fastcgi_pass 127.0.0.1:9000;",
        ),
        base.replace(
            "ssl_certificate /etc/letsencrypt/live/rpanel-beripick-api/fullchain.pem;",
            "ssl_certificate /etc/letsencrypt/live/rpanel-other/fullchain.pem;",
        ),
        base.replace(
            "listen 443 ssl http2;",
            "listen 443 ssl http2 default_server;",
        ),
        base.replace("deny all;", "allow all;"),
        format!("{base}\nserver {{\n    listen 80;\n    return 302 http://evil;\n}}\n"),
    ];
    for (i, c) in cases.iter().enumerate() {
        assert_ne!(c, &base, "case {i} did not change");
        let (ok, _) = validate_site_reg("validate_site", c, "beripick-api");
        assert!(!ok, "dangerous file-site case {i} accepted:\n{c}");
    }
    // 레지스트리에 없는 slug 의 사이트 루트는 거부
    let (ok, _) = validate_site_reg("validate_site", &base, "other-app");
    assert!(!ok);
}

#[test]
fn site_units_contract() {
    for kind in ["rust", "python"] {
        let a = files_app(kind);
        let p = AppPaths::new(Path::new(DATA), &a.slug);
        let u = systemd_unit(&UnitInput {
            app: &a,
            paths: &p,
            run_user: "rpanel-app",
            path_env: "/usr/bin:/bin",
        })
        .unwrap();
        let (ok, err) = validate_site_reg("validate_unit", &u, "beripick-api");
        assert!(ok, "site unit rejected: {err}\n{u}");
        for bad in [
            u.replace("User=beripick", "User=root"),
            u.replace("User=beripick", "User=rpanel-app"),
            u.replace("ReadWritePaths=/home/beripick", "ReadWritePaths=/"),
            u.replace(
                "WorkingDirectory=/home/beripick/htdocs/beripick.com",
                "WorkingDirectory=/root",
            ),
        ] {
            let (ok, _) = validate_site_reg("validate_unit", &bad, "beripick-api");
            assert!(!ok, "dangerous site unit accepted:\n{bad}");
        }
    }
}

#[test]
fn helper_rejects_bad_site_args() {
    let run = |args: &[&str]| {
        Command::new("bash")
            .arg(helper())
            .args(args)
            .output()
            .unwrap()
    };
    for (user, domain) in [
        ("root", "a.com"),
        ("www-data", "a.com"),
        ("Ab", "a.com"),
        ("ok-user", "bad_domain"),
        ("ok-user", "a.com;id"),
        ("x;id", "a.com"),
    ] {
        assert!(
            !run(&["site-create", "app", user, domain, "rust"])
                .status
                .success(),
            "site-create {user} {domain}"
        );
    }
    assert!(!run(&["site-create", "app", "okuser", "a.com", "shell"])
        .status
        .success());
    assert!(!run(&["site-create", "app", "okuser", "a.com", "php"])
        .status
        .success());
    assert!(!run(&["ssl-issue", "app", "not-an-email", "a.com"])
        .status
        .success());
    assert!(
        !run(&["ssl-issue", "app", "a@b.com", "a.com", "--deploy-hook=id"])
            .status
            .success()
    );
    assert!(
        !run(&["restore-files", "app", "20261013-030000-data.tar.gz"])
            .status
            .success()
    );
}

/// helper 함수 하나를 인자와 함께 실행 (source 후 호출)
fn helper_fn(call: &str) -> bool {
    let script = format!("set -euo pipefail; source '{}'; {call}", helper());
    Command::new("bash")
        .arg("-c")
        .arg(script)
        .output()
        .expect("bash")
        .status
        .success()
}

#[test]
fn account_and_backup_args() {
    // Redis 계정 이름·키 패턴: 설정 파일·명령 주입에 쓰일 수 있는 문자는 거부
    assert!(helper_fn("valid_redis_user cache_ro"));
    for bad in [
        "default",
        "ab",
        "Bad",
        "'a b'",
        "'x\nuser evil on'",
        "'a;b'",
    ] {
        assert!(!helper_fn(&format!("valid_redis_user {bad}")), "{bad}");
    }
    assert!(helper_fn("valid_key_pattern 'myapp:*'"));
    for bad in ["'a b'", "'~* +@all'", "'x\nuser evil'", "''", "'&*'"] {
        assert!(!helper_fn(&format!("valid_key_pattern {bad}")), "{bad}");
    }
    // 비밀번호: URL·SQL 에 그대로 넣을 수 있는 문자만
    assert!(helper_fn("echo Good_pass-123 | read_password >/dev/null"));
    for bad in [
        "short",
        "\"has space 123\"",
        "\"quote'1234567\"",
        "\"semi;colon123\"",
    ] {
        assert!(
            !helper_fn(&format!("echo {bad} | read_password >/dev/null")),
            "{bad}"
        );
    }
    // 백업 폴더: 앱 slug 또는 _databases 만
    assert!(helper_fn("valid_backup_scope _databases"));
    assert!(helper_fn("valid_backup_scope beripick-api"));
    for bad in ["../etc", "_other", "''"] {
        assert!(!helper_fn(&format!("valid_backup_scope {bad}")), "{bad}");
    }
}

#[test]
fn helper_rejects_bad_runtime_args() {
    let tmp = std::env::temp_dir().join(format!("rpanel-rt-{}", std::process::id()));
    let run = |args: &[&str]| {
        Command::new("bash")
            .arg(helper())
            .args(args)
            .env("RPANEL_TEST_RUNTIMES", &tmp)
            .output()
            .unwrap()
    };
    for (lang, v) in [
        ("python", "3"),
        ("python", "2.7"),
        ("python", "3.12;id"),
        ("python", "../3.12"),
        ("python", "3.12.1"),
        ("node", "v22"),
        ("node", "8"),
        ("node", "22/../../etc"),
        ("node", "lts"),
        ("ruby", "3.3"),
        ("rust", "1.85"),
        ("", ""),
    ] {
        let out = run(&["runtime-install", lang, v]);
        assert!(!out.status.success(), "runtime-install {lang} {v} accepted");
        let out = run(&["runtime-remove", lang, v]);
        assert!(!out.status.success(), "runtime-remove {lang} {v} accepted");
    }
    // 정상 인자로 지우기는 (없어도) 성공하고, 지정 폴더 밖은 건드리지 않음
    std::fs::create_dir_all(tmp.join("node/22/bin")).unwrap();
    std::fs::create_dir_all(tmp.join("node/20/bin")).unwrap();
    let out = run(&["runtime-remove", "node", "22"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!tmp.join("node/22").exists());
    assert!(tmp.join("node/20").exists());
    let out = run(&["runtime-remove", "python", "3.12"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn helper_system_log_whitelist() {
    let run = |args: &[&str]| {
        Command::new("bash")
            .arg(helper())
            .args(args)
            .output()
            .unwrap()
    };
    for (kind, n) in [
        ("auth", "10"),
        ("../etc/shadow", "10"),
        ("panel;id", "10"),
        ("", "10"),
        ("panel", "0"),
        ("panel", "99999"),
        ("panel", "1;id"),
    ] {
        assert!(
            !run(&["system-log", kind, n]).status.success(),
            "system-log {kind} {n} accepted"
        );
    }
    for kind in ["panel", "nginx", "ssh", "mariadb", "redis"] {
        assert!(run(&["system-log", kind, "5"]).status.success(), "{kind}");
    }
}

#[test]
fn helper_rejects_bad_mongo_backup_args() {
    let run = |args: &[&str]| {
        Command::new("bash")
            .arg(helper())
            .args(args)
            .env("RPANEL_TEST_ENV_FILE", "/nonexistent")
            .output()
            .unwrap()
    };
    for db in [
        "admin",
        "local",
        "config",
        "rpanel_logs",
        "Shop",
        "a;b",
        "../x",
        "x",
    ] {
        let out = run(&["backup-mongo", "_databases", db, "20261016-120000"]);
        assert!(!out.status.success(), "backup-mongo {db} accepted");
        let out = run(&[
            "restore-mongo",
            "_databases",
            db,
            &format!("20261016-120000-mongo-{db}.archive.gz"),
        ]);
        assert!(!out.status.success(), "restore-mongo {db} accepted");
    }
    // 다른 DB 의 백업 파일로 복원 금지, 잘못된 파일 이름 거부
    assert!(!run(&[
        "restore-mongo",
        "_databases",
        "shop",
        "20261016-120000-mongo-other.archive.gz"
    ])
    .status
    .success());
    assert!(
        !run(&["restore-mongo", "_databases", "shop", "../../etc/passwd"])
            .status
            .success()
    );
    assert!(
        !run(&["backup-mongo", "bad scope", "shop", "20261016-120000"])
            .status
            .success()
    );
    // MONGODB_URL 이 없으면 덤프하지 않음
    let out = run(&["backup-mongo", "_databases", "shop", "20261016-120000"]);
    assert!(!out.status.success());
}

#[test]
fn helper_panel_domain() {
    let run = |args: &[&str]| {
        Command::new("bash")
            .arg(helper())
            .args(args)
            .output()
            .unwrap()
    };
    for (d, e) in [
        ("", "a@b.co"),
        ("panel", "a@b.co"),
        ("Panel.Example.com", "a@b.co"),
        ("panel.example.com;id", "a@b.co"),
        ("panel.example.com", "not-an-email"),
        ("panel.example.com", ""),
    ] {
        assert!(
            !run(&["panel-domain-set", d, e]).status.success(),
            "{d} {e} accepted"
        );
    }
    // 만들어지는 설정: 인증서 전에는 80(ACME)만, 뒤에는 443 + 패널 API 프록시
    let tmp = std::env::temp_dir().join(format!("rpanel-pd-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("rpanel-shop.conf"),
        "server {\n    server_name shop.example.com www.shop.example.com;\n}\n",
    )
    .unwrap();
    let script = |body: &str| {
        Command::new("bash")
            .arg("-c")
            .arg(format!(
                "source '{}'; AVAIL='{}'; {body}",
                helper(),
                tmp.display()
            ))
            .output()
            .unwrap()
    };
    let out = script("panel_domain_conf panel.example.com");
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.contains("server_name panel.example.com;")
            && s.contains("acme-challenge")
            && !s.contains("443"),
        "{s}"
    );
    let out = script("panel_domain_conf panel.example.com /etc/letsencrypt/live/rpanel-panel-domain/fullchain.pem /k.pem");
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.contains("listen 443 ssl http2;")
            && s.contains("proxy_pass http://127.0.0.1:8090;")
            && s.contains("return 301 https://$host$request_uri;"),
        "{s}"
    );
    assert!(!script("panel_domain_conflict www.shop.example.com")
        .status
        .success());
    assert!(script("panel_domain_conflict panel.example.com")
        .status
        .success());
    assert!(
        script("panel_domain_conflict shop.example")
            .status
            .success(),
        "no partial match"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn helper_db_engine_and_panel_access() {
    let run = |args: &[&str], env: Option<&std::path::Path>| {
        let mut c = Command::new("bash");
        c.arg(helper()).args(args);
        if let Some(p) = env {
            c.env("RPANEL_TEST_PANEL_ALLOW", p);
        }
        c.output().unwrap()
    };
    for (e, v) in [
        ("mariadb", "10.6"),
        ("mariadb", "11.4;id"),
        ("mysql", "8.0"),
        ("mongodb", "7.0"),
        ("redis", "6"),
        ("", ""),
    ] {
        let out = run(&["db-engine-upgrade", e, v], None);
        assert!(!out.status.success(), "{e} {v} accepted");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("고를 수 없는 버전"),
            "{e} {v}: validation must run first"
        );
    }
    let f = std::env::temp_dir().join(format!("rpanel-allow-{}.conf", std::process::id()));
    for bad in [
        "",
        "1.2.3.4;deny all",
        "300.1.1.1",
        "10.0.0.0/33",
        "abc",
        "1.2.3.4 5.6.7.8",
        "::1/129",
    ] {
        assert!(
            !run(&["panel-access", bad], Some(&f)).status.success(),
            "{bad} accepted"
        );
    }
    assert!(run(
        &["panel-access", "203.0.113.5,10.0.0.0/24,2001:db8::/32"],
        Some(&f)
    )
    .status
    .success());
    let s = std::fs::read_to_string(&f).unwrap();
    assert!(s.starts_with("# managed by rpanel"), "{s}");
    assert!(s.contains("allow 203.0.113.5;\nallow 10.0.0.0/24;\nallow 2001:db8::/32;\nallow 127.0.0.1;\ndeny all;"), "{s}");
    assert!(run(&["panel-access", "any"], Some(&f)).status.success());
    let s = std::fs::read_to_string(&f).unwrap();
    assert!(!s.contains("deny"), "{s}");
    let _ = std::fs::remove_file(&f);
}
