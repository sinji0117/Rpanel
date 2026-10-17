#!/bin/bash
# ──────────────────────────────────────────────────────────────
#  Rpanel 설치
#
#  Ubuntu 24.04 를 막 설치한 서버에서 아래 한 줄이면 끝납니다:
#
#    curl -fsSL https://raw.githubusercontent.com/sinji0117/Rpanel/main/install.sh | sudo bash
#
#  옵션은 뒤에 붙입니다 (자세한 목록: --help):
#    curl -fsSL .../install.sh | sudo bash -s -- --mode=proxy --trusted-proxy=10.0.0.10
#
#  업데이트: sudo rpanel-update      삭제: sudo rpanel-uninstall
#
#  환경변수: RPANEL_REPO   소스 저장소 (기본: GitHub sinji0117/Rpanel)
#            RPANEL_BRANCH 브랜치 (기본: main)
#            RPANEL_SRC    소스를 받아 둘 위치 (기본: /opt/rpanel-src)
# ──────────────────────────────────────────────────────────────
set -euo pipefail

REPO_URL="${RPANEL_REPO:-https://github.com/sinji0117/Rpanel.git}"
BRANCH="${RPANEL_BRANCH:-main}"
SRC="${RPANEL_SRC:-/opt/rpanel-src}"

if [[ $EUID -ne 0 ]]; then
  echo "관리자 권한이 필요합니다. 앞에 sudo 를 붙여 실행하세요:" >&2
  echo "  curl -fsSL https://raw.githubusercontent.com/sinji0117/Rpanel/main/install.sh | sudo bash" >&2
  exit 1
fi

for a in "$@"; do
  if [[ "$a" == -h || "$a" == --help ]]; then
    sed -n '2,17p' "${BASH_SOURCE[0]}" 2>/dev/null || true
    cat <<'EOF'
  설치 옵션
    --mode=standalone        (기본) 이 서버가 인터넷에서 80/443 을 직접 받음. https 인증서도 여기서 자동 발급
    --mode=proxy             앞에 다른 리버스 프록시(Nginx Proxy Manager 등)가 있고 https 는 거기서 처리
    --trusted-proxy=IP       proxy 모드일 때 앞단 프록시의 IP
    --allow-from=CIDR,...    SSH·패널(8443)에 접속할 수 있는 주소 대역 (기본: 어디서나)
    --panel-domain=DOMAIN    패널을 도메인으로 접속 (예: panel.example.com, --email 필요)
    --email=EMAIL            인증서(Let's Encrypt) 알림을 받을 이메일
    --no-firewall            방화벽(ufw)을 건드리지 않음
    --no-ssh-password        사이트 계정의 SSH/SFTP 비밀번호 로그인을 켜지 않음 (키만 허용)
    --dry-run-mode           실제로 배포하지 않고 둘러보기만
EOF
    exit 0
  fi
done

export DEBIAN_FRONTEND=noninteractive
. /etc/os-release 2>/dev/null || true
if [[ "${ID:-}" != ubuntu ]]; then
  echo "! Ubuntu 24.04 용으로 만들어졌습니다 (현재: ${PRETTY_NAME:-알 수 없음}). 계속 진행합니다." >&2
fi

# 저장소를 이미 받아 둔 경우(./install.sh 로 실행) 그 소스를 그대로 씁니다
HERE=""
if [[ -n "${BASH_SOURCE[0]:-}" && -f "${BASH_SOURCE[0]}" ]]; then
  HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
fi
if [[ -n "$HERE" && -f "$HERE/deploy/install.sh" && "$HERE" != /usr/local/* ]]; then
  echo "▶ 이 폴더의 소스로 설치합니다: $HERE"
  exec bash "$HERE/deploy/install.sh" "$@"
fi

if ! command -v git >/dev/null; then
  echo "▶ git 설치"
  apt-get update -q >/dev/null
  apt-get install -y -q git ca-certificates >/dev/null
fi

if [[ -d "$SRC/.git" ]]; then
  echo "▶ 소스 업데이트: $SRC ($BRANCH)"
  git -C "$SRC" remote set-url origin "$REPO_URL"
  git -C "$SRC" fetch -q --depth 1 origin "$BRANCH"
  git -C "$SRC" reset -q --hard FETCH_HEAD
else
  echo "▶ 소스 받기: $REPO_URL → $SRC"
  rm -rf "$SRC"
  git clone -q --depth 1 --branch "$BRANCH" "$REPO_URL" "$SRC" || {
    echo "✘ 소스를 받지 못했습니다. 인터넷 연결과 저장소 주소($REPO_URL)를 확인하세요." >&2
    exit 1
  }
fi
echo "  버전 $(git -C "$SRC" log -1 --format='%h (%cd)' --date=format:'%Y-%m-%d')"

exec bash "$SRC/deploy/install.sh" "$@"
