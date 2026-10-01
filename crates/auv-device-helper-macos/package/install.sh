#!/bin/sh
set -eu

if [ "$(id -u)" -ne 0 ]; then
  echo 'run the installer as root after reviewing the signed package' >&2
  exit 1
fi
if [ "$#" -ne 1 ]; then
  echo 'usage: install.sh /path/to/AUV\ Device\ Entry\ Host.app' >&2
  exit 1
fi
team_id='433DLLA855'

package_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
source_app=$1
destination_dir='/Library/Application Support/AUV'
destination_app="$destination_dir/AUV Device Entry Host.app"
agent='/Library/LaunchAgents/dev.moeru.auv.device-entry-host.plist'
team_file="$destination_dir/device-entry-host.team-id"
if [ -e "$destination_app" ] || [ -e "$agent" ] || [ -e "$team_file" ]; then
  echo 'Device Entry Host is already installed; refusing to overwrite it' >&2
  exit 1
fi
/usr/bin/codesign --verify --strict --verbose=2 \
  --test-requirement="=identifier \"dev.moeru.auv.device-entry-host\" and anchor apple generic and certificate leaf[subject.OU] = \"$team_id\"" "$source_app"
/usr/bin/plutil -lint "$package_dir/LaunchAgent.plist"
/bin/mkdir -p "$destination_dir"
/bin/chmod 755 "$destination_dir"
/usr/bin/ditto "$source_app" "$destination_app"
/usr/sbin/chown -R root:wheel "$destination_app"
/bin/cp "$package_dir/LaunchAgent.plist" "$agent"
/usr/sbin/chown root:wheel "$agent"
/bin/chmod 644 "$agent"
printf '%s\n' "$team_id" > "$team_file"
/usr/sbin/chown root:wheel "$team_file"
/bin/chmod 644 "$team_file"
/usr/bin/codesign --verify --strict --verbose=2 \
  --test-requirement="=identifier \"dev.moeru.auv.device-entry-host\" and anchor apple generic and certificate leaf[subject.OU] = \"$team_id\"" "$destination_app"
echo 'Signed Device Entry Host installed. Log in to load its Aqua LaunchAgent, then grant Accessibility to the installed app.'
