use super::*;
use crate::input_cancellation::{InputCancellation, with_input_cancellation};
use std::sync::atomic::{AtomicBool, Ordering};
#[derive(Default)]
struct Receiver {
  events: Mutex<Vec<String>>,
  fail_up: AtomicBool,
  up_gate: Option<(std::sync::mpsc::Sender<()>, Mutex<std::sync::mpsc::Receiver<()>>)>,
}
impl MouseBackend for Receiver {
  fn move_to(&self, _: Point, held: Option<MouseButton>) -> DriverResult<InputActionResult> {
    self.events.lock().unwrap().push(if held.is_some() { "drag" } else { "move" }.into());
    Ok(InputActionResult::single_success(crate::InputDeliveryPath::ForegroundSystemEvents))
  }
  fn button(&self, _: Point, _: MouseButton, down: bool) -> DriverResult<InputActionResult> {
    self.events.lock().unwrap().push(if down { "down" } else { "up" }.into());
    if !down {
      if let Some((entered, release)) = &self.up_gate {
        entered.send(()).map_err(|_| invalid("release fixture observer exited"))?;
        release.lock().unwrap().recv_timeout(Duration::from_secs(2)).map_err(|_| invalid("release fixture was not resumed"))?;
      }
    }
    if !down && self.fail_up.load(Ordering::SeqCst) {
      return Err(invalid("injected release failure"));
    }
    Ok(InputActionResult::single_success(crate::InputDeliveryPath::ForegroundSystemEvents))
  }
}
#[test]
fn foreign_waiter_does_not_block_holders_release() {
  let coordinator = Arc::new(MouseCoordinator::default());
  let a = coordinator.create_mouse().unwrap();
  let b = coordinator.create_mouse().unwrap();
  let receiver = Arc::new(Receiver::default());
  coordinator.down(a, Point::new(1., 2.), MouseButton::Left, Duration::from_secs(1), receiver.clone()).unwrap();
  let other = coordinator.clone();
  let backend = receiver.clone();
  let waiting = std::thread::spawn(move || other.move_to(b, Point::new(3., 4.), backend).unwrap());
  // Synchronize on admission rather than assuming a thread scheduling delay.
  wait_for(|| !coordinator.state.lock().unwrap().waiting.is_empty());
  coordinator.move_to(a, Point::new(5., 6.), receiver.clone()).unwrap();
  coordinator.up(a).unwrap();
  waiting.join().unwrap();
  assert_eq!(*receiver.events.lock().unwrap(), ["move", "down", "drag", "up", "move"]);
}
#[test]
fn failed_release_retains_reservation_until_explicit_recovery() {
  let coordinator = Arc::new(MouseCoordinator::default());
  let receiver = Arc::new(Receiver::default());
  coordinator.down(0, Point::new(1., 2.), MouseButton::Left, Duration::from_secs(1), receiver.clone()).unwrap();
  receiver.fail_up.store(true, Ordering::SeqCst);
  assert!(coordinator.up(0).is_err());
  assert_eq!(coordinator.state.lock().unwrap().holder, Some(0));
  assert!(coordinator.move_to(0, Point::new(3., 4.), receiver.clone()).is_err());
  receiver.fail_up.store(false, Ordering::SeqCst);
  coordinator.up(0).unwrap();
  assert_eq!(coordinator.state.lock().unwrap().holder, None);
  coordinator.down(0, Point::new(3., 4.), MouseButton::Right, Duration::from_secs(1), receiver.clone()).unwrap();
  coordinator.up(0).unwrap();
  assert_eq!(*receiver.events.lock().unwrap(), ["move", "down", "up", "up", "move", "down", "up"]);
}
fn wait_for(mut ready: impl FnMut() -> bool) {
  let deadline = Instant::now() + Duration::from_secs(2);
  while !ready() {
    assert!(Instant::now() < deadline, "receiver did not reach expected state");
    std::thread::sleep(Duration::from_millis(1));
  }
}

