# Rpanel 기술 문서

처음 쓰는 분은 [README](../README.md)와 [사용 설명서](README.md)부터 보세요. 이 문서는 설치 옵션, 구조, 보안 설계, 개발 방법을 다룹니다.

## 구성

| 계층 | 기술 |
|---|---|
| 백엔드 | Rust + Axum 0.8 (Tokio) |
| DB | SQLx (`mysql` 기능) + MariaDB 10.11 LTS |
| 캐시·세션 | Redis 7.x |
| 로그 저장소 | MongoDB 8 (선택, `MONGODB_URL`) — 없으면 MariaDB·Redis |
| 프런트 | React 19 + Vite + Tailwind CSS 4 |
| 인증 | JWT(액세스, 15분) + Redis 리프레시 세션(회전·재사용 탐지) |
| 실시간 | Axum WebSocket — 서버 지표, 배포 로그 스트림 |
| 테스트 | `cargo test` · Vitest · Playwright |

```
 브라우저 ──▶ :8443 (HTTPS, Nginx) ─┬─ /        → 패널 UI (React)
                                    └─ /api, ws → 127.0.0.1:8090 (rpanel, 일반 사용자 권한)
                                                   │ sudo rpanel-helper  (검증된 시스템 작업만)
                                                   │ sudo -u <사이트유저> rpanel-files  (사이트 파일만)
 방문자 ──▶ :80 / :443 (Nginx, 사이트별 설정) ─┬─ Rust·Python 앱 → 127.0.0.1:81xx (systemd 서비스)
                                               ├─ HTML 사이트   → /home/<사이트유저>/htdocs/<도메인>
                                               └─ 리버스 프록시  → 지정한 주소
```

## 설치 옵션

| 모드 | 언제 | 하는 일 |
|---|---|---|
| `--mode=standalone` (기본) | 서버가 인터넷에서 80/443 을 직접 받을 때 (VPS, 공인 IP 포트포워딩) | 사이트마다 Let's Encrypt 발급, 80·443 개방 |
| `--mode=proxy --trusted-proxy=<IP>` | 앞단에 리버스 프록시(Nginx Proxy Manager, Cloudflare Tunnel 등)가 SSL 을 처리할 때 | 실제 접속자 IP 복원, 80 은 프록시에서만 허용, SSL 은 프록시에서 |

옵션은 한 줄 설치 뒤에 붙입니다: `curl … | sudo bash -s -- --mode=proxy --trusted-proxy=10.0.0.10`

고른 옵션은 `/etc/rpanel/rpanel.env`(`RPANEL_ALLOW_FROM`, `RPANEL_FIREWALL` …)에 저장되고, `rpanel-update` 때 그대로 다시 적용됩니다. 바꾸려면 `sudo rpanel-update --force --allow-from=…` 처럼 옵션을 붙입니다.

## 설치 · 업데이트 · 삭제 스크립트

| 스크립트 | 설치 후 명령 | 하는 일 |
|---|---|---|
| `install.sh` | — | 소스를 `/opt/rpanel-src` 에 받고 `deploy/install.sh` 실행. 저장소 안에서 `sudo ./install.sh` 로 실행하면 그 소스를 그대로 사용 |
| `update.sh` | `sudo rpanel-update` | 새 커밋 확인(`--check`) → 패널 DB 덤프·현재 바이너리 보관(`/var/lib/rpanel/backups/_panel`, DB 덤프 최근 5개) → 새 소스의 `update.sh` 로 이어서 빌드·설치 → 실패 시 바이너리·UI 복원, 마이그레이션이 진행됐다면 DB 도 복원 |
| `uninstall.sh` | `sudo rpanel-uninstall` | `--keep-data`(패널·사이트 설정만 제거, 데이터·설정 보존) / `--purge`(사이트 유저·관리 DB·백업·인증서·`/var/lib/rpanel`·`/etc/rpanel` 까지) · `--yes` 로 질문 생략. 기본 패키지는 지우지 않음 |
| `deploy/install.sh` | — | 실제 설치 엔진 (여러 번 실행해도 안전, 비밀값 유지) |

환경변수 `RPANEL_REPO`·`RPANEL_BRANCH`·`RPANEL_SRC` 로 저장소(포크)·브랜치·소스 위치를 바꿀 수 있습니다.

