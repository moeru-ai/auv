//! A sustained background session: up to five target windows an agent operates and watches while
//! the user keeps working in another window.
//!
//! The contract is what makes it safe to leave running:
//!
//! - **Background only, by construction.** Operations take no input policy. The only delivery
//!   routes are posted window messages ([`crate::background_input`]) and Windows.Graphics.Capture.
//!   Nothing here activates, restores, raises or resizes a window, moves the real cursor, or sends
//!   global input, and there is no fallback to any of those: [`Backend`] has no such operation to
//!   fall back to. An operation that cannot be done in the background reports
//!   [`Outcome::Unsupported`].
//! - **Delivery is not effect.** A posted message means the target's queue accepted it, not that
//!   the application acted on it ([`InputActionResult::verified`] stays `false`). Semantic
//!   verification is a separate step; see [`crate::background_input`] for which toolkits ignore
//!   posted input.
//! - **Targets are cached.** A window is discovered once, by the caller, and [`attach`ed](BackgroundSession::attach).
//!   Every operation then revalidates it with one cheap handle probe instead of enumerating
//!   windows, and follows it if it moved or was resized.
//! - **Degradation is typed, and the run goes on.** A closed window marks its target
//!   [`TargetState::Lost`] and every later operation on it answers [`Outcome::TargetLost`], while
//!   the other targets carry on. A minimized window answers [`Outcome::SkippedMinimized`] and is
//!   never restored, because DWM stops rendering it.
//! - **Observation never goes stale.** Each target has a [`crate::wgc::FrameStream`] that drains
//!   continuously and keeps only the newest frame, so a capture is never an older state of the
//!   window and watching five windows never renegotiates a capture session.

use auv_driver_common::Position;
use auv_driver_common::capture::Capture;
use auv_driver_common::error::DriverResult;
use auv_driver_common::geometry::{Point, Rect, WindowPoint};
use auv_driver_common::input::{Click, ClickModifiers, InputActionResult, InputDeliveryPath, MouseButton, Scroll};
use auv_driver_common::window::Window;
use serde::Serialize;

use crate::background_input;
use crate::wgc::{FrameUpdate, StreamError};
use crate::window::WindowProbe;

/// How many windows one session targets at a time.
pub const MAX_TARGETS: usize = 5;

/// How long the first read of a target's stream waits for the compositor's first frame.
#[cfg(target_os = "windows")]
const FIRST_FRAME_WAIT: std::time::Duration = std::time::Duration::from_millis(1500);

/// A target within one session. Ids are never reused, so one that outlives its target cannot
/// name another.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct TargetId(u32);

impl std::fmt::Display for TargetId {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "target-{}", self.0)
  }
}

/// Why a target cannot be operated on any more.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LostReason {
  /// The window no longer exists.
  WindowDestroyed,
  /// The handle now belongs to another process.
  ProcessChanged,
  /// The handle was reused by a different window of the same process.
  HandleReused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TargetState {
  /// Operable.
  Live,
  /// Minimized. Windows stops rendering a minimized window, so nothing sent to it could be seen
  /// and nothing can be captured. The session never restores it; it is operable again when the
  /// user restores it.
  Minimized,
  /// Gone for good: the session never re-attaches it. Attach the new window instead.
  Lost { reason: LostReason },
}

/// A target that moved or was resized between two operations, and which the session followed.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct FrameChange {
  pub from: Rect,
  pub to: Rect,
}

/// The result of an operation on a target.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Outcome<T> {
  /// The operation ran.
  Done { value: T },
  /// The target is minimized, so the operation was skipped. Nothing was sent and the window was
  /// left minimized.
  SkippedMinimized,
  /// The target is gone. The session marked it lost; other targets are unaffected.
  TargetLost { reason: LostReason },
  /// The operation cannot be done safely in the background on Windows. Nothing was sent, and the
  /// session did not try the foreground instead.
  Unsupported {
    operation: &'static str,
    reason: String,
  },
  /// The operation was attempted and the OS or the target refused it, or the request was invalid.
  Failed {
    operation: &'static str,
    message: String,
  },
}

impl<T> Outcome<T> {
  pub fn is_done(&self) -> bool {
    matches!(self, Self::Done { .. })
  }

  /// The value, if the operation ran.
  pub fn done(self) -> Option<T> {
    match self {
      Self::Done { value } => Some(value),
      _ => None,
    }
  }

