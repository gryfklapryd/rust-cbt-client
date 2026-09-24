#!/usr/bin/env bash
# Salin ulang paket bersama dari repo server (rust-cbt-master) ke repo ini.
#   scripts/sync-shared.sh ../rust-cbt-master
# Sumber kebenaran skema soal, penilaian, dan komponen soal ada di repo server.
set -euo pipefail
MASTER="${1:-../rust-cbt-master}"
HERE="$(cd "$(dirname "$0")/.." && pwd)"
for pkg in shared question-ui; do
  src="$MASTER/packages/$pkg/src"
  [ -d "$src" ] || { echo "Tidak ditemukan: $src" >&2; exit 1; }
  rm -rf "$HERE/packages/$pkg/src"
  cp -r "$src" "$HERE/packages/$pkg/src"
done
echo "Disalin dari $(git -C "$MASTER" rev-parse --short HEAD 2>/dev/null || echo "$MASTER")"
