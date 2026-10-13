#!/bin/bash
# Rpanel 설치/업데이트 스크립트 — VM102 (Ubuntu 24.04, Vol.6 구성) 에서 root 로 실행
#
#   sudo -i
#   cd /srv/build/rpanel
#   ./deploy/install.sh            # 빌드 + 설치 (처음/업데이트 모두)
#   ./deploy/install.sh --no-build # 이미 빌드된 결과물만 설치
#
# 하는 일:
#   1. 사용자: rpanel(패널), rpanel-app(앱 실행 전용, 로그인 불가)
#   2. 디렉토리: /var/lib/rpanel/{builds,apps,www,staging,keys}
#   3. rpanel 사용자용 Rust 툴체인(rustup) — 앱 빌드에 사용
#   4. 백엔드 빌드 → /usr/local/bin/rpanel, 프런트 빌드 → /usr/share/rpanel/www
#   5. /usr/local/lib/rpanel/rpanel-helper + sudoers (helper 한 개만 허용)
#   6. /etc/rpanel/rpanel.env (없을 때만 생성), systemd rpanel.service
#   7. Nginx :9000 패널 사이트 + ufw (Tailscale/내부망만)
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD=1
[[ "${1:-}" == "--no-build" ]] && BUILD=0

say() { printf '\n\033[1;32m▶ %s\033[0m\n' "$*"; }
warn() { printf '\033[1;33m! %s\033[0m\n' "$*"; }
die() { printf '\033[1;31m✘ %s\033[0m\n' "$*" >&2; exit 1; }

[[ $EUID -eq 0 ]] || die "root로 실행하세요 (sudo -i)"
. /etc/os-release
[[ "${ID:-}" == "ubuntu" ]] || warn "Ubuntu 24.04(VM102) 기준 스크립트입니다 — 현재: ${PRETTY_NAME:-unknown}"

say "패키지 확인"
apt-get install -y -q git rsync curl ca-certificates build-essential pkg-config libssl-dev nginx sudo openssh-client >/dev/null
command -v node >/dev/null || die "Node.js가 없습니다. Vol.6 §9.8 (NodeSource 22.x)을 먼저 진행하세요"
node -e 'const [a,b]=process.versions.node.split(".").map(Number); process.exit((a>22||(a===22&&b>=12)||(a===20&&b>=19))?0:1)' \
  || die "Node $(node -v) — Vite 빌드에는 20.19+ 또는 22.12+ 가 필요합니다 (Vol.6 §9.8)"
systemctl is-active --quiet mariadb || warn "mariadb 서비스가 실행 중이 아닙니다 (Vol.6 §9.7)"
systemctl is-active --quiet redis-server || warn "redis-server 서비스가 실행 중이 아닙니다 (Vol.6 §9.7.1)"

say "사용자 / 디렉토리"
id rpanel >/dev/null 2>&1 || useradd --system --home-dir /var/lib/rpanel --create-home --shell /usr/sbin/nologin rpanel
id rpanel-app >/dev/null 2>&1 || useradd --system --no-create-home --home-dir /nonexistent --shell /usr/sbin/nologin rpanel-app
install -d -o rpanel -g rpanel -m 0755 /var/lib/rpanel /var/lib/rpanel/builds /var/lib/rpanel/apps /var/lib/rpanel/www
install -d -o rpanel -g rpanel -m 0750 /var/lib/rpanel/staging
install -d -o rpanel -g rpanel -m 0700 /var/lib/rpanel/keys
install -d -o root -g rpanel -m 0750 /etc/rpanel

say "rpanel 사용자 Rust 툴체인 (앱 빌드용)"
if [[ ! -x /var/lib/rpanel/.cargo/bin/cargo ]]; then
  sudo -u rpanel -H bash -c 'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --no-modify-path'
fi
sudo -u rpanel -H /var/lib/rpanel/.cargo/bin/cargo --version