| 옵션 | 설명 |
|---|---|
| `--allow-from=CIDR,…` | SSH(22)·패널(8443) 접속 허용 대역 (기본: 어디서나, `any` 로 되돌림) — 예: `10.0.0.0/24,100.64.0.0/10` |
| `--panel-domain=panel.example.com --email=you@example.com` | 패널을 도메인으로 접속 + Let's Encrypt 인증서 (standalone) |
| `--no-firewall` / `--firewall` | ufw 를 건드리지 않음 / 다시 관리 |
| `--no-ssh-password` / `--ssh-password` | 사이트 유저의 SSH/SFTP 비밀번호 로그인 끄기(공개키만) / 다시 켜기 |
| `--no-mongodb` / `--mongodb` | MongoDB 설치 안 함 / 다시 설치 (CPU AVX 필요, 없으면 자동으로 건너뜀) |
| `--dry-run-mode` | 실제 배포 명령 없이 둘러보기 |

## 설치 스크립트가 하는 일

| 단계 | 내용 |
|---|---|
| Nginx | 기본 서버는 등록되지 않은 도메인·IP 직접 접속을 끊음(444 / TLS 거부), Let's Encrypt 확인 경로, 갱신 시 자동 reload |
| MariaDB 10.11 | `127.0.0.1` 바인딩, 익명 계정·test DB·원격 root 제거, RAM 기준 튜닝(없을 때만), 패널 DB |
| Redis 7 | `127.0.0.1` 바인딩, 비밀번호 생성(이미 있으면 유지), `maxmemory 1gb` |
| MongoDB 8 | repo.mongodb.org, `bindIp 127.0.0.1`, 인증 필수, 관리자 `rpanel_admin` → `MONGODB_URL` (AVX 없으면 건너뜀) |
| Node.js 22 / Rust / Python | NodeSource(패널 UI·React 정적 앱 빌드용), rustup (Rust 앱 빌드용), `python3` + `python3-venv`·`pip`·`dev` (Python 앱) |
| 패널 | `rpanel`·`rpanel-app` 사용자, `rpanel-sites` 그룹, 빌드·설치, helper + sudoers, systemd, HTTPS :8443 |
| SSH | `rpanel-sites` 그룹만 비밀번호 로그인 허용 (`Match Group`) |
| 비밀값 | `/etc/rpanel/rpanel.env` — DB·Redis 비밀번호, JWT 키 자동 생성 (재실행해도 유지) |
| ufw | 22·8443 ← `--allow-from`, 80·443 ← 어디서나(standalone) / 프록시만(proxy). **지금 SSH 로 접속 중인 주소는 자동 허용** |

이미 MariaDB·Redis 를 쓰고 있는 서버에서 실행해도 기존 데이터와 Redis 비밀번호는 유지되고, 패널용 DB·설정만 추가됩니다.

실행하는 서버 프로그램은 Rust 와 Python 입니다. PHP·Node.js 런타임은 지원하지 않습니다 (Node.js 는 프런트 빌드 도구로만 사용). 다른 언어의 서비스는 다른 서버에 두고 리버스 프록시 사이트로 연결할 수 있습니다.

## 보안 모델

