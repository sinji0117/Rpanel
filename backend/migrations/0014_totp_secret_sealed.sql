-- 2단계 인증 비밀키를 암호화해 저장하므로 길이를 늘림 ("v1:" + hex, 약 130자)
ALTER TABLE users MODIFY COLUMN totp_secret VARCHAR(255) NULL;
