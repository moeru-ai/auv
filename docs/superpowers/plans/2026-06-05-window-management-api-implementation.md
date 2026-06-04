# Window Management API Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add typed window management APIs for moving, resizing, framing, minimizing, restoring, and zooming windows, with macOS AX as the first configurable delivery candidate.

**Architecture:** Keep AUV's current `session.window().method(&window, ...)` style. Add platform-neutral mutation types to `auv-driver`, implement macOS mutation through an internal native AX bridge, and expose debug commands through the existing macOS command driver. Window management returns `WindowMutationResult`, not `InputActionResult`.

**Tech Stack:** Rust 2024, `serde`, `swift-bridge`, macOS Swift Accessibility APIs, existing AUV command catalog and typed macOS driver.

---

### File Structure

- Modify `crates/auv-driver/src/window.rs`: add public mutation option/result/path/state types next to `Window`.
- Modify `crates/auv-driver/src/lib.rs`: re-export the new window management types and update public API export tests.
- Modify `crates/auv-driver-macos/src/session.rs`: add `WindowApi` methods and policy/candidate execution logic.
- Modify `crates/auv-driver-macos/src/native/binding.rs`: add bridge request/response structs and Swift externs.
- Modify `crates/auv-driver-macos/src/native/window.rs`: add safe Rust wrappers and response decoding for native window mutation.
- Modify `crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/Window.swift`: implement AX-backed move, resize, minimize, restore, and zoom.
- Modify `src/driver/macos/typed.rs`: add command-driver bridge adapters for window mutation results.
- Modify `src/driver/macos/control/window.rs`: add command handlers, parsers, result note rendering, and tests.
- Modify `src/driver/macos/control/mod.rs`: export the new command handlers.
- Modify `src/driver/macos/dispatch.rs`: dispatch new window management operations.
- Modify `src/catalog.rs`: register debug commands.
- Modify `src/driver/macos/tests.rs`: add parser/catalog/dispatch coverage where existing tests live.
- Run `hack/generate-swift-bridge` and SwiftPM build after bridge or Swift edits.

### Task 1: Add Public Window Mutation Types

**Files:**
- Modify: `crates/auv-driver/src/window.rs`
- Modify: `crates/auv-driver/src/lib.rs`

- [ ] **Step 1: Add failing type/default tests**

Add tests to `crates/auv-driver/src/window.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use std::time::Duration;

  #[test]
  fn window_mutation_options_default_to_native_preferred_ax_candidates() {
    let options = WindowMutationOptions::default();

    assert_eq!(options.policy, WindowMutationPolicy::NativePreferred);
    assert_eq!(
      options.strategy,
      WindowMutationStrategy {
        candidates: vec![
          WindowMutationCandidate::AxWindowAttribute,
          WindowMutationCandidate::AxWindowAction,
        ],
      }
    );
    assert_eq!(options.settle, Duration::from_millis(100));
    assert_eq!(
      options.verification,
      WindowMutationVerification::FrameTolerance { points: 2.0 }
    );
  }

  #[test]
  fn window_mutation_types_serde_as_snake_case() {
    let result = WindowMutationResult {
      selected_path: WindowMutationPath::AxWindowAttribute,
      attempts: vec![WindowMutationAttempt {
        path: WindowMutationPath::AxWindowAttribute,
        succeeded: true,
        message: Some("set AXPosition".to_string()),
      }],
      fallback_reason: None,
      before_frame: Some(Rect::new(0.0, 0.0, 400.0, 300.0)),
      after_frame: Some(Rect::new(10.0, 20.0, 400.0, 300.0)),
      before_state: Some(WindowState {
        is_minimized: Some(false),
        is_visible: Some(true),
      }),
      after_state: Some(WindowState {
        is_minimized: Some(false),
        is_visible: Some(true),
      }),
      focus_disturbance: crate::input::DisturbanceLevel::None,
      mouse_disturbance: crate::input::DisturbanceLevel::None,
    };

    let encoded = serde_json::to_value(&result).expect("serialize");
    assert_eq!(encoded["selected_path"], "ax_window_attribute");
    assert_eq!(encoded["attempts"][0]["path"], "ax_window_attribute");

    let decoded: WindowMutationResult =
      serde_json::from_value(encoded).expect("deserialize");
    assert_eq!(decoded, result);
  }
}
```

- [ ] **Step 2: Run tests to verify failure**

