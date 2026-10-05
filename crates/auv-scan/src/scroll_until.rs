//! Scroll-until: step a window scroll and observe after each step until the
//! viewport stops moving, target text appears, or a step budget runs out.
//!
//! The loop is platform-independent. Input, capture, and text recognition are
//! external boundaries supplied through [`ScrollUntilSurface`], so the Runner
//! and local invoke share one implementation. See
//! `docs/ai/references/driver/2026-10-06-scroll-motion-design.md`.

use std::time::Duration;

use auv_driver::{Capture, DriverError, DriverResult, InputActionResult, RatioRect, Rect, Scroll};
use serde::{Deserialize, Serialize};

use crate::viewport_pixels::{ScrollAxis, ViewportPixelMotion, ViewportPixelPolicy, compare_viewport_pixels, crop_ratio};

const MAX_SCROLL_UNTIL_STEPS: u32 = 1_000;
// NOTICE(scroll-until-settle-limit): a settle beyond 10 s per step is longer
// than any lazy-load wait this operation targets; longer waits belong to an
// explicit observation loop.
const MAX_SCROLL_UNTIL_SETTLE: Duration = Duration::from_secs(10);

/// What ends a scroll-until besides the step budget.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ScrollUntilCondition {
  /// Stop when consecutive steps produce no visual motion.
  End,
  /// Stop when recognized window text contains `query` (case-insensitive).
  /// Reaching the end without a match also stops.
  TextVisible { query: String },
}

/// How each step scrolls.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ScrollUntilStep {
  /// One instant wheel scroll of `delta` logical pixels.
  Instant { delta: Scroll },
  /// One timed scroll of `motion.total` logical pixels per step.
  Motion { motion: auv_driver::ScrollMotion },
}

