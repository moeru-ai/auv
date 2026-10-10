//! Frame pacing for the animator loop: when to draw, when to wait and when to remove.
//!
//! Pure over `Instant` values passed in by the caller, so the decisions are unit
//! tested on every platform and the Win32 thread only executes them.

use std::time::{Duration, Instant};

use auv_driver_overlay_common::Wake;

/// 60 frames per second. The renderer's measured full-frame cost (P50 about 9 ms on the
/// #306 evidence machine) leaves headroom under this budget.
pub(crate) const FRAME_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / 60);

/// What the loop should do next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
  /// Compose and draw a frame now.
  Render,
  /// Nothing to do until `Instant`, unless an event arrives first.
  WaitUntil(Instant),
  /// Nothing to do until an event arrives.
  WaitForEvent,
  /// The overlay has been idle long enough: clear the scene and hide the window.
  Remove,
}

pub(crate) struct Pacer {
  interval: Duration,
  idle_removal: Option<Duration>,
  last_frame: Option<Instant>,
  /// Set by events: draw soon, but not faster than one frame per `interval`.
  dirty: bool,
  /// When the scene asked to be drawn again without an event.
  due: Option<Instant>,
  /// When the scene last became fully still, for idle removal.
  idle_since: Option<Instant>,
}

impl Pacer {
  pub(crate) fn new(interval: Duration, idle_removal: Option<Duration>) -> Self {
    Self {
      interval,
      idle_removal,
      last_frame: None,
      dirty: false,
      due: None,
      idle_since: None,
    }
  }

  /// An action event arrived, so the scene changed and must be drawn.
  pub(crate) fn event(&mut self) {
    self.dirty = true;
    self.idle_since = None;
  }

  /// A frame was composed at `at`; `wake` says when the scene next needs drawing.
  pub(crate) fn drew(&mut self, at: Instant, wake: Wake) {
    self.last_frame = Some(at);
    self.dirty = false;
    self.due = match wake {
      Wake::NextFrame => Some(at + self.interval),
      Wake::After(wait) => Some(at + wait),
      Wake::Idle => None,
    };
    self.idle_since = match wake {
      Wake::Idle => self.idle_since.or(Some(at)),
      Wake::NextFrame | Wake::After(_) => None,
    };
  }

  /// The overlay was removed; stay quiet until the next event.
  pub(crate) fn removed(&mut self) {
    self.dirty = false;
    self.due = None;
    self.idle_since = None;
  }

  pub(crate) fn step(&self, now: Instant) -> Step {
    let earliest = self.last_frame.map_or(now, |last| last + self.interval);
    let due = self.due.filter(|due| *due <= now);
    if self.dirty || due.is_some() {
      return if now >= earliest {
        Step::Render
      } else {
        Step::WaitUntil(earliest)
      };
    }
    if let Some(due) = self.due {
      return Step::WaitUntil(due.max(earliest));
    }
    match (self.idle_since, self.idle_removal) {
      (Some(since), Some(after)) if now >= since + after => Step::Remove,
      (Some(since), Some(after)) => Step::WaitUntil(since + after),
      _ => Step::WaitForEvent,
    }
  }
}

#[cfg(test)]
#[path = "pacing_test.rs"]
mod tests;
