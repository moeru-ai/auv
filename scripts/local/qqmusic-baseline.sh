#!/usr/bin/env bash
set -euo pipefail

APP_ID="${1:-com.tencent.QQMusicMac}"
REVEAL_SHORTCUT="${REVEAL_SHORTCUT:-cmd+f}"
REVEAL_SETTLE_MS="${REVEAL_SETTLE_MS:-300}"
MAX_DEPTH="${MAX_DEPTH:-5}"
MAX_CHILDREN="${MAX_CHILDREN:-20}"

echo "[1/4] Capture screenshot contract"
cargo run --quiet -- invoke debug.captureScreen --label qqmusic-baseline

echo "[2/4] Observe QQ音乐 AX tree with reveal step"
cargo run --quiet -- invoke debug.observeWindowTree \
  --target "${APP_ID}" \
  --max_depth "${MAX_DEPTH}" \
  --max_children "${MAX_CHILDREN}" \
  --reveal_shortcut "${REVEAL_SHORTCUT}" \
  --reveal_settle_ms "${REVEAL_SETTLE_MS}"

echo "[3/4] Focus the search input"
cargo run --quiet -- invoke debug.focusTextInput \
  --target "${APP_ID}" \
  --query 搜索 \
  --max_depth "${MAX_DEPTH}" \
  --max_children "${MAX_CHILDREN}" \
  --reveal_shortcut "${REVEAL_SHORTCUT}" \
  --reveal_settle_ms "${REVEAL_SETTLE_MS}"

echo "[4/4] Press a known control"
cargo run --quiet -- invoke debug.pressButton \
  --target "${APP_ID}" \
  --query 刷新 \
  --max_depth "${MAX_DEPTH}" \
  --max_children "${MAX_CHILDREN}" \
  --reveal_shortcut "${REVEAL_SHORTCUT}" \
  --reveal_settle_ms "${REVEAL_SETTLE_MS}"
