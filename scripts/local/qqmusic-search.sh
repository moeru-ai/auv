#!/usr/bin/env bash
set -euo pipefail

QUERY="${1:-aa}"
APP_ID="${APP_ID:-com.tencent.QQMusicMac}"
REVEAL_SHORTCUT="${REVEAL_SHORTCUT:-cmd+f}"
REVEAL_SETTLE_MS="${REVEAL_SETTLE_MS:-300}"
SUBMIT_SETTLE_MS="${SUBMIT_SETTLE_MS:-900}"
MAX_DEPTH="${MAX_DEPTH:-5}"
MAX_CHILDREN="${MAX_CHILDREN:-20}"

echo "[1/3] Focus QQ音乐 search input"
cargo run --quiet -- invoke debug.focusTextInput \
  --target "${APP_ID}" \
  --query 搜索 \
  --max_depth "${MAX_DEPTH}" \
  --max_children "${MAX_CHILDREN}" \
  --reveal_shortcut "${REVEAL_SHORTCUT}" \
  --reveal_settle_ms "${REVEAL_SETTLE_MS}"

echo "[2/3] Type query and submit"
cargo run --quiet -- invoke debug.typeText \
  --target "${APP_ID}" \
  --text "${QUERY}" \
  --replace_existing true \
  --submit_key return \
  --submit_settle_ms "${SUBMIT_SETTLE_MS}"

echo "[3/4] Observe the updated QQ音乐 tree"
cargo run --quiet -- invoke debug.observeWindowTree \
  --target "${APP_ID}" \
  --max_depth "${MAX_DEPTH}" \
  --max_children "${MAX_CHILDREN}" \
  --reveal_shortcut "${REVEAL_SHORTCUT}" \
  --reveal_settle_ms "${REVEAL_SETTLE_MS}"

echo "[4/4] Capture a screenshot artifact for visual confirmation"
cargo run --quiet -- invoke debug.captureScreen --label "qqmusic-search-${QUERY}"
