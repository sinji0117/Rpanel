#!/bin/bash
# Rpanel 부트스트랩 — OS(Ubuntu 24.04) 설치 직후 한 줄로 설치/업데이트
#
#   curl -fsSL https://raw.githubusercontent.com/sinji0117/Rpanel/main/deploy/bootstrap.sh | sudo bash
#   curl -fsSL https://raw.githubusercontent.com/sinji0117/Rpanel/main/deploy/bootstrap.sh | sudo bash -s -- --dry-run-mode
#
# 설치 후에는 같은 동작을 `sudo rpanel-update` 로 실행할 수 있습니다.
# 환경변수: RPANEL_REPO (기본 GitHub), RPANEL_BRANCH (main), RPANEL_SRC (/opt/rpanel-src)
set -euo pipefail

REPO_URL="${RPANEL_REPO:-https://github.com/sinji0117/Rpanel.git}"
BRANCH="${RPANEL_BRANCH:-main}"
SRC="${RPANEL_SRC:-/opt/rpanel-src}"

[[ $EUID -eq 0 ]] || { echo "root 로 실행하세요: curl ... | sudo bash" >&2; exit 1; }
export DEBIAN_FRONTEND=noninteractive

if ! command -v git >/dev/null; then
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
  git clone -q --depth 1 --branch "$BRANCH" "$REPO_URL" "$SRC" \
    || { echo "✘ clone 실패 — 저장소가 private 이면 RPANEL_REPO 에 접근 가능한 주소를 지정하세요" >&2; exit 1; }
fi
echo "  커밋 $(git -C "$SRC" rev-parse --short HEAD)"

exec bash "$SRC/deploy/install.sh" "$@"
