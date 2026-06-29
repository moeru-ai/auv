# Window Management API V0

This note records the first AUV window management slice landed on 2026-06-05.
It covers driver-level window geometry and coarse state mutation, not pointer
drag or interaction dragging.

## Landed Surface

- `auv-driver::WindowMutationKind`
  - `MoveTo { point }`
  - `Resize { size }`
  - `SetFrame { frame }`
  - `Minimize`
  - `Restore`
  - `Zoom`
- `auv-driver::WindowMutationOptions`
  - policy: `NativeOnly`, `NativePreferred`, `ForegroundPreferred`
  - strategy candidates: AX attribute, AX action, platform native, foreground
    system events
  - settle delay
  - verification: frame tolerance or best-effort state
- `auv-driver::WindowMutationResult`
  - selected path
  - attempts
  - fallback reason
  - before/after frame
  - before/after minimized/visible state
  - focus and mouse disturbance metadata
- `auv-driver-macos::WindowApi`
  - `move_to(&window, point, options)`
  - `resize(&window, size, options)`
  - `set_frame(&window, frame, options)`
  - `minimize(&window, options)`
  - `restore(&window, options)`
  - `zoom(&window, options)`

No legacy `src/driver/macos` debug command surface is part of this slice.
Window management callers should use `auv-driver-macos::WindowApi` directly
until the legacy command layer is retired or a new non-legacy frontend is
approved.
  - actions: `move_to`, `resize`, `set_frame`, `minimize`, `restore`, `zoom`

## macOS Backend

The macOS implementation uses Accessibility APIs through
`crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/Window.swift`.

Geometry mutations use:

- `kAXPositionAttribute`
- `kAXSizeAttribute`

State/action mutations use:

- `kAXMinimizedAttribute`
- `kAXZoomButtonAttribute` + `kAXPressAction`

The native bridge resolves AX windows by pid and native window id. Because
ordinary AX windows do not reliably expose a public `AXWindowNumber` attribute,
the bridge uses the private `_AXUIElementGetWindow` symbol through `dlsym` at
the native boundary. A positive `window_number` is authoritative: if it cannot
be matched, the bridge fails instead of falling back to title matching. Title
matching is only used when no native window id is requested.

## Verification

The typed macOS API verifies successful native results before returning success.

- Move and set-frame verify position within the configured frame tolerance.
- Resize and set-frame verify size within the configured frame tolerance.
- Minimize verifies `is_minimized == true`.
- Restore verifies `is_minimized == false`.
- Zoom is currently best-effort because apps differ in how they expose zoomed
  state.

Native failures preserve attempt messages so stale window ids, AX rejections,
and recovery hints are not collapsed into a generic unsupported error.

## Intentional Deferrals

- Foreground/system-events window mutation fallback is not implemented. It is
  represented as an explicit candidate/attempt and requires owner approval
  before pointer or foreground repositioning is added.
- Drag-to-screen, drag-for-repositioning, and interaction drag remain outside
  this slice.
- No public AX-facing API was introduced. AX remains an internal macOS delivery
  path for this slice.

## Validation Commands

- `cargo test`
- `cargo test -p auv-driver`
- `cargo test -p auv-driver-macos`
- `cargo check`
- `scripts/generate-swift-bridge`
- `swift build` in `crates/auv-driver-macos/native/swift`
- `git diff --check`

`cargo fmt --check` currently reports pre-existing formatting drift in files
outside this slice:

- `crates/auv-netease-music/src/view_parsers/sidebar/region.rs`
- `src/app/mod.rs`
