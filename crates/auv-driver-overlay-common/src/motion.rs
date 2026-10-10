//! Pure model of live overlay motion: how cursors follow the points an operation really
//! acted on, how clicks ripple and how targeted windows are marked.
//!
//! The model owns no clock, thread or renderer. Callers pass a monotonic `now`
//! (time since an epoch they choose) and get an [`Overlay`] back, so the same
//! animation runs against a fake clock in tests and against a platform frame loop in
//! production. Platform adapters only draw the returned layers.
//!
//! # Motion
//!
//! A cursor rides a critically damped spring toward the last reported point. Unlike a
//! fixed-duration ease, a spring keeps the cursor's speed when a new point arrives
//! mid-flight instead of stopping and starting again. Its position is evaluated in closed
//! form, so any instant is exact whatever the frame rate. The cursor tilts with its
//! horizontal speed and dips in size when a click lands. This is the live overlay's own
//! motion (owner decision, 2026-10-10); one-shot overlays keep the shared
//! [`crate::MotionOptions`] easing contract.
//!
//! # One cursor per window
//!
//! Each window an operation acts in gets its own cursor in its own color, and actions
//! aimed at the screen rather than a window drive one more (owner decision, 2026-10-10).
//! A window keeps its color while the scene lives, and its mark is drawn in that color
//! too. A window's first cursor sets off from the cursor that acted last, growing in as
//! it goes, as if AUV carried its pointer over; the very first cursor grows in on its
//! point. The cursor that acted last is drawn on top.
//!
//! # Status
//!
//! A caller can describe what it is doing in a window, such as "Recording the run", with
//! [`MotionScene::set_status`]. The text types out in a pill beside that window's cursor,
//! in the cursor's color.
//!
//! # Faithfulness
//!
//! Every cursor movement, ripple and mark comes from an [`ActionEvent`] that a driver
//! reported after the action happened. The scene moves toward reported points and fades
//! what it drew; it never moves toward a point nobody reported and never draws a click
//! that was not delivered. The spring never carries the cursor past the point it is
//! heading for, and a turn mid-flight swings at most a quarter of the remaining distance
//! off the direct line. Tilt and press scale pivot on the cursor's hotspot, so the tip
//! stays on the reported point. Motion changes how the cursor gets to a real point, not
//! where the operation acted.
//!
//! A cursor fades out a few seconds after its window's last action or status, so a parked
//! cursor never suggests work that stopped. A status is the caller's own words, shown as
//! given apart from a length bound; the scene never writes one.

use std::f64::consts::{E, PI};
use std::time::Duration;

use auv_driver_common::{MouseButton, Point, Rect, ScreenPoint};

use crate::Overlay;
use crate::layers::{BuiltInCursor, Cursor, CursorImage, CursorPose, Outline, Status};
use crate::style::{Color, CursorStyle, Insets, OutlineStyle, StatusStyle, Stroke};

// TODO(overlay-motion-event-wire): `ActionEvent` is an in-process type. It is not
// serializable and not a tracing event, so a remote Runner cannot stream it to a
// viewer yet. Unlock when a concrete out-of-process consumer needs it; the shape is
// provisional until then (see docs/ai/references/driver/2026-10-10-windows-overlay-motion.md).

/// How a reported cursor position was reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Travel {
  /// The operation acted at a new place in one step, such as a click target or a
  /// pointer warp. The cursor springs there from wherever it is drawn.
  Jump,
  /// One point of a trajectory the driver already timed. The cursor follows it with a
  /// short spring that trails the samples by about 25 ms, because a jump-length spring
  /// per sample would leave the cursor far behind the real path.
  Sampled,
}

