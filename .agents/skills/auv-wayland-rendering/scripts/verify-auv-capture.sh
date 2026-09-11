#!/usr/bin/env bash
set -euo pipefail

auv_bin=auv
repeat=3
store_root=${AUV_STORE_ROOT:-$PWD/.auv/wayland-validation}

while (( $# > 0 )); do
  case "$1" in
    --auv)
      auv_bin=${2:?--auv requires a path}
      shift 2
      ;;
    --repeat)
      repeat=${2:?--repeat requires a count}
      shift 2
      ;;
    --store-root)
      store_root=${2:?--store-root requires a path}
      shift 2
      ;;
    -h|--help)
      printf 'usage: %s [--auv PATH] [--repeat N] [--store-root PATH]\n' "$0"
      exit 0
      ;;
    *)
      printf 'unknown argument: %s\n' "$1" >&2
      exit 2
      ;;
  esac
done

if [[ ! "$repeat" =~ ^[1-9][0-9]*$ ]]; then
  printf -- '--repeat must be a positive integer\n' >&2
  exit 2
fi

if [[ "$auv_bin" == */* ]]; then
  [[ -x "$auv_bin" ]] || {
    printf 'AUV executable is not executable: %s\n' "$auv_bin" >&2
    exit 1
  }
elif ! command -v "$auv_bin" >/dev/null 2>&1; then
  printf 'AUV executable was not found: %s\n' "$auv_bin" >&2
  exit 1
fi

command -v python3 >/dev/null 2>&1 || {
  printf 'python3 is required to validate AUV JSON output\n' >&2
  exit 1
}

mkdir -p "$store_root"
work_dir=$(mktemp -d "${TMPDIR:-/tmp}/auv-wayland-capture.XXXXXX")
trap 'find "$work_dir" -type f -delete 2>/dev/null || true; rmdir "$work_dir" 2>/dev/null || true' EXIT

for ((attempt = 1; attempt <= repeat; attempt++)); do
  result_file=$work_dir/result-$attempt.json
  "$auv_bin" invoke display.capture --store-root "$store_root" --json >"$result_file"

  python3 - "$result_file" "$attempt" <<'PY'
import json
import pathlib
import sys

result_path = pathlib.Path(sys.argv[1])
attempt = sys.argv[2]
result = json.loads(result_path.read_text())

if result.get("status") != "completed":
    raise SystemExit(f"capture {attempt} did not complete: {result}")
if result.get("command_id") != "display.capture":
    raise SystemExit(f"capture {attempt} returned unexpected command_id: {result.get('command_id')}")

artifacts = [
    artifact
    for artifact in result.get("artifacts", [])
    if artifact.get("purpose") == "auv.driver.display_capture"
]
if len(artifacts) != 1:
    raise SystemExit(f"capture {attempt} returned {len(artifacts)} display artifacts")

artifact = artifacts[0]
if artifact.get("content_type") != "image/png":
    raise SystemExit(f"capture {attempt} returned unexpected content type: {artifact.get('content_type')}")

file_path = artifact.get("file_path")
if not file_path:
    raise SystemExit(f"capture {attempt} did not expose a file_path")

png = pathlib.Path(file_path)
if not png.is_file():
    raise SystemExit(f"capture {attempt} artifact does not exist: {png}")
with png.open("rb") as stream:
    signature = stream.read(8)
if signature != b"\x89PNG\r\n\x1a\n":
    raise SystemExit(f"capture {attempt} artifact is not a PNG: {png}")

print(f"capture {attempt}: run={result.get('run_id')} artifact={png}")
PY
done

printf 'validated %d AUV display capture(s); store=%s\n' "$repeat" "$store_root"
