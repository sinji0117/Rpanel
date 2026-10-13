# Rpanel

**VM102(app-server)용 셀프호스팅 배포 패널** — Dokploy처럼 Git 저장소를 연결하고 버튼 한 번(또는 GitLab push)으로
빌드 → 배포 → Nginx 연결 → 헬스체크 → 롤백까지 처리합니다. Docker 대신 **Vol.6에서 손으로 하던 구조(systemd + Nginx)를 그대로 자동화**합니다.

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
 관리 PC ──Tailscale──▶ z820 ──▶ VM102 :9000 (Nginx) ─┬─ /        → /usr/share/rpanel/www  (React)
                                                      └─ /api, ws → 127.0.0.1:8090        (rpanel)
                                                                      │ sudo -n rpanel-helper (검증된 작업만)
 인터넷 ─▶ CT100(NPM, SSL) ─▶ VM102 :80 (Nginx) ─▶ rpanel-<slug>.conf ─▶ 127.0.0.1:81xx  (rpanel-app-<slug>.service)
                                                                    └─▶ /var/lib/rpanel/www/<slug>/current (정적)
```

---

## 기능

- **프로젝트 / 앱** — 앱 종류 3가지
  - `rust` : `cargo build --release` → `target/release/<바이너리>` → systemd 서비스 (Axum 등)
  - `static` : `npm ci && npm run build` → `dist/`를 Nginx가 SPA로 서비스. **`/api/` 프록시 포트**를 지정하면 Vol.6 §9.13과 같은 “React + /api → Rust” 구조
  - `node` : `npm ci` (+빌드) → 시작 명령을 systemd 서비스로
- **포트 자동 할당** (기본 8100–8999, Vol.6 수동 앱 8080–8083과 겹치지 않음) — `PORT`, `BIND_ADDR=127.0.0.1:<포트>` 자동 주입
- **배포 파이프라인** — git fetch(shallow) → 빌드 → 릴리스 디렉토리 → `current` 심볼릭 링크 원자 교체 → 서비스 재시작 → **헬스체크·Nginx 단계 실패(또는 취소) 시 이전 릴리스와 env 파일로 자동 원복** → Nginx 사이트 설치(`nginx -t` 실패 시 원복) → 최근 3개 릴리스만 보관
- **실시간 배포 로그** (WebSocket, 재접속해도 줄 중복 없음), **배포 취소**, **원클릭 롤백**
- **환경변수** 편집 (.env 붙여넣기 지원, 값 마스킹, 감사 로그에는 값 미기록)
- **Deploy key** — 앱별 ed25519 키 생성 → GitLab Deploy keys에 등록 (Vol.6 §9.10.0의 수작업 대체)
- **Webhook** — `POST /api/hooks/<slug>` + `X-Gitlab-Token` → push 시 자동 배포 (브랜치 필터)
- **데이터베이스** — 앱마다 MariaDB DB·전용 계정 생성(그 DB에만 권한), 비밀번호 교체, 삭제. 접속 URL이 `DATABASE_URL`(키 변경 가능)로 자동 주입
- **Redis** — 앱마다 Redis DB 번호 자동 배정(기본 4–15, 패널 DB·Vol.6 수동 앱 0–3 제외) → `REDIS_URL` 자동 주입, 키 개수 확인·FLUSHDB
- **사이트** — 도메인별로 VM102 Nginx에 실제 요청해 응답코드·지연 확인, 앱별 Nginx 접속/에러 로그와 상태코드·경로·IP 통계
- **백업·복원** — DB 덤프(.sql.gz) + 앱 `data/`(.tar.gz), 지금 백업 / 매일·매주 예약(기본 03시), 대상별 보관 개수, 다운로드, 원클릭 복원
- **크론** — 앱별 예약 명령을 systemd timer로 등록(패널이 꺼져도 실행), 프리셋·일정 검증(다음 실행 시각 표시), 지금 실행, 실행 로그
- **서비스 제어** (start/stop/restart), **journalctl** 조회
- **대시보드** — CPU·메모리·디스크·load 실시간, 최근 배포
- **보안** — 감사 로그, 로그인 rate limit, 리프레시 토큰 재사용 시 세션 전체 폐기, 비밀번호 변경 시 전 세션 로그아웃

## 보안 모델 (꼭 읽어주세요)

- 패널 프로세스는 **root가 아닌 `rpanel` 사용자**로 실행됩니다.
- root가 필요한 작업(systemd 유닛·Nginx 사이트 설치, 서비스 제어, journal)은 **`/usr/local/lib/rpanel/rpanel-helper` 한 개**만 sudo로 허용합니다.
  helper는 slug·인자를 다시 검증하고, 스테이징 파일을 **화이트리스트**로 검사합니다:
  - 유닛: `User=rpanel-app` 강제, `NoNewPrivileges=true` 필수, 허용된 키만, 경로는 해당 앱 디렉토리 안으로 고정
  - Nginx: `listen 80;`만, 허용된 지시어·location만, `root`/로그/`proxy_pass 127.0.0.1` 고정, **다른 사이트(Vol.6 수동 설정 포함)와 도메인이 겹치면 거부**
  - 크론 timer: `OnCalendar`는 `systemd-analyze`로 검증, `Unit=`은 해당 작업 서비스로 고정
  - MariaDB: helper가 만든 DB만 `/etc/rpanel/managed-dbs`에 기록하고, **목록에 있는 DB만** 삭제·덤프·복원합니다. 이미 있는 DB(수동으로 만든 beripick 등)는 가져오거나 건드리지 않습니다. 시스템 DB·패널 DB 이름은 거부합니다.
  - 백업 복원: 아카이브 안 경로가 `data/` 아래인지, `..`·링크·특수 파일이 없는지 검사한 뒤 풉니다.
  - 이 계약은 `backend/tests/helper_contract.rs`가 생성 템플릿 통과 + 위험한 변형(유닛·사이트·타이머·DB 인자 등)으로 검증합니다.
- 앱은 모두 `rpanel-app` 사용자로 실행됩니다 (systemd 하드닝: `ProtectSystem=strict`, `PrivateTmp`, `NoNewPrivileges` …). 앱끼리는 같은 사용자라 서로의 릴리스 파일을 읽을 수 있습니다 — 서로 신뢰하지 않는 앱을 한 VM에 섞지 마세요.
- `env` 파일은 `0600 rpanel` 소유이며 systemd(root)가 읽어 주입합니다.
- 패널 UI(:9000)는 **Tailscale(100.64.0.0/10)과 내부망에만** 엽니다. CT100에 패널 도메인을 등록해 인터넷에 노출하지 마세요.

---

## 설치 (VM102)

> 전제: Vol.6 §9.1–§9.9, §9.13까지 완료 (Ubuntu 24.04, Nginx, MariaDB 10.11, Redis 7, NodeSource 22.x).
> Tailscale 서브넷 라우터의 `--snat-subnet-routes=false`(Vol.3 §4.5.1)가 적용되어 있어야 ufw의 100.64.0.0/10 규칙이 맞게 동작합니다.

### 1. 소스 받기

```bash
ssh sysadmin@10.0.0.12
sudo mkdir -p /srv/build && sudo chown $(whoami):$(whoami) /srv/build
cd /srv/build
git clone https://github.com/sinji0117/Rpanel.git rpanel
```

### 2. DB·Redis 준비

```bash
sudo mariadb
```
```sql
CREATE DATABASE rpanel CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;
CREATE USER 'rpanel'@'localhost' IDENTIFIED BY '<openssl rand -hex 24 로 만든 값>';
GRANT ALL PRIVILEGES ON rpanel.* TO 'rpanel'@'localhost';   -- 테이블은 패널이 시작할 때 마이그레이션으로 생성
FLUSH PRIVILEGES;
```
Redis는 §9.7.1에서 만든 `requirepass`를 그대로 쓰고, DB 번호는 **10** (앱들이 0–3 사용).

### 3. 설치 스크립트

```bash
sudo -i
cd /srv/build/rpanel
./deploy/install.sh
```
처음 실행하면 `/etc/rpanel/rpanel.env`가 생성되고(`JWT_SECRET` 자동) 멈춥니다. 비밀번호를 채우고 다시 시작합니다.

```bash
nano /etc/rpanel/rpanel.env      # DATABASE_URL, REDIS_URL 의 <…> 부분
systemctl restart rpanel
curl -s http://127.0.0.1:8090/api/health    # {"db":true,"redis":true,"status":"ok"}
```

### 4. 접속

Tailscale이 켜진 관리 PC 브라우저에서 **`http://10.0.0.12:9000`** → 첫 화면에서 관리자 계정 생성.

