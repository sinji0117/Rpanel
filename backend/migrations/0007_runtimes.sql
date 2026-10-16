-- 언어 런타임 버전 (Rust 툴체인 · Python · Node.js)
CREATE TABLE runtimes (
  id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
  lang VARCHAR(8) NOT NULL,
  version VARCHAR(32) NOT NULL,
  -- installing | ready | failed
  status VARCHAR(12) NOT NULL DEFAULT 'installing',
  is_default TINYINT(1) NOT NULL DEFAULT 0,
  -- 서버에 원래 있던 것 (지울 수 없음)
  is_system TINYINT(1) NOT NULL DEFAULT 0,
  -- 실제 버전 (예: 3.12.7, v22.11.0, 1.90.0)
  detail VARCHAR(80) NOT NULL DEFAULT '',
  log TEXT NULL,
  created_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
  updated_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3) ON UPDATE CURRENT_TIMESTAMP(3),
  UNIQUE KEY uq_runtime (lang, version)
);

-- 앱별 버전 (NULL = 기본 버전)
ALTER TABLE apps ADD COLUMN runtime_version VARCHAR(32) NULL;