#[test]
fn abandoned_hold_releases_without_another_request() {
  // ROOT CAUSE:
  //
  // If the backend recorded up before native release returned, this test
  // asserted that the reservation was already clear while release was in flight.
  // Before the fix, that assertion raced with valid watchdog cleanup.
  // The fix waits for the completed reservation release, not only the up event.
  let coordinator = Arc::new(MouseCoordinator::default());
  let receiver = Arc::new(Receiver::default());
  coordinator.down(0, Point::new(1., 2.), MouseButton::Left, Duration::from_millis(20), receiver.clone()).unwrap();
  wait_for(|| receiver.events.lock().unwrap().last().is_some_and(|event| event == "up"));
  wait_for(|| coordinator.state.lock().unwrap().holder.is_none());
  let holder = coordinator.state.lock().unwrap().holder;
  assert_eq!(holder, None);
}

// https://github.com/moeru-ai/auv/actions/runs/41103991540/job/124228286676
#[test]
fn ci_124228286676_up_receipt_precedes_reservation_release() {
  let coordinator = Arc::new(MouseCoordinator::default());
  let (entered, entered_rx) = std::sync::mpsc::channel();
  let (release, release_rx) = std::sync::mpsc::channel();
  let receiver = Arc::new(Receiver {
    up_gate: Some((entered, Mutex::new(release_rx))),
    ..Default::default()
  });
  coordinator.down(0, Point::new(1., 2.), MouseButton::Left, Duration::from_millis(20), receiver.clone()).unwrap();
  entered_rx.recv_timeout(Duration::from_secs(2)).expect("watchdog entered native release");
  let up_was_recorded = receiver.events.lock().unwrap().last().is_some_and(|event| event == "up");
  let holder_while_native_release_runs = coordinator.state.lock().unwrap().holder;
  release.send(()).unwrap();
  // Finish the worker before testing the old assumption, so a failing assertion
  // cannot strand the fixture or poison a mutex during coordinator destruction.
  wait_for(|| coordinator.state.lock().unwrap().holder.is_none());
  assert!(up_was_recorded);
  assert_eq!(holder_while_native_release_runs, Some(0), "reservation must remain owned while native release runs");
}

#[test]
fn cancelled_waiter_never_posts_after_the_holder_releases() {
  let coordinator = Arc::new(MouseCoordinator::default());
  let receiver = Arc::new(Receiver::default());
  let other = coordinator.create_mouse().unwrap();
  coordinator.down(0, Point::new(1., 2.), MouseButton::Left, Duration::from_secs(1), receiver.clone()).unwrap();
  let cancelled = Arc::new(InputCancellation::default());
  let worker = {
    let coordinator = coordinator.clone();
    let receiver = receiver.clone();
    let flag = cancelled.clone();
    std::thread::spawn(move || {
      with_input_cancellation(flag, || coordinator.down(other, Point::new(3., 4.), MouseButton::Right, Duration::from_secs(1), receiver))
    })
  };
  wait_for(|| !coordinator.state.lock().unwrap().waiting.is_empty());
  cancelled.cancel();
  assert!(worker.join().unwrap().is_err());
  coordinator.up(0).unwrap();
  assert_eq!(*receiver.events.lock().unwrap(), ["move", "down", "up"]);
}

#[test]
fn cancelled_complete_hold_releases_promptly() {
  let coordinator = Arc::new(MouseCoordinator::default());
  let receiver = Arc::new(Receiver::default());
  let cancelled = Arc::new(InputCancellation::default());
  let worker = {
    let coordinator = coordinator.clone();
    let receiver = receiver.clone();
    let flag = cancelled.clone();
    std::thread::spawn(move || {
      with_input_cancellation(flag, || coordinator.hold(0, Point::new(3., 4.), MouseButton::Right, Duration::from_secs(3600), receiver))
    })
  };
  wait_for(|| receiver.events.lock().unwrap().iter().any(|event| event == "down"));
  cancelled.cancel();
  assert!(worker.join().unwrap().is_err());
  assert_eq!(*receiver.events.lock().unwrap(), ["move", "down", "up"]);
  assert_eq!(coordinator.state.lock().unwrap().holder, None);
}

