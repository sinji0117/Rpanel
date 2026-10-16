-- Rust·Python Git 앱 안의 프런트엔드(React·Vue 등) 폴더 — 배포 때 npm 으로 먼저 빌드
ALTER TABLE apps ADD COLUMN frontend_dir VARCHAR(200) NULL;
