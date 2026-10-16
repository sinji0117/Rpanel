#!/bin/bash
# Rpanel 설치 엔진 — Ubuntu 24.04 에 패널과 필요한 프로그램을 설치·설정합니다.
# (Nginx · MariaDB · Redis · Rust · Node.js(프런트 빌드용) · Let's Encrypt)
#
#   보통은 저장소 맨 위의 install.sh / update.sh 로 실행합니다:
#     curl -fsSL https://raw.githubusercontent.com/sinji0117/Rpanel/main/install.sh | sudo bash
#
#   소스를 직접 받은 경우:
#     sudo ./deploy/install.sh [옵션]
#
# 옵션
#   --mode=standalone        (기본) 이 서버가 80/443 을 직접 받고 SSL 도 여기서 발급 (Let's Encrypt)
#   --mode=proxy             앞단 리버스 프록시(Nginx Proxy Manager 등)가 SSL 을 처리하고 :80 으로 전달
#   --trusted-proxy=IP       proxy 모드에서 앞단 프록시 IP (실제 접속자 IP 복원 + 80 포트 허용 대상)
#   --allow-from=CIDR,...    SSH·패널(8443) 접속 허용 대역 (기본: 어디서나, any 로 되돌림)
#   --panel-domain=DOMAIN    패널 접속 도메인 (standalone 에서 Let's Encrypt 인증서 발급, --email 필요)
#   --email=EMAIL            Let's Encrypt 계정 이메일
#   --no-firewall            ufw 를 건드리지 않음 (--firewall 로 다시 켬)
#   --no-ssh-password        사이트 유저의 SSH/SFTP 비밀번호 로그인을 켜지 않음 (--ssh-password 로 다시 켬)
#   --dry-run-mode           패널을 RPANEL_EXEC_MODE=dry-run 으로 시작 (둘러보기용)
#   --no-build               빌드 생략 (이미 빌드된 결과물 사용)
#
# 여러 번 실행해도 안전합니다. 비밀값과 처음 고른 옵션은 /etc/rpanel/rpanel.env 에 저장돼 다음 실행(업데이트)에도 그대로 쓰입니다.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENVF=/etc/rpanel/rpanel.env
envget() { [[ -f $ENVF ]] && grep -E "^$1=" "$ENVF" | tail -n1 | cut -d= -f2- || true; }

# 옵션 기본값: 지난번 설치 때 저장한 값 → 없으면 기본
BUILD=1
EXEC_MODE=real
MODE_ARG=""
FIREWALL="$(envget RPANEL_FIREWALL)"; [[ "$FIREWALL" == 0 ]] || FIREWALL=1
SSH_PASSWORD="$(envget RPANEL_SSH_PASSWORD)"; [[ "$SSH_PASSWORD" == 0 ]] || SSH_PASSWORD=1
PROXY_IP="${RPANEL_TRUSTED_PROXY:-}"
ALLOW_FROM="$(envget RPANEL_ALLOW_FROM)"
PANEL_DOMAIN="$(envget RPANEL_PANEL_DOMAIN)"
EMAIL="$(envget RPANEL_EMAIL)"
for arg in "$@"; do
  case "$arg" in
    --mode=*) MODE_ARG="${arg#*=}" ;;
    --trusted-proxy=*) PROXY_IP="${arg#*=}" ;;
    --allow-from=any|--allow-from=) ALLOW_FROM="" ;;
    --allow-from=*) ALLOW_FROM="${arg#*=}" ;;
    --panel-domain=*) PANEL_DOMAIN="${arg#*=}" ;;
    --email=*) EMAIL="${arg#*=}" ;;
    --no-build) BUILD=0 ;;
    --no-firewall) FIREWALL=0 ;;
    --firewall) FIREWALL=1 ;;
    --no-ssh-password) SSH_PASSWORD=0 ;;
    --ssh-password) SSH_PASSWORD=1 ;;
    --dry-run-mode) EXEC_MODE=dry-run ;;
    -h|--help) sed -n '2,23p' "$0"; exit 0 ;;
    *) echo "알 수 없는 옵션: $arg  (--help)" >&2; exit 2 ;;
  esac
done

