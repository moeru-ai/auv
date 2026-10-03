# Held mouse input

Date: 2026-09-19. Classification: approved feature.
Status: implemented; platform-specific evidence is listed below.

Update 2026-09-23: [Native validation](2026-09-23-held-input-native-validation.md)
adds Windows posted-message/RDP and Linux GNOME/uinput receipt, plus isolated
Portal protocol receipt. Live Portal and macOS desktop receipt remain pending.

Update 2026-10-01: `auv invoke input.drag` exposes the complete DragMouse
gesture to the CLI, MCP, and selected Runner routes. See
[Invoke drag command](#invoke-drag-command). Cross-call MouseDown and MouseUp
remain outside invoke.

The owner approved primitive mouse input, complete hold/drag composition,
logical mice, and conservative shared-resource scheduling. The implementation
covers macOS, Windows, and Linux desktop routes, plus existing macOS PID-targeted
and Windows posted-message window routes. Linux window background delivery
remains explicitly unsupported.

## Contract

- Extend InputService with CreateMouse, RemoveMouse, MouseDown, MouseUp,
  and DragMouse. Existing MoveMouse and StreamMouseMotion share the
  driver execution path. Library and Rust/TypeScript clients expose these calls.
- Mouse ID zero is shared across Runs using the same local driver process.
  CreateMouse explicitly allocates separate logical state, not an OS cursor.
  RemoveMouse releases owned input first and refuses to remove the default mouse.
- Reuse InputTarget for addressing. Omission on MouseDown selects foreground;
  omission during movement continues a held route, otherwise selects foreground.
  Window targets retain observed window and process identity. Application-only
  mouse addressing is unsupported rather than implicitly choosing a window.
- All movement points are screen coordinates, including window-targeted input.
  Window native adapters translate using the receiver's current geometry.
- MoveMouseRequest directly contains mouse, target, start, curve, mapping, and
  options. MouseMotionPlan is removed. Wire field 1 (`plan`) is reserved, and
  flattened fields use new numbers. This is an intentional experimental API
  break; regenerate/update clients together. Older request bodies fail validation.
  Alpha APIs do not retain a legacy alias, request decoder, or migration shim.
  The reserved field is a Protobuf deletion marker, not a compatibility path.
- MouseDown holds one left/right/middle button. RPC omission selects left and
  a 30-second timeout; explicit timeouts must be positive, with no fixed maximum.
  MouseUp is idempotent after known release. Additional down while held is rejected.
- Local driver `hold_mouse` is a convenience operation that keeps a button down
  for a caller-selected positive duration, using the shared press/release lifecycle.
  Durations must fit the platform clock; there is no product-level hold limit.
  There is no HoldMouse RPC or remote Rust/TypeScript hold method. Remote callers
  use MouseDown/MoveMouse/MouseUp; a timed remote convenience is deferred until
  an approved atomic sequence contract can preserve admission and cleanup.
  DragMouse performs down, sampled movement, and up under one admission.
  Both use the same primitive state transitions and cleanup as cross-call input.
- InputActionResult reports delivery only. Feedback, video, and verification
  remain separate. A successful release does not prove application drop success.

## Scheduling and lifecycle

The shared driver-common coordinator owns logical mouse state, ordered admission,
held resource reservations, and cleanup. It is below the Runner so direct library
calls use the same rules. Each local process initially has one conservative
exclusive desktop resource. Different target windows do not prove independence.
The coordinator admits only the first eligible request for each mouse; the holder
can continue move/up even when another mouse is waiting. Complete gestures keep
one admission across the entire sequence. Existing complete clicks/scrolls reserve
the same resource, including window preparation/fallback.

A successful down retains the reservation after returning. Its backend is pinned
through up, including Linux's original Portal session/uinput device and Windows'
original child receiver. Movement cannot change an explicitly selected held target.
No foreground fallback is attempted during release.

Cancellation is execution context, not replay input. Runner blocking operations
bind a cancellation flag; cancelled queued requests are removed before posting.
Complete holds poll cancellation; movement checks cancellation between samples.
Closing movement feedback aborts delivery and cleans up held input. Cross-call
down has a watchdog even if the client never issues another call. Failed cleanup
retains ownership and quarantines normal reuse; explicit MouseUp retries release.
RemoveMouse does not erase uncertain ownership and returns the release's typed
delivery result. Normal Runner shutdown stops admission and releases cross-call
input. A force-killed process cannot run its watchdog or cleanup; time bounds
apply while the driver remains alive and its native calls return. Native post failures may follow
partial delivery, so down ownership is recorded before posting and cleanup is
attempted even when down reports failure.

The coordinator protects cooperating calls in one driver process. It does not
coordinate unrelated Runner processes, external automation, or physical input.
Use one Runner authority per desktop for cooperating agents. Truly independent
native mice and finer-grained backend resources need receiver evidence before
weakening this conservative policy.

## Caller-selected motion and event-driven scheduling

Motion duration, sampling frequency, curve segment count, and logical mouse count
have no fixed product maximum. Counts/identities must fit their integer types;
clock deadlines and memory allocations must be representable/available. Requested
sampling frequency is not a guarantee of native delivery throughput: overdue
samples coalesce and the final endpoint is retained.

`MouseMotionOptions.curve_tolerance` is a positive screen-space arc-length error
budget per cubic segment. Adaptive subdivision replaces the fixed 24 steps;
nonempty curves require an explicit tolerance. Timed curves require an explicit
positive sample rate. Instant direct positioning needs neither and uses zero for
both fields. `MoveMouseRequest::samples` returns `MouseSamples`, whose `at` and
`latest_due` methods evaluate the schedule without allocating one item per tick.
The former movement segment/sample caps and 60-second/240-Hz guards are removed.
Protocol sample counts/indices are uint64; Rust exposes u64 and JS exposes bigint.

`InputCancellation` wakes subscribed coordinators through condition variables.
Admission, active waits, and watchdogs sleep until state changes or their actual
deadline; there is no 10/20-ms polling policy. Shutdown and explicit cancellation
wake these waits immediately. The former AtomicBool cancellation argument is
replaced by `Arc<InputCancellation>`; callers request cancellation with `cancel()`.
The token and thread binding now live in `auv-driver-common::input_cancellation`;
mouse waiters register their lock/condition-variable wakeup there, and keyboard
holds observe the same token.

Progress uses a latest-value mailbox, not a fixed backlog of 16 events. Native
execution never waits for network feedback and has no one-second feedback timeout.
Started and terminal events are retained, and a single downstream handoff slot
applies transport backpressure without buffering progress history. Disconnecting
the receiver cancels even an operation still waiting for admission.

## Invoke drag command

`input.drag <START_X> <START_Y> <END_X> <END_Y>` is the invoke frontend for
DragMouse. It is available on macOS, Windows, and Linux, and it uses the target
and coordinate rules of `input.clickPoint`.

- The command builds one straight cubic segment in screen coordinates. It
  samples motion at 60 Hz over `--duration-ms`, which defaults to 300 and
  accepts 0 through 30000. `--button` selects left, right, or middle.
- A screen basis and a display basis use foreground desktop delivery. A window
  basis, selected by `app:` or `window:`, calls `WindowInput::drag` with
  `--input-policy`, which defaults to background preferred, like a window
  click:
  - Background policies use the window-targeted route on macOS and Windows,
    without moving the pointer. Linux has no window-targeted pointer route,
    so `background-preferred` focuses the window through AT-SPI and uses the
    foreground portal, and `background-only` is rejected.
  - `foreground-preferred` foregrounds the window as a foreground click does
    (macOS input preparation, Windows `SetForegroundWindow`, Linux AT-SPI
    focus) and then uses the desktop drag.
  - No route falls back after a background drag reports `delivered`, because
    delivery does not show whether the application consumed the gesture.
  - Windows and Linux adapters compile from the same shape as their click
    adapters. They have not been run.
- Window and display bases require both endpoints inside the target frame.
  `--normalized` applies to all four coordinates.
- Local invoke and MCP call the local driver session. A selected Device or Run
  sends the same plan to the Runner as one DragMouse request. DragMouse has no
  window policy, so the Runner route refuses `foreground-preferred` before any
  I/O (`TODO(drag-runner-window-policy)` in `crates/auv-cli-invoke/src/runner.rs`).
- The result reports both requested and screen endpoints, the resolved window
  or display, the final pointer position, and InputActionResult. It does not
  verify the application drop. Callers verify that result separately.
- Each invoke process ends after the command. Therefore invoke does not expose
  cross-call MouseDown or MouseUp. `TODO(invoke-mouse-hold)` in
  `crates/auv-cli-invoke/src/commands/input.rs` marks this deferral.

### Live macOS observations (2026-10-02)

Evidence level: one manual session on macOS 26 with an external display. These
observations do not establish support across applications or OS versions.

- **Foreground screen drag, Chrome receiver page.** A listen-only CGEventTap
  recorded `mouseMoved` at the start point, one `leftMouseDown`
  (`clickState=1`), 18 `leftMouseDragged` events about 16 ms apart along the
  line, and one `leftMouseUp` (`clickState=0`). With the terminal frontmost and
  Chrome inactive, the page element did not move. After `app.activate` for
  Chrome, the same drag moved the element (owner observation). An inactive
  window can therefore consume the first press as activation only.
- **Window-targeted drag, inactive TextEdit.** A drag across the first text
  line of an 84-character document reported `delivered`,
  `window_targeted_mouse`, and no disturbance. TextEdit stayed inactive, and
  its AX selected range stayed `(84, 0)`. A window-targeted
  `input.clickPoint` at the same point and at an empty point near the bottom
  left the range unchanged too. `delivered` is not consumption evidence (gap
  review BG-7).
- **Same drag, active TextEdit.** After `app.activate` for TextEdit, the same
  window-targeted drag selected `(1, 40)` without moving the pointer. A
  foreground screen drag over the same line, after a click collapsed the
  selection, also selected `(1, 40)`. The NSTextView therefore ignored the
  window-targeted events only while TextEdit was inactive.
- **`--input-policy foreground-preferred`, inactive TextEdit.** With Chrome
  frontmost, a window drag over the second line foregrounded TextEdit, used
  `foreground_system_events`, reported `focus_disturbance: foreground`, and
  selected `(73, 9)`. The document length stayed 84 characters.
- The foreground route leaves the pointer at the end point; only the click and
  scroll paths warp it back. `InputActionResult` still reports
  `mouse_disturbance: temporary` for a drag.
- The 2026-09-19 window-route receipt came from a fixture NSView that handles
  raw mouse events directly. It does not cover standard AppKit controls.

## Platform boundaries

| Route | Implementation | Evidence boundary |
| --- | --- | --- |
| macOS desktop | CGEvent down/dragged/up | 2026-10-02: event-tap sequence recorded; Chrome page drag after activation (owner observation) |
| macOS window | Fixed window/PID, window-local stamping, postToPid | Fixture NSView observed down → dragged → up on 2026-09-19; on 2026-10-02 a TextEdit NSTextView consumed a drag only while TextEdit was active (AX readback) |
| Windows desktop | SendInput button transitions and absolute motion | Win32 receiver in Windows 11 RDP session: 15 cases passed on 2026-09-23 |
| Windows window | Fixed child HWND, button mask on WM_MOUSEMOVE, posted down/up | Hidden Win32 receiver: 15 cases passed on 2026-09-23 |
| Linux Portal | Retained RemoteDesktop session, button/motion calls | Native tests and isolated D-Bus held receipt passed on 2026-09-23; live receipt blocked by missing RemoteDesktop interface |
| Linux uinput | Retained virtual device, key/button transitions and motion | GNOME/GTK4 receiver: 15 cases passed on 2026-09-23 |

NOTICE: The macOS Chromium-compatible click primer remains click-only. Extending
its offscreen priming and dual-post sequence to cross-call held gestures requires
receiver evidence; the new held window route is explicitly PID-targeted.
NOTICE: Multi-button chords and held keyboard modifiers are not implemented by
this pointer lifecycle. They need a separate accepted native transition contract.
PR #185's keyboard hold proposal is not silently included in this change.

## Validation evidence

Shared scheduler regression tests cover owner continuation ahead of a foreign
waiter, cancellation before admission, cancelled complete hold/drag cleanup,
watchdog release without another request, quarantine/recovery after failed up,
invalid coordinates, held target changes, and overdue-sample coalescing.
Validation commands:

- `cargo check`: passed for the workspace on macOS.
- `cargo test`: runnable default-package tests passed. The rustdoc phase initially
  reported E0463 for auv_cli_invoke; `cargo test -p auv-cli --doc` then passed.
- `cargo fmt --check`, `git diff --check`, and `cargo run --quiet -- invoke --help`: passed.
- `cargo test -p auv-driver-common mouse_input --lib`: scheduler regression tests passed.
- Focused `auv`, `auv-cli`, `auv-driver-common`, and host-portable Linux tests passed.
- `cargo check -p auv-driver-windows --target x86_64-pc-windows-msvc --tests`: passed.
- `scripts/generate-swift-bridge` and macOS native `swift build`: passed.
- `cargo test -p auv-driver-macos --test held_mouse -- --ignored --nocapture`:
  passed against a separate AppKit process. The test fixture and receiver are
  checked in under `crates/auv-driver-macos/tests/`.
- `pnpm --filter @auv-js/sdk exec vitest run src/apis/auv/driver.test.ts`: four tests passed.
- `buf lint` and `buf generate`: passed. `buf breaking` reports the intentional
  removal of MouseMotionPlan and the old MoveMouseRequest.plan field.
- After a frozen-lockfile dependency install in the isolated PR worktree,
  `pnpm lint` and `pnpm typecheck` pass. Earlier AbortSignal.any typing errors
  in the original workspace did not reproduce with these installed dependencies.
- Running Windows unit tests on the macOS host fails two pre-existing native
  expectations (OCR InvalidImage and ScreenToClient); the Windows target check
  is the relevant compilation evidence, not a native execution claim.

The original 2026-09-19 remote validation encountered a broken SOCKS path.
That transport was restored for the [2026-09-23 native validation](2026-09-23-held-input-native-validation.md).
The earlier macOS-host Linux cross-check lacked a Linux pkg-config/sysroot for
leptonica; the later checks ran natively on Linux with its installed dependencies.

## Related references

- [BG-1 review](2026-09-09-background-ax-and-media-gap-review.md)
- [Project research](2026-09-18-held-input-project-research.md)
- [Button contract](2026-09-17-click-buttons-contract.md)
- [Keyboard hold PR](https://github.com/moeru-ai/auv/pull/185)
- [Shared terms](../../../TERMS_AND_CONCEPTS.md#mouse-movement-request)
