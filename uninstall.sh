#!/bin/bash
# ──────────────────────────────────────────────────────────────
#  Rpanel 삭제  —  sudo rpanel-uninstall
#
#  두 가지 방법 중 하나를 고릅니다 (실행하면 물어봅니다):
#
#   1) 패널만 삭제   (--keep-data)
#      패널 프로그램과 사이트 연결 설정을 지웁니다 → 사이트 접속이 멈춥니다.
#      사이트 파일(/home/…), 데이터베이스, 백업, 설정은 남겨 둡니다.
#      나중에 다시 설치하면 사이트 목록이 그대로 돌아오고, 각 사이트에서 '적용'을 누르면 다시 서비스됩니다.
#
#   2) 전부 삭제     (--purge)
#      위에 더해 사이트 파일과 사이트 계정, 패널이 만든 데이터베이스, 백업, 인증서, 설정까지 모두 지웁니다.
#      되돌릴 수 없습니다.
#
#  어느 쪽이든 Nginx · MariaDB · Redis 같은 기본 프로그램은 지우지 않습니다 (다른 곳에서 쓰고 있을 수 있음).
#
#    sudo rpanel-uninstall                       물어보면서 진행
#    sudo rpanel-uninstall --keep-data --yes     묻지 않고 패널만 삭제
#    sudo rpanel-uninstall --purge --yes         묻지 않고 전부 삭제
# ──────────────────────────────────────────────────────────────
set -euo pipefail

say()  { printf '\n\033[1;32m▶ %s\033[0m\n' "$*"; }
info() { printf '  %s\n' "$*"; }
warn() { printf '\033[1;33m! %s\033[0m\n' "$*"; }
die()  { printf '\033[1;31m✘ %s\033[0m\n' "$*" >&2; exit 1; }

MODE=""
YES=0
for a in "$@"; do
  case "$a" in
    --keep-data) MODE=keep ;;
    --purge) MODE=purge ;;
    --yes|-y) YES=1 ;;
    -h|--help) sed -n '2,22p' "$0"; exit 0 ;;
    *) die "알 수 없는 옵션: $a  (--help)" ;;
  esac
done

[[ $EUID -eq 0 ]] || die "관리자 권한이 필요합니다: sudo rpanel-uninstall"

ENVF=/etc/rpanel/rpanel.env
envget() { [[ -f $ENVF ]] && grep -E "^$1=" "$ENVF" | tail -n1 | cut -d= -f2- || true; }
if [[ ! -f $ENVF && ! -x /usr/local/bin/rpanel && ! -f /etc/systemd/system/rpanel.service ]]; then
  die "Rpanel 이 설치되어 있지 않습니다."
fi

# 질문은 터미널에서 직접 받음 (curl … | sudo bash 로 실행해도 동작)
ask() {
  local prompt="$1" ans=""
  if ! { read -r -p "$prompt" ans < /dev/tty; } 2>/dev/null; then
    printf '\n질문에 답할 수 없는 환경입니다. 이렇게 실행하세요: sudo rpanel-uninstall --keep-data --yes  (또는 --purge --yes)\n' >&2
  fi
  printf '%s' "$ans"
}

if [[ -z "$MODE" ]]; then
  cat <<'EOF'

Rpanel 을 삭제합니다. 방법을 고르세요.

  1) 패널만 삭제 — 사이트 파일·데이터베이스·백업은 남겨 둠 (다시 설치하면 이어서 사용)
  2) 전부 삭제   — 사이트 파일·데이터베이스·백업·인증서까지 모두 삭제 (되돌릴 수 없음)
  q) 취소

EOF
  case "$(ask '선택 [1/2/q]: ')" in
    1) MODE=keep ;;
    2) MODE=purge ;;
    *) echo "취소했습니다."; exit 0 ;;
  esac
fi

SITE_USERS=()
if [[ -f /etc/rpanel/site-users ]]; then
  while read -r _slug user _root; do
    [[ -n "${user:-}" ]] && SITE_USERS+=("$user")
  done < /etc/rpanel/site-users
fi

