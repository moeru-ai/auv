//! Timed wheel scrolling: one [`Scroll`] total spread over time by a timing
//! function.
//!
//! The schedule is evaluated on demand, like mouse motion samples, so memory
//! does not grow with duration or sample rate. Each platform quantizes the
//! **cumulative** target in its own native wheel unit and sends only the
//! difference from the previous sample, so the delivered total is exact even
//! when one sample is smaller than one native unit. See
//! `docs/ai/references/driver/2026-10-06-scroll-motion-design.md`.

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::input::{
  InputActionResult, InputDeliveryPath, InputPolicy, Scroll, ScrollDeliveryCandidate, ScrollDeliveryStrategy, ScrollOptions,
};
use crate::input_cancellation::current_input_cancellation;
use crate::{DriverError, DriverResult, Window, WindowInput, WindowPoint};

/// Maps normalized elapsed time `t` in `[0, 1]` to normalized progress.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum TimingFunction {
  Linear,
  EaseInCubic,
  EaseOutCubic,
  EaseInOutCubic,
  /// CSS `cubic-bezier(x1, y1, x2, y2)`; `x1` and `x2` must be within `[0, 1]`.
  CubicBezier {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
  },
}

impl TimingFunction {
  pub fn validate(&self) -> DriverResult<()> {
    if let Self::CubicBezier { x1, y1, x2, y2 } = *self {
      if ![x1, y1, x2, y2].iter().all(|value| value.is_finite()) {
        return Err(invalid("cubic-bezier control values must be finite"));
      }
      if !(0.0..=1.0).contains(&x1) || !(0.0..=1.0).contains(&x2) {
        return Err(invalid("cubic-bezier x1 and x2 must be within 0..=1"));
      }
    }
    Ok(())
  }

  /// Progress for normalized time `t`. Values outside `[0, 1]` are clamped.
  pub fn progress(&self, t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    match *self {
      Self::Linear => t,
      Self::EaseInCubic => t * t * t,
      Self::EaseOutCubic => 1.0 - (1.0 - t).powi(3),
      Self::EaseInOutCubic => {
        if t < 0.5 {
          4.0 * t * t * t
        } else {
          1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
        }
      }
      Self::CubicBezier { x1, y1, x2, y2 } => cubic_bezier_progress(x1, y1, x2, y2, t),
    }
  }
}

/// How a timed motion distributes progress over time.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum MotionTiming {
  FixedDuration {
    duration: Duration,
    function: TimingFunction,
  },
  // TODO(scroll-acceleration-limited): an AccelerationLimited timing (speed and
  // acceleration limits instead of a fixed duration), as planned for mouse
  // motion, is deferred until a caller needs it.
}

/// One wheel scroll of `total` logical pixels spread over time.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScrollMotion {
  /// Logical pixels; positive is toward later content (down/right).
  pub total: Scroll,
  pub timing: MotionTiming,
  /// Samples per second; must be positive when the duration is positive.
  pub sample_rate_hz: u32,
}

/// Latest delivery progress of a running scroll motion.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScrollMotionProgress {
  /// Can skip values when overdue samples are coalesced.
  pub sample_index: u64,
  pub planned_sample_count: u64,
  pub scheduled_elapsed: Duration,
  /// Logical pixels delivered so far.
  pub delivered: Scroll,
}

/// Completed scroll motion.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScrollMotionResult {
  /// Delivery evidence of the first non-empty sample; later samples reuse its path.
  pub action: InputActionResult,
  /// Logical pixels delivered. Equals the quantized total on success.
  pub delivered: Scroll,
  pub planned_sample_count: u64,
}

/// Validated, on-demand sample schedule for one [`ScrollMotion`].
#[derive(Clone, Copy, Debug)]
pub struct ScrollMotionSchedule {
  intervals: u64,
  duration: Duration,
  sample_rate_hz: u32,
  function: TimingFunction,
}

impl ScrollMotion {
  pub fn schedule(&self) -> DriverResult<ScrollMotionSchedule> {
    if !self.total.delta_x.is_finite() || !self.total.delta_y.is_finite() {
      return Err(invalid("scroll motion total must be finite"));
    }
    if self.total.delta_x == 0.0 && self.total.delta_y == 0.0 {
      return Err(invalid("scroll motion requires a non-zero total"));
    }
    let MotionTiming::FixedDuration { duration, function } = self.timing;
    function.validate()?;
    if duration.is_zero() {
      return Ok(ScrollMotionSchedule {
        intervals: 0,
        duration,
        sample_rate_hz: self.sample_rate_hz,
        function,
      });
    }
    if self.sample_rate_hz == 0 {
      return Err(invalid("timed scroll motion requires a positive sample_rate_hz"));
    }
    let intervals = (duration.as_nanos() * u128::from(self.sample_rate_hz)).div_ceil(1_000_000_000).max(1);
    let intervals = u64::try_from(intervals)
      .ok()
      .filter(|value| *value < u64::MAX)
      .ok_or_else(|| invalid("scroll motion sample count exceeds the protocol integer range"))?;
    Ok(ScrollMotionSchedule {
      intervals,
      duration,
      sample_rate_hz: self.sample_rate_hz,
      function,
    })
  }
}