PANEL_PORT=8443
# IPv6 가 꺼진 서버(일부 VM·컨테이너)에서는 [::] 리스닝을 쓰지 않음
if [[ -s /proc/net/if_inet6 ]]; then IPV6=true; else IPV6=false; fi
say()  { printf '\n\033[1;32m▶ %s\033[0m\n' "$*"; }
info() { printf '  %s\n' "$*"; }
warn() { printf '\033[1;33m! %s\033[0m\n' "$*"; }
die()  { printf '\033[1;31m✘ %s\033[0m\n' "$*" >&2; exit 1; }

[[ $EUID -eq 0 ]] || die "root 로 실행하세요: sudo $0"
. /etc/os-release
[[ "${ID:-}" == "ubuntu" ]] || warn "Ubuntu 24.04 기준 스크립트입니다 — 현재: ${PRETTY_NAME:-unknown}"
export DEBIAN_FRONTEND=noninteractive

genpw() { openssl rand -hex 24; }
mem_mb() { awk '/MemTotal/ {print int($2/1024)}' /proc/meminfo; }

# 모드: 인자 > 기존 설정 > standalone
EDGE="${MODE_ARG:-}"
if [[ -z "$EDGE" ]]; then
  case "$(envget RPANEL_EDGE_MODE)" in proxy) EDGE=proxy ;; *) EDGE=standalone ;; esac
fi
[[ "$EDGE" == standalone || "$EDGE" == proxy ]] || die "--mode 는 standalone 또는 proxy"
[[ -n "$PROXY_IP" ]] || PROXY_IP="$(envget RPANEL_TRUSTED_PROXY)"
if [[ "$EDGE" == proxy ]]; then
  [[ -n "$PROXY_IP" ]] || die "proxy 모드에는 --trusted-proxy=<앞단 프록시 IP> 가 필요합니다"
  [[ "$PROXY_IP" =~ ^[0-9a-fA-F.:/]+$ ]] || die "--trusted-proxy 형식 오류: $PROXY_IP"
else
  PROXY_IP=""
fi
[[ -z "$ALLOW_FROM" ]] || [[ "$ALLOW_FROM" =~ ^[0-9a-fA-F.:/,]+$ ]] || die "--allow-from 형식 오류 (예: 10.0.0.0/24,100.64.0.0/10)"
[[ -z "$PANEL_DOMAIN" ]] || [[ "$PANEL_DOMAIN" =~ ^([a-z0-9]([a-z0-9-]*[a-z0-9])?\.)+[a-z]{2,}$ ]] || die "--panel-domain 형식 오류"
[[ -z "$PANEL_DOMAIN" || -n "$EMAIL" ]] || die "--panel-domain 에는 --email 이 필요합니다"
info "설치 모드: $EDGE${PROXY_IP:+ (앞단 프록시 $PROXY_IP)}"

# ───────────────────────── 1. 기본 ─────────────────────────
say "1/8 기본 패키지"
apt-get update -q >/dev/null
apt-get install -y -q curl ca-certificates gnupg git rsync openssl sudo build-essential pkg-config libssl-dev \
  openssh-client openssh-server nginx mariadb-server mariadb-client redis-server ufw certbot unzip zip >/dev/null
# 가상머신이면 게스트 에이전트 (없어도 무관)
apt-get install -y -q qemu-guest-agent >/dev/null 2>&1 || true
# 시간대가 UTC 기본값이면 서울로 (이미 정했다면 유지)
[[ "$(timedatectl show -p Timezone --value 2>/dev/null || echo UTC)" == "Etc/UTC" || "$(timedatectl show -p Timezone --value 2>/dev/null || echo UTC)" == "UTC" ]] \
  && timedatectl set-timezone "${RPANEL_TZ:-Asia/Seoul}" 2>/dev/null || true
systemctl enable --now qemu-guest-agent >/dev/null 2>&1 || true
info "$(nginx -v 2>&1) · $(mariadb --version | awk '{print $5}' | tr -d ,) · $(redis-server --version | awk '{print $3}')"

