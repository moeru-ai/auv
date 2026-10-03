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
# Applications embedding AUV may ship the helper under their own name, bundle
# identifier, icon, and signing team; AUV then trusts it through
# AUV_MACOS_HELPER_APP. The defaults build the official AUV Helper.
helper_name=${AUV_MACOS_HELPER_NAME:-'AUV Helper'}
bundle_id=${AUV_MACOS_HELPER_BUNDLE_ID:-'ai.moeru.auv.helper'}
team_id=${AUV_MACOS_TEAM_ID:-'433DLLA855'}

package_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$package_dir/../../.." && pwd)
package_id=$(cargo pkgid --manifest-path "$repo_root/Cargo.toml" -p auv-device-helper-macos)
package_version=${package_id##*#}
if [ -z "$package_version" ] || [ "$package_version" = "$package_id" ]; then
  echo 'could not resolve the auv-device-helper-macos package version' >&2
  exit 1
fi
output_dir=${1:-"$repo_root/target/auv-helper-macos"}
app="$output_dir/$helper_name.app"
if [ -e "$app" ]; then
  echo 'output app already exists; choose a new output directory' >&2
  exit 1
fi
# An Icon Composer `.icon` document compiles to a Liquid Glass Assets.car plus
# a fallback .icns. A prebuilt `.icns` is copied as the only icon.
icon_source=${AUV_MACOS_HELPER_ICON:-"$package_dir/AUV Helper.icon"}
case "$icon_source" in
  *.icon) icon_name=$(basename "$icon_source" .icon) ;;
  *.icns) icon_name=$(basename "$icon_source" .icns) ;;
  *)
    echo 'AUV_MACOS_HELPER_ICON must name an Icon Composer .icon document or an .icns file' >&2
    exit 1
    ;;
esac
icon_output=$(/usr/bin/mktemp -d "${TMPDIR:-/tmp}/auv-helper-icon.XXXXXX")
trap '/bin/rm -rf "$icon_output"' EXIT
icon_compiled="$icon_output/compiled"
mkdir -p "$icon_compiled"

if [ "${icon_source%.icns}" != "$icon_source" ]; then
  cp "$icon_source" "$icon_compiled/$icon_name.icns"
else
  # actool names the compiled icon after the .icon document.
  /usr/bin/xcrun actool "$icon_source" \
    --compile "$icon_compiled" \
    --platform macosx \
    --minimum-deployment-target 13.0 \
    --app-icon "$icon_name" \
    --output-partial-info-plist "$icon_output/partial.plist" \
    --output-format human-readable-text
  # NOTICE(helper-icon-xcode): actool before Xcode 26 cannot read Icon Composer
  # `.icon` bundles and exits successfully without output. Fail here with the
  # cause instead of at a later copy.
  for compiled in "$icon_name.icns" 'Assets.car'; do
    if [ ! -f "$icon_compiled/$compiled" ]; then
      echo "actool produced no $compiled; $(basename "$icon_source") requires Xcode 26 or later ($(/usr/bin/xcodebuild -version | head -n 1))" >&2
      exit 1
    fi
  done
fi

if [ -n "${AUV_MACOS_TARGET:-}" ]; then
  MACOSX_DEPLOYMENT_TARGET=13.0 cargo build --release --locked -p auv-device-helper-macos --features host --manifest-path "$repo_root/Cargo.toml" --target "$AUV_MACOS_TARGET"
  helper_binary="$repo_root/target/$AUV_MACOS_TARGET/release/auv-device-helper-macos"
else
  MACOSX_DEPLOYMENT_TARGET=13.0 cargo build --release --locked -p auv-device-helper-macos --features host --manifest-path "$repo_root/Cargo.toml"
  helper_binary="$repo_root/target/release/auv-device-helper-macos"
fi
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$app/Contents/Library/LaunchAgents"
cp "$helper_binary" "$app/Contents/MacOS/auv-device-helper-macos"
info="$app/Contents/Info.plist"
# ServiceManagement finds the LaunchAgent by the helper's bundle identifier.
launch_agent="$app/Contents/Library/LaunchAgents/$bundle_id.plist"
cp "$package_dir/Info.plist" "$info"
/usr/bin/plutil -replace CFBundleShortVersionString -string "$package_version" "$info"
/usr/bin/plutil -replace CFBundleIdentifier -string "$bundle_id" "$info"
/usr/bin/plutil -replace CFBundleName -string "$helper_name" "$info"
/usr/bin/plutil -replace CFBundleDisplayName -string "$helper_name" "$info"
/usr/bin/plutil -replace CFBundleIconFile -string "$icon_name" "$info"
if [ -f "$icon_compiled/Assets.car" ]; then
  /usr/bin/plutil -replace CFBundleIconName -string "$icon_name" "$info"
  cp "$icon_compiled/Assets.car" "$app/Contents/Resources/Assets.car"
else
  # Without an asset catalog, CFBundleIconName would name a missing icon.
  /usr/bin/plutil -remove CFBundleIconName "$info"
fi
cp "$package_dir/LaunchAgent.plist" "$launch_agent"
/usr/bin/plutil -replace Label -string "$bundle_id" "$launch_agent"
cp "$icon_compiled/$icon_name.icns" "$app/Contents/Resources/$icon_name.icns"
/usr/bin/plutil -lint "$info"
/usr/bin/plutil -lint "$launch_agent"
/usr/bin/codesign --force --timestamp --options runtime --sign "$AUV_MACOS_SIGN_IDENTITY" "$app"
/usr/bin/codesign --verify --strict --verbose=2 \
  --test-requirement="=identifier \"$bundle_id\" and anchor apple generic and certificate leaf[subject.OU] = \"$team_id\"" "$app"
echo "Signed helper app: $app"
