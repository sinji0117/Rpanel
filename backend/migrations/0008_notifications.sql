-- 알림 (패널 안 알림 목록) + 패널 설정 (키·값)
CREATE TABLE notifications (
  id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
  kind VARCHAR(32) NOT NULL,
  -- info | success | warning | error
  level VARCHAR(10) NOT NULL DEFAULT 'info',
  -- 한국어 원문 템플릿 ({app} 같은 자리표시) — 화면에서 번역 후 params 로 채움
  title VARCHAR(200) NOT NULL,
  params JSON NULL,
  body TEXT NULL,
  link VARCHAR(200) NULL,
  read_at DATETIME(3) NULL,
  created_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
  KEY idx_unread (read_at, id)
);

CREATE TABLE panel_settings (
  k VARCHAR(64) PRIMARY KEY,
  v MEDIUMTEXT NOT NULL,
  updated_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3) ON UPDATE CURRENT_TIMESTAMP(3)
);