# ───────────────────────── 2. MariaDB ─────────────────────────
say "2/8 MariaDB"
systemctl enable --now mariadb >/dev/null 2>&1 || true
# 다른 설정 파일보다 나중에 읽히도록 z- 접두어 (bind-address 를 덮어쓰는 설정이 있어도 확실히 적용)
cat > /etc/mysql/mariadb.conf.d/z-rpanel-bind.cnf <<'EOF'
# managed by rpanel install.sh — DB는 이 서버 안에서만 접속
[mysqld]
bind-address = 127.0.0.1
EOF
if [[ ! -f /etc/mysql/mariadb.conf.d/99-tuning.cnf ]]; then
  BP=$(( $(mem_mb) * 35 / 100 )); (( BP < 256 )) && BP=256
  cat > /etc/mysql/mariadb.conf.d/99-tuning.cnf <<EOF
# rpanel install.sh 가 RAM $(mem_mb)MB 기준으로 생성 — 자유롭게 수정하세요
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
say "3/8 Redis"
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
say "4/8 Node.js 22"
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
say "5/8 사용자 · Rust · 빌드"
id rpanel >/dev/null 2>&1 || useradd --system --home-dir /var/lib/rpanel --create-home --shell /usr/sbin/nologin rpanel
id rpanel-app >/dev/null 2>&1 || useradd --system --no-create-home --home-dir /nonexistent --shell /usr/sbin/nologin rpanel-app
install -d -o rpanel -g rpanel -m 0755 /var/lib/rpanel /var/lib/rpanel/builds /var/lib/rpanel/apps /var/lib/rpanel/www
install -d -o rpanel -g rpanel -m 0750 /var/lib/rpanel/staging /var/lib/rpanel/backups
install -d -o rpanel -g rpanel -m 0700 /var/lib/rpanel/keys
install -d -o root -g rpanel -m 0750 /etc/rpanel
install -d -o root -g root -m 0755 /var/lib/rpanel/acme
getent group rpanel-sites >/dev/null || groupadd --system rpanel-sites

# rpanel 사용자로 실행 (프록시를 쓰는 서버라면 프록시 설정도 함께 전달)
PROXY_ENV=()
for v in http_proxy https_proxy HTTP_PROXY HTTPS_PROXY no_proxy NO_PROXY SSL_CERT_FILE NODE_EXTRA_CA_CERTS; do
  [[ -n "${!v:-}" ]] && PROXY_ENV+=("$v=${!v}")
done
as_rpanel() { sudo -u rpanel -H env "${PROXY_ENV[@]}" "$@"; }

CARGO=/var/lib/rpanel/.cargo/bin/cargo
if [[ ! -x $CARGO ]]; then
  info "Rust 설치 (rustup — 앱 빌드에도 사용)"
  RUSTUP_INIT="$(mktemp /tmp/rustup-init.XXXXXX.sh)"
  curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs -o "$RUSTUP_INIT" \
    || die "Rust 설치 파일을 받지 못했습니다 — 인터넷 연결(https://sh.rustup.rs)을 확인하세요"
  chmod 0644 "$RUSTUP_INIT"
  as_rpanel sh "$RUSTUP_INIT" -y --profile minimal --no-modify-path >/dev/null \
    || { rm -f "$RUSTUP_INIT"; die "Rust 설치에 실패했습니다 — 인터넷 연결을 확인하고 다시 실행하세요"; }
  rm -f "$RUSTUP_INIT"
fi
[[ -x $CARGO ]] || die "Rust(cargo)가 설치되지 않았습니다: $CARGO"
info "$(as_rpanel "$CARGO" --version)"

# 소스 저장소는 건드리지 않고 rpanel 소유 사본에서 빌드 (target/node_modules 보존 → 증분 빌드)
SRC=/var/lib/rpanel/.self-build
if [[ $BUILD -eq 1 ]]; then
  install -d -o rpanel -g rpanel -m 0755 "$SRC"
  rsync -a --delete --exclude .git --exclude target --exclude node_modules --exclude dist "$REPO/" "$SRC/"
  chown -R rpanel:rpanel "$SRC"
  info "백엔드 빌드 (cargo build --release) — 처음엔 5~10분 걸립니다"
  as_rpanel bash -c "cd '$SRC/backend' && '$CARGO' build --release --locked --quiet --bins" \
    || die "백엔드 빌드에 실패했습니다 (위 오류 참고)"
  info "프런트 빌드 (React 19 + Vite + Tailwind 4)"
  as_rpanel bash -c "cd '$SRC/frontend' && npm ci --no-audit --no-fund --loglevel=error >/dev/null && npm run build >/dev/null" \
    || die "화면(프런트) 빌드에 실패했습니다 (위 오류 참고)"