impl ScrollUntilStep {
  pub fn delta(&self) -> Scroll {
    match self {
      Self::Instant { delta } => *delta,
      Self::Motion { motion } => motion.total,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScrollUntilRequest {
  pub step: ScrollUntilStep,
  pub condition: ScrollUntilCondition,
  /// Step budget, 1..=1000.
  pub max_steps: u32,
  /// Wait after each step before observing, so lazy content can arrive.
  pub settle: Duration,
  /// Consecutive no-motion observations that count as the end, 1..=10.
  pub no_motion_confirmations: u32,
  /// Normalized region of the window compared for motion; `None` is the whole window.
  pub motion_region: Option<RatioRect>,
}

impl ScrollUntilRequest {
  pub fn validate(&self) -> DriverResult<()> {
    let delta = self.step.delta();
    if !delta.delta_x.is_finite() || !delta.delta_y.is_finite() || (delta.delta_x == 0.0 && delta.delta_y == 0.0) {
      return Err(invalid("scroll-until step must be finite and non-zero"));
    }
    if delta.delta_x != 0.0 && delta.delta_y != 0.0 {
      return Err(invalid("scroll-until step must move along one axis"));
    }
    if let ScrollUntilStep::Motion { motion } = self.step {
      motion.schedule()?;
    }
    if let ScrollUntilCondition::TextVisible { query } = &self.condition
      && query.trim().is_empty()
    {
      return Err(invalid("scroll-until text query must not be empty"));
    }
    if !(1..=MAX_SCROLL_UNTIL_STEPS).contains(&self.max_steps) {
      return Err(invalid(format!("scroll-until max_steps must be within 1..={MAX_SCROLL_UNTIL_STEPS}")));
    }
    if self.settle > MAX_SCROLL_UNTIL_SETTLE {
      return Err(invalid("scroll-until settle must be at most 10s"));
    }
    if !(1..=10).contains(&self.no_motion_confirmations) {
      return Err(invalid("scroll-until no_motion_confirmations must be within 1..=10"));
    }
    if let Some(region) = self.motion_region {
      let values = [region.x, region.y, region.width, region.height];
      if values.iter().any(|value| !value.is_finite())
        || region.x < 0.0
        || region.y < 0.0
        || region.width <= 0.0
        || region.height <= 0.0
        || region.x + region.width > 1.0
        || region.y + region.height > 1.0
      {
        return Err(invalid("scroll-until motion_region must be a non-empty normalized rectangle"));
      }
    }
    Ok(())
  }

  fn axis(&self) -> ScrollAxis {
    if self.step.delta().delta_y != 0.0 {
      ScrollAxis::Vertical
    } else {
      ScrollAxis::Horizontal
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollUntilStopReason {
  /// Consecutive steps showed no visual progress. This is not proof that no
  /// more content exists; see the completeness-claim rule in TERMS.
  EndByNoVisualProgress,
  /// The text query became visible.
  TextVisible,
  /// The step budget ran out first.
  BudgetExhausted,
}

/// A recognized text match; bounds are screen coordinates, as reported by
/// window text recognition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScrollUntilTextMatch {
  pub text: String,
  pub bounds: Rect,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScrollUntilProgress {
  /// Steps delivered so far.
  pub steps: u32,
  /// Logical pixels delivered so far.
  pub delivered: Scroll,
  /// Motion evidence of the latest observation, when one was compared.
  pub motion: Option<ViewportPixelMotion>,
  pub no_motion_streak: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScrollUntilResult {
  pub reason: ScrollUntilStopReason,
  pub steps: u32,
  pub delivered: Scroll,
  /// Delivery evidence of the first step; `None` if text was visible before any step.
  pub action: Option<InputActionResult>,
  pub text_match: Option<ScrollUntilTextMatch>,
  pub last_motion: Option<ViewportPixelMotion>,
}

/// External input, capture, and recognition for one target window.
pub trait ScrollUntilSurface {
  /// Delivers one step and returns the delivery evidence and delivered delta.
  fn scroll(&mut self, step: &ScrollUntilStep) -> DriverResult<(InputActionResult, Scroll)>;
  /// Captures the current window. One capture serves both motion and text.
  fn capture(&mut self) -> DriverResult<Capture>;
  /// Returns the first recognized text in `capture` containing `query`, if any.
  fn find_text(&mut self, capture: &Capture, query: &str) -> DriverResult<Option<ScrollUntilTextMatch>>;
  /// Waits between a step and its observation; returns an error if cancelled.
  fn wait(&mut self, duration: Duration) -> DriverResult<()>;
}

// TODO(scroll-until-artifacts): the final capture and per-step motion are
// reported in the result but not persisted as run artifacts; add artifact
// emission when an inspector consumer needs scroll-until evidence.
// TODO(scroll-until-ax-boundary): accessibility scrollbar values (as NetEase
// uses) could confirm the end with one observation; add when a platform-neutral
// scrollbar read exists.
pub fn scroll_until(
  surface: &mut impl ScrollUntilSurface,
  request: &ScrollUntilRequest,
  notify: &mut dyn FnMut(&ScrollUntilProgress),
) -> DriverResult<ScrollUntilResult> {
  request.validate()?;
  let axis = request.axis();
  let policy = ViewportPixelPolicy::default();
  let query = match &request.condition {
    ScrollUntilCondition::TextVisible { query } => Some(query.as_str()),
    ScrollUntilCondition::End => None,
  };
  let mut result = ScrollUntilResult {
    reason: ScrollUntilStopReason::BudgetExhausted,
    steps: 0,
    delivered: Scroll::new(0.0, 0.0),
    action: None,
    text_match: None,
    last_motion: None,
  };
  let initial = surface.capture()?;
  if let Some(query) = query
    && let Some(found) = surface.find_text(&initial, query)?
  {
    result.reason = ScrollUntilStopReason::TextVisible;
    result.text_match = Some(found);
    return Ok(result);
  }
  let mut previous = crop_ratio(&initial.image, request.motion_region);
  let mut no_motion_streak = 0;
  while result.steps < request.max_steps {
    let (action, delivered) = surface.scroll(&request.step)?;
    result.steps += 1;
    result.delivered = Scroll::new(result.delivered.delta_x + delivered.delta_x, result.delivered.delta_y + delivered.delta_y);
    result.action.get_or_insert(action);
    surface.wait(request.settle)?;

    let capture = surface.capture()?;
    let current = crop_ratio(&capture.image, request.motion_region);
    let motion = compare_viewport_pixels(&previous, &current, axis, policy);
    previous = current;
    no_motion_streak = if motion.no_motion {
      no_motion_streak + 1
    } else {
      0
    };
    result.last_motion = Some(motion);
    notify(&ScrollUntilProgress {
      steps: result.steps,
      delivered: result.delivered,
      motion: Some(motion),
      no_motion_streak,
    });

    if let Some(query) = query
      && let Some(found) = surface.find_text(&capture, query)?
    {
      result.reason = ScrollUntilStopReason::TextVisible;
      result.text_match = Some(found);
      return Ok(result);
    }
    if no_motion_streak >= request.no_motion_confirmations {
      result.reason = ScrollUntilStopReason::EndByNoVisualProgress;
      return Ok(result);
    }
  }
  Ok(result)
}

/// [`ScrollUntilSurface`] over the local desktop driver for one window point.
/// The Runner and local invoke both use it.
pub struct WindowScrollUntilSurface<'a> {
  session: &'a auv_driver::LocalDriverSession,
  window: auv_driver::Window,
  point: auv_driver::WindowPoint,
  options: auv_driver::ScrollOptions,
}

impl<'a> WindowScrollUntilSurface<'a> {
  pub fn new(
    session: &'a auv_driver::LocalDriverSession,
    window: auv_driver::Window,
    point: auv_driver::WindowPoint,
    options: auv_driver::ScrollOptions,
  ) -> Self {
    Self {
      session,
      window,
      point,
      options,
    }
  }
}

impl ScrollUntilSurface for WindowScrollUntilSurface<'_> {
  fn scroll(&mut self, step: &ScrollUntilStep) -> DriverResult<(InputActionResult, Scroll)> {
    use auv_driver::WindowInput as _;
    match step {
      ScrollUntilStep::Instant { delta } => {
        let action = self.session.window().scroll(&self.window, self.point, *delta, self.options.clone())?;
        Ok((action, *delta))
      }
      ScrollUntilStep::Motion { motion } => {
        let result = self.session.window().scroll_motion(&self.window, self.point, motion, self.options.clone(), &mut |_| {})?;
        Ok((result.action, result.delivered))
      }
    }
  }

  fn capture(&mut self) -> DriverResult<Capture> {
    self.session.window().capture(&self.window)
  }

  fn find_text(&mut self, capture: &Capture, query: &str) -> DriverResult<Option<ScrollUntilTextMatch>> {
    let matches = self.session.vision().find_text_in_capture_with_options(
      capture,
      query,
      RatioRect::new(0.0, 0.0, 1.0, 1.0),
      auv_driver::TextRecognitionOptions::default(),
    )?;
    Ok(matches.matches.into_iter().next().map(|matched| ScrollUntilTextMatch {
      text: matched.text,
      bounds: matched.bounds,
    }))
  }

  fn wait(&mut self, duration: Duration) -> DriverResult<()> {
    let deadline = std::time::Instant::now() + duration;
    let cancellation = auv_driver::input_cancellation::current_input_cancellation();
    loop {
      if cancellation.as_ref().is_some_and(|cancellation| cancellation.is_cancelled()) {
        return Err(invalid("scroll-until cancelled"));
      }
      let remaining = deadline.saturating_duration_since(std::time::Instant::now());
      if remaining.is_zero() {
        return Ok(());
      }
      std::thread::sleep(remaining.min(Duration::from_millis(20)));
    }
  }
}

fn invalid(message: impl Into<String>) -> DriverError {
  DriverError::InvalidInput {
    message: message.into(),
  }
}

#[cfg(test)]
#[path = "scroll_until_test.rs"]
mod tests;
