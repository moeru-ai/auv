# macOS Swift Bridge Migration Design

## Context

The current macOS driver executes several Swift capabilities by rendering Swift
source templates into temporary files and invoking `xcrun swift`. That shape was
useful for PoC work, but it makes Swift source a runtime artifact and creates a
time-of-check/time-of-use surface around temporary source files.

AUV should treat Swift as a first-party macOS driver implementation detail, not
as a runtime scripting substrate. The repository is still pre-release and does
not need to preserve the dynamic Swift script execution model.

## Goals

- Replace runtime Swift source generation with compiled macOS Swift bridge
  calls.
- Gate the Swift bridge by target OS, not by a user-facing feature flag.
- Keep the shared driver model portable for future Linux, Windows, Android,
  iOS, and Web backends.
- Use typed Rust/Swift bridge boundaries for driver payloads instead of JSON
  tunnels.
- Preserve AUV's inspectable command behavior through structured reports and
  artifacts.
- Avoid making `swift-bridge` a top-level architecture concept or command
  namespace.

## Non-Goals

- Do not design non-macOS driver implementations in this phase.
- Do not introduce a user-facing Cargo feature for choosing the macOS bridge.
- Do not preserve runtime `*.swift` script execution as a supported backend.
- Do not introduce temporary compatibility shims for the old dynamic Swift
  script path.

## Dependency and Gating Model

`swift-bridge` is a macOS-only implementation dependency.

Cargo dependencies should be target-specific:

```toml
[target.'cfg(target_os = "macos")'.dependencies]
swift-bridge = "0.1.59"

[target.'cfg(target_os = "macos")'.build-dependencies]
swift-bridge-build = "0.1.59"
```

If the repository needs a `build.rs`, it must guard Swift bridge generation with
`CARGO_CFG_TARGET_OS == "macos"`. Non-macOS targets must not require Swift,
Xcode, Apple frameworks, generated Swift bridge files, or macOS linker flags.

Rust source should use `#[cfg(target_os = "macos")]` around bridge modules and
macOS backend implementations. Cross-platform model types, command specs, and
runtime APIs should remain outside that gate where practical.

## Architecture

The public driver surface remains capability-oriented:

```text
screen
window
ax_tree
pointer
keyboard
clipboard
app
permission
```

The Swift bridge should not become a broad module namespace. Most macOS code
should live under capability modules such as `screen`, `window`, `ax_tree`,
`keyboard`, and `clipboard`. Only the generated interop boundary and the
smallest possible wrapper should live in an FFI-oriented module.

The preferred structure is:

```text
src/driver/macos/
  native/
    ffi.rs
    screen.rs
    window.rs
    ax_tree.rs
    pointer.rs
    keyboard.rs
    clipboard.rs
    permission.rs
```

`native/ffi.rs` owns the `#[swift_bridge::bridge]` declarations and binding
DTOs. The sibling files convert between those DTOs and the capability modules.
Use `native`, `binding`, or `ffi` for this layer. Avoid making `bridge` a broad
module namespace, and never leak `swift_bridge` into command names, capability
names, or shared AUV concepts.

## Bridge API Shape

The bridge should use typed request and response shapes instead of passing JSON
strings through the FFI boundary. This keeps the migration aligned with
`swift-bridge`'s value: compile-time interface generation, explicit ownership,
and low-overhead cross-language calls.

Simple scalar calls can stay scalar. Complex operations should use
`swift-bridge` structs and enums for request and response data.

Example shape:

```rust
#[swift_bridge::bridge]
mod ffi {
  struct ListWindowsRequest {
    limit: u64,
    app_filter: String,
  }

  struct ListWindowsResponse {
    observed_at_unix_ms: u64,
    candidates: Vec<WindowCandidate>,
  }

  struct WindowCandidate {
    window_ref: String,
    native_window_id: i64,
    owner_bundle_id: String,
    owner_pid: i64,
    title: String,
    bounds: RectI64,
    display_ref: String,
    selection_reason: String,
  }

  struct RectI64 {
    x: i64,
    y: i64,
    width: i64,
    height: i64,
  }

  extern "Swift" {
    fn list_windows(request: ListWindowsRequest) -> ListWindowsResponse;
    fn capture_ax_tree(request: CaptureAxTreeRequest) -> CaptureAxTreeResponse;
    fn find_image_text(request: FindImageTextRequest) -> FindImageTextResponse;
    fn click_point(x: f64, y: f64, button: i32, click_count: u64) -> String;
    fn probe_permissions() -> PermissionProbeResponse;
  }
}
```

