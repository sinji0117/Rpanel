-- 프로젝트용 MongoDB 데이터베이스와 계정 (MariaDB 의 app_databases / db_users 와 같은 구조)
CREATE TABLE mongo_databases (
  id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
  app_id BIGINT UNSIGNED NULL,
  name VARCHAR(63) NOT NULL UNIQUE,
  username VARCHAR(63) NOT NULL,
  password VARCHAR(128) NOT NULL,
  env_key VARCHAR(64) NOT NULL DEFAULT 'MONGODB_URL',
  created_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
  KEY ix_mongo_app (app_id),
  CONSTRAINT fk_mongo_app FOREIGN KEY (app_id) REFERENCES apps(id) ON DELETE SET NULL
);

CREATE TABLE mongo_users (
  id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
  database_id BIGINT UNSIGNED NOT NULL,
  username VARCHAR(63) NOT NULL,
  password VARCHAR(128) NOT NULL,
  -- readWrite | read
  role VARCHAR(16) NOT NULL DEFAULT 'readWrite',
  created_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
  UNIQUE KEY uq_mongo_user (database_id, username),
  CONSTRAINT fk_mongo_users_db FOREIGN KEY (database_id) REFERENCES mongo_databases(id) ON DELETE CASCADE
);

-- 백업: MongoDB 데이터베이스 백업 연결
ALTER TABLE backups ADD COLUMN mongo_id BIGINT UNSIGNED NULL;
