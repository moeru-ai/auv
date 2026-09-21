//! Desktop input admission and cross-call button ownership.
//!
//! All logical mice share one conservative desktop resource. Native adapters
//! must not infer independent resources from different windows or processes.
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::{DriverError, DriverResult, InputActionResult, MouseButton, Point};

/// Zero selects the shared mouse, independently of run identity.
pub type MouseId = u64;
const MAX_MICE: usize = 256;
pub const MAX_MOUSE_HOLD: Duration = Duration::from_secs(60);

/// The native boundary preserves the same delivery route until release.
/// Implementations must report uncertain delivery as an error.
pub trait MouseBackend: Send + Sync + 'static {
  fn target(&self) -> crate::InputTarget {
    crate::InputTarget::Foreground
  }
  fn current_position(&self) -> DriverResult<Point> {
    Err(DriverError::unsupported("current mouse position"))
  }
  fn move_to(&self, point: Point, held: Option<MouseButton>) -> DriverResult<InputActionResult>;
  fn button(&self, point: Point, button: MouseButton, down: bool) -> DriverResult<InputActionResult>;
}

struct Held {
  button: MouseButton,
  backend: Arc<dyn MouseBackend>,
  deadline: Instant,
  generation: u64,
  uncertain: bool,
}

#[derive(Default)]
struct MouseState {
  point: Option<Point>,
  held: Option<Held>,
}

struct State {
  mice: HashMap<MouseId, MouseState>,
  next_id: MouseId,
  next_ticket: u64,
  waiting: VecDeque<(u64, MouseId)>,
  active: bool,
  stopping: bool,
  holder: Option<MouseId>,
}

/// One authority per local driver process. Other processes and physical input
/// are outside this guarantee; remote callers must use the same Runner.
pub struct MouseCoordinator {
  state: Mutex<State>,
  changed: Condvar,
}

impl Default for MouseCoordinator {
  fn default() -> Self {
    Self {
      state: Mutex::new(State {
        mice: HashMap::from([
          (0, MouseState::default()),
          (u64::MAX, MouseState::default()),
        ]),
        next_id: 1,
        next_ticket: 0,
        waiting: VecDeque::new(),
        active: false,
        stopping: false,
        holder: None,
      }),
      changed: Condvar::new(),
    }
  }
}

pub fn mouse_coordinator() -> &'static Arc<MouseCoordinator> {
  static COORDINATOR: OnceLock<Arc<MouseCoordinator>> = OnceLock::new();
  COORDINATOR.get_or_init(|| Arc::new(MouseCoordinator::default()))
}

impl MouseCoordinator {
  pub fn create_mouse(&self) -> DriverResult<MouseId> {
    let mut state = self.state.lock().unwrap();
    if state.stopping {
      return Err(invalid("mouse input is shutting down"));
    }
    if state.mice.len() >= MAX_MICE {
      return Err(invalid("too many logical mice; remove unused mice"));
    }
    let id = state.next_id;
    state.next_id += 1;
    state.mice.insert(id, MouseState::default());
    Ok(id)
  }

