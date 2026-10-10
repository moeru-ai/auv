//! Live overlay animator: turns a stream of reported actions into smooth 60 fps frames.
//!
//! The renderer stays one-shot: every frame is a complete [`crate::window::present`] of
//! the layers [`MotionScene`] composes for that instant. This module only owns the
//! thread, the clock and the decisions about when to draw (see `pacing.rs`).
//!
//! # Faithfulness
//!
//! The animator draws only what [`ActionEvent`]s describe. It never reads the pointer,
//! never synthesizes input and never moves toward a point nobody reported. The overlay
//! is a visual trust adapter, not an input backend.
//!
//! NOTICE: while an animator runs, it must be the only presenter in its process. The
//! overlay window belongs to the thread that first presented, and `ShowWindow` from
//! another thread waits for that thread's message queue. Mixing one-shot `render` calls
//! from other threads with a running animator can therefore stall.
//! TODO(driver-overlay-windows-window-owner-thread): give each presenter its own window,
//! or route every presenter through one owner thread, when a consumer needs both at once.

use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use auv_driver_overlay_common::{ActionEvent, FrameStats, ShowOptions};

use crate::AuvResult;

enum Message {
  Event {
    reported: Instant,
    event: ActionEvent,
  },
  Stop,
}

/// Handle to the animation thread. Dropping it stops the thread and removes the overlay.
pub struct Animator {
  sender: Sender<Message>,
  worker: Option<JoinHandle<FrameStats>>,
}

impl Animator {
  /// Starts the animation thread and returns once it has warmed up, so the first reported
  /// action is never delayed by first-use initialization. `options.motion()` sets how the
  /// cursor eases between jumps; `options.lifecycle()` sets whether the overlay removes
  /// itself after the scene has been still for a while (`Removal::Manual` keeps it until
  /// [`Animator::stop`]).
  #[cfg(target_os = "windows")]
  pub fn start(options: ShowOptions) -> AuvResult<Self> {
    /// Warm-up measured 40 to 330 ms; this bound only stops a wedged window system from
    /// blocking the caller forever.
    const WARM_UP_TIMEOUT: Duration = Duration::from_secs(10);

    let (sender, receiver) = mpsc::channel();
    let (ready_sender, ready) = mpsc::channel();
    let epoch = Instant::now();
    let worker = std::thread::Builder::new()
      .name("auv-overlay-animator".into())
      .spawn(move || native::Worker::new(receiver, options, epoch).run(ready_sender))
      .map_err(|error| format!("failed to start the overlay animator thread: {error}"))?;
    let animator = Self {
      sender,
      worker: Some(worker),
    };
    ready.recv_timeout(WARM_UP_TIMEOUT).map_err(|_| "the overlay animator did not finish warming up".to_string())?;
    Ok(animator)
  }

  #[cfg(not(target_os = "windows"))]
  pub fn start(_options: ShowOptions) -> AuvResult<Self> {
    Err("windows overlay animator is unsupported on this target".to_string())
  }

  /// Reports one real action. Returns immediately; the event is stamped now so the
  /// measured latency covers the whole path to the screen.
  pub fn report(&self, event: ActionEvent) -> AuvResult<()> {
    self
      .sender
      .send(Message::Event {
        reported: Instant::now(),
        event,
      })
      .map_err(|_| "the overlay animator has stopped".to_string())
  }

  /// Stops the thread, removes the overlay and returns what the run measured.
  pub fn stop(mut self) -> AuvResult<FrameStats> {
    self.shutdown()
  }

  fn shutdown(&mut self) -> AuvResult<FrameStats> {
    let Some(worker) = self.worker.take() else {
      return Ok(FrameStats::default());
    };
    let _ = self.sender.send(Message::Stop);
    worker.join().map_err(|_| "the overlay animator thread panicked".to_string())
  }
}

impl Drop for Animator {
  fn drop(&mut self) {
    let _ = self.shutdown();
  }
}

#[cfg(target_os = "windows")]
mod native {
  use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
  use std::time::{Duration, Instant};

