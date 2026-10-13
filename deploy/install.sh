#!/bin/bash
# Rpanel 원스톱 설치 — Ubuntu 24.04 를 막 설치한 서버(VM102)에서 그대로 실행합니다.
#
#   한 줄 설치 (OS 설치 직후):
#     curl -fsSL https://raw.githubusercontent.com/sinji0117/Rpanel/main/deploy/bootstrap.sh | sudo bash
#
#   소스를 직접 받은 경우:
#     sudo ./deploy/install.sh [옵션]
#
#   옵션
#     --no-firewall   ufw 를 건드리지 않음 (직접 관리할 때)
#     --no-build      빌드 생략, 이미 빌드된 backend/target/release, frontend/dist 사용
#     --dry-run-mode  패널을 RPANEL_EXEC_MODE=dry-run 으로 시작 (처음 둘러보기용, 나중에 env 에서 real 로)
#
# 하는 일 (여러 번 실행해도 안전 — 업데이트도 이 스크립트로)
#   1. 기본 패키지 + 시간대(Asia/Seoul) + qemu-guest-agent
#   2. Nginx · MariaDB 10.11 · Redis 7 설치/보안 설정 (127.0.0.1 바인딩, Redis 비밀번호, 익명 계정 제거)
#   3. Node.js 22 (NodeSource) — 프런트·Node 앱 빌드용
#   4. 패널용 DB·계정, Redis 비밀번호, JWT 비밀키 자동 생성 → /etc/rpanel/rpanel.env
#   5. rpanel / rpanel-app 사용자, Rust 툴체인(rpanel 사용자), 패널 빌드·설치
#   6. 권한 helper + sudoers, systemd 서비스, Nginx :9000 패널 사이트
#   7. ufw: SSH·패널은 Tailscale(100.64.0.0/10)·내부망(10.0.0.0/24)만, 80 은 CT100(10.0.0.10)만
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD=1
FIREWALL=1
EXEC_MODE=real
for arg in "$@"; do
  case "$arg" in
    --no-build) BUILD=0 ;;
    --no-firewall) FIREWALL=0 ;;
    --dry-run-mode) EXEC_MODE=dry-run ;;
    -h|--help) sed -n '2,25p' "$0"; exit 0 ;;
    *) echo "알 수 없는 옵션: $arg" >&2; exit 2 ;;
  esac
done

# Vol.3/Vol.6 네트워크 값 (필요하면 환경변수로 덮어쓰기)
INTERNAL_NET="${RPANEL_INTERNAL_NET:-10.0.0.0/24}"
TAILSCALE_NET="${RPANEL_TAILSCALE_NET:-100.64.0.0/10}"
PROXY_IP="${RPANEL_TRUSTED_PROXY:-10.0.0.10}"
PANEL_PORT=9000

say()  { printf '\n\033[1;32m▶ %s\033[0m\n' "$*"; }
info() { printf '  %s\n' "$*"; }
warn() { printf '\033[1;33m! %s\033[0m\n' "$*"; }
die()  { printf '\033[1;31m✘ %s\033[0m\n' "$*" >&2; exit 1; }

[[ $EUID -eq 0 ]] || die "root 로 실행하세요: sudo $0"
. /etc/os-release
[[ "${ID:-}" == "ubuntu" ]] || warn "Ubuntu 24.04 기준 스크립트입니다 — 현재: ${PRETTY_NAME:-unknown}"
export DEBIAN_FRONTEND=noninteractive

ENVF=/etc/rpanel/rpanel.env
envget() { [[ -f $ENVF ]] && grep -E "^$1=" "$ENVF" | tail -n1 | cut -d= -f2- || true; }
genpw() { openssl rand -hex 24; }
mem_mb() { awk '/MemTotal/ {print int($2/1024)}' /proc/meminfo; }

# ───────────────────────── 1. 기본 ─────────────────────────
say "1/7 기본 패키지"
apt-get update -q >/dev/null
apt-get install -y -q curl ca-certificates gnupg git rsync openssl sudo build-essential pkg-config libssl-dev \
  openssh-client nginx mariadb-server mariadb-client redis-server ufw qemu-guest-agent >/dev/null
