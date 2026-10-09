use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

struct FakeBackend {
  events: Mutex<Vec<(usize, bool)>>,
  fail_release_once: AtomicBool,
  keys: usize,
}

impl FakeBackend {
  fn new(keys: usize) -> Arc<Self> {
    Arc::new(Self {
      events: Mutex::new(Vec::new()),
      fail_release_once: AtomicBool::new(false),
      keys,
    })
  }
}

impl KeyboardBackend for FakeBackend {
  fn key_count(&self) -> usize {
    self.keys
  }
  fn key(&self, index: usize, down: bool) -> DriverResult<()> {
    self.events.lock().unwrap().push((index, down));
    if !down && self.fail_release_once.swap(false, Ordering::SeqCst) {
      return Err(DriverError::Backend {
        message: "injected release failure".into(),
      });
    }
    Ok(())
  }
  fn result(&self) -> InputActionResult {
    InputActionResult::single_success(InputDeliveryPath::ForegroundSystemEvents)
  }
}

#[test]
fn combination_releases_in_reverse_order() {
  let controller = Arc::new(KeyboardHoldController::default());
  let backend = FakeBackend::new(3);
  let id = controller.down(backend.clone(), Duration::from_secs(1)).unwrap().into_id();
  controller.up(id).unwrap();
  assert_eq!(
    *backend.events.lock().unwrap(),
    vec![
      (0, true),
      (1, true),
      (2, true),
      (2, false),
      (1, false),
      (0, false)
    ]
  );
  assert_eq!(controller.up(id).unwrap().selected_path, InputDeliveryPath::Noop);
}

#[test]
fn an_older_released_hold_remains_idempotent_after_a_later_hold() {
  // ROOT CAUSE:
  //
  // The controller remembered only the most recently released ID, even though
  // `up` promises that a known released ID is idempotent. Releasing a later
  // hold therefore made retrying an earlier release fail as unknown.
  let controller = Arc::new(KeyboardHoldController::default());
  let first = controller.down(FakeBackend::new(1), Duration::from_secs(1)).unwrap().into_id();
  controller.up(first).unwrap();
  let second = controller.down(FakeBackend::new(1), Duration::from_secs(1)).unwrap().into_id();
  controller.up(second).unwrap();

  assert_eq!(controller.up(first).unwrap().selected_path, InputDeliveryPath::Noop);
}

#[test]
fn a_released_hold_remains_idempotent_while_a_later_hold_is_active() {
  // ROOT CAUSE:
  //
  // `up` compared the requested ID only against the active hold, so retrying
  // an earlier release while a later hold was down failed as unknown.
  //
  // The fix answers a known released ID with Noop and leaves the active hold
  // untouched.
  let controller = Arc::new(KeyboardHoldController::default());
  let first = controller.down(FakeBackend::new(1), Duration::from_secs(1)).unwrap().into_id();
  controller.up(first).unwrap();
  let backend = FakeBackend::new(1);
  let second = controller.down(backend.clone(), Duration::from_secs(1)).unwrap().into_id();

  assert_eq!(controller.up(first).unwrap().selected_path, InputDeliveryPath::Noop);
  assert_eq!(*backend.events.lock().unwrap(), vec![(0, true)]);
  controller.up(second).unwrap();
  assert_eq!(*backend.events.lock().unwrap(), vec![(0, true), (0, false)]);
}

#[test]
fn failed_release_retains_hold_for_explicit_retry() {
  let controller = Arc::new(KeyboardHoldController::default());
  let backend = FakeBackend::new(1);
  let id = controller.down(backend.clone(), Duration::from_secs(1)).unwrap().into_id();
  backend.fail_release_once.store(true, Ordering::SeqCst);
  assert!(controller.up(id).is_err());
  assert!(controller.down(FakeBackend::new(1), Duration::from_secs(1)).is_err());
  controller.up(id).unwrap();
  assert_eq!(*backend.events.lock().unwrap(), vec![(0, true), (0, false), (0, false)]);
}

#[test]
fn cancellation_releases_without_waiting_for_timeout() {
  let controller = Arc::new(KeyboardHoldController::default());
  let backend = FakeBackend::new(1);
  let flag = Arc::new(crate::input_cancellation::InputCancellation::default());
  let id = crate::input_cancellation::with_input_cancellation(flag.clone(), || controller.down(backend.clone(), Duration::from_secs(5)))
    .unwrap()
    .into_id();
  flag.cancel();
  let deadline = Instant::now() + Duration::from_secs(1);
  let mut state = controller.state.lock().unwrap();
  while state.held.is_some() && Instant::now() < deadline {
    state = controller.changed.wait_timeout(state, Duration::from_millis(20)).unwrap().0;
  }
  assert!(state.held.is_none());
  drop(state);
  assert_eq!(controller.up(id).unwrap().selected_path, InputDeliveryPath::Noop);
  assert_eq!(*backend.events.lock().unwrap(), vec![(0, true), (0, false)]);
}

#[test]
fn deadline_releases_an_abandoned_hold() {
  let controller = Arc::new(KeyboardHoldController::default());
  let backend = FakeBackend::new(1);
  let id = controller.down(backend.clone(), Duration::from_millis(10)).unwrap().into_id();
  let deadline = Instant::now() + Duration::from_secs(1);
  let mut state = controller.state.lock().unwrap();
  while state.held.is_some() && Instant::now() < deadline {
    state = controller.changed.wait_timeout(state, Duration::from_millis(20)).unwrap().0;
  }
  assert!(state.held.is_none());
  drop(state);
  assert_eq!(controller.up(id).unwrap().selected_path, InputDeliveryPath::Noop);
  assert_eq!(*backend.events.lock().unwrap(), vec![(0, true), (0, false)]);
}
