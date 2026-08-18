#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export UPSTREAM_BASE_URL="${UPSTREAM_BASE_URL:-https://ai.zkmjnic.tech}"
export BIND_HOST="${BIND_HOST:-127.0.0.1}"
export PORT="${PORT:-8787}"

exec python3 "$ROOT/scripts/claude-sse-proxy.py" \
  --bind "$BIND_HOST" \
  --port "$PORT" \
  --upstream "$UPSTREAM_BASE_URL"
