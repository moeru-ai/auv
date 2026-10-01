# Linux Screenshot fallback coordinate binding

This bug fix prevents partial Screenshot images from acquiring full-display coordinates.
It changes the shared Linux capture path used by display, region, and window captures.

## Failure and boundary

On 2026-10-01, the interactive Screenshot fallback returned a selected `377×48` area.
AUV assigned the complete `2752×1152` display bounds to this image.
This assignment produced an incorrect pixel scale for subsequent crops and coordinate projection.

The [Screenshot response](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Screenshot.html)
contains a URI, without the selected area origin or dimensions.
Image dimensions alone cannot establish screen coordinates when display discovery fails.

## Implemented behavior

The [Linux capture module](../../../../crates/auv-driver-linux/src/capture.rs) requests Screenshot with `interactive=false`.
Screenshot version 3 also receives the explicit `Screen` target.
Version 2 remains usable through the checks described here.
The portal can still require user consent.

Screenshot fallback requires exactly one known Wayland output.
The output identity, logical bounds, and scale must match the requested display before and after the portal request.
The returned image dimensions must equal the rounded logical dimensions multiplied by the output scale.
These checks occur before region cropping or screen-coordinate binding.

Missing output geometry, multiple outputs, changed output geometry, and incorrect image dimensions produce errors.
The error retains both the primary ScreenCast failure and the fallback failure.
Valid captures retain the output origin and scale, including negative desktop coordinates.

Multi-output Screenshot mapping remains deferred.
The response has no composite layout or per-output pixel map.
The inline deferral requires an owner-approved desktop mapping contract before this fallback expands.
PipeWire capture retains its existing behavior.
The separate default-output selection issue in `capture_window()` remains outside this fix.

## Evidence

Evidence level: **unit-tested**, plus two **live-validated** Screenshot fallback captures on GNOME Wayland.
This result does not establish support for other compositors or multi-output layouts.
GPU rendering and GPU-buffer transport were not measured.

The [regression tests](../../../../crates/auv-driver-linux/src/capture_test.rs) rejected the reproduced partial image and missing output geometry.
Both tests failed before the production fix.
Additional tests cover a partial image with matching aspect ratio, scaled crops, changed output geometry, and multiple outputs.

The live test directly called the real Screenshot fallback after an injected primary failure.
It did not simulate the Screenshot response or claim a real ScreenCast failure in that pass.
The output was `DP-1`, with `2752×1152` logical bounds and scale `1.25`.
Screenshot version 2 returned `3440×1440` pixels.
Visual inspection confirmed the complete desktop.
The PNG remains local at `/tmp/auv-window-capture-repro/screenshot-fixed.png`.

Run the automated regression tests:

```sh
cargo test -p auv-driver-linux --all-targets
```

On a live single-output Wayland desktop, run the real fallback test:

```sh
AUV_SCREENSHOT_TEST_PNG=/tmp/auv-screenshot.png \
  cargo test -p auv-driver-linux --lib \
  -- --ignored --exact capture::tests::screenshot_fallback_live_single_display --nocapture
```

The live test requires Screenshot portal consent.
The output path is optional. Without it, the test removes its temporary PNG.

## Repository validation

`cargo check`, `cargo fmt --check`, `git diff --check`, and Linux driver Clippy completed successfully.
The Linux driver suite passed 90 unit tests and four example tests.
The real Screenshot test also passed.

Default `cargo test` failed three existing daemon tests during process cleanup.
Those tests hardcode `/bin/kill`, which this NixOS host does not provide.
All remaining default tests passed after those three tests were excluded.

Candidate next slice: remove the CLI test cleanup dependency on `/bin/kill` so those tests run on NixOS.
