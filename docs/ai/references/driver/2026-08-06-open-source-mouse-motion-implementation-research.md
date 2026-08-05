# Open-source mouse motion implementation research

Status: research note, 2026-08-06. This note compares implementation source,
not marketing claims. Links are pinned to the revisions inspected.

## Conclusion

The common computer/browser automation primitive is much simpler than an
animation system: move to an endpoint, optionally emit a fixed number of
linearly interpolated positions, and let the browser or OS accept those
events. Playwright, Puppeteer, PyAutoGUI, and Cua's input-delivery paths do not
model acceleration or jerk as first-class values.

Cua contains substantially richer motion code, but it belongs to its visual
agent-cursor overlay rather than the physical/background input-delivery
contract. It is useful evidence for keeping geometry, timing, and rendering
separate, not evidence that input drivers should own spring animation.

Projects that advertise "human-like" cursor movement usually combine a
reasonable geometric idea (Bezier paths, Fitts-law-inspired point counts, or
wind/gravity integration) with unseeded randomness. That is useful as an
optional style generator, but it is neither a deterministic motion contract
nor a principled acceleration-limited controller.

For AUV, keep the curve as spatial geometry and add a separate scalar motion
profile over mapped arc length. Implement deterministic fixed-duration easing
first, then an acceleration-limited triangular/trapezoidal profile. Derive
velocity and acceleration vectors from the sampled path; do not ask callers to
attach arbitrary velocity vectors to curve points.

## Findings by project

### trycua/cua