  /// The same answer with the value dropped: for logging an outcome whose value is a frame.
  pub fn without_value(&self) -> Outcome<()> {
    match self {
      Self::Done { .. } => Outcome::Done { value: () },
      Self::SkippedMinimized => Outcome::SkippedMinimized,
      Self::TargetLost { reason } => Outcome::TargetLost { reason: *reason },
      Self::Unsupported { operation, reason } => Outcome::Unsupported {
        operation,
        reason: reason.clone(),
      },
      Self::Failed { operation, message } => Outcome::Failed {
        operation,
        message: message.clone(),
      },
    }
  }

  pub fn map<U>(self, convert: impl FnOnce(T) -> U) -> Outcome<U> {
    match self {
      Self::Done { value } => Outcome::Done {
        value: convert(value),
      },
      Self::SkippedMinimized => Outcome::SkippedMinimized,
      Self::TargetLost { reason } => Outcome::TargetLost { reason },
      Self::Unsupported { operation, reason } => Outcome::Unsupported { operation, reason },
      Self::Failed { operation, message } => Outcome::Failed { operation, message },
    }
  }
}

/// Input delivered to a target.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Delivery {
  pub target: TargetId,
  /// What was sent, and by which path. `verified` is always `false`: delivery says the target's
  /// message queue accepted the input, not that the application did anything with it.
  pub result: InputActionResult,
  /// The frame the point was resolved against: the target's live frame at the time of the call.
  pub frame: Rect,
  /// Set when the target had moved or been resized since the previous operation and this one
  /// followed it.
  pub followed: Option<FrameChange>,
}

/// What a capture of a target found.
#[expect(
  clippy::large_enum_variant,
  reason = "the pixels are on the heap already; boxing the small rest buys nothing"
)]
pub enum FrameRead {
  /// The window has changed since the previous capture of it.
  Fresh {
    capture: Capture,
    frame_number: u64,
    followed: Option<FrameChange>,
  },
  /// The window looks as it did at the previous capture. No pixels were copied.
  Unchanged { frame_number: u64 },
}

/// Metadata only: a frame is megabytes of pixels that a log line must never carry.
impl std::fmt::Debug for FrameRead {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Fresh {
        capture,
        frame_number,
        followed,
      } => f
        .debug_struct("Fresh")
        .field("frame_number", frame_number)
        .field("size", &format_args!("{}x{}", capture.image.width(), capture.image.height()))
        .field("followed", followed)
        .finish(),
      Self::Unchanged { frame_number } => f.debug_struct("Unchanged").field("frame_number", frame_number).finish(),
    }
  }
}

/// A click to deliver. There is no input policy: a session only ever delivers in the background.
#[derive(Clone, Debug, PartialEq)]
pub struct ClickRequest {
  pub button: MouseButton,
  pub click: Click,
  /// Posted mouse messages carry Shift and Control only; Alt and Meta are [`Outcome::Unsupported`].
  pub modifiers: ClickModifiers,
}

impl Default for ClickRequest {
  fn default() -> Self {
    Self {
      button: MouseButton::Left,
      click: Click::Single,
      modifiers: ClickModifiers::default(),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TargetInfo {
  pub id: TargetId,
  /// The window handle, as the driver's window references carry it.
  pub window: String,
  pub process_id: u32,
  pub state: TargetState,
  /// The last known frame: kept current by every operation and by [`BackgroundSession::refresh`].
  pub frame: Rect,
}

/// Counters for the session's own work, for evidence that it caches what it should.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SessionStats {
  pub attached: u64,
  /// Handle probes: one per operation or refresh. These are not window enumerations.
  pub probes: u64,
  pub clicks: u64,
  pub scrolls: u64,
  /// Captures that returned pixels.
  pub frames_fresh: u64,
  /// Captures that found nothing new and copied nothing.
  pub frames_unchanged: u64,
  pub streams_opened: u64,
  /// Streams reopened after a fault.
  pub streams_reopened: u64,
  /// Streams replaced because the window's capture region changed size; see
  /// [`crate::wgc::StreamError::Resized`].
  pub streams_resized: u64,
  pub skipped_minimized: u64,
  pub lost: u64,
  pub unsupported: u64,
  /// Frames the compositor delivered to the session's streams, closed ones included, whether or not
  /// anyone read them. Constant while the windows do not change.
  pub frames_delivered: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttachError {
  /// The session already targets [`MAX_TARGETS`] windows.
  Full,
  /// The window is already a target.
  AlreadyAttached(TargetId),
  /// The reference is not a window handle.
  NotAWindowHandle(String),
  /// No such window exists, or the handle now belongs to another process than the window says.
  Gone,
}

impl std::fmt::Display for AttachError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Full => write!(f, "the session already targets {MAX_TARGETS} windows"),
      Self::AlreadyAttached(id) => write!(f, "the window is already {id}"),
      Self::NotAWindowHandle(reference) => write!(f, "window reference {reference:?} is not a window handle"),
      Self::Gone => write!(f, "the window no longer exists"),
    }
  }
}