# 소스 저장소(/srv/build/rpanel 등, 배포 계정 소유)는 건드리지 않고 rpanel 소유 사본에서 빌드
SRC=/var/lib/rpanel/.self-build
if [[ $BUILD -eq 1 ]]; then
  say "소스 동기화 → $SRC"
  install -d -o rpanel -g rpanel -m 0755 "$SRC"
  # target/ node_modules/ dist/ 는 제외(=보존)해서 증분 빌드
  rsync -a --delete --exclude .git --exclude target --exclude node_modules --exclude dist "$REPO/" "$SRC/"
  chown -R rpanel:rpanel "$SRC"

  say "백엔드 빌드 (cargo build --release) — 처음엔 수 분 걸립니다"
  sudo -u rpanel -H bash -c "cd '$SRC/backend' && /var/lib/rpanel/.cargo/bin/cargo build --release --locked"

  say "프런트 빌드 (React 19 + Vite + Tailwind 4)"
  sudo -u rpanel -H bash -c "cd '$SRC/frontend' && npm ci --no-audit --no-fund && npm run build"
else
  SRC="$REPO"
fi
[[ -x "$SRC/backend/target/release/rpanel" ]] || die "백엔드 바이너리가 없습니다: $SRC/backend/target/release/rpanel"
[[ -f "$SRC/frontend/dist/index.html" ]] || die "프런트 빌드 결과가 없습니다: $SRC/frontend/dist"

say "설치"
install -m 0755 -o root -g root "$SRC/backend/target/release/rpanel" /usr/local/bin/rpanel
install -d -m 0755 /usr/share/rpanel/www
rsync -a --delete "$SRC/frontend/dist/" /usr/share/rpanel/www/
chmod -R a+rX /usr/share/rpanel/www

install -d -m 0755 /usr/local/lib/rpanel
install -m 0755 -o root -g root "$REPO/deploy/rpanel-helper" /usr/local/lib/rpanel/rpanel-helper

SUDOERS=/etc/sudoers.d/rpanel
cat > "$SUDOERS.tmp" <<'EOF'
# Rpanel: 패널은 이 helper 하나만 root로 실행할 수 있습니다 (인자·파일은 helper가 검증)
Defaults:rpanel !requiretty, env_reset
rpanel ALL=(root) NOPASSWD: /usr/local/lib/rpanel/rpanel-helper
EOF
chmod 0440 "$SUDOERS.tmp"
visudo -cf "$SUDOERS.tmp" >/dev/null || die "sudoers 문법 오류"
mv "$SUDOERS.tmp" "$SUDOERS"

ENVF=/etc/rpanel/rpanel.env
if [[ ! -f $ENVF ]]; then
  say "환경 파일 생성: $ENVF"
  sed "s/^JWT_SECRET=.*/JWT_SECRET=$(openssl rand -hex 32)/" "$REPO/deploy/rpanel.env.example" > "$ENVF"
  chown root:rpanel "$ENVF"; chmod 0640 "$ENVF"
  warn "$ENVF 의 DATABASE_URL / REDIS_URL 비밀번호를 채워야 패널이 시작됩니다 (README '2. DB·Redis 준비')"
fi

say "systemd"
install -m 0644 "$REPO/deploy/systemd/rpanel.service" /etc/systemd/system/rpanel.service
systemctl daemon-reload
systemctl enable rpanel >/dev/null 2>&1

say "Nginx :9000 (패널)"
install -m 0644 "$REPO/deploy/nginx/rpanel-panel.conf" /etc/nginx/sites-available/rpanel-panel.conf
ln -sfn /etc/nginx/sites-available/rpanel-panel.conf /etc/nginx/sites-enabled/rpanel-panel.conf
nginx -t && systemctl reload nginx

if command -v ufw >/dev/null && ufw status | grep -q "Status: active"; then
  say "ufw: 9000 은 Tailscale(100.64.0.0/10)·내부망(10.0.0.0/24)만"
  ufw allow from 100.64.0.0/10 to any port 9000 proto tcp >/dev/null
  ufw allow from 10.0.0.0/24 to any port 9000 proto tcp >/dev/null
fi

if grep -q '<DB_비밀번호>\|<Redis_비밀번호>' "$ENVF"; then
  warn "아직 $ENVF 에 자리표시자가 남아 있어 패널을 시작하지 않았습니다. 값을 채운 뒤: systemctl restart rpanel"
else
  systemctl restart rpanel
  sleep 2
  if curl -fsS http://127.0.0.1:8090/api/health >/dev/null; then
    say "완료 — Tailscale이 켜진 PC에서 http://10.0.0.12:9000 접속 → 첫 관리자 계정 생성"
  else
    warn "패널이 아직 응답하지 않습니다: journalctl -u rpanel -n 50 --no-pager"
  fi
fi
