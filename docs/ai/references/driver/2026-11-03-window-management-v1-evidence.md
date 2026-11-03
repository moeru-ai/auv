# Window Management V1 Scenario Evidence

Date: 2026-11-03, re-run 2026-11-04 at the API PR head (see
[Re-run at the API PR head](#re-run-at-the-api-pr-head-2026-11-04)). Design:
[`2026-11-02-window-management-api-v1-design.md`](2026-11-02-window-management-api-v1-design.md).

Evidence level: **live probe** on one host per platform, through the JS SDK
against a daemon and Runner built from the working tree. This is not a support
claim. Each row is one run of one throwaway window in one app; other apps,
display layouts and scale factors are not covered.

## Source and method

- Source: branch `refactor/api-naming-normalization` at `539bd272` plus the
  uncommitted working tree (window management v1 slice 2), plus the three
  macOS fixes below. Linux and Windows were built from the tree before the
  macOS Swift fixes; those fixes touch only `auv-driver-macos`.
- Script: the Node ESM script at the end of this note. It is bundled with
  esbuild against `js/packages/sdk/src/node/index.ts`
  (`esbuild wm-scenario.mjs --bundle --platform=node --format=esm --alias:@auv-js/sdk/node=<repo>/js/packages/sdk/src/node/index.ts`)
  and run as `AUV_BIN=<tree>/target/debug/auv node wm-scenario.bundle.mjs`.
  It calls `startAuv({ noRegister: true })` with a private store, so it
  uses a private socket or pipe and does not touch a registered daemon.
- Fixtures: the script opens its own windows. macOS: TextEdit documents opened
  with `open -g` (the script refuses to run if TextEdit is already running).
  Windows: a PowerShell WinForms process with one button that fills each form.
  Linux: a Python GTK 3 process with the same layout. On Windows and Linux a
  click on the button appends ` clicked` to the window title, so the click
  check reads the effect through `refresh()`.
- Frame checks use a 2 px tolerance.

## Environment

| Host | OS | Display | Build | Runner session |
| --- | --- | --- | --- | --- |
| macOS (owner's Mac) | macOS 26.3 (25D2125) | 3 displays, all 2x; test window on a secondary 3008x1692 display at `0,1692` (no menu bar on it) | `cargo build -p auv-cli` (debug) | Terminal user session; Node 26.12.1 |
| `luoling-windows-11` | Windows 11 (NT 10.0.26100) | 3840x2160 at 150% | `cargo build -p auv-cli` (debug, rustup via scoop) | Console session 1 (user `luoling8192`) through a temporary Interactive scheduled task; Node 26.8.1 at `%USERPROFILE%\.proto\tools\node\26.8.1\node.exe` |
| `neko-gpu-1` | Debian 13, GNOME Shell 48.7 Wayland | 2560x1440 at 1x | `cargo +1.95 build -p auv-cli` (debug) | GNOME session over SSH env; Node 24.14.0 (`~/.volta/bin`); portal consent by `~/auv-xfix-portal-approve.py` |

## Fixes made during this run

1. **macOS state animation** (the requested step 0).
   `setState(NORMAL)` from minimized returned a mid-animation `after_frame`
   (`4430,2628 252x25`). `WindowApi::set_state` now polls the WindowServer
   frame after each state step until two reads 50 ms apart agree, for at most
   1 s (`wait_for_stable_window_frame`, `NOTICE(macos-window-state-animation)`
   in `crates/auv-driver-macos/src/session.rs`). This covers minimize, unminimize and zoom.
   After the fix, `unminimize()` reported `after_frame` `456,1883 620x440`,
   equal to the restore frame. No unit test: the helper is a native polling
   loop, not pure logic.
2. **macOS `MAXIMIZED` entered full screen.** (Superseded: the zoom-button
   path was removed by option A; see gap 1.) `AXPress` on the zoom button
   (green button) put TextEdit into full screen on its own Space
   (`is_fullscreen: true`, frame `0,1692 3008x1692`), and the scenario then
   lost the window (`NOT_FOUND`). The zoom button exposes `AXPress`,
   `AXZoomWindow` and `AXShowMenu`. `mutate_window` now performs
   `AXZoomWindow`, which toggles zoom without full screen. It returns
   `kAXErrorAttributeUnsupported` (-25205) even when it zooms, so that result
   is accepted and the Rust side verifies the state it reads back
   (`NOTICE(macos-zoom-window-action)` in `Window.swift`).
3. **macOS focus verification read a stale frontmost app.**
   `window.focus` activated TextEdit but failed with "did not receive focus".
   `NSWorkspace.frontmostApplication` is updated only when the main run loop
   runs. The Runner and CLI do not run it, so the value stayed at the app that was
   frontmost at the first read (reproduced with a small Swift program: still
   the terminal 3 s after `open -a TextEdit`; correct after
   `RunLoop.run(until:)`). `list_windows` now reads the system-wide AX focused
   application, with NSWorkspace as the fallback
   (`NOTICE(macos-frontmost-stale)` in `Window.swift`). After the fix,
   `auv invoke window.focus` and `setFocus()` pass.

Validation: `cargo build -p auv-cli`, `cargo test -p auv-driver-macos`
(133 passed), `cargo fmt --check -p auv-driver-macos`,
`scripts/generate-swift-bridge` and `swift build` in
`crates/auv-driver-macos/native/swift`.

## Results: macOS

Final run (after the three fixes), 17/19 passed.

| Step | Check | Observed | Result |
| --- | --- | --- | --- |
| 1 | `list()` has the window | 89 windows; id `78441`, pid 66301, `com.apple.TextEdit`, `376,1813 656x422`, not minimized | pass |
| 1 | `list({ excludeMinimized })` has it | 89 windows | pass |
| 1 | `resolve({ pid, titleContains })`, `resolve({ pid })` | both `78441` | pass |
| 2 | `setFrame` move and resize | target `406,1843 560x400`; `after_frame` and `refresh()` equal; `adjustedByApp: false`; path `AX_WINDOW_ATTRIBUTE` | pass |
| 2 | `setPosition` only | `456,1883 560x400` | pass |
| 2 | `setSize` only | `456,1883 620x440` | pass |
| 3 | `minimize()` | `after_state.is_minimized: true`; `focus_disturbance` NONE | pass |
| 3 | `list()` while minimized | `is_minimized: true`, frame `456,1883 620x440` (restore frame) | pass |
| 3 | `list({ excludeMinimized })` | window not listed | pass |
| 3 | `setFrame` on minimized | `FAILED_PRECONDITION` "set_frame needs a normal window, but the window is minimized: call set_state(Normal) first" | pass |
| 4 | `unminimize()` | `after_frame` `456,1883 620x440` = restore frame (fix 1) | pass |
| 4 | `maximize()` | window zoomed to `0,1722 3008x1662`, but `after_state.is_maximized: false` | **fail** (gap 1; fixed afterwards) |
| 4 | `unmaximize()` | no-op (0 attempts) because the state already read as normal; window stayed zoomed | **fail** (gap 1; fixed afterwards) |
| 4 | `setFocus()` | `after_state.is_focused: true`, `focus_disturbance` FOREGROUND, 2 attempts (fix 3) | pass |
| 5 | shown `a` + minimized `b`, `resolve({ pid })` | picks `a` (`78441`) | pass |
| 5 | only minimized `b` left | picks `b` (`78456`, `is_minimized: true`) | pass |
| 6 | `refresh()` after close | `NOT_FOUND` for `b` and `a` | pass |

Extra check for gap 1 (`auv invoke`, same build): after moving the window to
the main display (`200,200 600x400`), `window.setState maximized` zoomed it
to `0,30 3008x1662` with `is_maximized: true`, and `window.setState normal`
restored `200,200 600x400` with `is_maximized: false`.

## Results: Windows

Run with the Runner made DPI-aware (`startAuv({ environment: { __COMPAT_LAYER: 'HIGHDPIAWARE' } })`):
21/21 passed. A run with the default (DPI-unaware) Runner gave the same
values for every window step; only the click check failed (gap 2).

| Step | Check | Observed | Result |
| --- | --- | --- | --- |
| 1 | `list()` has the window | 24 windows; id `9373674`, `powershell.exe`, `461,300 1028x739`, focused | pass |
| 1 | `list({ excludeMinimized })`, `resolve({ pid, titleContains })`, `resolve({ pid })` | listed; both resolve to `9373674` | pass |
| 2 | `setFrame` move and resize | target `491,330 560x400`; `after_frame` and `refresh()` equal; path `PLATFORM_NATIVE` (3) | pass |
| 2 | `setPosition` only | target `541,370`, got `541,371 560x400` (1 px, within tolerance) | pass |
| 2 | `setSize` only | target `620x440`, got `620x441` | pass |
| 3 | `minimize()` | `is_minimized: true`; `focus_disturbance` FOREGROUND | pass |
| 3 | `list()` while minimized | `is_minimized: true`, frame `541,371 620x441` (restore frame, not `-32000`) | pass |
| 3 | `list({ excludeMinimized })` | not listed | pass |
| 3 | `setFrame` on minimized | `FAILED_PRECONDITION` "window.setFrame needs a normal window, but the window is minimized: call set_state(Normal) first" | pass |
| 4 | `unminimize()` | `after_frame` `541,371 620x441` = restore frame, focused | pass |
| 4 | `maximize()` | `is_maximized: true`, `0,0 3840x2088` (work area) | pass |
| 4 | `unmaximize()` | `is_maximized: false`, back to `541,371 620x441` | pass |
| 4 | `setFocus()` | `is_focused: true`, `focus_disturbance` NONE, 1 attempt | pass |
| N | `displays.capture()` + `captures.image(screenRegion: window frame)` | bounds `0,0 3840x2160`, pixel size 3840x2160, backend `xcap.windows`, scale 1; 200x142 crop with 426 distinct colors | pass |
| N | `click(window center)` (`input.clickPoint`, window space) | screen point `851,592`, path `WINDOW_TARGETED_MOUSE`; title became `… clicked` | pass |
| 5 | shown `a` + minimized `b`, `resolve({ pid })` | picks `a` | pass |
| 5 | only minimized `b` left | picks `b` (`is_minimized: true`) | pass |
| 6 | `refresh()` after close | `NOT_FOUND` for `b` and `a` | pass |

## Results: Linux

13/14 passed. Mutations are expected to be unsupported on Linux.

| Step | Check | Observed | Result |
| --- | --- | --- | --- |
| 1 | `list()` has the window | 2 windows; id `atspi::1.38/org/a11y/atspi/accessible/7`, app name `fixture.py`, `904,445 752x589`, `is_focused: true`, `is_minimized` unset | pass |
| 1 | `list({ excludeMinimized })`, `resolve({ appName, titleContains })`, `resolve({ appName })` | listed; both resolve to the window | pass |
| 2-4 | `setFrame`, `minimize`, `unminimize`, `maximize`, `setFocus` | each `UNIMPLEMENTED` (12): "window.setFrame is not supported by this driver; Linux support is not written yet; pull requests are welcome at https://github.com/moeru-ai/auv" (and the same for `window.setState` / `window.focus`) | pass |
| N | `displays.capture()` + `captures.image` | bounds `0,0 2560x1440`, backend `xdg-desktop-portal.screencast.pipewire`, 192x150 crop with 2881 distinct colors | pass |
| N | `click(window center)` | the click was delivered (title became `… clicked`), but the RPC failed `INTERNAL` (13): "driver returned invalid InputActionResult: successful input attempt must match selected_path" | **fail** (gap 3) |
| 5 | two shown windows, `resolve({ appName })` | picks `b`, the newer (active) window; Linux cannot minimize, so the minimized cases are not run | pass (partial) |
| 6 | `refresh()` after close | `NOT_FOUND` for both | pass |

## Re-run at the API PR head (2026-11-04)

Source: branch `feat/window-management-api` on `origin/main` `86eca391`
(driver layer #468, DPI manifest #465, focus preparation #466) plus this
slice. Same script (below), same fixtures, same hosts and displays. Each
host built its own debug `auv`. The Windows Runner was the default `auv.exe`
(per-monitor DPI aware through its manifest); no `__COMPAT_LAYER`.

| Host | Result | Values that differ from the first run |
| --- | --- | --- |
| macOS | 19/19 | `maximize()` on the lower display: `0,1722 3008x1662`, `is_maximized: true` (gap 1 fix); `unmaximize()` back to `456,1883 620x440` in 1 attempt. `setFocus()`: focus disturbance FOREGROUND, 2 attempts |
| Windows | 21/21 | Same frames as the first run (`491,330 560x400`; `541,371 620x441`; maximized `0,0 3840x2088`). `click(center)` default policy: `WINDOW_TARGETED_MOUSE` only, title `… clicked` (gap 2 fixed by #465). `setFocus()`: focus disturbance NONE, 1 attempt |
| Linux | 14/14 | Every mutation `UNIMPLEMENTED` (12) with the PR-welcome message. `click(center)`: OK, attempts `WINDOW_TARGETED_MOUSE:false` then `FOREGROUND_SYSTEM_EVENTS:true`, title `… clicked` (gap 3 fixed by #466) |

Eval cases in `evals/auv-base` (same day, same hosts):

- macOS `just eval appkit-window` (dedicated AppKit window,
  `test-objects/appkit/window.swift`): pass. `set_frame` `240,1170 520x360`
  exact (`AX_WINDOW_ATTRIBUTE`); minimized `set_frame` `FailedPrecondition`;
  unminimize and unmaximize back to `240,1170 520x360`; maximized
  `0,30 3008x1662`; focus `is_focused: true`, disturbance FOREGROUND.
- Windows `background-session-eval window-management` (`win32-receiver`,
  console session 1): pass. `set_frame` `3469,1800 520x360` exact
  (`PLATFORM_NATIVE`); minimized `set_frame` `FailedPrecondition`; restores to
  `3469,1800 520x360`; maximized `0,0 3840x2088`; focus `is_focused: true`.
- Linux `just eval linux-window` (GNOME 48): pass; `set_frame`, `set_state`
  and `focus` return `Unsupported` with the PR-welcome recovery, and the
  window frame is unchanged.

## Gaps

1. **macOS maximize: fixed by option A** (owner decision 2026-11-03). Before:
   the green-button zoom (`AXZoomWindow`) zoomed TextEdit on the secondary
   display to `0,1722 3008x1662`, which did not match that display's
   `visibleFrame` (`0,1692 3008x1692`). The window read as not maximized and
   `unmaximize()` did nothing. Now `MAXIMIZED` writes the `visibleFrame` of
   the window's display (the display under the window's center, else the
   largest overlap) through the `SetWindowFrame` AX path, after recording the
   frame in Runner memory. `NORMAL` writes the recorded frame back. The
   display list also no longer uses `NSScreen.main` as the Cocoa-to-CG
   reference; Rust converts with the primary display's height.
   Re-run with `auv invoke` against a private daemon (`--no-register`), all
   calls in one Run (`auv --run <id> invoke`) so they share one Runner;
   TextEdit opened with `open -g`, then quit:

   | Display (CG) | `visibleFrame` | Before | `maximized` | `normal` |
   | --- | --- | --- | --- | --- |
   | main `0,0 3008x1692` (menu bar) | `0,30 3008x1662` | `200,200 600x400` | `0,30 3008x1662`, 1 pass, `is_maximized: true` | `200,200 600x400`, `is_maximized: false` |
   | lower `0,1692 3008x1692` (no menu bar) | `0,1692 3008x1692` | `400,1900 600x400` | `0,1722 3008x1662`, 6 passes, `adjusted_by_app: true`, `is_maximized: true` | `400,1900 600x400` |
   | portrait right `3008,894 1692x3008` (Dock right) | `3008,894 1646x3008` | `3200,1500 600x400` | `3008,924 1645x2978`, `adjusted_by_app: true`, `is_maximized: true` | `3200,1500 600x400` |
   | main, from minimized | `0,30 3008x1662` | `300,300 600x400`, minimized | unminimize, then `0,30 3008x1662`, `is_maximized: true` | `300,300 600x400` |

   A second `maximized` on the lower display made no attempts. TextEdit
   (AppKit) keeps the window 30 pt below the top of every display, even one
   without a menu bar, so an AX write there settles at the same frame as the
   zoom. The driver therefore also counts the frame an AUV maximize settled at
   as maximized (`NOTICE(macos-window-maximized)` in `session.rs`). Still
   open: a window maximized by hand or before a Runner restart has no record,
   so `normal` leaves its frame and says so in the attempt message (unit
   test only, not observed live); each `auv invoke` without `--run` gets a
   new Runner, so maximize and restore in separate plain invokes do not pair
   up; `SetWindowFrame` now moves a maximized macOS window instead of
   refusing it.
2. **Windows click from a DPI-unaware Runner misses at 150%.** With the
   default Runner, both `WINDOW_TARGETED_MOUSE` and `FOREGROUND_PREFERRED`
   (`FOREGROUND_SYSTEM_EVENTS`) clicks at screen point `851,592` reported success, but
   the button did not receive them. With `__COMPAT_LAYER=HIGHDPIAWARE` the
   same click worked. The window frames from `list`/`setFrame` were identical
   in both runs, so window management is not affected. This is the same root
   cause class as issue #460 (Runner DPI awareness). Fixed afterwards; see
   [DPI fix](#dpi-fix-windows). The display capture reports `scaleFactor: 1`
   because Windows capture bounds are physical pixels; the display's 150% is
   `Display.scaleFactor` (1.5).
3. **Linux `window.click` returns an invalid `InputActionResult` after a
   successful AT-SPI focus.** `click_impl` in
   `crates/auv-driver-linux/src/session.rs` splices the successful `AxFocus`
   attempt in front of the portal attempts. `InputActionResult::validate`
   rejects any successful attempt whose path is not `selected_path`. The
   click is delivered, but the caller gets `INTERNAL`. This code is not part
   of this slice (it dates from #159/#218). Fixed after this run; see
   [Linux click fix](#linux-click-fix).
4. **Linux windows have no `process_id`** (`atspi.rs` sets `None`), so
   `resolve({ pid })` cannot be used. The script selects by `appName` on
   Linux. This is unchanged by design.
5. Not covered: windows on other desktops (`exclude_other_desktops`),
   full-screen windows, apps that limit their size (`adjusted_by_app: true`),
   multi-scale moves on Windows, and `WatchWindows` (not implemented).

## Linux click fix

- Root cause: `InputActionResult.attempts` lists delivery attempts, and
  `validate` allows a successful attempt only on `selected_path`. The AT-SPI
  window focus before the portal click is input preparation ("Prepare For
  Input Options" in `docs/TERMS_AND_CONCEPTS.md`), not a delivery attempt.
  The Linux driver still recorded it as a successful `AxFocus` attempt.
- Fix: `record_focus_preparation` in `crates/auv-driver-linux/src/session.rs`
  (used by `window.click` and `window.drag`) keeps only failed focus steps as
  `AxFocus` attempts. A successful focus shows only as a `foreground` focus
  disturbance. The shared validator is unchanged.
  `TODO(input-preparation-record)` marks the missing preparation field.
- Test: `focused_foreground_click_result_passes_input_contract_validation`
  in `crates/auv-driver-linux/src/session_test.rs`. It failed with
  "successful input attempt must match selected_path" before the fix.
- Live (`neko-gpu-1`, own debug build, private store, `noRegister`): SDK
  `window.click` at the center of a GTK 3 button window returned `OK`,
  `selectedPath` `FOREGROUND_SYSTEM_EVENTS`, attempts
  `WINDOW_TARGETED_MOUSE:false` (fallback reason) then
  `FOREGROUND_SYSTEM_EVENTS:true`, focus disturbance `FOREGROUND`. The title
  became `… clicked`. The build directory, private store and its portal
  tokens were removed. `auv.service` and the shared portal tokens were not
  touched.
- macOS does not record activation as an attempt. Windows recorded its
  foreground activation as a successful `FOREGROUND_SYSTEM_EVENTS` attempt.
  That passed validation because it is the same path as the `SendInput`
  delivery, but it is also preparation, not delivery. Fixed: see
  [Maximized set_frame and activation attempts](#maximized-set_frame-and-activation-attempts-windows).

## DPI fix (Windows)

Date: 2026-11-03, same host (luoling-windows-11, one 3840x2160 display at
150%), same tree plus the DPI fix. Evidence level: live probe, plus native
tests. The default Runner was used (no `__COMPAT_LAYER`).

Root cause: `auv` ran DPI-unaware. Windows then hands that process
virtualized (logical) values from `GetWindowRect`, `GetSystemMetrics`,
`ScreenToClient` and the `SendInput` absolute axis, but the DWM frame bounds
stay physical. Window frames were physical (`window.rs` switched its thread),
input and the `PrintWindow` bitmap were logical.

Fix: `auv_driver_windows::dpi::set_process_per_monitor_aware()` runs first in
`auv`'s `main` (and in `WindowsDriver::open_local` for library hosts), so the
whole driver uses physical pixels. The per-thread switch in `window.rs` and
`mutation.rs` is removed. See `NOTICE(windows-dpi-awareness)` for the
manifest trade-off. `PrintWindow` draws a window at its own DPI, so the
bitmap is sized at `GetDpiForWindow / GetDpiForMonitor`
(`NOTICE(windows-printwindow-dpi)`): a DPI-unaware window is captured at 96
DPI, and `Capture.scaleFactor` is below 1.

Probe: a PowerShell WinForms window (DPI-unaware, logical 1200,700 400x300,
one blue button that appends ` clicked` to the title), through the JS SDK.

| Check | Before (default Runner) | After |
| --- | --- | --- |
| `window.capture()` | frame `1811,1050 578x439`; bounds `1200,700 400x300` (logical), 400x300 px | bounds `1811,1050 578x439`, 386x293 px, scale 0.668, 85% button blue |
| same, bitmap at physical size (intermediate build) | - | 578x439 px, 56% black: `PrintWindow` drew 400x300 |
| `displays.capture()` | bounds `0,0 3840x2160`, 3840x2160 px, capture scale 1, display scale 1.5 | same |
| `click(center)`, default (`WINDOW_TARGETED_MOUSE`) | screen point `2100,1270`, OK, title unchanged (missed) | OK, title `… clicked` |
| `click(center)`, `FOREGROUND_PREFERRED` | OK, missed | OK, title `… clicked clicked` |
| window-management scenario above | 20/21 in the run above (click missed) | 21/21 |

Native tests (`cargo test -p auv-driver-common -p auv-driver-windows
--no-fail-fast`, in the console session): all pass, 10 + 4 ignored.
`wgc_capture::issue_460_window_capture_covers_visible_frame_on_scaled_display`
failed before the fix (bounds `311,300 369x260`, DWM frame `311,300 698x529`).
The earlier failure of
`wgc_capture::test_wgc_pixel_correctness_and_gdi_comparison` (WGC vs GDI diff
8.64%) had the same cause and now passes.

Not covered: 100% and 200% scaling (the display scale was not changed on a
shared desktop), Notepad (a user's Notepad was open, so it was not touched),
mixed scales across displays, a DPI-unaware Runner forced by a manifest or
compatibility setting (`auv` then prints a warning), and the overlay
(`TODO(windows-overlay-dpi)`: positions are physical, art keeps 96 DPI sizes).

## Maximized set_frame and activation attempts (Windows)

Date: 2026-11-03, same host and display, same tree plus these two changes.
Evidence level: live probe, plus native tests. Build: `cargo build -p
auv-cli` (debug); the JS SDK script ran in console session 1 through a
temporary Interactive scheduled task. Fixture: two DPI-unaware WinForms
windows (`a`, `b`); `a` logs its own bounds on every move and resize.

Changes:

- `set_frame` on a maximized window (owner decision, see the design doc's
  Decided list). `SetWindowPlacement` with `SW_SHOWNOACTIVATE` sets
  `rcNormalPosition` to the requested outer rect in workspace coordinates
  (`workspace_offset` in `crates/auv-driver-windows/src/window.rs`). Then, as
  soon as the window reads normal but is not at the request, one `SetWindowPos`
  uses the border read from the normal window. A minimized window is still
  refused.
- Activation before a foreground click, scroll or drag is no longer a
  successful `FOREGROUND_SYSTEM_EVENTS` attempt. It shows only as focus
  disturbance `FOREGROUND`. A failed activation stays a failed attempt
  (`record_activation` in `crates/auv-driver-windows/src/session.rs`, the
  Linux rule).

| Check | Result |
| --- | --- |
| maximized `a` (visible `0,0 3840x2088`), `b` focused, `setFrame(600,400 900x650)` | OK: `after` `601,401 901x651`, `isMaximized false`, `adjustedByApp false`, focus disturbance `NONE`, `b` still focused |
| same, then `setFrame({ origin: 500,300 })` | OK: `500,300 901x651` (size from the restore placement), `NONE`, `b` focused |
| minimized `a`, `setFrame` | `FAILED_PRECONDITION` (9) |
| `b` focused, `a.click(center, FOREGROUND_PREFERRED)` | OK, title `… clicked`, attempts `FOREGROUND_SYSTEM_EVENTS:true` only, focus disturbance `FOREGROUND` |

Findings:

- The overhang of a maximized window is not its normal invisible border. At
  150%, a per-monitor aware WinForms window hung 11 px past every side but
  had 9 px (left, right, bottom, none on top) once normal; the DPI-unaware
  fixture had 12 px and 11 px. The first placement guesses the overhang
  without a top border, and the correction fixes the rest.
- WinForms puts a form back at its own remembered restore bounds when it
  leaves maximized. The bounds log showed the requested frame, then 4 ms
  later the old `300,200 700x500` (logical), then the request again after the
  correction. With the correction issued only after the full settle poll,
  the old frame stayed for about 360 ms. It now stays for 17-20 ms. That jump
  comes from the app, not from the placement. The 1 px offset in the first row
  comes from 1.5x rounding of a DPI-unaware window (within 2 px).
- Native tests (`cargo test -p auv-driver-common -p auv-driver-windows
  --no-fail-fast`, console session): all pass, 10 + 4 ignored. New unit
  tests: `workspace_coordinates_are_offset_by_the_work_area`,
  `set_frame_allows_a_maximized_window_and_refuses_a_minimized_one`,
  `successful_activation_is_a_focus_disturbance_not_a_delivery_attempt`.

Not covered: a top or left taskbar (the work area here starts at `0,0`, so
the workspace offset was 0 live), multiple displays, and non-WinForms apps.

## Cleanup

- 2026-11-04 re-run: TextEdit quit (not running afterwards). Windows: task
  `auv-wmapi-scenario` unregistered, `%USERPROFILE%\auv-pr468` (source,
  target, logs) removed, the user's Notepad not touched. Linux: `~/auv-wmapi`
  (source, target, private store) removed, no fixture or approve helper left,
  `auv.service` active and not touched.

- macOS: TextEdit quit after every run; scratch files stay only in the
  session scratchpad.
- Windows: the scheduled task `auv-wm-v1-scenario` is unregistered, no
  process with `auv-wm-v1` in its command line is left, and
  `%USERPROFILE%\auv-wm-v1` is removed.
- Windows DPI fix: the scheduled task `auv-dpi-fix` is unregistered, no
  process with `auv-dpi` in its command line is left, and
  `%USERPROFILE%\auv-dpi` (both source trees, probe logs) is removed.
- Windows maximized set_frame: the scheduled task `auv-maxframe` is
  unregistered, no fixture or `auv` process from it is left, and
  `%USERPROFILE%\auv-maxframe` (source tree, target, logs) is removed. The
  user's Notepad was not touched.
- Linux: `~/auv-wm-v1` (source, target, private store with its own portal
  restore tokens) is removed, and no fixture or approve helper process is
  left. The shared `~/.local/state/auv/portal` tokens (dated 2026-10-21) and
  `auv.service` (active) were not touched.

## Scenario script

```js
// Window management v1 scenario (2026-11-03). Bundle with esbuild against
// js/packages/sdk and run with AUV_BIN=<path to this tree's auv binary>.
// Opens its own throwaway windows, records every returned state as JSON lines.
import { spawn, spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { setTimeout as sleep } from 'node:timers/promises'

import { createAuv, startAuv } from '@auv-js/sdk/node'

const platform = process.platform
const work = mkdtempSync(join(process.env.WM_WORK ?? tmpdir(), 'auv-wm-'))
const tag = Math.random().toString(36).slice(2, 8)
const results = []
const fmt = f => (f ? `${f.x},${f.y} ${f.width}x${f.height}` : 'none')
const st = s => (s ? `min=${s.isMinimized} max=${s.isMaximized} focus=${s.isFocused}` : 'none')
const close = (a, b, tol = 2) => a && b && ['x', 'y', 'width', 'height'].every(k => Math.abs(a[k] - b[k]) <= tol)
function record(step, check, ok, observed) {
  const row = { step, check, ok: Boolean(ok), observed }
  results.push(row)
  console.log(JSON.stringify(row))
}
async function rpcCode(promise) {
  try {
    await promise
    return { code: 'OK' }
  }
  catch (error) {
    return { code: error.rpcCode ?? error.name, message: String(error.message).slice(0, 200) }
  }
}

// Each fixture opens windows named `a` / `b` in one process and closes them on request.
function fixture() {
  if (platform === 'darwin') {
    const running = spawnSync('pgrep', ['-x', 'TextEdit']).status === 0
    if (running)
      throw new Error('TextEdit is already running; quit it first so the scenario owns every TextEdit window')
    const title = name => `auv-wm-${name}-${tag}.txt`
    return {
      title,
      open(name) {
        const file = join(work, title(name))
        writeFileSync(file, `auv window management ${name}\n`)
        spawnSync('open', ['-g', '-a', 'TextEdit', file])
      },
      close(name) {
        spawnSync('osascript', ['-e', `tell application "TextEdit" to close (every document whose name is "${title(name)}") saving no`])
      },
      cleanup() {
        spawnSync('osascript', ['-e', 'tell application "TextEdit" to quit saving no'])
      },
    }
  }
  const control = join(work, 'control')
  mkdirSync(control)
  let child
  const send = command => writeFileSync(join(control, `cmd-${command}`), '')
  const title = name => `auv-wm-${name}-${tag}`
  if (platform === 'win32') {
    const script = join(work, 'fixture.ps1')
    writeFileSync(script, `param($Dir, $Tag)
Add-Type -AssemblyName System.Windows.Forms
$forms = @{}
function New-Fixture($name, $x) {
  $f = New-Object System.Windows.Forms.Form
  $f.Text = "auv-wm-$name-$Tag"; $f.StartPosition = 'Manual'; $f.Left = $x; $f.Top = 200; $f.Width = 700; $f.Height = 500
  $b = New-Object System.Windows.Forms.Button
  $b.Dock = 'Fill'; $b.Text = "click $name"
  $b.Add_Click({ $f.Text = $f.Text + ' clicked' }.GetNewClosure())
  $f.Controls.Add($b); $f.Show(); $forms[$name] = $f
}
New-Fixture 'a' 300
$t = New-Object System.Windows.Forms.Timer; $t.Interval = 150
$t.Add_Tick({
  foreach ($c in Get-ChildItem $Dir -Filter 'cmd-*') {
    $parts = $c.Name.Substring(4).Split('-'); Remove-Item $c.FullName
    if ($parts[0] -eq 'open') { New-Fixture $parts[1] 1100 }
    elseif ($parts[0] -eq 'close') { $forms[$parts[1]].Close() }
    elseif ($parts[0] -eq 'exit') { [System.Windows.Forms.Application]::Exit() }
  }
})
$t.Start()
[System.Windows.Forms.Application]::Run()
`)
    child = spawn('powershell.exe', ['-NoProfile', '-STA', '-ExecutionPolicy', 'Bypass', '-File', script, '-Dir', control, '-Tag', tag], { stdio: 'ignore' })
  }
  else {
    const script = join(work, 'fixture.py')
    writeFileSync(script, `import os, sys, gi
gi.require_version('Gtk', '3.0')
from gi.repository import GLib, Gtk
control, tag = sys.argv[1], sys.argv[2]
windows = {}
def new(name):
    w = Gtk.Window(title=f'auv-wm-{name}-{tag}')
    w.set_default_size(700, 500)
    b = Gtk.Button(label=f'click {name}')
    b.connect('clicked', lambda *_: w.set_title(w.get_title() + ' clicked'))
    w.add(b)
    w.show_all()
    windows[name] = w
def tick():
    for f in sorted(os.listdir(control)):
        if not f.startswith('cmd-'):
            continue
        os.remove(os.path.join(control, f))
        verb, _, name = f[4:].partition('-')
        if verb == 'open':
            new(name)
        elif verb == 'close':
            windows.pop(name).destroy()
        elif verb == 'exit':
            Gtk.main_quit()
    return True
new('a')
GLib.timeout_add(150, tick)
Gtk.main()
`)
    child = spawn('python3', [script, control, tag], { stdio: 'ignore' })
  }
  let opened = false
  return {
    title,
    open(name) {
      if (opened || name !== 'a')
        send(`open-${name}`)
      opened = true
    },
    close: name => send(`close-${name}`),
    cleanup() {
      send('exit')
      setTimeout(() => child.kill(), 1500).unref()
    },
  }
}

const fx = fixture()
const daemon = await startAuv({ binaryPath: process.env.AUV_BIN, noRegister: true, storeRoot: join(work, 'store'), workingDirectory: work, startupTimeoutMs: 30_000, ...(process.env.WM_COMPAT_LAYER ? { environment: { __COMPAT_LAYER: process.env.WM_COMPAT_LAYER } } : {}) })
const connection = await daemon.connect()
const runner = createAuv(connection).runner({ runnerClass: 'auv.core.local' })
const linux = platform === 'linux'

async function waitWindow(name, predicate = () => true) {
  for (let i = 0; i < 50; i++) {
    const found = (await runner.windows.list()).find(w => w.window.title?.startsWith(fx.title(name)) && predicate(w.window))
    if (found)
      return found
    await sleep(200)
  }
  throw new Error(`window ${fx.title(name)} did not appear`)
}

try {
  fx.open('a')
  // Step 1: listing and resolve.
  let a = await waitWindow('a')
  const pid = a.window.processId
  // NOTICE: Linux AT-SPI windows carry no process id; select by app name there.
  const app = pid !== undefined ? { pid } : { appName: a.window.applicationName }
  const all = await runner.windows.list()
  record(1, 'list() has test window', true, { count: all.length, id: a.id, pid, app: a.window.applicationId ?? a.window.applicationName, frame: fmt(a.window.frame), state: st(a.window.state) })
  const shown = await runner.windows.list({ excludeMinimized: true })
  record(1, 'list({excludeMinimized}) has it', shown.some(w => w.id === a.id), { count: shown.length })
  const byTitle = await runner.windows.resolve({ ...app, titleContains: fx.title('a') })
  record(1, 'resolve({app,titleContains})', byTitle.id === a.id, { id: byTitle.id })
  const main = await runner.windows.resolve(app)
  record(1, 'resolve({app}) main_visible', main.id === a.id, { id: main.id })

  // Step 2: frame mutations.
  const f0 = (await a.refresh()).frame
  const target = { x: f0.x + 30, y: f0.y + 30, width: 560, height: 400 }
  if (linux) {
    const r = await rpcCode(a.setFrame({ origin: { x: target.x, y: target.y }, size: { width: 560, height: 400 } }))
    record(2, 'setFrame UNIMPLEMENTED (12) + PR welcome', r.code === 12 && /pull requests are welcome/.test(r.message), r)
  }
  else {
    let r = await a.setFrame({ origin: { x: target.x, y: target.y }, size: { width: target.width, height: target.height } })
    let now = (await a.refresh()).frame
    record(2, 'setFrame move+resize', close(r.afterFrame, target) && close(now, target), { before: fmt(r.beforeFrame), target: fmt(target), after: fmt(r.afterFrame), refresh: fmt(now), adjustedByApp: r.adjustedByApp, path: r.selectedPath })
    const moved = { ...now, x: now.x + 50, y: now.y + 40 }
    r = await a.setPosition({ x: moved.x, y: moved.y })
    now = (await a.refresh()).frame
    record(2, 'setPosition only', close(r.afterFrame, moved) && close(now, moved), { target: fmt(moved), after: fmt(r.afterFrame), refresh: fmt(now) })
    const sized = { ...now, width: 620, height: 440 }
    r = await a.setSize({ width: 620, height: 440 })
    now = (await a.refresh()).frame
    record(2, 'setSize only', close(r.afterFrame, sized) && close(now, sized), { target: fmt(sized), after: fmt(r.afterFrame), refresh: fmt(now), adjustedByApp: r.adjustedByApp })
  }

  // Step 3: minimize.
  const restoreFrame = (await a.refresh()).frame
  if (linux) {
    const r = await rpcCode(a.minimize())
    record(3, 'minimize UNIMPLEMENTED (12) + PR welcome', r.code === 12 && /pull requests are welcome/.test(r.message), r)
  }
  else {
    const r = await a.minimize()
    record(3, 'minimize()', r.afterState?.isMinimized === true, { after: st(r.afterState), afterFrame: fmt(r.afterFrame), focusDisturbance: r.focusDisturbance })
    const listed = (await runner.windows.list()).find(w => w.id === a.id)
    record(3, 'list shows isMinimized + restore frame', listed?.window.state?.isMinimized === true && close(listed.window.frame, restoreFrame), { frame: fmt(listed?.window.frame), restoreFrame: fmt(restoreFrame), state: st(listed?.window.state) })
    const hidden = (await runner.windows.list({ excludeMinimized: true })).some(w => w.id === a.id)
    record(3, 'excludeMinimized drops it', !hidden, { listed: hidden })
    const fp = await rpcCode(a.setFrame({ origin: { x: restoreFrame.x + 10, y: restoreFrame.y } }))
    record(3, 'setFrame on minimized FAILED_PRECONDITION (9)', fp.code === 9, fp)
  }

  // Step 4: unminimize, maximize, unmaximize, focus.
  if (linux) {
    for (const [name, call] of [['unminimize', () => a.unminimize()], ['maximize', () => a.maximize()], ['setFocus', () => a.setFocus()]]) {
      const r = await rpcCode(call())
      record(4, `${name} UNIMPLEMENTED (12) + PR welcome`, r.code === 12 && /pull requests are welcome/.test(r.message), r)
    }
  }
  else {
    let r = await a.unminimize()
    let now = (await a.refresh()).frame
    record(4, 'unminimize() after_frame = restore frame', r.afterState?.isMinimized === false && close(r.afterFrame, restoreFrame) && close(now, restoreFrame), { after: st(r.afterState), afterFrame: fmt(r.afterFrame), refresh: fmt(now), restoreFrame: fmt(restoreFrame) })
    r = await a.maximize()
    record(4, 'maximize()', r.afterState?.isMaximized === true, { after: st(r.afterState), afterFrame: fmt(r.afterFrame) })
    r = await a.unmaximize()
    now = (await a.refresh()).frame
    record(4, 'unmaximize() back to normal frame', r.afterState?.isMaximized === false && close(now, restoreFrame), { after: st(r.afterState), afterFrame: fmt(r.afterFrame), refresh: fmt(now), attempts: r.attempts.length })
    if (!close(now, restoreFrame)) {
      // Not a check: put the window back so the later steps start from a normal frame.
      const fix = await rpcCode(a.setFrame({ origin: { x: restoreFrame.x, y: restoreFrame.y }, size: { width: restoreFrame.width, height: restoreFrame.height } }))
      console.log(JSON.stringify({ recovery: 'setFrame(restoreFrame)', ...fix }))
    }
    r = await a.setFocus()
    record(4, 'setFocus()', r.afterState?.isFocused === true, { after: st(r.afterState), focusDisturbance: r.focusDisturbance, attempts: r.attempts.length })
  }

  // Naming checks: display capture and clickPoint into the fixture (Windows and Linux).
  if (platform !== 'darwin') {
    a = await runner.windows.get(a)
    const frame = a.window.frame
    const shot = await runner.displays.capture()
    const image = await runner.captures.image(shot.capture, { screenRegion: frame, maxSize: { width: 200, height: 150 } })
    const colors = new Set()
    for (let i = 0; i < image.data.length; i += 4)
      colors.add(`${image.data[i]},${image.data[i + 1]},${image.data[i + 2]}`)
    record('N', 'displays.capture + captures.image(window region)', image.width > 0 && colors.size > 1, { display: shot.display?.ref?.displayId ?? shot.display?.id, bounds: fmt(shot.capture.bounds), pixelSize: shot.capture.pixelSize, scale: shot.capture.scaleFactor, backend: shot.capture.backend, crop: `${image.width}x${image.height}`, distinctColors: colors.size })
    const center = { x: Math.round(frame.width / 2), y: Math.round(frame.height / 2) }
    for (const [label, clickOptions] of [['default policy', undefined], ['FOREGROUND_PREFERRED', { policy: 3 }]]) {
      let click
      try {
        const r = await a.click(center, clickOptions)
        const result = r.action ?? {}
        click = { code: 'OK', screenPoint: r.screenPoint && `${r.screenPoint.x},${r.screenPoint.y}`, selectedPath: result.selectedPath, attempts: result.attempts?.map(x => `${x.path}:${x.succeeded}${x.message ? `:${x.message.slice(0, 80)}` : ''}`) }
      }
      catch (error) {
        click = { code: error.rpcCode ?? error.name, message: String(error.message).slice(0, 200) }
      }
      await sleep(500)
      const title = (await a.refresh()).title
      record('N', `click(window center, ${label}) changes title`, click.code === 'OK' && title.endsWith('clicked'), { center, click, title })
      if (title.endsWith('clicked'))
        break
    }
  }

  // Step 5: one shown + one minimized window, then only the minimized one.
  fx.open('b')
  const b = await waitWindow('b')
  if (linux) {
    const pick = await runner.windows.resolve(app)
    record(5, 'resolve({app}) with two shown windows (no minimize on Linux)', [a.id, b.id].includes(pick.id), { picked: pick.id === a.id ? 'a' : 'b', a: a.id, b: b.id })
  }
  else {
    await b.minimize()
    let pick = await runner.windows.resolve(app)
    record(5, 'resolve picks shown a over minimized b', pick.id === a.id, { picked: pick.id, a: a.id, b: b.id, pickedState: st(pick.window.state) })
    fx.close('a')
    await sleep(1500)
    pick = await runner.windows.resolve(app)
    record(5, 'only minimized b left: resolve picks b', pick.id === b.id, { picked: pick.id, pickedState: st(pick.window.state) })
  }

  // Step 6: close and refresh.
  if (linux)
    fx.close('a')
  fx.close('b')
  await sleep(1500)
  const gone = await rpcCode(b.refresh())
  record(6, 'refresh() of closed window NOT_FOUND (5)', gone.code === 5, gone)
  const goneA = await rpcCode(a.refresh())
  record(6, 'refresh() of closed a NOT_FOUND (5)', goneA.code === 5, goneA)
}
catch (error) {
  record('X', 'scenario aborted', false, { error: String(error?.stack ?? error).slice(0, 600) })
}
finally {
  try { fx.cleanup() } catch {}
  await connection.close?.()
  await daemon.stop()
  const failed = results.filter(r => !r.ok)
  console.log(`SUMMARY ${platform}: ${results.length - failed.length}/${results.length} passed`)
  if (process.env.WM_KEEP !== '1')
    rmSync(work, { recursive: true, force: true })
}
```
