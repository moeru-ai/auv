use super::*;
#[test]
fn overdue_mouse_samples_coalesce_but_keep_the_final_sample() {
  let samples = [
    crate::MouseMotionSample {
      point: crate::Point::new(0.0, 0.0),
      elapsed: std::time::Duration::ZERO,
    },
    crate::MouseMotionSample {
      point: crate::Point::new(1.0, 1.0),
      elapsed: std::time::Duration::from_millis(8),
    },
    crate::MouseMotionSample {
      point: crate::Point::new(2.0, 2.0),
      elapsed: std::time::Duration::from_millis(16),
    },
  ];

  assert_eq!(latest_due_mouse_sample(&samples, 0, std::time::Duration::from_millis(12)), 1);
  assert_eq!(latest_due_mouse_sample(&samples, 2, std::time::Duration::from_secs(1)), 2);
}

use std::sync::atomic::{AtomicBool, Ordering};
#[derive(Default)]
struct Receiver {
  events: Mutex<Vec<String>>,
  fail_up: AtomicBool,
}
impl MouseBackend for Receiver {
  fn move_to(&self, _: Point, held: Option<MouseButton>) -> DriverResult<InputActionResult> {
    self.events.lock().unwrap().push(if held.is_some() { "drag" } else { "move" }.into());
    Ok(InputActionResult::single_success(crate::InputDeliveryPath::ForegroundSystemEvents))
  }
  fn button(&self, _: Point, _: MouseButton, down: bool) -> DriverResult<InputActionResult> {
    self.events.lock().unwrap().push(if down { "down" } else { "up" }.into());
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
  let coordinator = Arc::new(MouseCoordinator::default());
  let receiver = Arc::new(Receiver::default());
  coordinator.down(0, Point::new(1., 2.), MouseButton::Left, Duration::from_millis(20), receiver.clone()).unwrap();
  wait_for(|| receiver.events.lock().unwrap().last().is_some_and(|event| event == "up"));
  assert_eq!(coordinator.state.lock().unwrap().holder, None);
}

#[test]
fn cancelled_waiter_never_posts_after_the_holder_releases() {
  let coordinator = Arc::new(MouseCoordinator::default());
  let receiver = Arc::new(Receiver::default());
  let other = coordinator.create_mouse().unwrap();
  coordinator.down(0, Point::new(1., 2.), MouseButton::Left, Duration::from_secs(1), receiver.clone()).unwrap();
  let cancelled = Arc::new(AtomicBool::new(false));
  let worker = {
    let coordinator = coordinator.clone();
    let receiver = receiver.clone();
    let flag = cancelled.clone();
    std::thread::spawn(move || {
      with_input_cancellation(flag, || coordinator.down(other, Point::new(3., 4.), MouseButton::Right, Duration::from_secs(1), receiver))
    })
  };
  wait_for(|| !coordinator.state.lock().unwrap().waiting.is_empty());
  cancelled.store(true, Ordering::Release);
  assert!(worker.join().unwrap().is_err());
  coordinator.up(0).unwrap();
  assert_eq!(*receiver.events.lock().unwrap(), ["move", "down", "up"]);
}

#[test]
fn cancelled_complete_hold_releases_promptly() {
  let coordinator = Arc::new(MouseCoordinator::default());
  let receiver = Arc::new(Receiver::default());
  let cancelled = Arc::new(AtomicBool::new(false));
  let worker = {
    let coordinator = coordinator.clone();
    let receiver = receiver.clone();
    let flag = cancelled.clone();
    std::thread::spawn(move || {
      with_input_cancellation(flag, || coordinator.hold(0, Point::new(3., 4.), MouseButton::Right, Duration::from_secs(60), receiver))
    })
  };
  wait_for(|| receiver.events.lock().unwrap().iter().any(|event| event == "down"));
  cancelled.store(true, Ordering::Release);
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
    bundle_id: "other.target".into(),
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
    std::thread::spawn(move || coordinator.hold(0, Point::new(1., 2.), MouseButton::Left, Duration::from_secs(60), receiver))
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