Run:

```bash
cargo test -p auv-driver window_mutation
```

Expected: FAIL with missing `WindowMutationOptions`, `WindowMutationPolicy`, `WindowMutationStrategy`, `WindowMutationCandidate`, `WindowMutationVerification`, `WindowMutationResult`, `WindowMutationAttempt`, `WindowMutationPath`, or `WindowState`.

- [ ] **Step 3: Add mutation types**

Append these definitions to `crates/auv-driver/src/window.rs` after `ObservedWindows`:

```rust
use std::time::Duration;

use crate::input::DisturbanceLevel;
use crate::geometry::{Point, Size};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowMutationOptions {
  pub policy: WindowMutationPolicy,
  pub strategy: WindowMutationStrategy,
  pub settle: Duration,
  pub verification: WindowMutationVerification,
}

impl Default for WindowMutationOptions {
  fn default() -> Self {
    Self {
      policy: WindowMutationPolicy::NativePreferred,
      strategy: WindowMutationStrategy::default(),
      settle: Duration::from_millis(100),
      verification: WindowMutationVerification::default(),
    }
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowMutationPolicy {
  NativeOnly,
  #[default]
  NativePreferred,
  ForegroundPreferred,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowMutationStrategy {
  pub candidates: Vec<WindowMutationCandidate>,
}

impl Default for WindowMutationStrategy {
  fn default() -> Self {
    Self {
      candidates: vec![
        WindowMutationCandidate::AxWindowAttribute,
        WindowMutationCandidate::AxWindowAction,
      ],
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowMutationCandidate {
  AxWindowAttribute,
  AxWindowAction,
  PlatformNative,
  ForegroundSystemEvents,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowMutationVerification {
  FrameTolerance { points: f64 },
  BestEffortState,
}

impl Default for WindowMutationVerification {
  fn default() -> Self {
    Self::FrameTolerance { points: 2.0 }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowMutationPath {
  AxWindowAttribute,
  AxWindowAction,
  PlatformNative,
  ForegroundSystemEvents,
  Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowMutationAttempt {
  pub path: WindowMutationPath,
  pub succeeded: bool,
  pub message: Option<String>,
}

impl WindowMutationAttempt {
  pub fn success(path: WindowMutationPath, message: impl Into<String>) -> Self {
    Self {
      path,
      succeeded: true,
      message: Some(message.into()),
    }
  }

  pub fn failure(path: WindowMutationPath, message: impl Into<String>) -> Self {
    Self {
      path,
      succeeded: false,
      message: Some(message.into()),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowState {
  pub is_minimized: Option<bool>,
  pub is_visible: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowMutationKind {
  MoveTo { point: Point },
  Resize { size: Size },
  SetFrame { frame: Rect },
  Minimize,
  Restore,
  Zoom,
}
```

If the new imports duplicate existing imports, merge them into the existing `use crate::geometry::{...}` line instead of adding a second import block.

- [ ] **Step 4: Re-export types**

Update `crates/auv-driver/src/lib.rs`:

```rust
pub use window::{
  ObservedWindows, Window, WindowMutationAttempt, WindowMutationCandidate,
  WindowMutationKind, WindowMutationOptions, WindowMutationPath, WindowMutationPolicy,
  WindowMutationResult, WindowMutationStrategy, WindowMutationVerification, WindowRef,
  WindowState,
};
```

Add these type-name checks to `public_api_exports_agreed_driver_names`:

```rust
let _window_mutation_options = WindowMutationOptions::default();
let _window_mutation_attempt =
  WindowMutationAttempt::success(WindowMutationPath::AxWindowAttribute, "ok");
let _ = std::any::type_name::<WindowMutationCandidate>();
let _ = std::any::type_name::<WindowMutationKind>();
let _ = std::any::type_name::<WindowMutationPolicy>();
let _ = std::any::type_name::<WindowMutationResult>();
let _ = std::any::type_name::<WindowState>();
```

- [ ] **Step 5: Run tests**

Run:

```bash
cargo test -p auv-driver window_mutation
cargo test -p auv-driver public_api_exports_agreed_driver_names
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/auv-driver/src/window.rs crates/auv-driver/src/lib.rs
git commit -m "feat(auv-driver): add window mutation contract"
```

### Task 2: Add macOS Native AX Window Mutation Bridge

