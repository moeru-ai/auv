//! Pure model of live overlay motion: how a cursor glides between the points an
//! operation really acted on, how clicks ripple and how targeted windows are marked.
//!
//! The model owns no clock, thread or renderer. Callers pass a monotonic `now`
//! (time since an epoch they choose) and get an [`Overlay`] back, so the same
//! animation runs against a fake clock in tests and against a platform frame loop in
//! production. Platform adapters only draw the returned layers.
//!
//! # Faithfulness
//!
//! Every visual comes from an [`ActionEvent`] that a driver reported after the
//! action happened. The scene interpolates between reported points and fades what it
//! drew; it never moves toward a point nobody reported and never draws a click that
//! was not delivered. Easing changes how the cursor gets to a real point, not where
//! the operation acted.

use std::time::Duration;

use auv_driver_common::{MouseButton, Point, Rect, ScreenPoint};

use crate::layers::{BuiltInCursor, Cursor, CursorImage, Outline, Status};
use crate::style::{Color, CursorStyle, Insets, OutlineStyle, StatusStyle, Stroke};
use crate::{Easing, MotionOptions, Overlay};

// TODO(overlay-motion-event-wire): `ActionEvent` is an in-process type. It is not
// serializable and not a tracing event, so a remote Runner cannot stream it to a
// viewer yet. Unlock when a concrete out-of-process consumer needs it; the shape is
// provisional until then (see docs/ai/references/driver/2026-10-10-windows-overlay-motion.md).

/// How a reported cursor position was reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Travel {
  /// The operation acted at a new place in one step, such as a click target or a
  /// pointer warp. The cursor eases there using the scene's [`MotionOptions`].
  Jump,
  /// One point of a trajectory the driver already timed. The cursor follows it
  /// without added delay, because easing every sample of a fast stream would make the
  /// cursor crawl behind the real path.
  Sampled,
}

/// One fact about a real action, reported by the driver that performed it.
///
/// Provisional name and shape: this is the overlay's projection of an operation, not a
/// tracing event and not a replacement for `InputActionResult`.
#[derive(Clone, Debug, PartialEq)]
pub enum ActionEvent {
  /// The operation moved the pointer, or aimed its cursor, at `point`.
  Moved { point: ScreenPoint, travel: Travel },
  /// A click was delivered at `point`. It also places the cursor there.
  Clicked {
    point: ScreenPoint,
    button: MouseButton,
  },
  /// The operation targeted the window `id`, currently occupying `frame`. Reporting the
  /// same `id` again refreshes the mark and follows the window if it moved.
  WindowTargeted {
    id: String,
    frame: Rect,
    label: Option<String>,
  },
}

/// When a scene next needs to be drawn without any new event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wake {
  /// Something is moving or fading: draw again on the next frame.
  NextFrame,
  /// Everything is still, but a mark starts fading or expires after this long.
  After(Duration),
  /// Nothing changes until the next event.
  Idle,
}

/// One drawable state of the scene and when it next needs drawing.
#[derive(Clone, Debug, PartialEq)]
pub struct MotionFrame {
  pub overlay: Overlay,
  pub wake: Wake,
}

impl Easing {
  /// Maps linear progress in `[0, 1]` to eased progress. The curve is the shared
  /// contract the macOS renderer implements natively (`easeInOutExpo` in
  /// `Overlay.swift`); a Rust renderer must evaluate the same function.
  pub fn apply(self, progress: f64) -> f64 {
    if progress.is_nan() || progress <= 0.0 {
      return 0.0;
    }
    if progress >= 1.0 {
      return 1.0;
    }
    match self {
      Self::EaseInOutExpo => {
        if progress < 0.5 {
          2f64.powf(20.0 * progress - 10.0) / 2.0
        } else {
          (2.0 - 2f64.powf(-20.0 * progress + 10.0)) / 2.0
        }
      }
    }
  }
}

impl MotionOptions {
  /// Eased progress `elapsed` after a motion began. A zero duration has already arrived.
  pub fn progress(self, elapsed: Duration) -> f64 {
    if self.duration().is_zero() {
      return 1.0;
    }
    self.easing().apply(elapsed.as_secs_f64() / self.duration().as_secs_f64())
  }
}

