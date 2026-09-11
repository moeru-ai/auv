#!/usr/bin/env bash
set -euo pipefail

if (( $# == 0 )); then
  printf 'usage: %s <command> [args...]\n' "$0" >&2
  exit 2
fi

runtime_dir=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
export XDG_RUNTIME_DIR=$runtime_dir

wayland_socket_is_live() {
  local display=${WAYLAND_DISPLAY:-}
  [[ -n "$display" ]] || return 1
  if [[ "$display" = /* ]]; then
    [[ -S "$display" ]]
  else
    [[ -S "$XDG_RUNTIME_DIR/$display" ]]
  fi
}

manager_value() {
  local key=$1
  systemctl --user show-environment 2>/dev/null | sed -n "s/^${key}=//p" | tail -n 1
}

if ! wayland_socket_is_live && command -v systemctl >/dev/null 2>&1; then
  for key in WAYLAND_DISPLAY SWAYSOCK XDG_CURRENT_DESKTOP XDG_SESSION_TYPE; do
    current=$(manager_value "$key")
    if [[ -n "$current" ]]; then
      printf -v "$key" '%s' "$current"
      export "$key"
    fi
  done
fi

if ! wayland_socket_is_live; then
  printf 'no live Wayland socket was found in the shell or systemd user environment\n' >&2
  exit 1
fi

if [[ -z ${DBUS_SESSION_BUS_ADDRESS:-} && -S "$XDG_RUNTIME_DIR/bus" ]]; then
  export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"
fi

exec "$@"
