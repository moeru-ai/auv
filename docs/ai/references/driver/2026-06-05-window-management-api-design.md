# AUV Window Management API Design

## Background

AUV currently has a typed `session.window()` surface for observing and acting
inside application windows:

- `resolve` and `list` find observed windows.
- `capture`, `find_text`, and coordinate conversion read window state.
- `click`, `type_text`, and `scroll` deliver input relative to a window.

The existing `Window` type is an observed value/reference. It does not own
platform IO. This design keeps that shape: mutation remains on `WindowApi`
through methods that accept `&Window`.

The missing surface is window management: moving, resizing, setting frame, and
changing state such as minimize, restore, and macOS zoom. These operations are
not pointer interactions. They mutate the window itself and should be verified
through window geometry or state, not through successful input event delivery.

## Goals

- Add a unified `WindowApi` management surface that matches current AUV driver
  style.
- Keep public API platform-neutral and not AX-facing.
- Treat macOS Accessibility as one delivery candidate, not as the contract.
- Return typed attempts and before/after evidence so unsupported or partially
  supported windows are inspectable.
- Keep pointer drag out of this slice.

## Non-Goals

- Do not introduce a public AX window API.
- Do not add pointer drag, title-bar drag fallback, or general interaction
  intent in this slice.
- Do not claim Windows or Linux support before a platform implementation exists.
- Do not rename macOS `zoom` to `maximize`; macOS zoom is not strict maximize.

## Public API Shape

The API follows the existing session service style:

```rust
let window = session
  .window()
  .resolve(Window::main_visible().owned_by(App::bundle("com.example.App")))?;

session.window().move_to(&window, ScreenPoint::new(100.0, 80.0), options)?;
session.window().resize(&window, Size::new(1200.0, 800.0), options)?;
session.window().set_frame(&window, Rect::new(100.0, 80.0, 1200.0, 800.0), options)?;

session.window().minimize(&window, options)?;
session.window().restore(&window, options)?;
session.window().zoom(&window, options)?;
```

`move_to` changes origin while preserving size. `resize` changes size while
preserving origin. `set_frame` changes origin and size together.

`minimize` requests minimized state. `restore` requests a visible,
non-minimized state when the platform can express it. `zoom` maps to the
platform's zoom/maximize-like window action. On macOS this is `AXZoomWindow`,
which may be a toggle and is not equivalent to Windows maximize.

## Options and Delivery Candidates

Window management gets its own options instead of reusing `InputPolicy`, because
these operations are not input delivery:

```rust
pub struct WindowMutationOptions {
  pub policy: WindowMutationPolicy,
  pub strategy: WindowMutationStrategy,
  pub settle: Duration,
  pub verification: WindowMutationVerification,
}

pub enum WindowMutationPolicy {
  NativeOnly,
  NativePreferred,
  ForegroundPreferred,
}

pub struct WindowMutationStrategy {
  pub candidates: Vec<WindowMutationCandidate>,
}

pub enum WindowMutationCandidate {
  AxWindowAttribute,
  AxWindowAction,
  PlatformNative,
  ForegroundSystemEvents,
}
```

Default candidates should prefer low-disturbance native paths:

```rust
WindowMutationStrategy {
  candidates: vec![
    WindowMutationCandidate::AxWindowAttribute,
    WindowMutationCandidate::AxWindowAction,
  ],
}
```

`PlatformNative` is reserved for platform-specific window management APIs. It is
not implemented on macOS in the first slice because macOS does not expose a
stable cross-process public WindowServer mutation API for arbitrary app
windows.

`ForegroundSystemEvents` is reserved for future explicit fallbacks such as menu
commands, shortcuts, or pointer-based title-bar operations. It should not be
enabled by default because it may disturb focus or the mouse.

## Result Model

Window mutation should return a typed result rather than an `InputActionResult`:

