-- Rpanel 초기 스키마 (MariaDB 10.11 LTS / MySQL 8.x 호환)
-- 모든 시각은 UTC (커넥션 time_zone = '+00:00')

CREATE TABLE users (
    id            BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    email         VARCHAR(190)    NOT NULL,
    password_hash VARCHAR(255)    NOT NULL,
    role          VARCHAR(16)     NOT NULL DEFAULT 'admin',
    created_at    DATETIME(3)     NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    updated_at    DATETIME(3)     NOT NULL DEFAULT CURRENT_TIMESTAMP(3) ON UPDATE CURRENT_TIMESTAMP(3),
    UNIQUE KEY uq_users_email (email)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE projects (
    id          BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    name        VARCHAR(100)    NOT NULL,
    description VARCHAR(500)    NOT NULL DEFAULT '',
    created_at  DATETIME(3)     NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    updated_at  DATETIME(3)     NOT NULL DEFAULT CURRENT_TIMESTAMP(3) ON UPDATE CURRENT_TIMESTAMP(3),
    UNIQUE KEY uq_projects_name (name)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- kind: rust | static | node
--   rust   : cargo build --release → systemd 서비스, Nginx가 포트로 프록시
--   static : npm ci && npm run build → output_dir을 Nginx가 정적 서비스 (SPA), api_proxy_port가 있으면 /api/ 프록시
--   node   : npm ci && (build) → start_cmd를 systemd로 실행, Nginx가 포트로 프록시
CREATE TABLE apps (
    id              BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    project_id      BIGINT UNSIGNED NOT NULL,
    name            VARCHAR(100)    NOT NULL,
    slug            VARCHAR(40)     NOT NULL,
    kind            VARCHAR(16)     NOT NULL,
    git_url         VARCHAR(500)    NOT NULL,
    git_branch      VARCHAR(100)    NOT NULL DEFAULT 'main',
    root_dir        VARCHAR(200)    NOT NULL DEFAULT '',
    build_cmd       VARCHAR(500)    NULL,
    start_cmd       VARCHAR(500)    NULL,
    binary_name     VARCHAR(100)    NULL,
    output_dir      VARCHAR(200)    NOT NULL DEFAULT 'dist',
    port            INT UNSIGNED    NULL,
    api_proxy_port  INT UNSIGNED    NULL,
    domains         VARCHAR(1000)   NOT NULL DEFAULT '',
    health_path     VARCHAR(200)    NOT NULL DEFAULT '/',
    status          VARCHAR(16)     NOT NULL DEFAULT 'idle',
    current_commit  VARCHAR(64)     NULL,
    current_release BIGINT UNSIGNED NULL,
    webhook_token   VARCHAR(64)     NOT NULL,
    created_at      DATETIME(3)     NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    updated_at      DATETIME(3)     NOT NULL DEFAULT CURRENT_TIMESTAMP(3) ON UPDATE CURRENT_TIMESTAMP(3),
    UNIQUE KEY uq_apps_slug (slug),
    UNIQUE KEY uq_apps_port (port),
    KEY ix_apps_project (project_id),
    CONSTRAINT fk_apps_project FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE app_env (
    id      BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    app_id  BIGINT UNSIGNED NOT NULL,
    `key`   VARCHAR(128)    NOT NULL,
    value   TEXT            NOT NULL,
    UNIQUE KEY uq_app_env (app_id, `key`),
    CONSTRAINT fk_env_app FOREIGN KEY (app_id) REFERENCES apps(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- status: queued | running | success | failed | cancelled
CREATE TABLE deployments (
    id           BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    app_id       BIGINT UNSIGNED NOT NULL,
    status       VARCHAR(16)     NOT NULL DEFAULT 'queued',
    triggered_by BIGINT UNSIGNED NULL,
    commit_sha   VARCHAR(64)     NULL,
    log          MEDIUMTEXT      NULL,
    error        VARCHAR(1000)   NULL,
    started_at   DATETIME(3)     NULL,
    finished_at  DATETIME(3)     NULL,
    created_at   DATETIME(3)     NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    KEY ix_deploy_app (app_id, id),
    CONSTRAINT fk_deploy_app FOREIGN KEY (app_id) REFERENCES apps(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE audit_logs (
    id         BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    user_id    BIGINT UNSIGNED NULL,
    action     VARCHAR(64)     NOT NULL,
    target     VARCHAR(255)    NOT NULL DEFAULT '',
    ip         VARCHAR(64)     NOT NULL DEFAULT '',
    created_at DATETIME(3)     NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    KEY ix_audit_created (created_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