> 처음엔 `RPANEL_EXEC_MODE=dry-run`으로 둘러보면 실제 명령 없이 배포 흐름(로그)만 볼 수 있습니다.

### 업데이트

```bash
cd /srv/build/rpanel && git pull
sudo ./deploy/install.sh
```

---

## 사용 예 — Vol.6의 beripick을 패널로

1. **프로젝트** `Beripick` 생성
2. **앱 1 – Rust**: Git `ssh://git@<GitLab Tailscale IP>:2222/beripick/beripick_backend.git`, 바이너리 이름 = Cargo.toml `name`, 헬스체크 `/api/health`, 도메인은 비워둠(프런트가 /api로 프록시)
3. 앱 화면 → **Git · 웹훅** → *Deploy key 만들기* → GitLab 프로젝트 → Settings → Repository → Deploy keys에 등록 (쓰기 권한 X)
4. **환경변수**에 `DATABASE_URL`, `REDIS_URL` 등 입력 (`BIND_ADDR`는 자동 — 앱은 `std::env::var("BIND_ADDR")`로 바인딩)
5. **배포** → 실시간 로그 확인 → 할당된 포트(예: 8100) 확인
6. **앱 2 – Static**: 프런트 저장소, 도메인 `beripick.com www.beripick.com`, **/api/ 프록시 포트 = 8100**
7. CT100(NPM)에 Proxy Host `beripick.com, www.beripick.com → 10.0.0.12:80` (Vol.4 §10.2) — 이미 있으면 그대로
8. 이미 Vol.6 수동 설정(`/etc/nginx/sites-enabled/app`)이 같은 도메인을 쓰고 있으면 helper가 거부합니다 → 수동 설정을 정리한 뒤 배포하세요.