impl std::error::Error for AttachError {}

/// The OS operations a session needs, and no others. This is the real external boundary: tests
/// stand in a fake for it, and its absence of any foreground, activation or global-input operation
/// is what makes a fallback impossible.
pub(crate) trait Backend: Send {
  /// The live state of a window handle, without enumerating windows.
  fn probe(&self, hwnd: isize) -> WindowProbe;
  fn post_click(&self, window: &Window, screen_point: Point, request: &ClickRequest) -> DriverResult<()>;
  fn post_scroll(&self, window: &Window, screen_point: Point, scroll: Scroll) -> DriverResult<()>;
  fn open_frames(&self, window: &Window) -> DriverResult<Box<dyn FrameSource>>;
}

/// A window's frames, newest first and never stale.
pub(crate) trait FrameSource: Send {
  fn read(&mut self) -> Result<FrameUpdate, StreamError>;
  fn frames_delivered(&self) -> u64;
}

/// What identifies a window beyond its handle, which Windows reuses once a window is gone.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Identity {
  process_id: u32,
  thread_id: u32,
  class_name: String,
}

struct Target {
  id: TargetId,
  hwnd: isize,
  identity: Identity,
  /// The window as last seen. Its frame is kept live.
  window: Window,
  state: TargetState,
  frames: Option<Box<dyn FrameSource>>,
}

pub struct BackgroundSession {
  backend: Box<dyn Backend>,
  targets: Vec<Target>,
  next_id: u32,
  stats: SessionStats,
  /// Frames delivered to streams that have been closed since, so the total only ever grows.
  retired_frames: u64,
}

impl BackgroundSession {
  #[cfg(target_os = "windows")]
  pub(crate) fn open() -> DriverResult<Self> {
    Ok(Self::with_backend(Box::new(win32::Win32)))
  }

  #[cfg(not(target_os = "windows"))]
  pub(crate) fn open() -> DriverResult<Self> {
    Err(auv_driver_common::error::DriverError::unsupported("windows background session"))
  }

  // Only Windows opens a real backend; elsewhere the tests' fake is the one caller.
  #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
  pub(crate) fn with_backend(backend: Box<dyn Backend>) -> Self {
    Self {
      backend,
      targets: Vec::new(),
      next_id: 0,
      stats: SessionStats::default(),
      retired_frames: 0,
    }
  }

  fn hwnd_of(window: &Window) -> Result<isize, AttachError> {
    window.reference.id.parse().map_err(|_| AttachError::NotAWindowHandle(window.reference.id.clone()))
  }

  /// Makes `window`, which the caller discovered once, a target. Nothing is sent to it.
  pub fn attach(&mut self, window: &Window) -> Result<TargetId, AttachError> {
    let hwnd = Self::hwnd_of(window)?;
    if let Some(existing) = self.targets.iter().find(|target| target.hwnd == hwnd) {
      return Err(AttachError::AlreadyAttached(existing.id));
    }
    if self.targets.len() >= MAX_TARGETS {
      return Err(AttachError::Full);
    }
    self.stats.probes += 1;
    let probe = self.backend.probe(hwnd);
    if !probe.exists || window.process_id.is_some_and(|expected| expected != probe.process_id) {
      return Err(AttachError::Gone);
    }
    let id = TargetId(self.next_id);
    self.next_id += 1;
    let mut window = window.clone();
    if !probe.minimized
      && let Some(frame) = probe.frame
    {
      window.frame = frame;
    }
    self.targets.push(Target {
      id,
      hwnd,
      identity: Identity {
        process_id: probe.process_id,
        thread_id: probe.thread_id,
        class_name: probe.class_name,
      },
      window,
      state: if probe.minimized {
        TargetState::Minimized
      } else {
        TargetState::Live
      },
      frames: None,
    });
    self.stats.attached += 1;
    Ok(id)
  }

