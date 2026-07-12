# macOS Capture Fast Path Design

Date: 2026-05-26

Status: Draft design, derived from
`2026-05-26-driver-capture-input-interaction-roadmap.md`.

## Purpose

This design focuses only on AUV's macOS capture performance. It deliberately
does not design Surface reconstruction, no-steal input, focus safety, or the
interaction layer.

The target outcome is a capture pipeline that can support fast verification,
OCR, scroll scan, and future visual reconstruction without forcing every
operation to synchronously write PNG files.

## Current Problems

AUV has working capture, but several paths are slower than they need to be:

- Window capture often re-enumerates windows before capturing.
- `xcap` is the dominant capture backend even when a faster native path may be
  available.
- OCR over a typed `Capture` currently writes a temporary PNG, calls native OCR
  with a file path, then removes the file.
- Driver command paths commonly write screenshot artifacts first, then runtime
  stages those artifacts into `.auv/runs` by copying them again.
- Inspect server artifact upload is synchronous from the action path when
  enabled.
- Timing data is not granular enough to tell capture cost from encode, file
  write, OCR, artifact staging, or inspect upload cost.

## Reference Findings

### CUA Rust

`trycua/cua/libs/cua-driver-rs/crates/platform-macos/src/capture.rs` uses the
macOS `screencapture` CLI:

- capture to temp PNG file
- read bytes
- optionally resize/convert to JPEG

This is simple and reliable, but it is not the performance model AUV should
copy.

### CUA Swift

`trycua/cua/libs/cua-driver/Sources/CuaDriverCore/Capture/WindowCapture.swift`
is the better reference:

- uses `ScreenCaptureKit` and `SCScreenshotManager.captureImage`
- returns a `CGImage`
- encodes to PNG/JPEG `Data` only when needed
- supports `maxImageDimension`
- retries transient ScreenCaptureKit streaming failures
- falls back to `CGWindowListCreateImage`

This is the closest reference for AUV's fast path.

### KWWK

`kwwk-computer-use-core/Sources/KWWKComputerUseCore/BackgroundWindowCapture.swift`
uses `CGWindowListCreateImage` into a `CGImage`, scales the image, then writes a
JPEG into its snapshot store.

KWWK proves the useful intermediate shape is `CGImage`, but its final path is
still synchronous file persistence.

## Design Direction

The capture fast path should separate four concerns:

1. Target resolution
2. Pixel capture
3. Consumer formatting
4. Recording/artifact persistence

The operation path should return pixels and metadata as soon as possible.
Recording should persist images later or through a separate boundary.

## Proposed Types

Names are provisional.

```rust
pub struct CaptureFrame {
  pub image: CaptureImage,
  pub source: CaptureSource,
  pub bounds: Rect,
  pub scale_factor: f64,
  pub backend: CaptureBackend,
  pub timing: CaptureTiming,
}

pub enum CaptureImage {
  Rgba(image::RgbaImage),
  Encoded {
    bytes: Vec<u8>,
    format: ImageFormat,
    width: u32,
    height: u32,
  },
}

pub enum ImageFormat {
  Png,
  Jpeg { quality: u8 },
}

pub struct CaptureTiming {
  pub resolve_target_ms: u64,
  pub enumerate_ms: u64,
  pub capture_ms: u64,
  pub crop_ms: u64,
  pub resize_ms: u64,
  pub encode_ms: u64,
  pub temp_write_ms: u64,
  pub artifact_stage_ms: u64,
  pub total_ms: u64,
}
```

`CaptureFrame` is not an artifact. It is operation data. The runtime or recorder
decides whether it becomes an artifact.

## Backend Order

First macOS backend order:

1. ScreenCaptureKit
   - preferred for fast window/display capture
   - returns `CGImage`
   - retry once for transient streaming failures

2. CGWindowListCreateImage
   - fallback for windows where SCK fails
   - useful for compatibility and as a control benchmark

3. xcap
   - retained as a compatibility fallback during migration
   - useful because existing code already understands its coordinate contracts

The implementation should keep backend selection explicit in telemetry.

## OCR Path

The OCR API should accept capture data without mandatory temp files.

Preferred shape:

```rust
driver.ocr().recognize_frame(&frame, region)?;
```

If the native OCR bridge still needs a file path in the first implementation,
the temporary write must be timed separately and treated as an implementation
detail. It should not be hidden inside `recognize_text_in_capture` without
telemetry.

## Artifact Persistence Boundary

Current behavior stages artifacts synchronously:

```text
driver writes source file
-> runtime copies source file into .auv/runs
-> recorder optionally reads file and uploads bytes
```

Target behavior:

```text
driver returns CaptureFrame / bytes
-> runtime records artifact metadata
-> recorder persists bytes through a write queue or explicit flush point
```

Rules:

- Driver crates should not know `.auv/runs`.
- Driver crates should not allocate final artifact filenames.
- Local artifact writes may be synchronous only when the caller explicitly asks
  for durable evidence before continuing.
- Inspect server writes should not block the operation path unless the caller
  requires successful inspect-server delivery.
