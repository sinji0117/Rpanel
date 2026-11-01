-- 관리자 2단계 인증 (TOTP)
ALTER TABLE users ADD COLUMN totp_secret VARCHAR(64) NULL;
ALTER TABLE users ADD COLUMN totp_enabled TINYINT(1) NOT NULL DEFAULT 0;
-- 복구 코드: SHA-256 해시의 JSON 배열 (쓴 코드는 빠짐)
ALTER TABLE users ADD COLUMN totp_recovery TEXT NULL;
