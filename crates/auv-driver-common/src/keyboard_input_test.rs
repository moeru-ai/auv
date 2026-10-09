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
  while !state.held.is_empty() && Instant::now() < deadline {
    state = controller.changed.wait_timeout(state, Duration::from_millis(20)).unwrap().0;
  }
  assert!(state.held.is_empty());
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
  while !state.held.is_empty() && Instant::now() < deadline {
    state = controller.changed.wait_timeout(state, Duration::from_millis(20)).unwrap().0;
  }
  assert!(state.held.is_empty());
  drop(state);
  assert_eq!(controller.up(id).unwrap().selected_path, InputDeliveryPath::Noop);
  assert_eq!(*backend.events.lock().unwrap(), vec![(0, true), (0, false)]);
}

#[test]
fn independent_holds_release_only_their_own_keys() {
  let controller = Arc::new(KeyboardHoldController::default());
  let control = FakeBackend::new(1);
  let shift = FakeBackend::new(1);
  let control_id = controller
    .down_independent(control.clone(), Duration::from_secs(2), KeyboardHoldIdentity::new("display-1", ["Control_L"]))
    .unwrap()
    .into_id();
  let shift_id = controller
    .down_independent(shift.clone(), Duration::from_secs(2), KeyboardHoldIdentity::new("display-1", ["Shift_L"]))
    .unwrap()
    .into_id();
  controller.up(shift_id).unwrap();
  assert_eq!(*control.events.lock().unwrap(), vec![(0, true)]);
  controller.up(control_id).unwrap();
  assert_eq!(*shift.events.lock().unwrap(), vec![(0, true), (0, false)]);
  assert_eq!(*control.events.lock().unwrap(), vec![(0, true), (0, false)]);
  assert_eq!(controller.up(shift_id).unwrap().selected_path, InputDeliveryPath::Noop);
}

#[test]
fn independent_identity_and_route_conflicts_fail_before_delivery() {
  let controller = Arc::new(KeyboardHoldController::default());
  let control = FakeBackend::new(1);
  let first = controller
    .down_independent(control.clone(), Duration::from_secs(2), KeyboardHoldIdentity::new("display-1", ["Control_L"]))
    .unwrap()
    .into_id();
  let duplicate = FakeBackend::new(1);
  assert!(
    controller.down_independent(duplicate.clone(), Duration::from_secs(2), KeyboardHoldIdentity::new("display-1", ["Control_L"])).is_err()
  );
  let other_route = FakeBackend::new(1);
  assert!(
    controller.down_independent(other_route.clone(), Duration::from_secs(2), KeyboardHoldIdentity::new("display-2", ["Shift_L"])).is_err()
  );
  assert!(controller.down(FakeBackend::new(1), Duration::from_secs(1)).is_err());
  assert!(duplicate.events.lock().unwrap().is_empty());
  assert!(other_route.events.lock().unwrap().is_empty());
  controller.up(first).unwrap();
}

#[test]
fn independent_timeout_and_cancellation_do_not_release_other_holds() {
  let controller = Arc::new(KeyboardHoldController::default());
  let long = FakeBackend::new(1);
  let short = FakeBackend::new(1);
  let long_id = controller
    .down_independent(long.clone(), Duration::from_secs(2), KeyboardHoldIdentity::new("display-1", ["Control_L"]))
    .unwrap()
    .into_id();
  let short_id = controller
    .down_independent(short.clone(), Duration::from_millis(20), KeyboardHoldIdentity::new("display-1", ["Shift_L"]))
    .unwrap()
    .into_id();
  let deadline = Instant::now() + Duration::from_secs(1);
  let mut state = controller.state.lock().unwrap();
  while state.held.iter().any(|held| held.id == short_id) && Instant::now() < deadline {
    state = controller.changed.wait_timeout(state, Duration::from_millis(20)).unwrap().0;
  }
  assert!(!state.held.iter().any(|held| held.id == short_id));
  assert!(state.held.iter().any(|held| held.id == long_id));
  drop(state);
  assert_eq!(*long.events.lock().unwrap(), vec![(0, true)]);
  controller.up(long_id).unwrap();
}