timedatectl set-timezone Asia/Seoul 2>/dev/null || true
systemctl enable --now qemu-guest-agent >/dev/null 2>&1 || true
info "$(nginx -v 2>&1) · $(mariadb --version | awk '{print $5}' | tr -d ,) · $(redis-server --version | awk '{print $3}')"

# ───────────────────────── 2. MariaDB ─────────────────────────
say "2/7 MariaDB"
systemctl enable --now mariadb >/dev/null 2>&1 || true
# 다른 설정 파일보다 나중에 읽히도록 z- 접두어 (Vol.5 §8.7 의 교훈)
cat > /etc/mysql/mariadb.conf.d/z-rpanel-bind.cnf <<'EOF'
# managed by rpanel install.sh — DB는 이 VM 안에서만 접속
[mysqld]
bind-address = 127.0.0.1
EOF
if [[ ! -f /etc/mysql/mariadb.conf.d/99-tuning.cnf ]]; then
  BP=$(( $(mem_mb) * 35 / 100 )); (( BP < 256 )) && BP=256
  cat > /etc/mysql/mariadb.conf.d/99-tuning.cnf <<EOF
# rpanel install.sh 가 RAM $(mem_mb)MB 기준으로 생성 (Vol.6 §9.7 튜닝과 같은 항목) — 자유롭게 수정하세요
[mysqld]
innodb_buffer_pool_size = ${BP}M
innodb_log_file_size = 512M
innodb_flush_log_at_trx_commit = 2
innodb_flush_method = O_DIRECT
max_connections = 200
tmp_table_size = 64M
max_heap_table_size = 64M
slow_query_log = 1
slow_query_log_file = /var/log/mysql/slow.log
long_query_time = 2
EOF
  info "innodb_buffer_pool_size = ${BP}M"
fi
systemctl restart mariadb
for _ in $(seq 1 30); do mariadb -e "SELECT 1" >/dev/null 2>&1 && break; sleep 1; done
mariadb -e "SELECT 1" >/dev/null 2>&1 || die "MariaDB 가 시작되지 않았습니다: journalctl -u mariadb -n 50"
# mariadb-secure-installation 과 같은 정리 (root 는 unix_socket 그대로)
mariadb <<'SQL'
DELETE FROM mysql.global_priv WHERE User = '';
DELETE FROM mysql.global_priv WHERE User = 'root' AND Host NOT IN ('localhost', '127.0.0.1', '::1');
DROP DATABASE IF EXISTS test;
DELETE FROM mysql.db WHERE Db = 'test' OR Db = 'test\_%';
FLUSH PRIVILEGES;
SQL
if ss -Htln 2>/dev/null | awk '{print $4}' | grep -E ':3306$' | grep -qvE '^(127\.0\.0\.1|\[::1\]):'; then
  warn "3306 이 127.0.0.1 밖에서도 리스닝 중입니다: ss -tlnp | grep 3306"
else
  info "3306 → 127.0.0.1 만 리스닝"
fi

DB_URL="$(envget DATABASE_URL)"
if [[ -z "$DB_URL" || "$DB_URL" == *"<"* ]]; then
  DB_PW="$(genpw)"
  mariadb <<SQL
CREATE DATABASE IF NOT EXISTS rpanel CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;
CREATE USER IF NOT EXISTS 'rpanel'@'localhost' IDENTIFIED BY '$DB_PW';
ALTER USER 'rpanel'@'localhost' IDENTIFIED BY '$DB_PW';
GRANT ALL PRIVILEGES ON rpanel.* TO 'rpanel'@'localhost';
FLUSH PRIVILEGES;
SQL
  DB_URL="mysql://rpanel:$DB_PW@127.0.0.1:3306/rpanel"
  info "패널 DB 'rpanel' 과 계정 생성"
else
  info "기존 패널 DB 설정 유지"
fi

# ───────────────────────── 3. Redis ─────────────────────────
say "3/7 Redis"
RCONF=/etc/redis/redis.conf
[[ -f $RCONF ]] || die "$RCONF 가 없습니다"
redis_set() { # 활성 줄만 지우고 끝에 다시 씀 (주석 예시는 보존)
  sed -i -E "/^[[:space:]]*$1[[:space:]]/d" "$RCONF"
  echo "$1 $2" >> "$RCONF"
}
REDIS_PW="$(grep -E '^[[:space:]]*requirepass[[:space:]]' "$RCONF" | tail -n1 | awk '{print $2}' | tr -d '"' || true)"
if [[ -z "$REDIS_PW" ]]; then
  REDIS_PW="$(genpw)"
  info "Redis 비밀번호(requirepass) 새로 설정"