### GitLab push 자동 배포

앱 → **Git · 웹훅**의 URL과 토큰을 GitLab → Settings → Webhooks에 등록 (Push events, 브랜치 필터).
패널은 Tailscale 전용이므로 GitLab(ML110) 노드가 `10.0.0.0/24` 서브넷 라우트를 받도록(`tailscale set --accept-routes`) 하거나,
GitLab CI 잡에서 직접 호출하세요:

```bash
curl -fsS -X POST -H "X-Gitlab-Token: $RPANEL_HOOK_TOKEN" http://10.0.0.12:9000/api/hooks/beripick-api
```

---

## 실환경 검증

이 저장소는 실제 실행 모드(`RPANEL_EXEC_MODE=real`)로 **자기 자신을 배포**해 검증했습니다 (Ubuntu 24.04 + MariaDB 10.11 + Redis 7 + Nginx):
GitHub에서 clone → `cargo build --release`(백엔드)·`npm ci && npm run build`(프런트) → `rpanel-app` 사용자로 서비스 기동 →
헬스체크 → Nginx가 `Host` 헤더로 정적 파일과 `/api/` 프록시를 분기. DB·Redis·백업·크론 기능도 같은 방식으로 확인했습니다: 패널이 만든 MariaDB DB로 앱이 마이그레이션되고(전용 계정, 그 DB에만 권한), Redis는 배정된 번호에만 키가 쌓이며,
DB 덤프·data 백업 → 데이터 삭제 → 복원으로 원상 복구, 다운로드 크기 일치, 크론 유닛 설치·제거, `systemd-analyze`로 잘못된 일정 거부, 기존 수동 DB 가져오기 거부까지.
이어서 잘못된 환경변수로 재배포 → 헬스체크 실패 → 이전 릴리스·env로 자동 원복,
수동 Nginx 설정과 도메인 충돌 시 helper 거부, 배포 취소, 서비스 stop/start, journal, Deploy key, 앱 삭제(유닛·사이트·파일 정리)까지 확인했습니다.

### 이전 버전에서 업데이트할 때

DB 스키마는 패널 시작 시 자동 마이그레이션됩니다(`0002_hosting.sql`). `./deploy/install.sh`를 다시 실행하면 새 helper도 설치됩니다.

### beripick·bandiuri 를 패널 DB로 옮기려면

패널은 기존 DB를 가져오지 않습니다(실수로 지우는 것을 막기 위해). 패널에서 새 DB를 만든 뒤 데이터를 옮기세요:
```bash
sudo mariadb-dump --single-transaction app_db | sudo mariadb beripick_api   # 기존 → 패널이 만든 DB
```
환경변수 탭에서 직접 넣었던 `DATABASE_URL`을 지우면 패널이 주입하는 값이 쓰입니다.

## 디렉토리 구조 (VM102)