  use auv_driver_common::ScreenPoint;
  use auv_driver_overlay_common::layers::{Cursor, Status};
  use auv_driver_overlay_common::{ActionEvent, FrameStats, Layer, MotionScene, Overlay, Removal, ShowOptions, Wake};
  use windows::Win32::Media::{timeBeginPeriod, timeEndPeriod};
  use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage};

  use super::Message;
  use crate::pacing::{FRAME_INTERVAL, Pacer, Step};
  use crate::stats::Samples;

  /// Longest the thread blocks without pumping its message queue.
  const MAX_BLOCK: Duration = Duration::from_millis(250);

  /// Raises the system timer resolution to 1 ms for the animator's lifetime. Without it a
  /// 16.7 ms wait rounds to the default 15.6 ms tick and frames arrive unevenly.
  struct TimerResolution;

  impl TimerResolution {
    fn raise() -> Self {
      unsafe {
        timeBeginPeriod(1);
      }
      Self
    }
  }

  impl Drop for TimerResolution {
    fn drop(&mut self) {
      unsafe {
        timeEndPeriod(1);
      }
    }
  }

  /// Pays the first-use cost of Direct2D, DirectWrite (font and glyph caches), the SVG
  /// rasterizer and the overlay window before the first action arrives. Measured on the
  /// #306 evidence machine, an unwarmed first frame took 40 to 330 ms against 11 ms for the
  /// rest, which would show as the first cursor appearing late and stuttering.
  ///
  /// The layers sit far outside the virtual screen and the window is transparent, so
  /// nothing visible is drawn.
  fn warm_up() {
    let outside = ScreenPoint::new(-1.0e5, -1.0e5);
    let layers = [
      Layer::Cursor(Cursor::new(outside).with_label("auv").with_label_visible()),
      Layer::Status(Status::new(outside, "auv")),
    ];
    let _ = crate::window::present(&layers);
    let _ = crate::window::hide_all();
  }

  /// The overlay window belongs to this thread, so it must keep pumping messages.
  fn pump_messages() {
    unsafe {
      let mut message = MSG::default();
      while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
        let _ = TranslateMessage(&message);
        DispatchMessageW(&message);
      }
    }
  }

  pub(super) struct Worker {
    receiver: Receiver<Message>,
    epoch: Instant,
    scene: MotionScene,
    pacer: Pacer,
    last_overlay: Option<Overlay>,
    /// Report times of events that the next frame will show.
    unshown: Vec<Instant>,
    previous_frame: Option<(Instant, bool)>,
    frames: u64,
    late_frames: u64,
    present_failures: u64,
    last_failure: Option<String>,
    frame_ms: Samples,
    event_latency_ms: Samples,
  }

  impl Worker {
    pub(super) fn new(receiver: Receiver<Message>, options: ShowOptions, epoch: Instant) -> Self {
      let idle_removal = match options.lifecycle().removal() {
        Removal::Manual => None,
        Removal::AutoAfter(after) => Some(after),
      };
      Self {
        receiver,
        epoch,
        scene: MotionScene::new(options.motion()),
        pacer: Pacer::new(FRAME_INTERVAL, idle_removal),
        last_overlay: None,
        unshown: Vec::new(),
        previous_frame: None,
        frames: 0,
        late_frames: 0,
        present_failures: 0,
        last_failure: None,
        frame_ms: Samples::default(),
        event_latency_ms: Samples::default(),
      }
    }

    pub(super) fn run(mut self, ready: Sender<()>) -> FrameStats {
      let _timer = TimerResolution::raise();
      warm_up();
      let _ = ready.send(());
      loop {
        pump_messages();
        let now = Instant::now();
        let wait = match self.pacer.step(now) {
          Step::Render => {
            self.draw(now);
            continue;
          }
          Step::Remove => {
            self.remove();
            continue;
          }
          Step::WaitUntil(until) => until.saturating_duration_since(now).min(MAX_BLOCK),
          Step::WaitForEvent => MAX_BLOCK,
        };
        match self.receiver.recv_timeout(wait) {
          Ok(message) => {
            if !self.handle(message) {
              break;
            }
            // Take everything already queued so one frame shows all of it.
            while let Ok(message) = self.receiver.try_recv() {
              if !self.handle(message) {
                self.remove();
                return self.finish();
              }
            }
          }
          Err(RecvTimeoutError::Timeout) => {}
          Err(RecvTimeoutError::Disconnected) => break,
        }
      }
      self.remove();
      self.finish()
    }

    /// Returns false when the animator was asked to stop.
    fn handle(&mut self, message: Message) -> bool {
      match message {
        Message::Event { reported, event } => {
          self.apply(reported, event);
          true
        }
        Message::Stop => false,
      }
    }

    fn apply(&mut self, reported: Instant, event: ActionEvent) {
      self.scene.apply(event, reported.saturating_duration_since(self.epoch));
      self.unshown.push(reported);
      self.pacer.event();
    }

    fn draw(&mut self, started: Instant) {
      let frame = self.scene.frame(started.saturating_duration_since(self.epoch));
      self.pacer.drew(started, frame.wake);

      // Only a scene that asks for the very next frame can be late; one waiting on a mark
      // timer is still, and its next frame is legitimately far away.
      let animating = frame.wake == Wake::NextFrame;
      if let Some((previous, was_animating)) = self.previous_frame
        && was_animating
        && started.saturating_duration_since(previous) > FRAME_INTERVAL.mul_f64(1.5)
      {
        self.late_frames += 1;
      }
      self.previous_frame = Some((started, animating));

      // A scene that did not change needs no new frame; drawing it again would only
      // spend the frame budget.
      if self.last_overlay.as_ref() == Some(&frame.overlay) {
        self.unshown.clear();
        return;
      }
      match crate::window::present(frame.overlay.layers()) {
        Ok(()) => {
          let done = Instant::now();
          self.frames += 1;
          self.frame_ms.push(done.saturating_duration_since(started).as_secs_f64() * 1000.0);
          for reported in self.unshown.drain(..) {
            self.event_latency_ms.push(done.saturating_duration_since(reported).as_secs_f64() * 1000.0);
          }
          self.last_overlay = Some(frame.overlay);
        }
        Err(error) => {
          self.present_failures += 1;
          self.last_failure = Some(error);
          self.unshown.clear();
          self.last_overlay = None;
        }
      }
    }

    fn remove(&mut self) {
      self.scene.clear();
      self.last_overlay = None;
      self.previous_frame = None;
      self.unshown.clear();
      self.pacer.removed();
      let _ = crate::window::hide_all();
    }

    fn finish(self) -> FrameStats {
      FrameStats {
        frames: self.frames,
        late_frames: self.late_frames,
        present_failures: self.present_failures,
        last_failure: self.last_failure,
        frame_ms: self.frame_ms.percentiles(),
        event_latency_ms: self.event_latency_ms.percentiles(),
      }
    }
  }
}
