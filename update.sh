#!/bin/bash
# ──────────────────────────────────────────────────────────────
#  Rpanel 업데이트  —  sudo rpanel-update
#
#  최신 버전을 받아 다시 빌드·설치합니다. 사이트, 데이터베이스, 설정, 비밀번호는 그대로 유지됩니다.
#  업데이트 전에 패널 DB와 현재 프로그램을 백업하고, 새 버전이 시작되지 않으면 자동으로 되돌립니다.
#
#    sudo rpanel-update            최신 버전이 있으면 업데이트
#    sudo rpanel-update --check    새 버전이 있는지 확인만
#    sudo rpanel-update --force    최신이어도 다시 빌드·설치
#    (설치 옵션도 붙일 수 있습니다. 예: --allow-from=10.0.0.0/24)
#
#  환경변수: RPANEL_REPO (기본: 설치할 때 받은 저장소), RPANEL_BRANCH, RPANEL_SRC
# ──────────────────────────────────────────────────────────────
set -euo pipefail

SRC="${RPANEL_SRC:-/opt/rpanel-src}"
# 저장소 주소: 지정값 → 설치할 때 받은 곳 → 기본 GitHub
REPO_URL="${RPANEL_REPO:-$(git -C "$SRC" remote get-url origin 2>/dev/null || true)}"
REPO_URL="${REPO_URL:-https://github.com/sinji0117/Rpanel.git}"
BRANCH="${RPANEL_BRANCH:-main}"
ENVF=/etc/rpanel/rpanel.env
KEEP=/var/lib/rpanel/backups/_panel

say()  { printf '\n\033[1;32m▶ %s\033[0m\n' "$*"; }
info() { printf '  %s\n' "$*"; }
warn() { printf '\033[1;33m! %s\033[0m\n' "$*"; }
die()  { printf '\033[1;31m✘ %s\033[0m\n' "$*" >&2; exit 1; }
# 업데이트 단계 표시 (진행 바) — 설치 단계는 install.sh 가 따로 표시
USTEP=0
# 새 소스로 다시 실행된 두 번째 단계는 '새 버전 확인' 다음부터
if [[ "${RPANEL_UPDATE_STAGE:-1}" == 2 ]]; then USTEP=1; fi
ustep() {
  USTEP=$(( USTEP + 1 ))
  local w=30 fill=$(( USTEP * 30 / 3 ))
  printf '\n\033[1;36m업데이트 [%s%s] %d/3  %s\033[0m\n' "$(printf '%*s' "$fill" '' | tr ' ' '#')" "$(printf '%*s' "$(( w - fill ))" '' | tr ' ' '-')" "$USTEP" "$*"
}

CHECK=0
FORCE=0
PASS=()
for a in "$@"; do
  case "$a" in
    --check) CHECK=1 ;;
    --force) FORCE=1 ;;
    -h|--help) sed -n '2,14p' "$0"; exit 0 ;;
    *) PASS+=("$a") ;;
  esac
done

[[ $EUID -eq 0 ]] || die "관리자 권한이 필요합니다: sudo rpanel-update"
[[ -f $ENVF && -x /usr/local/bin/rpanel ]] || die "Rpanel 이 설치되어 있지 않습니다. 먼저 설치하세요:
  curl -fsSL https://raw.githubusercontent.com/sinji0117/Rpanel/main/install.sh | sudo bash"

# ── 1. 최신 소스 받기 (두 번째 실행 단계에서는 생략) ──
if [[ "${RPANEL_UPDATE_STAGE:-1}" != 2 ]]; then
  ustep "새 버전 확인"
  command -v git >/dev/null || { apt-get update -q >/dev/null; apt-get install -y -q git >/dev/null; }
  if [[ -d "$SRC/.git" ]]; then
    git -C "$SRC" remote set-url origin "$REPO_URL"
    git -C "$SRC" fetch -q --depth 1 origin "$BRANCH" || die "새 버전을 받지 못했습니다 (인터넷 연결·저장소 주소 확인: $REPO_URL)"
  else
    rm -rf "$SRC"
    git clone -q --depth 1 --branch "$BRANCH" "$REPO_URL" "$SRC" || die "소스를 받지 못했습니다: $REPO_URL"
    git -C "$SRC" fetch -q --depth 1 origin "$BRANCH"
  fi
  CUR="$(awk '{print $1}' /usr/share/rpanel/VERSION 2>/dev/null || true)"
  NEW="$(git -C "$SRC" rev-parse --short FETCH_HEAD)"
  NEW_DATE="$(git -C "$SRC" log -1 --format=%cd --date=format:'%Y-%m-%d' FETCH_HEAD)"
  info "지금 버전: ${CUR:-알 수 없음}"
  info "최신 버전: $NEW ($NEW_DATE)"
  if [[ -n "$CUR" && "$CUR" == "$NEW" && $FORCE -eq 0 ]]; then
    echo; echo "이미 최신 버전입니다. (다시 설치하려면 --force)"; exit 0
  fi
  if [[ $CHECK -eq 1 ]]; then
    echo; echo "새 버전이 있습니다. 업데이트: sudo rpanel-update"; exit 0
  fi
  git -C "$SRC" reset -q --hard FETCH_HEAD
  # 새 버전의 업데이트 스크립트로 이어서 실행 (스크립트 자체가 바뀌었을 수 있으므로)
  RPANEL_UPDATE_STAGE=2 exec bash "$SRC/update.sh" "$@"