Rust remains responsible for converting bridge responses into AUV command
responses, artifacts, and trace events. Swift remains responsible for direct
Apple framework calls.

If a field is still provisional, model it as an explicit provisional field in a
typed struct rather than hiding it in an untyped JSON payload.

Before broad migration, implement a small compatibility spike that proves the
typed bridge shape works in this repository:

```text
list_windows(ListWindowsRequest) -> Result<ListWindowsResponse, NativeDriverError>
```

The spike must verify nested structs, collection representation, `Option<String>`,
and error handling. A local spike on macOS with Swift 6.3.2 and
`swift-bridge 0.1.59` verified:

- typed Rust -> Swift calls through build.rs and `swiftc`
- nested transparent structs
- `Option<String>` crossing into Rust as `Option<String>`
- `Vec<String>` crossing as Swift `RustVec<RustString>`
- `Result<T, E>` crossing as Swift typed `throws(E)`

The same spike found that `Vec<WindowCandidate>` where `WindowCandidate` is a
transparent shared struct did not compile because the generated Swift type did
not conform to `Vectorizable`. Do not assume `Vec<transparent struct>` works for
candidate lists. Prefer one of these shapes until a stronger spike proves
otherwise:

- split repeated fields into supported vectors when the shape is simple
- use opaque native types with explicit accessors where lifetime semantics are
  acceptable
- use a macro-assisted Rust decode layer for repeated structured records

If nested typed responses fail in practice, the fallback should not be a raw JSON
tunnel by default. Prefer a small Rust macro-assisted decode layer that keeps a
declared response schema near the FFI boundary and centralizes validation,
conversion, and error reporting.

## Build Toolchain Shape

For Rust-calls-Swift in this repository, the verified shape is:

```text
build.rs
-> swift_bridge_build::parse_bridges(...)
-> generated SwiftBridgeCore.{h,swift} and crate bridge files
-> swiftc -emit-library -static -parse-as-library
-> cargo links the produced static Swift library
```

The spike did not require a Swift sub-package. `swift_bridge_build::create_package`
exists for bundling generated bridge code and compiled Rust static libraries
into a Swift Package, which is useful when Swift/Xcode is the consumer. That is
not the shortest path for AUV's current Rust binary calling macOS Swift code.

The implementation should start with direct `swiftc` compilation from build.rs.
Introduce a Swift Package only if the direct static-library path becomes
unmaintainable or if AUV later needs to expose the Rust library to Swift/Xcode
consumers.

The completed migration keeps `build.rs` as the authority for compiling the
bridge, but organizes the Swift implementation with a SwiftPM package skeleton
created by `swift package init --type library --name AuvMacosNative`:

```text
src/driver/macos/native/swift/
  Package.swift
  Sources/
    AuvMacosNative/
      Support.swift
      Permission.swift
      Window.swift
      AxTree.swift
      Ocr.swift
      Pointer.swift
      Clipboard.swift
```

`Package.swift` declares a static `AuvMacosNative` library target and is useful
for standard Swift source layout and manifest inspection. Cargo still invokes
`swiftc` directly because the build needs swift-bridge generated files from
`OUT_DIR`; standalone `swift build` is not the current integration contract.

## Foundation Slice Status

The first implementation slice establishes the macOS-only native binding
toolchain and migrates screen-recording/accessibility permission probes to a
compiled Swift bridge call.

The remaining runtime Swift script migrations are intentionally deferred:

- window listing
- AX tree capture
- OCR text matching and visual row detection
- pointer click and scroll
- clipboard snapshot/set/restore
- native keyboard input

The foundation slice keeps `osascript` only for Automation/System Events probing
until the separate osascript backend design is implemented.

Implementation notes from the foundation slice:

- `build.rs` compiles the Swift bridge only for macOS targets and requires a
  macOS host with `swiftc` when that target is selected.
- Swift sources live in the SwiftPM target directory
  `src/driver/macos/native/swift/Sources/AuvMacosNative`; `build.rs` discovers
  `*.swift` files from that target directory and compiles them with the
  swift-bridge generated sources.
- The native binding code lives under `src/driver/macos/native`; `ffi` remains
  private, while capability wrappers such as `native::permission` expose the
  Rust-facing API.
- `native::permission` keeps non-macOS targets free of Swift bridge symbols, but
  calling the native permission probe outside macOS returns an unsupported
  error instead of a synthetic successful probe.
- Sandboxed Swift builds may fail if `swiftc`/clang cannot write its module
  cache under the user's cache directory; tests that compile the bridge need
  permission to use the host Swift toolchain cache.

