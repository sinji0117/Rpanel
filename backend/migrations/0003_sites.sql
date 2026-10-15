-- CloudPanel 방식 사이트: 파일 기반(HTML·PHP·Node) / 리버스 프록시, 사이트 유저, SSL

ALTER TABLE apps
    MODIFY project_id BIGINT UNSIGNED NULL,
    MODIFY git_url VARCHAR(500) NULL,
    -- git: 저장소에서 빌드 · files: 사이트 유저 홈의 파일을 그대로 서비스 · none: 파일 없음(프록시)
    ADD COLUMN source             VARCHAR(8)    NOT NULL DEFAULT 'git',
    ADD COLUMN site_user          VARCHAR(32)   NULL,
    -- /home/<site_user>/htdocs/<도메인>
    ADD COLUMN site_root          VARCHAR(255)  NULL,
    -- site_root 아래에서 실제로 서비스할 하위 폴더 (예: public). 빈 문자열이면 site_root
    ADD COLUMN web_root           VARCHAR(200)  NOT NULL DEFAULT '',
    ADD COLUMN php_version        VARCHAR(8)    NULL,
    ADD COLUMN php_memory_mb      INT UNSIGNED  NOT NULL DEFAULT 256,
    ADD COLUMN php_upload_mb      INT UNSIGNED  NOT NULL DEFAULT 64,
    ADD COLUMN php_max_exec       INT UNSIGNED  NOT NULL DEFAULT 60,
    ADD COLUMN php_max_children   INT UNSIGNED  NOT NULL DEFAULT 10,
    ADD COLUMN proxy_target       VARCHAR(300)  NULL,
    -- none | letsencrypt | custom
    ADD COLUMN ssl_mode           VARCHAR(16)   NOT NULL DEFAULT 'none',
    ADD COLUMN force_https        TINYINT(1)    NOT NULL DEFAULT 0,
    ADD COLUMN client_max_body_mb INT UNSIGNED  NOT NULL DEFAULT 64,
    ADD UNIQUE KEY uq_apps_site_user (site_user);