- 패널 프로세스는 **root 가 아닌 `rpanel` 사용자**로 실행됩니다. 패널 UI 는 HTTPS(8443) 로만 서비스되고, `--allow-from` 으로 접근 대역을 제한할 수 있습니다.
- root 가 필요한 작업은 **`/usr/local/lib/rpanel/rpanel-helper` 한 개**만 sudo 로 허용합니다. helper 는 모든 인자와 스테이징 파일을 **화이트리스트**로 다시 검사합니다:
  - Nginx: 허용된 지시어·location 만, `root` 는 해당 사이트 폴더로 고정, 인증서 경로 고정, `include`·`fastcgi_*` 불가, 다른 설정과 도메인 중복 금지, `nginx -t` 실패 시 원복
  - systemd: 실행 사용자 고정(앱은 `rpanel-app`, 사이트는 그 사이트 유저), `NoNewPrivileges` 필수, 경로는 해당 앱/사이트 안
  - 사이트 유저: `rpanel-sites` 그룹, 시스템 계정 이름 거부, 레지스트리(`/etc/rpanel/site-users`)에 있는 계정만 삭제
  - MariaDB: 패널이 만든 DB(`/etc/rpanel/managed-dbs`)·추가 계정(`/etc/rpanel/managed-db-users`)만 삭제·덤프·복원·비밀번호 변경. 추가 계정 권한은 `ALL` 또는 `SELECT, SHOW VIEW` 를 그 DB 에만
  - Redis 계정: ACL 사용자. 이름·키 패턴을 화이트리스트로 검사하고, 비밀번호는 stdin 으로 받아 SHA-256 해시만 `/etc/redis/rpanel-users.conf`(root:redis 0640, redis.conf 가 include)에 저장. 권한은 `+@all -@admin -@dangerous`(읽기·쓰기) 또는 `+@read`(읽기 전용) + 키·채널 패턴 제한. `default` 등 예약 이름 거부
  - 비밀번호(DB 추가 계정·Redis 계정)는 명령줄이 아닌 stdin 으로 전달하고 `[A-Za-z0-9_.~-]{10,64}` 만 허용 (URL·SQL 에 그대로 넣어도 안전)
  - SSL: 인증서와 개인키 일치 검사, 키는 root 0600
  - 사이트 추가 계정: 레지스트리(`/etc/rpanel/site-accounts`)에 있는 계정만 수정·삭제. 주 그룹은 그 사이트 유저 그룹, `htdocs`·`logs` 에 POSIX ACL(`u:<계정>:rwX` + default ACL) 로 쓰기 권한. SSH 공개키는 키 종류로 시작하는 줄만 허용(옵션·제어문자 거부). 비밀번호는 stdin 으로 전달
- **사이트 파일은 그 사이트 유저 권한으로만** 다룹니다 (`sudo -u <사이트유저> rpanel-files`). 홈 디렉토리 밖을 가리키는 경로·심볼릭 링크는 거부합니다.
- 사이트끼리는 리눅스 계정으로 격리됩니다 (홈 0750, 업로드한 Rust 프로그램은 사이트 유저로 실행). Nginx(`www-data`)만 각 사이트 그룹에 들어가 정적 파일을 읽습니다.
- 이 계약은 `backend/tests/helper_contract.rs` 가 생성 템플릿 통과 + 위험한 변형(설정 주입, 경로 탈출, 권한 상승 시도 등)으로 검증합니다.

## 사이트 계정 (SSH · SFTP)

- 사이트마다 기본 계정(사이트 유저) + 추가 계정. DB 테이블 `site_accounts`.
- **SFTP 전용**: 셸 `nologin` + `rpanel-sftp` 그룹. `/etc/ssh/sshd_config.d/49-rpanel-access.conf` 의 `Match Group rpanel-sftp` → `ForceCommand internal-sftp -u 0002`, 포워딩 금지.
- **잠금**: `rpanel-locked` 그룹 + `DenyGroups rpanel-locked`. 기본 계정을 잠가도 사이트 서비스는 그대로 돕니다.
- 추가 계정 홈은 `/home/<계정>`(0700)이고 `htdocs` 심볼릭 링크가 사이트 폴더를 가리킵니다. 셸 umask 002.
- SFTP 클라이언트가 원래 파일 권한(0644 등)을 그대로 보내면 다른 계정이 그 파일을 고칠 수 없습니다. 이럴 땐 패널 파일 관리자로 고치거나 클라이언트에서 권한 유지를 끄세요.
- 설치 스크립트가 `acl` 패키지와 두 그룹, sshd 설정을 준비합니다 (`sshd -t` 검사 후 reload).

## Rust·Python 앱의 프런트엔드 (npm)

- `apps.frontend_dir`(앱 폴더 기준, `..`·절대경로 불가)가 있으면 빌드 단계 전에 그 폴더에서 `npm ci|install` → `npm run build`(Node.js 기본 런타임, `node_modules` 는 빌드 폴더에 남겨 재사용).
- 결과 `<frontend_dir>/<output_dir>` 를 릴리스의 같은 경로로 rsync → 서비스 `WorkingDirectory`(current) 기준 상대 경로가 그대로 맞습니다. Python 릴리스 복사는 `node_modules` 를 제외합니다.

## 언어 런타임 버전

