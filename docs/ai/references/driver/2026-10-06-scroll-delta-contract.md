# Scroll Delta Contract and Chromium Wheel Probe

Date: 2026-10-06

## Status

The convention is accepted by the owner and **implemented** on branch
`feat/scroll-delta-contract` (see [Implementation](#implementation)). macOS is
live-validated through every frontend route. Windows is live-validated for the
CLI with known Chromium background limits. Linux is live-validated on GNOME
Wayland for both the CLI's portal route and the uinput backend, after fixes
found during validation. See [Open Items](#open-items) for the remaining
limits. This note records:

- the problem and the pre-change backend behavior
  ([Pre-Change State](#pre-change-state));
- reference designs (Playwright, CUA);
- live macOS probe evidence for Chrome and Electron;
- the convention, what landed, and the remaining validation.

It extends the
[background scroll policy design](../ops/2026-06-02-background-scroll-policy-design.md),
which defines delivery paths and the attempt ladder but not delta semantics.

## Problem

Scroll exists in every desktop driver through `WindowInput::scroll`. It is not
yet exposed through the Runner protocol, the Rust client, the invoke registry,
or the JS SDK. Before it crosses that wire, the meaning of `delta_y` must be
the same on every platform. After a protobuf field ships, a semantic change is
a breaking protocol change.

## Pre-Change State

Source at `67910252`, before this slice:

| Backend | How `delta_y` is used | Positive means |
| --- | --- | --- |
| macOS global HID and window-targeted wheel (`Pointer.swift`, `units: .pixel`) | Pixels, passed as `wheel1` | Content moves up (live-verified below) |
| Windows `SendInput` and posted `WM_MOUSEWHEEL` (`input.rs` `wheel_amount`, `background_input.rs`) | **Notches**: multiplied by `WHEEL_DELTA` (120) | Up (Win32 convention) |
| Linux uinput (`uinput.rs`) | Continuous units, 15 per notch; y is negated | Down (by reading the code; not live-verified here) |
| Linux RemoteDesktop portal (`portal/input.rs`) | Passed through to `notify_pointer_axis` | Not verified |

Consequence: NetEase uses `scroll_amount = 300.0`. On macOS this is 300
pixels. On Windows it would be 300 notches (36,000 wheel units).

## Reference Designs

Local sources: Playwright from repository `node_modules`
(`playwright-core@1.62.1`). CUA from a local checkout at `02fdd98a`
(2026-06-02).

- **Playwright** `mouse.wheel(deltaX, deltaY)`. Units are CSS pixels. The
  direction follows DOM `WheelEvent`: positive `deltaY` scrolls down and
  positive `deltaX` scrolls right. The method dispatches a wheel event at the
  current mouse position. The type docs say it "does not wait for the
  scrolling to finish". Semantic scrolling is a separate API
  (`locator.scrollIntoViewIfNeeded`).
- **CUA Python computer interface** (`libs/python/computer/computer/interface/base.py`):
  `scroll(x, y)` passes amounts to pynput, in wheel steps, positive is up.
  The agent adapters use the opposite convention:
  `cua_agent/loops/yutori.py` maps `direction="down"` to positive `scroll_y`
  pixels at "~100 pixels per scroll unit". The result is two units and two
  sign conventions in one codebase.
- **CUA driver** (`libs/cua-driver/swift/Sources/CuaDriverServer/Tools/ScrollTool.swift`):
  `scroll(pid, direction, amount, by: line|page)` sends arrow or
  PageUp/PageDown keys, with optional AX focus first. It does not use wheel
  events. Its comment says that wheel events posted per pid through
  `SLEventPostToPid` are "silently dropped" by Chromium. The probe below
  **did not reproduce** that claim on current Chrome and Electron.

## Live Probe (macOS)

Environment: macOS 26.3 (25D2125), arm64, Google Chrome 154.0.8037.93,
Electron 44.5.1. Natural scrolling was **on**, then **off** for the sign
check in finding 4. The harness is test-only and
not committed:

- a page that logs `wheel` events, `scrollTop` for an inner `overflow:auto`
  box and the document, and `document.visibilityState`;
- a scratch binary that calls `session.window().scroll(...)` with one
  candidate per run:
  - `[WindowTargetedWheel]` with `BackgroundOnly`;
  - `[ForegroundHid]` with `ForegroundPreferred`.

The receiver state was read directly over CDP `Runtime.evaluate`.
`agent-browser eval` returned stale values in this setup and was not used for
the readings.

| Target | Path | Target visibility | Result |
| --- | --- | --- | --- |
| Chrome, Electron (default flags) | window-targeted wheel | `visible`, not focused | Scrolls. `wheel1=+120` gives exactly 120 CSS px |
| Chrome, Electron (default flags) | window-targeted wheel | `hidden` (covered, or on another Space) | Driver reports `window_targeted_wheel`. The page receives the `wheel` event about 5–6 s later, and **nothing scrolls** |
| Chrome with `--disable-backgrounding-occluded-windows`; Electron with that switch plus `backgroundThrottling: false` | window-targeted wheel | covered, but the page stays `visible` | Scrolls within 60–75 ms (inner box and document) |
| Chrome (window topmost) | foreground HID | `visible` | Scrolls. Same direction and magnitude as window-targeted wheel |

Findings:

1. Window-targeted wheel posted per pid works on current Chrome and Electron
   when the page is visible. The events arrive with `isTrusted=true` and
   `deltaMode=0`.
2. The failure mode is **Chromium occlusion backgrounding**. A hidden
   renderer receives the wheel event late and does not scroll. Delivery
   metadata cannot see this, so scroll success needs post-action observation,
   as the background scroll design already requires.
3. With natural scrolling on, a synthetic `wheel1=+120` gives DOM
   `deltaY=-120` and the content moves up. This holds on both the HID-tap and
   per-pid paths, so natural scrolling does not invert synthetic events in
   this configuration.
4. Natural scrolling **off** gives the same result. The setting was switched
   live through the private `setSwipeScrollDirection` symbol in
   `PreferencePanesSupport.framework`, which System Settings also calls.
   `defaults read -g com.apple.swipescrolldirection` read `0` during the run,
   and the setting was restored to `1` afterwards. With the setting off,
   `wheel1=+120` still moved content up by 120 px in:
   - Chrome, window-targeted wheel;
   - Electron, window-targeted wheel;
   - Chrome, HID-tap.

   The sign of synthetic events therefore does not depend on the user's
   natural scrolling preference. Inversion applies only to physical device
   input. Limit: a physical-device check that the live toggle took effect was
   not part of this run.

Additional defect found in passing (separate slice): macOS
`ScrollDeliveryCandidate::ForegroundHid` does not activate the target window
before it posts a global wheel at the screen point
(`crates/auv-driver-macos/src/session.rs`, `scroll_impl`). If the target is
covered, the wheel goes to whatever window is on top. Windows activates the
window first (`foreground_window_attempt`).

## Accepted Convention

Use Playwright/DOM semantics for `Scroll`:

- Unit: logical pixels in the same space as `WindowPoint`:
  - macOS: points;
  - Windows: DPI-scaled pixels;
  - Wayland: logical pixels.
- Direction: `delta_y > 0` moves the viewport toward later content (down).
  `delta_x > 0` moves it right.
- Backends convert the value to their native unit:
  - macOS negates the value into `.pixel` wheel units.
  - Windows maps pixels to `WHEEL_DELTA` units. The pixel-per-notch factor
    needs a `NOTICE:` and live validation.
  - Linux uinput already appears to treat positive as down. The portal path
    needs live verification.
- NetEase call sites flip their sign (`scroll_up` currently passes
  `+scroll_amount`).

Out of scope for this primitive: discrete `direction + line|page` scrolling,
as in the CUA driver. It belongs to the reserved
`WindowTargetedKeyboardScroll` path or to a future semantic scroll operation.
It is not a second unit on `Scroll`.

## Implementation

Contract:

- `Scroll` in `crates/auv-driver-common/src/input.rs` documents the unit and
  direction.
- macOS negates both axes into CoreGraphics pixel wheel values
  (`core_graphics_wheel_pixels` in `crates/auv-driver-macos/src/session.rs`).
  This covers the window-targeted wheel and `scroll_global_hid`.
- Windows converts pixels to Win32 wheel units in one shared
  `wheel_units` function (`crates/auv-driver-windows/src/input.rs`). It
  applies 100 px per notch (120 units), negates the vertical axis, and rejects
  values outside the signed 16-bit wheel word. Both `SendInput` and posted
  `WM_MOUSEWHEEL`/`WM_MOUSEHWHEEL` use it.
- Linux shares one pure `wheel_notches` function (`crates/auv-driver-linux/src/native.rs`)
  between both backends. It maps 120 logical px to one wheel notch, positive
  toward later content, and carries the sub-notch remainder within one input
  session. `TODO(linux-hi-res-wheel)` defers high-resolution wheel output.
  - uinput emits `REL_HWHEEL` and the negated `REL_WHEEL`. Before this slice it
    assumed 15 px per notch, so 120 px became 8 notches.
  - The portal now sends `NotifyPointerAxisDiscrete` steps. Before, it sent the
    continuous `NotifyPointerAxis` followed by `finish`. That is a
    finger/touchpad axis: GNOME scaled it about 12x in Chromium, and `finish`
    started kinetic scrolling.
- NetEase flips every scroll call site and the recorded
  `SidebarScrolled.requested_delta`. Recorded deltas in new runs follow the
  new sign.

Wire:

- `InputService/ScrollWindowPoint` with `Scroll`, `ScrollOptions`, and
  `ScrollDeliveryCandidate` in `proto/auv/api/driver/v1/input.proto`.
  An empty candidate list selects the Driver default ladder. The Runner
  rejects:
  - non-finite or all-zero deltas;
  - unknown or repeated candidates;
  - points outside the freshly resolved window.
- Runner handler in `crates/auv-cli/src/runner/local_driver.rs`. It shares
  `require_point_inside_window` with `ClickWindowPoint`.
- Rust client: `WindowClient::scroll` returns `WindowPointScroll`
  (`crates/auv/src/client/runner.rs`).
- Invoke: `auv invoke input.scroll <x> <y> --dx --dy --target app:|window:`
  with the new `TargetPolicy::RequiredWindow`. Local and selected-Runner
  execution share one `ScrollPlan`, and MCP gets the command through the
  registry.
- JS SDK: `WindowClient.scroll(point, scroll, scrollOptions?)`.

Deliberately deferred, with `TODO` markers at the code site:

- scrolling without a target window. This landed on 2026-10-09 as
  `InputService/ScrollPoint` (`input.scroll`), which takes a screen or display
  `Position` and delivers in the foreground on Linux and Windows. macOS
  answers `UNIMPLEMENTED` (`TODO(macos-screen-scroll-point)`), and the
  `input.scrollPoint` CLI takes screen coordinates only
  (`TODO(scroll-point-display-cli)`);
- a CLI flag for ordered delivery candidates
  (`TODO(scroll-delivery-candidates-cli)`).

## Live Validation (macOS, 2026-10-06)

Same receiver page and environment as the probe above. The scrollers were
extended with horizontal overflow. Targets were Chrome and Electron with
occlusion backgrounding disabled, so background delivery is measurable while
covered. Each case ran `auv invoke input.scroll ... --json` and read
`scrollTop`/`scrollLeft` over CDP before and after.

| Route | Policy | Cases | Result |
| --- | --- | --- | --- |
| Local invoke | `background-only` | Chrome and Electron: `dy=±120`, `dx=±150` on the inner box; `dy=300` on the document | All exact: `+dy` moved `scrollTop` +N, `+dx` moved `scrollLeft` +N; path `window_targeted_wheel` |
| Local invoke | `foreground-preferred` (window raised first) | Chrome: `dy=±120`, `dx=±150` | All exact; path `foreground_system_events` |
| Selected Runner (`auv --device <local daemon> invoke`, isolated daemon from this branch) | `background-only`, `background-preferred` | Chrome `dy=+120`, `dx=-150`; Electron `dy=-120`, document `dy=300` | All exact; path `window_targeted_wheel` |
| Selected Runner | n/a | zero delta; point outside window | Rejected as `invalid_input` before delivery |

## Live Validation (Windows, 2026-10-06)

Host `luoling-windows-11`: Windows 11 `10.0.26100`, Microsoft Edge (Chromium)
with occlusion backgrounding disabled. The same probe page was used. The
branch was built natively, and `cargo test -p auv-driver-common
-p auv-driver-windows --lib` passed 55 + 95 tests. The new `wheel_units`
tests passed. One unrelated failure was
`device_unlock_host::installed_host_resolves_the_shipped_helper_beside_it`,
which returns `Unavailable` without an installed helper.

The cases ran inside the active console session through a scheduled task. SSH
service session 0 cannot inject input. The window target was the decimal
HWND, because the CLI `window.list` command is macOS/Linux only.

| Policy | Edge state | Cases | Result |
| --- | --- | --- | --- |
| `background-only` | foreground window | `dy=±100`, `dx=+100` | Exact; path `window_targeted_wheel` |
| `foreground-preferred` | n/a | `dy=±100`, `dx=±100`, `dy=300` on the inner box and on the document | Exact; path `foreground_system_events` |
| `background-only` | **not** focused (a Notepad window in front) | `dy=±100`, `dx=+100`, document `dy=300` | Driver reports `window_targeted_wheel`, but Edge received **no** wheel event and nothing scrolled |

The 100 px/notch factor therefore matches Chromium exactly. Posted
`WM_MOUSEWHEEL`/`WM_MOUSEHWHEEL` reaches Chromium only while its window is in
the foreground, as the `background_input.rs` header already warns. This
existing limit is not changed by this slice. It is another case where
delivery evidence is not proof of a scroll.

## Live Validation (Linux, 2026-10-06)

Host `neko-gpu-1`: Debian, GNOME Wayland on the physical display (`wayland-0`,
DP-3, 2560x1440, scale 1). Electron 44 ran fullscreen with
`--ozone-platform=wayland`, so window and screen coordinates match. The branch
builds with Rust 1.95. `cargo test -p auv-driver-common -p auv-driver-linux
--lib` passed 55 + 99 tests, including the new `wheel_steps` test.

The uinput backend is not selectable from the CLI. A scratch program used
`LocalDriver::new().with_linux_input_backend(Uinput)` and the same
`window().scroll` call that `input.scroll` uses.

uinput backend (scratch program):

| Build | Cases | Result |
| --- | --- | --- |
| Before the fix (15 px/notch) | `dy=+15` | One notch; Chromium `deltaY=120`, scrolled 120 px |
| Before the fix | `dy=±120` | 8 notches, `deltaY=±960` (overshoot) |
| After the fix (120 px/notch) | inner box `dy=±120`, `dy=360`, `dx=±120`; document `dy=240` | All exact |
| After the fix | `dy=60` in a fresh process | No event: the half-notch remainder is not carried across processes |

Portal route (`auv invoke input.scroll`, the CLI default). The portal frontend
was restarted with the GNOME environment. The branch binary was registered with
`auv doctor --portal-setup` and authorized once with
`auv doctor --portal-authorize`; the owner approved the GNOME dialog.

| Build | Cases | Result |
| --- | --- | --- |
| Before the fix (continuous axis + `finish`) | `dy=15` | First event `deltaY=180` (about 12x) |
| Before the fix | `dy=120` | First event 1440, then about 90 decaying kinetic events; 5742 px total, spilling into later cases |
| After the fix (discrete steps) | inner box `dy=±120`, `dy=360`, `dx=±120`; document `dy=240`; `background-preferred` falls back to foreground | All exact; path `foreground_system_events`; no kinetic events |
| After the fix | `dy=60` | No event (sub-notch remainder) |

Signs were correct on both axes before and after the fixes. Environment
details for reruns:

- A target on a non-active GNOME workspace received no pointer events.
- The rc headless Sway session cannot validate input. Its portal has no
  RemoteDesktop frontend, and headless Sway does not consume uinput.
- The `xdg-desktop-portal` user service keeps the environment it started
  with. After a GNOME login it must be restarted to expose GNOME's
  RemoteDesktop.
- Portal consent has a fixed 10 s response deadline.

## Open Items

- Linux sub-notch scrolls (below 120 px) accumulate only within one input
  session (`TODO(linux-hi-res-wheel)`).
- Windows posted background wheel reaches Chromium only while it is the
  foreground window.
- macOS foreground HID scroll does not activate the target first.
- Linux validation covered GNOME/Mutter only. KDE and wlroots portals were not
  tested.

Follow-up candidates, not part of this slice:

- Make macOS `ForegroundHid` scroll activate the target first. Without this,
  a foreground fallback scrolls the wrong window.
- Turn the Chrome/Electron scroll receiver into an `evals/auv-base` case, so
  the occlusion and sign findings have a repeatable gate.
- Live-validate Windows and Linux units and signs after the conversion change.
- A shared scroll verification consumer. Motion and boundary detection are
  currently NetEase-local.