const RIPPLE_DURATION: Duration = Duration::from_millis(450);
const RIPPLE_START_RADIUS: f64 = 6.0;
const RIPPLE_END_RADIUS: f64 = 34.0;
const RIPPLE_START_WIDTH: f64 = 3.0;
const RIPPLE_END_WIDTH: f64 = 1.0;
/// The cursor shows its pressed variant for this long after a click.
const PRESSED_DURATION: Duration = Duration::from_millis(180);
/// A window mark outlives its last report by this long, so a short operation sequence
/// leaves every window it touched marked at once.
const MARK_TTL: Duration = Duration::from_secs(3);
const MARK_FADE: Duration = Duration::from_millis(600);
/// NOTICE: bounds memory when a caller reports clicks faster than ripples finish. The
/// oldest ripple is dropped first. Normal operation never has this many live at once.
const MAX_RIPPLES: usize = 16;
/// Status pill placement inside a marked window's top-left corner.
const MARK_LABEL_INSET: (f64, f64) = (12.0, 16.0);

/// Departure point and live target of the cursor.
///
/// The cursor is drawn at `from + (to - from) * progress`. Retargeting a sampled stream
/// replaces `to` without touching `from` or `started`, so the cursor stays continuous
/// while it converges on the live position instead of restarting its ease per sample.
#[derive(Clone, Copy, Debug)]
struct Glide {
  from: Point,
  to: Point,
  started: Duration,
}

impl Glide {
  fn at(self, motion: MotionOptions, now: Duration) -> Point {
    let progress = motion.progress(now.saturating_sub(self.started));
    Point::new(self.from.x + (self.to.x - self.from.x) * progress, self.from.y + (self.to.y - self.from.y) * progress)
  }

  fn arrived(self, motion: MotionOptions, now: Duration) -> bool {
    now.saturating_sub(self.started) >= motion.duration()
  }
}

#[derive(Clone, Copy, Debug)]
struct Ripple {
  center: Point,
  started: Duration,
  button: MouseButton,
}

#[derive(Clone, Debug)]
struct Mark {
  id: String,
  frame: Rect,
  label: Option<String>,
  reported: Duration,
}

/// The live state behind one animated overlay.
#[derive(Clone, Debug)]
pub struct MotionScene {
  motion: MotionOptions,
  glide: Option<Glide>,
  last_click: Option<Duration>,
  ripples: Vec<Ripple>,
  marks: Vec<Mark>,
}

impl MotionScene {
  pub fn new(motion: MotionOptions) -> Self {
    Self {
      motion,
      glide: None,
      last_click: None,
      ripples: Vec::new(),
      marks: Vec::new(),
    }
  }

  /// Where the cursor is drawn at `now`, or `None` before any action placed it.
  pub fn cursor_point(&self, now: Duration) -> Option<ScreenPoint> {
    self.glide.map(|glide| ScreenPoint(glide.at(self.motion, now)))
  }

  /// Forgets everything drawn so far, for example when the overlay was removed.
  pub fn clear(&mut self) {
    self.glide = None;
    self.last_click = None;
    self.ripples.clear();
    self.marks.clear();
  }

  /// Applies one reported action that happened at `now`.
  pub fn apply(&mut self, event: ActionEvent, now: Duration) {
    match event {
      ActionEvent::Moved { point, travel } => self.move_cursor(point.point(), travel, now),
      ActionEvent::Clicked { point, button } => {
        let center = point.point();
        self.move_cursor(center, Travel::Jump, now);
        // NOTICE: the ripple starts at the click's true time and place, not when the
        // cursor arrives. Waiting for the glide would drop ripples of clicks reported
        // faster than the glide finishes, which hides real clicks.
        if self.ripples.len() == MAX_RIPPLES {
          self.ripples.remove(0);
        }
        self.ripples.push(Ripple {
          center,
          started: now,
          button,
        });
        self.last_click = Some(now);
      }
      ActionEvent::WindowTargeted { id, frame, label } => match self.marks.iter_mut().find(|mark| mark.id == id) {
        Some(mark) => {
          mark.frame = frame;
          mark.label = label;
          mark.reported = now;
        }
        None => self.marks.push(Mark {
          id,
          frame,
          label,
          reported: now,
        }),
      },
    }
  }

  fn move_cursor(&mut self, target: Point, travel: Travel, now: Duration) {
    let Some(current) = self.glide else {
      // First report: nothing to glide from, so the cursor appears at the real point.
      self.glide = Some(Glide {
        from: target,
        to: target,
        started: now,
      });
      return;
    };
    match travel {
      Travel::Sampled => {
        self.glide = Some(Glide {
          to: target,
          ..current
        })
      }
      Travel::Jump => {
        self.glide = Some(Glide {
          from: current.at(self.motion, now),
          to: target,
          started: now,
        });
      }
    }
  }