elif [[ -x "$REPO/backend/target/release/rpanel" && -f "$REPO/frontend/dist/index.html" ]]; then
  SRC="$REPO"
fi
[[ -x "$SRC/backend/target/release/rpanel" ]] || die "백엔드 바이너리가 없습니다: $SRC/backend/target/release/rpanel"
[[ -x "$SRC/backend/target/release/rpanel-files" ]] || die "파일 관리 도구가 없습니다: $SRC/backend/target/release/rpanel-files"
[[ -f "$SRC/frontend/dist/index.html" ]] || die "프런트 빌드 결과가 없습니다: $SRC/frontend/dist"

# ───────────────────────── 6. 설치 ─────────────────────────
say "6/8 패널 설치"
install -m 0755 -o root -g root "$SRC/backend/target/release/rpanel" /usr/local/bin/rpanel
install -d -m 0755 /usr/share/rpanel/www
rsync -a --delete "$SRC/frontend/dist/" /usr/share/rpanel/www/
chmod -R a+rX /usr/share/rpanel/www
# 설치된 버전 기록 (rpanel-update 가 새 버전과 비교)
if git -C "$REPO" rev-parse --short HEAD >/dev/null 2>&1; then
  git -C "$REPO" log -1 --format='%h %cd' --date=format:'%Y-%m-%d' > /usr/share/rpanel/VERSION
else
  echo "local $(date +%Y-%m-%d)" > /usr/share/rpanel/VERSION
fi
install -d -m 0755 /usr/local/lib/rpanel
install -m 0755 -o root -g root "$REPO/deploy/rpanel-helper" /usr/local/lib/rpanel/rpanel-helper
install -m 0755 -o root -g root "$SRC/backend/target/release/rpanel-files" /usr/local/lib/rpanel/rpanel-files
# 관리 명령: sudo rpanel-update / sudo rpanel-uninstall
install -m 0755 -o root -g root "$REPO/update.sh" /usr/local/sbin/rpanel-update
install -m 0755 -o root -g root "$REPO/uninstall.sh" /usr/local/sbin/rpanel-uninstall

cat > /etc/sudoers.d/rpanel.tmp <<'EOF'
# Rpanel: 패널은 이 helper 하나만 root로 실행할 수 있습니다 (인자·파일은 helper가 검증)
#         사이트 파일은 해당 사이트 유저 권한으로만 다룹니다 (rpanel-files)
Defaults:rpanel !requiretty, env_reset
rpanel ALL=(root) NOPASSWD: /usr/local/lib/rpanel/rpanel-helper
rpanel ALL=(%rpanel-sites) NOPASSWD: /usr/local/lib/rpanel/rpanel-files
EOF
chmod 0440 /etc/sudoers.d/rpanel.tmp
visudo -cf /etc/sudoers.d/rpanel.tmp >/dev/null || die "sudoers 문법 오류"
mv /etc/sudoers.d/rpanel.tmp /etc/sudoers.d/rpanel