if [[ $YES -eq 0 ]]; then
  echo
  if [[ $MODE == purge ]]; then
    warn "전부 삭제: 사이트 ${#SITE_USERS[@]}개의 파일과 계정, 패널이 만든 데이터베이스, 백업이 모두 사라집니다."
    [[ ${#SITE_USERS[@]} -gt 0 ]] && info "삭제될 사이트 계정: ${SITE_USERS[*]}"
    [[ "$(ask "정말 지우려면 '전부삭제' 라고 입력하세요: ")" == "전부삭제" ]] || { echo "취소했습니다."; exit 0; }
  else
    info "패널만 삭제: 사이트 접속은 멈추고, 파일·데이터베이스·백업은 남겨 둡니다."
    case "$(ask '계속할까요? [y/N]: ')" in y|Y|yes|YES|예|ㅇ) ;; *) echo "취소했습니다."; exit 0 ;; esac
  fi
fi

# ───────────── 1. 패널과 사이트 서비스 중지 ─────────────
say "패널과 사이트 서비스 중지"
systemctl disable --now rpanel >/dev/null 2>&1 || true
units=()
for f in /etc/systemd/system/rpanel-app-*.service /etc/systemd/system/rpanel-cron-*.timer /etc/systemd/system/rpanel-cron-*.service; do
  [[ -e "$f" ]] && units+=("$(basename "$f")")
done
if [[ ${#units[@]} -gt 0 ]]; then
  systemctl disable --now "${units[@]}" >/dev/null 2>&1 || true
  info "서비스·예약 작업 ${#units[@]}개 중지"
fi
rm -f /etc/systemd/system/rpanel.service /etc/systemd/system/rpanel-app-*.service \
      /etc/systemd/system/rpanel-cron-*.timer /etc/systemd/system/rpanel-cron-*.service
systemctl daemon-reload

# ───────────── 2. Nginx 설정 ─────────────
say "Nginx 에서 패널·사이트 설정 제거"
n=0
for f in /etc/nginx/sites-enabled/rpanel-* /etc/nginx/sites-available/rpanel-*; do
  [[ -e "$f" || -L "$f" ]] && { rm -f "$f"; n=$((n + 1)); }
done
rm -f /etc/nginx/conf.d/rpanel.conf /etc/letsencrypt/renewal-hooks/deploy/rpanel-nginx-reload
# 설치할 때 꺼 둔 Nginx 기본 페이지를 되살림
if [[ -f /etc/nginx/sites-available/default && ! -e /etc/nginx/sites-enabled/default ]] \
   && ! ls /etc/nginx/sites-enabled/ 2>/dev/null | grep -q .; then
  ln -s /etc/nginx/sites-available/default /etc/nginx/sites-enabled/default
  # IPv6 가 없는 서버 등에서 기본 페이지가 오류를 내면 다시 끔
  command -v nginx >/dev/null && ! nginx -t >/dev/null 2>&1 && rm -f /etc/nginx/sites-enabled/default
fi
if command -v nginx >/dev/null; then
  if nginx -t >/dev/null 2>&1; then
    systemctl reload nginx 2>/dev/null || true
    info "설정 파일 ${n}개 제거, Nginx 다시 읽음"
  else
    warn "Nginx 설정 검사 실패 — 다른 설정을 확인하세요: sudo nginx -t"
  fi
fi

# ───────────── 3. 프로그램·권한·SSH·방화벽 ─────────────
say "패널 프로그램 제거"
rm -f /usr/local/bin/rpanel /usr/local/sbin/rpanel-update /usr/local/sbin/rpanel-uninstall /etc/sudoers.d/rpanel
rm -rf /usr/local/lib/rpanel /usr/share/rpanel /var/lib/rpanel/.self-build /var/lib/rpanel/staging
info "프로그램·sudo 권한 제거"
if [[ -f /etc/ssh/sshd_config.d/50-rpanel-sites.conf ]]; then
  rm -f /etc/ssh/sshd_config.d/50-rpanel-sites.conf
  systemctl reload ssh 2>/dev/null || systemctl reload sshd 2>/dev/null || true
  info "사이트 계정 SSH 비밀번호 로그인 설정 제거"
fi
if command -v ufw >/dev/null && ufw status 2>/dev/null | grep -q "Status: active"; then
  ufw --force delete allow 8443/tcp >/dev/null 2>&1 || true
  allow="$(envget RPANEL_ALLOW_FROM)"
  for net in ${allow//,/ }; do
    ufw --force delete allow from "$net" to any port 8443 proto tcp >/dev/null 2>&1 || true
  done
  info "방화벽에서 패널 포트(8443) 규칙 제거 (22·80·443 은 그대로)"
fi

# Redis: 패널 로그인 세션 지우기
REDIS_URL_V="$(envget REDIS_URL)"
REDIS_PW="$(sed -nE 's#^redis://:([^@]*)@.*#\1#p' <<<"$REDIS_URL_V")"
redis_flush() { REDISCLI_AUTH="$REDIS_PW" redis-cli -n "$1" FLUSHDB >/dev/null 2>&1 || true; }
PANEL_REDIS_DB="$(sed -nE 's#.*/([0-9]+)$#\1#p' <<<"$REDIS_URL_V")"

if [[ $MODE == keep ]]; then
  [[ -n "$PANEL_REDIS_DB" ]] && command -v redis-cli >/dev/null && redis_flush "$PANEL_REDIS_DB"
  cat <<EOF

$(printf '\033[1;32m')✔ 패널을 삭제했습니다$(printf '\033[0m')  (사이트 접속은 멈춘 상태)

  남겨 둔 것
    사이트 파일      /home/<사이트 계정>/   ${SITE_USERS[*]:-(없음)}
    데이터베이스     MariaDB 안의 패널 DB와 사이트 DB 그대로
    백업            /var/lib/rpanel/backups/
    설정·인증서      /etc/rpanel/  /etc/letsencrypt/

  다시 설치하면 이어서 쓸 수 있습니다:
    curl -fsSL https://raw.githubusercontent.com/sinji0117/Rpanel/main/install.sh | sudo bash

  모두 지우려면:  curl -fsSL https://raw.githubusercontent.com/sinji0117/Rpanel/main/uninstall.sh | sudo bash -s -- --purge
EOF
  exit 0
fi

# ───────────── 4. (전부 삭제) 데이터 ─────────────
say "사이트 데이터베이스·Redis 삭제"
systemctl start mariadb >/dev/null 2>&1 || true
DB_NAME="$(sed -nE 's#^DATABASE_URL=.*/([^/?]+)(\?.*)?$#\1#p' "$ENVF" 2>/dev/null || true)"
DB_NAME="${DB_NAME:-rpanel}"
if command -v redis-cli >/dev/null; then
  systemctl start redis-server >/dev/null 2>&1 || true
  for db in $(mariadb -N -e "SELECT redis_db FROM \`$DB_NAME\`.apps WHERE redis_db IS NOT NULL" 2>/dev/null || true); do
    redis_flush "$db"
  done
  [[ -n "$PANEL_REDIS_DB" ]] && redis_flush "$PANEL_REDIS_DB"
fi
if [[ -f /etc/rpanel/managed-dbs ]]; then
  while read -r db user; do
    [[ "$db" =~ ^[a-z][a-z0-9_]+$ && "$user" =~ ^[a-z][a-z0-9_]+$ ]] || continue
    mariadb -e "DROP DATABASE IF EXISTS \`$db\`; DROP USER IF EXISTS '$user'@'localhost';" 2>/dev/null \
      && info "데이터베이스 $db 삭제"
  done < /etc/rpanel/managed-dbs
fi
if [[ "$DB_NAME" =~ ^[a-zA-Z0-9_]+$ ]]; then
  mariadb -e "DROP DATABASE IF EXISTS \`$DB_NAME\`; DROP USER IF EXISTS 'rpanel'@'localhost'; FLUSH PRIVILEGES;" 2>/dev/null \
    && info "패널 데이터베이스 $DB_NAME 삭제"
fi

say "사이트 계정·파일 삭제"
for user in "${SITE_USERS[@]}"; do
  [[ "$user" =~ ^[a-z][a-z0-9-]{2,31}$ ]] || continue
  # rpanel 이 만든 계정(rpanel-sites 그룹)만 지움
  id -nG "$user" 2>/dev/null | tr ' ' '\n' | grep -qx rpanel-sites || continue
  pkill -u "$user" 2>/dev/null || true
  sleep 0.3
  gpasswd -d www-data "$user" >/dev/null 2>&1 || true
  userdel --remove "$user" >/dev/null 2>&1 || userdel "$user" >/dev/null 2>&1 || true
  rm -rf --one-file-system "/home/$user"
  info "$user (/home/$user) 삭제"
done

say "인증서·설정·백업 삭제"
if command -v certbot >/dev/null; then
  for d in /etc/letsencrypt/live/rpanel-*; do
    [[ -d "$d" ]] || continue
    certbot delete --non-interactive --cert-name "$(basename "$d")" >/dev/null 2>&1 && info "인증서 $(basename "$d") 삭제"
  done
fi
rm -f /var/log/nginx/rpanel-*.log*
rm -rf --one-file-system /var/lib/rpanel /etc/rpanel /opt/rpanel-src
userdel rpanel >/dev/null 2>&1 || true
userdel rpanel-app >/dev/null 2>&1 || true
groupdel rpanel-sites >/dev/null 2>&1 || true
info "/var/lib/rpanel · /etc/rpanel · 패널 계정 삭제"

cat <<EOF

$(printf '\033[1;32m')✔ Rpanel 을 모두 삭제했습니다$(printf '\033[0m')

  Nginx · MariaDB · Redis · Node.js · certbot 같은 기본 프로그램은 남아 있습니다.
  이것들도 필요 없다면 직접 지우세요 (MariaDB 안의 다른 데이터도 함께 사라집니다):
    sudo apt purge nginx mariadb-server redis-server certbot nodejs && sudo apt autoremove
EOF
