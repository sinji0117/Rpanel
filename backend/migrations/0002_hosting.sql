-- 호스팅 기능: DB·Redis 관리, 백업, 크론

ALTER TABLE apps
    ADD COLUMN redis_db              INT UNSIGNED NULL,
    ADD COLUMN backup_schedule       VARCHAR(16)  NOT NULL DEFAULT 'off',
    ADD COLUMN backup_keep           INT UNSIGNED NOT NULL DEFAULT 7,
    ADD COLUMN backup_last_scheduled DATETIME(3)  NULL,
    ADD UNIQUE KEY uq_apps_redis_db (redis_db);

-- 패널이 만든 MariaDB 데이터베이스 (앱에 소속)
CREATE TABLE app_databases (
    id         BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    app_id     BIGINT UNSIGNED NOT NULL,
    name       VARCHAR(64)     NOT NULL,
    username   VARCHAR(32)     NOT NULL,
    password   VARCHAR(128)    NOT NULL,
    env_key    VARCHAR(128)    NOT NULL DEFAULT 'DATABASE_URL',
    created_at DATETIME(3)     NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    UNIQUE KEY uq_db_name (name),
    UNIQUE KEY uq_db_user (username),
    UNIQUE KEY uq_db_env (app_id, env_key),
    CONSTRAINT fk_db_app FOREIGN KEY (app_id) REFERENCES apps(id) ON DELETE RESTRICT
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- kind: db | data ,  status: running | success | failed
CREATE TABLE backups (
    id          BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    app_id      BIGINT UNSIGNED NOT NULL,
    kind        VARCHAR(8)      NOT NULL,
    target      VARCHAR(64)     NOT NULL,
    file        VARCHAR(200)    NOT NULL,
    size_bytes  BIGINT UNSIGNED NOT NULL DEFAULT 0,
    status      VARCHAR(16)     NOT NULL DEFAULT 'running',
    error       VARCHAR(1000)   NULL,
    trigger_by  VARCHAR(16)     NOT NULL DEFAULT 'manual',
    created_at  DATETIME(3)     NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    finished_at DATETIME(3)     NULL,
    KEY ix_backups_app (app_id, id),
    CONSTRAINT fk_backup_app FOREIGN KEY (app_id) REFERENCES apps(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- systemd timer 로 실행되는 앱별 예약 작업
CREATE TABLE cron_jobs (
    id         BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    app_id     BIGINT UNSIGNED NOT NULL,
    name       VARCHAR(100)    NOT NULL,
    schedule   VARCHAR(100)    NOT NULL,
    command    VARCHAR(500)    NOT NULL,
    enabled    TINYINT(1)      NOT NULL DEFAULT 1,
    created_at DATETIME(3)     NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    updated_at DATETIME(3)     NOT NULL DEFAULT CURRENT_TIMESTAMP(3) ON UPDATE CURRENT_TIMESTAMP(3),
    KEY ix_cron_app (app_id),
    CONSTRAINT fk_cron_app FOREIGN KEY (app_id) REFERENCES apps(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