# 환경 파일: 비밀값은 기존 값을 유지하고, 없으면 생성
JWT="$(envget JWT_SECRET)"; [[ ${#JWT} -ge 32 && "$JWT" != *"<"* ]] || JWT="$(openssl rand -hex 32)"
MODE="$(envget RPANEL_EXEC_MODE)"; [[ -n "$MODE" ]] || MODE="$EXEC_MODE"
[[ "$EXEC_MODE" == dry-run ]] && MODE=dry-run
# 패널은 항상 HTTPS(8443) 로 서비스 → 리프레시 쿠키 Secure
COOKIE=true
EXTRA=""
if [[ -f $ENVF ]]; then
  # 스크립트가 관리하지 않는 사용자 추가 설정은 보존
  EXTRA="$(grep -Ev '^(#|$|DATABASE_URL=|REDIS_URL=|JWT_SECRET=|BIND_ADDR=|RPANEL_EXEC_MODE=|COOKIE_SECURE=|RPANEL_EDGE_MODE=|RPANEL_TRUSTED_PROXY=|RPANEL_IPV6=|RPANEL_ALLOW_FROM=|RPANEL_FIREWALL=|RPANEL_SSH_PASSWORD=|RPANEL_PANEL_DOMAIN=|RPANEL_EMAIL=)' "$ENVF" || true)"
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
  echo "RPANEL_EDGE_MODE=$([[ $EDGE == proxy ]] && echo proxy || echo direct)"
  echo "RPANEL_TRUSTED_PROXY=$PROXY_IP"
  echo "RPANEL_IPV6=$IPV6"
  echo "# 설치 옵션 (업데이트 때 그대로 다시 적용)"
  echo "RPANEL_ALLOW_FROM=$ALLOW_FROM"
  echo "RPANEL_FIREWALL=$FIREWALL"
  echo "RPANEL_SSH_PASSWORD=$SSH_PASSWORD"
  echo "RPANEL_PANEL_DOMAIN=$PANEL_DOMAIN"
  echo "RPANEL_EMAIL=$EMAIL"
  if [[ -n "$EXTRA" ]]; then
    echo "$EXTRA"
  else
    echo "RUST_LOG=info"
    echo "RPANEL_APP_PORT_MIN=8100"
    echo "RPANEL_APP_PORT_MAX=8999"
    echo "RPANEL_MAX_CONCURRENT_BUILDS=2"
    echo "RPANEL_BACKUP_HOUR=3"
  fi
} > "$ENVF.new"
chown root:rpanel "$ENVF.new"; chmod 0640 "$ENVF.new"; mv "$ENVF.new" "$ENVF"

install -m 0644 "$REPO/deploy/systemd/rpanel.service" /etc/systemd/system/rpanel.service
systemctl daemon-reload
systemctl enable rpanel >/dev/null 2>&1

# ───────────────────────── Nginx ─────────────────────────
[[ -L /etc/nginx/sites-enabled/default ]] && rm -f /etc/nginx/sites-enabled/default && info "Nginx 기본 사이트 비활성화"

# 기본 서버(등록되지 않은 도메인·IP 직접 접속)는 응답 없이 끊고, 인증서용 ACME 경로만 엽니다
CERT_DIR=/etc/rpanel/panel-cert
install -d -m 0700 -o root -g root "$CERT_DIR"
if [[ ! -f "$CERT_DIR/selfsigned.pem" ]]; then
  openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -subj "/CN=rpanel" \
    -keyout "$CERT_DIR/selfsigned.key" -out "$CERT_DIR/selfsigned.pem" >/dev/null 2>&1
  chmod 0600 "$CERT_DIR/selfsigned.key"
  info "패널용 자체 서명 인증서 생성"
fi
if [[ "$EDGE" == proxy ]]; then
  PROTO_MAP='map $http_x_forwarded_proto $rpanel_proto { default $scheme; https https; http http; }'
else
  PROTO_MAP='map $scheme $rpanel_proto { default $scheme; }'
fi
cat > /etc/nginx/conf.d/rpanel.conf <<EOF
# managed by rpanel install.sh
server_names_hash_bucket_size 128;
$PROTO_MAP
map \$rpanel_proto \$rpanel_https { https on; default ""; }

server {
    listen 80 default_server;
    listen [::]:80 default_server;
    server_name _;
    location ^~ /.well-known/acme-challenge/ {
        root /var/lib/rpanel/acme;
        default_type "text/plain";
        try_files \$uri =404;
    }
    location / { return 444; }
}

server {
    listen 443 ssl http2 default_server;
    listen [::]:443 ssl http2 default_server;
    server_name _;
    ssl_reject_handshake on;
}
EOF
[[ $IPV6 == true ]] || sed -i '/listen \[::\]/d' /etc/nginx/conf.d/rpanel.conf

PANEL_CERT="$CERT_DIR/selfsigned.pem"; PANEL_KEY="$CERT_DIR/selfsigned.key"
if [[ -f "/etc/letsencrypt/live/rpanel-panel/fullchain.pem" ]]; then
  PANEL_CERT=/etc/letsencrypt/live/rpanel-panel/fullchain.pem; PANEL_KEY=/etc/letsencrypt/live/rpanel-panel/privkey.pem
fi
write_panel_conf() {
  sed -e "s|__CERT__|$PANEL_CERT|; s|__KEY__|$PANEL_KEY|; s|__PORT__|$PANEL_PORT|g" \
    "$REPO/deploy/nginx/rpanel-panel.conf" > /etc/nginx/sites-available/rpanel-panel.conf
  [[ $IPV6 == true ]] || sed -i '/listen \[::\]/d' /etc/nginx/sites-available/rpanel-panel.conf
  ln -sfn /etc/nginx/sites-available/rpanel-panel.conf /etc/nginx/sites-enabled/rpanel-panel.conf
}
write_panel_conf
nginx -t >/dev/null 2>&1 || { nginx -t; die "Nginx 설정 오류"; }
systemctl enable nginx >/dev/null 2>&1 || true
systemctl reload nginx 2>/dev/null || systemctl restart nginx

# Let's Encrypt 갱신 시 Nginx 재적용
install -d -m 0755 /etc/letsencrypt/renewal-hooks/deploy
cat > /etc/letsencrypt/renewal-hooks/deploy/rpanel-nginx-reload <<'EOF'
#!/bin/sh
# managed by rpanel — 인증서 갱신 후 Nginx 재적용
systemctl reload nginx
EOF
chmod 0755 /etc/letsencrypt/renewal-hooks/deploy/rpanel-nginx-reload
systemctl enable --now certbot.timer >/dev/null 2>&1 || true

# 패널 도메인 인증서 (standalone)
if [[ -n "$PANEL_DOMAIN" ]]; then
  if [[ "$EDGE" == standalone ]]; then
    if certbot certonly --webroot -w /var/lib/rpanel/acme --cert-name rpanel-panel -d "$PANEL_DOMAIN" \
         --non-interactive --agree-tos -m "$EMAIL" --keep-until-expiring >/dev/null 2>&1; then
      PANEL_CERT=/etc/letsencrypt/live/rpanel-panel/fullchain.pem; PANEL_KEY=/etc/letsencrypt/live/rpanel-panel/privkey.pem
      write_panel_conf
      nginx -t >/dev/null 2>&1 && systemctl reload nginx
      info "패널 인증서 발급: https://$PANEL_DOMAIN:$PANEL_PORT"
    else
      warn "패널 도메인 인증서 발급 실패 — DNS 가 이 서버를 가리키는지, 80 포트가 열려 있는지 확인하세요 (자체 서명 인증서로 계속)"
    fi
  else
    warn "proxy 모드에서는 --panel-domain 인증서를 발급하지 않습니다 (앞단 프록시에서 처리하세요)"
  fi
fi

# ───────────────────────── 사이트 유저 SSH/SFTP ─────────────────────────
if [[ $SSH_PASSWORD -eq 0 && -f /etc/ssh/sshd_config.d/50-rpanel-sites.conf ]]; then
  rm -f /etc/ssh/sshd_config.d/50-rpanel-sites.conf
  systemctl reload ssh 2>/dev/null || systemctl reload sshd 2>/dev/null || true
  info "사이트 유저 SSH/SFTP 비밀번호 로그인 끔 (공개키만)"
fi
if [[ $SSH_PASSWORD -eq 1 && -d /etc/ssh/sshd_config.d ]]; then
  cat > /etc/ssh/sshd_config.d/50-rpanel-sites.conf <<'EOF'
# managed by rpanel — 사이트 유저(rpanel-sites 그룹)는 비밀번호로 SSH/SFTP 접속 가능
Match Group rpanel-sites
    PasswordAuthentication yes
    KbdInteractiveAuthentication yes
Match all
EOF
  install -d -m 0755 /run/sshd   # sshd -t 에 필요 (ssh 서비스가 아직 안 떴을 때)
  if sshd -t 2>/dev/null; then
    systemctl reload ssh 2>/dev/null || systemctl reload sshd 2>/dev/null || true
    info "사이트 유저 SSH/SFTP 비밀번호 로그인 허용"
  else
    rm -f /etc/ssh/sshd_config.d/50-rpanel-sites.conf; warn "sshd 설정 검사 실패 — 사이트 유저 비밀번호 로그인 설정을 건너뜁니다"
  fi
fi

# ───────────────────────── 7. 시작 ─────────────────────────
say "7/8 패널 시작"
systemctl restart rpanel
ok=0
for _ in $(seq 1 30); do
  if curl -fsS http://127.0.0.1:8090/api/health 2>/dev/null | grep -q '"status":"ok"'; then ok=1; break; fi
  sleep 1
done
(( ok )) || die "패널이 시작되지 않았습니다: journalctl -u rpanel -n 50 --no-pager"
info "패널 동작 확인 (db·redis ok)"

# ───────────────────────── 8. 방화벽 ─────────────────────────
say "8/8 방화벽 (ufw)"
if [[ $FIREWALL -eq 1 ]]; then
  # 지금 SSH 로 접속해 있는 주소는 잠기지 않도록 허용 (sudo·파이프 실행에서도 동작하도록 소켓에서 직접 읽음)
  SSH_FROM="$(ss -Htn state established '( sport = :22 )' 2>/dev/null | awk '{print $4}' | sed -E 's/:[0-9]+$//; s/^\[//; s/\]$//; s/^::ffff://' | sort -u | tr '\n' ' ')"
  ufw default deny incoming >/dev/null
  ufw default allow outgoing >/dev/null
  if [[ -n "$ALLOW_FROM" ]]; then
    for net in ${ALLOW_FROM//,/ }; do
      ufw allow from "$net" to any port 22 proto tcp >/dev/null
      ufw allow from "$net" to any port "$PANEL_PORT" proto tcp >/dev/null
    done
    for ip in $SSH_FROM; do
      ufw allow from "$ip" to any port 22 proto tcp >/dev/null && info "현재 SSH 접속 주소 $ip 허용"
    done
    ADMIN_DESC="${ALLOW_FROM//,/, }"
  else
    ufw allow 22/tcp >/dev/null
    ufw allow "$PANEL_PORT/tcp" >/dev/null
    ADMIN_DESC="어디서나"
  fi
  if [[ "$EDGE" == proxy ]]; then
    ufw allow from "$PROXY_IP" to any port 80 proto tcp >/dev/null
    WEB_DESC="80 ← $PROXY_IP (앞단 프록시)"
  else
    ufw allow 80/tcp >/dev/null
    ufw allow 443/tcp >/dev/null
    WEB_DESC="80·443 ← 어디서나"
  fi
  ufw --force enable >/dev/null
  info "22·$PANEL_PORT ← $ADMIN_DESC  |  $WEB_DESC"
else
  info "건너뜀 (--no-firewall)"
fi

if [[ -n "$PANEL_DOMAIN" && "$PANEL_CERT" == /etc/letsencrypt/* ]]; then
  URL="https://$PANEL_DOMAIN:$PANEL_PORT"
else
  IP="$(hostname -I 2>/dev/null | awk '{print $1}')"
  URL="https://${IP:-<서버IP>}:$PANEL_PORT   (자체 서명 인증서 — 브라우저 경고에서 '계속' 선택)"
fi
cat <<EOF

$(printf '\033[1;32m')✔ Rpanel ${RPANEL_DONE_WORD:-설치} 완료$(printf '\033[0m')

  접속      $URL
$([[ -z "${RPANEL_DONE_WORD:-}" ]] && echo "            → 첫 화면에서 관리자 계정을 만드세요")
  모드      $EDGE$([[ $EDGE == proxy ]] && echo " — SSL 은 앞단 프록시($PROXY_IP)에서, 도메인은 프록시에서 이 서버 :80 으로 연결")
  실행 모드  $MODE$( [[ $MODE == dry-run ]] && echo "  — 실제 배포하려면 $ENVF 에서 RPANEL_EXEC_MODE=real 후 systemctl restart rpanel")
  설정·비밀  $ENVF
  업데이트   sudo rpanel-update
  삭제      sudo rpanel-uninstall
  로그      journalctl -u rpanel -f
EOF
