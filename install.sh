#!/bin/bash
# ──────────────────────────────────────────────────────────────
#  Rpanel 설치 (올인원)
#
#  Ubuntu 24.04 를 막 설치한 서버에서 아래 한 줄이면 끝납니다:
#
#    curl -fsSL https://raw.githubusercontent.com/sinji0117/Rpanel/main/install.sh | sudo bash
#
#  설치 마법사가 몇 가지(도메인 등)만 묻고, 미리 빌드한 묶음을 받아 5~10분 안에 설치합니다.
#  모르는 질문은 Enter 만 누르면 됩니다. 묻지 않고 기본값으로 설치하려면 --yes
#
#  업데이트: sudo rpanel-update      삭제: sudo rpanel-uninstall
#
#  환경변수: RPANEL_GH_REPO  릴리스를 받을 GitHub 저장소 (기본: sinji0117/Rpanel)
#            RPANEL_RELEASE_BASE 묶음을 받을 주소 (기본: https://github.com/<저장소>/releases, 미러용)
#            RPANEL_REPO     소스 저장소 (기본: GitHub sinji0117/Rpanel)
#            RPANEL_BRANCH   브랜치 (기본: main)
#            RPANEL_SRC      소스를 받아 둘 위치 (기본: /opt/rpanel-src)
# ──────────────────────────────────────────────────────────────
set -euo pipefail

GH_REPO="${RPANEL_GH_REPO:-sinji0117/Rpanel}"
REPO_URL="${RPANEL_REPO:-https://github.com/$GH_REPO.git}"
BRANCH="${RPANEL_BRANCH:-main}"
SRC="${RPANEL_SRC:-/opt/rpanel-src}"
RELEASE_BASE="${RPANEL_RELEASE_BASE:-https://github.com/$GH_REPO/releases}"
PKG="/opt/rpanel-pkg"
ENVF=/etc/rpanel/rpanel.env
ONE_LINE="curl -fsSL https://raw.githubusercontent.com/$GH_REPO/main/install.sh | sudo bash"

say()  { printf '\n\033[1;32m▶ %s\033[0m\n' "$*"; }
info() { printf '  %s\n' "$*"; }
warn() { printf '\033[1;33m! %s\033[0m\n' "$*" >&2; }
die()  { printf '\033[1;31m✘ %s\033[0m\n' "$*" >&2; exit 1; }

usage() {
  sed -n '2,19p' "${BASH_SOURCE[0]}" 2>/dev/null || true
  cat <<'EOF'
  설치 옵션 (옵션을 하나라도 붙이면 마법사는 묻지 않고 그 값으로 설치합니다)
    -y, --yes                묻지 않고 기본값으로 설치
    --wizard                 옵션을 붙였어도 마법사로 묻기
    --mode=standalone        (기본) 이 서버가 인터넷에서 80/443 을 직접 받음. https 인증서도 여기서 자동 발급
    --mode=proxy             앞에 다른 리버스 프록시(Nginx Proxy Manager 등)가 있고 https 는 거기서 처리
    --trusted-proxy=IP       proxy 모드일 때 앞단 프록시의 IP
    --allow-from=CIDR,...    SSH·패널(8443)에 접속할 수 있는 주소 대역 (기본: 어디서나)
    --panel-domain=DOMAIN    패널을 도메인으로 접속 (예: panel.example.com, --email 필요)
    --email=EMAIL            인증서(Let's Encrypt) 알림을 받을 이메일
    --no-mongodb             MongoDB 를 설치하지 않음 (메모리가 적은 서버)
    --no-firewall            방화벽(ufw)을 건드리지 않음
    --no-ssh-password        사이트 계정의 SSH/SFTP 비밀번호 로그인을 켜지 않음 (키만 허용)
    --no-swap                메모리가 적어도 스왑 파일을 만들지 않음
    --dry-run-mode           실제로 배포하지 않고 둘러보기만

  받는 방식
    (기본)                   미리 빌드한 묶음(GitHub 릴리스) → 없으면 소스를 받아 서버에서 빌드
    --version=v0.16.0        특정 버전의 묶음
    --from-source            묶음을 쓰지 않고 main 브랜치 소스로 빌드 (메모리 4GB 이상 권장, 15~25분)
    --force                  다른 관리 패널이 깔려 있어도 설치
EOF
}

# ── 옵션 ──
YES=0; WIZARD=0; FORCE=0; FROM=""; WANT_VER=""
PASS=()
for a in "$@"; do
  case "$a" in
    -h|--help) usage; exit 0 ;;
    -y|--yes) YES=1 ;;
    --wizard) WIZARD=1 ;;
    --force) FORCE=1 ;;
    --from-source) FROM=source ;;
    --from-release) FROM=release ;;
    --version=*) WANT_VER="${a#*=}"; [[ "$WANT_VER" == v* ]] || WANT_VER="v$WANT_VER"; FROM=release ;;
    *) PASS+=("$a") ;;
  esac