  /// Draws the scene at `now` and drops what has finished. Layer order is window marks,
  /// ripples, then the cursor on top.
  pub fn frame(&mut self, now: Duration) -> MotionFrame {
    self.ripples.retain(|ripple| now.saturating_sub(ripple.started) < RIPPLE_DURATION);
    self.marks.retain(|mark| now.saturating_sub(mark.reported) < MARK_TTL);

    let mut overlay = Overlay::new();
    for mark in &self.marks {
      let fade = mark_opacity(now.saturating_sub(mark.reported));
      overlay = overlay.with_layer(mark_outline(mark, fade));
      if let Some(label) = &mark.label {
        overlay = overlay.with_layer(mark_status(mark, label, fade));
      }
    }
    for ripple in &self.ripples {
      overlay = overlay.with_layer(self.ripple_outline(*ripple, now));
    }
    if let Some(glide) = self.glide {
      overlay = overlay.with_layer(self.cursor(glide, now));
    }

    MotionFrame {
      overlay,
      wake: self.wake(now),
    }
  }

  fn cursor(&self, glide: Glide, now: Duration) -> Cursor {
    let pressed = self.last_click.is_some_and(|clicked| now.saturating_sub(clicked) < PRESSED_DURATION);
    let (variant, style) = if pressed {
      (BuiltInCursor::AuvClick, CursorStyle::auv_click())
    } else {
      (BuiltInCursor::Auv, CursorStyle::auv())
    };
    Cursor::new(ScreenPoint(glide.at(self.motion, now))).with_image(CursorImage::built_in(variant)).with_style(style)
  }

  fn ripple_outline(&self, ripple: Ripple, now: Duration) -> Outline {
    let age = now.saturating_sub(ripple.started);
    let linear = age.as_secs_f64() / RIPPLE_DURATION.as_secs_f64();
    // The radius uses the scene's easing so every motion in the overlay shares one
    // curve; opacity is plain linear fade-out, which is not a position easing.
    let eased = self.motion.easing().apply(linear);
    let radius = RIPPLE_START_RADIUS + (RIPPLE_END_RADIUS - RIPPLE_START_RADIUS) * eased;
    let width = RIPPLE_START_WIDTH + (RIPPLE_END_WIDTH - RIPPLE_START_WIDTH) * linear.clamp(0.0, 1.0);
    let color = match ripple.button {
      MouseButton::Left => Color::AUV_CYAN,
      MouseButton::Right => Color::AUV_LIME,
      MouseButton::Middle => Color::WHITE,
    };
    let style = OutlineStyle::new()
      .with_stroke(Stroke::new(color.with_alpha((1.0 - linear).clamp(0.0, 1.0)), width))
      .with_padding(Insets::default())
      .with_corner_radius(radius);
    Outline::new(Rect::new(ripple.center.x - radius, ripple.center.y - radius, radius * 2.0, radius * 2.0)).with_style(style)
  }

  fn wake(&self, now: Duration) -> Wake {
    let gliding = self.glide.is_some_and(|glide| glide.from != glide.to && !glide.arrived(self.motion, now));
    let fading = self.marks.iter().any(|mark| now.saturating_sub(mark.reported) >= MARK_TTL - MARK_FADE);
    // The pressed cursor art lasts less than a ripple, so a live ripple already covers it.
    if gliding || !self.ripples.is_empty() || fading {
      return Wake::NextFrame;
    }
    match self.marks.iter().map(|mark| (MARK_TTL - MARK_FADE).saturating_sub(now.saturating_sub(mark.reported))).min() {
      Some(until_fade) => Wake::After(until_fade),
      None => Wake::Idle,
    }
  }
}

fn mark_opacity(age: Duration) -> f64 {
  let fade_start = MARK_TTL - MARK_FADE;
  if age <= fade_start {
    return 1.0;
  }
  (1.0 - (age - fade_start).as_secs_f64() / MARK_FADE.as_secs_f64()).clamp(0.0, 1.0)
}

fn scale_alpha(color: Color, opacity: f64) -> Color {
  color.with_alpha(color.alpha * opacity)
}

fn mark_outline(mark: &Mark, opacity: f64) -> Outline {
  let base = OutlineStyle::default();
  Outline::new(mark.frame).with_style(base.with_stroke(Stroke::new(scale_alpha(base.stroke.color, opacity), base.stroke.width)))
}

fn mark_status(mark: &Mark, label: &str, opacity: f64) -> Status {
  let base = StatusStyle::default();
  let style = base.with_foreground(scale_alpha(base.foreground, opacity)).with_background(scale_alpha(base.background, opacity));
  let anchor = ScreenPoint::new(mark.frame.origin.x + MARK_LABEL_INSET.0, mark.frame.origin.y + MARK_LABEL_INSET.1);
  Status::new(anchor, label).with_style(style)
}

#[cfg(test)]
#[path = "motion_test.rs"]
mod tests;
