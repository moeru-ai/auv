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

The driver works in physical pixels and needs a per-monitor DPI aware host process. Who sets
that awareness (`NOTICE(windows-dpi-awareness)` in `auv_driver_windows::dpi`, revised
2026-11-04 by owner decision):

- `auv.exe` declares per-monitor-v2 in an embedded application manifest
  (`crates/auv-cli/auv.exe.manifest`, linked into the `auv` binary only by
  `crates/auv-cli/build.rs` through `winresource`, `asInvoker`, no `uiAccess`). The process is
  aware from its first instruction, before any `HWND`. This is Microsoft's recommended way
  ([Setting the default DPI awareness for a process](https://learn.microsoft.com/en-us/windows/win32/hidpi/setting-the-default-dpi-awareness-for-a-process)),
  and the trycua/cua precedent: `libs/cua-driver/rust/crates/cua-driver/build.rs` embeds
  `cua-driver.manifest` (`true/pm`, `permonitorv2`, Windows 10/11 `supportedOS`), and its driver
  library never changes the host's mode. AUV's manifest lists `permonitorv2,permonitor`, the
  documented fallback for Windows 10 1607.
- The driver library does not change the host's awareness. `WindowsDriver::open_local` prints
  one warning per process when the host is not per-monitor aware: on a scaled display its
  coordinates are then DPI-virtualized. A host of the library must declare per-monitor-v2
  awareness itself, best in its own manifest. A compatibility setting (`__COMPAT_LAYER`, or
  "Override high DPI scaling behavior" in the file's properties) still overrides `auv.exe`'s
  manifest, so the same warning covers that case.
- AUV's own executables without the manifest opt in explicitly with
  `auv_driver_windows::dpi::set_process_per_monitor_aware()` before their first window: the
  `wgc_capture` integration tests (in `create_test_window`) and `background-session-eval`.

The first revision of this fix called `SetProcessDpiAwarenessContext` from `auv`'s `main` and
again from `open_local`. That changed the process mode of every library host on its first driver
call, after the host may already have created windows; the manifest replaces both calls.

`PrintWindow` draws a window at its own DPI, so the bitmap is sized at
`GetDpiForWindow / GetDpiForMonitor` (`NOTICE(windows-printwindow-dpi)`). A DPI-unaware window
is captured at 96 DPI: the bounds are the physical frame, the image has the real, smaller pixel
size (no upscaling), and `Capture.scaleFactor` is below 1.

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

## Manifest revision (2026-11-04)

Same host and session. `auv.exe` built from the revised branch; `auv serve --no-register` on a
private named pipe in the console session (Interactive scheduled task), driven with
`auv invoke` through `AUV_ENDPOINT`. Awareness is
`GetAwarenessFromDpiAwarenessContext(GetDpiAwarenessContextForProcess)` read from outside the
process (0 unaware, 2 per-monitor).

| Check | Result |
| --- | --- |
| `mt.exe -inputresource:auv.exe;#1` | the `auv.exe.manifest` content (`asInvoker`, `true/pm`, `permonitorv2,permonitor`) |
| Fresh `auv.exe`, read 300 ms after start | 2; the Runner process: 2 |
| `auv.exe` with `__COMPAT_LAYER=DpiUnaware` | 0: a compatibility setting overrides the manifest |
| Example host without a manifest, two `open_local` calls | 0 before and after, one warning |
| `window.capture`, DPI-unaware form | bounds `1811,1050 587x438`, 392x292 px, scale 0.668 |
| `window.capture`, per-monitor-v2 form | bounds `309,300 586x447`, 586x447 px, scale 1 |
| `input.clickPoint` at 20% and 85%, default, `foreground-preferred`, `background-only`, both forms | exit 0, 12/12 hit the expected button |

Native tests: `cargo test -p auv-driver-common -p auv-driver-windows --no-fail-fast` in the
console session passes (all 7 `wgc_capture` tests, including
`test_wgc_pixel_correctness_and_gdi_comparison` and the issue #460 test, with the tests'
explicit opt-in).

## Not covered

- 100% and 200% scaling, and mixed scales across displays (the shared desktop's scale was not
  changed).
- Window mutation (`move_to`, `resize`, `set_frame`) at 150%. `SetWindowPos` now takes the same
  physical pixels the frames report; the invisible resize border between the outer rect and the
  visible frame remains, as at 100%.
- Captures and clicks of a Runner forced DPI-unaware by a compatibility setting (only its
  awareness was read; the driver prints the warning).
- `background-session-eval` with its new opt-in was built, not run.
- The overlay: positions are physical, art keeps 96 DPI sizes (`TODO(windows-overlay-dpi)`).