else
  info "기존 Redis 비밀번호 유지"
fi
redis_set bind "127.0.0.1 -::1"
redis_set protected-mode yes
redis_set requirepass "$REDIS_PW"
redis_set supervised systemd
grep -Eq '^[[:space:]]*maxmemory[[:space:]]' "$RCONF" || redis_set maxmemory 1gb
grep -Eq '^[[:space:]]*maxmemory-policy[[:space:]]' "$RCONF" || redis_set maxmemory-policy allkeys-lru
chmod 0640 "$RCONF"
systemctl enable redis-server >/dev/null 2>&1 || true
systemctl restart redis-server
for _ in $(seq 1 20); do REDISCLI_AUTH="$REDIS_PW" redis-cli ping 2>/dev/null | grep -q PONG && break; sleep 0.5; done
REDISCLI_AUTH="$REDIS_PW" redis-cli ping 2>/dev/null | grep -q PONG || die "Redis 가 응답하지 않습니다: journalctl -u redis-server -n 50"
REDIS_URL_V="$(envget REDIS_URL)"
if [[ -z "$REDIS_URL_V" || "$REDIS_URL_V" == *"<"* || "$REDIS_URL_V" != *":$REDIS_PW@"* ]]; then
  REDIS_URL_V="redis://:$REDIS_PW@127.0.0.1:6379/10"
fi

# ───────────────────────── 4. Node.js ─────────────────────────
say "4/7 Node.js 22"
node_ok() {
  command -v node >/dev/null && node -e 'const [a,b]=process.versions.node.split(".").map(Number); process.exit((a>22||(a===22&&b>=12)||(a===20&&b>=19))?0:1)'
}
if ! node_ok; then
  curl -fsSL https://deb.nodesource.com/setup_22.x | bash - >/dev/null
  apt-get install -y -q nodejs >/dev/null
fi
node_ok || die "Node.js 20.19+/22.12+ 설치 실패 (현재: $(node -v 2>/dev/null || echo 없음))"
info "node $(node -v) · npm $(npm -v)"

# ───────────────────────── 5. 사용자 · 빌드 ─────────────────────────
say "5/7 사용자 · Rust · 빌드"
id rpanel >/dev/null 2>&1 || useradd --system --home-dir /var/lib/rpanel --create-home --shell /usr/sbin/nologin rpanel
id rpanel-app >/dev/null 2>&1 || useradd --system --no-create-home --home-dir /nonexistent --shell /usr/sbin/nologin rpanel-app
install -d -o rpanel -g rpanel -m 0755 /var/lib/rpanel /var/lib/rpanel/builds /var/lib/rpanel/apps /var/lib/rpanel/www
install -d -o rpanel -g rpanel -m 0750 /var/lib/rpanel/staging /var/lib/rpanel/backups
install -d -o rpanel -g rpanel -m 0700 /var/lib/rpanel/keys
install -d -o root -g rpanel -m 0750 /etc/rpanel

if [[ ! -x /var/lib/rpanel/.cargo/bin/cargo ]]; then
  info "rustup 설치 (rpanel 사용자 — 앱 빌드에도 사용)"
  sudo -u rpanel -H bash -c 'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --no-modify-path' >/dev/null
fi
info "$(sudo -u rpanel -H /var/lib/rpanel/.cargo/bin/cargo --version)"

# 소스 저장소는 건드리지 않고 rpanel 소유 사본에서 빌드 (target/node_modules 보존 → 증분 빌드)
SRC=/var/lib/rpanel/.self-build
if [[ $BUILD -eq 1 ]]; then
  install -d -o rpanel -g rpanel -m 0755 "$SRC"
  rsync -a --delete --exclude .git --exclude target --exclude node_modules --exclude dist "$REPO/" "$SRC/"
  chown -R rpanel:rpanel "$SRC"
  info "백엔드 빌드 (cargo build --release) — 처음엔 5~10분 걸립니다"
  sudo -u rpanel -H bash -c "cd '$SRC/backend' && /var/lib/rpanel/.cargo/bin/cargo build --release --locked --quiet"
  info "프런트 빌드 (React 19 + Vite + Tailwind 4)"
  sudo -u rpanel -H bash -c "cd '$SRC/frontend' && npm ci --no-audit --no-fund --loglevel=error >/dev/null && npm run build >/dev/null"
