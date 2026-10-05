# Scroll Motion, Live Scroll Control, and Scroll-Until

Date: 2026-10-06

## Status

Accepted by the owner on 2026-10-06 with these choices:

- a separate streaming RPC for timed scroll;
- deterministic timing first;
- velocity plus lease for live control;
- phased PRs.

Phase 1 (scroll motion and Linux high-resolution wheel) is implemented; see
[Phase 1 Implementation and Evidence](#phase-1-implementation-and-evidence).
Phase 2 (live scroll control) is implemented; see
[Phase 2 Implementation and Evidence](#phase-2-implementation-and-evidence).
Phase 3 (scroll-until) is implemented; see
[Phase 3 Implementation and Evidence](#phase-3-implementation-and-evidence).
The implemented shape replaces the provisional sketch in
[Scroll-until](#3-scroll-until-observation-loop). This note builds on the
[scroll delta contract](2026-10-06-scroll-delta-contract.md): logical pixels,
positive toward later content (down/right).

Owner direction (2026-10-06):

- Use cases: human-like scrolling, lazy-load loading, and speed changes driven
  by live observation.
- Build the declarative curve, the live control, and the SDK generator helper
  together.
- Try Linux high-resolution wheel output. If it does not validate, leave a
  `TODO`.
- Add a scroll-until operation. NetEase already has a private version.

## Problem

`WindowInput::scroll` delivers one wheel delta at once. Callers cannot:

- spread one scroll over time with a non-linear speed profile (human-like
  scrolling);
- change the speed while scrolling, based on what they observe;
- scroll until the end of a list, or until some content appears, without
  writing their own capture/compare loop. NetEase did this privately in
  `song.rs::seek_boundary` and `sidebar/scan.rs::scroll_to_top_by_motion`.

## Prior Art

- AUV mouse motion (`2026-08-05-mouse-motion-streaming-design.md`):
  - It samples a plan over time, skips overdue samples, can be cancelled, and
    streams progress.
  - Its planned (not yet implemented) timing model is
    `FixedDuration { duration, Linear | EaseInOutCubic | CubicBezier }` and
    `AccelerationLimited`.
- Maestro `scrollUntilVisible`, Appium `UiScrollable.scrollIntoView`, and
  Detox `whileElement().scroll()`: scroll in steps until an element is
  visible, bounded by a timeout or step budget.
- Puppeteer/Playwright recipes for infinite lists: scroll until the page height
  stops growing.
- NetEase: after each scroll, compare a region crop with the previous crop
  (shift search, normalized pixel diff). Two consecutive "no motion" results
  after a successful delivery mean the boundary is likely. An AX scrollbar at
  the boundary lowers the threshold to one. A scroll budget bounds the loop.

## Design

Three layers, each usable alone:

```text
scroll-until (observation loop)        <- lazy-load, "until text visible"
  live scroll control (velocity stream) <- speed changes driven by observation
    scroll motion (timed plan)          <- human-like curve
      WindowInput::scroll (one delta)   <- existing
```

### 1. Scroll motion (timed plan)

*Provisional* types in `auv-driver-common`:

```rust
pub struct ScrollMotion {
  pub total: Scroll,            // logical px, positive = down/right
  pub timing: MotionTiming,
  pub sample_rate_hz: u32,      // > 0 when duration > 0
}

pub enum MotionTiming {
  FixedDuration { duration: Duration, function: TimingFunction },
  // TODO(scroll-acceleration-limited): AccelerationLimited from the mouse
  // kinematics plan; add when a caller needs speed/acceleration limits
  // instead of a fixed duration.
}

pub enum TimingFunction {
  Linear,
  EaseInCubic,
  EaseOutCubic,
  EaseInOutCubic,
  CubicBezier { x1: f64, y1: f64, x2: f64, y2: f64 }, // CSS semantics, x in [0, 1]
}
```

- `TimingFunction` maps normalized time to normalized progress. It is shared
  vocabulary: mouse motion can adopt it later instead of defining its own.
- Sampling is on demand, as in `MouseSamples`. Sample `k` has a progress
  `p_k`. The executor quantizes the **cumulative** target
  `round(total_native * p_k)` and sends the difference from the previous
  sample. The sum is therefore exact, even when one sample is smaller than
  one native unit.
- Execution lives on `WindowInput` (*provisional*
  `scroll_motion(window, point, motion, options)`), so each platform
  quantizes in its own unit:
  - macOS: 1 pixel;
  - Windows: 1 wheel unit (1/120 notch);
  - Linux uinput: hi-res unit (1/120 notch) if validated, otherwise a notch.
- Delivery path selection runs once, at the start. Each sample then uses the
  selected path. A later sample failure ends the motion with a partial-progress
  error and does not fall back mid-motion.
- Timing reuses the mouse executor's approach: wait for each sample's due
  time, skip overdue samples arithmetically, and honor invoke cancellation
  between samples.
- The result is `InputActionResult` plus the delivered total in logical px.

### 2. Live scroll control (velocity stream)

For speed changes during observation, the caller controls **velocity**. A
list of deltas would be the wrong control. *Provisional* bidirectional RPC
`InputService/StreamScroll`:

```text
client -> Begin { window, point, options(policy, candidates), sample_rate_hz, max_acceleration?, lease }
client -> SetVelocity { velocity_x, velocity_y }   // logical px/s, any number of times
client -> Stop                                      // or Cancel
server -> Started { selected_path }
server -> Progress { elapsed, delivered: Scroll, velocity }   // latest-value mailbox
server -> Completed { delivered, action } | Failed { delivered, error }
```

- The Runner integrates velocity into deltas at `sample_rate_hz`. It uses the
  same cumulative quantization as scroll motion.
- With `max_acceleration` set, the Runner ramps toward the requested velocity
  instead of jumping. This gives human-like starts and stops without client
  timing.
- Safety:
  - `lease` is a required keep-alive. If no `SetVelocity` arrives within the
    lease, velocity drops to zero and the stream completes.
  - A client disconnect stops delivery immediately.
- Unlike `StreamMouseMotion`, execution starts at `Begin`. This is the
  "live execution" that the mouse design deferred; it is accepted here for
  scroll only.
- Progress reports delivery only. Observation (captures) stays with the
  caller.

SDK sugar (B1):

- `window.scrollStream(options)` returns a controller with `setVelocity`,
  `stop`, and an async `progress` iterator.
- `window.scrollWith(generator)` consumes an `async function*` that yields
  `{ velocityX?, velocityY?, holdMs? }` and drives the stream. User code
  (generators, closures) stays client-side. The wire carries only data, so
  runs remain inspectable.

### 3. Scroll-until (observation loop)

*Provisional* operation `scroll until` with a typed result. It generalizes the
NetEase loop:

```text
ScrollUntil {
  window, point, direction(down|up|left|right), step: Scroll | ScrollMotion,
  condition: End | TextVisible { query, region? },
  region: Option<RatioRect>,          // area compared for motion; default: whole window
  settle, max_steps, no_motion_confirmations (default 2), policy
}
-> ScrollUntilResult { stop_reason, steps, delivered: Scroll, matches?, last_capture artifact }
```

- `End`: after each step, capture the window (or region) and compare it with
  the previous capture. N consecutive "no motion" results after successful
  deliveries stop with `stop_reason = end_by_no_visual_progress`.
  - Lazy loading is the reason N is at least 2: a load can add content after
    a pause. The default settle gives that content time to arrive.
  - The claim follows the completeness rule in `TERMS_AND_CONCEPTS.md`:
    "no visual progress observed" is not proof that no more content exists.
- `TextVisible`: OCR the window after each step. Stop when the query matches
  (`text_visible`). This is the Maestro-style `scrollUntilVisible`.
- Bounded by `max_steps`; ending at the budget sets `budget_exhausted`.
- The pixel motion comparison moves from NetEase
  (`scroll/policies/detection_motion.rs`) into a shared core module, so NetEase
  and core use one implementation.
  - It must also handle horizontal shift, which NetEase does not.
  - NetEase migration is a follow-up, not part of this slice.
- Placement: the loop runs where capture happens. For the selected-Runner
  route it is one Runner RPC (*provisional* `ScrollUntil`), so screenshots do
  not cross the network on each step. Local invoke calls the same typed
  operation in-process.
- Exposure:
  - invoke `input.scrollUntil --until end|text:<query>`;
  - MCP via the registry;
  - Rust and JS clients.

### 4. Linux high-resolution wheel

- uinput: advertise and emit `REL_WHEEL_HI_RES`/`REL_HWHEEL_HI_RES`
  (120 units per notch). Also emit legacy `REL_WHEEL`/`REL_HWHEEL` when the
  cumulative hi-res total crosses a notch, per the kernel convention. Then
  validate on GNOME that Chromium receives sub-notch deltas.
- Portal: v2 has no high-resolution discrete API. The continuous axis showed
  12x scaling and kinetic scrolling. Try it once without `finish`. If that is
  still not exact, keep notches for the portal with
  `TODO(linux-portal-hi-res-wheel)`.

## Validation Plan

Extend the Chrome/Electron probe page so it records a `scrollTop` timeline
(timestamped samples):

- Motion: delivered total is exact. The `scrollTop(t)` shape follows the
  timing function within the application's own smoothing (Chromium animates
  wheel input). The claim is limited to event timing.
- Live control: velocity changes appear in the timeline. A lease expiry or
  disconnect stops it.
- Scroll-until: a lazy-loading page appends rows after a delay when near the
  bottom. `End` must not stop before the final batch, and must stop after
  it. `TextVisible` stops on a marker row.
- Platforms: macOS (Chrome, Electron), Windows (Edge, foreground), Linux GNOME
  (portal, uinput; hi-res where validated).

## Phasing

1. Scroll motion: types, executors, wire (`ScrollWindowPointMotion` or a
   `motion` field; see open questions), invoke flags, SDK. Linux hi-res
   attempt.
2. Live scroll control: `StreamScroll`, SDK `scrollStream`/`scrollWith`.
3. Scroll-until: shared motion comparison, the `ScrollUntil` operation and
   RPC, the invoke command.

Each phase lands with unit tests and live evidence before the next starts.

## Phase 1 Implementation and Evidence

What landed:

- `auv-driver-common::scroll_motion`:
  - `TimingFunction` (`Linear`, `EaseInCubic`, `EaseOutCubic`,
    `EaseInOutCubic`, `CubicBezier` with CSS semantics);
  - `MotionTiming::FixedDuration`;
  - `ScrollMotion`, an on-demand `ScrollMotionSchedule`, and
    `CumulativeQuantizer`;
  - `run_window_scroll_motion`.
- `WindowInput` gains two default methods:
  - `scroll_quantum` (logical px per native wheel unit);
  - `scroll_motion`.
- Each platform sets its quantum:
  - macOS: 1 px;
  - Windows: 100/120 px;
  - Linux uinput: 1 px (high-resolution);
  - Linux portal: 120 px.
- Linux uinput now emits `REL_WHEEL_HI_RES`/`REL_HWHEEL_HI_RES`, plus legacy
  notches at notch boundaries. Instant uinput scrolls are therefore
  pixel-precise too. The portal keeps notches
  (`TODO(linux-portal-hi-res-wheel)`).
- Wire: `InputService/ScrollWindowPointMotion` (server streaming). It uses the
  `ScrollMotion`, `MotionTimingFunction`, `FixedDurationMotionTiming`, and
  started/progress/completed events.
- Runner handler: progress uses a latest-value mailbox. A client disconnect
  aborts the task, and its cancellation guard stops the next sample.
- Rust client: `WindowClient::scroll_motion` returns a `ScrollMotionStream`.
- JS SDK: `WindowClient.scrollMotion`.
- `auv invoke input.scroll` takes three new flags, on both the local and the
  selected-Runner route:
  - `--duration-ms` (0..=60000);
  - `--easing` (`linear|ease-in|ease-out|ease-in-out|cubic-bezier:x1,y1,x2,y2`);
  - `--sample-rate-hz` (1..=1000, default 60).

Deferred with code-site markers:

- `TODO(scroll-acceleration-limited)`
- `TODO(scroll-motion-admission)`: each sample takes its own input admission.
- `TODO(scroll-motion-duration-limit)`
- `TODO(linux-portal-hi-res-wheel)`

Live evidence, 2026-10-06. Each case ran `auv invoke input.scroll ...
--duration-ms` against the probe page, which records a per-frame
`scrollTop`/`scrollLeft` timeline. Progress is sampled at 25/50/75% of the
observed motion window:

| Platform / path | Cases | Delivered vs observed | Curve vs ideal (25/50/75%) |
| --- | --- | --- | --- |
| macOS Chrome, window-targeted wheel | linear, ease-in, ease-out, ease-in-out; 600 px / 800 ms | Exact (600) | Linear 0.27/0.50/0.75; ease-in-out 0.10/0.50/0.92 vs 0.06/0.50/0.94 |
| macOS Electron, window-targeted wheel | ease-in-out −480 px; cubic-bezier(0.2,0.8,0.2,1) +300 px horizontal; ease-out 900 px document | Exact | Ease-out 0.58/0.86/0.97 vs 0.58/0.88/0.98 |
| macOS Chrome, foreground HID | ease-in-out 450 px | Exact | Follows ideal within smoothing |
| macOS selected Runner (isolated daemon) | ease-in-out 600 px; linear −240 px horizontal; linear 3000 px / 5000 ms | Exact; the 5 s motion took 5001 ms | Linear 0.25/0.50/0.75 |
| macOS selected Runner, client killed (SIGKILL) after 1.5 s of a 3000 px / 5 s motion | — | Stopped at +880 px and stayed there | — |
| Windows 11 Edge, `SendInput` (foreground) | linear 600; ease-in-out 600; ease-out −450; cubic-bezier +300 horizontal; ease-in 900 document | Exact, except one ease-in-out run that observed 608.7 for 720 delivered units (600 px) | Linear 0.25/0.50/0.75 exactly |
| Windows 11 Edge, posted messages (Edge foreground) | ease-in-out 600 | Exact | 0.10/0.50/0.92 |
| Linux GNOME, uinput hi-res | linear 600; ease-in-out 600; ease-out −450; ease-in +300 horizontal; ease-in-out 900 | 0-1.6% short (for example 886 of 900); Chromium received 886 in wheel deltas | Front-loaded; shorter observed duration |
| Linux GNOME, uinput hi-res instant | 30, −45, 7 px vertical; 60 px horizontal; 120 px | 60 and 120 exact; 30, −45, 7 not delivered | — |
| Linux GNOME, portal (CLI default, `auv invoke input.scroll --duration-ms`) | linear 600; ease-in-out 600; ease-out −480; ease-in +360 horizontal; ease-in-out 1200 document | Exact | Coarse: whole 120 px notches, so 600 px is five steps (linear 0.27/0.55/0.78) |

The Chromium probe smooths each wheel event, so curve comparisons are
approximate. Durations look shorter for ease-in, because the first movement
appears late.

Linux findings:

- libinput holds a new high-resolution wheel movement until it reaches half
  a notch (60 units). An isolated scroll below 60 px therefore never reaches
  the application. The held amount is released at once when the threshold is
  crossed: the first Chromium delta in the 900 px case was 113.
- Small trailing samples lose fractional pixels before Chromium, about 1.5%
  on a 900 px ease-in-out.

These are compositor/libinput behaviors on GNOME. The AUV conversion is
exact: the driver delivers the requested hi-res units.

The portal route delivered exact totals because each sample is a whole
discrete notch. Neither the libinput threshold nor fractional rounding
applies.

Portal consent for this run was approved by AUV itself, at the owner's
explicit request. A test-only helper used the Linux driver's AT-SPI support
while `auv doctor --portal-authorize` was waiting:

- it found the `xdg-desktop-portal-gnome` windows "Remote Desktop" and
  "Share Screen";
- it activated the "Allow Remote Interaction" switch (off by default; without
  it the session starts with no keyboard or pointer access);
- it activated "Share" in each dialog with `select_node` (`AxPress`).

This is a validation technique for an owner-controlled machine, not a product
feature. The saved tokens were deleted after the run.

## Phase 2 Implementation and Evidence

What landed:

- `auv-driver-common::scroll_stream`:
  - `ScrollVelocity`, with a ±50,000 px/s limit (`NOTICE(scroll-stream-velocity-limit)`);
  - `ScrollStreamOptions`: sample rate 1..=1000, an optional
    `max_acceleration` in px/s², and a lease in (0, 60s];
  - `ScrollStreamControl` with `set_velocity`, `stop`, and `cancel`;
  - `ScrollStreamStopReason` (`stopped`, `cancelled`, `lease_expired`);
  - `run_window_scroll_stream`.
- How the executor works:
  - It integrates velocity into a cumulative position and quantizes it with
    the phase 1 `CumulativeQuantizer::step_to`, so the delivered total is
    exact.
  - It ramps toward each requested velocity under `max_acceleration`.
  - `stop` and lease expiry ramp to zero before completing. `cancel` ends at
    the next sample.
  - Like timed motion, it pins the first selected delivery path.
- `WindowInput::scroll_stream` default method.
- Wire: `InputService/StreamScroll` (bidirectional). It uses
  `StreamScrollBegin`, `StreamScrollSetVelocity`, `StreamScrollStop`, and
  `StreamScrollCancel`, and returns `started`, `progress` (latest value), and
  `completed` events. `completed` carries the delivered total, an optional
  action (absent when nothing moved), the stop reason, and the elapsed time.
- Runner behavior:
  - A half-closed request stream counts as a stop.
  - An invalid velocity or out-of-order event cancels the stream with
    `INVALID_ARGUMENT`.
  - A disconnect aborts the native task.
- Rust client: `WindowClient::scroll_stream` returns a `ScrollStreamSession`
  with `set_velocity`, `stop`, `cancel`, and `next`.
- JS SDK:
  - `WindowClient.scrollStream(begin)` returns a controller with
    `setVelocity`, `stop`, `cancel`, and `events`.
  - `WindowClient.scrollWith(generator, begin)` consumes a sync or async
    generator of `{ velocityX?, velocityY?, holdMs? }`. It renews the lease
    while a step holds, and stops when the generator finishes. Generators stay
    client-side; only velocity data crosses the wire.

Not exposed through `auv invoke` or MCP: live control needs a client that keeps
a stream open (`TODO(scroll-stream-invoke)`). The CLI keeps timed scroll for
one-shot use.

Live evidence, 2026-10-06. macOS Chrome with occlusion backgrounding disabled.
The full path was the JS SDK (`tsx`), an isolated local daemon, its Runner,
and window-targeted wheel delivery:

| Case | Result |
| --- | --- |
| `scrollWith` profile 300 → 900 → 1500 → 600 px/s, 300 ms each, `max_acceleration` 6000 | Observed speed (6-frame windows) about 300 → 650-1100 (ramping) → 1500 → 600 → 0; delivered 1014 px = observed 1014 px; `stopped` |
| Observation-driven generator: reads `scrollTop` over CDP each step and slows as the target nears | Target +1500 px, stopped at +1501 px |
| `scrollStream` with a 300 ms lease and one update at 600 px/s | `lease_expired` after 404 ms; delivered 173 px |
| Abort the SDK call 400 ms into 1000 px/s | 314 px before the abort, 0 px after |

Windows and Linux: unit tests and native builds pass (see the PR). Each stream
sample uses the same per-platform `WindowInput::scroll` delivery that phase 1
validated live on Windows Edge and Linux GNOME. A separate live stream run on
those hosts was not performed.

## Phase 3 Implementation and Evidence

What landed:

- `auv-scan::viewport_pixels`:
  - `compare_viewport_pixels(before, after, axis, policy)` searches a bounded
    shift along `ScrollAxis::Vertical` or `Horizontal`. It reports
    `ViewportPixelMotion { estimated_shift, normalized_diff, no_motion }`.
  - `ViewportPixelPolicy` defaults come from the NetEase sidebar policy:
    ±24 px search, 0.01 threshold, stride 4
    (`NOTICE(viewport-pixel-defaults)`).
  - Images of different sizes count as motion, so a resize never looks like
    a stuck viewport.
  - NetEase still uses its own copy and carries
    `TODO(netease-core-viewport-pixels)` for the migration.
- `auv-scan::scroll_until`:
  - `ScrollUntilRequest` has these fields:
    - `step`: `Instant { delta }` or `Motion { motion }`;
    - `condition`: `End` or `TextVisible { query }`;
    - `max_steps`: 1..=1000;
    - `settle`: at most 10 s;
    - `no_motion_confirmations`: 1..=10;
    - `motion_region`: an optional normalized rectangle.
  - The step decides the axis, so there is no separate `direction` field. A
    step must move exactly one axis by a non-zero amount.
  - `scroll_until(surface, request, notify)`:
    - Captures once and checks the text condition before the first step.
    - Each round is step → settle → one capture → motion comparison → text
      check → end check.
    - `End` needs `no_motion_confirmations` consecutive no-motion rounds.
  - `ScrollUntilResult` holds:
    - the stop reason: `end_by_no_visual_progress`, `text_visible`, or
      `budget_exhausted`;
    - the step count and the delivered total;
    - the first step's `InputActionResult`;
    - the text match, with bounds in screen coordinates;
    - the last motion.
  - `ScrollUntilSurface` is the dependency-injection boundary for scroll,
    capture, OCR, and waiting. `WindowScrollUntilSurface` binds it to a
    `LocalDriverSession` window. Its OCR searches the whole window, and its
    waits check cancellation.
  - Deferred:
    - `TODO(scroll-until-artifacts)`: no per-step capture artifacts;
    - `TODO(scroll-until-ax-boundary)`: no accessibility boundary check;
    - text search has no separate region.
- Wire: `InputService/ScrollUntil` is a server-streaming RPC that sends
  `progress` events, then one `completed` event. The loop runs on the Runner,
  so screenshots never cross the network.
- Rust client: `WindowClient::scroll_until`, which returns a stream of
  `ScrollUntilEvent`.
- JS SDK: `WindowClient.scrollUntil(point, request, options)`.
- Invoke and MCP: `auv invoke input.scrollUntil <x> <y> (--dx|--dy) --until
  end|text:<query>`, with `--max-steps`, `--settle-ms`, `--confirmations`,
  `--region`, and the timed-step flags from phase 1. MCP reaches it through
  the registry `invoke` tool.

Choosing a step:

- **Text search:** keep the step at about 60-70% of the visible height (or
  width). Each line then appears whole in at least one observation. A step
  close to the viewport size can cut the target line at the edge every time.
- **`End` and lazy loading:** a round takes at least `settle` plus capture
  time. The rounds a load needs to show its content must fit within
  `no_motion_confirmations` rounds; otherwise raise `settle` or the
  confirmation count. "No visual progress" is observation, not proof that no
  more content exists.

Live evidence, 2026-10-06. The probe was a lazy-loading feed: near the bottom
it appends a 40-row batch after a delay, four times, then shows `END OF FEED`.
Row 137 is `TARGET ROW 137`.

| Platform and route | Case | Result |
| --- | --- | --- |
| macOS Chrome (occlusion backgrounding off), direct invoke | `--dy 700 --until end`, 700 ms load | `end_by_no_visual_progress` after 17 steps; all 4 batches loaded; `scrollY` = max (7528) |
| macOS Chrome, direct invoke | `--dy 700 --until 'text:TARGET ROW 137'` | `text_visible` at step 9 |
| macOS Chrome, direct invoke | 1500 ms load with the default settle (400 ms); then `--settle-ms 1700` | Both ended after the final batch (a round took about 0.95 s with the default) |
| macOS Chrome, direct invoke | Timed steps (`--step-duration-ms`) | Correct end |
| macOS Chrome, isolated daemon Runner | Same end and text cases | Same results as direct invoke |
| macOS, validation | `--max-steps 2000` | Rejected |
| Windows Edge (foreground) | `--dy 700 --until end`; timed steps | `end_by_no_visual_progress` after 20 steps; timed run 11 steps; all batches loaded |
| Windows Edge (foreground) | `--dy 700 --until 'text:TARGET ROW 137'` | Missed (ran to the end): the step was close to the viewport height, so the row was always cut at an edge |
| Windows Edge (foreground) | `--dy 500 --until 'text:…'`, instant and timed steps | `text_visible` at step 11 in both |

Linux GNOME Wayland (portal route): not validated live. Unit tests pass and the
CLI builds. Two existing `auv-driver-linux` capture problems block the
observation loop:

- **Repeated captures return a stale frame.** With one driver session, the
  persistent PipeWire receiver drops frames that arrive while no request is
  pending. A later request waits 100 ms (`PIPEWIRE_REFRESH_WAIT`), then
  returns the last decoded frame. A probe scrolled the page 300 px between
  five captures: all five images had the same hash. As a result, scroll-until
  always reports "no motion" and stops as `end_by_no_visual_progress` after
  `no_motion_confirmations` steps, even though the page moved.
- **No first frame with a fullscreen window in front.** With a fullscreen
  Electron window in front, the first PipeWire frame did not arrive within
  5 s; this is likely GNOME direct scanout. The Screenshot portal fallback
  then opened GNOME's "Allow Applications to Take Screenshots?" dialog, which
  also swallowed the wheel input. With the window maximized instead, capture
  worked.

Both problems belong to the Linux capture driver, not to scroll-until. They are
tracked in [#244](https://github.com/moeru-ai/auv/issues/244) (stale frames)
and [#245](https://github.com/moeru-ai/auv/issues/245) (fullscreen first
frame), with reproduction steps and probe files.

## Open Questions

Resolved: motion uses a separate server-streaming RPC, and timing is
deterministic first.

- `TODO(scroll-motion-jitter)`: human-like jitter needs a seeded randomness
  contract for replay.
- Mouse motion can adopt `TimingFunction` in its own slice.