#[test]
fn cancelled_drag_uses_the_same_release_primitive() {
  let coordinator = Arc::new(MouseCoordinator::default());
  let receiver = Arc::new(Receiver::default());
  let result = coordinator.motion(crate::MoveMouseRequest::direct(Point::new(1., 2.)), Some(MouseButton::Left), receiver.clone(), |event| {
    matches!(event, MotionEvent::Started { .. })
  });
  assert!(result.is_err());
  assert_eq!(*receiver.events.lock().unwrap(), ["move", "down", "drag", "up"]);
  assert_eq!(coordinator.state.lock().unwrap().holder, None);
}

#[test]
fn held_route_cannot_switch_targets() {
  let coordinator = Arc::new(MouseCoordinator::default());
  let receiver = Arc::new(Receiver::default());
  coordinator.down(0, Point::new(1., 2.), MouseButton::Left, Duration::from_secs(1), receiver.clone()).unwrap();
  let mut request = crate::MoveMouseRequest::direct(Point::new(3., 4.));
  request.target = Some(crate::InputTarget::Application {
    id: "other.target".into(),
  });
  assert!(coordinator.motion(request, None, receiver.clone(), |_| true).is_err());
  assert_eq!(*receiver.events.lock().unwrap(), ["move", "down"]);
  coordinator.up(0).unwrap();
}

#[test]
fn invalid_down_does_not_deliver_input() {
  let coordinator = Arc::new(MouseCoordinator::default());
  let receiver = Arc::new(Receiver::default());
  assert!(coordinator.down(0, Point::new(f64::NAN, 0.), MouseButton::Left, Duration::from_secs(1), receiver.clone()).is_err());
  assert!(receiver.events.lock().unwrap().is_empty());
}

#[test]
fn closing_feedback_before_movement_releases_an_existing_hold() {
  let coordinator = Arc::new(MouseCoordinator::default());
  let receiver = Arc::new(Receiver::default());
  coordinator.down(0, Point::new(1., 2.), MouseButton::Left, Duration::from_secs(1), receiver.clone()).unwrap();
  assert!(coordinator.motion(crate::MoveMouseRequest::direct(Point::new(3., 4.)), None, receiver.clone(), |_| false).is_err());
  assert_eq!(*receiver.events.lock().unwrap(), ["move", "down", "up"]);
  assert_eq!(coordinator.state.lock().unwrap().holder, None);
}

#[test]
fn shutdown_releases_cross_call_input_and_rejects_new_delivery() {
  let coordinator = Arc::new(MouseCoordinator::default());
  let receiver = Arc::new(Receiver::default());
  coordinator.down(0, Point::new(1., 2.), MouseButton::Left, Duration::from_secs(1), receiver.clone()).unwrap();
  coordinator.shutdown().unwrap();
  assert!(coordinator.move_to(0, Point::new(3., 4.), receiver.clone()).is_err());
  assert!(coordinator.create_mouse().is_err());
  assert_eq!(*receiver.events.lock().unwrap(), ["move", "down", "up"]);
}

#[test]
fn shutdown_interrupts_an_active_hold_and_releases_before_returning() {
  let coordinator = Arc::new(MouseCoordinator::default());
  let receiver = Arc::new(Receiver::default());
  let worker = {
    let coordinator = coordinator.clone();
    let receiver = receiver.clone();
    std::thread::spawn(move || coordinator.hold(0, Point::new(1., 2.), MouseButton::Left, Duration::from_secs(3600), receiver))
  };
  wait_for(|| receiver.events.lock().unwrap().iter().any(|event| event == "down"));
  coordinator.shutdown().unwrap();
  assert!(worker.join().unwrap().is_err());
  assert_eq!(*receiver.events.lock().unwrap(), ["move", "down", "up"]);
}

