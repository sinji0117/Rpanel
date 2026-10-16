-- Rust 전용 패널로 정리: PHP·Node.js 실행 지원 제거. 혹시 남은 PHP 사이트는 정적(HTML) 사이트로 바꿉니다.
UPDATE apps SET kind = 'static' WHERE kind = 'php';

ALTER TABLE apps
    DROP COLUMN IF EXISTS php_version,
    DROP COLUMN IF EXISTS php_memory_mb,
    DROP COLUMN IF EXISTS php_upload_mb,
    DROP COLUMN IF EXISTS php_max_exec,
    DROP COLUMN IF EXISTS php_max_children;

-- Rust 전용: Node.js 실행 앱 종류도 제거 (남아 있으면 Rust 실행 파일 사이트로 바꿔 둠 — 시작 명령은 그대로)
UPDATE apps SET kind = 'rust' WHERE kind = 'node';
