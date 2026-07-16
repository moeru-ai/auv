#!/usr/bin/env bash
set -euo pipefail

workspace="$(git rev-parse --show-toplevel)"
readme="$workspace/README.md"
matrix="$workspace/docs/SUPPORT_MATRIX.md"

assert_contains() {
  local file="$1"
  local expected="$2"
  if ! grep -Fq -- "$expected" "$file"; then
    printf 'missing required public claim in %s:\n%s\n' "$file" "$expected" >&2
    exit 1
  fi
}

assert_contains "$readme" 'cargo install --git https://github.com/moeru-ai/auv auv-cli --bin auv'
assert_contains "$readme" '[support matrix](docs/SUPPORT_MATRIX.md)'

if grep -Fq '| Capability | AUV |' "$readme"; then
  printf 'README must not restore an unaudited competitor capability table\n' >&2
  exit 1
fi

for level in contract compiles tested live-validated supported; do
  assert_contains "$matrix" "| \`$level\` |"
done

assert_contains "$matrix" '| App-local recorded operation: TextEdit document write | `auv invoke app.textedit.document.write` | `live-validated` | `not claimed` | `not claimed` |'
assert_contains "$matrix" 'Live closure was manually validated on one macOS environment (2026-07-15, `semantic_matched=true`). It is not an automated live regression gate, and `state_changed` remains `false` because no pre-write AX observation is recorded. Not yet `supported`.'
assert_contains "$matrix" '| JS/TS and Python bindings | none | `not claimed` | `not claimed` | `not claimed` |'

if grep -nE '[✅❌⚠️💡]' "$matrix"; then
  printf 'public capability claims must use evidence levels, not icon classifications\n' >&2
  exit 1
fi

awk '
  /^## Product Surface$/ { in_matrix = 1; next }
  /^## / && in_matrix { exit }
  in_matrix && /^\|/ && $0 !~ /^\| Capability/ && $0 !~ /^\| ---/ {
    column_count = split($0, columns, /\|/)
    if (column_count < 9 || columns[7] !~ /\]\(/) {
      printf "matrix row has no evidence-column link: %s\n", $0 > "/dev/stderr"
      exit 1
    }
  }
' "$matrix"
