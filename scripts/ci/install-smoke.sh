#!/usr/bin/env bash
set -euo pipefail

workspace="$(git rev-parse --show-toplevel)"
temp_root="$(mktemp -d "${TMPDIR:-/tmp}/auv-install-smoke.XXXXXX")"
trap 'rm -rf "$temp_root"' EXIT INT TERM

source_snapshot="$temp_root/source"
install_root="$temp_root/install-root"
target_dir="$temp_root/target"

# Cargo installs a Git revision, so make a disposable Git snapshot that includes
# tracked and non-ignored uncommitted files from the worktree under test.
git clone --quiet --no-hardlinks --recurse-submodules "$workspace" "$source_snapshot"
git -C "$workspace" diff --binary HEAD | git -C "$source_snapshot" apply --binary
while IFS= read -r -d '' path; do
  mkdir -p "$(dirname "$source_snapshot/$path")"
  cp -p "$workspace/$path" "$source_snapshot/$path"
done < <(git -C "$workspace" ls-files --others --exclude-standard -z)
git -C "$source_snapshot" add --all
if ! git -C "$source_snapshot" diff --cached --quiet; then
  git -C "$source_snapshot" -c user.name='AUV install smoke' -c user.email='install-smoke@localhost' commit --quiet -m 'install smoke snapshot'
fi
commit="$(git -C "$source_snapshot" rev-parse HEAD)"

cargo install \
  --git "file://$source_snapshot" \
  --rev "$commit" \
  --root "$install_root" \
  --target-dir "$target_dir" \
  --locked \
  --force \
  --bin auv \
  auv-cli

installed_auv="$install_root/bin/auv"
test -x "$installed_auv"

version_output="$("$installed_auv" --version)"
expected_version="auv $(cargo pkgid --manifest-path "$workspace/crates/auv-cli/Cargo.toml" | sed 's/.*#//')"
test "$version_output" = "$expected_version"

"$installed_auv" invoke --help
