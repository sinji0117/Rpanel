-- 사이트 화면의 Content-Security-Policy: off | report (Report-Only) | enforce
ALTER TABLE apps ADD COLUMN csp_mode VARCHAR(16) NOT NULL DEFAULT 'off';
ALTER TABLE apps ADD COLUMN csp_policy VARCHAR(2000) NULL;
