# macOS No-Steal Input Design

Date: 2026-05-26

Status: stage 1 implemented, later stages provisional

## Purpose

AUV should be able to drive a target macOS application while minimizing
disturbance to the user's current work. The first implementation should focus
on macOS only and should not create a fake cross-platform abstraction before
the macOS behavior is understood.

This design covers window-level no-steal input. It does not cover Surface IR,
interaction orchestration, OCR/model inference, or capture performance.

## Current AUV State

- `MacosDriverSession::window()` resolves and captures windows, and can run
  OCR against window captures.
- `MacosDriverSession::input()` currently exposes global input primitives such
  as `click_at`, `copy`, `paste`, and `paste_text`.
- `paste_text` uses System Events and depends on the current foreground focus.
- AX focus and AX action functionality exists in the legacy command path and
  native AX tree code, but it is not exposed as a typed no-steal action path.
- Pointer input uses global click coordinates and does not yet post
  pid/window-targeted mouse events.

## Reference Implementations

CUA and KWWK are both macOS-focused references for low-disturbance input. AUV
should learn the mechanics but keep its public API target-oriented.

CUA references:

- [`type_text.rs`](https://github.com/trycua/cua/blob/a3448588286b6373013a5fa9072ac8bafb6681d6/libs/cua-driver-rs/crates/platform-macos/src/tools/type_text.rs#L1-L14)
  documents `AXSelectedText` as the preferred type path and CGEvent keyboard as
  the fallback for Chromium/Electron.
- [`type_text.rs`](https://github.com/trycua/cua/blob/a3448588286b6373013a5fa9072ac8bafb6681d6/libs/cua-driver-rs/crates/platform-macos/src/tools/type_text.rs#L177-L204)
  writes `AXSelectedText`, verifies `AXValue`, detects silent accept, and falls
  back to CGEvent typing.
- [`input/keyboard.rs`](https://github.com/trycua/cua/blob/a3448588286b6373013a5fa9072ac8bafb6681d6/libs/cua-driver-rs/crates/platform-macos/src/input/keyboard.rs#L1-L7)
  explains background keyboard delivery through `SLEventPostToPid` with public
  `CGEvent::post_to_pid` fallback.
- [`tools/click.rs`](https://github.com/trycua/cua/blob/a3448588286b6373013a5fa9072ac8bafb6681d6/libs/cua-driver-rs/crates/platform-macos/src/tools/click.rs#L235-L247)
  translates window-local screenshot pixels into screen coordinates and
  window-local logical coordinates.
- [`tools/click.rs`](https://github.com/trycua/cua/blob/a3448588286b6373013a5fa9072ac8bafb6681d6/libs/cua-driver-rs/crates/platform-macos/src/tools/click.rs#L307-L318)
  passes both screen and window-local coordinates into the background click
  implementation.
- [`input/mouse.rs`](https://github.com/trycua/cua/blob/a3448588286b6373013a5fa9072ac8bafb6681d6/libs/cua-driver-rs/crates/platform-macos/src/input/mouse.rs#L20-L40)
  exposes pid-targeted click and window-local event stamping.
- [`input/mouse.rs`](https://github.com/trycua/cua/blob/a3448588286b6373013a5fa9072ac8bafb6681d6/libs/cua-driver-rs/crates/platform-macos/src/input/mouse.rs#L383-L438)
  stamps window-local and Chromium routing fields and posts through both
  SkyLight and public CGEvent routes.

KWWK references:

- [`ComputerUseActions.swift`](https://github.com/EYHN/kwwk-computer-use-core/blob/eddd9e5475095de58bcb81cafbad79d1f5c5495d/Sources/KWWKComputerUseCore/ComputerUseActions.swift#L144-L162)
  chooses AX action first for element clicks, then falls back to a window-local
  background mouse click.
- [`ComputerUseActions.swift`](https://github.com/EYHN/kwwk-computer-use-core/blob/eddd9e5475095de58bcb81cafbad79d1f5c5495d/Sources/KWWKComputerUseCore/ComputerUseActions.swift#L175-L205)
  focuses an editable AX element when possible and types through a background
  keyboard dispatcher.
- [`ComputerUseActions.swift`](https://github.com/EYHN/kwwk-computer-use-core/blob/eddd9e5475095de58bcb81cafbad79d1f5c5495d/Sources/KWWKComputerUseCore/ComputerUseActions.swift#L215-L256)
  exposes a separate AXValue-setting operation.
- [`ComputerUseActionSupport.swift`](https://github.com/EYHN/kwwk-computer-use-core/blob/eddd9e5475095de58bcb81cafbad79d1f5c5495d/Sources/KWWKComputerUseCore/ComputerUseActionSupport.swift#L81-L93)
  converts AX node frames to window-local points.
- [`ComputerUseActionSupport.swift`](https://github.com/EYHN/kwwk-computer-use-core/blob/eddd9e5475095de58bcb81cafbad79d1f5c5495d/Sources/KWWKComputerUseCore/ComputerUseActionSupport.swift#L155-L166)
  sends a click through a background mouse dispatcher.
- [`BackgroundInputDispatcher.swift`](https://github.com/EYHN/kwwk-computer-use-core/blob/eddd9e5475095de58bcb81cafbad79d1f5c5495d/Sources/KWWKComputerUseCore/BackgroundInputDispatcher.swift#L47-L60)
  posts mouse down/up events to a target pid/window.
- [`BackgroundInputDispatcher.swift`](https://github.com/EYHN/kwwk-computer-use-core/blob/eddd9e5475095de58bcb81cafbad79d1f5c5495d/Sources/KWWKComputerUseCore/BackgroundInputDispatcher.swift#L130-L162)
  stamps pid/window addressing fields and posts events to the target pid.
- [`BackgroundInputDispatcher.swift`](https://github.com/EYHN/kwwk-computer-use-core/blob/eddd9e5475095de58bcb81cafbad79d1f5c5495d/Sources/KWWKComputerUseCore/BackgroundInputDispatcher.swift#L264-L333)
  types and presses keys by posting keyboard events to the target pid/window.
- [`ComputerUseSession.swift`](https://github.com/EYHN/kwwk-computer-use-core/blob/eddd9e5475095de58bcb81cafbad79d1f5c5495d/Sources/KWWKComputerUseCore/ComputerUseSession.swift#L97-L120)
  wraps actions in a background activation session.

## Public API Shape

Public APIs are grouped by automation target, not native mechanism. Window-local
actions belong under `session.window()`. `session.input()` remains a lower-level
escape hatch for raw pointer, keyboard, clipboard, and paste primitives.

First-stage API:

```rust
let lease = session.window().prepare_for_input(
  &window,
  PrepareForInputOptions {
    activation: ActivationPolicy::Background,
    preserve_frontmost: true,
    install_focus_guard: true,
    settle: Duration::from_millis(80),
  },
)?;

let result = session.window().type_text(
  &window,
  "bluefieldcreator",
  TypeTextOptions {
    policy: InputPolicy::BackgroundPreferred,
    replace_existing: true,
    submit: TextSubmit::Return,
    ..Default::default()
  },
)?;

session.window().restore_input(lease)?;
```

Single-action helpers may internally prepare and restore input if no explicit
lease is supplied. Complex interactions should use an explicit lease so the
preparation lifetime is visible and errors from restore can be returned.

## Proposed Types

Names are provisional and should be reviewed during implementation.

```rust
pub enum InputPolicy {
  BackgroundOnly,
  BackgroundPreferred,
  ForegroundPreferred,
}
```

- `BackgroundOnly`: only no-steal or background delivery is allowed. If no such
  path works, the action fails.
- `BackgroundPreferred`: try no-steal/background paths first. In the stage 1
  macOS implementation this does not foreground-fallback yet; later interaction
  policy can choose whether to allow that and must record the fallback reason
  and disturbance when it does.
- `ForegroundPreferred`: in the stage 1 macOS implementation, try
  no-steal/window-targeted delivery first and use foreground fallback only if
  that fails. The name is provisional and may be refined once action policy is
  separated from delivery preference.

NOTICE(2026-06-02-scroll-policy): Scroll now has a narrower owner-approved
policy override in `docs/ai/references/ops/2026-06-02-background-scroll-policy-design.md`.
For scroll input, `ForegroundPreferred` means choose foreground/global HID first;
`BackgroundPreferred` is the policy that tries window-targeted delivery before
foreground fallback. Non-input capabilities such as capture remain background
when they are naturally non-disturbing.

```rust
pub enum ActivationPolicy {
  NoChange,
  Background,
  FocusWithoutRaise,
  Foreground { settle: Duration },
}
```

- `NoChange`: do not change foreground app/window and do not perform background
  activation.
- `Background`: allow background preparation and pid/window-targeted delivery,
  but do not bring the target app/window to the foreground.
- `FocusWithoutRaise`: allow best-effort internal focus without raising the
  target window. On macOS this may use AX focus or private WindowServer/SkyLight
  techniques. It is advanced and not required for the first closed loop.
- `Foreground`: activate the target app/window and wait for UI stability.

```rust
pub struct PrepareForInputOptions {
  pub activation: ActivationPolicy,
  pub preserve_frontmost: bool,
  pub install_focus_guard: bool,
  pub settle: Duration,
}

pub struct InputPreparationLease {
  // Opaque public handle. macOS implementation stores previous frontmost
  // state, background activation state, and focus guard state.
}

pub struct ClickOptions {
  pub policy: InputPolicy,
  pub click: Click,
}

pub struct TypeTextOptions {
  pub policy: InputPolicy,
  pub replace_existing: bool,
  pub submit: TextSubmit,
  pub inter_char_delay: Duration,
  pub allow_clipboard_fallback: bool,
  pub settle: Duration,
}

pub struct InputActionResult {
  pub selected_path: InputDeliveryPath,
  pub attempts: Vec<InputAttempt>,
  pub fallback_reason: Option<String>,
  pub mouse_disturbance: DisturbanceLevel,
  pub focus_disturbance: DisturbanceLevel,
  pub clipboard_disturbance: DisturbanceLevel,
}
```

## Type Text Strategy

CUA and KWWK do not treat clipboard paste as the primary no-steal fallback.
CUA prefers `AXSelectedText`, verifies `AXValue`, and falls back to pid-targeted
CGEvent typing. KWWK focuses the AX element when possible and uses a background
keyboard dispatcher; it exposes AXValue as a separate operation.

AUV should follow that shape:

1. If a target AX element is known, try `AXSelectedText` for insertion.
2. Verify success by reading `AXValue` where meaningful.
3. If `replace_existing` is true and the element supports `AXValue`, prefer
   `AXValue` for replacement.
4. If AX text paths fail, try pid/window-targeted keyboard delivery.
5. Use clipboard paste only when `allow_clipboard_fallback` is true.
6. Use foreground System Events only when the action policy is
   `ForegroundPreferred` in stage 1. `BackgroundPreferred` foreground fallback
   is intentionally deferred until the interaction policy layer can make that
   disturbance explicit.

Clipboard fallback must snapshot and restore the clipboard, and the action
result must record clipboard disturbance even when restoration succeeds.

## Click Strategy And Coordinates

Window-level click APIs should accept coordinates in a clearly named coordinate
space. The first implementation should not keep using ambiguous global `Point`
parameters for window actions.

Recommended first-stage methods:

```rust
session.window().click(&window, WindowPoint::new(x, y), options)?;
session.screen().click(ScreenPoint::new(x, y), options)?;
```

Where:

- `WindowPoint` is window-local logical coordinates.
- `ScreenPoint` is desktop/global coordinates.
- Conversion helpers live under `session.window()`:

```rust
let screen_point = session.window().to_screen_point(&window, window_point)?;
let window_point = session.window().to_window_point(&window, screen_point)?;
```

The macOS no-steal click implementation should compute both:

- screen coordinates for the CGEvent location;
- window-local coordinates for `CGEventSetWindowLocation` and window routing
  fields.

This mirrors CUA and KWWK: user-facing window tools operate in window-local
space, while native dispatch receives both screen-space and window-local
coordinates.

## Native Boundary

The public API should not expose implementation verbs such as `post_*` or
generic names such as `perform_action`.

Target-facing native wrappers should use semantic names:

```rust
native::window::prepare_for_input(...)
native::window::restore_input(...)
native::input::click_window_point(...)
native::input::type_text_in_window(...)
native::ax::press(...)
native::ax::focus(...)
native::ax::set_value(...)
native::ax::set_selected_text(...)
```

Implementation-specific details may be nested deeper:

```rust
native::input::cg_event::post_to_pid(...)
native::input::skylight::post_to_pid(...)
native::input::skylight::set_window_location(...)
```

Implementation comments should cite the reference code when borrowing behavior
from CUA or KWWK, using GitHub permalinks with commit hashes and line anchors.

## Implementation Stages

### Stage 1: Close the NetEase Loop

- Added shared `WindowPoint`, `ScreenPoint`, input policy, preparation,
  option, attempt, and result types in `auv-driver`.
- Added public macOS window APIs:
  - `prepare_for_input`
  - `restore_input`
  - `click`
  - `type_text`
  - coordinate conversion helpers
- Added native Swift/Rust bridge wrappers for pid/window-targeted mouse and
  keyboard delivery.
- Added result/attempt records for selected path and fallback reason.
- Implemented window-local background mouse click and pid/window-targeted
  keyboard delivery where the macOS backend supports the required event
  routing fields.
- Migrated `examples/netease_play_visible_anchor.rs` away from
  `session.input().click_at` and `session.input().paste_text`.

Deferred from stage 1:

- AX text path (`AXSelectedText`, `AXValue`) is still a next step.
- `FocusWithoutRaise`, foreground restoration, and focus guard installation
  currently return unsupported where restoration semantics would otherwise be
  misleading.
- Window-level scroll, drag, hotkey, and press-key public helpers are not yet
  exposed, although native keyboard wrappers exist for `type_text` internals.
- Driver results are not yet written into run trace data beyond existing
  example spans and screenshot artifacts.
### Stage 2: Improve Coverage

- Add `press_key`, `hotkey`, `scroll`, and `drag` under `session.window()`.
- Add focus guard / focus steal suppression.
- Add richer AX action helpers.
- Add foreground fallback paths where policy allows them.

### Stage 3: Integrate With Interaction Layer

- Use no-steal window actions in scroll scan and higher-level programmable
  interactions.
- Record selected path, fallback reason, and disturbance in run trace data.
- Keep driver results independent from `.auv/runs` persistence.

## Acceptance Criteria

- NetEase visible-anchor example can be expressed through `session.window()`
  actions instead of raw global input.
- A window click can be attempted without moving the real mouse when the macOS
  backend supports pid/window-targeted dispatch.
- Text entry first attempts AX/background paths before clipboard or foreground
  fallback.
- `InputActionResult` records selected delivery path and fallback reason.
- Foreground focus/mouse/clipboard disturbance is visible in the result.
- Code comments near macOS no-steal implementation include reference permalinks
  for CUA/KWWK-derived behavior.

## Known Risks

- Focus-without-raise is macOS-specific and may depend on private API behavior.
- Chromium/Electron and AppKit may need different event fields or auth-message
  behavior.
- AX text writes may silently succeed without changing app state; verification
  is required.
- Clipboard fallback is not no-steal in the strict sense because it mutates a
  shared system resource, even if restored.
- Window-local coordinate conversion needs careful handling of display scale,
  capture scale, and window bounds.

## Implementation Decisions

- Add `WindowPoint` and `ScreenPoint` newtypes in `auv-driver` rather than
  relying on ambiguous `Point` parameters for window actions.
- Keep `InputPreparationLease` restoration explicit in the first
  implementation. A best-effort `Drop` guard can be added later, but it cannot
  replace explicit restore because `Drop` cannot return restore errors.
- Keep `FocusWithoutRaise` in `ActivationPolicy` as a named policy, but allow
  the first implementation to return an unsupported result for that policy if
  the native path is not ready.
