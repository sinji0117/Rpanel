#!/bin/bash
# 미리 빌드한 설치 묶음(올인원 패키지) 만들기 — 릴리스 워크플로(.github/workflows/release.yml)가 씁니다.
#
#   먼저 빌드:  (cd backend && cargo build --release --locked --bins)
#               (cd frontend && npm ci && npm run build)
#   묶기:      deploy/make-package.sh [출력 폴더]      (기본: dist-pkg)
#
# 결과: rpanel-linux-<arch>.tar.gz  (압축을 풀면 rpanel/ 폴더 — 그 안에서 sudo ./install.sh)
#   rpanel/
#     install.sh update.sh uninstall.sh   설치 · 업데이트 · 삭제
#     deploy/                             설치 엔진 · nginx · systemd · helper
#     backend/target/release/rpanel       패널 실행 파일
#     backend/target/release/rpanel-files 파일 관리 도구
#     frontend/dist/                      화면
#     VERSION                             예: v0.16.0 2026-11-05 ab12cd3
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${1:-$ROOT/dist-pkg}"
case "$(uname -m)" in
  x86_64|amd64) ARCH=x86_64 ;;
  aarch64|arm64) ARCH=aarch64 ;;
  *) echo "지원하지 않는 CPU: $(uname -m)" >&2; exit 1 ;;
esac
ARCH="${RPANEL_PKG_ARCH:-$ARCH}"

BIN="$ROOT/backend/target/release"
for f in "$BIN/rpanel" "$BIN/rpanel-files" "$ROOT/frontend/dist/index.html"; do
  [[ -e "$f" ]] || { echo "빌드 결과가 없습니다: $f (먼저 빌드하세요)" >&2; exit 1; }
done

VER="v$(sed -nE 's/^version *= *"([^"]+)".*/\1/p' "$ROOT/backend/Cargo.toml" | head -n1)"
SHA="$(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null || echo local)"

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
P="$STAGE/rpanel"
install -d "$P/backend/target/release" "$P/frontend"
cp -a "$ROOT/install.sh" "$ROOT/update.sh" "$ROOT/uninstall.sh" "$ROOT/LICENSE" "$ROOT/README.md" "$ROOT/CHANGELOG.md" "$P/"
cp -a "$ROOT/deploy" "$P/deploy"
rm -f "$P/deploy/make-package.sh"
install -m 0755 "$BIN/rpanel" "$BIN/rpanel-files" "$P/backend/target/release/"
cp -a "$ROOT/frontend/dist" "$P/frontend/dist"
echo "$VER $(date -u +%Y-%m-%d) $SHA" > "$P/VERSION"
chmod 0755 "$P/install.sh" "$P/update.sh" "$P/uninstall.sh" "$P/deploy/install.sh" "$P/deploy/rpanel-helper"

install -d "$OUT"
NAME="rpanel-linux-$ARCH.tar.gz"
tar -C "$STAGE" --owner=0 --group=0 -czf "$OUT/$NAME" rpanel
echo "$VER" > "$OUT/VERSION"
echo "$OUT/$NAME  ($VER, $(du -h "$OUT/$NAME" | cut -f1))"
