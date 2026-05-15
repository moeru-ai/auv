#!/usr/bin/env bash
set -euo pipefail

QUERY="${1:-aa}"
ANCHOR="${2:-Cure For Me}"
PLAYBACK_TITLE="${3:-Cure For Me - AURORA}"
CLICK_COUNT="${CLICK_COUNT:-2}"
MAX_DISTURBANCE="${MAX_DISTURBANCE:-pointer}"
DRY_RUN="${DRY_RUN:-0}"
VERIFY_MIN_CONFIDENCE="${VERIFY_MIN_CONFIDENCE:-0.90}"
VERIFY_REGION_LEFT_RATIO="${VERIFY_REGION_LEFT_RATIO:-0.22}"
VERIFY_REGION_TOP_RATIO="${VERIFY_REGION_TOP_RATIO:-0.80}"
VERIFY_REGION_RIGHT_RATIO="${VERIFY_REGION_RIGHT_RATIO:-0.45}"
VERIFY_REGION_BOTTOM_RATIO="${VERIFY_REGION_BOTTOM_RATIO:-0.90}"
APP_ID="${APP_ID:-com.tencent.QQMusicMac}"
REVEAL_SHORTCUT="${REVEAL_SHORTCUT:-cmd+f}"
REVEAL_SETTLE_MS="${REVEAL_SETTLE_MS:-300}"
SUBMIT_SETTLE_MS="${SUBMIT_SETTLE_MS:-900}"
DISMISS_OVERLAY_KEY="${DISMISS_OVERLAY_KEY:-escape}"
DISMISS_OVERLAY_SETTLE_MS="${DISMISS_OVERLAY_SETTLE_MS:-300}"
SELECTION_REGION_LEFT_RATIO="${SELECTION_REGION_LEFT_RATIO:-0.14}"
SELECTION_REGION_TOP_RATIO="${SELECTION_REGION_TOP_RATIO:-0.34}"
SELECTION_REGION_RIGHT_RATIO="${SELECTION_REGION_RIGHT_RATIO:-0.90}"
SELECTION_REGION_BOTTOM_RATIO="${SELECTION_REGION_BOTTOM_RATIO:-0.95}"
SELECTION_MIN_CONFIDENCE="${SELECTION_MIN_CONFIDENCE:-0.90}"

if [[ "${DRY_RUN}" == "1" ]]; then
  RUN_ARGS=(--dry-run)
else
  RUN_ARGS=()
fi

if [[ -n "${MAX_DISTURBANCE}" ]]; then
  RUN_ARGS+=(--max-disturbance "${MAX_DISTURBANCE}")
fi

python3 scripts/recipes/run_recipe.py \
  recipes/macos/qqmusic/play-visible-anchor.v0.json \
  "${RUN_ARGS[@]}" \
  --set "app_id=${APP_ID}" \
  --set "query=${QUERY}" \
  --set "anchor_text=${ANCHOR}" \
  --set "playback_title=${PLAYBACK_TITLE}" \
  --set "click_count=${CLICK_COUNT}" \
  --set "reveal_shortcut=${REVEAL_SHORTCUT}" \
  --set "reveal_settle_ms=${REVEAL_SETTLE_MS}" \
  --set "submit_settle_ms=${SUBMIT_SETTLE_MS}" \
  --set "dismiss_overlay_key=${DISMISS_OVERLAY_KEY}" \
  --set "dismiss_overlay_settle_ms=${DISMISS_OVERLAY_SETTLE_MS}" \
  --set "selection_region_left_ratio=${SELECTION_REGION_LEFT_RATIO}" \
  --set "selection_region_top_ratio=${SELECTION_REGION_TOP_RATIO}" \
  --set "selection_region_right_ratio=${SELECTION_REGION_RIGHT_RATIO}" \
  --set "selection_region_bottom_ratio=${SELECTION_REGION_BOTTOM_RATIO}" \
  --set "selection_min_confidence=${SELECTION_MIN_CONFIDENCE}" \
  --set "verification_region_left_ratio=${VERIFY_REGION_LEFT_RATIO}" \
  --set "verification_region_top_ratio=${VERIFY_REGION_TOP_RATIO}" \
  --set "verification_region_right_ratio=${VERIFY_REGION_RIGHT_RATIO}" \
  --set "verification_region_bottom_ratio=${VERIFY_REGION_BOTTOM_RATIO}" \
  --set "verification_min_confidence=${VERIFY_MIN_CONFIDENCE}"
