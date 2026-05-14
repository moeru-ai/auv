#!/usr/bin/env bash
set -euo pipefail

QUERY="${1:-aa}"
ANCHOR="${2:-I DRINK THE LIGHT}"
CLICK_COUNT="${CLICK_COUNT:-1}"

echo "[1/4] Refresh QQ音乐 search results for query: ${QUERY}"
./scripts/local/qqmusic-search.sh "${QUERY}"

echo "[2/4] Resolve OCR text anchor: ${ANCHOR}"
cargo run --quiet -- invoke debug.findScreenText \
  --query "${ANCHOR}"

echo "[3/4] Click OCR text anchor: ${ANCHOR}"
cargo run --quiet -- invoke debug.clickScreenText \
  --query "${ANCHOR}" \
  --click_count "${CLICK_COUNT}"

echo "[4/4] Capture post-click screenshot evidence"
cargo run --quiet -- invoke debug.captureScreen \
  --label "qqmusic-result-anchor-${QUERY}"
