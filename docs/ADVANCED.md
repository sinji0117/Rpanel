# Rpanel 기술 문서

처음 쓰는 분은 [README](../README.md)부터 보세요. 이 문서는 설치 옵션, 구조, 보안 설계, 개발 방법을 다룹니다.

## 구성

| 계층 | 기술 |
|---|---|
| 백엔드 | Rust + Axum 0.8 (Tokio) |
| DB | SQLx (`mysql` 기능) + MariaDB 10.11 LTS |
| 캐시·세션 | Redis 7.x |
| 프런트 | React 19 + Vite + Tailwind CSS 4 |
| 인증 | JWT(액세스, 15분) + Redis 리프레시 세션(회전·재사용 탐지) |
| 실시간 | Axum WebSocket — 서버 지표, 배포 로그 스트림 |
| 테스트 | `cargo test` · Vitest · Playwright |

```
 브라우저 ──▶ :8443 (HTTPS, Nginx) ─┬─ /        → 패널 UI (React)
                                    └─ /api, ws → 127.0.0.1:8090 (rpanel, 일반 사용자 권한)
                                                   │ sudo rpanel-helper  (검증된 시스템 작업만)
                                                   │ sudo -u <사이트유저> rpanel-files  (사이트 파일만)
 방문자 ──▶ :80 / :443 (Nginx, 사이트별 설정) ─┬─ Rust 앱       → 127.0.0.1:81xx (systemd 서비스)
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
| `--dry-run-mode` | 실제 배포 명령 없이 둘러보기 |

## 설치 스크립트가 하는 일

| 단계 | 내용 |
|---|---|
| Nginx | 기본 서버는 등록되지 않은 도메인·IP 직접 접속을 끊음(444 / TLS 거부), Let's Encrypt 확인 경로, 갱신 시 자동 reload |
| MariaDB 10.11 | `127.0.0.1` 바인딩, 익명 계정·test DB·원격 root 제거, RAM 기준 튜닝(없을 때만), 패널 DB |
| Redis 7 | `127.0.0.1` 바인딩, 비밀번호 생성(이미 있으면 유지), `maxmemory 1gb` |
| Node.js 22 / Rust | NodeSource(패널 UI·React 정적 앱 빌드용), rustup (Rust 앱 빌드용) |
| 패널 | `rpanel`·`rpanel-app` 사용자, `rpanel-sites` 그룹, 빌드·설치, helper + sudoers, systemd, HTTPS :8443 |
| SSH | `rpanel-sites` 그룹만 비밀번호 로그인 허용 (`Match Group`) |
| 비밀값 | `/etc/rpanel/rpanel.env` — DB·Redis 비밀번호, JWT 키 자동 생성 (재실행해도 유지) |
| ufw | 22·8443 ← `--allow-from`, 80·443 ← 어디서나(standalone) / 프록시만(proxy). **지금 SSH 로 접속 중인 주소는 자동 허용** |

이미 MariaDB·Redis 를 쓰고 있는 서버에서 실행해도 기존 데이터와 Redis 비밀번호는 유지되고, 패널용 DB·설정만 추가됩니다.

Rust 전용 패널입니다. 실행하는 서버 프로그램은 Rust 뿐이고, PHP·Node.js 런타임은 지원하지 않습니다 (Node.js 는 프런트 빌드 도구로만 사용). 다른 언어의 서비스는 다른 서버에 두고 리버스 프록시 사이트로 연결할 수 있습니다.

## 보안 모델

- 패널 프로세스는 **root 가 아닌 `rpanel` 사용자**로 실행됩니다. 패널 UI 는 HTTPS(8443) 로만 서비스되고, `--allow-from` 으로 접근 대역을 제한할 수 있습니다.
- root 가 필요한 작업은 **`/usr/local/lib/rpanel/rpanel-helper` 한 개**만 sudo 로 허용합니다. helper 는 모든 인자와 스테이징 파일을 **화이트리스트**로 다시 검사합니다:
  - Nginx: 허용된 지시어·location 만, `root` 는 해당 사이트 폴더로 고정, 인증서 경로 고정, `include`·`fastcgi_*` 불가, 다른 설정과 도메인 중복 금지, `nginx -t` 실패 시 원복
  - systemd: 실행 사용자 고정(앱은 `rpanel-app`, 사이트는 그 사이트 유저), `NoNewPrivileges` 필수, 경로는 해당 앱/사이트 안
  - 사이트 유저: `rpanel-sites` 그룹, 시스템 계정 이름 거부, 레지스트리(`/etc/rpanel/site-users`)에 있는 계정만 삭제
  - MariaDB: 패널이 만든 DB(`/etc/rpanel/managed-dbs`)만 삭제·덤프·복원
  - SSL: 인증서와 개인키 일치 검사, 키는 root 0600
- **사이트 파일은 그 사이트 유저 권한으로만** 다룹니다 (`sudo -u <사이트유저> rpanel-files`). 홈 디렉토리 밖을 가리키는 경로·심볼릭 링크는 거부합니다.
- 사이트끼리는 리눅스 계정으로 격리됩니다 (홈 0750, 업로드한 Rust 프로그램은 사이트 유저로 실행). Nginx(`www-data`)만 각 사이트 그룹에 들어가 정적 파일을 읽습니다.
- 이 계약은 `backend/tests/helper_contract.rs` 가 생성 템플릿 통과 + 위험한 변형(설정 주입, 경로 탈출, 권한 상승 시도 등)으로 검증합니다.

## 업로드형 Rust 사이트

1. **사이트 → 사이트 추가 → Rust**, 도메인 입력. 시작 명령 기본값 `./app`
2. 로컬에서 `cargo build --release` — 서버와 같은 아키텍처·같거나 낮은 glibc 로 빌드 (다르면 `cross` 나 `--target x86_64-unknown-linux-musl`)
3. 실행 파일을 문서 루트(`/home/<사이트유저>/htdocs/<도메인>/app`)에 업로드 → **적용**
   - 실행 파일이 아직 없으면 Nginx 만 설정하고 서비스는 띄우지 않습니다 (상태 `idle`)
   - 적용할 때 `chmod u+x` 를 자동으로 하고, systemd 서비스(사이트 유저, `NoNewPrivileges`, `MemoryDenyWriteExecute`)로 실행한 뒤 헬스체크
4. 프로그램은 `PORT` 환경변수의 포트로 127.0.0.1 에서 열어야 합니다 (`BIND_ADDR=127.0.0.1:<포트>` 도 함께 들어감). 데이터베이스·Redis 를 만들면 접속 URL 이 환경변수로 자동 추가됩니다.

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
/etc/rpanel/site-users, managed-dbs        helper 레지스트리
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
| GET/POST | `/api/sites` | 사이트 목록(응답 확인) / 만들기 (`type`: rust·html·proxy) |
| PUT | `/api/apps/{id}/site-user/password` | 사이트 유저(SSH·SFTP) 비밀번호 |
| GET/POST/PUT/DELETE | `/api/apps/{id}/ssl`, `/ssl/letsencrypt`, `/ssl/custom`, `/ssl/force` | SSL |
| GET/PUT/POST | `/api/apps/{id}/files`, `/files/content`, `/files/upload`, `/files/download`, `/files/op` | 파일 관리자 |
| GET/POST | `/api/projects`, `/api/projects/{id}` (PUT/DELETE), `/api/projects/{id}/apps` | Git 프로젝트·앱 |
| GET/PUT/DELETE | `/api/apps/{id}` · GET/PUT `/env` · POST `/deploy` | 앱 |
| GET/POST | `/api/apps/{id}/deployments`, `/api/deployments/{id}`, `/cancel`, `/rollback` | 배포·적용 기록 |
| GET/POST | `/api/apps/{id}/service`, `/journal`, `/deploy-key`, `/webhook`, POST `/api/hooks/{slug}` | 운영 |
| GET/POST/DELETE | `/api/databases`, `/api/apps/{id}/databases`, `/api/apps/{id}/redis` | DB·Redis |
| GET/POST/PUT/DELETE | `/api/backups`, `/api/apps/{id}/backups`, `/backup-settings`, `/api/backups/{id}/restore`, `/download` | 백업 |
| GET/POST/PUT/DELETE | `/api/apps/{id}/cron`, `/api/cron/{id}`, `/run`, `/log`, `/api/cron/preview` | 크론 |
| GET | `/api/apps/{id}/nginx-logs`, `/api/system/info`, `/overview`, `/edge`, `/api/audit` | 로그·시스템 |
| WS | `/api/ws/metrics?ticket=`, `/api/ws/deployments/{id}?ticket=` | 실시간 |

인증된 요청은 `Authorization: Bearer <access>`, 쿠키 기반 엔드포인트(`/auth/refresh`, `/auth/logout`)는 `X-Requested-With: rpanel` 헤더가 필요합니다(CSRF 방지).

## 라이선스

MIT — [LICENSE](LICENSE)
