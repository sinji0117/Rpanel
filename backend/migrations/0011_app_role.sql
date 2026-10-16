-- Static(프런트) 앱의 용도: front = 사용자 화면, admin = 관리자 화면 (프로젝트 화면에서 묶는 기준)
ALTER TABLE apps ADD COLUMN app_role VARCHAR(16) NULL;
UPDATE apps SET app_role = IF(LOWER(CONCAT(name, ' ', slug, ' ', domains)) LIKE '%admin%', 'admin', 'front')
 WHERE kind = 'static' AND source = 'git';