```rust
pub struct WindowMutationResult {
  pub selected_path: WindowMutationPath,
  pub attempts: Vec<WindowMutationAttempt>,
  pub fallback_reason: Option<String>,
  pub before_frame: Option<Rect>,
  pub after_frame: Option<Rect>,
  pub before_state: Option<WindowState>,
  pub after_state: Option<WindowState>,
  pub focus_disturbance: DisturbanceLevel,
  pub mouse_disturbance: DisturbanceLevel,
}

pub struct WindowMutationAttempt {
  pub path: WindowMutationPath,
  pub succeeded: bool,
  pub message: Option<String>,
}

pub enum WindowMutationPath {
  AxWindowAttribute,
  AxWindowAction,
  PlatformNative,
  ForegroundSystemEvents,
  Unsupported,
}

pub struct WindowState {
  pub is_minimized: Option<bool>,
  pub is_visible: Option<bool>,
}
```

This is a new schema, but it is not a third input action result. It belongs to
window management and records window mutation attempts. Action delivery remains
covered by `ActionResolverDecision` and `InputActionResult`.

## macOS First Implementation

The first macOS implementation should use Accessibility internally:

- `move_to`: set `kAXPositionAttribute`.
- `resize`: set `kAXSizeAttribute`.
- `set_frame`: set position and size, then verify the resulting frame.
- `minimize`: set `kAXMinimizedAttribute` to `true`.
- `restore`: set `kAXMinimizedAttribute` to `false`, then verify visibility or
  minimized state when observable.
- `zoom`: perform `kAXZoomWindowAction`.

The public API should not expose AX types or AX attribute names. Error messages
and attempts may mention the selected delivery path and failure reason, such as
missing AX window, unsupported attribute, read-only attribute, failed action, or
post-action verification mismatch.

## Verification

Geometry mutations should capture a before frame and an after frame. Verification
should compare requested and observed values with a small point tolerance.

State mutations should verify the best observable state:

- `minimize`: `is_minimized == true` when the platform reports it.
- `restore`: `is_minimized == false` or visible frame availability when reported.
- `zoom`: frame changed or the platform reports action success; because macOS
  zoom may be a toggle, the result must not claim strict maximize.

When the platform cannot observe the state, the result should record that limit
instead of fabricating success.

## CLI / Command Surface

The command-facing surface should mirror current macOS command parameters:

- `--input_policy` style names should not be reused for window management.
- Use `--window_mutation_policy` or `--mutation_policy`.
- Use `--delivery_path` only when selecting an explicit candidate for a debug
  command; otherwise prefer the options strategy.

Likely debug commands:

- `debug.moveWindow`
- `debug.resizeWindow`
- `debug.setWindowFrame`
- `debug.minimizeWindow`
- `debug.restoreWindow`
- `debug.zoomWindow`

These commands should emit artifacts and notes with requested frame/state,
before frame/state, after frame/state, selected path, fallback reason, and
known limits.

## Platform Behavior

macOS implements the AX candidates first.

Windows and Linux should initially return typed unsupported results from the
driver if no implementation exists. They should not silently no-op or pretend
that AX-style behavior exists. Later implementations can add native candidates
under the same API:

- Windows may use Win32 window management APIs.
- Linux support depends on X11/EWMH or compositor-specific Wayland protocols.

## Testing

Unit tests should cover:

- Default `WindowMutationOptions`.
- Serde for mutation policy, candidates, paths, attempts, and results.
- Candidate ordering and policy interpretation.
- Command parsers for mutation policy and explicit candidate selection.
- Verification tolerance logic for geometry operations.
- Unsupported platform result shape.

macOS native tests should stay narrow and may use a small controllable app or
fixture window when available. Tests should assert observable window frame/state
changes rather than only AX call success.

## Deferrals

- TODO(window-management-pointer-fallback): pointer/title-bar drag fallback is
  deferred because this slice is window management, not pointer interaction.
  Reopen only when the owner approves a foreground-disturbing fallback path.
- TODO(window-management-platform-native): macOS platform-native non-AX window
  mutation is reserved because no stable public cross-process WindowServer API
  is available for arbitrary windows. Reopen if a supported platform primitive
  is identified.
- TODO(window-management-maximize): strict maximize is deferred because macOS
  zoom is not equivalent to Windows maximize. Reopen when cross-platform state
  semantics are designed.
