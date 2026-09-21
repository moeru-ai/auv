# Positional targets and NetEase live validation

Status: implemented; macOS NetEase live probe passed. This is not a cross-platform support claim.

## Accepted scope

Introduce `Position`, `Positional`, and `Positioned<T>` in
`auv-driver-common::geometry`, reuse `CoordinateSpace::Window` and `WindowRef`,
and connect NetEase OCR targets to macOS window clicks. Do not add application
fixtures, simulated application behavior, or fixture-based E2E tests. Pure
coordinate/serialization tests do not substitute for live application evidence.

## Contract

- `Position` contains a logical point and its coordinate space; window positions
  retain the original window ID, not an app/main-window selector.
- `Positional` only supplies geometry. No blanket `Clickable` implementation,
  hidden locator refresh, or guarantee of actionability is introduced.
- `Positioned<T>` retains recognition data and the chosen position. NetEase's
  card-cover offset remains explicit app policy.
- `Capture` implements `Positional` through an optional `origin`, the image's
  top-left in its owning space. Full-window captures use window `(0, 0)`;
  display/region captures use screen coordinates. Detached images are unbound.
- `TextRecognition.origin` interprets numeric region bounds as logical offsets.
  Native capture OCR keeps its existing numeric geometry and records the offset
  between that geometry and the capture's owning space. No second pixel scaling
  occurs. `recognition.relative_to(&capture)?` then yields image-local logical
  bounds, retaining the origin. Same-origin rebasing is idempotent; cross-window,
  cross-space, and unbound rebasing fail rather than silently relabel coordinates.
- `positioned_regions()?` creates text targets from that origin plus each center.
  NetEase selection no longer attaches a window manually. A nonzero crop origin
  remains part of the target position. `Positioned<T>` and `Position` can also be
  used as rebasing anchors in the same space.
- `Positional::position` is fallible for unbound data and performs no IO. Runner
  protobuf capture/recognition messages preserve optional origins. Older payloads
  without origins remain readable but cannot supply positional targets.
- macOS `WindowApi::click_target` accepts the target directly, validates finite
  coordinates and a window coordinate space, looks up the exact current window,
  and delegates to the existing typed click implementation. Policies, modifiers,
  and `InputActionResult` retain their existing meanings.
- A window move is handled by the current frame; content scrolling, reflow,
  resize-induced layout changes, and native window-ID reuse still need explicit
  observation/lifecycle handling. There is no new stale-target guarantee.

## Migration

Daily recommendation sidebar, card, and Play All clicks plus playlist Play All
use the bound targets. The three duplicated NetEase origin-conversion functions
are replaced by the common recognition method. Foreground retries also call
`click_target` with `ForegroundPreferred`, removing duplicated projection,
prepare/click/restore code and manually constructed delivery results. They now
use the driver's existing 50 ms activation settle; the app's configurable
post-click settle remains. Driver-returned foreground/mouse disturbance is
preserved instead of being reported as `none` by `single_success`.

Other desktop platform consumers, Screen/Display routing, and Locator behavior
are deliberately deferred. Named-playlist selection itself is unchanged; its
Play All migration compiled and passed existing tests but was not separately
live-probed with a named playlist.

## Validation

Commands:

```sh
cargo build -p auv-netease-music --features tracing --bin auv-netease-music
cargo test -p auv-driver-common -p auv-driver-macos -p auv-netease-music --features auv-netease-music/tracing --lib
cargo clippy -p auv-driver-common -p auv-driver-macos -p auv-netease-music --features auv-netease-music/tracing --lib
target/debug/auv-netease-music --store-root /tmp/auv-positional-live-20260919 playlist play daily-recommended --json
target/debug/auv-netease-music now-playing --format json
```

- The application binary built and was live-tested in the originating checkout.
  On 2026-09-20 the PR was extracted into a clean worktree based on `3e843821`,
  excluding the concurrent held-input work. Isolated focused tests passed:
  common 36, macOS 105 / 5 ignored, NetEase 122 (263 passed total).
  This was validation of the initial target implementation; capture-origin
  follow-up validation is recorded below.
- In the isolated PR worktree, `cargo fmt --all --check`, focused Clippy (with
  existing warnings), and `git diff --check` passed.
- First live navigation run `01a0b9fd-1b1a-7146-bfb9-01541e3e8b7a` exercised
  Recommend, the daily card, its existing foreground-title retry, and Play All.
  This preceded migration of foreground retries to the new interface.
