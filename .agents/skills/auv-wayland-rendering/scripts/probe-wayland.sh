#!/usr/bin/env bash
set -uo pipefail

require_gpu=false
case "${1:-}" in
  "") ;;
  --require-gpu) require_gpu=true ;;
  -h|--help)
    printf 'usage: %s [--require-gpu]\n' "$0"
    exit 0
    ;;
  *)
    printf 'unknown argument: %s\n' "$1" >&2
    exit 2
    ;;
esac

failures=0

pass() {
  printf '[ok]   %s\n' "$1"
}

warn() {
  printf '[warn] %s\n' "$1"
}

fail() {
  printf '[fail] %s\n' "$1"
  failures=$((failures + 1))
}

value() {
  local name=$1
  local current=${!name-}
  if [[ -n "$current" ]]; then
    printf '[info] %s=%s\n' "$name" "$current"
  else
    printf '[info] %s is unset\n' "$name"
  fi
}

printf 'AUV Wayland environment probe\n'
printf '[info] kernel=%s\n' "$(uname -srm 2>/dev/null || printf unknown)"

if [[ $(uname -s 2>/dev/null) == Linux ]]; then
  pass 'Linux host detected'
else
  fail 'this workflow requires Linux'
fi

for name in XDG_SESSION_TYPE XDG_CURRENT_DESKTOP XDG_RUNTIME_DIR WAYLAND_DISPLAY SWAYSOCK DBUS_SESSION_BUS_ADDRESS; do
  value "$name"
done

runtime_dir=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
if [[ -d "$runtime_dir" ]]; then
  pass "runtime directory exists: $runtime_dir"
else
  fail "runtime directory is missing: $runtime_dir"
fi

if [[ -n ${WAYLAND_DISPLAY:-} ]]; then
  if [[ ${WAYLAND_DISPLAY} = /* ]]; then
    wayland_socket=$WAYLAND_DISPLAY
  else
    wayland_socket=$runtime_dir/$WAYLAND_DISPLAY
  fi
  if [[ -S "$wayland_socket" ]]; then
    pass "Wayland socket is reachable: $wayland_socket"
  else
    fail "Wayland socket is missing: $wayland_socket"
  fi
else
  fail 'WAYLAND_DISPLAY is unset'
fi

if [[ -S "$runtime_dir/bus" ]]; then
  pass "session D-Bus socket exists: $runtime_dir/bus"
else
  fail "session D-Bus socket is missing: $runtime_dir/bus"
fi

if command -v busctl >/dev/null 2>&1; then
  if busctl --user --no-pager introspect \
    org.freedesktop.portal.Desktop \
    /org/freedesktop/portal/desktop \
    org.freedesktop.portal.ScreenCast >/dev/null 2>&1; then
    pass 'ScreenCast portal interface is available'
  else
    fail 'ScreenCast portal interface is unavailable on the session bus'
  fi
else
  warn 'busctl is unavailable; portal introspection was skipped'
fi

if command -v pw-cli >/dev/null 2>&1; then
  if pw-cli info 0 >/dev/null 2>&1; then
    pass 'PipeWire core is reachable'
  else
    fail 'pw-cli cannot reach the PipeWire core'
  fi
else
  warn 'pw-cli is unavailable; PipeWire probing was skipped'
fi

render_nodes=()
while IFS= read -r node; do
  render_nodes+=("$node")
done < <(compgen -G '/dev/dri/renderD*' || true)

if (( ${#render_nodes[@]} == 0 )); then
  if $require_gpu; then
    fail 'no DRM render node was found'
  else
    warn 'no DRM render node was found; only software rendering may be available'
  fi
else
  for node in "${render_nodes[@]}"; do
    if [[ -r "$node" && -w "$node" ]]; then
      pass "DRM render node is accessible: $node"
    elif $require_gpu; then
      fail "DRM render node is not readable and writable: $node"
    else
      warn "DRM render node is not readable and writable: $node"
    fi
  done
fi

renderer_evidence=false
renderer_summary=
if command -v eglinfo >/dev/null 2>&1; then
  renderer_output=$(eglinfo -B 2>/dev/null || true)
  renderer_summary=$(printf '%s\n' "$renderer_output" | sed -n '/EGL vendor string/p;/OpenGL.*renderer:/p;/Device platform:/p;/Device:/p')
  if [[ -n "$renderer_summary" ]]; then
    printf '[info] eglinfo renderer summary:\n'
    printf '%s\n' "$renderer_summary" | sed 's/^/       /'
    renderer_evidence=true
  else
    warn 'eglinfo is installed but did not report a renderer'
  fi
fi

if ! $renderer_evidence && command -v vulkaninfo >/dev/null 2>&1; then
  renderer_output=$(vulkaninfo --summary 2>/dev/null || true)
  renderer_summary=$(printf '%s\n' "$renderer_output" | sed -n '/deviceName/p;/driverName/p;/driverInfo/p')
  if [[ -n "$renderer_summary" ]]; then
    printf '[info] vulkaninfo summary:\n'
    printf '%s\n' "$renderer_summary" | sed 's/^/       /'
    renderer_evidence=true
  else
    warn 'vulkaninfo is installed but did not report a renderer'
  fi
elif ! command -v eglinfo >/dev/null 2>&1 && ! command -v vulkaninfo >/dev/null 2>&1; then
  warn 'neither eglinfo nor vulkaninfo is installed; inspect compositor logs for renderer evidence'
fi

if $require_gpu && ! $renderer_evidence; then
  fail 'GPU prerequisites were required but no renderer tool produced evidence'
elif $require_gpu && ! printf '%s\n' "$renderer_summary" | grep -Ei 'renderer|deviceName' | grep -Eivq 'llvmpipe|softpipe|swrast|pixman'; then
  fail 'GPU prerequisites were required but renderer tools reported only software candidates'
elif $renderer_evidence && printf '%s\n' "$renderer_summary" | grep -Eiq 'llvmpipe|softpipe|swrast|pixman'; then
  warn 'renderer output includes a software implementation; correlate the relevant platform with compositor logs'
fi

if command -v swaymsg >/dev/null 2>&1 && [[ -n ${SWAYSOCK:-} ]]; then
  if swaymsg -t get_outputs -r >/dev/null 2>&1; then
    pass 'Sway IPC reports at least an output response'
  else
    fail 'Sway IPC is configured but get_outputs failed'
  fi
elif command -v wayland-info >/dev/null 2>&1 && [[ -n ${WAYLAND_DISPLAY:-} ]]; then
  if wayland-info >/dev/null 2>&1; then
    pass 'wayland-info connected to the compositor'
  else
    fail 'wayland-info could not connect to the compositor'
  fi
else
  warn 'no compositor connection probe was available'
fi

if command -v systemctl >/dev/null 2>&1; then
  for service_name in xdg-desktop-portal.service xdg-desktop-portal-wlr.service pipewire.service wireplumber.service; do
    service_state=$(systemctl --user is-active "$service_name" 2>/dev/null || true)
    printf '[info] %-35s %s\n' "$service_name" "${service_state:-unknown}"
  done
fi

for command_name in sway swaymsg wayland-info grim wayvnc auv; do
  if command -v "$command_name" >/dev/null 2>&1; then
    printf '[info] %-27s %s\n' "$command_name" "$(command -v "$command_name")"
  else
    printf '[info] %-27s not found\n' "$command_name"
  fi
done

if (( failures > 0 )); then
  printf '[fail] probe completed with %d required check(s) failing\n' "$failures"
  exit 1
fi

pass 'probe completed without required-check failures'
