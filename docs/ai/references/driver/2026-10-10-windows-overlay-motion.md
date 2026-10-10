# Windows overlay motion: a live cursor that follows real input

Follows [#306](https://github.com/moeru-ai/auv/pull/306) (the Direct2D renderer).
Owner direction, 2026-10-10: this is a product feature of the Windows driver, not a
promotional recording. The overlay shows, in real time, what AUV is doing: a cursor that
glides to where AUV acted, a ripple where it clicked, and a mark on each window it
targeted, while the automation never touches the user's real mouse or focus ("beside you,
without taking your mouse").

Status: implemented on branch `feat/overlay-windows-motion`. Evidence level for the
Windows live overlay is `live-validated` on one machine (see [Evidence](#evidence)); it is
not `supported`.

## Contract

```text
driver delivers input (SendInput, posted window messages, logical mouse)
  -> auv-driver-windows reports ActionEvent after delivery succeeded
  -> LiveOverlay / Animator (auv-driver-overlay-windows), 60 fps frame loop
  -> MotionScene composes an Overlay for that instant (auv-driver-overlay-common, pure)
  -> the existing one-shot renderer presents it (window::present, unchanged)
```

The renderer stays one-shot. The animator calls it once per frame with the layers the scene
composes for that instant, so #306's canvas and window drawing is untouched except for the
wiring listed below. This matches the shared rule in
[`TERMS_AND_CONCEPTS.md`](../../../TERMS_AND_CONCEPTS.md#live-overlay): callers report
actions, the adapter owns animation timing, and no caller drives frames across the platform
seam.

### Events (provisional names)

`ActionEvent` is the overlay's projection of what a driver did. It is not a tracing event
and not a second action-result schema; `InputActionResult` stays the delivery record.

| Event | Reported when | Visual |
| --- | --- | --- |
| `Moved { point, travel: Jump }` | A pointer warp or the first delivered sample of a movement | The cursor eases there with the shared `MotionOptions` (`EaseInOutExpo`, 320 ms by default) |
| `Moved { point, travel: Sampled }` | A later sample of a timed trajectory the driver is already playing | The cursor is drawn at that point with no added delay |
| `Clicked { point, button }` | A click was delivered, once per press | A ripple at the true point and time; the cursor shows its pressed art for 180 ms |
| `WindowTargeted { id, frame, label }` | An action was delivered to that window | An outline and label on the window's frame; refreshed by later actions, faded and removed 3 s after the last one |

### Faithfulness (visual-trust rules)

1. Every visual comes from an event the driver sent after delivery. A failed delivery
   reports nothing; `Started` of a movement reports nothing because nothing was delivered
   yet.
2. The cursor only moves between reported points (a unit test sweeps a multi-jump sequence
   and asserts it never leaves their span). It never moves toward a point nobody reported.
3. A click ripple appears at the click's true point at the click's true time. It is not held
   back until the cursor arrives, because holding it would drop the ripple of any click
   reported faster than the glide finishes. The consequence is visible in the captures: for up
   to 320 ms after a click, the ripple is on the point and the cursor is still gliding in.
4. The overlay sends no input. There is no `SetCursorPos`, `SendInput` or focus call in the
   overlay crates, and `follow_operations` only listens.
5. Only the shared default mouse (zero) drives the cursor. One cursor cannot honestly stand
   for several logical mice.

### Easing and retargeting

- `Easing::apply` evaluates `EaseInOutExpo`, the curve the macOS renderer implements natively
  (`easeInOutExpo` in `Overlay.swift`). Unit tests pin its values to that formula. The enum and
  `MotionOptions` are unchanged; no second easing exists.
- A `Jump` retargets from where the cursor is *drawn*, so it never teleports, even mid-glide.
- A `Sampled` point replaces the glide's live target without restarting the ease. A restart
  per sample would crawl behind a fast stream, because `EaseInOutExpo` starts almost still.
  Instead the cursor stays continuous while it converges on the live path.
- The ripple is an existing `Outline` layer (a square with a half-side corner radius) whose
  rectangle and stroke are animated. No new `Layer` variant was needed.

## What changed

| Crate | Change |
| --- | --- |
| `auv-driver-overlay-common` | `motion` module: `ActionEvent`, `Travel`, `MotionScene`, `MotionFrame`, `Wake`, `Easing::apply`, `MotionOptions::progress`; `FrameStats` / `Percentiles`. Pure and cross-platform. `overlay.rs` (the easing contract) is untouched |
| `auv-driver-overlay-windows` | `Animator` (thread, 1 ms timer resolution, message pump, warm-up), `pacing` (render/wait/remove decisions, pure), bounded sample buffer. `window.rs`: an `IsWindow` check so a window destroyed with its owner thread is recreated. Built-in cursor art, below |
| `auv-driver-overlay` | `LiveOverlay` facade (Windows adapter; `Unavailable` elsewhere) |
| `auv-driver-windows` | `OverlayApi::follow_operations` and the report sites below; `overlay_follow` example |

Report sites (`auv-driver-windows`, behind the `overlay` feature, after successful delivery):

| Site | Reports |
| --- | --- |
| `input::click_at` (foreground `SendInput`) | `Clicked` per press |
| `WindowApi::click` | `WindowTargeted`; background clicks also `Clicked` |
| `InputApi::move_mouse` (mouse zero) | The target window if any, then `Moved` per delivered sample |
| `InputApi::move_mouse_to` (mouse zero) | `Moved { Jump }` |

### Built-in cursor art

Resolves `TODO(driver-overlay-windows-builtin-art)`. Windows built-in cursors now draw a
pixel-art pointer (`assets/cursor-pixel.svg`, generated from an ASCII grid kept in the file)
with the macOS default glow (`Shadow::auv()` when the style sets none; a transparent shadow
turns it off). The 12 x 12 grid of 2 px cells keeps its edges pixel-crisp, and its tip is the
top-left cell, so the tip sits exactly on the point the operation acted on. Custom SVG cursors
keep macOS's 4 px offset. The old disc sprite and `Canvas::stroke_circle` are removed.

Not done: the owner's reference image
(`workspace/user/media_library/image/60/60a92983...png`) is not on the development machine,
so the art was designed without it. The pointer is a first pass for the owner to compare.

## Evidence

Machine: Windows 11 Pro Insider Preview 10.0.29648, NVIDIA GeForce RTX 4070 Ti (not used:
the renderer is a software target), one 2560 x 1440 display at 100% scaling, 180 Hz.
Branch head before commit: `277e0993` plus this change. Runs on 2026-10-10 in release mode.

### Unit tests (cross-platform unless noted)

- `auv-driver-overlay-common` (22 new): easing values against the macOS formula, clamping,
  monotonicity; `MotionOptions::progress`; first appearance; jump eases and never moves more
  than 2.5 px per millisecond over 100 px; mid-flight retarget starts from the drawn point;
  sampled points drawn with no delay; a sample during a glide stays continuous and converges;
  the cursor never leaves the span of reported points; click ripple at the reported point and
  time; every click gets a ripple even when reported faster than the glide; at most 16 live
  ripples; two window marks at once, refreshed and expiring; an empty scene draws nothing.
- `auv-driver-overlay-windows` (`pacing`, `stats`, window pixels; Windows for the pixel
  tests): 60 fps cap, zero added delay after a quiet period, no catch-up burst after a slow
  frame, idle removal timing; built-in pointer tip pixel, crispness, glow on and off, variants.
- `auv-driver-windows` (10, `--features overlay`): one click event per press; window and label
  mapping; a movement reports nothing until it delivered a sample, jumps first and follows
  after; a window-targeted movement marks the window once; one follower per process.

### Timeline harness (Windows)

```text
cargo run --release -p auv-driver-overlay-windows --example overlay_motion_timeline -- <out-dir>
```

A scripted timeline (two window marks, two clicks 617 px apart, a 125 Hz sampled drag, a right
click) played through the real animator over an owned backdrop, with the screen captured
continuously. The harness finds the pointer tip in each capture. The harness is for
developing and testing the animation; it is not the product.

![eight frames of the scripted timeline](assets/overlay-windows-motion-timeline.png)

| Measurement (final build) | Result |
| --- | --- |
| Frame time (compose + present) | P50 10.5 ms, P95 11.7 ms, max 28.2 ms in the run recorded above; across the last three runs P50 10.4 to 11.0 ms, P95 12.2 to 14.2 ms, max 14.4 to 17.0 ms |
| Late frames | 0 |
| Event latency (report to `present` returned) | P50 18.6 to 19.4 ms, P95 27.0 to 29.7 ms, max 28.5 to 33.3 ms (four runs) |
| Click-to-click glide, 617 px | 23 captures strictly between the endpoints; largest step between captures 183 px (30%); never off the straight path; settles exactly on the point |
| Sampled drag | The pointer was at most 38 ms behind the driver's sample in the recorded run (51 px at 1.33 px/ms; 31 to 39 ms in the other runs). An eased copy of the same stream would be hundreds of pixels behind, so the check separates them |

### Real operations through the driver (Windows)

```text
cargo run --release -p auv-driver-windows --features overlay --example overlay_follow -- <out-dir>
```

The example opens an opaque backdrop and two top-level windows of its own, finds them through
`list_windows`, and performs real operations through `WindowsDriverSession` while
`follow_operations` runs: a 500 ms logical-mouse movement posted to window A, a click in A,
then a left and a right click in B. It never calls `SendInput`, `SetCursorPos` or any focus
API. Captures contain only the backdrop and the example's windows.

![the overlay following four real driver operations across two windows](assets/overlay-windows-motion-real-operations.png)

| Check | Result |
| --- | --- |
| The driver delivered what it reported | A received 1 click and 31 move messages; B received 2 clicks |
| No mouse or focus disturbance | All four `InputActionResult`s: `selected_path` `window_targeted_mouse`, `mouse_disturbance` and `focus_disturbance` `none` |
| Two real windows annotated at once | Both windows outlined and labelled with their titles in the captures |
| Ripple at the real click points | Visible at the clicked point in A and B |
| Frame time | P50 10.8 ms, P95 12.7 ms, 0 late frames, 0 present failures |
| Event latency | P50 18.2 ms, P95 27.2 ms |

The OS pointer and the foreground window are printed before and after as information only: a
person using the machine can move either while this runs (a first version of this check failed
exactly because the machine's user moved their mouse), so the typed delivery record above is
the assertion.

## Known limits

- **Cost.** A frame is a full virtual-screen software render, 10.5 ms of every 16.7 ms while
  animating (about 60% of one core, derived from frame time, not separately measured). It is
  idle when nothing moves. Multi-monitor virtual screens, which make the bitmap larger, were not
  measured. Reusing the bitmap between frames or presenting only a dirty rectangle is a
  `canvas.rs` change and was left out on purpose.
- **Latency.** About one frame of pacing plus about one frame of render separate a driver
  event from the frame that shows it (P50 about 19 ms). Easing is the only delay added on
  purpose; the shared `EaseInOutExpo` starts nearly still, so a jump's cursor barely moves for
  its first 100 ms or so. That follows from the shared contract.
- **One animator per process, and the sole presenter.** The window belongs to the thread that
  first presents, so mixing a running animator with one-shot `render` from another thread can
  stall (`TODO(driver-overlay-windows-window-owner-thread)`).
- **Start-up.** `Animator::start` returns after a warm-up of 40 to 330 ms, because the first
  Direct2D/DirectWrite use otherwise produced a 120 to 330 ms first frame.
- **Coordinates and DPI.** Positions are the physical screen pixels drivers already use; not
  validated at scaling other than 100%.
- **Real applications.** The evidence drives windows the example owns. It does not click into
  third-party applications on a person's desktop.
- **CI.** Not run from this machine; Windows CI is the gate.

## Deferred (markers in code)

- `TODO(overlay-follow-other-input)`: scroll, key, text and held-button delivery report
  nothing. A click-shaped ripple for a scroll would imply an action that did not happen.
- `TODO(overlay-follow-remote-runner)`: events are reported inside the process that delivers
  input. A Runner serving remote callers needs the same hook at its own seam.
- `TODO(overlay-follow-multi-mouse)`: only mouse zero drives the single cursor.
- `TODO(overlay-motion-event-wire)`: `ActionEvent` is in-process; no serialization or tracing
  event until an out-of-process consumer needs it.
- `TODO(overlay-live-theme)`: the host theme applies to one-shot `show` only.
- `TODO(overlay-live-macos)`: macOS already animates natively per `show`; routing live events
  there is a separate slice.
- `TODO(driver-overlay-windows-window-owner-thread)`: see Known limits.
- Still open from #306: `TODO(driver-overlay-windows-silhouette-shadow)` and
  `TODO(driver-overlay-windows-outline-label)`.

## Questions for the owner

1. The reference image for the pointer art was not available (see above). Is the pixel
   pointer close enough in feel to adjust, or should it be redone against the image?
2. Click ripples appear at the click's true time while the cursor may still be gliding in
   (rule 3). The alternative, showing the ripple on arrival, looks tidier for a single click
   but hides clicks reported faster than 320 ms apart. Keep the faithful behavior?
3. Should the evidence also include clicks into a real third-party application, or is
   harness-owned windows plus the typed delivery record enough?