done

if [[ $EUID -ne 0 ]]; then
  echo "관리자 권한이 필요합니다. 앞에 sudo 를 붙여 실행하세요:" >&2
  echo "  $ONE_LINE" >&2
  exit 1
fi
export DEBIAN_FRONTEND=noninteractive

# 받는 방식: 옵션 > 지난번 설치 때 고른 값 > auto
if [[ -z "$FROM" && -f $ENVF ]]; then
  FROM="$(grep -E '^RPANEL_INSTALL_FROM=' "$ENVF" | tail -n1 | cut -d= -f2- || true)"
fi
case "$FROM" in release|source) ;; *) FROM=auto ;; esac

# ── 키보드 입력 (curl | bash 로 실행해도 /dev/tty 에서 읽음) ──
TTYIN=0
if [[ -t 0 ]]; then TTYIN=1; elif { : </dev/tty; } 2>/dev/null; then TTYIN=1; fi
ask() { # ask <변수> <질문> [기본값]
  local __v __def="${3:-}"
  if [[ -t 0 ]]; then read -r -p "$2" __v || __v=""; else read -r -p "$2" __v </dev/tty || __v=""; fi
  printf -v "$1" '%s' "${__v:-$__def}"
}
yesno() { # yesno <질문> <기본 y|n> → 0=예
  local a def="$2" hint
  [[ $def == y ]] && hint="[Y/n]" || hint="[y/N]"
  while true; do
    ask a "$1 $hint " "$def"
    case "${a,,}" in y|yes|예|ㅇ|네) return 0 ;; n|no|아니오|아니요|ㄴ) return 1 ;; esac
    echo "  y(예) 또는 n(아니오)으로 답하세요"
  done
}

# ── 서버 확인 ──
. /etc/os-release 2>/dev/null || true
ARCH_RAW="$(uname -m)"
case "$ARCH_RAW" in x86_64|amd64) ARCH=x86_64 ;; aarch64|arm64) ARCH=aarch64 ;; *) ARCH="" ;; esac
MEM_MB="$(awk '/MemTotal/ {print int($2/1024)}' /proc/meminfo)"
SWAP_MB="$(awk '/SwapTotal/ {print int($2/1024)}' /proc/meminfo)"
DISK_GB="$(df -Pk / | awk 'NR==2 {print int($4/1024/1024)}')"
CPUS="$(nproc 2>/dev/null || echo 1)"
FRESH=1; [[ -f $ENVF && -x /usr/local/bin/rpanel ]] && FRESH=0

OTHER_PANEL=""
for p in /usr/local/cpanel:cPanel /www/server/panel:aaPanel /usr/local/hestia:HestiaCP /usr/local/vesta:VestaCP \
         /usr/local/CyberCP:CyberPanel /usr/local/psa:Plesk /usr/local/directadmin:DirectAdmin /usr/local/cwpsrv:CWP; do
  [[ -e "${p%%:*}" ]] && OTHER_PANEL="${p#*:}" && break
