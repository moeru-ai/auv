# Linux window capture reference validation

This bug fix prevents absent AT-SPI window references from receiving successful desktop crops.
It changes the shared Linux `window.capture` path, including callers of `window.find_text`.
It adds no public API.

## Failure and implemented behavior

Previously, `capture_window()` decoded the reference syntax but never resolved the requested window.
It checked other windows for a shared origin, then cropped the caller's frame from the display.
A fabricated reference with frame `(10, 10, 10, 10)` received a successful `13×13` capture.
The result assigned window coordinates to a reference that did not exist.

The [window module](../../../../crates/auv-driver-linux/src/window.rs) now resolves the complete reference against the current AT-SPI window list.
It does not substitute a window with a matching title or frame.
An absent reference or a window that is not visible returns `DriverError::NotFound`, with the requested reference in the diagnostic.
AT-SPI enumeration errors remain errors; they do not permit a crop.

Capture uses the resolved window's current frame instead of the caller's snapshot.
The existing shared-origin rejection also uses that current frame.
After display capture, AUV resolves the same reference again.
A missing window returns `NotFound`; a changed frame returns `InvalidInput` and asks the caller to retry.
Only a stable target receives the cropped image, current bounds, and window-bound origin.

## Evidence

Evidence level: **unit-tested**, plus **live-validated** reference rejection and frame refresh on GNOME Wayland.
This result does not establish support for other compositors or multiple outputs.

The [tests](../../../../crates/auv-driver-linux/src/window_test.rs) cover exact reference matching, invisibility, current-frame crops, and shared-origin rejection.
They also cover disappearance, movement, resizing, and unchanged geometry after display capture.

The live missing-reference regression failed before the production change and passed after it.
The reference was `atspi::1.999999/org/a11y/atspi/accessible/999999`.
The test uses the public Linux session window capture API.

The live frame-refresh test retained a real Ghostty reference but supplied frame `(10, 10, 10, 10)`.
Capture returned the current logical frame `(1230, 55, 1502, 1077)` and `1878×1346` pixels at scale `1.25`.
The backend was `atspi.extents+xdg-desktop-portal.screencast.pipewire.crop`.
Visual inspection confirmed the expected crop bounds and also showed portal UI over part of the window.
This is evidence of reference and geometry validation, not isolated window content.
The local image is `/tmp/auv-window-capture-repro/window-ref-fixed.png`.

Run the automated tests:

```sh
cargo test -p auv-driver-linux --all-targets
```

On a stable live Wayland desktop, run both public capture regressions:

```sh
AUV_WINDOW_TEST_PNG=/tmp/auv-window.png \
  cargo test -p auv-driver-linux --lib -- \
  --ignored window::tests::window_capture_ --nocapture --test-threads=1
```

The frame-refresh test requires an AT-SPI application window and capture portal consent.
The PNG output path is optional.

## Remaining boundaries

AT-SPI visibility retains the existing enumeration meaning: a window with nonzero extents.
This fix does not add compositor visibility or occlusion detection.
Before/after snapshots cannot detect movement that returns to the same frame during capture.
The inline deferral requires an owner-approved identity-bound capture source before atomic frame binding expands.

Window capture still crops a display source.
Default-output selection, fractional-scale edge rounding, and ScreenCast stream geometry matching remain separate reviewed issues.
The [Screenshot fallback checks](2026-10-01-linux-screenshot-fallback.md) retain their existing single-output boundary.

## Repository validation

Linux driver tests passed: 98 unit tests and four example tests, plus both live capture regressions.
`cargo check`, `cargo fmt --check`, Linux driver Clippy, and `git diff --check` completed successfully.
Clippy retains existing warnings outside the new code.

Default `cargo test` still fails three existing CLI daemon tests because they hardcode `/bin/kill`, which this NixOS host lacks.
All remaining default tests passed with those tests excluded.
