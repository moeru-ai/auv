# Driver Capability Crate Split

Status: the owner accepted seven capabilities on 2026-11-09, then clarified the
package and behavior boundaries on 2026-11-10. This extraction covers existing
pixel capture on Linux, macOS, and Windows. It does not redesign Runner or
session execution.

Change classification: narrow refactor. Evidence levels are source inspection,
automated tests, and native build checks where recorded below. Moving code does
not establish new platform support.

## Accepted boundaries

Each capability has two crates:

| Crate | Responsibility |
| --- | --- |
| `auv-driver-<capability>-common` | Traits, request/result types, invariants, and portable contracts or shared algorithms |
| `auv-driver-<capability>` | Concrete drivers, native dependencies, and implementation resources |

The implementation crate can contain several drivers for the same platform.
Cargo features select which implementations and dependencies compile. They do
not select a driver, change a fallback order, or create a session.
The caller chooses its driver and any composition policy explicitly.

```text
auv-driver-capture-common/
  src/lib.rs          Capture types, backend traits, resolution and origin rules

auv-driver-capture/
  src/linux/          ScreenCast/PipeWire, Screenshot, and window crop policy
  src/macos/          xcap and ScreenCaptureKit
  src/windows/        xcap, PrintWindow, WGC, and native frame streams
  native/swift/       ScreenCaptureKit Swift package and generated bridge setup
```

The existing overlay family remains unchanged by this capture slice. Its
platform packages do not require separate platform packages for capture.
The earlier single-crate capture consolidation and independent Linux capture
handle were intermediate changes. This note describes their replacement.

## Capability inventory

| Capability | Owns |
| --- | --- |
| Input | Pointer and keyboard delivery, window-targeted input, holds, cancellation, timed motion, fallback policy, and delivery evidence |
| Capture | Window/display/region pixels, capture origins, resolution conversion, acquisition fallbacks, and native frame sources |
| Overlay | Layers, styles, animation, rendering resources, and presentation thread rules |
| Desktop | One capability with distinct window and display APIs: discovery, resolution, state, focus, frame changes, restore history, and display topology |
| Clipboard | Contents, snapshots, restore, ownership, delayed rendering, and transfer lifetime |
| Accessibility | Native trees, node queries, AX/UIA/AT-SPI actions, and semantic readback |
| Vision | OCR, text matching, and existing image matching; pixel acquisition remains with capture |

Window and display are one capability. Its provisional crate name is
`auv-driver-desktop`. Placement of application resolution and activation in its
`application` module remains provisional. Media retains its existing owner.
Permission probes stay with the capability or native boundary that owns them.
Combined readiness reports remain operation composition responsibilities.

Shared contracts retain one owner in `auv-driver-common`: geometry, coordinate
spaces, target identity, `Window`, `Display`, `DriverError`, and input-delivery
evidence. In particular, `DriverError::InputAction` and accessibility actions use
canonical `InputActionProgress` and `InputActionResult` types.

## Behavior preservation

The owner requires existing driver, Runner, and session behavior to remain
unchanged. The comparison baseline is `7a0a7839`, before this branch's extraction.

Preserve these boundaries:

- Existing operations, arguments, results, errors, metadata, and backend tags.
- Existing default backend selection, fallback order, and unsupported behavior.
- Session resource ownership, synchronization, lazy initialization, authorization,
  cancellation, retry, timeout, and cleanup policies.
- Process-wide WGC caches, worker coordination, telemetry, and desktop attachment.
- Runner service routing, retained pixels, capture references, and run artifacts.
- Existing public aggregate paths through canonical re-exports where necessary.

Linux capture again uses the aggregate session's original mutex and optional
ScreenCast session. The session opens its native source at the original call
site, then passes frame acquisition to the extracted algorithm. Display
discovery precedes that lock, and Screenshot fallback follows its release.
Explicit authorization retains the original input/capture ordering and lifetime.

Runner display capture, text search, and recent-frame producers call the
existing session APIs. They do not retain an independent capture handle.
The two Runner capture files match the comparison baseline.

