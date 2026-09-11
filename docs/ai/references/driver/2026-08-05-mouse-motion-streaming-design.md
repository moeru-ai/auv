# Mouse motion streaming design

Status: accepted implementation slice, 2026-08-05.

## Contract

`InputService` exposes two pointer-motion operations:

- `MoveMouse` accepts one complete motion plan and streams started,
  progress, and completed events.
- `StreamMouseMotion` is bidirectional. The client sends one begin event,
  ordered segment batches, and then finish or cancel. The server streams
  acceptance, execution progress, and a terminal event.

A motion plan contains a start policy, a local vector curve, a mapping, and
timing options. The start is either an explicit logical `ScreenPoint` or the
pointer position resolved immediately before execution. The curve is a chain
of cubic Bezier segments in a local coordinate space.

The local editor is deliberately not a remote-desktop viewport. For a curve
point `p`, curve origin `o`, resolved screen start `s`, and mapping dimensions
`m`, the delivered point is:

```text
s + ((p.x - o.x) * m.width, (p.y - o.y) * m.height)
```

The editor stays in a compact, fixed-size window. The caller selects the map
from normalized displacement to logical screen coordinates.

## Execution and safety

The driver validates finite coordinates, positive mapping dimensions, bounded
sample rate and duration, and the complete mapped trajectory before delivering
the first point. The sampler flattens cubic curves and uses mapped arc length.
As a result, progress has an approximately constant speed across segments.

The first bidirectional implementation buffers segment batches until finish.
This preserves all-or-nothing validation and avoids leaving the pointer at a
partially executed position when a later batch is invalid. Cancellation before
finish has no pointer effect. A disconnect during execution stops delivery at
the last sample.

`MouseMotionCompleted.action` is input-delivery evidence. It is not semantic
verification of any application state.

## Deferred boundaries

- Live execution before `finish` is deferred. The current duration applies to
  the complete curve. A per-batch or absolute timing contract can unlock this
  behavior after owner approval.
- The vector editor does not display or capture the remote desktop. Screenshot
  composition belongs to capture/viewer surfaces, not input delivery.
- Easing functions and pressure or velocity input are deferred until a concrete
  producer needs them. Arc-length timing is the single V1 policy.

## Planned kinematics extension

The vector editor produces acceleration-aware timing. The next contract
revision must separate path geometry from motion timing. It must not attach an
unconstrained velocity vector to each curve point.

For an arc-length-parameterized screen path `r(s)` and scalar distance profile
`s(t)`:

```text
screen velocity     = dr/ds * ds/dt
screen acceleration = d²r/ds² * (ds/dt)² + dr/ds * d²s/dt²
```

Path curvature causes the first acceleration term. Motion timing selects the
second, tangential term. Constant-speed motion still accelerates during a turn.

### Proposed domain interface

Replace the duration-only `MouseMotionOptions` field with one timing value:

```rust
pub struct MouseMotionOptions {
  pub timing: MouseMotionTiming,
  pub sample_rate_hz: u32,
}

pub enum MouseMotionTiming {
  FixedDuration {
    duration: Duration,
    timing: MouseTimingFunction,
  },
  AccelerationLimited {
    max_speed: f64,
    max_acceleration: f64,
    max_deceleration: f64,
    start_speed: f64,
    end_speed: f64,
  },
}

pub enum MouseTimingFunction {
  Linear,
  EaseInOutCubic,
  CubicBezier { x1: f64, y1: f64, x2: f64, y2: f64 },
}
```

Speed uses logical screen units per second after `MouseCurveMapping` applies.
Acceleration uses logical screen units per second squared. Boundary speeds are
scalar magnitudes along the path tangent. The curve owns direction. Thus, a
caller cannot provide a velocity that points away from the path.

`FixedDuration` answers animation-authoring needs. Its timing function maps
normalized elapsed time to normalized arc-length progress, so ease-in/out
naturally produces acceleration. `AccelerationLimited` answers automation
needs: the sampler derives the minimum-time triangular or trapezoidal speed
profile that satisfies the limits. `max_deceleration` is separate because
human-like pointer motion often brakes differently from how it accelerates.

The first extension does not include a jerk limit. A jerk limit requires an
S-curve solver and more feasibility rules. Add the limit with that solver and
its tests.

### Proposed wire projection

`MouseMotionOptions` must own a `oneof timing` containing dedicated
`MouseFixedDurationTiming` and `MouseAccelerationLimits` messages.
`MouseTimingFunction` must use concrete standard variants and a typed
`MouseCubicBezierTimingFunction`. It must not reuse overlay easing. Pointer
timing has different validation, units, and evolution.

Add a semantic vector message and planned kinematics to progress:

```proto
message MouseMotionVector {
  double x = 1;
  double y = 2;
}

message MouseMotionProgress {
  uint32 sample_index = 1;
  ScreenPoint point = 2;
  google.protobuf.Duration scheduled_elapsed = 3;
  double path_progress = 4;
  MouseMotionVector planned_velocity = 5;
  MouseMotionVector planned_acceleration = 6;
}
```

The field names deliberately say `planned`: ordinary OS pointer injection
accepts positions, not velocity, and scheduler or compositor latency can make
observed motion differ. Actual observed velocity would require timestamped
pointer observations and belongs in a separate evidence field.

`MouseMotionStarted.duration` remains the resolved duration. For an
acceleration-limited profile it is solver output rather than caller input. It
must also report mapped path length. Animation tools can then explain the
resolved timing.

### Validation and streaming

- Timing-control X coordinates must be in `[0, 1]` and monotonic so elapsed
  time has one progress value.
- Speeds and acceleration limits must be finite and non-negative. Maximum
  values must be positive. Boundary speeds must not exceed `max_speed`.
- A non-zero speed across a Bezier segment join requires tangent continuity.
  A sharp corner must either resolve to zero speed or be rejected before any
  pointer delivery.
- The complete timing value is part of `MoveMouse.plan` or
  `StreamMouseMotionBegin`. Append packets continue to carry geometry only.
- Both RPCs use the same sampler. The GUI, CLI, Balatro integration, and
  driver server must not implement separate easing or acceleration math.

The editor will display the derived velocity arrow at the playhead. It can also
display acceleration as a second arrow. A geometric tangent changes Bezier
control points. A speed control changes motion timing. These controls prevent
ambiguous curves.