  /// Stops targeting a window. Its capture stream is closed; the window is untouched.
  pub fn detach(&mut self, id: TargetId) -> bool {
    let Some(index) = self.index_of(id) else {
      return false;
    };
    self.retire_frames(index);
    self.targets.remove(index);
    true
  }

  /// Closes a target's frame stream, which releases its capture session, and keeps the count of
  /// frames it delivered.
  fn retire_frames(&mut self, index: usize) {
    if let Some(frames) = self.targets[index].frames.take() {
      self.retired_frames += frames.frames_delivered();
    }
  }

  fn index_of(&self, id: TargetId) -> Option<usize> {
    self.targets.iter().position(|target| target.id == id)
  }

  /// Revalidates one target against the live window and returns how its frame changed, if it did.
  fn update(&mut self, index: usize) -> Option<FrameChange> {
    if matches!(self.targets[index].state, TargetState::Lost { .. }) {
      return None;
    }
    self.stats.probes += 1;
    let hwnd = self.targets[index].hwnd;
    let probe = self.backend.probe(hwnd);
    let target = &mut self.targets[index];
    let lost = if !probe.exists {
      Some(LostReason::WindowDestroyed)
    } else if probe.process_id != target.identity.process_id {
      Some(LostReason::ProcessChanged)
    } else if probe.thread_id != target.identity.thread_id || probe.class_name != target.identity.class_name {
      Some(LostReason::HandleReused)
    } else {
      None
    };
    if let Some(reason) = lost {
      target.state = TargetState::Lost { reason };
      // Closing the stream of a window that is gone releases its capture session.
      self.retire_frames(index);
      self.stats.lost += 1;
      return None;
    }
    // TODO(windows-background-cloaked): a window DWM cloaks (another virtual desktop) is treated
    // as live. Posting still reaches it and capture may stall; a typed state for it needs live
    // evidence first. Reopen with a validated virtual-desktop run.
    if probe.minimized {
      target.state = TargetState::Minimized;
      return None;
    }
    target.state = TargetState::Live;
    let frame = probe.frame?;
    if frame == target.window.frame {
      return None;
    }
    let change = FrameChange {
      from: target.window.frame,
      to: frame,
    };
    target.window.frame = frame;
    Some(change)
  }

  /// Checks that a target can be operated on. `Err` is the answer to give instead.
  fn gate<T>(&mut self, id: TargetId, operation: &'static str) -> Result<(usize, Option<FrameChange>), Outcome<T>> {
    let Some(index) = self.index_of(id) else {
      return Err(Outcome::Failed {
        operation,
        message: format!("{id} is not a target of this session"),
      });
    };
    let followed = self.update(index);
    match self.targets[index].state {
      TargetState::Live => Ok((index, followed)),
      TargetState::Minimized => {
        self.stats.skipped_minimized += 1;
        Err(Outcome::SkippedMinimized)
      }
      TargetState::Lost { reason } => Err(Outcome::TargetLost { reason }),
    }
  }

  /// Revalidates every target and reports their state. The way to notice a closed or moved window
  /// without operating on it.
  pub fn refresh(&mut self) -> Vec<(TargetInfo, Option<FrameChange>)> {
    (0..self.targets.len())
      .map(|index| {
        let change = self.update(index);
        (self.info(index), change)
      })
      .collect()
  }

  fn info(&self, index: usize) -> TargetInfo {
    let target = &self.targets[index];
    TargetInfo {
      id: target.id,
      window: target.window.reference.id.clone(),
      process_id: target.identity.process_id,
      state: target.state,
      frame: target.window.frame,
    }
  }

  pub fn targets(&self) -> Vec<TargetInfo> {
    (0..self.targets.len()).map(|index| self.info(index)).collect()
  }

  pub fn stats(&self) -> SessionStats {
    let mut stats = self.stats.clone();
    stats.frames_delivered = self.retired_frames
      + self.targets.iter().filter_map(|target| target.frames.as_ref()).map(|frames| frames.frames_delivered()).sum::<u64>();
    stats
  }