macOS activation and settling stay in the session operation. Capture retains its
Swift-first window policy, transformed-frame retry, AX size check, stale-target
errors, and xcap fallback. The session supplies its existing accessibility query
at the actual native service boundary. Display exclusion still fails without an
unfiltered fallback.

Windows keeps PrintWindow window capture, xcap display/region capture, and the
explicit WGC operations. WGC frame draining, cache keys, freshness, worker
recovery, and session lifetimes retain their existing algorithms. Aggregate
paths re-export the moved implementation and share its statics.

Independent capability lifetimes, new backend-selection policy, and further
Runner/session decoupling require separate owner-approved work.

## Capture contracts and drivers

[`auv-driver-capture-common`](../../../../crates/auv-driver-capture-common/src/lib.rs)
owns `Capture`, `CaptureOptions`, `CaptureResolution`, `Activation`,
`DisplayCapture`, `RegionCapture`, and `ImageView`. Its three backend traits are
`WindowCaptureDriver`, `DisplayCaptureDriver`, and `RegionCaptureDriver`.
A driver implements only the operations it provides.

[`auv-driver-capture`](../../../../crates/auv-driver-capture/src/lib.rs) re-exports
these contracts and contains the concrete implementations. It has no dependency
on an aggregate driver, Runner, Protobuf, or CLI crate. Its default feature set
contains no native backend.

| Feature | Implementation |
| --- | --- |
| `linux-screencast` | Existing Portal ScreenCast session and PipeWire frame receiver |
| `linux-screenshot` | Existing Screenshot portal source and geometry checks |
| `macos-xcap` | Existing xcap window, display, and region capture |
| `macos-screencapturekit` | Existing Swift capture and native image response decoding |
| `windows-xcap` | Existing xcap display and region capture |
| `windows-printwindow` | Existing GDI window capture and visible-frame crop |
| `windows-wgc` | Existing WGC acquisition, health checks, caches, prewarming, and frame streams |
| `latency-telemetry` | Existing Windows telemetry buffer and recording controls |

Native dependencies also have target conditions. Enabling a Windows feature on
Linux does not compile Windows native libraries. Each existing platform driver
explicitly enables its original backend set. Its existing public operations
retain their original backend policy.

A direct caller can select one backend:

```toml
[dependencies.auv-driver-capture]
version = "0.0.43"
default-features = false
features = ["windows-printwindow"]
```

```rust
use auv_driver_capture::windows::PrintWindow;
use auv_driver_capture::{CaptureResolution, WindowCaptureDriver};

let capture = PrintWindow.capture_window(&window, CaptureResolution::Native)?;
```

Linux `ScreenCastSession` implements display and region capture without implicit
Screenshot fallback. The existing aggregate calls an explicit composition that
retains its current Screenshot fallback. Compiling both sources alone does not
change a direct source's selection.

ScreenCaptureKit accepts the host's AX size query for mismatch classification.
Its native display-id capture remains available independently. The existing
self-excluding display operation also uses xcap target resolution.
WGC window capture and frame streams work without xcap. Its existing display
operation requires `windows-xcap` for target discovery. These dependencies retain
existing resolver behavior rather than introduce a new discovery backend.

## Shared native prerequisites

Linux display discovery remains in `auv-driver-desktop-linux`, shared by capture,
uinput, and the existing display API. Portal request handling, connection identity,
and restore-token persistence remain in `auv-driver-portal-linux`, shared by
capture, input, and clipboard. These packages do not own session orchestration.

Windows read-only target probes, desktop attachment, and telemetry have one
implementation beside the extracted native backends. Existing driver modules
re-export them, so input and capture share the same native and diagnostic state.
A `NOTICE:` marker records their later ownership decision during desktop/input
extraction. Duplicating their caches, buffers, or thread-local state is not an
acceptable migration strategy.

