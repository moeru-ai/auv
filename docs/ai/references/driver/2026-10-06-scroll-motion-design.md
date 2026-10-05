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
Phases 2 and 3 are not implemented yet. Names marked *provisional* in those
sections may still change. This note builds on the
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

Not live-validated: timed scroll through the Linux portal (CLI default). The
consent dialog timed out on 2026-10-06. Its behavior follows from the
instant-scroll evidence, but it is coarse: whole 120 px notches.

## Open Questions

Resolved: motion uses a separate server-streaming RPC, and timing is
deterministic first.

- `TODO(scroll-motion-jitter)`: human-like jitter needs a seeded randomness
  contract for replay.
- Mouse motion can adopt `TimingFunction` in its own slice.
