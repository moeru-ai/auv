# Pointer Position, Point Scroll and Typed Key Holds

Status: implemented in PR #257. Owner direction from 2026-10-07; this note
records the accepted shape, the deferred parts and the current evidence.

## Pointer, logical mouse and cursor

AUV keeps three positions apart. The shared definition is
"Pointer, Logical Mouse and Cursor" in
[`TERMS_AND_CONCEPTS.md`](../../../TERMS_AND_CONCEPTS.md#pointer-logical-mouse-and-cursor).

- **Pointer**: the OS pointer. Human input and other processes move it.
- **Logical mouse**: AUV's own position and held-button state per mouse
  identity, kept by `MouseCoordinator`. Mouse zero is a logical mouse.
- **Cursor**: a visual overlay layer, such as the AUV cursor. It delivers no
  input.

The owner rejected a CUA-style model where each session owns a cursor. Pointer
and logical-mouse reads belong to the `input` domain, next to `CreateMouse` and
`RemoveMouse`. No cursor crate is added, and a Run does not own a mouse.

## Implemented

| Surface | Behavior |
| --- | --- |
| `InputService/GetPointerPosition` (`input.pointer_position`) | Read-only. Returns the OS pointer as a `ScreenPoint` in logical screen space. |
| `auv invoke input.pointerPosition` | Same read from the CLI. `--dry-run` reads nothing. |
| Rust `InputClient::pointer_position()` | Returns `ScreenPoint`; rejects a missing or non-finite point. |
| `InputService/ScrollPoint` (`input.scroll`) | One foreground wheel scroll at a screen or display `Position`, with no target window. A window position fails with `INVALID_ARGUMENT`; use `ScrollWindowPoint`. |
| `auv invoke input.scrollPoint X Y DX DY` | Screen coordinates only (`TODO(scroll-point-display-cli)`). |
| Rust `InputClient::scroll(&impl Positional, Scroll, Duration)` | Returns `PointScroll { screen_point, action }`. |
| Rust `InputClient::key_down` / `key_up` | Typed client for the existing `KeyDown` / `KeyUp` RPCs. Rejects a timeout outside `(0, 30s]` and hold ID `0` before transport. |
| `KeyboardHoldController::down_independent` | Up to 16 concurrent holds when every hold names the same route and disjoint keys. `down` keeps one combination. No production caller yet. |

`ScrollPoint` is the scroll counterpart of `ClickPoint`'s global branch from
#284: it uses the same screen and display position conversion.

### Platform behavior

| Read or action | macOS | Windows | Linux |
| --- | --- | --- | --- |
| Pointer read | `NSEvent.mouseLocation` | `GetCursorPos` | Wayland fails with `unsupported` (`TODO(linux-wayland-pointer-position)`); no X11 backend on `main` |
| Scroll without a window | `UNIMPLEMENTED` (`TODO(macos-screen-scroll-point)`) | Foreground `scroll_at` | Foreground `scroll_at` (portal or uinput) |

A pointer read that the platform cannot answer fails. It never returns
`(0, 0)` or the last delivered position.

## Deferred

These are accepted directions, not approved slices. Each has a marker at the
`GetPointerPosition` RPC in `proto/auv/api/driver/v1/input.proto`.

- `TODO(logical-mouse-state)`: read one logical mouse's state and list a
  Runner's logical mice. Proposed names were `getMouseState(mouse)` and
  `listMice()`. The state is `MouseCoordinator`'s last delivered position and
  held-button state. Reads must not queue behind input actions.
- `TODO(pointer-watch)`: streamed updates as server-streaming RPCs, exposed as
  async iterators in the SDKs. Proposed names were `WatchPointer` and
  `WatchMice`. Agreed rules:
  - The first message is the current state; later messages are changes.
  - Pointer positions may coalesce. Each update carries `sampled_at`, the time
    the Runner read it. The draft message name was `PointerSample`, not
    `PointerObservation`.
  - Held, released, release-uncertain and removed mouse changes are never
    dropped. A consumer that falls behind gets an error and resubscribes.
  - Cancelling the RPC ends the subscription. No replay or resume in the first
    version.
  - Unlock trigger: a concrete consumer, such as a live viewer or a pointer
    picker.
- `TODO(pointer-click-atomic)`: a click at the current pointer position as one
  atomic action. A read followed by a click can see a different position.

## Evidence

All runs use PR #257 head `ba9f08c6` on 2026-10-09.

| Environment | Check | Result |
| --- | --- | --- |
| macOS 26.3, local CLI | `auv invoke input.pointerPosition --json` | The live pointer, for example `{ "x": 1318.55, "y": 778.38 }` |
| macOS 26.3, through the local daemon's Runner (`auv --device-id <local> invoke …`) | 3 pointer reads | Each matched a local read at the same time |
| macOS 26.3, through the Runner | `input.moveMouse 800 500`, then a pointer read | `{ "x": 800, "y": 500 }`: the read follows the pointer, it is not cached. The pointer was moved back afterwards |
| macOS 26.3, through the Runner | `input.scrollPoint 640 360 0 120` | `UNIMPLEMENTED`: "scrolling without a target window is unavailable on this Runner platform" |
| Windows 11, 1024×768 at scale 1, local CLI in the logged-on console session | `input.moveMouse 300 200` and `700 550`, each followed by a pointer read | `{ "x": 300, "y": 200 }` and `{ "x": 700, "y": 550 }` |
| Windows 11, local CLI from an SSH session (no interactive desktop) | Pointer read | Fails: `GetCursorPos failed: This operation requires an interactive window station. (0x800705B3)`. No fake point |
| Windows 11, console session, Edge window with a page that writes `scrollY` to its title | `input.scrollPoint 512 400 0 300`, then `0 -120`, then `0 240` | `scrollY` went `0 → 300 → 180 → 420`: exact logical-pixel totals and directions. Path `foreground_system_events` |
| Debian 13, headless Sway 1.10.1 (wlroots `headless` backend, `xdg-desktop-portal-wlr`) | Pointer read | Fails: `linux.input.current_position on Wayland is not supported by this driver` |
| Same headless Sway session | `input.scrollPoint 100 100 0 120` | Fails: `open RemoteDesktop: A portal frontend implementing org.freedesktop.portal.RemoteDesktop was not found`. The wlr portal has no input interface |
| Debian 13, GNOME Shell 48.7 Wayland (GDM autologin on seat0, 2560×1440), RemoteDesktop portal | Pointer read | Fails: `linux.input.current_position on Wayland is not supported by this driver` |
| Same GNOME session, maximized Firefox page that writes `scrollY` to its title | `input.scrollPoint 1280 700 0 360`, then `0 -120` | `scrollY` went `0 → 342 → 228`: 3 notches down, 1 notch up. The portal delivers whole 120 px notches; Firefox maps one notch to 114 px. Path `foreground_system_events` |

GNOME note: right after autologin, GNOME 48 shows the Activities overview. The
first two scrolls were delivered there and switched workspaces instead of
scrolling the page. After the Firefox window was activated, delivery reached
the page. The first input after the portal consent dialog is also not a
reliable test point.

Windows note: the first scroll attempts reported delivery but did not move the
page. A terminal window that the test harness opened was over the scroll
point, and Windows routes the wheel to the window under the pointer
(`MouseWheelRouting = 2`). With the harness window hidden, delivery was exact.
This is test-harness behavior, not an AUV defect, but it shows that delivery
evidence alone does not prove that the intended window scrolled.

Automated tests:

- Runner: `scroll_point_rpc_reuses_window_scroll_validation_before_delivery`,
  `scroll_point_rpc_rejects_a_window_position_before_delivery`
  (`crates/auv-cli/src/runner/local_driver_test.rs`).
- Client over a loopback gRPC fixture:
  `typed_point_scroll_sends_a_display_position_and_returns_the_screen_point`,
  `typed_key_holds_preserve_target_policy_timeout_id_and_release_results`
  (`crates/auv/src/client/runner_test.rs`).
- Hold controller: `crates/auv-driver-common/src/keyboard_input_test.rs`,
  including both #256 released-ID regressions.

Not yet recorded: Linux `ScrollPoint` through the uinput backend, and a
Windows or Linux pointer read or `ScrollPoint` through a Runner (the local CLI
path was used there).

## Research

Owner notes from 2026-10-07: `docs/notes/neko/2026-10-08-cua-sky-cursor-research.md`
and `docs/notes/neko/2026-10-08-electron-cursor-research.md` (not committed).
Summary:

- trycua/cua separates the system cursor (`get_cursor_position`) from
  per-session agent cursors (`get_agent_cursor_state`), whose positions come
  from a session table, not the OS.
- `@oai/sky` 0.7.1 exposes neither read.
- Electron's `screen.getCursorScreenPoint()` is a synchronous read. Continuous
  tracking means polling.
