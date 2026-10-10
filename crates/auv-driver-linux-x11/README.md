# AUV Linux X11 Driver

> **Legacy compatibility only — continued use is not recommended.**
> Prefer `auv-driver-linux` and a Wayland session for new deployments. This
> crate exists for environments that still require X11, such as existing
> benchmark images. Migrate away when those environments support Wayland.

The name refers to the X11 protocol, not a particular server implementation.
Xorg and Xvfb are intended compatibility targets. XWayland does not grant
access to native Wayland windows or the complete Wayland desktop.

## Scope

This Rust driver crate uses the shared `auv-driver-common` contracts. It can be
selected explicitly, and `auv-driver::LocalDriver` selects it for a pure X11
process environment. The scope is screen observation and foreground
mouse/keyboard input. Dispatch success is not application-level verification:
callers must inspect the resulting state.

## When to use

- An existing X11 benchmark or application environment cannot migrate yet.
- A dedicated X11 session is available for repeatable automation.

## When not to use

- New desktop automation deployments that can use Wayland.
- Access to native Wayland windows through XWayland.
- Background input or semantic completion guarantees.

## Integration boundary

`auv-driver::LocalDriver` selects X11 only when `WAYLAND_DISPLAY` is missing or
empty and `DISPLAY` is nonempty. A nonempty `WAYLAND_DISPLAY` keeps the Wayland
driver even when `DISPLAY` also names an XWayland server. With neither variable,
the facade preserves the Wayland default and its existing diagnostics.

The daemon Runner uses the same selection. Display capture and foreground
input are available there, including sampled movement/drag, bounded held
buttons/keys, and screen-point scrolling. Unsupported X11 window and clipboard
calls remain explicit errors. This does not make every higher-level Electron
workflow X11-capable.

## Evidence

Validated first on Debian 12 arm64 with Xvfb 21.1.7 and Tk 8.6.13, then through
the integrated root facade on Ubuntu 24.04 x86_64 with XFCE and dummy Xorg
1920x1080. macOS rejects session opening.
Physical-seat and GPU-accelerated Xorg and XWayland remain unvalidated.

## Build and session setup

Use the workspace Rust toolchain (Rust >= 1.91). Input uses Enigo 0.6.1 with
only its `x11rb` feature; capture reuses xcap 0.6.2. On Debian/Ubuntu, xcap's
Linux build requires development packages including `pkg-config`, `libxcb1-dev`,
`libxrandr-dev`, `libxkbcommon-dev`, `libwayland-dev`, `libegl1-mesa-dev`,
`libgbm-dev`, `libdbus-1-dev`,
`libpipewire-0.3-dev`, and `libclang-dev`.
Wayland, EGL, GBM and PipeWire are build dependencies of xcap's Linux
support; this driver uses its X11 capture path. An existing X server must expose XTEST and XRandR.

Run in the logged-in X11 user's environment with a valid `DISPLAY` and the
session's Xauthority credentials. Do not use `xhost +` or run the desktop as
root. The driver does not start a display server or manage authentication.

For a read-only capture:

```bash
cargo run -p auv-driver-linux-x11 --example capture -- /tmp/auv-x11.png
```

The process must have `XDG_SESSION_TYPE=x11` (or unset) and no nonempty
`WAYLAND_DISPLAY`. For an explicitly chosen XWayland compatibility display,
launch a **separate** process with the matching DISPLAY/XAUTHORITY and those
X11-only environment settings. This still only observes that X server, not
native Wayland surfaces; rootless XWayland desktop capture is not claimed.
Never change DISPLAY/XAUTHORITY within a running process. xcap caches its XCB
connection globally, so use one display per process and do not initialize
xcap against another display before opening this driver.

## Rust API

```rust,no_run
use auv_driver_common::{CaptureOptions, Click, ClickModifiers, Driver, Point};
use auv_driver_linux_x11::X11Driver;

let session = X11Driver.open_local()?;
let screenshot = session.display().capture(CaptureOptions::default())?;
let delivery = session.input().click_at(
  Point::new(100.0, 100.0),
  Click::Single,
  ClickModifiers::default(),
)?;
assert!(!delivery.verified); // Inspect a new observation to verify the UI effect.
# Ok::<(), auv_driver_common::DriverError>(())
```

- `display().list/capture/capture_region`: selected monitor or contained region.
- `input().current_position/move_to/click_at/click_button_at/drag`: foreground
  pointer operations. `drag` is a direct move, not a timed trajectory.
- `input().move_mouse/drag_mouse`: shared sampled trajectories with progress
  notification and one-admission press/move/release cleanup.
- `input().create_mouse/mouse_down/move_mouse_to/mouse_up/remove_mouse` and
  `hold_mouse`: bounded logical-button ownership over the shared OS cursor.
- `input().press_key/press_keys/type_text`: named keys, scoped key combinations,
  and Unicode text. `type_text` does not use the clipboard.
  `replace_existing` sends Ctrl+A then Backspace and requires the focused app
  to bind Ctrl+A to select-all; this is not universal (Tk defaults differ).
- `input().key_down/key_up/hold_keys`: bounded held combinations using the
  shared reverse-order release controller.
- `input().scroll_at`: logical-pixel deltas (positive X right, positive Y down),
  converted to XTEST wheel detents at 120 pixels per notch with sub-notch
  remainder carried within one session; at most 1024 detents per axis per call.

The same screen-point wheel operation is available through the Runner
`ScrollScreenPoint` RPC and `auv invoke input.scrollPoint X Y DX DY`.

Coordinates are integral X11 root-window pixels. Capture scale is 1.0;
Xft font DPI is not treated as input coordinate scaling. XTEST motion is
limited to signed 16-bit coordinates. Use one X screen per worker; multi-screen
pointer routing is not validated. Monitor topology changes require fresh
observations.

Input errors may follow partial delivery. The driver attempts to release
scoped keys/buttons after errors and never replays the action automatically.
Clones share one input mutex. Other sessions, processes, or humans can still
interleave input; dedicate the session to one agent worker.

## Intentional limits

Window enumeration/activation, AT-SPI, clipboard ownership, background input,
and overlay rendering are not part of this compatibility crate. The root facade
can apply its existing capture-driven OCR to X11 display captures. Existing
shared types are reused; there is no alternate action-result or artifact schema.
`allow_clipboard_fallback` permits but does not require a fallback; this driver
never takes that path.

## Validation commands

```bash
cargo test -p auv-driver-linux-x11
cargo clippy -p auv-driver-linux-x11 --all-targets -- -D warnings
cargo fmt -p auv-driver-linux-x11 --check
```

The integration test is ignored by default because it moves the pointer and
types into a real application. Run it in a dedicated Xvfb display with
`xvfb`, `xauth`, and `python3-tk` installed:

```bash
env -u WAYLAND_DISPLAY XDG_SESSION_TYPE=x11 \
  xvfb-run -a -s '-screen 0 800x600x24' \
  cargo test -p auv-driver-linux-x11 --test xvfb -- --ignored --nocapture
```

It checks monitor pixels, region bounds, application text, screenshot freshness,
mouse buttons, scrolling, sampled movement/drag, logical button down/up, held
key down/up, and pointer position against a temporary Tk fixture with an
explicit Ctrl+A select-all binding. When using Docker, pass `--init` so
`xvfb-run` receives the X server readiness signal.
The fixture and X server are disposed after the test. It does not establish
general Xorg/Wayland desktop support.
