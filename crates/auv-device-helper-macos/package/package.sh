#!/bin/sh
set -eu

if [ "$(uname -s)" != Darwin ]; then
  echo 'macOS packaging requires a Mac' >&2
  exit 1
fi
if [ -z "${AUV_MACOS_SIGN_IDENTITY:-}" ] || [ "$AUV_MACOS_SIGN_IDENTITY" = '-' ]; then
  echo 'set AUV_MACOS_SIGN_IDENTITY to a stable non-adhoc signing identity' >&2
  exit 1
fi
team_id='433DLLA855'

package_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$package_dir/../../.." && pwd)
output_dir=${1:-"$repo_root/target/device-entry-macos"}
app="$output_dir/AUV Device Entry Host.app"
if [ -e "$app" ]; then
  echo 'output app already exists; choose a new output directory' >&2
  exit 1
fi

cargo build --release --locked -p auv-device-helper-macos --manifest-path "$repo_root/Cargo.toml"
mkdir -p "$app/Contents/MacOS"
cp "$repo_root/target/release/auv-device-helper-macos" "$app/Contents/MacOS/auv-device-helper-macos"
cp "$package_dir/Info.plist" "$app/Contents/Info.plist"
/usr/bin/plutil -lint "$app/Contents/Info.plist"
/usr/bin/codesign --force --sign "$AUV_MACOS_SIGN_IDENTITY" "$app"
/usr/bin/codesign --verify --strict --verbose=2 \
  --test-requirement="=identifier \"dev.moeru.auv.device-entry-host\" and anchor apple generic and certificate leaf[subject.OU] = \"$team_id\"" "$app"
echo "Signed helper package: $app"
