# Rpanel

**셀프호스팅 웹 호스팅 패널 — CloudPanel처럼 사이트를 만들고, Dokploy처럼 Git에서 배포합니다.**
Ubuntu 24.04 서버에 이 패널 하나만 설치하면 Nginx · PHP-FPM · MariaDB · Redis · Node.js · Let's Encrypt 까지 함께 설정되어
바로 웹사이트를 운영할 수 있습니다. Docker 없이 systemd + Nginx 위에서 동작합니다.

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
 방문자 ──▶ :80 / :443 (Nginx, 사이트별 설정) ─┬─ PHP 사이트    → PHP-FPM 풀 (사이트 유저로 실행)
                                               ├─ HTML 사이트   → /home/<사이트유저>/htdocs/<도메인>
                                               ├─ Node·Rust 앱  → 127.0.0.1:81xx (systemd 서비스)
                                               └─ 리버스 프록시  → 지정한 주소
```

---

## 기능

### 사이트 (CloudPanel 방식)
- **사이트 추가** — 도메인만 입력하면 사이트 유저(리눅스 계정)·홈 폴더·Nginx 설정·PHP-FPM 풀이 만들어지고 바로 응답합니다.
  - **PHP** — WordPress·Laravel·그누보드 등. 사이트마다 전용 PHP-FPM 풀(사이트 유저 권한, `ondemand`), 버전 선택(여러 버전 공존), memory_limit·업로드 크기·실행 시간 설정, 웹 루트 하위 폴더(`public`)
  - **HTML** — 파일을 올리면 바로 서비스
  - **Node.js** — 파일을 올리고 시작 명령으로 실행 (systemd, 사이트 유저 권한, 포트 자동 할당)
  - **리버스 프록시** — 도메인을 다른 주소(내부 서버·포트)로 연결
- **파일 관리자** — 탐색, 업로드(드래그 앤 드롭, 최대 2GB), 다운로드, 편집기, 새 폴더/파일, 이름 변경, 권한, 삭제, zip·tar.gz 풀기
- **SSH · SFTP** — 사이트 유저로 FileZilla·WinSCP·터미널 접속 (비밀번호 또는 공개키)
- **SSL** — Let's Encrypt 무료 인증서(자동 갱신) 또는 가지고 있는 인증서 업로드, HTTP→HTTPS 강제, HTTP/2
- **데이터베이스** — MariaDB DB·전용 계정 생성, 접속 정보 자동 주입 (`DATABASE_URL`, PHP 는 `getenv()`)
- **Redis** — 사이트마다 DB 번호 배정, `REDIS_URL` 자동 주입
- **백업·복원** — DB 덤프 + 사이트 파일(`htdocs`), 매일/매주 예약, 보관 개수, 다운로드, 복원
- **크론** — 사이트 유저 권한으로 예약 실행 (systemd timer — 예: `php artisan schedule:run`, `wp cron event run`)
- **로그** — 도메인별 Nginx 접속/에러 로그와 상태코드·경로·IP 통계, PHP 에러 로그, 서비스 로그

### Git 배포 (Dokploy 방식)
- 프로젝트 / 앱 — **Rust**(cargo) · **Static**(React·Vite 빌드) · **Node** 앱을 Git 에서 빌드해 배포
- 실시간 배포 로그(WebSocket), 헬스체크 실패 시 이전 릴리스로 자동 원복, 원클릭 롤백, 배포 취소
- Deploy key(앱별 SSH 키), Webhook(GitLab/GitHub push 시 자동 배포)
- 프런트(Static)의 `/api/` 를 백엔드 포트로 프록시 → 한 도메인에서 React + API

### 공통
- 대시보드 (CPU·메모리·디스크·load 실시간), 감사 로그, 로그인 rate limit, 비밀번호 변경 시 전 세션 로그아웃

---

## 설치

Ubuntu Server **24.04** 를 설치한 직후의 서버에서 한 줄이면 됩니다 (root 권한, 인터넷 연결 필요).

```bash
curl -fsSL https://raw.githubusercontent.com/sinji0117/Rpanel/main/deploy/bootstrap.sh | sudo bash
```

10분 안팎(대부분 Rust 빌드) 뒤:

```
✔ Rpanel 설치 완료
  접속      https://<서버IP>:8443   (자체 서명 인증서 — 브라우저 경고에서 '계속' 선택)
            → 첫 화면에서 관리자 계정을 만드세요
