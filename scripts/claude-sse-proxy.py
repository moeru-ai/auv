#!/usr/bin/env python3
"""Local Anthropic-compatible proxy that repairs broken SSE content_block indices.

Some third-party gateways emit malformed streaming events when tools are used,
for example:

  start index=0 thinking
  stop  index=0
  start index=2 tool_use      # skipped index=1
  stop  index=1               # phantom stop (never started)
  stop  index=2

Claude Code expects contiguous indices and fails with "Content block not found".
This proxy rewrites indices to 0..n-1 and drops phantom stop/delta events.

Usage:
  export UPSTREAM_BASE_URL="https://ai.zkmjnic.tech"
  python3 scripts/claude-sse-proxy.py --port 8787

Then point Claude Code at:
  ANTHROPIC_BASE_URL=http://127.0.0.1:8787
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import IO

DEFAULT_UPSTREAM = os.environ.get("UPSTREAM_BASE_URL", "https://ai.zkmjnic.tech")
DEFAULT_BIND = os.environ.get("BIND_HOST", "127.0.0.1")
DEFAULT_PORT = int(os.environ.get("PORT", "8787"))

HOP_BY_HOP_HEADERS = {
  "connection",
  "keep-alive",
  "proxy-authenticate",
  "proxy-authorization",
  "te",
  "trailers",
  "transfer-encoding",
  "upgrade",
  "host",
  "content-length",
}


class StreamIndexFixer:
  """Rewrite SSE content_block indices to a contiguous 0..n-1 sequence."""

  def __init__(self) -> None:
    self._remap: dict[int, int] = {}
    self._next_index = 0

  def fix_event(self, event: dict) -> dict | None:
    event_type = event.get("type")
    if event_type == "content_block_start":
      original = event["index"]
      fixed = self._next_index
      self._next_index += 1
      self._remap[original] = fixed
      return {**event, "index": fixed}

    if event_type == "content_block_delta":
      original = event["index"]
      fixed = self._remap.get(original)
      if fixed is None:
        return None
      return {**event, "index": fixed}

    if event_type == "content_block_stop":
      original = event["index"]
      fixed = self._remap.get(original)
      if fixed is None:
        return None
      return {**event, "index": fixed}

    return event

  def fix_sse_line(self, line: bytes) -> bytes | None:
    text = line.decode("utf-8", errors="replace").rstrip("\r\n")
    if not text.startswith("data: "):
      return line

    payload = text[6:]
    if payload == "[DONE]":
      return line

    try:
      event = json.loads(payload)
    except json.JSONDecodeError:
      return line

    fixed = self.fix_event(event)
    if fixed is None:
      return None

    return f"data: {json.dumps(fixed, ensure_ascii=False)}\n".encode("utf-8")


def request_wants_stream(body: bytes) -> bool:
  if not body:
    return False
  try:
    payload = json.loads(body)
  except json.JSONDecodeError:
    return False
  return bool(payload.get("stream"))


def build_upstream_url(upstream_base: str, path: str, query: str) -> str:
  base = upstream_base.rstrip("/")
  target = path or "/"
  if not target.startswith("/"):
    target = f"/{target}"
  if query:
    return f"{base}{target}?{query}"
  return f"{base}{target}"


def forward_headers(handler: BaseHTTPRequestHandler) -> dict[str, str]:
  headers: dict[str, str] = {}
  for key, value in handler.headers.items():
    lower = key.lower()
    if lower in HOP_BY_HOP_HEADERS:
      continue
    headers[key] = value
  return headers


def iter_fixed_sse_frames(upstream: IO[bytes]) -> bytes:
  """Rewrite SSE frames while preserving event/data pairing.

  Upstream gateways may emit phantom ``content_block_stop`` events. Dropping only
  the ``data:`` line leaves orphaned ``event:`` lines, which makes Claude Code fail
  with "JSON Parse error: Unexpected EOF".
  """
  fixer = StreamIndexFixer()
  pending_event: bytes | None = None

  for raw_line in upstream:
    stripped = raw_line.rstrip(b"\r\n")
    is_blank = stripped == b""

    if stripped.startswith(b"event: "):
      pending_event = raw_line
      continue

    if stripped.startswith(b"data: "):
      fixed = fixer.fix_sse_line(raw_line)
      if fixed is None:
        pending_event = None
        continue
      if pending_event is not None:
        yield pending_event
        pending_event = None
      yield fixed
      continue

    if is_blank:
      if pending_event is not None:
        # Upstream sent event: without a following data: line — drop the orphan.
        pending_event = None
      yield raw_line
      continue

    if pending_event is not None:
      yield pending_event
      pending_event = None
    yield raw_line


class AnthropicProxyHandler(BaseHTTPRequestHandler):
  server_version = "ClaudeSSEProxy/1.0"

  def log_message(self, format: str, *args) -> None:  # noqa: A003
    sys.stderr.write("%s - %s\n" % (self.address_string(), format % args))

  def _proxy(self) -> None:
    upstream_base: str = self.server.upstream_base  # type: ignore[attr-defined]
    body = self.rfile.read(int(self.headers.get("Content-Length", "0") or "0"))
    if "?" in self.path:
      path, query = self.path.split("?", 1)
      url = build_upstream_url(upstream_base, path, query)
    else:
      url = build_upstream_url(upstream_base, self.path, "")

    request = urllib.request.Request(url=url, data=body or None, method=self.command)
    for key, value in forward_headers(self).items():
      request.add_header(key, value)

    try:
      upstream_response = urllib.request.urlopen(request, timeout=600)
    except urllib.error.HTTPError as exc:
      payload = exc.read()
      self.send_response(exc.code)
      for key, value in exc.headers.items():
        lower = key.lower()
        if lower in HOP_BY_HOP_HEADERS:
          continue
        self.send_header(key, value)
      self.end_headers()
      if payload:
        self.wfile.write(payload)
      return
    except urllib.error.URLError as exc:
      body_bytes = json.dumps(
        {"error": {"type": "proxy_error", "message": f"upstream unreachable: {exc.reason}"}}
      ).encode("utf-8")
      self.send_response(502)
      self.send_header("Content-Type", "application/json")
      self.send_header("Content-Length", str(len(body_bytes)))
      self.end_headers()
      self.wfile.write(body_bytes)
      return

    content_type = upstream_response.headers.get("Content-Type", "")
    should_fix = request_wants_stream(body) or "text/event-stream" in content_type.lower()

    self.send_response(upstream_response.status)
    for key, value in upstream_response.headers.items():
      lower = key.lower()
      if lower in HOP_BY_HOP_HEADERS:
        continue
      self.send_header(key, value)
    self.end_headers()

    if should_fix:
      for chunk in iter_fixed_sse_frames(upstream_response):
        self.wfile.write(chunk)
        self.wfile.flush()
    else:
      while True:
        chunk = upstream_response.read(64 * 1024)
        if not chunk:
          break
        self.wfile.write(chunk)

    upstream_response.close()

  def do_GET(self) -> None:  # noqa: N802
    self._proxy()

  def do_POST(self) -> None:  # noqa: N802
    self._proxy()

  def do_PUT(self) -> None:  # noqa: N802
    self._proxy()

  def do_PATCH(self) -> None:  # noqa: N802
    self._proxy()

  def do_DELETE(self) -> None:  # noqa: N802
    self._proxy()


def parse_args() -> argparse.Namespace:
  parser = argparse.ArgumentParser(description="Repair malformed Anthropic SSE streams for Claude Code.")
  parser.add_argument("--bind", default=DEFAULT_BIND, help="Bind host (default: 127.0.0.1)")
  parser.add_argument("--port", type=int, default=DEFAULT_PORT, help="Listen port (default: 8787)")
  parser.add_argument(
    "--upstream",
    default=DEFAULT_UPSTREAM,
    help="Upstream Anthropic-compatible base URL (default: UPSTREAM_BASE_URL or ai.zkmjnic.tech)",
  )
  return parser.parse_args()


def main() -> int:
  args = parse_args()
  server = ThreadingHTTPServer((args.bind, args.port), AnthropicProxyHandler)
  server.upstream_base = args.upstream  # type: ignore[attr-defined]
  print(
    f"claude-sse-proxy listening on http://{args.bind}:{args.port} -> {args.upstream}",
    file=sys.stderr,
  )
  print("Set ANTHROPIC_BASE_URL to the listen URL above.", file=sys.stderr)
  try:
    server.serve_forever()
  except KeyboardInterrupt:
    print("\nshutting down", file=sys.stderr)
  finally:
    server.server_close()
  return 0


if __name__ == "__main__":
  raise SystemExit(main())