done
BUSY_PORTS=""
if [[ $FRESH -eq 1 ]] && command -v ss >/dev/null; then
  for port in 80 443 8443; do
    who="$(ss -Hltnp "sport = :$port" 2>/dev/null | grep -o 'users:(("[^"]*' | head -n1 | cut -d'"' -f2 || true)"
    [[ -n "$who" && "$who" != nginx ]] && BUSY_PORTS+="$port($who) "
  done
fi
public_ip() {
  local ip
  ip="$(curl -fsS --max-time 4 https://1.1.1.1/cdn-cgi/trace 2>/dev/null | sed -n 's/^ip=//p' || true)"
  [[ -n "$ip" ]] || ip="$(hostname -I 2>/dev/null | awk '{print $1}')"
  echo "${ip:-알 수 없음}"
}
mark() { if "$@"; then printf '\033[32mOK\033[0m'; else printf '\033[33m주의\033[0m'; fi; }

preflight() {
  PUB_IP="$(public_ip)"
  echo
  printf '\033[1m서버 확인\033[0m\n'
  printf '  운영체제   %-28s %s\n' "${PRETTY_NAME:-알 수 없음}" "$(mark test "${ID:-}-${VERSION_ID:-}" = ubuntu-24.04)"
  printf '  CPU       %-28s %s\n' "$ARCH_RAW · ${CPUS}코어" "$(mark test -n "$ARCH")"
  printf '  메모리     %-28s %s\n' "${MEM_MB}MB (스왑 ${SWAP_MB}MB)" "$(mark test "$MEM_MB" -ge 1900)"
  printf '  디스크     %-28s %s\n' "남은 공간 ${DISK_GB}GB" "$(mark test "$DISK_GB" -ge 10)"
  printf '  서버 IP    %s\n' "$PUB_IP"
  [[ "${ID:-}" == ubuntu ]] || warn "Ubuntu 24.04 용으로 만들어졌습니다. 다른 OS 에서는 동작을 보장하지 않습니다."
  [[ -n "$ARCH" ]] || warn "미리 빌드한 묶음이 없는 CPU 입니다 ($ARCH_RAW) — 서버에서 직접 빌드합니다."
  (( MEM_MB >= 1900 )) || warn "메모리가 2GB 보다 적습니다 — 2GB 스왑 파일을 자동으로 만들고, MongoDB 는 빼는 것을 권장합니다."
  (( DISK_GB >= 10 )) || warn "디스크 여유 공간이 10GB 보다 적습니다 — 설치 중 공간이 모자랄 수 있습니다."
  [[ -z "$BUSY_PORTS" ]] || warn "이미 다른 프로그램이 쓰는 포트가 있습니다: $BUSY_PORTS— 그 프로그램을 멈추지 않으면 Rpanel 이 시작되지 않을 수 있습니다."
  if [[ -n "$OTHER_PANEL" ]]; then
    warn "다른 서버 관리 패널($OTHER_PANEL)이 설치되어 있습니다. 함께 쓰면 설정이 서로 망가집니다. 깨끗한 서버에 설치하세요."
    if [[ $FORCE -eq 0 ]]; then
      if [[ $TTYIN -eq 1 && $YES -eq 0 ]]; then
        yesno "  그래도 설치할까요?" n || exit 1
      else
        die "설치를 멈췄습니다. 정말 설치하려면 --force 를 붙이세요."
      fi
    fi
  fi
}

valid_domain() { [[ "$1" =~ ^([a-z0-9]([a-z0-9-]*[a-z0-9])?\.)+[a-z]{2,}$ ]]; }
valid_email()  { [[ "$1" =~ ^[^@[:space:]]+@[^@[:space:]]+\.[^@[:space:]]+$ ]]; }
valid_ips()    { [[ "$1" =~ ^[0-9a-fA-F.:/,]+$ ]]; }

# ── 설치 마법사 ──
wizard() {
  local d e ip cidr dns
  echo
  printf '\033[1;36m══════════════════════════════════════════════\033[0m\n'
  printf '\033[1;36m   Rpanel 설치 마법사\033[0m\n'
  printf '\033[1;36m══════════════════════════════════════════════\033[0m\n'
  echo "  몇 가지만 물어볼게요. 잘 모르면 그냥 Enter 를 누르세요 (기본값으로 진행)."

  # 1. 도메인
  echo
  printf '\033[1m1. 패널을 도메인으로 열까요?\033[0m\n'
  echo "   예: panel.example.com  (도메인의 DNS 가 이 서버 IP($PUB_IP)를 가리켜야 합니다)"
  echo "   없으면 Enter → https://$PUB_IP:8443 으로 엽니다 (나중에 패널 설정에서 바꿀 수 있어요)"
  while true; do
    ask d "   도메인: " ""
    d="${d,,}"; d="${d#http://}"; d="${d#https://}"; d="${d%%/*}"
    [[ -z "$d" ]] && break
    if ! valid_domain "$d"; then echo "   도메인 형식이 아닙니다 (예: panel.example.com)"; continue; fi
    dns="$(getent ahostsv4 "$d" 2>/dev/null | awk '{print $1}' | sort -u | tr '\n' ' ' || true)"
    if [[ -z "$dns" ]]; then
      warn "   $d 의 DNS 를 찾지 못했습니다. 인증서 발급이 실패하고 IP 주소로 열립니다."
      yesno "   그래도 이 도메인을 쓸까요?" n || continue
    elif [[ "$PUB_IP" != "알 수 없음" && " $dns" != *" $PUB_IP "* ]]; then
      warn "   $d 는 $dns를 가리킵니다 (이 서버: $PUB_IP)."
      yesno "   그래도 이 도메인을 쓸까요?" n || continue
    fi
    break
  done
  W_DOMAIN="$d"
  W_EMAIL=""
  if [[ -n "$W_DOMAIN" ]]; then
    while true; do
      ask e "   인증서 만료 알림을 받을 이메일: " ""
      valid_email "$e" && break
      echo "   이메일 형식이 아닙니다"
    done
    W_EMAIL="$e"
  fi

  # 2. MongoDB
  echo
  printf '\033[1m2. MongoDB 도 설치할까요?\033[0m\n'
  echo "   패널 기록 저장과 프로젝트용 DB 입니다. 메모리를 300MB 쯤 더 씁니다 (MariaDB·Redis 는 항상 설치)."
  local mdef=y; (( MEM_MB < 1900 )) && mdef=n
  if yesno "   MongoDB 설치" "$mdef"; then W_MONGO=1; else W_MONGO=0; fi

  # 3. 고급
  W_MODE=standalone; W_PROXY=""; W_ALLOW=""; W_FW=1
  echo
  printf '\033[1m3. 고급 설정\033[0m (앞단 프록시 · 접속 IP 제한 · 방화벽) — 처음이면 n\n'
  if yesno "   고급 설정을 바꿀까요?" n; then
    echo
    echo "   이 서버 앞에 다른 리버스 프록시(Nginx Proxy Manager, 공유기 포트포워딩 장비 등)가 있고"
    echo "   https 인증서를 거기서 처리하나요? (대부분은 n)"
    if yesno "   앞단 프록시 사용" n; then
      W_MODE=proxy
      while true; do
        ask ip "   앞단 프록시의 IP: " ""
        valid_ips "$ip" && [[ "$ip" != *,* ]] && break
        echo "   IP 형식이 아닙니다 (예: 192.168.0.10)"
      done
      W_PROXY="$ip"
    fi
    echo
    echo "   패널(8443)과 SSH(22)에 접속할 수 있는 주소를 제한할 수 있습니다."
    echo "   예: 1.2.3.4/32 (내 집 IP 만), 10.0.0.0/24  — 여러 개는 쉼표로. 비우면 어디서나."
    echo "   (지금 SSH 로 접속한 주소는 자동으로 허용되어 접속이 끊기지 않습니다)"
    while true; do
      ask cidr "   허용할 주소: " ""
      [[ -z "$cidr" ]] || valid_ips "$cidr" && break
      echo "   주소 형식이 아닙니다"
    done
    W_ALLOW="$cidr"
    echo
    if yesno "   방화벽(ufw)을 자동으로 설정할까요? (22·80·443·8443 만 열기)" y; then W_FW=1; else W_FW=0; fi
  fi

  # 요약
  local url
  if [[ -n "$W_DOMAIN" ]]; then url="https://$W_DOMAIN:8443"; else url="https://$PUB_IP:8443"; fi
  echo
  printf '\033[1m설치 내용\033[0m\n'
  echo "  접속 주소   $url"
  [[ -n "$W_EMAIL" ]] && echo "  인증서 메일 $W_EMAIL"
  echo "  설치 항목   Nginx · MariaDB · Redis$([[ $W_MONGO -eq 1 ]] && echo ' · MongoDB') · Rust · Python · Node.js · Let's Encrypt"
  if [[ $W_MODE == proxy ]]; then echo "  모드       앞단 프록시($W_PROXY) 뒤에서 동작"; else echo "  모드       이 서버가 직접 80·443 을 받음"; fi
  echo "  접속 제한   ${W_ALLOW:-없음 (어디서나)}"
  echo "  방화벽      $([[ $W_FW -eq 1 ]] && echo '자동 설정' || echo '건드리지 않음')"
  echo "  받는 방식   $([[ $FROM == source ]] && echo '소스를 받아 서버에서 빌드 (15~25분)' || echo '미리 빌드한 묶음 (5~10분)')"
  echo
  yesno "이대로 설치할까요?" y || { echo "설치를 취소했습니다. 다시 하려면: $ONE_LINE"; exit 0; }

  PASS=("--mode=$W_MODE")
  [[ -n "$W_PROXY" ]] && PASS+=("--trusted-proxy=$W_PROXY")
  [[ -n "$W_DOMAIN" ]] && PASS+=("--panel-domain=$W_DOMAIN" "--email=$W_EMAIL")
  [[ -n "$W_ALLOW" ]] && PASS+=("--allow-from=$W_ALLOW")
  [[ $W_MONGO -eq 0 ]] && PASS+=("--no-mongodb")
  [[ $W_FW -eq 0 ]] && PASS+=("--no-firewall")
  return 0
}

# ── 미리 빌드한 묶음 받기 → $PKG ──
fetch_release() { # fetch_release [태그]
  local tag="${1:-}" base tmp name
  [[ -n "$ARCH" ]] || return 1
  name="rpanel-linux-$ARCH.tar.gz"
  if [[ -n "$tag" ]]; then base="$RELEASE_BASE/download/$tag"
  else base="$RELEASE_BASE/latest/download"; fi
  command -v curl >/dev/null || { apt-get update -q >/dev/null; apt-get install -y -q curl ca-certificates >/dev/null; }
  tmp="$(mktemp -d /tmp/rpanel-pkg.XXXXXX)"
  # 먼저 조용히 있는지만 확인 (없으면 오류 화면 없이 넘어감)
  curl -fsIL --retry 2 --connect-timeout 15 -o /dev/null "$base/$name" 2>/dev/null || { rm -rf "$tmp"; return 1; }
  info "받는 중: $base/$name"
  if ! curl -fSL --retry 3 --connect-timeout 15 --progress-bar -o "$tmp/$name" "$base/$name" \
     || ! curl -fsSL --retry 3 --connect-timeout 15 -o "$tmp/SHA256SUMS" "$base/SHA256SUMS"; then
    rm -rf "$tmp"; return 1
  fi
  if ! (cd "$tmp" && grep -E " \*?$name\$" SHA256SUMS | sha256sum -c --status); then
    rm -rf "$tmp"; warn "받은 파일이 손상되었습니다 (체크섬 불일치)"; return 1
  fi
  mkdir "$tmp/x"
  tar -xzf "$tmp/$name" -C "$tmp/x" || { rm -rf "$tmp"; return 1; }
  if [[ ! -f "$tmp/x/rpanel/deploy/install.sh" || ! -x "$tmp/x/rpanel/backend/target/release/rpanel" ]]; then
    rm -rf "$tmp"; warn "묶음 내용이 올바르지 않습니다"; return 1
  fi
  rm -rf "$PKG.old"
  [[ -d "$PKG" ]] && mv "$PKG" "$PKG.old"
  mv "$tmp/x/rpanel" "$PKG"
  rm -rf "$PKG.old" "$tmp"
  info "버전 $(cut -d' ' -f1 "$PKG/VERSION")"
}

fetch_source() {
  if ! command -v git >/dev/null; then
    info "git 설치"
    apt-get update -q >/dev/null
    apt-get install -y -q git ca-certificates >/dev/null
  fi
  if [[ -d "$SRC/.git" ]]; then
    info "소스 업데이트: $SRC ($BRANCH)"
    git -C "$SRC" remote set-url origin "$REPO_URL"
    git -C "$SRC" fetch -q --depth 1 origin "$BRANCH"
    git -C "$SRC" reset -q --hard FETCH_HEAD
  else
    info "소스 받기: $REPO_URL → $SRC"
    rm -rf "$SRC"
    git clone -q --depth 1 --branch "$BRANCH" "$REPO_URL" "$SRC" \
      || die "소스를 받지 못했습니다. 인터넷 연결과 저장소 주소($REPO_URL)를 확인하세요."
  fi
  info "버전 $(git -C "$SRC" log -1 --format='%h (%cd)' --date=format:'%Y-%m-%d')"
}

# ─────────────────────────── 시작 ───────────────────────────
printf '\033[1;32mRpanel 설치\033[0m — 리눅스 명령어 없이 브라우저로 쓰는 서버 관리 패널\n'
preflight

WIZ_RAN=0
if [[ $FRESH -eq 0 ]]; then
  info "이미 설치된 서버입니다 — 지난번 설정 그대로 다시 설치합니다 (사이트·DB·비밀번호 유지)."
  info "새 버전으로 올리려면 보통 sudo rpanel-update 를 씁니다."
elif [[ $TTYIN -eq 1 && $YES -eq 0 && ( ${#PASS[@]} -eq 0 || $WIZARD -eq 1 ) ]]; then
  wizard
  WIZ_RAN=1
elif [[ $TTYIN -eq 0 && $YES -eq 0 && ${#PASS[@]} -eq 0 ]]; then
  info "키보드 입력을 받을 수 없어 기본값으로 설치합니다 (도메인 없이 https://서버IP:8443)."
fi
# 마법사 없이 새로 설치하는데 메모리가 적으면 MongoDB 는 뺌 (직접 --mongodb 를 붙이면 설치)
if [[ $FRESH -eq 1 && $WIZ_RAN -eq 0 && " ${PASS[*]:-} " != *mongodb* ]] && (( MEM_MB < 1900 )); then
  info "메모리가 적어 MongoDB 는 설치하지 않습니다 (나중에: sudo rpanel-update --mongodb)"
  PASS+=("--no-mongodb")
fi

# 이 폴더(받아서 압축 푼 묶음, 또는 git 으로 받은 소스)에서 실행한 경우 그대로 씁니다
HERE=""
if [[ -n "${BASH_SOURCE[0]:-}" && -f "${BASH_SOURCE[0]}" ]]; then
  HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
fi
if [[ -n "$HERE" && -f "$HERE/deploy/install.sh" && "$HERE" != /usr/local/* ]]; then
  if [[ -x "$HERE/backend/target/release/rpanel" && -f "$HERE/frontend/dist/index.html" && -f "$HERE/VERSION" ]]; then
    say "이 폴더의 미리 빌드한 묶음으로 설치합니다: $HERE"
    if [[ "$HERE" != "$PKG" ]]; then
      rm -rf "$PKG.new"; cp -a "$HERE" "$PKG.new"; rm -rf "$PKG"; mv "$PKG.new" "$PKG"
    fi
    RPANEL_INSTALL_FROM="$FROM" exec bash "$PKG/deploy/install.sh" --no-build "${PASS[@]}"
  fi
  say "이 폴더의 소스로 설치합니다: $HERE"
  RPANEL_INSTALL_FROM="$FROM" exec bash "$HERE/deploy/install.sh" "${PASS[@]}"
fi

if [[ $FROM != source ]]; then
  say "미리 빌드한 설치 묶음 받기"
  if fetch_release "$WANT_VER"; then
    RPANEL_INSTALL_FROM="$FROM" exec bash "$PKG/deploy/install.sh" --no-build "${PASS[@]}"
  fi
  [[ $FROM == release ]] && die "설치 묶음을 받지 못했습니다${WANT_VER:+ ($WANT_VER)}. 인터넷 연결을 확인하거나 --from-source 로 설치하세요."
  warn "미리 빌드한 묶음을 받지 못해 소스를 받아 서버에서 빌드합니다 (15~25분, 메모리 4GB 권장)."
fi

say "소스 받기"
fetch_source
RPANEL_INSTALL_FROM="$FROM" exec bash "$SRC/deploy/install.sh" "${PASS[@]}"