elif [[ -x "$REPO/backend/target/release/rpanel" && -f "$REPO/frontend/dist/index.html" ]]; then
  SRC="$REPO"
fi
[[ -x "$SRC/backend/target/release/rpanel" ]] || die "백엔드 바이너리가 없습니다: $SRC/backend/target/release/rpanel"
[[ -f "$SRC/frontend/dist/index.html" ]] || die "프런트 빌드 결과가 없습니다: $SRC/frontend/dist"

# ───────────────────────── 6. 설치 ─────────────────────────
say "6/7 패널 설치"
install -m 0755 -o root -g root "$SRC/backend/target/release/rpanel" /usr/local/bin/rpanel
install -d -m 0755 /usr/share/rpanel/www
rsync -a --delete "$SRC/frontend/dist/" /usr/share/rpanel/www/
chmod -R a+rX /usr/share/rpanel/www
install -d -m 0755 /usr/local/lib/rpanel
install -m 0755 -o root -g root "$REPO/deploy/rpanel-helper" /usr/local/lib/rpanel/rpanel-helper
install -m 0755 -o root -g root "$REPO/deploy/bootstrap.sh" /usr/local/sbin/rpanel-update

cat > /etc/sudoers.d/rpanel.tmp <<'EOF'
# Rpanel: 패널은 이 helper 하나만 root로 실행할 수 있습니다 (인자·파일은 helper가 검증)
Defaults:rpanel !requiretty, env_reset
rpanel ALL=(root) NOPASSWD: /usr/local/lib/rpanel/rpanel-helper
EOF
chmod 0440 /etc/sudoers.d/rpanel.tmp
visudo -cf /etc/sudoers.d/rpanel.tmp >/dev/null || die "sudoers 문법 오류"
mv /etc/sudoers.d/rpanel.tmp /etc/sudoers.d/rpanel

