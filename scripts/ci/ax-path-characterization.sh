#!/usr/bin/env bash
# Characterizes the pure AX observed-path parse layer
# (`crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/AxPath.swift`).
#
# Compiles that REAL source file together with the standalone harness in a
# single `swiftc` invocation and runs it. No SwiftPM, no Rust static lib, no
# Accessibility permission required, so it runs on headless macOS CI.
#
# macOS-only: `swiftc` is not present on the Linux CI runner. The caller
# (.github/workflows/check.yml) guards this with `if: runner.os == 'macOS'`.
set -euo pipefail

if ! command -v swiftc >/dev/null 2>&1; then
  echo "ax-path-characterization: swiftc not found; this check is macOS-only" >&2
  exit 1
fi

workspace="$(git rev-parse --show-toplevel)"
swift_root="$workspace/crates/auv-driver-macos/native/swift"
parse_source="$swift_root/Sources/AuvMacosNative/AxPath.swift"
harness_source="$swift_root/characterization/AxPathCharacterization.swift"

for source in "$parse_source" "$harness_source"; do
  if [ ! -f "$source" ]; then
    echo "ax-path-characterization: missing source $source" >&2
    exit 1
  fi
done

binary="$(mktemp -t ax-path-characterization)"
trap 'rm -f "$binary"' EXIT

swiftc "$parse_source" "$harness_source" -o "$binary"
"$binary"