#[test]
fn cancelling_one_independent_hold_preserves_other_keys() {
  let controller = Arc::new(KeyboardHoldController::default());
  let long = FakeBackend::new(1);
  let cancelled = FakeBackend::new(1);
  let long_id = controller
    .down_independent(long.clone(), Duration::from_secs(2), KeyboardHoldIdentity::new("display-1", ["Control_L"]))
    .unwrap()
    .into_id();
  let flag = Arc::new(crate::input_cancellation::InputCancellation::default());
  let cancelled_id = crate::input_cancellation::with_input_cancellation(flag.clone(), || {
    controller.down_independent(cancelled.clone(), Duration::from_secs(2), KeyboardHoldIdentity::new("display-1", ["Shift_L"]))
  })
  .unwrap()
  .into_id();
  flag.cancel();
  let deadline = Instant::now() + Duration::from_secs(1);
  let mut state = controller.state.lock().unwrap();
  while state.held.iter().any(|held| held.id == cancelled_id) && Instant::now() < deadline {
    state = controller.changed.wait_timeout(state, Duration::from_millis(20)).unwrap().0;
  }
  assert!(!state.held.iter().any(|held| held.id == cancelled_id));
  assert!(state.held.iter().any(|held| held.id == long_id));
  drop(state);
  assert_eq!(*long.events.lock().unwrap(), vec![(0, true)]);
  controller.up(long_id).unwrap();
}

#[test]
fn independent_hold_limit_rejects_before_delivery() {
  let controller = Arc::new(KeyboardHoldController::default());
  let mut ids = Vec::new();
  for index in 0..MAX_INDEPENDENT_HOLDS {
    ids.push(
      controller
        .down_independent(FakeBackend::new(1), Duration::from_secs(2), KeyboardHoldIdentity::new("display-1", [format!("key-{index}")]))
        .unwrap()
        .into_id(),
    );
  }
  let extra = FakeBackend::new(1);
  assert!(controller.down_independent(extra.clone(), Duration::from_secs(2), KeyboardHoldIdentity::new("display-1", ["extra"])).is_err());
  assert!(extra.events.lock().unwrap().is_empty());
  controller.shutdown().unwrap();
  for id in ids {
    assert_eq!(controller.up(id).unwrap().selected_path, InputDeliveryPath::Noop);
  }
}

#[test]
fn shutdown_does_not_disable_a_replacement_runner_in_the_same_process() {
  // ROOT CAUSE:
  //
  // If one Runner service was dropped, later services could not hold any key
  // because the process-wide controller retained a permanent shutdown flag.
  // The fence now lasts only while the old service releases its held keys.
  let controller = Arc::new(KeyboardHoldController::default());
  let original = controller
    .down_independent(FakeBackend::new(1), Duration::from_secs(1), KeyboardHoldIdentity::new("display-1", ["Control_L"]))
    .unwrap()
    .into_id();
  controller.shutdown().unwrap();
  assert_eq!(controller.up(original).unwrap().selected_path, InputDeliveryPath::Noop);
  let replacement = FakeBackend::new(1);
  let id = controller
    .down_independent(replacement.clone(), Duration::from_secs(1), KeyboardHoldIdentity::new("display-1", ["Shift_L"]))
    .unwrap()
    .into_id();
  controller.up(id).unwrap();
  assert_eq!(*replacement.events.lock().unwrap(), vec![(0, true), (0, false)]);
}

#[test]
fn failed_independent_release_blocks_new_down_but_other_ids_remain_releasable() {
  let controller = Arc::new(KeyboardHoldController::default());
  let first = FakeBackend::new(1);
  let second = FakeBackend::new(1);
  let first_id = controller
    .down_independent(first.clone(), Duration::from_secs(2), KeyboardHoldIdentity::new("display-1", ["Control_L"]))
    .unwrap()
    .into_id();
  let second_id = controller
    .down_independent(second.clone(), Duration::from_secs(2), KeyboardHoldIdentity::new("display-1", ["Shift_L"]))
    .unwrap()
    .into_id();
  first.fail_release_once.store(true, Ordering::SeqCst);
  assert!(controller.up(first_id).is_err());
  let third = FakeBackend::new(1);
  assert!(controller.down_independent(third.clone(), Duration::from_secs(2), KeyboardHoldIdentity::new("display-1", ["Alt_L"])).is_err());
  assert!(third.events.lock().unwrap().is_empty());
  controller.up(second_id).unwrap();
  controller.up(first_id).unwrap();
}

#[test]
fn shutdown_releases_all_independent_holds_even_after_an_error() {
  let controller = Arc::new(KeyboardHoldController::default());
  let first = FakeBackend::new(1);
  let second = FakeBackend::new(1);
  let first_id = controller
    .down_independent(first.clone(), Duration::from_secs(2), KeyboardHoldIdentity::new("display-1", ["Control_L"]))
    .unwrap()
    .into_id();
  controller
    .down_independent(second.clone(), Duration::from_secs(2), KeyboardHoldIdentity::new("display-1", ["Shift_L"]))
    .unwrap()
    .into_id();
  first.fail_release_once.store(true, Ordering::SeqCst);
  assert!(controller.shutdown().is_err());
  assert_eq!(*second.events.lock().unwrap(), vec![(0, true), (0, false)]);
  controller.up(first_id).unwrap();
}
