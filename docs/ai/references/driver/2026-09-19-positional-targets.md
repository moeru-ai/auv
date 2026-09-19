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
- `TextRecognition::relative_to` translates already-scaled logical bounds.
  Full-window capture callers use the capture bounds origin. Cropped image
  origins must not be assumed to be window origins.
- `RecognizedText::in_window` binds already-window-local recognition to a
  `WindowRef`. Its center is the initial action point.
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
  Only one new test was added in this slice: pure coordinate translation and
  target serialization, with no application model.
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
- **Final implementation**, run `01a0ba04-0ae7-75c0-9b98-dd1467e1713c`, started
  on the daily detail page with system now-playing `is_playing=false` and rate
  `0.0`. Window-targeted Play All did not verify playback. The new
  `click_target(..., ForegroundPreferred)` retry succeeded; visual verification
  reported `pause_visible`, and independent system now-playing reported
  `is_playing=true`, rate `1.0`, with a different track.
- The final foreground delivery reports `focus_disturbance=foreground`,
  `mouse_disturbance=temporary`, and `verified=false`. The app's separate visual
  verification supplies semantic evidence. No claim of background-only success
  is made.

Local evidence retained in the originating checkout (ignored, unavailable in a
fresh clone, and not fixtures or committed UI datasets):

- Run records: `docs/notes/neko/2026-09-19-positional-live/records.jsonl`
- Final command result: `docs/notes/neko/2026-09-19-positional-live/final-result.json`
- Before media state: `docs/notes/neko/2026-09-19-positional-live/final-before-media.json`
- After media state: `docs/notes/neko/2026-09-19-positional-live/final-after-media.json`
- Final verification screenshot: `docs/notes/neko/2026-09-19-positional-live/artifacts/01a0ba04-0ae7-75c0-9b98-dd1467e1713c/01a0ba04-3192-70f5-9771-57c2317018af.png`

The observed main-window selection and foreground-input behavior remain
follow-up concerns; this slice does not broaden their support guarantees.
