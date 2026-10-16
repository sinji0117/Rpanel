-- 사이트 계정(SSH·SFTP) 여러 개, 관리자 마지막 로그인

-- access: ssh | sftp , enabled=0 이면 접속 잠금
CREATE TABLE site_accounts (
    id          BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    app_id      BIGINT UNSIGNED NOT NULL,
    username    VARCHAR(32)     NOT NULL,
    is_primary  TINYINT(1)      NOT NULL DEFAULT 0,
    access      VARCHAR(8)      NOT NULL DEFAULT 'ssh',
    enabled     TINYINT(1)      NOT NULL DEFAULT 1,
    ssh_keys    TEXT            NULL,
    note        VARCHAR(200)    NULL,
    created_at  DATETIME(3)     NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    updated_at  DATETIME(3)     NOT NULL DEFAULT CURRENT_TIMESTAMP(3) ON UPDATE CURRENT_TIMESTAMP(3),
    UNIQUE KEY uq_site_accounts_user (username),
    KEY ix_site_accounts_app (app_id),
    CONSTRAINT fk_site_accounts_app FOREIGN KEY (app_id) REFERENCES apps(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- 이미 있는 사이트의 기본 계정(사이트 유저)
INSERT INTO site_accounts (app_id, username, is_primary)
SELECT id, site_user, 1 FROM apps WHERE site_user IS NOT NULL;

ALTER TABLE users ADD COLUMN last_login_at DATETIME(3) NULL;