**Files:**
- Modify: `crates/auv-driver-macos/src/native/binding.rs`
- Modify: `crates/auv-driver-macos/src/native/window.rs`
- Modify: `crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/Window.swift`

- [ ] **Step 1: Add failing Rust wrapper tests**

Add tests to `crates/auv-driver-macos/src/native/window.rs`:

```rust
#[cfg(test)]
mod mutation_tests {
  use super::*;

  #[test]
  fn decode_window_mutation_rejects_native_error() {
    let response = DecodedWindowMutationResponse {
      ok: false,
      before_x: 0,
      before_y: 0,
      before_width: 0,
      before_height: 0,
      after_x: 0,
      after_y: 0,
      after_width: 0,
      after_height: 0,
      before_minimized: None,
      after_minimized: None,
      error_message: Some("AX window not found".to_string()),
      recovery_hint: Some("choose a visible app window".to_string()),
    };

    let error = decode_window_mutation_response("move_window", response)
      .expect_err("native error should surface");
    assert!(error.contains("AX window not found"));
    assert!(error.contains("choose a visible app window"));
  }
}
```

- [ ] **Step 2: Run tests to verify failure**

Run:

```bash
cargo test -p auv-driver-macos decode_window_mutation_rejects_native_error
```

Expected: FAIL with missing `DecodedWindowMutationResponse` or `decode_window_mutation_response`.

- [ ] **Step 3: Add bridge structs and externs**

In `crates/auv-driver-macos/src/native/binding.rs`, add:

```rust
#[swift_bridge(swift_repr = "struct")]
struct NativeWindowMutationRequest {
  pid: i64,
  window_number: i64,
  x: f64,
  y: f64,
  width: f64,
  height: f64,
}

#[swift_bridge(swift_repr = "struct")]
struct NativeWindowMutationResponse {
  ok: bool,
  before_x: i64,
  before_y: i64,
  before_width: i64,
  before_height: i64,
  after_x: i64,
  after_y: i64,
  after_width: i64,
  after_height: i64,
  before_minimized: Option<bool>,
  after_minimized: Option<bool>,
  error_message: Option<String>,
  recovery_hint: Option<String>,
}
```

Add extern functions:

```rust
fn move_window(request: NativeWindowMutationRequest) -> NativeWindowMutationResponse;
fn resize_window(request: NativeWindowMutationRequest) -> NativeWindowMutationResponse;
fn set_window_frame(request: NativeWindowMutationRequest) -> NativeWindowMutationResponse;
fn minimize_window(request: NativeWindowMutationRequest) -> NativeWindowMutationResponse;
fn restore_window(request: NativeWindowMutationRequest) -> NativeWindowMutationResponse;
fn zoom_window(request: NativeWindowMutationRequest) -> NativeWindowMutationResponse;
```

- [ ] **Step 4: Add Rust native wrapper**