```

> **저장소가 private 이면** 위 주소가 404 입니다. 토큰으로 받아서 실행하세요:
> `sudo git clone https://<토큰>@github.com/sinji0117/Rpanel.git /opt/rpanel-src && sudo /opt/rpanel-src/deploy/install.sh`

### 설치 모드

| 모드 | 언제 | 하는 일 |
|---|---|---|
| `--mode=standalone` (기본) | 서버가 인터넷에서 80/443 을 직접 받을 때 (VPS, 공인 IP 포트포워딩) | 사이트마다 Let's Encrypt 발급, 80·443 개방 |
| `--mode=proxy --trusted-proxy=<IP>` | 앞단에 리버스 프록시(Nginx Proxy Manager, Cloudflare Tunnel 등)가 SSL 을 처리할 때 | 실제 접속자 IP 복원, 80 은 프록시에서만 허용, SSL 은 프록시에서 |

옵션은 한 줄 설치 뒤에 붙입니다: `curl … | sudo bash -s -- --mode=proxy --trusted-proxy=10.0.0.10`

| 옵션 | 설명 |
|---|---|
| `--allow-from=CIDR,…` | SSH(22)·패널(8443) 접속 허용 대역 (기본: 어디서나) — 예: `10.0.0.0/24,100.64.0.0/10` |
| `--php=8.3,8.2,7.4` | 설치할 PHP 버전 (기본 `8.3`. 그 외 버전은 ondrej/php PPA) · `--no-php` |
| `--panel-domain=panel.example.com --email=you@example.com` | 패널을 도메인으로 접속 + Let's Encrypt 인증서 (standalone) |
| `--no-firewall` | ufw 를 건드리지 않음 |
| `--no-ssh-password` | 사이트 유저의 SSH/SFTP 비밀번호 로그인을 켜지 않음 (공개키만) |
| `--dry-run-mode` | 실제 배포 명령 없이 둘러보기 |

### 설치 스크립트가 하는 일

| 단계 | 내용 |
|---|---|
| Nginx | 기본 서버는 등록되지 않은 도메인·IP 직접 접속을 끊음(444 / TLS 거부), Let's Encrypt 확인 경로, 갱신 시 자동 reload |
| PHP-FPM | 선택한 버전 + 주요 확장(mysql·curl·gd·mbstring·xml·zip·intl·bcmath·opcache·redis·imagick), composer |
| MariaDB 10.11 | `127.0.0.1` 바인딩, 익명 계정·test DB·원격 root 제거, RAM 기준 튜닝(없을 때만), 패널 DB |
| Redis 7 | `127.0.0.1` 바인딩, 비밀번호 생성(이미 있으면 유지), `maxmemory 1gb` |
| Node.js 22 / Rust | NodeSource, rustup (Git 앱 빌드용) |
| 패널 | `rpanel`·`rpanel-app` 사용자, `rpanel-sites` 그룹, 빌드·설치, helper + sudoers, systemd, HTTPS :8443 |
| SSH | `rpanel-sites` 그룹만 비밀번호 로그인 허용 (`Match Group`) |
| 비밀값 | `/etc/rpanel/rpanel.env` — DB·Redis 비밀번호, JWT 키 자동 생성 (재실행해도 유지) |
| ufw | 22·8443 ← `--allow-from`, 80·443 ← 어디서나(standalone) / 프록시만(proxy). **지금 SSH 로 접속 중인 주소는 자동 허용** |

이미 MariaDB·Redis 를 쓰고 있는 서버에서 실행해도 기존 데이터와 Redis 비밀번호는 유지되고, 패널용 DB·설정만 추가됩니다.

### 업데이트

```bash
sudo rpanel-update        # 최신 소스 → 증분 빌드 → 재시작 (DB 스키마는 자동 마이그레이션)
```

---

## 자주 묻는 질문