```
/usr/local/bin/rpanel                      패널 바이너리
/usr/share/rpanel/www/                     패널 UI
/etc/rpanel/rpanel.env                     설정 (root:rpanel 0640)
/usr/local/lib/rpanel/rpanel-helper        권한 작업 helper (root 0755)
/etc/sudoers.d/rpanel                      rpanel → helper 만 허용
/var/lib/rpanel/
  builds/<slug>/repo                       git 체크아웃·빌드 캐시(target, node_modules)
  apps/<slug>/releases/<배포id>/           서비스 앱 릴리스 (최근 3개)
  apps/<slug>/current -> releases/<id>
  apps/<slug>/app.env                      환경변수 (0600)
  apps/<slug>/data/                        앱이 쓸 수 있는 유일한 디렉토리
  www/<slug>/releases/<id>, current        정적 앱
  keys/<slug>(.pub), known_hosts           Deploy key
  backups/<slug>/<시각>-db-<DB>.sql.gz      DB 백업 (rpanel 0640) — NAS가 가져가기 좋은 위치 (Vol.26)
  backups/<slug>/<시각>-data.tar.gz         data/ 백업  ※ 앱을 삭제해도 백업 파일은 남깁니다
/etc/rpanel/managed-dbs                    패널이 만든 DB 목록 (helper만 수정)
/etc/systemd/system/rpanel-cron-<slug>-<id>.{service,timer}
  staging/                                 helper가 검사 후 설치할 유닛·사이트
/etc/systemd/system/rpanel-app-<slug>.service
/etc/nginx/sites-available/rpanel-<slug>.conf
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
# 백엔드 단위 테스트 + helper 계약 테스트
cd backend && cargo test

# 통합 테스트 (실제 MariaDB/Redis, dry-run 실행기, WebSocket 포함) — ⚠ 테스트 DB 내용을 지웁니다
RPANEL_TEST_DATABASE_URL=mysql://rpanel:pw@127.0.0.1:3306/rpanel_test \
RPANEL_TEST_REDIS_URL=redis://:pw@127.0.0.1:6379/15 \
cargo test --test api

# 프런트 단위 테스트
cd frontend && npm test

# E2E (가짜 API로 동작, 백엔드 불필요)
npx playwright install chromium   # 최초 1회
npm run e2e
```

## API 요약

| 메서드 | 경로 | 설명 |
|---|---|---|
| GET | `/api/health` | DB·Redis 상태 |
| GET/POST | `/api/auth/status`, `/setup`, `/login`, `/refresh`, `/logout`, `/me`, `/password`, `/ws-ticket` | 인증 |
| GET/POST | `/api/projects`, `/api/projects/{id}` (PUT/DELETE) | 프로젝트 |
| POST | `/api/projects/{id}/apps` | 앱 생성 |
| GET/PUT/DELETE | `/api/apps/{id}` | 앱 |
| GET/PUT | `/api/apps/{id}/env` | 환경변수 (전체 교체) |
| POST | `/api/apps/{id}/deploy` | 배포 시작 |
| GET | `/api/apps/{id}/deployments`, `/api/deployments/{id}` | 배포 기록·로그 |
| POST | `/api/deployments/{id}/cancel`, `/rollback` | 취소·롤백 |
| GET/POST | `/api/apps/{id}/service`, `/journal`, `/deploy-key`, `/webhook` | 운영 |
| POST | `/api/hooks/{slug}` | Webhook (토큰) |
| GET | `/api/databases` · GET/POST `/api/apps/{id}/databases` · DELETE `…/databases/{db}` · POST `…/{db}/password` | MariaDB |
| POST/DELETE | `/api/apps/{id}/redis`, POST `/api/apps/{id}/redis/flush` | Redis 배정 |
| GET | `/api/sites`, `/api/apps/{id}/nginx-logs?kind=access\|error` | 사이트 |
| GET/POST/PUT/DELETE | `/api/backups`, `/api/apps/{id}/backups`, `/api/apps/{id}/backup-settings`, `/api/backups/{id}/restore`, `/download` | 백업 |
| GET/POST/PUT/DELETE | `/api/apps/{id}/cron`, `/api/cron/{id}`, `/run`, `/log`, POST `/api/cron/preview` | 크론 |
| GET | `/api/system/info`, `/overview`, `/api/audit` | 시스템 |
| WS | `/api/ws/metrics?ticket=`, `/api/ws/deployments/{id}?ticket=` | 실시간 |

인증된 요청은 `Authorization: Bearer <access>` 헤더, 쿠키 기반 엔드포인트(`/auth/refresh`, `/auth/logout`)는 `X-Requested-With: rpanel` 헤더가 필요합니다(CSRF 방지).