  /// Admission is FIFO within each mouse. A waiting foreign mouse cannot block
  /// the holder's continuation, which is necessary for its eventual release.
  fn enter(&self, id: MouseId, recovery: bool) -> DriverResult<Admission<'_>> {
    let mut state = self.state.lock().unwrap();
    if !state.mice.contains_key(&id) {
      return Err(invalid("unknown logical mouse"));
    }
    let ticket = state.next_ticket;
    state.next_ticket += 1;
    state.waiting.push_back((ticket, id));
    loop {
      if (input_cancelled() || state.stopping) && !recovery {
        state.waiting.retain(|(item, _)| *item != ticket);
        self.changed.notify_all();
        return Err(invalid("input cancelled while waiting"));
      }
      if !state.mice.contains_key(&id) {
        state.waiting.retain(|(item, _)| *item != ticket);
        self.changed.notify_all();
        return Err(invalid("logical mouse was removed"));
      }
      let eligible = state.waiting.iter().find(|(_, mouse)| state.holder.is_none_or(|holder| holder == *mouse));
      if !state.active && eligible == Some(&(ticket, id)) {
        state.waiting.retain(|(item, _)| *item != ticket);
        if state.mice[&id].held.as_ref().is_some_and(|held| held.uncertain) && !recovery {
          self.changed.notify_all();
          return Err(invalid("mouse release is uncertain; explicitly release before reusing this desktop"));
        }
        state.active = true;
        return Ok(Admission { coordinator: self });
      }
      state = self.changed.wait_timeout(state, Duration::from_millis(20)).unwrap().0;
    }
  }

  pub fn down(
    self: &Arc<Self>,
    id: MouseId,
    point: Point,
    button: MouseButton,
    timeout: Duration,
    backend: Arc<dyn MouseBackend>,
  ) -> DriverResult<InputActionResult> {
    validate_point(point)?;
    if timeout.is_zero() || timeout > MAX_MOUSE_HOLD {
      return Err(invalid("mouse hold timeout must be in (0, 60s]"));
    }
    validate_mouse(id)?;
    let _admission = self.enter(id, false)?;
    self.down_admitted(id, point, button, timeout, backend)
  }

  fn down_admitted(
    self: &Arc<Self>,
    id: MouseId,
    point: Point,
    button: MouseButton,
    timeout: Duration,
    backend: Arc<dyn MouseBackend>,
  ) -> DriverResult<InputActionResult> {
    if self.state.lock().unwrap().mice[&id].held.is_some() {
      // TODO: multi-button chords await an approved native receiver contract.
      return Err(invalid("this mouse already holds a button"));
    }
    backend.move_to(point, None)?;
    let generation = {
      let mut state = self.state.lock().unwrap();
      let generation = state.next_ticket;
      let mouse = state.mice.get_mut(&id).unwrap();
      mouse.point = Some(point);
      mouse.held = Some(Held {
        button,
        backend: backend.clone(),
        deadline: Instant::now() + timeout,
        generation,
        uncertain: false,
      });
      state.holder = Some(id);
      generation
    };
    // Record ownership before posting: a failed native reply may follow delivery.
    let result = backend.button(point, button, true);
    if result.is_err() {
      let cleanup = self.release_admitted(id);
      return combine(result, cleanup);
    }
    let coordinator = self.clone();
    let cancellation = INPUT_CANCELLATION.with(|value| value.borrow().clone());
    std::thread::spawn(move || {
      loop {
        std::thread::sleep(Duration::from_millis(20));
        let due = {
          let state = coordinator.state.lock().unwrap();
          let Some(held) = state.mice.get(&id).and_then(|mouse| mouse.held.as_ref()) else {
            return;
          };
          if held.generation != generation {
            return;
          }
          Instant::now() >= held.deadline || cancellation.as_ref().is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire))
        };
        if due {
          let Ok(_admission) = coordinator.enter(id, true) else {
            return;
          };
          let same_hold = coordinator.state.lock().unwrap().mice[&id].held.as_ref().is_some_and(|held| held.generation == generation);
          if same_hold {
            let _ = coordinator.release_admitted(id);
          }
          return;
        }
      }
    });
    result
  }

  pub fn move_to(&self, id: MouseId, point: Point, backend: Arc<dyn MouseBackend>) -> DriverResult<InputActionResult> {
    validate_point(point)?;
    validate_mouse(id)?;
    let _admission = self.enter(id, false)?;
    self.move_admitted(id, point, backend)
  }

  fn move_admitted(&self, id: MouseId, point: Point, backend: Arc<dyn MouseBackend>) -> DriverResult<InputActionResult> {
    let (backend, button) = {
      let state = self.state.lock().unwrap();
      let mouse = &state.mice[&id];
      match &mouse.held {
        Some(held) => (held.backend.clone(), Some(held.button)),
        None => (backend, None),
      }
    };
    let result = backend.move_to(point, button);
    if result.is_ok() {
      self.state.lock().unwrap().mice.get_mut(&id).unwrap().point = Some(point);
    }
    if result.is_err() && button.is_some() {
      return combine(result, self.release_admitted(id));
    }
    result
  }

  /// Executes a whole curve under one admission; an optional button composes a
  /// complete drag from the same press/move/release primitives.
  pub fn motion(
    self: &Arc<Self>,
    request: crate::MoveMouseRequest,
    button: Option<MouseButton>,
    backend: Arc<dyn MouseBackend>,
    mut notify: impl FnMut(MotionEvent) -> bool,
  ) -> DriverResult<(Point, InputActionResult)> {
    let id = request.mouse;
    validate_mouse(id)?;
    let _admission = self.enter(id, false)?;
    if let Some(target) = &request.target {
      let state = self.state.lock().unwrap();
      if let Some(held) = &state.mice[&id].held {
        if !same_target(target, &held.backend.target()) {
          return Err(invalid("cannot change the mouse target while a button is held"));
        }
      }
    }
    let start = match request.start {
      crate::MouseStart::Screen(point) => point,
      crate::MouseStart::Current => self.position(id)?.map(Ok).unwrap_or_else(|| backend.current_position())?,
    };
    let samples = request.samples(start)?;
    if !notify(MotionEvent::Started {
      point: start,
      samples: samples.len(),
      duration: request.options.duration,
    }) {
      return combine(Err(invalid("mouse movement cancelled before delivery")), self.release_admitted(id)).map(|action| (start, action));
    }
    if let Some(button) = button {
      self.down_admitted(id, start, button, MAX_MOUSE_HOLD, backend.clone())?;
    }
    let result = (|| {
      let started = Instant::now();
      let mut result = InputActionResult::single_success(crate::InputDeliveryPath::Noop);
      let mut next_index = 0;
      while next_index < samples.len() {
        let index = latest_due_mouse_sample(&samples, next_index, started.elapsed());
        let sample = &samples[index];
        self.wait_until(started + sample.elapsed, "mouse movement cancelled")?;
        let expired = self.state.lock().unwrap().mice[&id].held.as_ref().is_some_and(|held| Instant::now() >= held.deadline);
        if expired && button.is_none() {
          return Err(invalid("held mouse deadline expired"));
        }
        result = self.move_admitted(id, sample.point, backend.clone())?;
        if !notify(MotionEvent::Progress {
          index,
          sample: *sample,
        }) {
          return Err(invalid("mouse movement cancelled"));
        }
        next_index = index + 1;
      }
      Ok(result)
    })();
    let result = if button.is_some() || result.is_err() {
      combine(result, self.release_admitted(id))
    } else {
      result
    };
    Ok((samples.last().unwrap().point, result?))
  }

  /// A bounded hold is the primitive press/release lifecycle under one admission.
  pub fn hold(
    self: &Arc<Self>,
    id: MouseId,
    point: Point,
    button: MouseButton,
    duration: Duration,
    backend: Arc<dyn MouseBackend>,
  ) -> DriverResult<InputActionResult> {
    validate_point(point)?;
    if duration.is_zero() || duration > MAX_MOUSE_HOLD {
      return Err(invalid("hold duration must be in (0, 60s]"));
    }
    validate_mouse(id)?;
    let _admission = self.enter(id, false)?;
    self.down_admitted(id, point, button, duration, backend)?;
    let wait = self.wait_until(Instant::now() + duration, "mouse hold cancelled");
    let release = self.release_admitted(id);
    match wait {
      Ok(()) => release,
      Err(error) => combine(Err(error), release),
    }
  }

  pub fn up(&self, id: MouseId) -> DriverResult<InputActionResult> {
    validate_mouse(id)?;
    let _admission = self.enter(id, true)?;
    self.release_admitted(id)
  }

  pub fn remove_mouse(&self, id: MouseId) -> DriverResult<InputActionResult> {
    if id == 0 {
      return Err(invalid("the default mouse cannot be removed"));
    }
    validate_mouse(id)?;
    let _admission = self.enter(id, true)?;
    let action = self.release_admitted(id)?;
    self.state.lock().unwrap().mice.remove(&id);
    Ok(action)
  }

  /// Active gestures keep admission while waiting, but must let cancellation
  /// and shutdown reach their release path without waiting for the full delay.
  fn wait_until(&self, deadline: Instant, cancelled: &str) -> DriverResult<()> {
    loop {
      if input_cancelled() || self.state.lock().unwrap().stopping {
        return Err(invalid(cancelled));
      }
      let remaining = deadline.saturating_duration_since(Instant::now());
      if remaining.is_zero() {
        return Ok(());
      }
      std::thread::sleep(remaining.min(Duration::from_millis(10)));
    }
  }

  /// Stops admission, lets active work clean up, then releases any cross-call
  /// hold. Failed cleanup remains represented as uncertain state.
  pub fn shutdown(&self) -> DriverResult<()> {
    let mut state = self.state.lock().unwrap();
    state.stopping = true;
    self.changed.notify_all();
    while state.active {
      state = self.changed.wait(state).unwrap();
    }
    let holder = state.holder;
    state.active = true;
    drop(state);
    let _admission = Admission { coordinator: self };
    if let Some(id) = holder {
      self.release_admitted(id)?;
    }
    Ok(())
  }

  pub fn position(&self, id: MouseId) -> DriverResult<Option<Point>> {
    validate_mouse(id)?;
    self.state.lock().unwrap().mice.get(&id).map(|mouse| mouse.point).ok_or_else(|| invalid("unknown logical mouse"))
  }

  fn release_admitted(&self, id: MouseId) -> DriverResult<InputActionResult> {
    let release = {
      let state = self.state.lock().unwrap();
      let mouse = &state.mice[&id];
      mouse.held.as_ref().map(|held| (held.backend.clone(), mouse.point.unwrap(), held.button))
    };
    let Some((backend, point, button)) = release else {
      return Ok(InputActionResult::single_success(crate::InputDeliveryPath::Noop));
    };
    let result = backend.button(point, button, false);
    let mut state = self.state.lock().unwrap();
    let mouse = state.mice.get_mut(&id).unwrap();
    if result.is_ok() {
      mouse.held = None;
      state.holder = None;
    } else {
      // Uncertainty belongs to the retained hold, never to a released mouse.
      mouse.held.as_mut().unwrap().uncertain = true;
    }
    result
  }
}