**Nginx 가 두 군데(앞단 프록시 서버와 이 서버)에 다 있어도 되나요?**
네. 서로 다른 서버(또는 컨테이너)라면 문제없습니다. 예를 들어 `인터넷 → Nginx Proxy Manager(:443, SSL) → 이 서버(:80)` 처럼
앞단 Nginx 는 SSL 을 풀어 넘기고, 이 서버의 Nginx 는 도메인별로 PHP·파일·앱에 연결합니다. 이때는 `--mode=proxy` 로 설치하세요.
문제가 되는 경우는 **같은 서버에 Nginx 를 관리하는 패널이 둘** 있을 때입니다(예: CloudPanel 이 깔린 서버에 Rpanel 을 또 설치) —
둘 다 80/443 과 `/etc/nginx` 를 차지하려 하므로 Rpanel 은 깨끗한 서버에 설치하세요. 또 proxy 모드에서는 Let's Encrypt
확인 요청이 앞단 프록시에서 끝나므로, 인증서는 앞단에서 발급해야 합니다(패널 SSL 탭에 안내가 나옵니다).

**다른 Nginx 설정과 도메인이 겹치면?** 패널은 이미 다른 설정 파일이 쓰는 도메인을 거부합니다. 기존 파일을 정리한 뒤 적용하세요.

**기존 DB 를 패널로 가져오려면?** 실수로 지우는 것을 막기 위해 패널은 자기가 만든 DB 만 관리합니다. 패널에서 새 DB 를 만든 뒤 옮기세요:
`sudo mariadb-dump --single-transaction 기존DB | sudo mariadb 새DB`

**WordPress 설치는?** PHP 사이트 → 데이터베이스 탭에서 DB 생성 → 파일 관리자로 WordPress zip 업로드 후 "풀기" → 브라우저에서 설치 화면.
DB 접속 정보는 데이터베이스 탭에 표시됩니다(호스트 `127.0.0.1`).

---

## 보안 모델

- 패널 프로세스는 **root 가 아닌 `rpanel` 사용자**로 실행됩니다. 패널 UI 는 HTTPS(8443) 로만 서비스되고, `--allow-from` 으로 접근 대역을 제한할 수 있습니다.
- root 가 필요한 작업은 **`/usr/local/lib/rpanel/rpanel-helper` 한 개**만 sudo 로 허용합니다. helper 는 모든 인자와 스테이징 파일을 **화이트리스트**로 다시 검사합니다:
  - Nginx: 허용된 지시어·location 만, `root` 는 해당 사이트 폴더로 고정, 인증서 경로·`fastcgi_pass` 소켓 고정, `include` 는 `fastcgi_params` 만, 다른 설정과 도메인 중복 금지, `nginx -t` 실패 시 원복
  - systemd: 실행 사용자 고정(앱은 `rpanel-app`, 사이트는 그 사이트 유저), `NoNewPrivileges` 필수, 경로는 해당 앱/사이트 안
  - PHP-FPM 풀: 사이트 유저로만, 허용된 키만(`auto_prepend_file`·`disable_functions` 변경 등 거부), `php-fpm -t` 실패 시 원복
  - 사이트 유저: `rpanel-sites` 그룹, 시스템 계정 이름 거부, 레지스트리(`/etc/rpanel/site-users`)에 있는 계정만 삭제
  - MariaDB: 패널이 만든 DB(`/etc/rpanel/managed-dbs`)만 삭제·덤프·복원
  - SSL: 인증서와 개인키 일치 검사, 키는 root 0600
- **사이트 파일은 그 사이트 유저 권한으로만** 다룹니다 (`sudo -u <사이트유저> rpanel-files`). 홈 디렉토리 밖을 가리키는 경로·심볼릭 링크는 거부합니다.
- 사이트끼리는 리눅스 계정으로 격리됩니다 (홈 0750, PHP 는 사이트 유저로 실행). Nginx(`www-data`)만 각 사이트 그룹에 들어가 정적 파일을 읽습니다.
- 이 계약은 `backend/tests/helper_contract.rs` 가 생성 템플릿 통과 + 위험한 변형(설정 주입, 경로 탈출, 권한 상승 시도 등)으로 검증합니다.

---

## 사용 예

### PHP 사이트 (WordPress·Laravel)
1. **사이트 → + 사이트 추가 → PHP**, 도메인 `example.com www.example.com` → 접속 정보(SFTP 사용자·비밀번호)가 한 번 표시됩니다
2. **데이터베이스 탭**에서 DB 생성 → 다시 **적용**하면 PHP 에서 `getenv('DATABASE_URL')` 로도 읽을 수 있습니다
3. **파일 탭**에 사이트 파일 업로드 (또는 SFTP), Laravel 이면 **설정 → 웹 루트** 를 `public` 으로
4. **SSL 탭** → Let's Encrypt 발급 (standalone) — 도메인 DNS 가 이 서버를 가리켜야 합니다
5. **크론 탭** → `php artisan schedule:run` 을 `*:0/1` 로