Cua exposes endpoint-oriented mouse control in its sandbox API. `move(x, y)`
forwards one `move_cursor` request, while `drag` currently forwards a path
containing only its start and end points ([sandbox mouse
interface](https://github.com/trycua/cua/blob/e3078d671c5a5449294b346773baf4fb5f344990/libs/python/cua-sandbox/cua_sandbox/interfaces/mouse.py#L8-L42)).
Its browser-backed `cuabot` path delegates a move directly to Playwright and
implements a drag as a straight move with `steps: 10`; it supplies no duration
or easing profile ([cuabot browser input](https://github.com/trycua/cua/blob/e3078d671c5a5449294b346773baf4fb5f344990/libs/cuabot/src/cuabotd.ts#L810-L880)).

The newer Rust driver contract gives drag explicit `duration_ms` and `steps`
fields ([typed drag input](https://github.com/trycua/cua/blob/e3078d671c5a5449294b346773baf4fb5f344990/libs/cua-driver/rust/crates/cua-driver-contract/src/inputs.rs#L504-L530)).
The macOS implementation describes and performs linear interpolation, sleeping
for `duration_ms / steps` between native drag events ([macOS drag
implementation](https://github.com/trycua/cua/blob/e3078d671c5a5449294b346773baf4fb5f344990/libs/cua-driver/rust/crates/platform-macos/src/input/mouse.rs#L526-L639)).
This is a wall-clock budget plus event count, not a velocity or acceleration
model.

Cua separately labels its agent cursor as a visual overlay for demos and
recordings ([overlay contract](https://github.com/trycua/cua/blob/e3078d671c5a5449294b346773baf4fb5f344990/libs/cua-driver/rust/Skills/cua-driver/SKILL.md#L241-L268)).
That overlay has two interesting implementations:

- Its motion configuration separates path-shape controls, fixed-duration or
  speed-based timing, minimum/peak speeds, a minimum Dubins turn radius, and a
  post-arrival spring ([motion configuration](https://github.com/trycua/cua/blob/e3078d671c5a5449294b346773baf4fb5f344990/libs/cua-driver/rust/crates/cursor-overlay/src/motion.rs#L5-L56)).
- Its render tick advances by path distance using the envelope
  `16 u^2 (1-u)^2`, then integrates a damped spring in four substeps after
  arrival ([render-state integration](https://github.com/trycua/cua/blob/e3078d671c5a5449294b346773baf4fb5f344990/libs/cua-driver/rust/crates/cursor-overlay/src/render_state.rs#L270-L347)).
  Geometry is sampled by arc distance from a minimum-turning-radius Dubins
  path, with a linear fallback ([path planner](https://github.com/trycua/cua/blob/e3078d671c5a5449294b346773baf4fb5f344990/libs/cua-driver/rust/crates/cursor-overlay/src/path_planner.rs#L1-L69)).

This overlay profile is deterministic and more structured than random
"human-like" movement, but it is not acceleration-bounded. The speed floor
also means its mathematical start/end velocity is not generally zero, and the
spring is a visual arrival effect rather than input accuracy policy.

An older Cua overlay helper implements WindMouse: gravity attracts the cursor
to the destination, random wind perturbs it, velocity is randomly clamped, and
the resulting path is downsampled to a target step count ([WindMouse
generator](https://github.com/trycua/cua/blob/e3078d671c5a5449294b346773baf4fb5f344990/libs/cuabot/src/mcp/overlay-cursor.py#L87-L143)).
It is explicitly stochastic and does not expose physical units, timestamps,
acceleration bounds, or reproducible seeds.

### Playwright and Puppeteer

Playwright stores its last logical pointer position. For `steps = N`, it emits
`N` positions at `i / N` along the straight segment and awaits each browser
protocol move. There is no per-step delay or easing
([Playwright `Mouse.move`](https://github.com/microsoft/playwright/blob/4b8f821f293861c2154181a0ddefbc52f8c45002/packages/playwright-core/src/server/input.ts#L212-L234)).
Chromium delivery is one `Input.dispatchMouseEvent` with type `mouseMoved` per
position ([Chromium adapter](https://github.com/microsoft/playwright/blob/4b8f821f293861c2154181a0ddefbc52f8c45002/packages/playwright-core/src/server/chromium/crInput.ts#L104-L117)).

Puppeteer's CDP implementation has the same shape: linear interpolation by
`steps`, sequential `Input.dispatchMouseEvent` calls, and no timing profile
([Puppeteer CDP mouse](https://github.com/puppeteer/puppeteer/blob/33566d2dbb6485a459b9fc3826914b986c08e01e/packages/puppeteer-core/src/cdp/Input.ts#L357-L378)).

In these APIs, `steps` means event density, not duration. Network/browser
processing determines the effective cadence. This is adequate for generating
DOM mouse-move events, but it is not a useful precedent for remotely executing
a curve at a requested speed.

### PyAutoGUI

PyAutoGUI cleanly separates a straight spatial interpolation from a scalar
tween. For a non-trivial duration it applies `tween(progress)` to the line
between start and end. Its scheduler is deliberately coarse: movements at or
below 100 ms become instant, and longer movements normally sleep at least
50 ms per position ([PyAutoGUI constants and sampler](https://github.com/asweigart/pyautogui/blob/b4255d0be42c377154c7d92337d7f8515fc63234/pyautogui/__init__.py#L556-L559),
[move loop](https://github.com/asweigart/pyautogui/blob/b4255d0be42c377154c7d92337d7f8515fc63234/pyautogui/__init__.py#L1478-L1514)).

Its public tween choices include polynomial, sine, exponential, circular,
elastic, back, and bounce families ([tween imports](https://github.com/asweigart/pyautogui/blob/b4255d0be42c377154c7d92337d7f8515fc63234/pyautogui/__init__.py#L67-L97)).
This is the closest mainstream example of the proposed AUV split between path
and timing. It still does not promise acceleration continuity or hard dynamic
limits; elastic/back/bounce profiles can reverse progress and overshoot.

### ghost-cursor

`ghost-cursor` constructs a cubic Bezier with randomly selected control points
and optionally chooses a random overshoot destination ([Bezier construction
and overshoot](https://github.com/Xetera/ghost-cursor/blob/5525a783fcf833a60f1b0746a8f80653ccfee2ba/src/math.ts#L30-L91)).
Its point count is influenced by a Fitts-law expression involving path length
and target width, plus a speed/random term ([path generation](https://github.com/Xetera/ghost-cursor/blob/5525a783fcf833a60f1b0746a8f80653ccfee2ba/src/spoof.ts#L316-L345)).
It can attach timestamps derived from local derivative estimates
([timestamp generation](https://github.com/Xetera/ghost-cursor/blob/5525a783fcf833a60f1b0746a8f80653ccfee2ba/src/spoof.ts#L347-L389)) and sends
each sampled point as a CDP `mouseMoved` event
([delivery loop](https://github.com/Xetera/ghost-cursor/blob/5525a783fcf833a60f1b0746a8f80653ccfee2ba/src/spoof.ts#L510-L542)).

Fitts' law is a defensible model for expected acquisition time, but this
implementation uses it mainly to select point count. Random anchors,
overshoots, delays, and speed defaults are heuristics. There is no bounded
acceleration/jerk solver, and unseeded randomness makes exact replay difficult.

## Implications for AUV

### Contract shape

Keep these three responsibilities distinct:

1. **Path geometry** maps local curve coordinates into logical screen space
   and supports position, tangent, curvature, and arc-length queries.
2. **Motion profile** maps monotonic time to monotonic arc distance. It owns
   duration, boundary speeds, and dynamic limits.
3. **Delivery scheduler** turns the resolved trajectory into timestamped
   position samples and reports what was planned and delivered.

Velocity is therefore derived as path tangent times scalar path speed.
Acceleration combines tangential acceleration from the profile with normal
acceleration from curvature. A caller-authored vector on every control point
would duplicate those facts and can contradict the path.

### Recommended profile sequence

1. Implement `FixedDuration { timing }` first, with `Linear` and a monotonic
   cubic-Bezier timing function. This covers animation authoring and follows
   PyAutoGUI's useful path/tween separation while allowing a smoother cadence.
2. Add `AccelerationLimited` as a deterministic triangular/trapezoidal
   arc-length profile with `max_speed`, `max_acceleration`,
   `max_deceleration`, `start_speed`, and `end_speed` in mapped logical-screen
   units. None of the reviewed input APIs provides this solver to copy; AUV
   should specify and test its feasibility rules directly.
3. Defer a jerk bound until an S-curve profile is implemented. Cubic easing
   may look smooth but is not a substitute for a quantified jerk limit.

For the first timing curve, reject non-monotonic progress rather than silently
accepting bounce/back/elastic behavior. Spatial overshoot belongs in explicit
path geometry, where pre-execution bounds validation can see it.

### Scheduling and streaming

Do not interpret client stream arrival time as motion time. A remote gRPC
connection adds jitter that should not become cursor acceleration. The client
should stream geometry/profile data; after validation, the server should
resolve the complete schedule against a local monotonic clock.

Schedule sample `i` against an absolute deadline (`start + i * period`) rather
than repeatedly sleeping for one period, so processing latency does not
accumulate as drift. When late, coalesce obsolete intermediate positions,
record lateness/drop counts, and always attempt the exact final point. Keep
sample rate configurable and bounded; browser projects show that event count
and time are independent, while PyAutoGUI shows that a low cadence can still
be operationally valid. Platform measurements should determine AUV defaults.

Progress events should label velocity and acceleration as **planned**. OS and
browser injection accepts positions, and compositor/event coalescing can make
observed kinematics differ. Observed motion requires separately timestamped
pointer readings and should be reported as evidence, not inferred from the
requested samples.

### Human-like mode

Do not make randomness part of the base motion profile. If it becomes useful,
add an explicit, seeded path-style generator that produces ordinary validated
geometry before execution. Persist the seed and generated curve in the run
record so replay is exact. Treat WindMouse, random Bezier anchors, and random
overshoot as style heuristics, not guarantees of human behavior.

Cua's separation is the strongest architectural precedent here: its richer
spring/Dubins animation is visibly identified as an overlay, while native
input delivery remains a smaller linear event contract. AUV should likewise
keep visual cursor flourish out of the semantic input result unless those
positions are actually delivered by the driver.