In `crates/auv-driver-macos/src/native/window.rs`, import the bridge types/functions under the macOS cfg and add:

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeWindowMutationOutcome {
  pub before_frame: ObservedRect,
  pub after_frame: ObservedRect,
  pub before_minimized: Option<bool>,
  pub after_minimized: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedWindowMutationResponse {
  pub ok: bool,
  pub before_x: i64,
  pub before_y: i64,
  pub before_width: i64,
  pub before_height: i64,
  pub after_x: i64,
  pub after_y: i64,
  pub after_width: i64,
  pub after_height: i64,
  pub before_minimized: Option<bool>,
  pub after_minimized: Option<bool>,
  pub error_message: Option<String>,
  pub recovery_hint: Option<String>,
}

pub fn decode_window_mutation_response(
  operation: &str,
  response: DecodedWindowMutationResponse,
) -> AuvResult<NativeWindowMutationOutcome> {
  if response.error_message.is_some() || !response.ok {
    return super::error::native_result(
      operation,
      response.ok.then_some(()),
      response.error_message,
      response.recovery_hint,
    )
    .map(|_| NativeWindowMutationOutcome {
      before_frame: ObservedRect {
        x: response.before_x,
        y: response.before_y,
        width: response.before_width,
        height: response.before_height,
      },
      after_frame: ObservedRect {
        x: response.after_x,
        y: response.after_y,
        width: response.after_width,
        height: response.after_height,
      },
      before_minimized: response.before_minimized,
      after_minimized: response.after_minimized,
    });
  }

  Ok(NativeWindowMutationOutcome {
    before_frame: ObservedRect {
      x: response.before_x,
      y: response.before_y,
      width: response.before_width,
      height: response.before_height,
    },
    after_frame: ObservedRect {
      x: response.after_x,
      y: response.after_y,
      width: response.after_width,
      height: response.after_height,
    },
    before_minimized: response.before_minimized,
    after_minimized: response.after_minimized,
  })
}
```

Add public wrapper functions:

```rust
pub fn move_window_frame(
  pid: i64,
  window_number: i64,
  x: f64,
  y: f64,
  width: f64,
  height: f64,
) -> AuvResult<NativeWindowMutationOutcome> {
  window_mutation("move_window", pid, window_number, x, y, width, height)
}

pub fn resize_window_frame(
  pid: i64,
  window_number: i64,
  x: f64,
  y: f64,
  width: f64,
  height: f64,
) -> AuvResult<NativeWindowMutationOutcome> {
  window_mutation("resize_window", pid, window_number, x, y, width, height)
}

pub fn set_window_frame(
  pid: i64,
  window_number: i64,
  x: f64,
  y: f64,
  width: f64,
  height: f64,
) -> AuvResult<NativeWindowMutationOutcome> {
  window_mutation("set_window_frame", pid, window_number, x, y, width, height)
}
```

Add state wrappers:

```rust
pub fn minimize_window(
  pid: i64,
  window_number: i64,
) -> AuvResult<NativeWindowMutationOutcome> {
  window_mutation("minimize_window", pid, window_number, 0.0, 0.0, 0.0, 0.0)
}

pub fn restore_window(
  pid: i64,
  window_number: i64,
) -> AuvResult<NativeWindowMutationOutcome> {
  window_mutation("restore_window", pid, window_number, 0.0, 0.0, 0.0, 0.0)
}

pub fn zoom_window(pid: i64, window_number: i64) -> AuvResult<NativeWindowMutationOutcome> {
  window_mutation("zoom_window", pid, window_number, 0.0, 0.0, 0.0, 0.0)
}
```

Implement `window_mutation` using the extern selected by `operation`. On non-macOS, return operation-specific unsupported errors like `"macOS native window mutation is unsupported on this target"`.

- [ ] **Step 5: Add Swift AX implementation**

In `Window.swift`, add helpers:

```swift
private func axWindow(pid: Int64, windowNumber: Int64) -> AXUIElement? {
  let app = AXUIElementCreateApplication(pid_t(pid))
  var raw: CFTypeRef?
  guard AXUIElementCopyAttributeValue(app, kAXWindowsAttribute as CFString, &raw) == .success,
        let windows = raw as? [AXUIElement] else {
    return nil
  }

  for window in windows {
    if windowNumber == 0 { return window }
    var idValue: CGWindowID = 0
    if _AXUIElementGetWindow(window, &idValue) == .success,
       Int64(idValue) == windowNumber {
      return window
    }
  }
  return windows.first
}

@_silgen_name("_AXUIElementGetWindow")
private func _AXUIElementGetWindow(_ element: AXUIElement, _ identifier: UnsafeMutablePointer<CGWindowID>) -> AXError
```

Add frame/minimized readers and a response builder:

```swift
private func axWindowFrame(_ window: AXUIElement?) -> CGRect {
  guard let window else { return .zero }
  var point = CGPoint.zero
  var size = CGSize.zero
  var rawPosition: CFTypeRef?
  var rawSize: CFTypeRef?
  if AXUIElementCopyAttributeValue(window, kAXPositionAttribute as CFString, &rawPosition) == .success,
     let value = rawPosition as! AXValue?,
     AXValueGetValue(value, .cgPoint, &point),
     AXUIElementCopyAttributeValue(window, kAXSizeAttribute as CFString, &rawSize) == .success,
     let sizeValue = rawSize as! AXValue?,
     AXValueGetValue(sizeValue, .cgSize, &size) {
    return CGRect(origin: point, size: size)
  }
  return .zero
}

private func axMinimized(_ window: AXUIElement?) -> Bool? {
  guard let window else { return nil }
  var raw: CFTypeRef?
  guard AXUIElementCopyAttributeValue(window, kAXMinimizedAttribute as CFString, &raw) == .success else {
    return nil
  }
  return raw as? Bool
}
```

Implement the six extern functions:

```swift
func move_window(request: NativeWindowMutationRequest) -> NativeWindowMutationResponse {
  mutateWindow(request: request, operation: "move_window") { window in
    var point = CGPoint(x: request.x, y: request.y)
    guard let value = AXValueCreate(.cgPoint, &point) else {
      return ("failed to create AXPosition value", "retry with finite screen coordinates")
    }
    let result = AXUIElementSetAttributeValue(window, kAXPositionAttribute as CFString, value)
    return result == .success ? nil : ("AXPosition failed with \(result)", "target window may not allow programmatic movement")
  }
}
```

Use the same `mutateWindow` helper for `resize_window`, `set_window_frame`, `minimize_window`, `restore_window`, and `zoom_window`. For `zoom_window`, call:

```swift
let result = AXUIElementPerformAction(window, kAXZoomWindowAction as CFString)
```

For `minimize_window` / `restore_window`, call `AXUIElementSetAttributeValue` with `kAXMinimizedAttribute` and `true` / `false`.

- [ ] **Step 6: Generate Swift bridge and build**

Run:

```bash
hack/generate-swift-bridge
swift build --package-path crates/auv-driver-macos/native/swift
cargo test -p auv-driver-macos decode_window_mutation_rejects_native_error
```

Expected: bridge generation succeeds, SwiftPM build succeeds, Rust test passes.

- [ ] **Step 7: Commit**

```bash
git add crates/auv-driver-macos/src/native/binding.rs crates/auv-driver-macos/src/native/window.rs crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/Window.swift
git commit -m "feat(auv-driver-macos): add native window mutation bridge"
```

### Task 3: Implement WindowApi Mutation Methods

**Files:**
- Modify: `crates/auv-driver-macos/src/session.rs`

- [ ] **Step 1: Add failing policy/candidate tests**

Add tests near existing session tests:

```rust
#[test]
fn window_mutation_candidates_native_preferred_keep_strategy_order() {
  let options = auv_driver::WindowMutationOptions::default();
  let candidates = window_mutation_attempt_candidates(&options);

  assert_eq!(
    candidates,
    vec![
      auv_driver::WindowMutationCandidate::AxWindowAttribute,
      auv_driver::WindowMutationCandidate::AxWindowAction,
    ]
  );
}

#[test]
fn window_mutation_candidates_native_only_drop_foreground_events() {
  let options = auv_driver::WindowMutationOptions {
    policy: auv_driver::WindowMutationPolicy::NativeOnly,
    strategy: auv_driver::WindowMutationStrategy {
      candidates: vec![
        auv_driver::WindowMutationCandidate::ForegroundSystemEvents,
        auv_driver::WindowMutationCandidate::AxWindowAttribute,
      ],
    },
    ..auv_driver::WindowMutationOptions::default()
  };

  assert_eq!(
    window_mutation_attempt_candidates(&options),
    vec![auv_driver::WindowMutationCandidate::AxWindowAttribute]
  );
}
```

- [ ] **Step 2: Run tests to verify failure**

Run:

```bash
cargo test -p auv-driver-macos window_mutation_candidates
```

Expected: FAIL with missing `window_mutation_attempt_candidates`.

- [ ] **Step 3: Add imports**

Update `crates/auv-driver-macos/src/session.rs` imports:

```rust
use auv_driver::window::{
  WindowMutationAttempt, WindowMutationCandidate, WindowMutationKind, WindowMutationOptions,
  WindowMutationPath, WindowMutationPolicy, WindowMutationResult, WindowState,
};
```

Merge this with existing `Window` / `WindowRef` imports if rustfmt prefers a single grouped import.

- [ ] **Step 4: Add public methods**

Add to `impl WindowApi<'_>`:

```rust
pub fn move_to(
  &self,
  window: &Window,
  point: ScreenPoint,
  options: WindowMutationOptions,
) -> DriverResult<WindowMutationResult> {
  let size = window.frame.size;
  let target = point.point();
  self.mutate_window(
    window,
    WindowMutationKind::MoveTo { point: target },
    Rect::new(target.x, target.y, size.width, size.height),
    options,
  )
}

pub fn resize(
  &self,
  window: &Window,
  size: Size,
  options: WindowMutationOptions,
) -> DriverResult<WindowMutationResult> {
  self.mutate_window(
    window,
    WindowMutationKind::Resize { size },
    Rect::new(
      window.frame.origin.x,
      window.frame.origin.y,
      size.width,
      size.height,
    ),
    options,
  )
}

pub fn set_frame(
  &self,
  window: &Window,
  frame: Rect,
  options: WindowMutationOptions,
) -> DriverResult<WindowMutationResult> {
  self.mutate_window(
    window,
    WindowMutationKind::SetFrame { frame },
    frame,
    options,
  )
}

pub fn minimize(
  &self,
  window: &Window,
  options: WindowMutationOptions,
) -> DriverResult<WindowMutationResult> {
  self.mutate_window(window, WindowMutationKind::Minimize, window.frame, options)
}

pub fn restore(
  &self,
  window: &Window,
  options: WindowMutationOptions,
) -> DriverResult<WindowMutationResult> {
  self.mutate_window(window, WindowMutationKind::Restore, window.frame, options)
}

pub fn zoom(
  &self,
  window: &Window,
  options: WindowMutationOptions,
) -> DriverResult<WindowMutationResult> {
  self.mutate_window(window, WindowMutationKind::Zoom, window.frame, options)
}
```

- [ ] **Step 5: Implement candidate execution**

Add private helpers in `session.rs`:

```rust
fn window_mutation_attempt_candidates(
  options: &WindowMutationOptions,
) -> Vec<WindowMutationCandidate> {
  match options.policy {
    WindowMutationPolicy::NativeOnly | WindowMutationPolicy::NativePreferred => options
      .strategy
      .candidates
      .iter()
      .copied()
      .filter(|candidate| *candidate != WindowMutationCandidate::ForegroundSystemEvents)
      .collect(),
    WindowMutationPolicy::ForegroundPreferred => {
      vec![WindowMutationCandidate::ForegroundSystemEvents]
    }
  }
}
```

Add `mutate_window` and per-candidate methods. The AX attribute candidate should run only for move/resize/set frame/minimize/restore. The AX action candidate should run only for zoom. Unsupported candidates must append failure attempts with explicit messages.

- [ ] **Step 6: Convert native outcome to result**

Add conversion helpers:

```rust
fn rect_from_observed(rect: crate::native::types::ObservedRect) -> Rect {
  Rect::new(
    rect.x as f64,
    rect.y as f64,
    rect.width as f64,
    rect.height as f64,
  )
}

fn state_from_minimized(minimized: Option<bool>) -> WindowState {
  WindowState {
    is_minimized: minimized,
    is_visible: minimized.map(|value| !value),
  }
}
```

The successful result should use:

```rust
WindowMutationResult {
  selected_path,
  attempts,
  fallback_reason,
  before_frame: Some(rect_from_observed(outcome.before_frame)),
  after_frame: Some(rect_from_observed(outcome.after_frame)),
  before_state: Some(state_from_minimized(outcome.before_minimized)),
  after_state: Some(state_from_minimized(outcome.after_minimized)),
  focus_disturbance: DisturbanceLevel::None,
  mouse_disturbance: DisturbanceLevel::None,
}
```

- [ ] **Step 7: Run tests**

Run:

```bash
cargo test -p auv-driver-macos window_mutation_candidates
cargo test -p auv-driver-macos window_point_converts_to_screen_point
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/auv-driver-macos/src/session.rs
git commit -m "feat(auv-driver-macos): add window mutation api"
```

### Task 4: Add Command Driver Bridge and Debug Commands

**Files:**
- Modify: `src/driver/macos/typed.rs`
- Modify: `src/driver/macos/control/window.rs`
- Modify: `src/driver/macos/control/mod.rs`
- Modify: `src/driver/macos/dispatch.rs`
- Modify: `src/catalog.rs`
- Modify: `src/driver/macos/tests.rs`

- [ ] **Step 1: Add failing parser tests**

In `src/driver/macos/tests.rs`, add:

```rust
#[test]
fn parse_window_mutation_policy_accepts_native_values() {
  let cases = [
    ("native_only", auv_driver::WindowMutationPolicy::NativeOnly),
    (
      "native_preferred",
      auv_driver::WindowMutationPolicy::NativePreferred,
    ),
    (
      "foreground_preferred",
      auv_driver::WindowMutationPolicy::ForegroundPreferred,
    ),
  ];

  for (raw, expected) in cases {
    let call = build_call([("mutation_policy", raw)]);
    assert_eq!(
      super::control::window::parse_window_mutation_policy(&call).expect(raw),
      expected
    );
  }
}
```

- [ ] **Step 2: Run tests to verify failure**

Run:

```bash
cargo test parse_window_mutation_policy_accepts_native_values
```

Expected: FAIL with missing `parse_window_mutation_policy`.

- [ ] **Step 3: Add typed bridge outcome**

In `src/driver/macos/typed.rs`, add:

```rust
pub(crate) struct WindowMutationBridgeOutcome {
  pub(crate) selected_path: &'static str,
  pub(crate) fallback_reason: Option<String>,
  pub(crate) before_frame: Option<auv_driver::Rect>,
  pub(crate) after_frame: Option<auv_driver::Rect>,
  pub(crate) before_minimized: Option<bool>,
  pub(crate) after_minimized: Option<bool>,
}

impl WindowMutationBridgeOutcome {
  pub(crate) fn from_result(result: &auv_driver::WindowMutationResult) -> Self {
    Self {
      selected_path: window_mutation_path_name(result.selected_path),
      fallback_reason: result.fallback_reason.clone(),
      before_frame: result.before_frame,
      after_frame: result.after_frame,
      before_minimized: result
        .before_state
        .as_ref()
        .and_then(|state| state.is_minimized),
      after_minimized: result.after_state.as_ref().and_then(|state| state.is_minimized),
    }
  }
}

pub(crate) fn window_mutation_path_name(path: auv_driver::WindowMutationPath) -> &'static str {
  match path {
    auv_driver::WindowMutationPath::AxWindowAttribute => "ax_window_attribute",
    auv_driver::WindowMutationPath::AxWindowAction => "ax_window_action",
    auv_driver::WindowMutationPath::PlatformNative => "platform_native",
    auv_driver::WindowMutationPath::ForegroundSystemEvents => "foreground_system_events",
    auv_driver::WindowMutationPath::Unsupported => "unsupported",
  }
}
```

Add bridge functions for `move_window_bridge`, `resize_window_bridge`, `set_window_frame_bridge`, `minimize_window_bridge`, `restore_window_bridge`, and `zoom_window_bridge`. Each should open `MacosDriver`, call the corresponding `session.window()` method, and map errors with `typed macOS window mutation adapter failed: {error}`.

- [ ] **Step 4: Add command handlers**

In `src/driver/macos/control/window.rs`, add public functions:

```rust
pub(crate) fn move_window(call: &DriverCall) -> AuvResult<DriverResponse> { ... }
pub(crate) fn resize_window(call: &DriverCall) -> AuvResult<DriverResponse> { ... }
pub(crate) fn set_window_frame(call: &DriverCall) -> AuvResult<DriverResponse> { ... }
pub(crate) fn minimize_window(call: &DriverCall) -> AuvResult<DriverResponse> { ... }
pub(crate) fn restore_window(call: &DriverCall) -> AuvResult<DriverResponse> { ... }
pub(crate) fn zoom_window(call: &DriverCall) -> AuvResult<DriverResponse> { ... }
```

Use the existing app/window resolution from `click_window_point`: parse app, selector, list windows, resolve candidate, and convert to typed `Window` with `typed_window_from_ref`.

For coordinate inputs:

- `move_window`: require `x` and `y`.
- `resize_window`: require `width` and `height`.
- `set_window_frame`: require `x`, `y`, `width`, and `height`.

Return `DriverResponse` with:

- `backend: Some("macos.typed.window.mutation".to_string())`
- signals: `windowMutation.operation`, `windowMutation.selectedPath`
- notes: app, window ref/title/bounds, requested values, before/after values, selected path, fallback reason.
- one text artifact named from the operation and app.

- [ ] **Step 5: Add parsers**

In `window.rs`, add:

```rust
pub(crate) fn parse_window_mutation_policy(
  call: &DriverCall,
) -> AuvResult<auv_driver::WindowMutationPolicy> {
  match optional_string(call, "mutation_policy")
    .or_else(|| optional_string(call, "window_mutation_policy"))
    .unwrap_or_else(|| "native_preferred".to_string())
    .trim()
    .to_ascii_lowercase()
    .as_str()
  {
    "native_only" | "native-only" => Ok(auv_driver::WindowMutationPolicy::NativeOnly),
    "native_preferred" | "native-preferred" => {
      Ok(auv_driver::WindowMutationPolicy::NativePreferred)
    }
    "foreground_preferred" | "foreground-preferred" => {
      Ok(auv_driver::WindowMutationPolicy::ForegroundPreferred)
    }
    other => Err(format!(
      "invalid --mutation-policy value {other:?}; expected native_only, native_preferred, or foreground_preferred"
    )),
  }
}
```

Add `window_mutation_options(call)` returning `WindowMutationOptions` with the parsed policy and defaults.

- [ ] **Step 6: Wire command exports and dispatch**

Update `src/driver/macos/control/mod.rs`:

```rust
pub(crate) use self::window::{
  click_window_point, minimize_window, move_window, resize_window, restore_window,
  set_window_frame, zoom_window,
};
```

Update `src/driver/macos/dispatch.rs` imports and control dispatch:

```rust
"move_window" => move_window(call),
"resize_window" => resize_window(call),
"set_window_frame" => set_window_frame(call),
"minimize_window" => minimize_window(call),
"restore_window" => restore_window(call),
"zoom_window" => zoom_window(call),
```

- [ ] **Step 7: Register catalog commands**

Add these `CommandSpec`s near `debug.clickWindowPoint` in `src/catalog.rs`:

```rust
CommandSpec {
  id: "debug.moveWindow",
  namespace: ACTION,
  summary: "Move a resolved macOS app window to a screen logical point using the typed window management API.",
  driver_id: "macos.desktop",
  operation: "move_window",
  disturbance_classes: CAPTURE_AX_TREE_DISTURBANCE,
  max_disturbance: DisturbanceClass::Keyboard,
},
```

Repeat for:

- `debug.resizeWindow` -> `resize_window`
- `debug.setWindowFrame` -> `set_window_frame`
- `debug.minimizeWindow` -> `minimize_window`
- `debug.restoreWindow` -> `restore_window`
- `debug.zoomWindow` -> `zoom_window`

Use summaries that explicitly say macOS zoom is not strict maximize.

- [ ] **Step 8: Run command tests**

Run:

```bash
cargo test parse_window_mutation_policy_accepts_native_values
cargo test catalog_contains_current_macos_desktop_commands
cargo test dispatch_rejects_removed_ax_tree_operation_name
```

Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add src/driver/macos/typed.rs src/driver/macos/control/window.rs src/driver/macos/control/mod.rs src/driver/macos/dispatch.rs src/catalog.rs src/driver/macos/tests.rs
git commit -m "feat(macos): expose window management commands"
```

### Task 5: Verify Build, Formatting, and Native Bridge

**Files:**
- No source edits unless verification exposes a defect.

- [ ] **Step 1: Format**

Run:

```bash
cargo fmt
```

Expected: completes without errors.

- [ ] **Step 2: Check formatting**

Run:

```bash
cargo fmt --check
```

Expected: PASS.

- [ ] **Step 3: Run Rust checks**

Run:

```bash
cargo check
cargo test -p auv-driver
cargo test -p auv-driver-macos
cargo test
```

Expected: PASS. If macOS native tests require local permissions, record the exact failure and verify the pure Rust tests still pass.

- [ ] **Step 4: Regenerate and build Swift bridge**

Run:

```bash
hack/generate-swift-bridge
swift build --package-path crates/auv-driver-macos/native/swift
```

Expected: bridge generation succeeds and SwiftPM sees the generated bridge types.

- [ ] **Step 5: Run command smoke checks**

Run:

```bash
cargo run --quiet -- list-commands
cargo run --quiet -- debug.listWindows --limit 5
```

Expected: `list-commands` includes the new `debug.*Window` commands. `debug.listWindows` still returns visible windows or the same permission-related error it returned before this work.

- [ ] **Step 6: Commit verification fixes if needed**

If verification required source changes:

```bash
git add <changed-files>
git commit -m "fix: address window management verification"
```

If no changes were required, do not create an empty commit.

## Self-Review

- Spec coverage: The plan covers public API shape, configurable candidates, macOS AX-backed implementation, debug commands, unsupported non-macOS behavior, and verification.
- Scope: Pointer drag and strict maximize remain excluded. The plan does not introduce a public AX-facing API.
- Type consistency: Public result type is consistently `WindowMutationResult`; delivery attempts use `WindowMutationAttempt`; input delivery still uses `InputActionResult`.
- Verification: The final task includes Rust tests, Swift bridge generation, SwiftPM build, and command smoke checks.