struct Admission<'a> {
  coordinator: &'a MouseCoordinator,
}
impl Drop for Admission<'_> {
  fn drop(&mut self) {
    self.coordinator.state.lock().unwrap().active = false;
    self.coordinator.changed.notify_all();
  }
}

fn invalid(message: &str) -> DriverError {
  DriverError::InvalidInput {
    message: message.into(),
  }
}
fn validate_point(point: Point) -> DriverResult<()> {
  if point.x.is_finite() && point.y.is_finite() {
    Ok(())
  } else {
    Err(invalid("mouse coordinates must be finite"))
  }
}
fn combine(action: DriverResult<InputActionResult>, cleanup: DriverResult<InputActionResult>) -> DriverResult<InputActionResult> {
  match (action, cleanup) {
    (result, Ok(_)) => result,
    (Ok(_), Err(error)) => Err(error),
    (Err(action), Err(cleanup)) => Err(DriverError::Backend {
      message: format!("{action}; release also failed: {cleanup}"),
    }),
  }
}

#[cfg(test)]
#[path = "mouse_input_test.rs"]
mod tests;

/// Movement progress is delivery feedback, independent of semantic verification.
pub enum MotionEvent {
  Started {
    point: Point,
    samples: usize,
    duration: Duration,
  },
  Progress {
    index: usize,
    sample: crate::MouseMotionSample,
  },
}

