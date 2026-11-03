# Windows DPI awareness

Issue: [#460](https://github.com/moeru-ai/auv/issues/460). Evidence level: live probe, plus
native tests. Date: 2026-11-03. Host: luoling-windows-11, one 3840x2160 display at 150%,
console session 1. Runner: the default `auv` binary of each tree (no `__COMPAT_LAYER`), driven
through the JS SDK from a temporary Interactive scheduled task.

## Root cause and fix

`auv` ran DPI-unaware. Windows then hands the process virtualized (logical) values from
`GetWindowRect`, `GetSystemMetrics`, `ScreenToClient` and the `SendInput` absolute axis, but the
DWM extended frame bounds, which `Window.frame` uses, stay physical. Window frames were physical,
while input and the `PrintWindow` bitmap were logical.

`auv_driver_windows::dpi::set_process_per_monitor_aware()` now runs first in `auv`'s `main`, and
again in `WindowsDriver::open_local` for library hosts. The whole driver then works in physical
pixels (`NOTICE(windows-dpi-awareness)` records the manifest trade-off). `PrintWindow` draws a
window at its own DPI, so the bitmap is sized at `GetDpiForWindow / GetDpiForMonitor`
(`NOTICE(windows-printwindow-dpi)`). A DPI-unaware window is captured at 96 DPI: the bounds are
the physical frame, the image has the real, smaller pixel size (no upscaling), and
`Capture.scaleFactor` is below 1.

## Probe

Two PowerShell WinForms windows filled with blue buttons that append to the title (one button
for the capture rows; a left and a right button, appending ` L` or ` R`, for the click row).
`u` is DPI-unaware (logical 1200,700 400x300), `p` makes itself per-monitor-v2 aware (physical
300,300 600x450).

| Check | Before (`origin/main` `bfb60a6e`) | After |
| --- | --- | --- |
| `window.list`, `u` / `p` | `1811,1050 578x439` / `309,300 582x441` | same |
| `window.capture`, `u` | bounds `1200,700 400x300` (logical), 400x300 px, scale 1 | bounds `1811,1050 578x439`, 386x293 px, scale 0.668, 86% button blue |
| `window.capture`, `p` | bounds `309,300 291x200`, 291x200 px (a corner) | bounds `309,300 582x441`, 582x441 px, scale 1 |
| `displays.capture()` | bounds `0,0 3840x2160`, 3840x2160 px, capture scale 1, display scale 1.5 | same |
| `click` at 20% and 85% of the width, default, `FOREGROUND_PREFERRED`, `BACKGROUND_ONLY`, both windows | OK, 0/12 changed the title | OK, 12/12 hit the expected button |

Native tests: `cargo test -p auv-driver-common -p auv-driver-windows --no-fail-fast` in the
console session passes (14 ignored). On `origin/main`,
`wgc_capture::test_wgc_pixel_correctness_and_gdi_comparison` fails; with the fix it and
`wgc_capture::issue_460_window_capture_covers_visible_frame_on_scaled_display` pass.

## Not covered

- 100% and 200% scaling, and mixed scales across displays (the shared desktop's scale was not
  changed).
- Window mutation (`move_to`, `resize`, `set_frame`) at 150%. `SetWindowPos` now takes the same
  physical pixels the frames report; the invisible resize border between the outer rect and the
  visible frame remains, as at 100%.
- A Runner forced DPI-unaware by a manifest or compatibility setting (`auv` prints a warning).
- The overlay: positions are physical, art keeps 96 DPI sizes (`TODO(windows-overlay-dpi)`).