## Implementation Plans

This design was implemented through two historical plan slices:

- macOS Swift bridge foundation: Cargo/build integration,
  `src/driver/macos/native`, and the permission probe migration.
- macOS Swift bridge migration completion: display/window, AX tree, OCR,
  pointer, clipboard, keyboard decision, script removal, and final live
  validation.

Current status: foundation slice completed; completion plan executed for the
runtime Swift source removal scope.

## Keyboard Backend Decision

Keyboard input remains outside the Swift bridge migration for now. The current
typing and shortcut paths use System Events through `osascript`, not runtime
Swift source execution, so migrating them in this design would mix the Swift
bridge cleanup with the separate osascript backend design. Native keyboard input
can be revisited when the project decides the best per-platform input backend;
this migration only removes the runtime Swift source execution surface.

## Migration Completion Status

- Foundation permission probe: completed. Screen Recording and Accessibility
  checks use the compiled Swift bridge through `native::permission`; Automation
  probing remains with osascript/System Events.
- Display/window native bridge: completed. Display enumeration, window listing,
  and xcap bundle-id lookup now use `native::window`.
- AX tree native bridge: completed. AX tree capture and AX-dependent command
  paths now use `native::ax_tree`.
- OCR native bridge: completed. Vision OCR text matching and visual row
  detection now use `native::ocr`.
- Pointer native bridge: completed. Click and scroll event generation now use
  `native::pointer`.
- Clipboard native bridge: completed. Clipboard snapshot, set, and restore now
  use `native::clipboard`; Rust still owns the clipboard lock.
- Keyboard backend decision: deferred to the osascript/native input backend
  design because current keyboard paths are not runtime Swift source execution.
- Runtime Swift source execution: removed from the macOS driver. `swiftc`
  remains only in `build.rs` for build-time bridge compilation.

Live validation run during migration:

- `cargo run --quiet -- invoke debug.probePermissions`
- `cargo run --quiet -- invoke debug.listDisplays`
- `cargo run --quiet -- invoke debug.listWindows`
- `cargo run --quiet -- invoke debug.observeAxTree`
- `cargo run --quiet -- invoke debug.captureWindow --target Code`
- `cargo run --quiet -- invoke debug.findImageText --target '{}'`
  failed at Rust input validation because `query` was intentionally omitted for
  the smoke check.
- `cargo run --quiet -- invoke debug.clickPoint --x 1 --y 1`
- `cargo run --quiet -- invoke debug.pasteTextPreserveClipboard --text 'auv clipboard bridge smoke'`

`debug.pressKey` live validation was not run because it sends real keyboard
input to the frontmost app and keyboard migration is intentionally deferred.

## Migration Scope

The migration should cover Swift-backed code paths currently represented by
runtime Swift scripts:

- window listing via CoreGraphics window APIs
- AX tree capture and AX-related probes
- OCR text matching and visual row detection
- pointer click and scroll event generation
- clipboard capture, set, and restore
- keyboard input where Swift or Rust-native event generation is the best
  implementation
- screen recording and accessibility permission probes

`osascript`-backed app or application-specific automation is covered by a
separate osascript backend design.

## Artifact and Error Model

Swift bridge calls should return typed reports that Rust converts into AUV
artifacts and command summaries. Errors should include backend identity,
operation name, and enough context to recommend a next step.

Reports should preserve fields needed by the inspect server and future viewer:

```text
backend
operation
scope
captureSource
displayRef
windowRef
region
matchBounds
logicalPoint
selectionReason
failureReason
```

Swift should not write durable AUV run artifacts directly. Rust remains
responsible for artifact naming, copying, run storage integration, and trace
events.

## Security Notes

The migration removes the runtime Swift source file injection surface. It does
not remove all macOS automation risk. Swift bridge calls can still exercise
Accessibility, Screen Recording, and input-event permissions. Those calls must
remain exposed only as fixed AUV driver operations, not arbitrary user-provided
Swift or Apple framework calls.

Generated bridge files should be treated as build artifacts. Runtime execution
should not depend on writable source files in shared temporary directories.

## Testing

Unit tests should cover:

- non-macOS builds do not compile or link Swift bridge modules
- bridge typed request and response conversion for complex calls
- the typed spike for nested structs, vectors, options, and errors
- bridge response parsing and error mapping
- command artifact creation from bridge reports
- command catalog registration after migration

Live macOS validation should include:

- listing windows
- capturing an AX tree
- OCR over a captured image
- click and scroll through the bridge backend
- clipboard capture/set/restore
- permission probe behavior on missing and granted permissions