fi

[[ -f "$SRC/deploy/install.sh" ]] || die "소스가 올바르지 않습니다: $SRC"

# ── 2. 백업 ──
ustep "업데이트 전 백업"
STAMP="$(date +%Y%m%d-%H%M%S)"
install -d -o root -g root -m 0700 "$KEEP"
DB_NAME="$(grep -E '^DATABASE_URL=' "$ENVF" | sed -E 's#.*/([^/?]+)(\?.*)?$#\1#')"
DB_NAME="${DB_NAME:-rpanel}"
DUMP="$KEEP/panel-db-$STAMP.sql.gz"
mariadb-dump --single-transaction --routines --triggers "$DB_NAME" | gzip -c > "$DUMP" \
  || die "패널 DB 백업에 실패했습니다 — 업데이트를 중단합니다"
chmod 0600 "$DUMP"
info "패널 DB → $DUMP"
PREV="$KEEP/prev"
rm -rf "$PREV"; install -d -m 0700 "$PREV"
cp -a /usr/local/bin/rpanel "$PREV/rpanel"
cp -a /usr/local/lib/rpanel "$PREV/lib"
cp -a /usr/share/rpanel "$PREV/share"
info "현재 프로그램 → $PREV"
# 오래된 DB 백업은 최근 5개만
ls -1t "$KEEP"/panel-db-*.sql.gz 2>/dev/null | tail -n +6 | xargs -r rm -f
MIG_BEFORE="$(mariadb -N -e "SELECT COUNT(*) FROM \`$DB_NAME\`._sqlx_migrations" 2>/dev/null || echo 0)"

rollback() {
  warn "업데이트에 실패해 이전 버전으로 되돌립니다"
  # 새 프로그램이 한 번이라도 실행됐다면 DB 구조가 바뀌었을 수 있음 → DB 도 복원
  local new_started=0
  cmp -s "$PREV/rpanel" /usr/local/bin/rpanel || new_started=1
  install -m 0755 "$PREV/rpanel" /usr/local/bin/rpanel
  rm -rf /usr/local/lib/rpanel /usr/share/rpanel
  cp -a "$PREV/lib" /usr/local/lib/rpanel
  cp -a "$PREV/share" /usr/share/rpanel
  local after
  after="$(mariadb -N -e "SELECT COUNT(*) FROM \`$DB_NAME\`._sqlx_migrations" 2>/dev/null || echo 0)"
  if [[ $new_started -eq 1 || "$after" != "$MIG_BEFORE" ]]; then
    info "패널 DB 를 업데이트 전으로 복원 ($DUMP)"
    systemctl stop rpanel 2>/dev/null || true
    mariadb -e "DROP DATABASE IF EXISTS \`$DB_NAME\`; CREATE DATABASE \`$DB_NAME\` CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;"
    gunzip -c "$DUMP" | mariadb "$DB_NAME"
  fi
  systemctl restart rpanel
  for _ in $(seq 1 30); do
    curl -fsS http://127.0.0.1:8090/api/health 2>/dev/null | grep -q '"status":"ok"' && {
      warn "이전 버전으로 되돌렸습니다 — 패널은 정상 동작 중입니다. 위의 오류 내용을 확인하세요."
      exit 1
    }
    sleep 1
  done
  die "되돌린 뒤에도 패널이 시작되지 않습니다: journalctl -u rpanel -n 50 --no-pager"
}

# ── 3. 빌드·설치 (설치할 때 고른 옵션은 /etc/rpanel/rpanel.env 에서 그대로 읽음) ──
ustep "새 버전 설치"
if ! RPANEL_DONE_WORD=업데이트 RPANEL_STEP_LABEL="  설치 " bash "$SRC/deploy/install.sh" "${PASS[@]}"; then
  rollback
fi
