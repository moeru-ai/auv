//! Live scroll control: the caller steers a velocity while the driver delivers
//! wheel input at a fixed sample rate.
//!
//! The executor integrates velocity into a cumulative position and quantizes
//! it like timed scroll motion, so the delivered total stays exact. A lease
//! bounds how long delivery continues without fresh caller input. See
//! `docs/ai/references/driver/2026-10-06-scroll-motion-design.md`.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::input::{InputActionResult, Scroll, ScrollOptions};
use crate::input_cancellation::current_input_cancellation;
use crate::scroll_motion::{CumulativeQuantizer, partial_failure, pinned_options, wait_until};
use crate::{DriverError, DriverResult, Window, WindowInput, WindowPoint};

// NOTICE(scroll-stream-velocity-limit): 50,000 logical px/s is far above any
// human wheel or fling speed, and bounds an accidental runaway value from a
// caller bug before it reaches native input.
pub const MAX_SCROLL_VELOCITY: f64 = 50_000.0;
const MAX_SCROLL_STREAM_SAMPLE_RATE_HZ: u32 = 1_000;
// NOTICE(scroll-stream-lease-limit): one minute without renewal is longer than
// any interactive control loop needs; shorter leases stop faster when a caller
// stalls or disconnects without closing the stream.
pub const MAX_SCROLL_STREAM_LEASE: Duration = Duration::from_secs(60);

/// Scroll velocity in logical pixels per second; positive is toward later
/// content (down/right).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScrollVelocity {
  pub delta_x_per_second: f64,
  pub delta_y_per_second: f64,
}

impl ScrollVelocity {
  pub const ZERO: Self = Self::new(0.0, 0.0);

  pub const fn new(delta_x_per_second: f64, delta_y_per_second: f64) -> Self {
    Self {
      delta_x_per_second,
      delta_y_per_second,
    }
  }

  pub fn validate(&self) -> DriverResult<()> {
    for value in [self.delta_x_per_second, self.delta_y_per_second] {
      if !value.is_finite() || value.abs() > MAX_SCROLL_VELOCITY {
        return Err(invalid(format!("scroll velocity must be finite and within ±{MAX_SCROLL_VELOCITY} px/s")));
      }
    }
    Ok(())
  }

  fn is_zero(&self) -> bool {
    self.delta_x_per_second == 0.0 && self.delta_y_per_second == 0.0
  }
}

/// Fixed parameters of one live scroll stream.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScrollStreamOptions {
  /// Samples per second (1..=1000).
  pub sample_rate_hz: u32,
  /// Optional limit on velocity change in logical px/s²; `None` jumps to each
  /// requested velocity.
  pub max_acceleration: Option<f64>,
  /// Delivery ramps to zero and the stream completes when no velocity update
  /// arrives within this lease.
  pub lease: Duration,
}

impl ScrollStreamOptions {
  pub fn validate(&self) -> DriverResult<()> {
    if !(1..=MAX_SCROLL_STREAM_SAMPLE_RATE_HZ).contains(&self.sample_rate_hz) {
      return Err(invalid(format!("scroll stream sample_rate_hz must be within 1..={MAX_SCROLL_STREAM_SAMPLE_RATE_HZ}")));
    }
    if let Some(limit) = self.max_acceleration
      && (!limit.is_finite() || limit <= 0.0)
    {
      return Err(invalid("scroll stream max_acceleration must be finite and positive"));
    }
    if self.lease.is_zero() || self.lease > MAX_SCROLL_STREAM_LEASE {
      return Err(invalid("scroll stream lease must be within (0, 60s]"));
    }
    Ok(())
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollStreamStopReason {
  /// The caller requested a stop; delivery ramped to zero first.
  Stopped,
  /// The caller cancelled; delivery ended without ramping.
  Cancelled,
  /// No velocity update arrived within the lease; delivery ramped to zero.
  LeaseExpired,
}

/// Latest delivery progress of a live scroll stream.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScrollStreamProgress {
  pub elapsed: Duration,
  /// Logical pixels delivered so far.
  pub delivered: Scroll,
  /// Current (possibly ramping) velocity.
  pub velocity: ScrollVelocity,
}

/// Completed live scroll stream.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScrollStreamResult {
  /// Delivery evidence of the first non-empty sample; `None` if nothing moved.
  pub action: Option<InputActionResult>,
  pub delivered: Scroll,
  pub reason: ScrollStreamStopReason,
  pub elapsed: Duration,
}

#[derive(Debug)]
struct ControlState {
  target: ScrollVelocity,
  renewed: Instant,
  stop: bool,
  cancel: bool,
}

/// Caller-side handle that steers a running [`run_window_scroll_stream`].
/// Every accepted velocity update renews the lease.
#[derive(Clone, Debug)]
pub struct ScrollStreamControl {
  state: Arc<Mutex<ControlState>>,
}

impl Default for ScrollStreamControl {
  fn default() -> Self {
    Self::new()
  }
}