### Rust API + React 프런트 (Git)
1. **프로젝트** 생성 → 앱 1 **Rust**: Git 저장소, 바이너리 이름, 헬스체크 `/api/health` (도메인 비움)
2. **Git · 웹훅 탭** → Deploy key 를 저장소에 등록 → **배포**
3. 앱 2 **Static**: 프런트 저장소, 도메인, **/api/ 프록시 포트 = 앱 1 포트** → **배포**
4. Webhook URL·토큰을 GitLab/GitHub 에 등록하면 push 할 때마다 자동 배포

### 홈랩 예시 — 앞단에 Nginx Proxy Manager 가 있을 때
```bash
curl -fsSL https://raw.githubusercontent.com/sinji0117/Rpanel/main/deploy/bootstrap.sh \
  | sudo bash -s -- --mode=proxy --trusted-proxy=10.0.0.10 --allow-from=10.0.0.0/24,100.64.0.0/10
```
NPM 에 도메인마다 Proxy Host → `<이 서버 IP>:80` 을 등록하고 SSL 은 NPM 에서 발급합니다.

---

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
/etc/php/<ver>/fpm/pool.d/rpanel-<slug>.conf
/etc/systemd/system/rpanel-app-<slug>.service, rpanel-cron-<slug>-<id>.{service,timer}

/home/<사이트유저>/htdocs/<도메인>/        사이트 파일 (CloudPanel 과 같은 구조)
/home/<사이트유저>/logs/php-error.log
/var/lib/rpanel/
  builds/<slug>/repo                       Git 앱 체크아웃·빌드 캐시
  apps/<slug>/releases/<배포id>, current   Git 앱 릴리스 (최근 3개)
  apps/<slug>/app.env                      환경변수 (0600)
  www/<slug>/current                       Git 정적 앱
  backups/<slug>/                          백업 (앱을 삭제해도 남김)
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
| GET/POST | `/api/sites` | 사이트 목록(응답 확인) / 만들기 (`type`: html·php·node·proxy) |
| PUT | `/api/apps/{id}/site-user/password`, `/api/apps/{id}/php` | 사이트 유저 비밀번호, PHP 설정 |
| GET/POST/PUT/DELETE | `/api/apps/{id}/ssl`, `/ssl/letsencrypt`, `/ssl/custom`, `/ssl/force` | SSL |
| GET/PUT/POST | `/api/apps/{id}/files`, `/files/content`, `/files/upload`, `/files/download`, `/files/op` | 파일 관리자 |
| GET/POST | `/api/projects`, `/api/projects/{id}` (PUT/DELETE), `/api/projects/{id}/apps` | Git 프로젝트·앱 |
| GET/PUT/DELETE | `/api/apps/{id}` · GET/PUT `/env` · POST `/deploy` | 앱 |
| GET/POST | `/api/apps/{id}/deployments`, `/api/deployments/{id}`, `/cancel`, `/rollback` | 배포·적용 기록 |
| GET/POST | `/api/apps/{id}/service`, `/journal`, `/deploy-key`, `/webhook`, POST `/api/hooks/{slug}` | 운영 |
| GET/POST/DELETE | `/api/databases`, `/api/apps/{id}/databases`, `/api/apps/{id}/redis` | DB·Redis |
| GET/POST/PUT/DELETE | `/api/backups`, `/api/apps/{id}/backups`, `/backup-settings`, `/api/backups/{id}/restore`, `/download` | 백업 |
| GET/POST/PUT/DELETE | `/api/apps/{id}/cron`, `/api/cron/{id}`, `/run`, `/log`, `/api/cron/preview` | 크론 |
| GET | `/api/apps/{id}/nginx-logs`, `/api/system/info`, `/overview`, `/php`, `/edge`, `/api/audit` | 로그·시스템 |
| WS | `/api/ws/metrics?ticket=`, `/api/ws/deployments/{id}?ticket=` | 실시간 |

인증된 요청은 `Authorization: Bearer <access>`, 쿠키 기반 엔드포인트(`/auth/refresh`, `/auth/logout`)는 `X-Requested-With: rpanel` 헤더가 필요합니다(CSRF 방지).

## 라이선스

MIT — [LICENSE](LICENSE)