The macOS Swift capture package owns capture declarations and image acquisition.
The aggregate package retains its existing AX window-size reader and OCR's
RGBA-to-image helper. Shared native error formatting and bulk byte copying have
one implementation in capture-common's existing macOS contract namespace.
The two generated FFI wrappers use distinct C symbols for byte transfer.

The former macOS serialized capture contracts keep their shapes and remain
available through the old module paths. Their types, geometry, and rendering
logic now have one owner in capture-common. They are not a replacement for the
canonical `Capture` result.

## Rust path migration

Use the contract crate when only types or traits are needed:

```rust
use auv_driver_capture_common::{Capture, CaptureOptions, CaptureResolution};
```

Use the implementation crate to construct or call a backend. The aggregate
`auv-driver` retains its root capture exports while its callers migrate.
Existing macOS and Windows public native paths re-export the moved code.
Re-exports contain no duplicate implementation and have explicit migration
markers. `auv-driver-common` cannot re-export capture types without a dependency
cycle; its former capture module remains removed.

## Validation

Host Clang 23 produces an opaque `spa_pod_builder` with bindgen 0.69. Linux
validation uses the isolated Clang 18 library in `/tmp/auv-clang18`, through
`LIBCLANG_PATH`. Installed tools and project dependency versions are unchanged.
The library wheel SHA-256 is
`c533091d8a3bbf7460a00cb6c1a71da93bffe148f172c7d03b1c31fbf8aa2a0b`.

Relevant checks include:

```sh
LIBCLANG_PATH=/tmp/auv-clang18 cargo check --offline
LIBCLANG_PATH=/tmp/auv-clang18 cargo test --offline \
  -p auv-cli -p auv-core -p auv-cli-invoke -p auv-scan \
  -p auv-driver -p auv-driver-common \
  -p auv-driver-capture-common -p auv-driver-capture \
  -p auv-driver-desktop-linux -p auv-driver-portal-linux \
  -p auv-driver-linux -p auv-driver-macos -p auv-driver-windows
cargo check -p auv-driver-capture --no-default-features
cargo check -p auv-driver-capture --no-default-features --features linux-screencast --all-targets
cargo check -p auv-driver-capture --no-default-features --features linux-screenshot --all-targets
cargo test -p auv-driver-windows --features latency-telemetry --test capture_capability
cargo fmt --all --check
git diff --check
```

CI checks each native backend feature independently on its platform. macOS also
runs `scripts/generate-swift-bridge` and SwiftPM builds for the capture and
aggregate native packages. Generated sources remain ignored.

Existing tests move with pixel conversion, geometry, window binding, capture
response decoding, native fallback, WGC recovery, frame streams, and resource
cleanup. Added boundary tests cover window-read ordering and the shared Windows
telemetry buffer. Live desktop tests remain opt-in; compilation and unit tests
do not establish live capture support.

The affected Linux-host suites passed with 1,121 tests, zero failures, and 13
ignored tests. The telemetry integration test also passed with its feature
enabled. Each backend feature passed an isolated Linux-host build. Windows
xcap, PrintWindow, and WGC also passed isolated Windows GNU target checks,
including test compilation. The aggregate Windows driver also passed its
Windows GNU target check. Both changed Swift bridges generated with the
pinned generator, and the capture source passed Swift syntax parsing.
Native macOS framework builds and live desktop behavior require platform hosts.

A source comparison confirms that WGC, desktop attachment, and telemetry are
unchanged after import, path, and feature-gate substitution. Linux driver state
and both Runner capture files match the comparison baseline. Linux session code
differs only in its capture-contract import.

## Remaining work

TODO(capability-extraction): extract the other accepted capabilities in separate
owner-approved slices. Their producer/consumer paths and native lifetimes need
concrete boundaries before moving code.

TODO(capability-session-migration): revisit independent capability construction
and aggregate-session removal only after a separate behavior proposal is accepted.
This capture extraction preserves existing session policy.

TODO(capability-desktop-application): finalize application resolution and
activation ownership during desktop extraction. Their proposed placement is
provisional.