# 환경 파일: 비밀값은 기존 값을 유지하고, 없으면 생성
JWT="$(envget JWT_SECRET)"; [[ ${#JWT} -ge 32 && "$JWT" != *"<"* ]] || JWT="$(openssl rand -hex 32)"
MODE="$(envget RPANEL_EXEC_MODE)"; [[ -n "$MODE" ]] || MODE="$EXEC_MODE"
[[ "$EXEC_MODE" == dry-run ]] && MODE=dry-run
COOKIE="$(envget COOKIE_SECURE)"; [[ -n "$COOKIE" ]] || COOKIE=false
EXTRA=""
if [[ -f $ENVF ]]; then
  # 스크립트가 관리하지 않는 사용자 추가 설정은 보존
  EXTRA="$(grep -Ev '^(#|$|DATABASE_URL=|REDIS_URL=|JWT_SECRET=|BIND_ADDR=|RPANEL_EXEC_MODE=|COOKIE_SECURE=)' "$ENVF" || true)"
  cp -p "$ENVF" "$ENVF.bak"
fi
{
  echo "# /etc/rpanel/rpanel.env — install.sh 가 관리 (비밀값은 재실행해도 유지됩니다)"
  echo "DATABASE_URL=$DB_URL"
  echo "REDIS_URL=$REDIS_URL_V"
  echo "JWT_SECRET=$JWT"
  echo "BIND_ADDR=127.0.0.1:8090"
  echo "RPANEL_EXEC_MODE=$MODE"
  echo "COOKIE_SECURE=$COOKIE"
  if [[ -n "$EXTRA" ]]; then
    echo "$EXTRA"
  else
    echo "RUST_LOG=info"
    echo "RPANEL_APP_PORT_MIN=8100"
    echo "RPANEL_APP_PORT_MAX=8999"
    echo "RPANEL_TRUSTED_PROXY=$PROXY_IP"
    echo "RPANEL_MAX_CONCURRENT_BUILDS=2"
    echo "RPANEL_BACKUP_HOUR=3"
  fi
} > "$ENVF.new"
chown root:rpanel "$ENVF.new"; chmod 0640 "$ENVF.new"; mv "$ENVF.new" "$ENVF"

install -m 0644 "$REPO/deploy/systemd/rpanel.service" /etc/systemd/system/rpanel.service
systemctl daemon-reload
systemctl enable rpanel >/dev/null 2>&1

[[ -L /etc/nginx/sites-enabled/default ]] && rm -f /etc/nginx/sites-enabled/default && info "Nginx 기본 사이트 비활성화"
install -m 0644 "$REPO/deploy/nginx/rpanel-panel.conf" /etc/nginx/sites-available/rpanel-panel.conf
ln -sfn /etc/nginx/sites-available/rpanel-panel.conf /etc/nginx/sites-enabled/rpanel-panel.conf
nginx -t >/dev/null 2>&1 || { nginx -t; die "Nginx 설정 오류"; }
systemctl enable nginx >/dev/null 2>&1 || true
systemctl reload nginx 2>/dev/null || systemctl restart nginx

systemctl restart rpanel
ok=0
for _ in $(seq 1 30); do
  if curl -fsS http://127.0.0.1:8090/api/health 2>/dev/null | grep -q '"status":"ok"'; then ok=1; break; fi
  sleep 1
done
(( ok )) || die "패널이 시작되지 않았습니다: journalctl -u rpanel -n 50 --no-pager"
info "패널 동작 확인 (db·redis ok)"

# ───────────────────────── 7. 방화벽 ─────────────────────────
say "7/7 방화벽 (ufw)"
if [[ $FIREWALL -eq 1 ]]; then
  # 지금 SSH 로 접속해 있는 주소는 잠기지 않도록 허용 (sudo·파이프 실행에서도 동작하도록 소켓에서 직접 읽음)
  SSH_FROM="$(ss -Htn state established '( sport = :22 )' 2>/dev/null | awk '{print $4}' | sed -E 's/:[0-9]+$//; s/^\[//; s/\]$//; s/^::ffff://' | sort -u | tr '\n' ' ')"
  ufw default deny incoming >/dev/null
  ufw default allow outgoing >/dev/null
  for net in "$INTERNAL_NET" "$TAILSCALE_NET"; do
    ufw allow from "$net" to any port 22 proto tcp >/dev/null
    ufw allow from "$net" to any port "$PANEL_PORT" proto tcp >/dev/null
  done
  for ip in $SSH_FROM; do
    ufw allow from "$ip" to any port 22 proto tcp >/dev/null && info "현재 SSH 접속 주소 $ip 허용"
  done
  ufw allow from "$PROXY_IP" to any port 80 proto tcp >/dev/null
  ufw --force enable >/dev/null
  info "22·$PANEL_PORT ← $INTERNAL_NET, $TAILSCALE_NET  |  80 ← $PROXY_IP (CT100)"
else
  info "건너뜀 (--no-firewall)"
fi

IP="$(hostname -I 2>/dev/null | tr ' ' '\n' | grep -E '^10\.' | head -n1 || true)"
[[ -n "$IP" ]] || IP="$(hostname -I 2>/dev/null | awk '{print $1}')"
cat <<EOF

$(printf '\033[1;32m')✔ Rpanel 설치 완료$(printf '\033[0m')

  접속      http://${IP:-<서버IP>}:$PANEL_PORT   (Tailscale 이 켜진 PC에서) → 첫 관리자 계정 생성
  실행 모드  $MODE$( [[ $MODE == dry-run ]] && echo "  — 실제 배포하려면 $ENVF 에서 RPANEL_EXEC_MODE=real 후 systemctl restart rpanel")
  설정·비밀  $ENVF  (DB·Redis 비밀번호, JWT 키 — root/rpanel 만 읽기)
  업데이트   sudo rpanel-update
  로그      journalctl -u rpanel -f

  다음 단계: z820 에서 tailscale set --snat-subnet-routes=false (Vol.3 §4.5.1),
             서비스할 도메인은 CT100(NPM) Proxy Host → $IP:80 (Vol.4 §10.2)
EOF
