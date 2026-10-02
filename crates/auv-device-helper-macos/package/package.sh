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
package_id=$(cargo pkgid --manifest-path "$repo_root/Cargo.toml" -p auv-device-helper-macos)
package_version=${package_id##*#}
if [ -z "$package_version" ] || [ "$package_version" = "$package_id" ]; then
  echo 'could not resolve the auv-device-helper-macos package version' >&2
  exit 1
fi
output_dir=${1:-"$repo_root/target/auv-helper-macos"}
app="$output_dir/AUV Helper.app"
if [ -e "$app" ]; then
  echo 'output app already exists; choose a new output directory' >&2
  exit 1
fi
icon_source="$package_dir/AUV Helper.icon"
icon_output=$(/usr/bin/mktemp -d "${TMPDIR:-/tmp}/auv-helper-icon.XXXXXX")
trap '/bin/rm -rf "$icon_output"' EXIT
icon_compiled="$icon_output/compiled"
mkdir -p "$icon_compiled"

/usr/bin/xcrun actool "$icon_source" \
  --compile "$icon_compiled" \
  --platform macosx \
  --minimum-deployment-target 13.0 \
  --app-icon 'AUV Helper' \
  --output-partial-info-plist "$icon_output/partial.plist" \
  --output-format human-readable-text
# NOTICE(helper-icon-xcode): actool before Xcode 26 cannot read Icon Composer
# `.icon` bundles and exits successfully without output. Fail here with the
# cause instead of at a later copy.
for compiled in 'AUV Helper.icns' 'Assets.car'; do
  if [ ! -f "$icon_compiled/$compiled" ]; then
    echo "actool produced no $compiled; AUV Helper.icon requires Xcode 26 or later ($(/usr/bin/xcodebuild -version | head -n 1))" >&2
    exit 1
  fi
done

if [ -n "${AUV_MACOS_TARGET:-}" ]; then
  MACOSX_DEPLOYMENT_TARGET=13.0 cargo build --release --locked -p auv-device-helper-macos --features host --manifest-path "$repo_root/Cargo.toml" --target "$AUV_MACOS_TARGET"
  helper_binary="$repo_root/target/$AUV_MACOS_TARGET/release/auv-device-helper-macos"
else
  MACOSX_DEPLOYMENT_TARGET=13.0 cargo build --release --locked -p auv-device-helper-macos --features host --manifest-path "$repo_root/Cargo.toml"
  helper_binary="$repo_root/target/release/auv-device-helper-macos"
fi
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$app/Contents/Library/LaunchAgents"
cp "$helper_binary" "$app/Contents/MacOS/auv-device-helper-macos"
cp "$package_dir/Info.plist" "$app/Contents/Info.plist"
/usr/bin/plutil -replace CFBundleShortVersionString -string "$package_version" "$app/Contents/Info.plist"
cp "$package_dir/ai.moeru.auv.helper.plist" "$app/Contents/Library/LaunchAgents/ai.moeru.auv.helper.plist"
cp "$icon_compiled/AUV Helper.icns" "$app/Contents/Resources/AUV Helper.icns"
cp "$icon_compiled/Assets.car" "$app/Contents/Resources/Assets.car"
/usr/bin/plutil -lint "$app/Contents/Info.plist"
/usr/bin/plutil -lint "$app/Contents/Library/LaunchAgents/ai.moeru.auv.helper.plist"
/usr/bin/codesign --force --timestamp --options runtime --sign "$AUV_MACOS_SIGN_IDENTITY" "$app"
/usr/bin/codesign --verify --strict --verbose=2 \
  --test-requirement="=identifier \"ai.moeru.auv.helper\" and anchor apple generic and certificate leaf[subject.OU] = \"$team_id\"" "$app"
echo "Signed helper app: $app"
