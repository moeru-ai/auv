//! Scroll-until: scroll a window and inspect each resulting viewport until the
//! viewport stops moving, target text appears, the caller's observer stops it,
//! or a step budget runs out.
//!
//! The loop is platform-independent. Input, capture, and text recognition are
//! external boundaries supplied through [`ScrollUntilSurface`], so the Runner
//! and local invoke share one implementation. See
//! `docs/ai/references/driver/2026-10-06-scroll-motion-design.md`.

use std::time::Duration;

use auv_driver::{Capture, CaptureResolution, DriverError, DriverResult, InputActionResult, RatioRect, Rect, Scroll, TextRecognition};
use serde::{Deserialize, Serialize};

use crate::viewport_pixels::{ScrollAxis, ViewportPixelMotion, ViewportPixelPolicy, compare_viewport_pixels, crop_ratio};

const MAX_SCROLL_UNTIL_STEPS: u32 = 1_000;
// NOTICE(scroll-until-settle-limit): a settle beyond 10 s per step is longer
// than any lazy-load wait this operation targets; longer waits belong to an
// explicit update loop.
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
  /// Consecutive no-motion updates that count as the end, 1..=10.
  pub no_motion_confirmations: u32,
  /// Normalized region of the window compared for motion; `None` is the whole window.
  pub motion_region: Option<RatioRect>,
  /// What each update carries. Everything is included unless opted out.
  #[serde(default)]
  pub output: ScrollUntilOutputOptions,
}

/// Opt-outs for the data attached to each [`ScrollUntilUpdate`].
///
/// Motion evidence and the capture are always included: the loop captures
/// every step for motion detection anyway, and Runners return captures by
/// reference. Text recognition still runs for a
/// [`ScrollUntilCondition::TextVisible`] condition even when `text` is off;
/// only the update omits it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScrollUntilOutputOptions {
  pub text: bool,
}

impl Default for ScrollUntilOutputOptions {
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

/// A recognized text match. `bounds` are logical screen coordinates, as the
/// drivers report them for the capture.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScrollUntilTextMatch {
  pub text: String,
  pub bounds: Rect,
}

/// What the loop saw at one point: before the first step (`steps == 0`) and
/// after each step's settle. `C` is how the capture is held: in-process
/// pixels (`Capture`), or a Runner-held reference in the `auv-core` client.
#[derive(Clone, Debug, PartialEq)]
pub struct ScrollUntilUpdate<C = Capture> {
  /// Steps delivered so far.
  pub steps: u32,
  /// Logical pixels delivered so far.
  pub delivered: Scroll,
  /// Motion since the previous update; `None` before the first step.
  pub motion: Option<ViewportPixelMotion>,
  pub no_motion_streak: u32,
  /// The window capture this update was made from.
  pub capture: C,
  /// Text recognized in the capture, unless opted out. Region bounds are in
  /// the capture's screen space; `origin` maps them into its owning space.
  pub text: Option<TextRecognition>,
  /// Set when a built-in condition or the budget ends the loop at this
  /// update. The observer's decision is then ignored.
  pub stop: Option<ScrollUntilStopReason>,
}

/// The observer's verdict on one update.
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
  /// Captures the current window at `resolution`. One capture serves both
  /// motion and text.
  fn capture(&mut self, resolution: CaptureResolution) -> DriverResult<Capture>;
  /// Recognizes text in the whole capture.
  fn recognize_text(&mut self, capture: &Capture) -> DriverResult<TextRecognition>;
  /// Waits between a step and its update; returns an error if cancelled.
  fn wait(&mut self, duration: Duration) -> DriverResult<()>;
}

// TODO(scroll-until-artifacts): the final capture and per-step motion are
// reported in the result but not persisted as run artifacts; add artifact
// emission when an inspector consumer needs scroll-until evidence.
// TODO(scroll-until-ax-boundary): accessibility scrollbar values (as NetEase
// uses) could confirm the end with one update; add when a platform-neutral
// scrollbar read exists.
/// Runs the loop. `observer` sees every update, including the initial
/// one and the last one; it may stop the loop unless a built-in condition or
/// the budget already did (`update.stop`).
pub fn scroll_until(
  surface: &mut impl ScrollUntilSurface,
  request: &ScrollUntilRequest,
  observer: &mut dyn FnMut(ScrollUntilUpdate) -> DriverResult<ScrollUntilDecision>,
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
  // Text needs native pixels; motion alone needs only one pixel per point.
  let resolution = if query.is_some() || request.output.text {
    CaptureResolution::Native
  } else {
    CaptureResolution::Logical
  };
  let mut capture = surface.capture(resolution)?;
  let mut previous = motion_frame(&capture, request.motion_region);
  let mut no_motion_streak = 0;
  loop {
    let text = if query.is_some() || request.output.text {
      Some(surface.recognize_text(&capture)?)
    } else {
      None
    };
    result.text_match = query.zip(text.as_ref()).and_then(|(query, text)| text_match(text, query));
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
    let decision = observer(ScrollUntilUpdate {
      steps: result.steps,
      delivered: result.delivered,
      motion: result.last_motion,
      no_motion_streak,
      capture,
      text: text.filter(|_| request.output.text),
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

    capture = surface.capture(resolution)?;
    let current = motion_frame(&capture, request.motion_region);
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

/// `capture` cropped to `region` at one pixel per point, for motion checks.
///
/// NOTICE(scroll-until-logical-motion): `ViewportPixelPolicy` defaults (±24 px
/// search, stride 4) were validated on 1x captures, so motion is compared per
/// logical point whatever resolution the capture was taken at.
fn motion_frame(capture: &Capture, region: Option<RatioRect>) -> image::RgbaImage {
  let crop = crop_ratio(&capture.image, region);
  if !capture.scale_factor.is_finite() || capture.scale_factor <= 1.0 {
    return crop;
  }
  let width = ((f64::from(crop.width()) / capture.scale_factor).round() as u32).max(1);
  let height = ((f64::from(crop.height()) / capture.scale_factor).round() as u32).max(1);
  image::imageops::thumbnail(&crop, width, height)
}

/// The first recognized line containing `query` (case-insensitive). Driver
/// OCR bounds are already in the capture's screen space
/// (`capture.bounds.origin` plus pixels / scale), so they are used as they are.
fn text_match(text: &TextRecognition, query: &str) -> Option<ScrollUntilTextMatch> {
  text.best_contains(query).map(|region| ScrollUntilTextMatch {
    text: region.text.clone(),
    bounds: region.bounds,
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

  fn capture(&mut self, resolution: CaptureResolution) -> DriverResult<Capture> {
    let options = auv_driver::CaptureOptions {
      resolution,
      ..Default::default()
    };
    self.session.window().capture_with(&self.window, options)
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
