-- DB·Redis 계정 관리, 백업 예약(여러 개), Python 앱

-- 앱에 연결하지 않은 독립 데이터베이스 허용
ALTER TABLE app_databases MODIFY app_id BIGINT UNSIGNED NULL;

-- 데이터베이스에 추가로 만든 계정 (기본 계정은 app_databases.username)
-- privileges: all | read
CREATE TABLE db_users (
    id          BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    database_id BIGINT UNSIGNED NOT NULL,
    username    VARCHAR(32)     NOT NULL,
    password    VARCHAR(128)    NOT NULL,
    privileges  VARCHAR(8)      NOT NULL DEFAULT 'all',
    created_at  DATETIME(3)     NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    UNIQUE KEY uq_db_users_name (username),
    KEY ix_db_users_db (database_id),
    CONSTRAINT fk_db_users_db FOREIGN KEY (database_id) REFERENCES app_databases(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Redis ACL 계정
CREATE TABLE redis_users (
    id          BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    username    VARCHAR(32)     NOT NULL,
    password    VARCHAR(128)    NOT NULL,
    key_pattern VARCHAR(100)    NOT NULL,
    readonly    TINYINT(1)      NOT NULL DEFAULT 0,
    db_index    INT UNSIGNED    NOT NULL DEFAULT 0,
    note        VARCHAR(200)    NULL,
    created_at  DATETIME(3)     NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    UNIQUE KEY uq_redis_users_name (username)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- 백업 예약 — target: app | database | all ,  schedule: hourly | daily | weekly
CREATE TABLE backup_jobs (
    id          BIGINT UNSIGNED  NOT NULL AUTO_INCREMENT PRIMARY KEY,
    name        VARCHAR(100)     NOT NULL,
    target      VARCHAR(16)      NOT NULL,
    app_id      BIGINT UNSIGNED  NULL,
    database_id BIGINT UNSIGNED  NULL,
    schedule    VARCHAR(16)      NOT NULL DEFAULT 'daily',
    hour        TINYINT UNSIGNED NOT NULL DEFAULT 3,
    minute      TINYINT UNSIGNED NOT NULL DEFAULT 0,
    weekday     TINYINT UNSIGNED NOT NULL DEFAULT 0,
    keep_count  INT UNSIGNED     NOT NULL DEFAULT 7,
    enabled     TINYINT(1)       NOT NULL DEFAULT 1,
    last_run_at DATETIME(3)      NULL,
    created_at  DATETIME(3)      NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    updated_at  DATETIME(3)      NOT NULL DEFAULT CURRENT_TIMESTAMP(3) ON UPDATE CURRENT_TIMESTAMP(3),
    KEY ix_backup_jobs_app (app_id),
    KEY ix_backup_jobs_db (database_id),
    CONSTRAINT fk_backup_jobs_app FOREIGN KEY (app_id) REFERENCES apps(id) ON DELETE CASCADE,
    CONSTRAINT fk_backup_jobs_db FOREIGN KEY (database_id) REFERENCES app_databases(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- 백업 파일: 독립 DB 백업(app_id 없음)과 예약 출처
ALTER TABLE backups
    MODIFY app_id BIGINT UNSIGNED NULL,
    ADD COLUMN database_id BIGINT UNSIGNED NULL,
    ADD COLUMN job_id      BIGINT UNSIGNED NULL,
    ADD KEY ix_backups_job (job_id, id),
    ADD KEY ix_backups_db (database_id, id);

-- 기존 앱별 예약(매일/매주)을 백업 예약으로 옮김
INSERT INTO backup_jobs (name, target, app_id, schedule, hour, keep_count)
SELECT CONCAT(name, ' 자동 백업'), 'app', id, backup_schedule, 3, GREATEST(backup_keep, 1)
FROM apps WHERE backup_schedule IN ('daily', 'weekly');
UPDATE apps SET backup_schedule = 'off';
