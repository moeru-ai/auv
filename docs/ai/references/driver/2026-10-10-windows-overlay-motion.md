# Windows overlay motion: a live cursor that follows real input

Follows [#306](https://github.com/moeru-ai/auv/pull/306) (the Direct2D renderer).
Owner direction, 2026-10-10: this is a product feature of the Windows driver, not a
promotional recording. The overlay shows, in real time, what AUV is doing: a cursor that
moves to where AUV acted, a ripple where it clicked, and a mark on each window it targeted,
while the automation never touches the user's real mouse or focus ("beside you, without
taking your mouse").

Owner feedback later the same day: the first version's motion and pointer art did not feel
right. The owner compared rendered side-by-side previews and chose a spring-driven cursor
(tilt while moving, a dip on click, the macOS lime ripple) and the "Vector modern" pointer at
24 px, asking for a more modern look. [Motion](#motion) and
[Built-in cursor art](#built-in-cursor-art) record the result.

Status: implemented on branch `feat/overlay-windows-motion`. Evidence level for the
Windows live overlay is `live-validated` on one machine (see [Evidence](#evidence)); it is
not `supported`.

## Contract

```text
driver delivers input (SendInput, posted window messages, logical mouse)
  -> auv-driver-windows reports ActionEvent after delivery succeeded
  -> LiveOverlay / Animator (auv-driver-overlay-windows), 60 fps frame loop
  -> MotionScene composes an Overlay for that instant (auv-driver-overlay-common, pure)
  -> the existing one-shot renderer presents it (window::present)
```

The renderer stays one-shot. The animator calls it once per frame with the layers the scene
composes for that instant. This matches the shared rule in
[`TERMS_AND_CONCEPTS.md`](../../../TERMS_AND_CONCEPTS.md#live-overlay): callers report
actions, the adapter owns animation timing, and no caller drives frames across the platform
seam.

### Events (provisional names)

`ActionEvent` is the overlay's projection of what a driver did. It is not a tracing event
and not a second action-result schema; `InputActionResult` stays the delivery record.

| Event | Reported when | Visual |
| --- | --- | --- |
| `Moved { point, travel: Jump }` | A pointer warp or the first delivered sample of a movement | The cursor springs there from wherever it is drawn |
| `Moved { point, travel: Sampled }` | A later sample of a timed trajectory the driver is already playing | The cursor follows with a short spring, about 25 ms behind |
| `Clicked { point, button }` | A click was delivered, once per press | A ripple at the true point and time; the cursor shows its pressed art and dips in size for 180 ms |
| `WindowTargeted { id, frame, label }` | An action was delivered to that window | An outline and label on the window's frame; refreshed by later actions, faded and removed 3 s after the last one |

### Faithfulness (visual-trust rules)

1. Every visual comes from an event the driver sent after delivery. A failed delivery
   reports nothing; `Started` of a movement reports nothing because nothing was delivered
   yet.
2. The cursor only moves toward reported points. It never passes the point it is heading
   for, and a turn mid-flight swings at most a quarter of the remaining distance off the
   direct line (unit tests cover both, plus a multi-jump sweep that never leaves the span of
   the reported points).
3. A click ripple appears at the click's true point at the click's true time. It is not held
   back until the cursor arrives, because holding it would drop the ripple of any click
   reported faster than the cursor travels. The consequence is visible in the captures: right
   after a click, the ripple is on the point while the cursor is still arriving.
4. Tilt and the press dip turn and scale the art about its hotspot, so the tip stays on the
   reported point.
5. The overlay sends no input. There is no `SetCursorPos`, `SendInput` or focus call in the
   overlay crates, and `follow_operations` only listens.
6. Only the shared default mouse (zero) drives the cursor. One cursor cannot honestly stand
   for several logical mice.

## Motion

The first version eased every jump with the shared `EaseInOutExpo` over 320 ms. Two problems
made it feel wrong:

- The curve is extreme. It covers 3% of a move in the first 96 ms and 75% in the next 64 ms,
  about four frames at 60 fps, then creeps. A long move read as a pause, a jump and a crawl.
- A new point reported mid-flight restarted the ease at zero speed, so a fast cursor stopped
  dead and set off again.

The live cursor now rides a critically damped spring toward the last reported point
(`motion.rs`, owner-approved from a rendered preview):

- **Jumps** use a 110 ms time constant. The cursor covers about 60% of the way in 110 ms and
  is within a pixel of a 600 px jump after about half a second. Its position is the spring's
  closed-form solution, so any instant is exact whatever the frame rate.
- **Retargeting keeps the cursor's speed.** A new point starts a new spring from the drawn
  position and velocity. Two caps keep it faithful: speed toward the target is capped at
  `ω · distance`, the most a critically damped spring can have without passing its target,
  and sideways speed is capped so a turn swings at most 25% of the distance off the line.
- **Samples** use a 25 ms time constant, so the cursor trails a driver-timed stream by about
  25 ms. This smooths uneven sample arrival without a visible delay.
- **Settling is exact.** Once a spring can no longer move by 0.05 px it reports its target
  exactly, and the scene goes idle.
- **Tilt.** The art turns clockwise with rightward speed (0.006 degrees per px/s, at most
  14 degrees), so the body trails the tip. The tilt follows the speed with an 18/s lag, which
  smooths the speed changes between a stream's samples. It is the one piece of scene state
  that advances per drawn frame rather than in closed form.
- **Click.** The pressed art shows for 180 ms while the cursor dips to 84% of its size and back
  along half a sine. The ripple uses the macOS click ripple's geometry
  (`drawFlashRippleIfActive` in `Overlay.swift`): a 2 px ring growing from 3 to 28 px with an
  ease-out cubic, fading from 0.7 over 450 ms. Left clicks are lime (`AUV_LIME`), right clicks
  AUV cyan and middle clicks white.
- The ripple is still an existing `Outline` layer (a square with a half-side corner radius). No
  new `Layer` variant was needed.

The shared `Easing` / `MotionOptions` contract is unchanged and still governs one-shot
overlays (macOS animates its cursor per `show` call). The live overlay has its own motion, so
`Animator::start`, `LiveOverlay::start` and `OverlayApi::follow_operations` take
`LifecycleOptions` instead of `ShowOptions`. `Easing::apply` and `MotionOptions::progress`,
added earlier on this branch only for the live scene, were removed with it. The original
brief's "no second easing" constraint is superseded for the live cursor by the owner's choice.

The pose is a new field on the shared cursor layer: `CursorPose { tilt_degrees, scale }`,
serialized only when it is not at rest. Only the live scene sets it. The Runner wire format
does not carry it (NOTICE at the field), and the macOS adapter ignores it
(`TODO(overlay-live-macos)` at its cursor mapping).

## What changed

| Crate | Change |
| --- | --- |
| `auv-driver-overlay-common` | `motion` module: `ActionEvent`, `Travel`, `MotionScene` (spring, tilt, press, ripples, marks), `MotionFrame`, `Wake`; `FrameStats` / `Percentiles`; `CursorPose` on the `Cursor` layer. Pure and cross-platform. The easing contract in `overlay.rs` is untouched |
| `auv-driver-overlay-windows` | `Animator` (thread, 1 ms timer resolution, message pump, warm-up), `pacing` (render/wait/remove decisions, pure), bounded sample buffer. `window.rs`: an `IsWindow` check so a window destroyed with its owner thread is recreated. Built-in cursor art, cursor pose and silhouette shadows, below |
| `auv-driver-overlay` | `LiveOverlay` facade (Windows adapter; `Unavailable` elsewhere) |
| `auv-driver-windows` | `OverlayApi::follow_operations` and the report sites below; `overlay_follow` example |

Report sites (`auv-driver-windows`, behind the `overlay` feature, after successful delivery):

| Site | Reports |
| --- | --- |
| `input::click_at` (foreground `SendInput`) | `Clicked` per press |
| `WindowApi::click` | `WindowTargeted`; background clicks also `Clicked` |
| `InputApi::move_mouse` (mouse zero) | The target window if any, then `Moved` per delivered sample |
| `InputApi::move_mouse_to` (mouse zero) | `Moved { Jump }` |

## Built-in cursor art

Resolves `TODO(driver-overlay-windows-builtin-art)` and
`TODO(driver-overlay-windows-silhouette-shadow)`.

The owner picked the pointer from four rendered candidates: the 12 x 12 brand sprite, a fine
1 px pixel arrow, a classic vector arrow and a rounded vector dart. They chose the dart
("Vector modern") at 24 px. `assets/cursor-pointer.svg` draws it:

- A white rim 3.12 units wide with round joins, then the fill and a 1.2 unit stroke of one
  gradient over its inner part, leaving about one unit of white outside the colored shape.
- Gradients per variant: AUV `#2fd3df` to `#0896a6`, pressed `#8cecf2` to `#25bccb`, the user
  cursor `#51647f` to `#2a3a52`.
- The hotspot is (1, 1) of the 24-unit box, the outermost point of the rounded tip, so the rim
  ends exactly on the point the operation acted on.
- Built-ins cast a soft drop shadow by default (black at 35%, blur 4, 1.5 px down); a
  transparent style shadow turns it off.

Every cursor shadow, built-in or custom, is now a blur of the art's own silhouette instead
of the radial glow #306 used. The posed art is rasterized a second time at the shadow's
offset, its alpha is blurred with a separable Gaussian (sigma = blur radius / 2, as on
macOS) and painted under the art, all on the CPU in the resvg bitmap. `Canvas::draw_glow`
and `Canvas::fill_circle` are gone. The pose is applied when rasterizing, so a tilted
cursor is a crisp vector rotation, not a resampled bitmap. A posed bitmap is capped at
2048 px per side, so an untrusted pose cannot blow up the allocation.

![the pointer at 4x: at rest on a light backdrop, arriving under a left-click ripple, tilted mid-flight, and pressed under a right-click ripple](assets/overlay-windows-cursor-art.png)

The old pixel arrow (`assets/cursor-pixel.svg`) is removed. Two deliberate gaps:

- The preview also drew short lime "burst" ticks around the tip during a press. They were
  left out: the dip and the lime ring already mark the click, and the owner asked for a more
  modern, quieter look.
- macOS still draws the 12 x 12 brand sprite with its mint glow
  (`BuiltInCursor::svg_source`). Giving macOS the same pointer is a separate slice.

## Evidence

Machine: Windows 11 Pro Insider Preview 10.0.29648, NVIDIA GeForce RTX 4070 Ti (not used:
the renderer is a software target), one 2560 x 1440 display at 100% scaling, 180 Hz.
Branch head before commit: `5a567ab0` plus this change. Runs on 2026-10-10 in release mode.

### Unit tests (cross-platform unless noted)

- `auv-driver-overlay-common` (23 motion tests): a jump follows the straight line, covers
  1 - 3/e^2 of the way at one time constant, never moves more than 2.1 px per millisecond over
  300 px and settles exactly; the scene goes idle and upright after a jump; a retarget keeps
  the cursor's speed (regression for the stop-and-restart); a retarget never passes its new
  target; a sideways turn swings at most 25% of the distance; the cursor never leaves the span
  of reported points; a 1 px/ms sampled stream is followed 15 to 35 px behind; tilt direction,
  limit and rest; no tilt on vertical moves; the press dips to 0.84 about the click point; the
  left-click ripple matches the macOS ripple; ripple growth, fade, bounds and colors; window
  marks.
- `auv-driver-overlay-windows` (Windows): `pacing` and `stats`; rasterizer tests for a
  clockwise tilt and a scale about the hotspot, an unblurred shadow exactly under the
  silhouette, a blurred shadow fading monotonically inside the bitmap, a transparent shadow,
  and rejected poses; window pixel tests for the tip on the target pixel, the white rim and
  cyan body, the pose about the tip, the default shadow on and off, distinct variants, and a
  custom SVG's silhouette glow.
- `auv-driver-windows` (10, `--features overlay`): one click event per press; window and label
  mapping; a movement reports nothing until it delivered a sample, jumps first and follows
  after; a window-targeted movement marks the window once; one follower per process.

### Timeline harness (Windows)

```text
cargo run --release -p auv-driver-overlay-windows --example overlay_motion_timeline -- <out-dir>
```

A scripted timeline (two window marks, two clicks 617 px apart, a 125 Hz sampled drag, a right
click) played through the real animator over an owned backdrop, with the screen captured
continuously. The harness finds the pointer in each capture by its body color, which no other
layer uses (the first run with the new art matched a window mark's antialiased corner; the
color test now also requires the pointer's saturation). The harness is for developing and
testing the animation; it is not the product.

![eight frames of the scripted timeline](assets/overlay-windows-motion-timeline.png)

| Measurement (final build) | Result |
| --- | --- |
| Frame time (compose + present) | P50 11.0 ms, P95 12.3 ms, max 14.3 ms, 157 frames |
| Late frames | 0 |
| Event latency (report to `present` returned) | P50 19.8 ms, P95 27.8 ms, max 29.6 ms |
| Click-to-click jump, 617 px | 56 captures strictly between the endpoints (23 with the old ease); largest step between captures 71 px, 12% of the distance (183 px before); at most 0.9 px off the straight path; the last capture in the window is 3 px from the point (the spring's last pixel or two of settling, plus the body color starting a pixel inside the white tip) |
| Sampled drag | The pointer was at most 57 ms (77 px at 1.33 px/ms) behind the driver's sample, including the 25 ms the stream's spring trails by. The check allows 75 ms: the pipeline's own delay (about 50 ms) plus that spring |

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
| Frame time | P50 11.0 ms, P95 12.4 ms, 91 frames, 0 late frames, 0 present failures |
| Event latency | P50 16.0 ms, P95 27.5 ms |

The OS pointer and the foreground window are printed before and after as information only: a
person using the machine can move either while this runs (a first version of this check failed
exactly because the machine's user moved their mouse), so the typed delivery record above is
the assertion. In the recorded run neither changed.

## Known limits

- **Cost.** A frame is a full virtual-screen software render, about 11 ms of every 16.7 ms
  while animating (about two thirds of one core, derived from frame time, not separately
  measured). The CPU silhouette shadow adds about half a millisecond over the radial glow. It is
  idle when nothing moves. Multi-monitor virtual screens, which make the bitmap larger, were
  not measured. Reusing the bitmap between frames or presenting only a dirty rectangle is a
  `canvas.rs` change and was left out on purpose.
- **Latency.** About one frame of pacing plus about one frame of render separate a driver
  event from the frame that shows it (P50 16 to 20 ms). The spring adds its own, on purpose: a
  jump covers about 60% of the way in 110 ms and settles in about half a second, and a sampled
  stream is drawn about 25 ms behind.
- **Tilt depends on frame times.** It is smoothed per drawn frame, so two runs with different
  frame timing tilt very slightly differently. Position never depends on frame timing.
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
  event until an out-of-process consumer needs it. The cursor pose is not on the Runner wire
  for the same reason.
- `TODO(overlay-live-theme)`: the host theme applies to one-shot `show` only.
- `TODO(overlay-live-macos)`: macOS already animates natively per `show`; routing live events
  there, and drawing the cursor pose, is a separate slice.
- `TODO(driver-overlay-windows-window-owner-thread)`: see Known limits.
- Still open from #306: `TODO(driver-overlay-windows-outline-label)`.

## Questions for the owner

1. Click ripples appear at the click's true time while the cursor may still be arriving
   (rule 3). The alternative, showing the ripple on arrival, looks tidier for a single click
   but hides clicks reported faster than the cursor travels. Keep the faithful behavior?
2. Should the evidence also include clicks into a real third-party application, or is
   harness-owned windows plus the typed delivery record enough?
3. Should macOS adopt the rounded pointer, and the spring for its own cursor moves, or keep
   the brand pixel sprite?
4. The press "burst" ticks from the preview were left out (see above). Add them back?