#[test]
fn complete_hold_returns_release_evidence_after_matching_down_and_up() {
  let coordinator = Arc::new(MouseCoordinator::default());
  let receiver = Arc::new(Receiver::default());
  let action = coordinator.hold(0, Point::new(1., 2.), MouseButton::Left, Duration::from_millis(1), receiver.clone()).unwrap();
  assert_eq!(*receiver.events.lock().unwrap(), ["move", "down", "up"]);
  assert_eq!(action, InputActionResult::single_success(crate::InputDeliveryPath::ForegroundSystemEvents));
  coordinator.move_to(0, Point::new(3., 4.), receiver.clone()).unwrap();
  assert_eq!(*receiver.events.lock().unwrap(), ["move", "down", "up", "move"]);
}

#[derive(Default)]
struct DelayedBackend {
  move_delay: Duration,
  press_delay: Duration,
  pressed_at: Mutex<Option<Instant>>,
  released_at: Mutex<Option<Instant>>,
}
impl DelayedBackend {
  fn held_duration(&self) -> Duration {
    let pressed = self.pressed_at.lock().unwrap().expect("press");
    let released = self.released_at.lock().unwrap().expect("release");
    released.saturating_duration_since(pressed)
  }
}
impl MouseBackend for DelayedBackend {
  fn move_to(&self, _: Point, _: Option<MouseButton>) -> DriverResult<InputActionResult> {
    std::thread::sleep(self.move_delay);
    Ok(InputActionResult::single_success(crate::InputDeliveryPath::ForegroundSystemEvents))
  }
  fn button(&self, _: Point, _: MouseButton, down: bool) -> DriverResult<InputActionResult> {
    if down {
      std::thread::sleep(self.press_delay);
      *self.pressed_at.lock().unwrap() = Some(Instant::now());
    } else {
      *self.released_at.lock().unwrap() = Some(Instant::now());
    }
    Ok(InputActionResult::single_success(crate::InputDeliveryPath::ForegroundSystemEvents))
  }
}

// ROOT CAUSE:
//
// If the first `move_to` took longer than the hold timeout, the button was
// released right after the press because `down_admitted` computed the hold
// deadline before `move_to`. Opening the Linux portal input session happens
// inside that first `move_to`, so a slow session open consumed the whole hold.
//
// Before the fix, a slow `move_to` shortened the hold to almost zero.
// The fix keeps the deadline measured from the delivered button press.
//
// Report: docs/ai/references/driver/2026-10-19-linux-portal-eis-input.md,
// "Held-Button Deadline".
#[test]
fn moving_longer_than_the_timeout_keeps_the_full_hold() {
  let timeout = Duration::from_millis(120);
  let coordinator = Arc::new(MouseCoordinator::default());
  let backend = Arc::new(DelayedBackend {
    move_delay: Duration::from_millis(240),
    ..Default::default()
  });
  coordinator.hold(0, Point::new(1., 2.), MouseButton::Left, timeout, backend.clone()).unwrap();
  let held = backend.held_duration();
  assert!(held >= timeout, "hold lasted {held:?}, expected at least {timeout:?}");
}

// The delivered press starts the deadline, not the start of `down_admitted`.
#[test]
fn press_latency_does_not_count_toward_the_hold_timeout() {
  let timeout = Duration::from_millis(120);
  let coordinator = Arc::new(MouseCoordinator::default());
  let backend = Arc::new(DelayedBackend {
    press_delay: Duration::from_millis(240),
    ..Default::default()
  });
  coordinator.down(0, Point::new(1., 2.), MouseButton::Left, timeout, backend.clone()).unwrap();
  wait_for(|| backend.released_at.lock().unwrap().is_some());
  let held = backend.held_duration();
  assert!(held >= timeout, "hold lasted {held:?}, expected at least {timeout:?}");
}

#[test]
fn unrepresentable_deadline_fails_before_native_delivery() {
  let coordinator = Arc::new(MouseCoordinator::default());
  let receiver = Arc::new(Receiver::default());
  assert!(coordinator.down(0, Point::new(1., 2.), MouseButton::Left, Duration::MAX, receiver.clone()).is_err());
  assert!(coordinator.hold(0, Point::new(1., 2.), MouseButton::Left, Duration::MAX, receiver.clone()).is_err());
  assert!(receiver.events.lock().unwrap().is_empty());
}
