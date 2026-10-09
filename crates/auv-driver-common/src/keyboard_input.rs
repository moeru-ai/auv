//! Bounded ownership of a held keyboard combination.
//!
//! A hold retains its original delivery route until every key is released.
//! The controller owns one held combination per local driver process; other
//! processes and physical keyboard input are outside this guarantee.
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::{DriverError, DriverResult, InputActionResult, InputDeliveryPath};

pub type KeyboardHoldId = u64;

// NOTICE: Bound idempotence memory while retaining enough recent IDs for
// delayed/retried Runner responses. Increase only with a measured retry need.
const RELEASED_ID_HISTORY: usize = 64;

/// A platform adapter with keys already validated and resolved to native codes.
pub trait KeyboardBackend: Send + Sync + 'static {
  fn key_count(&self) -> usize;
  fn key(&self, index: usize, down: bool) -> DriverResult<()>;
  fn result(&self) -> InputActionResult;
}

struct Held {
  id: KeyboardHoldId,
  backend: Arc<dyn KeyboardBackend>,
  deadline: Instant,
  uncertain: bool,
  cancellation: Option<Arc<crate::input_cancellation::InputCancellation>>,
}

#[derive(Default)]
struct State {
  next_id: KeyboardHoldId,
  released_ids: VecDeque<KeyboardHoldId>,
  held: Option<Held>,
  posting: bool,
  releasing: bool,
}

#[derive(Default)]
pub struct KeyboardHoldController {
  state: Mutex<State>,
  changed: Condvar,
}

/// Return the process-wide keyboard hold controller.
///
/// NOTICE: The static retains a strong Arc and is not dropped at process exit.
/// Controller Drop therefore cannot release outstanding holds on exit; owners
/// that transfer holds with `KeyboardHold::into_id` must call `shutdown` while
/// the input backend is still available.
pub fn keyboard_hold_controller() -> &'static Arc<KeyboardHoldController> {
  static CONTROLLER: OnceLock<Arc<KeyboardHoldController>> = OnceLock::new();
  CONTROLLER.get_or_init(|| Arc::new(KeyboardHoldController::default()))
}

impl KeyboardHoldController {
  /// Post a bounded down transition. The returned ID remains valid after a
  /// failed release so callers can retry without selecting a new route.
  pub fn down(self: &Arc<Self>, backend: Arc<dyn KeyboardBackend>, timeout: Duration) -> DriverResult<KeyboardHold> {
    if timeout.is_zero() || Instant::now().checked_add(timeout).is_none() {
      return Err(invalid("keyboard hold timeout must be positive and representable"));
    }
    if backend.key_count() == 0 {
      return Err(invalid("keys must not be empty"));
    }
    let deadline = Instant::now() + timeout;
    let id = {
      let mut state = self.state.lock().unwrap();
      if state.held.as_ref().is_some_and(|held| held.uncertain) {
        return Err(invalid("keyboard release is uncertain; explicitly release before reusing this desktop"));
      }
      if state.held.is_some() || state.releasing {
        return Err(invalid("another keyboard combination is held; release it first"));
      }
      let id = state.next_id.checked_add(1).ok_or_else(|| invalid("keyboard hold IDs exhausted"))?;
      state.next_id = id;
      // Reserve before delivery: a failed native reply may follow a key-down.
      state.held = Some(Held {
        id,
        backend: backend.clone(),
        deadline,
        uncertain: false,
        cancellation: crate::input_cancellation::current_input_cancellation(),
      });
      state.posting = true;
      id
    };
    let mut posting_error = None;
    for index in 0..backend.key_count() {
      if let Err(error) = backend.key(index, true) {
        posting_error = Some(error);
        break;
      }
    }
    {
      let mut state = self.state.lock().unwrap();
      state.posting = false;
      if posting_error.is_none() {
        match Instant::now().checked_add(timeout) {
          Some(deadline) => state.held.as_mut().unwrap().deadline = deadline,
          None => posting_error = Some(invalid("keyboard hold timeout exceeds the platform clock range")),
        }
      }
      self.changed.notify_all();
    }
    if let Some(error) = posting_error {
      let cleanup = self.up(id);
      return Err(combine(error, cleanup.err()));
    }
    let controller = self.clone();
    std::thread::spawn(move || {
      let mut state = controller.state.lock().unwrap();
      loop {
        let Some(held) = state.held.as_ref().filter(|held| held.id == id) else {
          return;
        };
        let remaining = held.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() || held.cancellation.as_ref().is_some_and(|flag| flag.is_cancelled()) {
          drop(state);
          let _ = controller.up(id);
          return;
        }
        state = controller.changed.wait_timeout(state, remaining.min(Duration::from_millis(10))).unwrap().0;
      }
    });
    Ok(KeyboardHold {
      id: Some(id),
      controller: self.clone(),
      down: backend.result(),
    })
  }