- 테이블 `runtimes(lang, version, status, is_default, is_system, detail, log)`, 앱별 선택은 `apps.runtime_version` (NULL = 기본 버전).
- 처음 조회할 때 서버에 있던 것을 기본으로 등록: Rust `stable`, Python·Node.js `system`(apt 의 python3 / 설치 스크립트의 node).
- **Rust**: 패널 계정(rpanel)의 rustup 으로 `rustup toolchain install <v> --profile minimal`. 빌드할 때 `RUSTUP_TOOLCHAIN=<v>` 로 고릅니다 (`rustup default` 는 그대로 → 패널 자체 업데이트 빌드에 영향 없음).
- **Python**: helper 가 [uv](https://github.com/astral-sh/uv)(SHA-256 확인)로 standalone CPython 을 `/opt/rpanel/runtimes/python` 에 설치하고 `/opt/rpanel/runtimes/bin/python3.X` 링크. 가상환경을 이 실행 파일로 만듭니다. 파일 사이트는 `.venv` 의 버전이 고른 버전과 다르면 새로 만듭니다.
- **Node.js**: helper 가 nodejs.org 의 `node-vX.Y.Z-linux-<arch>.tar.xz` 를 `SHASUMS256.txt` 로 확인한 뒤 `/opt/rpanel/runtimes/node/<major>` 에 풉니다. 빌드 PATH 맨 앞에 `…/node/<major>/bin`.
- `/opt/rpanel/runtimes` 는 root 소유 0755 (사이트 유저도 실행 가능). helper 는 버전 문자열을 `python 3.8~3.20`, `node 18~40` 로만 받습니다.
- 설치는 백그라운드 작업이고 출력은 `runtimes.log` 에 남습니다. 서버 기본·현재 기본·앱이 쓰는 버전은 지울 수 없습니다.

## 버전 확인 (GitHub)

- `GET /api/system/version` → 지금 버전(`CARGO_PKG_VERSION`)과 GitHub 최신 버전. 최신 릴리스 태그를 먼저 보고, 없으면 `main` 브랜치 `backend/Cargo.toml` 의 `version` 을 읽습니다 (curl, 8초 제한, Redis 6시간 캐시, `?refresh=true` 로 즉시 확인).
- 저장소는 `RPANEL_UPDATE_REPO`(기본 `sinji0117/Rpanel`), 빈 값이면 확인하지 않습니다.

## 모니터링 기록 · 로그

- 패널이 1분마다 지표(CPU·메모리·디스크·load)를 Redis 리스트(`<prefix>:metrics:history`, 최대 7일 = 10080개)에 쌓습니다. `GET /api/system/metrics?range=30m|1h|6h|24h|7d` 는 기간 안의 값을 최대 120개로 평균 내어 돌려줍니다.
- 가동 시간은 `GET /api/system/info` 의 `boot_time`(부팅 시각)으로 화면에서 계산합니다 (실시간 연결이 끊겨도 표시).
- `GET /api/logs/system?kind=panel|nginx|ssh|mariadb|redis&lines=` 는 helper `system-log` 로 정해진 로그만 읽습니다. `GET /api/logs/deployments` 는 전체 배포 기록.

## 로그 저장소 (MongoDB)

- `MONGODB_URL` 이 있으면 활동 기록·배포 로그·알림·모니터링 지표를 DB `rpanel_logs` 의 `audit` · `deployment_logs` · `notifications` · `metrics` 컬렉션에 저장합니다 (번호는 `counters` 의 `$inc`). 지표는 TTL 인덱스 + 1시간마다 7일 지난 것 삭제.
- 처음 켤 때 MariaDB·Redis 에 있던 기록을 한 번 옮깁니다 (`panel_settings.logs_migrated_mongo`). MongoDB 에 접속하지 못하면 기존 MariaDB·Redis 저장소로 동작합니다.
- 프로젝트용 MongoDB: 패널이 관리자 연결로 DB 마다 `createUser`(그 DB 의 `readWrite`/`read`) 하고 `MONGODB_URL=mongodb://user:pw@127.0.0.1:27017/db?authSource=db` 를 주입합니다. 백업은 helper `backup-mongo`/`restore-mongo`(mongodump·mongorestore, 접속 주소는 임시 설정 파일로).

## DB 웹 관리

- 관리자 입구 `/api/dbweb/admin/{db_id}/…`: 패널이 만든 DB 의 **기본 계정**으로 접속 (root 아님).
- DB 계정 입구 `POST /api/dbweb/login` → 토큰(Redis, 2시간 유휴 만료) → 헤더 `X-DB-Token` 으로 `/api/dbweb/s/{db}/…`. 로그인은 IP·계정별 5분 10회 제한, `root`·`rpanel` 등은 거부, 시스템 DB 와 패널 DB 는 목록에서 뺍니다.
- 질의는 `max_statement_time=30`, 결과 1000행. 화면의 행 수정·삭제는 기본 키로 `LIMIT 1`. prepared 로 안 되는 문장(오류 1295)은 텍스트 프로토콜로 다시 실행합니다.

## 패널 도메인

- 설정 → **패널 도메인**: helper `panel-domain-set <domain> <email>` 이 `/etc/nginx/sites-available/rpanel-panel-domain.conf` 를 직접 만들고(80: ACME → 발급 → 443 + 패널 프록시), Let's Encrypt `--cert-name rpanel-panel-domain`. 앱 도메인과 겹치면 거부. 해제는 `panel-domain-remove`. 기존 `:8443` 접속은 그대로입니다.

## DB 서버 버전

- `GET /api/db-engines`: MariaDB(`SELECT VERSION()`) · Redis(`INFO server`) · MongoDB(`buildInfo`) 의 지금 버전과 고를 수 있는 계열. 목록 안에서 올리기·내리기 모두 허용하되, 사용자가 만든 DB(app_databases · mongo_databases · Redis 배정·계정)가 있으면 409 (`dbengines::check_target`, `user_db_count`). 내리기는 helper `db-engine-downgrade`(MariaDB: users/DB SQL 덤프 → datadir 이동 → purge·재설치 → 복원 / MongoDB: 패널이 FCV 를 먼저 내림 → 재설치 / Redis: dump.rdb 이동 → 재설치).
- `POST /api/db-engines/{engine}` → 백그라운드로 helper `db-engine-upgrade <engine> <version>`: 전체 백업(`_engines`, mariadb-dump / mongodump / redis SAVE+dump.rdb 복사) → 공식 저장소 추가(MariaDB `dlm.mariadb.com`, MongoDB `repo.mongodb.org`, Redis `packages.redis.io`) → `apt-get install --force-confold` → 재시작 (MariaDB 는 `mariadb-upgrade`, MongoDB 는 패널이 `setFeatureCompatibilityVersion`). 진행 로그는 패널 메모리, 결과는 `panel_settings.db_engine_<engine>`, 알림 `db_engine`.

## 패널 접속 IP

- `GET/PUT /api/system/panel-access` → helper `panel-access <any|CIDR,…>` 가 `/etc/nginx/rpanel-panel-allow.conf`(allow … / deny all)를 쓰고 Nginx reload. 패널 설정(`rpanel-panel.conf`, 패널 도메인)은 이 파일을 `include` 하고, `/api/hooks/` 는 `allow all`. ufw 가 켜져 있으면 패널 포트를 그 대역에 엽니다.
- 지금 요청한 IP 가 목록에 없으면 거부 (잠김 방지). 잠겼을 때: `sudo /usr/local/lib/rpanel/rpanel-helper panel-access any`.

## 알림

- 테이블 `notifications`(제목은 한국어 템플릿 + `params` JSON → 화면에서 번역), 설정은 `panel_settings`(k=`notifications`, JSON).
- 이벤트: 배포 결과(runner `finish`), 백업 결과(실행 단위로 한 번), 런타임 설치, 관리자 로그인, 1분 지표 샘플러의 디스크·메모리 기준(Redis 쿨다운), 5분 사이트 확인(상태가 바뀔 때만, real 모드), 6시간 GitHub 버전 확인(버전마다 한 번).
- 외부 전송은 curl 로 JSON POST. 주소·토큰은 명령줄이 아니라 stdin 설정(`-K -`)으로 넘기고, 오류 메시지의 토큰은 가립니다. Discord·Slack 주소는 공식 도메인만, Telegram 토큰·chat id 는 형식 검사.
- API: `GET/DELETE /api/notifications`, `POST /api/notifications/read`, `GET/PUT /api/notifications/settings`, `POST /api/notifications/test`.

## 관리자 · 화면 설정

- 관리자는 모두 같은 권한입니다. 자기 자신과 마지막 관리자는 지울 수 없고, 비밀번호 변경·삭제 시 그 관리자의 세션을 모두 끊습니다. 로그인 시각은 `users.last_login_at`.
- 테마(낮·밤·자동)와 언어(한국어·English)는 브라우저 `localStorage`(`rpanel.theme`, `rpanel.lang`)에 저장합니다. 번역은 `frontend/src/i18n/en.js` (한국어 원문이 키). 새 문구를 넣으면 `src/i18n/en.test.js` 가 번역 누락을 잡습니다.

## 업로드형 Rust 사이트

1. **사이트 → 사이트 추가 → Rust**, 도메인 입력. 시작 명령 기본값 `./app`
2. 로컬에서 `cargo build --release` — 서버와 같은 아키텍처·같거나 낮은 glibc 로 빌드 (다르면 `cross` 나 `--target x86_64-unknown-linux-musl`)
3. 실행 파일을 문서 루트(`/home/<사이트유저>/htdocs/<도메인>/app`)에 업로드 → **적용**
   - 실행 파일이 아직 없으면 Nginx 만 설정하고 서비스는 띄우지 않습니다 (상태 `idle`)
   - 적용할 때 `chmod u+x` 를 자동으로 하고, systemd 서비스(사이트 유저, `NoNewPrivileges`, `MemoryDenyWriteExecute`)로 실행한 뒤 헬스체크
4. 프로그램은 `PORT` 환경변수의 포트로 127.0.0.1 에서 열어야 합니다 (`BIND_ADDR=127.0.0.1:<포트>` 도 함께 들어감). 데이터베이스·Redis 를 만들면 접속 URL 이 환경변수로 자동 추가됩니다.

## Python 앱

- **파일 사이트**: 사이트 추가 → Python. 문서 루트에 예시 `app.py`·`requirements.txt`·`README.txt` 를 만듭니다. 적용할 때마다 사이트 유저 권한으로 `<고른 Python> -m venv .venv`(없거나 버전이 다를 때만) → `.venv/bin/pip install -r requirements.txt` → (선택) 빌드 명령 → systemd 서비스 → 헬스체크.
- **Git 앱**: 종류 Python. 소스를 릴리스 폴더로 복사한 뒤 그 안에 새 가상환경을 만들고 `requirements.txt` 를 설치합니다(`PIP_CACHE_DIR=/var/lib/rpanel/cache/pip` 재사용). 릴리스마다 가상환경이 따로라 롤백해도 안전합니다.
- 서비스·크론의 `PATH` 맨 앞에 `.venv/bin` 이 들어가므로 시작 명령에 `python`, `gunicorn`, `uvicorn` 을 그대로 씁니다. 기본 시작 명령은 `python app.py`.
- Python 서비스에는 `MemoryDenyWriteExecute` 를 걸지 않습니다 (cffi 등 확장 모듈 호환). 나머지 격리 설정은 Rust 와 같습니다.

## 백업 예약

- `backup_jobs`: 대상 `app`(연결된 DB + 사이트 파일/data) · `database`(DB 하나) · `all`(모든 앱 + 앱에 연결하지 않은 DB), 주기 `hourly`·`daily`·`weekly`(분·시·요일), 보관 개수, 켜기/끄기.
- 패널 안 스케줄러가 1분마다 확인합니다. 마지막 실행 이후 예약 시각이 지났고 그 시각에서 6시간이 넘지 않았으면 실행합니다 (패널이 잠깐 꺼져 있었던 경우 따라잡기, 오래 꺼져 있었으면 다음 예약까지 대기). 방금 만든 예약은 지난 시각으로 바로 돌지 않습니다.
- 보관 개수는 그 예약이 만든 백업에만 대상(앱·DB·종류)별로 적용됩니다. 직접 만든 백업은 지우기 전까지 남습니다.
- 앱에 연결하지 않은 DB 의 백업은 `/var/lib/rpanel/backups/_databases/` 에 저장합니다.

## Git 앱 예시 — Rust API + React 프런트
1. **프로젝트** 생성 → 앱 1 **Rust**: Git 저장소, 바이너리 이름, 헬스체크 `/api/health` (도메인 비움)
2. **Git · 웹훅 탭** → Deploy key 를 저장소에 등록 → **배포**
3. 앱 2 **Static**: 프런트 저장소, 도메인, **/api/ 프록시 포트 = 앱 1 포트** → **배포**
4. Webhook URL·토큰을 GitLab/GitHub 에 등록하면 push 할 때마다 자동 배포

## 디렉토리 구조

```
/usr/local/bin/rpanel                      패널
/usr/local/lib/rpanel/rpanel-helper        권한 작업 helper (root 0755)
/usr/local/lib/rpanel/rpanel-files         사이트 파일 도구 (사이트 유저로 실행)
/usr/share/rpanel/www/                     패널 UI
/etc/rpanel/rpanel.env                     설정·비밀값 (root:rpanel 0640)
/etc/rpanel/site-users, managed-dbs, managed-db-users   helper 레지스트리
/etc/redis/rpanel-users.conf               Redis 계정(ACL, 비밀번호 해시)
/etc/rpanel/certs/<slug>/                  직접 올린 인증서 (root 0600)
/etc/letsencrypt/live/rpanel-<slug>/       Let's Encrypt 인증서
/etc/nginx/conf.d/rpanel.conf              기본 서버(444) · 공통 map
/etc/nginx/sites-available/rpanel-<slug>.conf
/etc/systemd/system/rpanel-app-<slug>.service, rpanel-cron-<slug>-<id>.{service,timer}
/opt/rpanel-src/                           설치·업데이트용 소스
/usr/local/sbin/rpanel-update, rpanel-uninstall

/home/<사이트유저>/htdocs/<도메인>/        사이트 파일
/home/<사이트유저>/logs/
/var/lib/rpanel/
  builds/<slug>/repo                       Git 앱 체크아웃·빌드 캐시
  apps/<slug>/releases/<배포id>, current   Git 앱 릴리스 (최근 3개)
  apps/<slug>/app.env                      환경변수 (0600)
  www/<slug>/current                       Git 정적 앱
  backups/<slug>/                          백업 (앱을 삭제해도 남김)
  backups/_panel/                          업데이트 전 패널 DB 덤프·이전 바이너리
  backups/_databases/                      앱에 연결하지 않은 DB 의 백업
  cache/pip/                               Git Python 앱 pip 캐시
/home/<사이트유저>/htdocs/<도메인>/.venv/    Python 사이트 가상환경
  acme/                                    Let's Encrypt 확인 경로
```

## 개발

```bash
# MariaDB + Redis 준비 후
cd backend
DATABASE_URL=mysql://rpanel:pw@127.0.0.1:3306/rpanel \
REDIS_URL=redis://:pw@127.0.0.1:6379/10 \
JWT_SECRET=$(openssl rand -hex 32) \
RPANEL_EXEC_MODE=dry-run RPANEL_DATA_DIR=/tmp/rpanel-data \
cargo run

cd frontend && npm install && npm run dev     # http://localhost:5173 (/api → 127.0.0.1:8090 프록시)
```

### 테스트

```bash
cd backend && cargo test                       # 단위 + helper 계약 + rpanel-files

# 통합 테스트 (실제 MariaDB/Redis, dry-run 실행기, WebSocket 포함) — ⚠ 테스트 DB 내용을 지웁니다
RPANEL_TEST_DATABASE_URL=mysql://user:pw@127.0.0.1:3306/rpanel_test \
RPANEL_TEST_REDIS_URL=redis://:pw@127.0.0.1:6379/15 \
cargo test --test api

cd frontend && npm test                        # Vitest
npx playwright install chromium && npm run e2e # E2E (가짜 API, 백엔드 불필요)
```

## API 요약

| 메서드 | 경로 | 설명 |
|---|---|---|
| GET | `/api/health` | DB·Redis 상태 |
| GET/POST | `/api/auth/status`, `/setup`, `/login`, `/refresh`, `/logout`, `/me`, `/password`, `/ws-ticket` | 인증 |
| GET/POST | `/api/sites` | 사이트 목록(응답 확인) / 만들기 (`type`: rust·python·html·proxy) |
| GET/POST/PUT/DELETE | `/api/apps/{id}/accounts`, `/accounts/{aid}`, POST `/accounts/{aid}/password` | 사이트 계정(SSH·SFTP) |
| GET/POST/PUT/DELETE | `/api/users`, `/api/users/{id}` | 관리자 |
| GET/POST/PUT/DELETE | `/api/apps/{id}/ssl`, `/ssl/letsencrypt`, `/ssl/custom`, `/ssl/force` | SSL |
| GET/PUT/POST | `/api/apps/{id}/files`, `/files/content`, `/files/upload`, `/files/download`, `/files/op` | 파일 관리자 |
| GET/POST | `/api/projects`, `/api/projects/{id}` (PUT/DELETE), `/api/projects/{id}/apps` | Git 프로젝트·앱 |
| GET/PUT/DELETE | `/api/apps/{id}` · GET/PUT `/env` · POST `/deploy` | 앱 |
| GET/POST | `/api/apps/{id}/deployments`, `/api/deployments/{id}`, `/cancel`, `/rollback` | 배포·적용 기록 |
| GET/POST | `/api/apps/{id}/service`, `/journal`, `/deploy-key`, `/webhook`, POST `/api/hooks/{slug}` | 운영 |
| GET/POST/PUT/DELETE | `/api/databases`, `/api/databases/{id}` (앱 연결), `/password`, `/backups`, `/api/apps/{id}/databases`, `/api/apps/{id}/redis` | DB·Redis |
| POST/DELETE | `/api/databases/{id}/users`, `/users/{uid}`, `/users/{uid}/password` | DB 추가 계정 |
| POST/PUT/DELETE | `/api/redis-users`, `/api/redis-users/{id}`, `/password` | Redis 계정 |
| GET/POST/PUT/DELETE | `/api/backups`, `/api/apps/{id}/backups`, `/api/backups/{id}/restore`, `/download` | 백업 |
| GET/POST/PUT/DELETE | `/api/backup-jobs`, `/api/backup-jobs/{id}`, `/run` | 백업 예약 |
| GET/POST/PUT/DELETE | `/api/apps/{id}/cron`, `/api/cron/{id}`, `/run`, `/log`, `/api/cron/preview` | 크론 |
| GET | `/api/apps/{id}/nginx-logs`, `/api/system/info`, `/overview`, `/edge`, `/version`, `/api/audit` | 로그·시스템·버전 |
| GET | `/api/system/metrics?range=`, `/api/logs/system?kind=&lines=`, `/api/logs/deployments` | 모니터링 기록·로그 |
| GET/POST/PUT/DELETE | `/api/runtimes`, `/api/runtimes/{id}`, `/default`, PUT `/api/apps/{id}/runtime` | 언어 런타임 버전 |
| GET/POST/PUT/DELETE | `/api/mongo`, `/api/mongo/{id}` (앱 연결), `/password`, `/backups`, `/users`, `/users/{uid}`, `/users/{uid}/password` | MongoDB |
| GET/POST | `/api/dbweb/admin/{id}`, `/query`, `/table/{t}`, `/table/{t}/{insert,update,delete}` | DB 웹 관리 (관리자) |
| POST/GET | `/api/dbweb/login`, `/logout`, `/me`, `/api/dbweb/s/{db}/…` (`X-DB-Token`) | DB 웹 관리 (DB 계정) |
| GET/PUT/DELETE | `/api/system/panel-domain` | 패널 도메인 |
| GET/PUT | `/api/system/panel-access` | 패널 접속 허용 IP |
| GET/POST | `/api/db-engines`, `/api/db-engines/{engine}` | DB 서버 버전 |
| GET/POST | `/api/dbweb/mongo/{id}`, `/collections`, `/c/{coll}`, `/c/{coll}/{insert,update,delete}` | MongoDB 웹 관리 |
| GET/POST | `/api/dbweb/redis`, `/{db}/keys`, `/{db}/key?k=`, `/{db}/{set,delete}` | Redis 웹 관리 |
| GET/DELETE/POST/PUT | `/api/notifications`, `/read`, `/settings`, `/test` | 알림 |
| WS | `/api/ws/metrics?ticket=`, `/api/ws/deployments/{id}?ticket=` | 실시간 |

인증된 요청은 `Authorization: Bearer <access>`, 쿠키 기반 엔드포인트(`/auth/refresh`, `/auth/logout`)는 `X-Requested-With: rpanel` 헤더가 필요합니다(CSRF 방지).

## 라이선스

MIT — [LICENSE](LICENSE)