- An attempted paused precondition was disproven by the raw screenshot; that
  run is not counted as a paused-to-playing transition.
- Run `01a0ba00-b6db-774f-ac13-3f49713546bf` failed before clicking: its initial
  capture was a small black image and OCR did not find Recommend. The precise
  capture/window-selection cause was not established; a later retry succeeded.
- **Initial target implementation**, run `01a0ba04-0ae7-75c0-9b98-dd1467e1713c`, started
  on the daily detail page with system now-playing `is_playing=false` and rate
  `0.0`. Window-targeted Play All did not verify playback. The new
  `click_target(..., ForegroundPreferred)` retry succeeded; visual verification
  reported `pause_visible`, and independent system now-playing reported
  `is_playing=true`, rate `1.0`, with a different track.
- The final foreground delivery reports `focus_disturbance=foreground`,
  `mouse_disturbance=temporary`, and `verified=false`. The app's separate visual
  verification supplies semantic evidence. No claim of background-only success
  is made.

The observed main-window selection and foreground-input behavior remain
follow-up concerns; this slice does not broaden their support guarantees.

## Capture-origin follow-up

The follow-up extends native macOS/Linux/Windows capture and OCR producers,
Runner serialization, and the existing NetEase callers. Balatro capture crops
retain their logical origin; detached or enlarged image helpers remain unbound.
Pure geometry/transport tests cover nonzero origins, rebasing invariance,
cross-space rejection, serialization, and missing/invalid origin metadata.
No application fixtures or simulated E2E tests were added.

2026-09-20 live run `01a0bf2a-1048-72aa-beac-122568bb343f` exercised
Recommend -> daily card -> Play All with `relative_to(&capture)` and inherited
window identity. All three clicks used `window_targeted_mouse`; no foreground
retry was needed in this run. Visual verification reported `pause_visible` and
independent system media changed from paused/rate 0 to playing/rate 1 with a
new track. Input delivery still correctly reported `verified=false`; semantic
verification remained separate.

The first follow-up run `01a0bf28-9f5d-7559-b690-d82bf0bd7fe7` stopped at the
card selector on a 1754-unit-wide window. Its title was left of the existing
`x > width * 0.18` layout guard. Resizing to width 1252 allowed the existing
workflow to run. This layout limitation is unchanged; the successful run is not
a claim that all window sizes work.

Automated validation: common 38, macOS 105 (5 ignored), Linux 38, SDK 42,
Runner CLI 43 (1 ignored), and NetEase 122 passed. The Windows crate on macOS
had 63 passes and two failures in existing native-backend expectations
(`InvalidImage` versus the non-Windows `Unsupported` stub, and `ScreenToClient`
versus the non-Windows click error). These are not Windows-host validation.
Balatro's broad `--all-targets` check encountered an existing missing `Config.id`
in `tests/custom_runner_e2e.rs`; no application fixture or E2E was added or
changed to mask that failure.

Final focused checks passed: Linux/Windows vision geometry (4/5 tests), Runner
mapping (28 passed, 1 ignored), formatting, diff checks, and Clippy for common,
macOS, NetEase, SDK, Runner CLI, and the Balatro library (existing warnings).
Playback was paused after live testing; system media confirmed `is_playing=false`.

## Public constructors and shared wire conversion

`Position::in_screen(ScreenPoint)` and `Position::in_window(&WindowRef,
WindowPoint)` bind existing logical coordinates without conversion, resource
lookup, freshness checks, or actionability checks. Both are documented; screen
capture producers and `ScreenPoint::position()` use the screen constructor.

`auv::protocol::position` owns the shared Runner position encoder, decoder, and
`DecodeError`. The SDK and Runner CLI only map that error into
`CapabilityError::InvalidResponse` and `Status::invalid_argument`. The `auv`
crate already depends on domain and protocol types and is consumed by the CLI,
so this introduces no new crate dependencies and leaves `auv-api-proto`
independent of driver domain types. Validation tests exercise the shared decoder;
endpoint tests retain coverage for origin transport and error mapping.

Review follow-up validation: common, SDK, and Runner CLI library tests passed
(127 passed, 1 ignored). Focused Clippy passed with existing warnings; formatting
and diff checks passed. This refactor preserves encoding and validation behavior.
