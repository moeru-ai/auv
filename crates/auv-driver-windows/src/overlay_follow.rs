//! Reports input this driver delivered to a live overlay.
//!
//! The overlay is a visual trust adapter: it shows what AUV did, beside the user, and
//! delivers no input. Every event here is built after the driver delivered the action
//! it describes, from the point and window the delivery actually used. A failed
//! delivery reports nothing, so the overlay never shows an action that did not happen.
//!
//! Reporting never blocks or fails an operation: when no follower is running it is a
//! mutex check, and a stopped overlay's error is dropped.

use std::sync::Mutex;

use auv_driver_common::error::{DriverError, DriverResult};
use auv_driver_common::geometry::{Point, ScreenPoint};
use auv_driver_common::mouse_input::MotionEvent;
use auv_driver_common::window::Window;
use auv_driver_common::{Click, InputTarget, MouseButton};
use auv_driver_overlay::{ActionEvent, FrameStats, LifecycleOptions, LiveOverlay, Travel};

// TODO(overlay-follow-other-input): scroll, key, text and held-button delivery do not
// report yet. The overlay has no visual for them, and drawing a click-shaped ripple for a
// scroll would imply an action that did not happen. Add events and visuals together when
// an owner names the slice.
// TODO(overlay-follow-remote-runner): this follows input delivered inside this process. A
// Runner that executes input for remote callers needs the same hook at its own delivery
// seam before a remote UI can show the live overlay.

/// One follower per process: input delivery is process-global (`mouse_coordinator`), so
/// the overlay that mirrors it is too.
static FOLLOWER: Mutex<Option<LiveOverlay>> = Mutex::new(None);

/// Keeps a live overlay following this process's delivered input. Dropping it, or calling
/// [`OperationFollower::stop`], stops the overlay and removes it from the screen.
pub struct OperationFollower {
  _private: (),
}

pub(crate) fn follow(lifecycle: LifecycleOptions) -> DriverResult<OperationFollower> {
  let mut slot = FOLLOWER.lock().map_err(|_| backend("overlay follower state lock poisoned"))?;
  if slot.is_some() {
    return Err(backend("an overlay is already following this process's input"));
  }
  let live = LiveOverlay::start(lifecycle).map_err(|error| backend(&error.to_string()))?;
  *slot = Some(live);
  Ok(OperationFollower { _private: () })
}

impl OperationFollower {
  /// Shows `text`, the caller's description of its work such as "Recording the run",
  /// beside the cursor of `window` (None: of actions aimed at the screen), or removes it
  /// with `None`. Each window AUV acts in has its own cursor; a status keeps that cursor
  /// from fading out like an action does. The overlay shows the text as given and never
  /// writes one itself.
  ///
  /// TODO(overlay-live-status-producers): `auv invoke`, MCP and Runner callers cannot pass
  /// a status yet, because nothing starts a follower outside this API. Wire a producer when
  /// the owner names that slice, together with starting the follower from the CLI.
  pub fn set_status(&self, window: Option<&Window>, text: Option<&str>) -> DriverResult<()> {
    let slot = FOLLOWER.lock().map_err(|_| backend("overlay follower state lock poisoned"))?;
    let live = slot.as_ref().ok_or_else(|| backend("the overlay follower has stopped"))?;
    live.set_status(window.map(|window| window.reference.id.as_str()), text).map_err(|error| backend(&error.to_string()))
  }

  /// Stops following and returns the frame and latency measurements of the run.
  pub fn stop(self) -> DriverResult<FrameStats> {
    match take_live() {
      Some(live) => live.stop().map_err(|error| backend(&error.to_string())),
      None => Ok(FrameStats::default()),
    }
  }
}

impl Drop for OperationFollower {
  fn drop(&mut self) {
    drop(take_live());
  }
}

fn take_live() -> Option<LiveOverlay> {
  FOLLOWER.lock().ok().and_then(|mut slot| slot.take())
}

fn backend(message: &str) -> DriverError {
  DriverError::Backend {
    message: message.to_string(),
  }
}

/// Sends events to the running follower, if any.
pub(crate) fn report(events: impl IntoIterator<Item = ActionEvent>) {
  let Ok(slot) = FOLLOWER.lock() else {
    return;
  };
  let Some(live) = slot.as_ref() else {
    return;
  };
  for event in events {
    let _ = live.report(event);
  }
}

/// The clicks one delivered click request produced, one event per press. `window` is the
/// window the click was aimed at, whose cursor shows it; None for a click aimed at the
/// screen.
pub(crate) fn clicked(point: Point, button: MouseButton, click: &Click, window: Option<&Window>) -> Vec<ActionEvent> {
  (0..click.count())
    .map(|_| ActionEvent::Clicked {
      point: ScreenPoint(point),
      button,
      window: window.map(|window| window.reference.id.clone()),
    })
    .collect()
}

/// The window an operation acted on, with the frame it had when the operation ran.
pub(crate) fn window_targeted(window: &Window) -> ActionEvent {
  ActionEvent::WindowTargeted {
    id: window.reference.id.clone(),
    frame: window.frame,
    label: window.title.clone().filter(|title| !title.is_empty()).or_else(|| window.app_name.clone()),
  }
}

/// Turns one mouse movement's progress into overlay events.
///
/// `Started` reports nothing: the movement has not delivered anything yet. The first
/// delivered sample is where the logical mouse now is, so the cursor jumps there, and a
/// movement aimed at a window marks that window; later samples belong to the timed
/// trajectory and are followed without added delay. A movement aimed at a window moves
/// that window's cursor.
pub(crate) struct MotionReporter {
  target_window: Option<Window>,
  awaiting_first_sample: bool,
}

impl MotionReporter {
  pub(crate) fn new(target: Option<&InputTarget>) -> Self {
    Self {
      target_window: match target {
        Some(InputTarget::Window(window)) => Some(window.clone()),
        _ => None,
      },
      awaiting_first_sample: true,
    }
  }

  pub(crate) fn event(&mut self, event: &MotionEvent) -> Vec<ActionEvent> {
    match event {
      MotionEvent::Started { .. } => {
        self.awaiting_first_sample = true;
        Vec::new()
      }
      MotionEvent::Progress { sample, .. } => {
        let first = std::mem::take(&mut self.awaiting_first_sample);
        let mut events = Vec::with_capacity(2);
        if first && let Some(window) = &self.target_window {
          events.push(window_targeted(window));
        }
        events.push(ActionEvent::Moved {
          point: ScreenPoint(sample.point),
          travel: if first { Travel::Jump } else { Travel::Sampled },
          window: self.target_window.as_ref().map(|window| window.reference.id.clone()),
        });
        events
      }
    }
  }
}

/// A delivered pointer warp to `point`, which aims at the screen rather than a window.
pub(crate) fn moved_to(point: Point) -> ActionEvent {
  ActionEvent::Moved {
    point: ScreenPoint(point),
    travel: Travel::Jump,
    window: None,
  }
}

#[cfg(test)]
#[path = "overlay_follow_test.rs"]
mod tests;