impl ScrollMotionSchedule {
  /// Number of samples, including the final one. Always at least one.
  pub fn len(&self) -> u64 {
    self.intervals + 1
  }

  pub fn is_empty(&self) -> bool {
    false
  }

  pub fn duration(&self) -> Duration {
    self.duration
  }

  /// Scheduled elapsed time and normalized progress of sample `index`.
  pub fn at(&self, index: u64) -> (Duration, f64) {
    assert!(index < self.len());
    if index == self.intervals {
      return (self.duration, 1.0);
    }
    let rate = u64::from(self.sample_rate_hz);
    let elapsed =
      Duration::from_secs(index / rate) + Duration::from_nanos(((u128::from(index % rate) * 1_000_000_000) / u128::from(rate)) as u64);
    (elapsed, self.function.progress(elapsed.as_secs_f64() / self.duration.as_secs_f64()))
  }

  /// Skip overdue samples arithmetically, without walking them.
  pub fn latest_due(&self, next: u64, elapsed: Duration) -> u64 {
    if elapsed >= self.duration {
      return self.intervals;
    }
    let due = elapsed.as_nanos() * u128::from(self.sample_rate_hz) / 1_000_000_000;
    (due.min(u128::from(self.intervals)) as u64).max(next)
  }
}

/// Converts cumulative progress into per-sample deltas that are whole
/// multiples of the platform's native wheel unit, so their sum is exact.
#[derive(Clone, Copy, Debug)]
pub struct CumulativeQuantizer {
  total: Scroll,
  /// Logical pixels per native wheel unit.
  quantum: f64,
  emitted_units: (i64, i64),
}

impl CumulativeQuantizer {
  pub fn new(total: Scroll, quantum: f64) -> DriverResult<Self> {
    if !quantum.is_finite() || quantum <= 0.0 {
      return Err(invalid("scroll quantum must be finite and positive"));
    }
    Ok(Self {
      total,
      quantum,
      emitted_units: (0, 0),
    })
  }

  /// Delta in logical pixels needed to reach `progress` of the total.
  pub fn step(&mut self, progress: f64) -> Scroll {
    let target =
      ((self.total.delta_x * progress / self.quantum).round() as i64, (self.total.delta_y * progress / self.quantum).round() as i64);
    let delta = (target.0 - self.emitted_units.0, target.1 - self.emitted_units.1);
    self.emitted_units = target;
    Scroll::new(delta.0 as f64 * self.quantum, delta.1 as f64 * self.quantum)
  }

  /// Logical pixels emitted so far.
  pub fn delivered(&self) -> Scroll {
    Scroll::new(self.emitted_units.0 as f64 * self.quantum, self.emitted_units.1 as f64 * self.quantum)
  }
}

/// Runs a scroll motion through repeated [`WindowInput::scroll`] calls.
///
/// The first non-empty sample selects the delivery path through the caller's
/// policy and strategy. Later samples are pinned to that path: a mid-motion
/// fallback would move the pointer or change focus partway through a gesture.
/// A failed later sample ends the motion with an error that reports progress.
// TODO(scroll-motion-admission): each sample takes its own desktop input
// admission, so another input can interleave between samples. Hold one
// admission for the whole motion when adapters expose a re-entrant scroll.
pub fn run_window_scroll_motion<W: WindowInput + ?Sized>(
  input: &W,
  window: &Window,
  point: WindowPoint,
  motion: &ScrollMotion,
  options: ScrollOptions,
  notify: &mut dyn FnMut(ScrollMotionProgress),
) -> DriverResult<ScrollMotionResult> {
  let schedule = motion.schedule()?;
  let mut quantizer = CumulativeQuantizer::new(motion.total, input.scroll_quantum())?;
  let planned_sample_count = schedule.len();
  let started = Instant::now();
  let mut first_action: Option<InputActionResult> = None;
  let mut sample_options = ScrollOptions {
    settle: Duration::ZERO,
    ..options.clone()
  };
  let mut next = 0;
  loop {
    let (elapsed, _) = schedule.at(next);
    wait_until(started + elapsed)?;
    let index = schedule.latest_due(next, started.elapsed());
    let (scheduled_elapsed, progress) = schedule.at(index);
    let delta = quantizer.step(progress);
    if delta.delta_x != 0.0 || delta.delta_y != 0.0 {
      let action = input
        .scroll(window, point, delta, sample_options.clone())
        .map_err(|error| partial_failure(error, quantizer.delivered(), delta, first_action.is_some()))?;
      if first_action.is_none() {
        sample_options = pinned_options(&action, &options);
        first_action = Some(action);
      }
    }
    notify(ScrollMotionProgress {
      sample_index: index,
      planned_sample_count,
      scheduled_elapsed,
      delivered: quantizer.delivered(),
    });
    if index + 1 >= planned_sample_count {
      break;
    }
    next = index + 1;
  }
  if !options.settle.is_zero() {
    wait_until(Instant::now() + options.settle)?;
  }
  let action = first_action.ok_or_else(|| invalid("scroll motion total is smaller than one native wheel unit"))?;
  Ok(ScrollMotionResult {
    action,
    delivered: quantizer.delivered(),
    planned_sample_count,
  })
}