thread_local! { static LEGACY_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

/// Reserves the desktop around an existing complete native input operation.
/// Nested platform calls on the same thread share the outer reservation.
/// This guard is deliberately not Send: nesting belongs to its original thread.
pub struct DesktopInputGuard {
  _admission: Option<Admission<'static>>,
  _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}
pub fn reserve_desktop_input() -> DriverResult<DesktopInputGuard> {
  let nested = LEGACY_DEPTH.with(|depth| depth.get() > 0);
  let admission = if nested {
    None
  } else {
    Some(mouse_coordinator().enter(u64::MAX, false)?)
  };
  LEGACY_DEPTH.with(|depth| depth.set(depth.get() + 1));
  Ok(DesktopInputGuard {
    _admission: admission,
    _thread: std::marker::PhantomData,
  })
}
impl Drop for DesktopInputGuard {
  fn drop(&mut self) {
    LEGACY_DEPTH.with(|depth| depth.set(depth.get() - 1));
  }
}

thread_local! {
  static INPUT_CANCELLATION: std::cell::RefCell<Option<Arc<std::sync::atomic::AtomicBool>>> = const { std::cell::RefCell::new(None) };
}
fn input_cancelled() -> bool {
  INPUT_CANCELLATION.with(|value| value.borrow().as_ref().is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire)))
}

/// Binds cancellation to synchronous native work on a blocking worker. The
/// transport owns the flag; cancellation does not become part of replay input.
pub fn with_input_cancellation<T>(cancelled: Arc<std::sync::atomic::AtomicBool>, action: impl FnOnce() -> T) -> T {
  struct Restore(Option<Arc<std::sync::atomic::AtomicBool>>);
  impl Drop for Restore {
    fn drop(&mut self) {
      INPUT_CANCELLATION.with(|value| *value.borrow_mut() = self.0.take());
    }
  }
  let _restore = Restore(INPUT_CANCELLATION.with(|value| value.replace(Some(cancelled))));
  action()
}

fn latest_due_mouse_sample(samples: &[crate::MouseMotionSample], next_index: usize, elapsed: std::time::Duration) -> usize {
  let mut index = next_index;
  while index + 1 < samples.len() && samples[index + 1].elapsed <= elapsed {
    index += 1;
  }
  index
}

fn same_target(a: &crate::InputTarget, b: &crate::InputTarget) -> bool {
  match (a, b) {
    (crate::InputTarget::Foreground, crate::InputTarget::Foreground) => true,
    (crate::InputTarget::Window(a), crate::InputTarget::Window(b)) => a.reference == b.reference && a.process_id == b.process_id,
    _ => false,
  }
}

fn validate_mouse(id: MouseId) -> DriverResult<()> {
  if id == u64::MAX {
    Err(invalid("reserved mouse identity"))
  } else {
    Ok(())
  }
}