  // TODO(background-session-foreground-grant): the result reports no focus disturbance without
  // observing it. Windows can still let a recently launched process take the foreground when a
  // posted click first activates its window; it happened with and without user input since the
  // launch and could not be predicted (see
  // `docs/ai/references/driver/2026-10-10-windows-background-session.md`). Reporting that needs a
  // foreground observation around each operation in the typed result; it waits for an owner
  // decision on that report.
  fn delivery(&self, index: usize, path: InputDeliveryPath, followed: Option<FrameChange>) -> Delivery {
    let target = &self.targets[index];
    Delivery {
      target: target.id,
      result: InputActionResult::single_success(path),
      frame: target.window.frame,
      followed,
    }
  }

  fn screen_point(&self, index: usize, point: WindowPoint) -> Point {
    let frame = self.targets[index].window.frame;
    Point::new(frame.origin.x + point.point().x, frame.origin.y + point.point().y)
  }

  /// Posts a click at `point`, a position in the target's own frame, to the control under it.
  pub fn click(&mut self, id: TargetId, point: WindowPoint, request: ClickRequest) -> Outcome<Delivery> {
    let (index, followed) = match self.gate(id, "click") {
      Ok(found) => found,
      Err(answer) => return answer,
    };
    if let Err(error) = background_input::validate_modifiers(request.modifiers) {
      self.stats.unsupported += 1;
      return Outcome::Unsupported {
        operation: "click",
        reason: error.to_string(),
      };
    }
    let screen = self.screen_point(index, point);
    match self.backend.post_click(&self.targets[index].window, screen, &request) {
      Ok(()) => {
        self.stats.clicks += 1;
        Outcome::Done {
          value: self.delivery(index, InputDeliveryPath::WindowTargetedMouse, followed),
        }
      }
      Err(error) => Outcome::Failed {
        operation: "click",
        message: error.to_string(),
      },
    }
  }

  /// Posts a wheel scroll at `point`, a position in the target's own frame, to the control under it.
  pub fn scroll(&mut self, id: TargetId, point: WindowPoint, scroll: Scroll) -> Outcome<Delivery> {
    let (index, followed) = match self.gate(id, "scroll") {
      Ok(found) => found,
      Err(answer) => return answer,
    };
    let screen = self.screen_point(index, point);
    match self.backend.post_scroll(&self.targets[index].window, screen, scroll) {
      Ok(()) => {
        self.stats.scrolls += 1;
        Outcome::Done {
          value: self.delivery(index, InputDeliveryPath::WindowTargetedWheel, followed),
        }
      }
      Err(error) => Outcome::Failed {
        operation: "scroll",
        message: error.to_string(),
      },
    }
  }

  // TODO(windows-targeted-keyboard): background keyboard delivery is deferred until an owner-approved
  // slice defines how a recipient is leased and revalidated per action (see `InputApi::input_keyboard`).
  // Trigger: that contract. Until then a session answers keyboard requests as unsupported rather
  // than typing into whatever has the foreground, which is what the global path would do.
  fn keyboard_unsupported<T>(&mut self, id: TargetId, operation: &'static str) -> Outcome<T> {
    if self.index_of(id).is_none() {
      return Outcome::Failed {
        operation,
        message: format!("{id} is not a target of this session"),
      };
    }
    self.stats.unsupported += 1;
    Outcome::Unsupported {
      operation,
      reason: "Windows has no background keyboard delivery; the session does not send keys to the foreground window instead".into(),
    }
  }

  /// Types text into a target. Always [`Outcome::Unsupported`] today.
  pub fn type_text(&mut self, id: TargetId, _text: &str) -> Outcome<Delivery> {
    self.keyboard_unsupported(id, "type_text")
  }

  /// Presses keys in a target. Always [`Outcome::Unsupported`] today.
  pub fn press_keys(&mut self, id: TargetId, _keys: &[&str]) -> Outcome<Delivery> {
    self.keyboard_unsupported(id, "press_keys")
  }