/// Reuses the first sample's selected path for the rest of the motion.
fn pinned_options(action: &InputActionResult, options: &ScrollOptions) -> ScrollOptions {
  let pinned = match action.selected_path {
    InputDeliveryPath::WindowTargetedWheel => Some((ScrollDeliveryCandidate::WindowTargetedWheel, InputPolicy::BackgroundOnly)),
    InputDeliveryPath::AxScroll => Some((ScrollDeliveryCandidate::AxScroll, InputPolicy::BackgroundOnly)),
    InputDeliveryPath::WindowTargetedKeyboardScroll => {
      Some((ScrollDeliveryCandidate::WindowTargetedKeyboardScroll, InputPolicy::BackgroundOnly))
    }
    InputDeliveryPath::ForegroundSystemEvents => Some((ScrollDeliveryCandidate::ForegroundHid, InputPolicy::ForegroundPreferred)),
    _ => None,
  };
  match pinned {
    Some((candidate, policy)) => ScrollOptions {
      policy,
      delivery_strategy: ScrollDeliveryStrategy {
        candidates: vec![candidate],
      },
      settle: Duration::ZERO,
    },
    None => ScrollOptions {
      settle: Duration::ZERO,
      ..options.clone()
    },
  }
}

fn partial_failure(error: DriverError, delivered: Scroll, attempted: Scroll, started: bool) -> DriverError {
  if !started {
    return error;
  }
  DriverError::Backend {
    message: format!(
      "scroll motion stopped after delivering dx={} dy={} logical px; sample dx={} dy={} failed: {error}",
      delivered.delta_x - attempted.delta_x,
      delivered.delta_y - attempted.delta_y,
      attempted.delta_x,
      attempted.delta_y
    ),
  }
}

/// Sleeps until `deadline`, waking early with an error on input cancellation.
fn wait_until(deadline: Instant) -> DriverResult<()> {
  let cancellation = current_input_cancellation();
  let signal = Arc::new((Mutex::new(()), Condvar::new()));
  if let Some(cancellation) = &cancellation {
    let wake = signal.clone();
    cancellation.register_wakeup(move || wake.1.notify_all());
  }
  let mut guard = signal.0.lock().unwrap();
  loop {
    if cancellation.as_ref().is_some_and(|cancellation| cancellation.is_cancelled()) {
      return Err(invalid("scroll motion cancelled"));
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
      return Ok(());
    }
    guard = signal.1.wait_timeout(guard, remaining).unwrap().0;
  }
}

// CSS cubic-bezier: solve x(s) = t for the curve parameter, then return y(s).
// Newton iterations converge quickly for typical curves; bisection guarantees
// a result when the derivative is flat.
fn cubic_bezier_progress(x1: f64, y1: f64, x2: f64, y2: f64, t: f64) -> f64 {
  let curve = |a: f64, b: f64, s: f64| {
    let inverse = 1.0 - s;
    3.0 * inverse * inverse * s * a + 3.0 * inverse * s * s * b + s * s * s
  };
  let derivative = |a: f64, b: f64, s: f64| {
    let inverse = 1.0 - s;
    3.0 * inverse * inverse * a + 6.0 * inverse * s * (b - a) + 3.0 * s * s * (1.0 - b)
  };
  let mut s = t;
  for _ in 0..8 {
    let error = curve(x1, x2, s) - t;
    if error.abs() < 1e-7 {
      return curve(y1, y2, s);
    }
    let slope = derivative(x1, x2, s);
    if slope.abs() < 1e-6 {
      break;
    }
    s = (s - error / slope).clamp(0.0, 1.0);
  }
  let (mut low, mut high) = (0.0, 1.0);
  s = t;
  for _ in 0..50 {
    let x = curve(x1, x2, s);
    if (x - t).abs() < 1e-7 {
      break;
    }
    if x < t {
      low = s;
    } else {
      high = s;
    }
    s = (low + high) / 2.0;
  }
  curve(y1, y2, s)
}

fn invalid(message: impl Into<String>) -> DriverError {
  DriverError::InvalidInput {
    message: message.into(),
  }
}

#[cfg(test)]
#[path = "scroll_motion_test.rs"]
mod tests;
