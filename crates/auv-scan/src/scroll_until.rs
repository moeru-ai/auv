//! Scroll-until: step a window scroll and observe after each step until the
//! viewport stops moving, target text appears, the caller's observer stops it,
//! or a step budget runs out.
//!
//! The loop is platform-independent. Input, capture, and text recognition are
//! external boundaries supplied through [`ScrollUntilSurface`], so the Runner
//! and local invoke share one implementation. See
//! `docs/ai/references/driver/2026-10-06-scroll-motion-design.md`.

use std::time::Duration;

use auv_driver::{Capture, DriverError, DriverResult, InputActionResult, RatioRect, Rect, Scroll, TextRecognition};
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
  /// What each observation carries. Everything is included unless opted out.
  #[serde(default)]
  pub observe: ScrollUntilObserve,
}

/// Opt-outs for the data attached to each [`ScrollUntilObservation`].
///
/// Motion evidence and the capture are always included: the loop captures
/// every step for motion detection anyway, and Runners return captures by
/// reference. Text recognition still runs for a
/// [`ScrollUntilCondition::TextVisible`] condition even when `text` is off;
/// only the observation omits it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScrollUntilObserve {
  pub text: bool,
}

impl Default for ScrollUntilObserve {
  fn default() -> Self {
    Self { text: true }
  }
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
    if self.motion_region.is_some_and(|region| !region.is_normalized()) {
      return Err(invalid("scroll-until motion_region must be a non-empty normalized rectangle"));
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
  /// The caller's observer returned [`ScrollUntilDecision::Stop`].
  PredicateSatisfied,
}

/// A recognized text match. `bounds` are logical screen coordinates: the
/// recognized line's offset from the capture origin, placed at the capture's
/// screen bounds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScrollUntilTextMatch {
  pub text: String,
  pub bounds: Rect,
}

/// What the loop saw at one point: before the first step (`steps == 0`) and
/// after each step's settle. `C` is how the capture is held: in-process
/// pixels (`Capture`), or a Runner-held reference in the `auv-core` client.
#[derive(Clone, Debug, PartialEq)]
pub struct ScrollUntilObservation<C = Capture> {
  /// Steps delivered so far.
  pub steps: u32,
  /// Logical pixels delivered so far.
  pub delivered: Scroll,
  /// Motion since the previous observation; `None` before the first step.
  pub motion: Option<ViewportPixelMotion>,
  pub no_motion_streak: u32,
  /// The window capture this observation was made from.
  pub capture: C,
  /// Text recognized in the capture, unless opted out. Region bounds are
  /// offsets from the recognition origin, as for window text recognition.
  pub text: Option<TextRecognition>,
  /// Set when a built-in condition or the budget ends the loop at this
  /// observation. The observer's decision is then ignored.
  pub stop: Option<ScrollUntilStopReason>,
}

/// The observer's verdict on one observation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollUntilDecision {
  Continue,
  Stop,
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
  /// Recognizes text in the whole capture.
  fn recognize_text(&mut self, capture: &Capture) -> DriverResult<TextRecognition>;
  /// Waits between a step and its observation; returns an error if cancelled.
  fn wait(&mut self, duration: Duration) -> DriverResult<()>;
}

// TODO(scroll-until-artifacts): the final capture and per-step motion are
// reported in the result but not persisted as run artifacts; add artifact
// emission when an inspector consumer needs scroll-until evidence.
// TODO(scroll-until-ax-boundary): accessibility scrollbar values (as NetEase
// uses) could confirm the end with one observation; add when a platform-neutral
// scrollbar read exists.
/// Runs the loop. `observer` sees every observation, including the initial
/// one and the last one; it may stop the loop unless a built-in condition or
/// the budget already did (`observation.stop`).
pub fn scroll_until(
  surface: &mut impl ScrollUntilSurface,
  request: &ScrollUntilRequest,
  observer: &mut dyn FnMut(ScrollUntilObservation) -> DriverResult<ScrollUntilDecision>,
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
  let mut capture = surface.capture()?;
  let mut previous = crop_ratio(&capture.image, request.motion_region);
  let mut no_motion_streak = 0;
  loop {
    let text = if query.is_some() || request.observe.text {
      Some(surface.recognize_text(&capture)?)
    } else {
      None
    };
    result.text_match = query.zip(text.as_ref()).and_then(|(query, text)| text_match(text, query, &capture));
    // Every condition, not only `End`, stops once the viewport stays still:
    // further steps cannot reveal anything new.
    let stop = if result.text_match.is_some() {
      Some(ScrollUntilStopReason::TextVisible)
    } else if result.steps > 0 && no_motion_streak >= request.no_motion_confirmations {
      Some(ScrollUntilStopReason::EndByNoVisualProgress)
    } else if result.steps >= request.max_steps {
      Some(ScrollUntilStopReason::BudgetExhausted)
    } else {
      None
    };
    let decision = observer(ScrollUntilObservation {
      steps: result.steps,
      delivered: result.delivered,
      motion: result.last_motion,
      no_motion_streak,
      capture,
      text: text.filter(|_| request.observe.text),
      stop,
    })?;
    if let Some(reason) = stop {
      result.reason = reason;
      return Ok(result);
    }
    if decision == ScrollUntilDecision::Stop {
      result.reason = ScrollUntilStopReason::PredicateSatisfied;
      return Ok(result);
    }

    let (action, delivered) = surface.scroll(&request.step)?;
    result.steps += 1;
    result.delivered = Scroll::new(result.delivered.delta_x + delivered.delta_x, result.delivered.delta_y + delivered.delta_y);
    result.action.get_or_insert(action);
    surface.wait(request.settle)?;

    capture = surface.capture()?;
    let current = crop_ratio(&capture.image, request.motion_region);
    let motion = compare_viewport_pixels(&previous, &current, axis, policy);
    previous = current;
    no_motion_streak = if motion.no_motion {
      no_motion_streak + 1
    } else {
      0
    };
    result.last_motion = Some(motion);
  }
}

/// The first recognized line containing `query` (case-insensitive), in screen
/// coordinates. Window text recognition reports offsets from the capture
/// origin, which is the top-left of the capture's screen bounds.
fn text_match(text: &TextRecognition, query: &str, capture: &Capture) -> Option<ScrollUntilTextMatch> {
  text.best_contains(query).map(|region| ScrollUntilTextMatch {
    text: region.text.clone(),
    bounds: Rect::new(
      capture.bounds.origin.x + region.bounds.origin.x,
      capture.bounds.origin.y + region.bounds.origin.y,
      region.bounds.size.width,
      region.bounds.size.height,
    ),
  })
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

  fn recognize_text(&mut self, capture: &Capture) -> DriverResult<TextRecognition> {
    self.session.vision().recognize_text_in_capture(capture, RatioRect::new(0.0, 0.0, 1.0, 1.0))
  }

  fn wait(&mut self, duration: Duration) -> DriverResult<()> {
    auv_driver::input_cancellation::wait_until(std::time::Instant::now() + duration, "scroll-until")
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