  /// The newest frame of the target, if it changed since the previous capture of it.
  ///
  /// TODO(background-session-wait-for-frame): this answers at once (`Unchanged` costs one handle
  /// probe and copies nothing), so a caller that wants the first frame after an action polls it.
  /// A blocking wait on the stream's frame counter is deferred until a caller needs one; the
  /// evaluation harness polls with a short sleep (`POLL_INTERVAL` in
  /// `evals/auv-base/platforms/desktop-windows/tasks/sustained.rs`).
  pub fn capture(&mut self, id: TargetId) -> Outcome<FrameRead> {
    let (index, followed) = match self.gate(id, "capture") {
      Ok(found) => found,
      Err(answer) => return answer,
    };
    // Why a stream is opened in this call, for the counters.
    enum Opening {
      First,
      AfterFault,
      AfterResize,
    }
    let mut opening = Opening::First;
    let (mut faulted, mut resizes) = (false, 0);
    loop {
      if self.targets[index].frames.is_none() {
        match self.backend.open_frames(&self.targets[index].window) {
          Ok(frames) => {
            self.targets[index].frames = Some(frames);
            match opening {
              Opening::First => self.stats.streams_opened += 1,
              Opening::AfterFault => self.stats.streams_reopened += 1,
              Opening::AfterResize => self.stats.streams_resized += 1,
            }
          }
          Err(error) => {
            return Outcome::Failed {
              operation: "capture",
              message: error.to_string(),
            };
          }
        }
      }
      let read = self.targets[index].frames.as_mut().expect("frames were just opened").read();
      match read {
        Ok(FrameUpdate::Fresh {
          image,
          frame_number,
        }) => {
          self.stats.frames_fresh += 1;
          let target = &self.targets[index];
          let frame = target.window.frame;
          let scale_factor = if frame.size.width > 0.0 {
            f64::from(image.width()) / frame.size.width
          } else {
            1.0
          };
          return Outcome::Done {
            value: FrameRead::Fresh {
              capture: Capture {
                origin: Some(Position::in_window(&target.window.reference, WindowPoint::new(0.0, 0.0))),
                image,
                bounds: frame,
                scale_factor,
                backend: crate::wgc::WGC_BACKEND.to_string(),
                fallback_reason: None,
              },
              frame_number,
              followed,
            },
          };
        }
        Ok(FrameUpdate::Unchanged { frame_number }) => {
          self.stats.frames_unchanged += 1;
          return Outcome::Done {
            value: FrameRead::Unchanged { frame_number },
          };
        }
        Err(StreamError::Closed) => {
          self.retire_frames(index);
          self.targets[index].state = TargetState::Lost {
            reason: LostReason::WindowDestroyed,
          };
          self.stats.lost += 1;
          return Outcome::TargetLost {
            reason: LostReason::WindowDestroyed,
          };
        }
        // The window's capture region changed size, and the stream's frames no longer line up
        // with the window's frame: a new stream does. A window still changing size after a few
        // replacements is reported rather than chased.
        Err(StreamError::Resized) => {
          self.retire_frames(index);
          resizes += 1;
          if resizes > 3 {
            return Outcome::Failed {
              operation: "capture",
              message: "the window kept changing size while its capture was replaced".into(),
            };
          }
          opening = Opening::AfterResize;
        }
        // A stream that failed or never delivered is reopened once; a second failure is reported.
        Err(error) => {
          self.retire_frames(index);
          if faulted {
            return Outcome::Failed {
              operation: "capture",
              message: match error {
                StreamError::Failed(message) => message,
                other => format!("{other:?}"),
              },
            };
          }
          faulted = true;
          opening = Opening::AfterFault;
        }
      }
    }
  }
}

#[cfg(target_os = "windows")]
mod win32 {
  use super::*;
  use crate::wgc::FrameStream;
  use crate::window::{probe_window, window_handle};

  /// The real OS boundary: handle probes, posted window messages and Windows.Graphics.Capture.
  pub(super) struct Win32;

  struct WgcFrames(FrameStream);

  impl FrameSource for WgcFrames {
    fn read(&mut self) -> Result<FrameUpdate, StreamError> {
      self.0.read(FIRST_FRAME_WAIT)
    }

    fn frames_delivered(&self) -> u64 {
      self.0.frames_delivered()
    }
  }

  impl Backend for Win32 {
    fn probe(&self, hwnd: isize) -> WindowProbe {
      probe_window(hwnd)
    }

    fn post_click(&self, window: &Window, screen_point: Point, request: &ClickRequest) -> DriverResult<()> {
      background_input::click_at_window(window, screen_point, request.button, request.click.clone(), request.modifiers)
    }

    fn post_scroll(&self, window: &Window, screen_point: Point, scroll: Scroll) -> DriverResult<()> {
      background_input::scroll_at_window(window, screen_point, scroll)
    }

    fn open_frames(&self, window: &Window) -> DriverResult<Box<dyn FrameSource>> {
      Ok(Box::new(WgcFrames(FrameStream::open(window_handle(window)?)?)))
    }
  }
}

#[cfg(test)]
#[path = "background_session_test.rs"]
mod tests;