impl ScrollStreamControl {
  /// Starts with zero velocity; the lease starts now.
  pub fn new() -> Self {
    Self {
      state: Arc::new(Mutex::new(ControlState {
        target: ScrollVelocity::ZERO,
        renewed: Instant::now(),
        stop: false,
        cancel: false,
      })),
    }
  }

  pub fn set_velocity(&self, velocity: ScrollVelocity) -> DriverResult<()> {
    velocity.validate()?;
    let mut state = self.state.lock().unwrap();
    state.target = velocity;
    state.renewed = Instant::now();
    Ok(())
  }

  /// Ramps to zero (subject to `max_acceleration`), then completes.
  pub fn stop(&self) {
    self.state.lock().unwrap().stop = true;
  }

  /// Ends delivery at the next sample without ramping.
  pub fn cancel(&self) {
    self.state.lock().unwrap().cancel = true;
  }
}

/// Runs a live scroll stream through repeated [`WindowInput::scroll`] calls
/// until the caller stops or cancels, or the lease expires.
///
/// As with timed motion, the first non-empty sample selects the delivery path
/// and later samples reuse it. Input cancellation (for example a transport
/// disconnect) ends the stream with an error.
// TODO(scroll-motion-admission): each sample takes its own desktop input
// admission, as for timed scroll motion.
pub fn run_window_scroll_stream<W: WindowInput + ?Sized>(
  input: &W,
  window: &Window,
  point: WindowPoint,
  control: &ScrollStreamControl,
  stream: ScrollStreamOptions,
  options: ScrollOptions,
  notify: &mut dyn FnMut(ScrollStreamProgress),
) -> DriverResult<ScrollStreamResult> {
  stream.validate()?;
  let mut quantizer = CumulativeQuantizer::new(Scroll::new(0.0, 0.0), input.scroll_quantum())?;
  let period = Duration::from_secs_f64(1.0 / f64::from(stream.sample_rate_hz));
  let started = Instant::now();
  let mut last = started;
  let mut next_tick = started;
  let mut velocity = ScrollVelocity::ZERO;
  let mut position = (0.0, 0.0);
  let mut first_action: Option<InputActionResult> = None;
  let mut sample_options = ScrollOptions {
    settle: Duration::ZERO,
    ..options.clone()
  };
  let reason = loop {
    next_tick += period;
    wait_until(next_tick)?;
    let now = Instant::now();
    let (target, ending) = {
      let state = control.state.lock().unwrap();
      if state.cancel {
        break ScrollStreamStopReason::Cancelled;
      }
      if state.stop {
        (ScrollVelocity::ZERO, Some(ScrollStreamStopReason::Stopped))
      } else if now.duration_since(state.renewed) > stream.lease {
        (ScrollVelocity::ZERO, Some(ScrollStreamStopReason::LeaseExpired))
      } else {
        (state.target, None)
      }
    };
    let dt = now.duration_since(last).as_secs_f64();
    last = now;
    velocity = ramp(velocity, target, stream.max_acceleration.map(|limit| limit * dt));
    position = (position.0 + velocity.delta_x_per_second * dt, position.1 + velocity.delta_y_per_second * dt);
    let delta = quantizer.step_to(Scroll::new(position.0, position.1));
    if delta.delta_x != 0.0 || delta.delta_y != 0.0 {
      let action = input
        .scroll(window, point, delta, sample_options.clone())
        .map_err(|error| partial_failure(error, quantizer.delivered(), delta, first_action.is_some()))?;
      if first_action.is_none() {
        sample_options = pinned_options(&action, &options);
        first_action = Some(action);
      }
    }
    notify(ScrollStreamProgress {
      elapsed: now.duration_since(started),
      delivered: quantizer.delivered(),
      velocity,
    });
    if let Some(reason) = ending
      && velocity.is_zero()
    {
      break reason;
    }
    // Catch up without bursts if a slow sample overran several periods.
    if next_tick < now {
      next_tick = now;
    }
  };
  if !options.settle.is_zero() && current_input_cancellation().is_none_or(|cancellation| !cancellation.is_cancelled()) {
    wait_until(Instant::now() + options.settle)?;
  }
  Ok(ScrollStreamResult {
    action: first_action,
    delivered: quantizer.delivered(),
    reason,
    elapsed: started.elapsed(),
  })
}

/// Moves `current` toward `target`, limiting the change vector's length to
/// `max_change` when given.
fn ramp(current: ScrollVelocity, target: ScrollVelocity, max_change: Option<f64>) -> ScrollVelocity {
  let change = (target.delta_x_per_second - current.delta_x_per_second, target.delta_y_per_second - current.delta_y_per_second);
  let length = change.0.hypot(change.1);
  match max_change {
    Some(limit) if length > limit && length > 0.0 => {
      let scale = limit / length;
      ScrollVelocity::new(current.delta_x_per_second + change.0 * scale, current.delta_y_per_second + change.1 * scale)
    }
    _ => target,
  }
}

fn invalid(message: impl Into<String>) -> DriverError {
  DriverError::InvalidInput {
    message: message.into(),
  }
}

#[cfg(test)]
#[path = "scroll_stream_test.rs"]
mod tests;
