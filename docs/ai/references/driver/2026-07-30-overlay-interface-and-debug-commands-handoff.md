# Overlay interface and debug commands handoff

Date: 2026-07-30

## Implemented interface

The `auv-driver-overlay` crate is the public facade for the fluent,
renderer-independent contract:

```rust
Overlay::new()
  .with_layer(Outline::new(rect).with_style(style))
  .with_layer(ClickTarget::new(point).with_status("delivered"))
```

Layers own content and geometry. Typed `OutlineStyle`, `CursorStyle`, and
`StatusStyle` values own appearance. Presets are builder starting points, so
later setters deterministically refine them before the completed style is
attached to a layer.

Public string layer ids were removed. The macOS adapter generates stable
kind-relative ids while rendering an overlay, retaining native cursor
position inheritance without leaking AppKit window bookkeeping.

Outline, cursor, and capture-frame labels now default to hidden even when label
content is stored. Callers opt in with `with_label_visible()`. `ClickTarget`
keeps outline and cursor labels independent through
`with_outline_label(...)` / `with_cursor_label(...)` and their corresponding
visibility methods.

## Shared components

- `CaptureFrame`: one capture-styled outline.
- `ClickTarget`: an optional selected outline followed by cursor and optional
status layers.

Both primitives and composites implement `IntoOverlayLayers`, so callers use
one `with_layer(...)` method. Composite types are normalized before reaching a
platform renderer and are not variants of the renderer-facing `Layer` enum.

Production display/window capture and click presentation use these components.

`auv_driver::overlay::ShowOptions` owns the public motion and lifecycle policy.
Its common shortcut is:

```rust
ShowOptions::new()
  .with_motion_ease(Duration::from_millis(360), Easing::EaseInOutExpo)
  .with_auto_removal_after(Duration::from_millis(240))
```

The public driver capability uses `show` / `remove`; the macOS adapter uses
`render` / `remove`. Showing returns `()`: layer counts are already available
from `Overlay::layers()` and are not repeated in a result type.

The dependency direction is facade-first:

```text
auv-driver-overlay
├── auv-driver-overlay-common
└── auv-driver-overlay-macos  (feature = "macos")
    └── auv-driver-overlay-common
```

`auv-driver-overlay-common` owns renderer-independent types. Platform adapters
depend only on that common crate. Consumers depend on the facade, which
re-exports the common contract and exposes `show` / `remove`; they do not need
to select or call a platform adapter directly.

Invoke commands retain one local driver session across their capture, OCR, or
input operation and overlay display. The invoke overlay helper accepts that
session rather than opening another one. Debug-only `overlay.*` commands have
no preceding driver operation, so they open one session on demand when live
overlay display is enabled.

## Invoke visual inspection

The registered visual-only commands are:

- `overlay.outline`
- `overlay.cursor`
- `overlay.status`
- `overlay.captureFrame`
- `overlay.clickTarget`

They accept geometry, timing, and applicable style overrides. `overlay.cursor`
also accepts runtime SVG source. Debug commands use the same shared layer and
component constructors as production invoke commands.

## Evidence

- `cargo check -p auv-cli-invoke`
- `cargo test -p auv-driver-common -p auv-cli-invoke -p auv-driver-overlay-macos --lib` (89 tests)
- regenerated Swift bridge files with `target/debug/auv --xtask generate-swift-bridge`
- `swift build` in `crates/auv-driver-overlay-macos/native/swift`
- live `overlay.captureFrame`: shown 1 layer
- live `overlay.clickTarget`: shown 3 layers

Per-layer update/remove handles remain intentionally deferred. The current
show interface is one-shot; add an opaque handle only when a concrete
runtime consumer needs in-process incremental updates.

## Clean display capture (2026-10-16)

Continuous capture consumers need to keep their previous status visible while
capturing the underlying application. Hiding and restoring every layer between
frames causes flicker and requires waiting for pending AppKit window changes.
`DisplayApi::capture_excluding_self(CaptureOptions)` uses macOS 14+
ScreenCaptureKit screenshots to exclude all windows owned by the capturing
process while leaving them visible on the desktop. Exclusion failures return
an error and never fall back to an unfiltered screenshot. The requested
Native/Logical resolution, logical bounds, screen origin and pixel scale are
preserved. Other processes' overlays are not excluded; same-process ordinary
windows are excluded too. This is a local Rust driver capability independent of
any game or application; it does not add an invoke/MCP/Runner operation or
overlay lifecycle API.

Window and self-excluding display capture both transfer pixels through the
existing `nativeByteVec -> native_byte_vec_from_raw` bulk-copy path. Swift
borrows its contiguous buffer only during the synchronous call; Rust owns the
returned copy. No extra transfer helper or retained pointer is introduced.

### Reproducible checks

Opt-in probes live in the driver crate and keep all screenshots in memory.
They require an awake desktop, Screen Recording permission, and a display id
from `session.display().list()`. The Logical-resolution regression additionally
requires a Retina primary display.

```sh
cargo test -p auv-driver-macos -p auv-driver-overlay-common \
  -p auv-driver-overlay-macos -p auv-driver-overlay \
  --features auv-driver-overlay/macos --lib
cargo test -p auv-driver-macos --lib \
  self_excluding_capture_honors_logical_resolution -- --ignored --nocapture
cargo run --release -p auv-driver-macos --features overlay \
  --example verify_overlay_capture -- 1
cargo run --release -p auv-driver-macos --example measure_capture -- 1 5
```

The visibility probe checks that the status marker is visible in ordinary
capture, absent from self-excluding capture, and still visible after filtered
capture. The probe services Core Foundation's main-thread run loop until the
capture worker finishes, then its cleanup guard removes the overlay. It waits
up to ten seconds for the marker to become visible before testing exclusion.
It compares marker counts with a baseline and uses display-relative regions so
nonzero display origins are supported. The unit tests cover pixel ownership,
null/empty input, image-envelope validation and native error propagation.
`measure_capture` includes system screenshot, image conversion and the Swift/FFI
path. Local timings are not portable performance guarantees.

### Local evidence (2026-10-16)

On the available 3024x1964 Retina display (1512x982 logical points), the
visibility probe reported
`baseline=0 visible=27009 excluded=0 after_capture=27009` while keeping the
overlay visible until the worker completed. The opt-in Logical resolution regression
passed through the public driver API. No screenshots were saved. This verifies
behavior on this display; it does not establish coverage of every OS or Space
transition.

Before the Swift optimization change was split into a separate PR, five
release timing samples (one cold, four warm) measured self-excluding
capture median 129.746 ms, P95 132.977 ms and cold 269.921 ms; ordinary capture
median was 35.185 ms.
These historical timings used optimized Swift code and do not describe this
capture-only branch after the split. They are not cross-machine benchmarks.

Both native Swift packages built after bridge regeneration. Driver and overlay
unit tests, example compilation, workspace formatting and diff checks passed.
The stricter `clippy -- -D warnings` check encounters existing upstream lints
(`collapsible_if` in `auv-driver-common/src/mouse_input.rs` and `needless_return`
in the existing overlay render/remove branches). Those unrelated paths are
unchanged by this slice.