  /// Retryable release. A known released ID yields Noop; an unknown ID fails.
  pub fn up(&self, id: KeyboardHoldId) -> DriverResult<InputActionResult> {
    let backend = {
      let mut state = self.state.lock().unwrap();
      while state.posting || state.releasing {
        state = self.changed.wait(state).unwrap();
      }
      // A released ID never matches the active hold, so this check is safe
      // even while a later hold is down.
      if state.released_ids.contains(&id) {
        return Ok(InputActionResult::single_success(InputDeliveryPath::Noop));
      }
      let Some(held) = state.held.as_ref().filter(|held| held.id == id) else {
        return Err(invalid("unknown keyboard hold"));
      };
      let backend = held.backend.clone();
      state.releasing = true;
      backend
    };
    let mut error = None;
    for index in (0..backend.key_count()).rev() {
      if let Err(failure) = backend.key(index, false) {
        error = Some(combine(failure, error));
      }
    }
    let mut state = self.state.lock().unwrap();
    state.releasing = false;
    if error.is_none() {
      state.held = None;
      state.released_ids.push_back(id);
      if state.released_ids.len() > RELEASED_ID_HISTORY {
        state.released_ids.pop_front();
      }
    } else if let Some(held) = state.held.as_mut() {
      held.uncertain = true;
    }
    self.changed.notify_all();
    match error {
      Some(error) => Err(error),
      None => Ok(backend.result()),
    }
  }

  /// Release the active hold during Runner shutdown. A failed release remains
  /// available for explicit recovery while the process is still alive.
  pub fn shutdown(&self) -> DriverResult<()> {
    let id = self.state.lock().unwrap().held.as_ref().map(|held| held.id);
    if let Some(id) = id {
      self.up(id)?;
    }
    Ok(())
  }
}

/// Local Rust ownership. Explicit release reports errors; Drop attempts cleanup.
pub struct KeyboardHold {
  id: Option<KeyboardHoldId>,
  controller: Arc<KeyboardHoldController>,
  down: InputActionResult,
}

impl KeyboardHold {
  pub fn down_result(&self) -> &InputActionResult {
    &self.down
  }

  pub fn release(&mut self) -> DriverResult<InputActionResult> {
    let id = self.id.ok_or_else(|| invalid("keyboard hold already released"))?;
    let result = self.controller.up(id)?;
    self.id = None;
    Ok(result)
  }

  /// Wait for a bounded dwell; cancellation triggers prompt release.
  pub fn wait_and_release(&mut self, duration: Duration) -> DriverResult<InputActionResult> {
    let deadline = Instant::now().checked_add(duration).ok_or_else(|| invalid("keyboard hold duration exceeds the platform clock range"))?;
    let mut state = self.controller.state.lock().unwrap();
    let mut cancelled = false;
    loop {
      if state.held.as_ref().is_none_or(|held| Some(held.id) != self.id) {
        break;
      }
      cancelled = state.held.as_ref().unwrap().cancellation.as_ref().is_some_and(|flag| flag.is_cancelled());
      let remaining = deadline.saturating_duration_since(Instant::now());
      if cancelled || remaining.is_zero() {
        break;
      }
      state = self.controller.changed.wait_timeout(state, remaining.min(Duration::from_millis(10))).unwrap().0;
    }
    drop(state);
    cancelled |= crate::input_cancellation::current_input_cancellation().as_ref().is_some_and(|flag| flag.is_cancelled());
    let release = self.release();
    if cancelled {
      return Err(combine(invalid("keyboard hold cancelled"), release.err()));
    }
    release
  }

  /// Transfer release ownership to a Runner that persists the ID across RPCs.
  ///
  /// This disables this guard's Drop cleanup so the keys stay down after the
  /// current RPC returns. The Runner must release the ID with `key_up` or call
  /// controller `shutdown` when its service ends. Timeout cleanup can only
  /// run while the process is alive.
  pub fn into_id(mut self) -> KeyboardHoldId {
    self.id.take().expect("held keyboard ID")
  }
}

impl Drop for KeyboardHold {
  fn drop(&mut self) {
    if let Some(id) = self.id.take() {
      let _ = self.controller.up(id);
    }
  }
}

fn invalid(message: &str) -> DriverError {
  DriverError::InvalidInput {
    message: message.into(),
  }
}

fn combine(error: DriverError, cleanup: Option<DriverError>) -> DriverError {
  match cleanup {
    Some(cleanup) => DriverError::Backend {
      message: format!("{error}; release also failed: {cleanup}"),
    },
    None => error,
  }
}

#[cfg(test)]
#[path = "keyboard_input_test.rs"]
mod tests;