- Runs must still be inspectable after failures; async writes need a flush or
  completion point before final success is reported.

## Benchmark Protocol

The first implementation work should start with baseline measurements on the
current code.

Measure these cases separately:

1. CLI display capture
2. CLI window capture
3. typed `window.capture`
4. typed `window.capture` followed by OCR
5. PNG file write from an in-memory capture
6. artifact staging copy into `.auv/runs`

For each case record:

- target app/window
- image size
- backend
- total wall time
- capture time if available
- encode/write time if available
- OCR time if available
- artifact staging time if available

The first baseline may use wall-clock command timings. The first implementation
should then add internal telemetry and rerun the same benchmark.

## Baseline Measurement: 2026-05-26

Environment:

- Machine: local macOS host.
- Frontmost app during measurement: Visual Studio Code.
- Inspect server writes disabled with `--inspect-server-write false`.
- Commands were run outside the Codex sandbox for Screen Recording/window
  access.
- Code was already built; compile time is excluded.

Results:

| Case | Command | Result | Wall time | User CPU | Notes |
|---|---|---|---:|---:|---|
| Window list | `target/debug/auv-cli invoke debug.listWindows --inspect-server-write false` | 12 visible windows | 0.08s | 0.01s | Establishes CLI/runtime overhead is small. |
| AUV display capture | `target/debug/auv-cli invoke debug.captureDisplay --inspect-server-write false` | 6016x3384 PNG via xcap | 6.22s | 6.05s | Produces 3.4 MB PNG artifact. |
| AUV window capture | `target/debug/auv-cli invoke debug.captureWindow --target com.microsoft.VSCode --inspect-server-write false` | 6016x3324 PNG via xcap | 6.09s | 5.77s | Produces 2.9 MB PNG artifact. |
| Copy AUV display PNG | `cp -p artifact_0001_display-capture.png /private/tmp/...` | copy succeeds | 0.00s | 0.00s | Artifact copy is not the visible bottleneck for this file size. |
| Copy AUV window PNG | `cp -p artifact_0001_window-capture.png /private/tmp/...` | copy succeeds | 0.00s | 0.00s | Same. |
| macOS window `screencapture` | `screencapture -l 192727 -x -o /private/tmp/...png` | 6016x3324 PNG | 0.20s | 0.10s | Same dimensions as AUV window capture. |
| macOS display `screencapture` | `screencapture -x /private/tmp/...png` | 6016x3384 PNG | 0.21s | 0.11s | Same dimensions as AUV display capture. |

Initial read:

- AUV's command/runtime overhead is not the main issue; window listing completed
  in roughly 80 ms.
- Local file copy of the produced PNG artifacts is not the main issue in this
  baseline.
- AUV's xcap capture path is roughly 30x slower than macOS `screencapture` for
  same-size PNG output on this target.
- The high user CPU time suggests the slow section is likely local capture
  conversion and/or Rust PNG encoding, not waiting on I/O.
- Internal telemetry is still required to split `xcap::capture_image()` from
  `image.save()` and later from OCR.

## Implementation Phases

### Phase 1: Measurement First

- Add or run a small benchmark path for current `xcap` capture.
- Measure capture-only, OCR temp write, and artifact staging separately where
  possible.
- Record current results in the follow-up implementation notes.

### Phase 2: In-Memory Capture Cleanup

- Keep `xcap` initially.
- Return `CaptureFrame` from typed capture paths.
- Add in-memory crop/resize helpers.
- Make OCR temp writes visible in telemetry.
- Avoid writing screenshot files unless recording requires artifacts.

### Phase 3: ScreenCaptureKit Backend

- Add Swift `ScreenCaptureKit` capture to `auv-driver-macos`.
- Return image bytes or RGBA data through the Rust/Swift boundary.
- Add retry and fallback to `CGWindowListCreateImage`.
- Keep `xcap` as fallback until examples prove the new path.

### Phase 4: Async/Deferred Artifact Writes

- Add recorder-owned artifact byte persistence.
- Avoid driver-side final artifact filenames.
- Make inspect-server upload non-blocking by default.
- Add a flush point before reporting final run success when required.

## Acceptance Criteria

- AUV can report capture timing broken down enough to identify bottlenecks.
- Typed window capture can be used without creating a temp PNG.
- OCR over a capture reports whether it used in-memory input or a temp-file
  bridge.
- Existing CLI capture commands keep working.
- The NetEase example can record evidence while keeping capture/OCR operation
  overhead visible.
- The implementation can compare xcap, CGWindowList, and ScreenCaptureKit on
  the same target window.

## Open Questions

- Should `CaptureFrame` live in `auv-driver` or `auv-driver-macos` first?
- Should encoded bytes cross the Swift bridge, or should raw RGBA cross and
  Rust handle encoding?
- Should local artifact writes become async immediately, or should the first
  implementation only avoid unnecessary driver temp files?
- Should ScreenCaptureKit become default immediately after landing, or stay
  opt-in until benchmarked across NetEase and at least one browser/electron app?