/// One fact about a real action, reported by the driver that performed it.
///
/// Provisional name and shape: this is the overlay's projection of an operation, not a
/// tracing event and not a replacement for `InputActionResult`.
#[derive(Clone, Debug, PartialEq)]
pub enum ActionEvent {
  /// The operation moved the pointer, or aimed its cursor, at `point`. `window` is the
  /// id of the window the movement was aimed at (the `id` its `WindowTargeted` report
  /// uses), or `None` for movement aimed at the screen. It picks the cursor that moves.
  Moved {
    point: ScreenPoint,
    travel: Travel,
    window: Option<String>,
  },
  /// A click was delivered at `point`. It also places the cursor of `window`, chosen as
  /// for `Moved`, there.
  Clicked {
    point: ScreenPoint,
    button: MouseButton,
    window: Option<String>,
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

/// Spring time constant for a jump. The cursor covers about 60% of the way in this time
/// and comes within a pixel of a 600 px jump after about half a second. Owner-approved
/// feel (2026-10-10), chosen from a rendered side-by-side preview.
const JUMP_SMOOTH_TIME: Duration = Duration::from_millis(110);
/// Spring time constant while following a timed trajectory: the cursor trails the
/// driver's samples by about this long, which smooths their uneven arrival.
const SAMPLED_SMOOTH_TIME: Duration = Duration::from_millis(25);
/// A spring that can no longer move by more than this many pixels snaps onto its target.
const SETTLE_DISTANCE: f64 = 0.05;
/// Largest sideways swing after a turn mid-flight, as a share of the remaining distance.
const MAX_SWING: f64 = 0.25;
/// Tilt per unit of horizontal speed (degrees per px/s). Moving right turns the art
/// clockwise about its tip, so the body trails behind the tip.
const TILT_PER_SPEED: f64 = 0.006;
const MAX_TILT_DEGREES: f64 = 14.0;
/// How quickly the tilt follows the speed, per second. The lag smooths the speed changes
/// between the samples of a stream, which would otherwise make the art wobble.
const TILT_RATE: f64 = 18.0;
/// A tilt smaller than this, in degrees, snaps upright once the cursor has stopped.
const TILT_REST: f64 = 0.05;

/// Ripple timing, geometry and opacity match the macOS click ripple
/// (`drawFlashRippleIfActive` in `Overlay.swift`): a 2 px ring growing from 3 to 28 px
/// with an ease-out cubic, fading linearly from 0.7.
const RIPPLE_DURATION: Duration = Duration::from_millis(450);
const RIPPLE_START_RADIUS: f64 = 3.0;
const RIPPLE_END_RADIUS: f64 = 28.0;
const RIPPLE_WIDTH: f64 = 2.0;
const RIPPLE_OPACITY: f64 = 0.7;
/// The cursor shows its pressed art, and dips in size, for this long after a click.
const PRESSED_DURATION: Duration = Duration::from_millis(180);
/// How much smaller the cursor is at the deepest point of a press.
const PRESS_DEPTH: f64 = 0.16;
/// A window mark outlives its last report by this long, so a short operation sequence
/// leaves every window it touched marked at once.
const MARK_TTL: Duration = Duration::from_secs(3);
const MARK_FADE: Duration = Duration::from_millis(600);
/// NOTICE: bounds memory when a caller reports clicks faster than ripples finish. The
/// oldest ripple is dropped first. Normal operation never has this many live at once.
const MAX_RIPPLES: usize = 16;
/// Status pill placement inside a marked window's top-left corner.
const MARK_LABEL_INSET: (f64, f64) = (12.0, 16.0);
/// A cursor outlives its window's last action or status by this long, then fades out
/// over `CURSOR_FADE` by shrinking into its tip. It outlasts the window's mark, so a
/// short pause between two actions does not make the cursor blink out and back.
const CURSOR_TTL: Duration = Duration::from_secs(4);
const CURSOR_FADE: Duration = Duration::from_millis(600);
/// A new cursor grows from nothing to full size over this long.
const CURSOR_POP_IN: Duration = Duration::from_millis(260);
/// A cursor smaller than this share of its size is not drawn.
const MIN_DRAWN_SCALE: f64 = 0.05;
/// NOTICE: bounds memory when a caller reports actions or statuses for many windows in
/// quick succession. The cursor that acted longest ago is dropped first.
const MAX_CURSORS: usize = 16;
/// NOTICE: bounds the colors remembered over a long session. The oldest is forgotten
/// first, and its window takes a new color if an operation acts there again.
const MAX_REMEMBERED_COLORS: usize = 64;
/// A status types out one character per this long, like the website hero's ghost labels
/// (`labelAt` in `apps/docs-website/src/scene/film.ts`).
const STATUS_CHAR_TIME: Duration = Duration::from_millis(28);
/// A new status pill fades in over this long. One that replaces a showing status stays
/// up and only types its new text.
const STATUS_FADE_IN: Duration = Duration::from_millis(150);
/// NOTICE: bounds the pill width for caller text. Longer text is cut and ends with an
/// ellipsis.
const MAX_STATUS_CHARS: usize = 48;

/// One window's cursor color: the pointer art's accent and its status pill's background,
/// with the pill's text color.
#[derive(Clone, Copy, Debug, PartialEq)]
struct CursorColor {
  fill: Color,
  ink: Color,
}

const fn rgb8(red: u8, green: u8, blue: u8) -> Color {
  Color::rgb(red as f64 / 255.0, green as f64 / 255.0, blue as f64 / 255.0)
}

/// Cursor colors in the order windows take them: cyan, pink, violet, lime and amber,
/// after the website hero's ghost cursors (`GHOSTS` in `apps/docs-website/src/theme.ts`).
/// The cyan is the built-in pointer's own AUV cyan, so a run in one window looks as it did
/// before cursors were colored per window. After the fifth window the colors repeat.
const CURSOR_COLORS: [CursorColor; 5] = [
  CursorColor {
    fill: rgb8(0x2f, 0xd3, 0xdf),
    ink: rgb8(0x0b, 0x3a, 0x3c),
  },
  CursorColor {
    fill: rgb8(0xff, 0x74, 0xb1),
    ink: Color::WHITE,
  },
  CursorColor {
    fill: rgb8(0x9b, 0x7d, 0xff),
    ink: Color::WHITE,
  },
  CursorColor {
    fill: rgb8(0x86, 0xd9, 0x4a),
    ink: rgb8(0x1d, 0x3a, 0x08),
  },
  CursorColor {
    fill: rgb8(0xff, 0xb2, 0x38),
    ink: rgb8(0x4a, 0x2e, 0x00),
  },
];

/// One critically damped approach: the cursor left `start` at `started`, moving at
/// `velocity` (px/s), and heads for `target`.
///
/// The exact solution is `target + (c1 + c2·t)·e^(−ω·t)` with `c1 = start − target` and
/// `c2 = velocity + ω·c1`, so the cursor's place at any instant is computed directly
/// instead of integrated frame by frame.
#[derive(Clone, Copy, Debug)]
struct Spring {
  start: Point,
  velocity: Point,
  target: Point,
  started: Duration,
  /// Natural frequency ω: two over the smooth time.
  rate: f64,
}

impl Spring {
  fn resting(point: Point, now: Duration) -> Self {
    Self {
      start: point,
      velocity: Point::default(),
      target: point,
      started: now,
      rate: rate(JUMP_SMOOTH_TIME),
    }
  }

  fn coefficients(self) -> (Point, Point) {
    let c1 = minus(self.start, self.target);
    (c1, plus(self.velocity, times(c1, self.rate)))
  }

  /// Whether the spring is within `SETTLE_DISTANCE` of its target at `now` for good: past
  /// the peak of its envelope `(|c1| + |c2|·t)·e^(−ω·t)` and below the threshold.
  fn settled(self, now: Duration) -> bool {
    let (c1, c2) = self.coefficients();
    let (near, speed) = (length(c1), length(c2));
    let t = now.saturating_sub(self.started).as_secs_f64();
    let peak = if speed > 0.0 {
      (1.0 / self.rate - near / speed).max(0.0)
    } else {
      0.0
    };
    t >= peak && (near + speed * t) * (-self.rate * t).exp() <= SETTLE_DISTANCE
  }

  /// Position and velocity at `now`. A settled spring reports exactly its target, so the
  /// cursor comes to rest on the reported point, not a fraction of a pixel off it.
  fn state(self, now: Duration) -> (Point, Point) {
    if self.settled(now) {
      return (self.target, Point::default());
    }
    let (c1, c2) = self.coefficients();
    let t = now.saturating_sub(self.started).as_secs_f64();
    let decay = (-self.rate * t).exp();
    let offset = plus(c1, times(c2, t));
    (plus(self.target, times(offset, decay)), times(minus(c2, times(offset, self.rate)), decay))
  }

  /// Heads for `target` from wherever the cursor is drawn at `now`, keeping its speed.
  ///
  /// Two caps keep the motion faithful. A critically damped spring passes its target only
  /// when it approaches faster than `ω·distance`, so speed toward the target is capped
  /// there. Sideways speed `s` swings the cursor at most `s / (ω·e)` off the direct line,
  /// so it is capped to keep that swing within `MAX_SWING` of the distance.
  fn retarget(self, target: Point, smooth_time: Duration, now: Duration) -> Self {
    let (position, velocity) = self.state(now);
    let rate = rate(smooth_time);
    let offset = minus(target, position);
    let distance = length(offset);
    let velocity = if distance > 0.0 {
      let toward = times(offset, 1.0 / distance);
      let along = dot(velocity, toward);
      let side = minus(velocity, times(toward, along));
      let max_side = rate * E * MAX_SWING * distance;
      let side = if length(side) > max_side {
        times(side, max_side / length(side))
      } else {
        side
      };
      plus(times(toward, along.min(rate * distance)), side)
    } else {
      Point::default()
    };
    Self {
      start: position,
      velocity,
      target,
      started: now,
      rate,
    }
  }
}

fn rate(smooth_time: Duration) -> f64 {
  2.0 / smooth_time.as_secs_f64()
}

fn plus(a: Point, b: Point) -> Point {
  Point::new(a.x + b.x, a.y + b.y)
}

fn minus(a: Point, b: Point) -> Point {
  Point::new(a.x - b.x, a.y - b.y)
}

fn times(a: Point, factor: f64) -> Point {
  Point::new(a.x * factor, a.y * factor)
}

fn dot(a: Point, b: Point) -> f64 {
  a.x * b.x + a.y * b.y
}

fn length(a: Point) -> f64 {
  a.x.hypot(a.y)
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
  color: CursorColor,
  reported: Duration,
}

/// The cursor of one window, or of actions aimed at the screen (`window` None).
#[derive(Clone, Debug)]
struct Track {
  window: Option<String>,
  color: CursorColor,
  /// None until an action places the cursor; a status can arrive first.
  spring: Option<Spring>,
  /// When an action first placed the cursor.
  appeared: Duration,
  /// Current tilt in degrees. It follows the speed with a lag, so it is the one piece of
  /// state that advances per drawn frame rather than in closed form.
  tilt: f64,
  last_click: Option<Duration>,
  /// The window's last action or status. The cursor fades out `CURSOR_TTL` after it.
  active: Duration,
  status: Option<StatusText>,
}

#[derive(Clone, Debug)]
struct StatusText {
  text: String,
  set: Duration,
  /// It replaced a status that was showing, so the pill stays up and only retypes.
  replaced: bool,
}

impl Track {
  fn new(window: Option<&str>, color: CursorColor, now: Duration) -> Self {
    Self {
      window: window.map(str::to_owned),
      color,
      spring: None,
      appeared: now,
      tilt: 0.0,
      last_click: None,
      active: now,
      status: None,
    }
  }

  /// Moves the tilt toward the angle that matches the cursor's speed, by the share of its
  /// lag that `elapsed` since the previous frame covers.
  fn follow_speed(&mut self, elapsed: Duration, now: Duration) {
    let velocity = self.spring.map_or(Point::default(), |spring| spring.state(now).1);
    let target = (velocity.x * TILT_PER_SPEED).clamp(-MAX_TILT_DEGREES, MAX_TILT_DEGREES);
    self.tilt += (target - self.tilt) * (1.0 - (-TILT_RATE * elapsed.as_secs_f64()).exp());
    if target == 0.0 && self.tilt.abs() < TILT_REST {
      self.tilt = 0.0;
    }
  }

  fn pressed(&self, now: Duration) -> Option<Duration> {
    self.last_click.map(|clicked| now.saturating_sub(clicked)).filter(|age| *age < PRESSED_DURATION)
  }

  /// 1 while the window is active, falling to 0 over the last `CURSOR_FADE` of the
  /// cursor's life.
  fn presence(&self, now: Duration) -> f64 {
    fade_out(now.saturating_sub(self.active), CURSOR_TTL, CURSOR_FADE)
  }

  /// How long `status` has been typing at `now`. A status set before its cursor appeared
  /// starts typing when the cursor appears.
  fn status_age(&self, status: &StatusText, now: Duration) -> Duration {
    now.saturating_sub(status.set.max(self.appeared))
  }

  /// Whether the cursor looks different on the next frame without a new event.
  fn animating(&self, now: Duration) -> bool {
    let Some(spring) = self.spring else {
      return false;
    };
    let typing = self.status.as_ref().is_some_and(|status| {
      let age = self.status_age(status, now);
      (!status.replaced && age < STATUS_FADE_IN) || typed_chars(age) < status.text.chars().count()
    });
    !spring.settled(now)
      || self.tilt != 0.0
      || now.saturating_sub(self.appeared) < CURSOR_POP_IN
      || self.pressed(now).is_some()
      || self.presence(now) < 1.0
      || typing
  }

  /// The cursor layer at `now`, or None while no action has placed it or it is too small
  /// to see.
  fn cursor(&self, now: Duration) -> Option<Cursor> {
    let (position, _) = self.spring?.state(now);
    let pressed = self.pressed(now);
    let (variant, style) = match pressed {
      Some(_) => (BuiltInCursor::AuvClick, CursorStyle::auv_click()),
      None => (BuiltInCursor::Auv, CursorStyle::auv()),
    };
    // The press dips the cursor and brings it back along half a sine, so it never jumps
    // in size at either end.
    let press = pressed.map_or(1.0, |age| 1.0 - PRESS_DEPTH * (PI * age.as_secs_f64() / PRESSED_DURATION.as_secs_f64()).sin());
    // A new cursor grows in, and a leaving one shrinks into its tip, ever faster.
    let presence = self.presence(now);
    let scale = press * grow_in(now.saturating_sub(self.appeared)) * (1.0 - (1.0 - presence).powi(2));
    if scale < MIN_DRAWN_SCALE {
      return None;
    }

    let mut style = style.with_accent(Some(self.color.fill));
    let mut cursor = Cursor::new(ScreenPoint(position)).with_image(CursorImage::built_in(variant));
    if let Some(status) = &self.status {
      let age = self.status_age(status, now);
      let fade_in = if status.replaced {
        1.0
      } else {
        (age.as_secs_f64() / STATUS_FADE_IN.as_secs_f64()).min(1.0)
      };
      let opacity = presence * fade_in;
      style = style.with_label_background(self.color.fill.with_alpha(opacity)).with_label_foreground(self.color.ink.with_alpha(opacity));
      cursor = cursor.with_label(status.text.chars().take(typed_chars(age)).collect::<String>()).with_label_visible();
    }
    Some(cursor.with_style(style).with_pose(CursorPose {
      tilt_degrees: self.tilt,
      scale,
    }))
  }
}

/// The live state behind one animated overlay.
#[derive(Clone, Debug, Default)]
pub struct MotionScene {
  /// One cursor per window, in drawing order: the one that acted last is on top.
  tracks: Vec<Track>,
  /// The color each window took, oldest first, so a window keeps its color when its
  /// cursor comes back after fading out.
  colors: Vec<(Option<String>, CursorColor)>,
  next_color: usize,
  last_frame: Option<Duration>,
  ripples: Vec<Ripple>,
  marks: Vec<Mark>,
}

impl MotionScene {
  pub fn new() -> Self {
    Self::default()
  }

  /// Where the cursor of `window` (None: of actions aimed at the screen) is drawn at
  /// `now`, or `None` while no action has placed it.
  pub fn cursor_point(&self, window: Option<&str>, now: Duration) -> Option<ScreenPoint> {
    let track = self.tracks.iter().find(|track| track.window.as_deref() == window)?;
    track.spring.map(|spring| ScreenPoint(spring.state(now).0))
  }

  /// Forgets everything drawn so far, for example when the overlay was removed.
  pub fn clear(&mut self) {
    *self = Self::default();
  }

  /// Applies one reported action that happened at `now`.
  pub fn apply(&mut self, event: ActionEvent, now: Duration) {
    self.expire(now);
    match event {
      ActionEvent::Moved {
        point,
        travel,
        window,
      } => {
        self.move_cursor(window.as_deref(), point.point(), travel, now);
      }
      ActionEvent::Clicked {
        point,
        button,
        window,
      } => {
        let center = point.point();
        self.move_cursor(window.as_deref(), center, Travel::Jump, now).last_click = Some(now);
        // NOTICE: the ripple starts at the click's true time and place, not when the
        // cursor arrives. Waiting for the cursor would drop ripples of clicks reported
        // faster than it travels, which hides real clicks.
        if self.ripples.len() == MAX_RIPPLES {
          self.ripples.remove(0);
        }
        self.ripples.push(Ripple {
          center,
          started: now,
          button,
        });
      }
      ActionEvent::WindowTargeted { id, frame, label } => {
        let color = self.color_for(Some(&id));
        match self.marks.iter_mut().find(|mark| mark.id == id) {
          Some(mark) => {
            mark.frame = frame;
            mark.label = label;
            mark.reported = now;
          }
          None => self.marks.push(Mark {
            id,
            frame,
            label,
            color,
            reported: now,
          }),
        }
      }
    }
  }

  /// Shows `text` in a pill beside the cursor of `window` (None: of actions aimed at the
  /// screen), replacing its status, or removes the pill when `text` is `None` or blank.
  ///
  /// The text is the caller's description of its own work and is shown as given, on one
  /// line and cut to `MAX_STATUS_CHARS` characters. Setting a status keeps the cursor
  /// from fading out, like an action does. A status for a window no action has placed a
  /// cursor in yet appears with that cursor if an action follows within `CURSOR_TTL`.
  pub fn set_status(&mut self, window: Option<&str>, text: Option<&str>, now: Duration) {
    self.expire(now);
    let Some(text) = text.map(one_line).filter(|text| !text.is_empty()) else {
      if let Some(track) = self.tracks.iter_mut().find(|track| track.window.as_deref() == window) {
        track.status = None;
      }
      return;
    };
    let index = self.track_index(window, now);
    let track = &mut self.tracks[index];
    track.active = now;
    track.status = Some(StatusText {
      text,
      set: now,
      replaced: track.status.is_some() && track.spring.is_some(),
    });
  }

  /// Drops the cursors whose window has been quiet for `CURSOR_TTL`.
  fn expire(&mut self, now: Duration) {
    self.tracks.retain(|track| now.saturating_sub(track.active) < CURSOR_TTL);
  }

  /// The color of `window`'s cursor, taking the next one in `CURSOR_COLORS` on first
  /// sight.
  fn color_for(&mut self, window: Option<&str>) -> CursorColor {
    if let Some((_, color)) = self.colors.iter().find(|(key, _)| key.as_deref() == window) {
      return *color;
    }
    let color = CURSOR_COLORS[self.next_color % CURSOR_COLORS.len()];
    self.next_color += 1;
    if self.colors.len() == MAX_REMEMBERED_COLORS {
      self.colors.remove(0);
    }
    self.colors.push((window.map(str::to_owned), color));
    color
  }

  /// The index of `window`'s cursor, adding one that has no position yet if needed.
  fn track_index(&mut self, window: Option<&str>, now: Duration) -> usize {
    if let Some(index) = self.tracks.iter().position(|track| track.window.as_deref() == window) {
      return index;
    }
    if self.tracks.len() == MAX_CURSORS {
      self.tracks.remove(0);
    }
    let color = self.color_for(window);
    self.tracks.push(Track::new(window, color, now));
    self.tracks.len() - 1
  }

  /// Moves `window`'s cursor toward `target` and draws it on top of the others.
  fn move_cursor(&mut self, window: Option<&str>, target: Point, travel: Travel, now: Duration) -> &mut Track {
    let smooth_time = match travel {
      Travel::Jump => JUMP_SMOOTH_TIME,
      Travel::Sampled => SAMPLED_SMOOTH_TIME,
    };
    let index = self.track_index(window, now);
    let mut track = self.tracks.remove(index);
    let spring = match track.spring {
      Some(spring) => spring.retarget(target, smooth_time, now),
      None => {
        track.appeared = now;
        // A window's first action: its cursor sets off from wherever the cursor that
        // acted last is drawn, keeping that cursor's speed. Only the very first cursor
        // has nothing to travel from and appears on the reported point.
        match self.tracks.iter().rev().find_map(|other| other.spring) {
          Some(last) => last.retarget(target, smooth_time, now),
          None => Spring::resting(target, now),
        }
      }
    };
    track.spring = Some(spring);
    track.active = now;
    self.tracks.push(track);
    self.tracks.last_mut().expect("the cursor was just put back")
  }

  /// Draws the scene at `now` and drops what has finished. Layer order is window marks,
  /// ripples, then the cursors, the one that acted last on top.
  pub fn frame(&mut self, now: Duration) -> MotionFrame {
    self.ripples.retain(|ripple| now.saturating_sub(ripple.started) < RIPPLE_DURATION);
    self.marks.retain(|mark| now.saturating_sub(mark.reported) < MARK_TTL);
    self.expire(now);
    let elapsed = self.last_frame.map_or(Duration::ZERO, |last| now.saturating_sub(last));
    self.last_frame = Some(now);
    for track in &mut self.tracks {
      track.follow_speed(elapsed, now);
    }

    let mut overlay = Overlay::new();
    for mark in &self.marks {
      let fade = fade_out(now.saturating_sub(mark.reported), MARK_TTL, MARK_FADE);
      overlay = overlay.with_layer(mark_outline(mark, fade));
      if let Some(label) = &mark.label {
        overlay = overlay.with_layer(mark_status(mark, label, fade));
      }
    }
    for ripple in &self.ripples {
      overlay = overlay.with_layer(ripple_outline(*ripple, now));
    }
    for cursor in self.tracks.iter().filter_map(|track| track.cursor(now)) {
      overlay = overlay.with_layer(cursor);
    }

    MotionFrame {
      overlay,
      wake: self.wake(now),
    }
  }

  fn wake(&self, now: Duration) -> Wake {
    let animating = self.tracks.iter().any(|track| track.animating(now));
    let fading = self.marks.iter().any(|mark| now.saturating_sub(mark.reported) >= MARK_TTL - MARK_FADE);
    if animating || !self.ripples.is_empty() || fading {
      return Wake::NextFrame;
    }
    // Everything is still until a mark or a drawn cursor starts fading.
    let marks = self.marks.iter().map(|mark| (MARK_TTL - MARK_FADE).saturating_sub(now.saturating_sub(mark.reported)));
    let cursors = self
      .tracks
      .iter()
      .filter(|track| track.spring.is_some())
      .map(|track| (CURSOR_TTL - CURSOR_FADE).saturating_sub(now.saturating_sub(track.active)));
    match marks.chain(cursors).min() {
      Some(until_fade) => Wake::After(until_fade),
      None => Wake::Idle,
    }
  }
}

/// Size of a new cursor `age` after it appeared: an ease-out-back from nothing that
/// overshoots to about 110% before it settles at exactly full size.
fn grow_in(age: Duration) -> f64 {
  const OVERSHOOT: f64 = 1.70158;
  let t = (age.as_secs_f64() / CURSOR_POP_IN.as_secs_f64()).min(1.0) - 1.0;
  1.0 + (OVERSHOOT + 1.0) * t.powi(3) + OVERSHOOT * t.powi(2)
}

/// Characters of a status shown `age` after it started typing: the first one at once.
fn typed_chars(age: Duration) -> usize {
  (age.as_nanos() / STATUS_CHAR_TIME.as_nanos()) as usize + 1
}

/// Caller text as one line of at most `MAX_STATUS_CHARS` characters: control characters
/// such as line breaks become spaces, and a cut ends with an ellipsis.
fn one_line(text: &str) -> String {
  let line: String = text.trim().chars().map(|ch| if ch.is_control() { ' ' } else { ch }).collect();
  if line.chars().count() <= MAX_STATUS_CHARS {
    return line;
  }
  let mut cut: String = line.chars().take(MAX_STATUS_CHARS - 1).collect();
  cut.push('…');
  cut
}

/// Opacity of something that lives `ttl` and fades out over the last `fade` of it.
fn fade_out(age: Duration, ttl: Duration, fade: Duration) -> f64 {
  let fade_start = ttl - fade;
  if age <= fade_start {
    return 1.0;
  }
  (1.0 - (age - fade_start).as_secs_f64() / fade.as_secs_f64()).clamp(0.0, 1.0)
}

fn ripple_outline(ripple: Ripple, now: Duration) -> Outline {
  let progress = (now.saturating_sub(ripple.started).as_secs_f64() / RIPPLE_DURATION.as_secs_f64()).clamp(0.0, 1.0);
  // Ease-out cubic: the ring leaves the click point quickly and slows as it fades.
  let grown = 1.0 - (1.0 - progress).powi(3);
  let radius = RIPPLE_START_RADIUS + (RIPPLE_END_RADIUS - RIPPLE_START_RADIUS) * grown;
  let color = match ripple.button {
    MouseButton::Left => Color::AUV_LIME,
    MouseButton::Right => Color::AUV_CYAN,
    MouseButton::Middle => Color::WHITE,
  };
  let style = OutlineStyle::new()
    .with_stroke(Stroke::new(color.with_alpha(RIPPLE_OPACITY * (1.0 - progress)), RIPPLE_WIDTH))
    .with_padding(Insets::default())
    .with_corner_radius(radius);
  Outline::new(Rect::new(ripple.center.x - radius, ripple.center.y - radius, radius * 2.0, radius * 2.0)).with_style(style)
}

/// A window's mark is outlined in its cursor's color.
fn mark_outline(mark: &Mark, opacity: f64) -> Outline {
  let base = OutlineStyle::default();
  Outline::new(mark.frame).with_style(base.with_stroke(Stroke::new(mark.color.fill.with_alpha(opacity), base.stroke.width)))
}

/// The window's name in a pill of its cursor's color, as translucent as the default
/// status pill.
fn mark_status(mark: &Mark, label: &str, opacity: f64) -> Status {
  let base = StatusStyle::default();
  let style =
    base.with_foreground(mark.color.ink.with_alpha(opacity)).with_background(mark.color.fill.with_alpha(base.background.alpha * opacity));
  let anchor = ScreenPoint::new(mark.frame.origin.x + MARK_LABEL_INSET.0, mark.frame.origin.y + MARK_LABEL_INSET.1);
  Status::new(anchor, label).with_style(style)
}

#[cfg(test)]
#[path = "motion_test.rs"]
mod tests;
