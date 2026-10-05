use std::cell::RefCell;
use std::time::Duration;

use super::*;
use crate::geometry::{CoordinateSpace, Rect};
use crate::input::{ClickOptions, DisturbanceLevel, InputAttempt, InputDeliveryPath, InputPolicy, MouseButton, ScrollDeliveryCandidate};
use crate::window::WindowRef;

struct RecordingInput {
  calls: RefCell<Vec<(Scroll, ScrollOptions)>>,
}

impl WindowInput for RecordingInput {
  fn click(&self, _: &Window, _: WindowPoint, _: ClickOptions) -> DriverResult<InputActionResult> {
    unreachable!("scroll stream never clicks")
  }

  fn scroll(&self, _: &Window, _: WindowPoint, scroll: Scroll, options: ScrollOptions) -> DriverResult<InputActionResult> {
    self.calls.borrow_mut().push((scroll, options));
    Ok(InputActionResult {
      selected_path: InputDeliveryPath::WindowTargetedWheel,
      attempts: vec![InputAttempt::success(
        InputDeliveryPath::WindowTargetedWheel,
      )],
      verified: false,
      mouse_disturbance: DisturbanceLevel::None,
      focus_disturbance: DisturbanceLevel::None,
      clipboard_disturbance: DisturbanceLevel::None,
    })
  }

  fn drag(&self, _: &Window, _: crate::MoveMouseRequest, _: MouseButton, _: InputPolicy) -> DriverResult<(crate::Point, InputActionResult)> {
    unreachable!("scroll stream never drags")
  }
}

fn recording() -> RecordingInput {
  RecordingInput {
    calls: RefCell::new(Vec::new()),
  }
}

fn test_window() -> Window {
  Window {
    reference: WindowRef {
      id: "window-1".into(),
    },
    title: None,
    app_name: None,
    app_bundle_id: None,
    process_id: Some(1),
    frame: Rect::new(0.0, 0.0, 800.0, 600.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
  }
}

fn options(max_acceleration: Option<f64>, lease_ms: u64) -> ScrollStreamOptions {
  ScrollStreamOptions {
    sample_rate_hz: 200,
    max_acceleration,
    lease: Duration::from_millis(lease_ms),
  }
}

fn run(
  input: &RecordingInput,
  control: &ScrollStreamControl,
  stream: ScrollStreamOptions,
) -> (ScrollStreamResult, Vec<ScrollStreamProgress>) {
  let mut progress = Vec::new();
  let result = input
    .scroll_stream(&test_window(), WindowPoint::new(10.0, 20.0), control, stream, ScrollOptions::default(), &mut |update| {
      progress.push(update)
    })
    .unwrap();
  (result, progress)
}

#[test]
fn velocity_and_stream_options_are_validated_before_delivery() {
  assert!(ScrollVelocity::new(0.0, MAX_SCROLL_VELOCITY + 1.0).validate().is_err());
  assert!(ScrollVelocity::new(f64::NAN, 0.0).validate().is_err());
  assert!(ScrollStreamControl::new().set_velocity(ScrollVelocity::new(0.0, f64::INFINITY)).is_err());
  for invalid in [
    ScrollStreamOptions {
      sample_rate_hz: 0,
      ..options(None, 100)
    },
    options(Some(0.0), 100),
    options(Some(f64::NAN), 100),
    options(None, 0),
    options(None, 60_001),
  ] {
    assert!(matches!(invalid.validate(), Err(DriverError::InvalidInput { .. })), "{invalid:?}");
  }
}

#[test]
fn ramp_limits_the_velocity_change_vector() {
  let current = ScrollVelocity::ZERO;
  let target = ScrollVelocity::new(300.0, 400.0);
  assert_eq!(ramp(current, target, None), target);
  let step = ramp(current, target, Some(50.0));
  assert!((step.delta_x_per_second - 30.0).abs() < 1e-9 && (step.delta_y_per_second - 40.0).abs() < 1e-9);
  assert_eq!(ramp(ScrollVelocity::new(290.0, 400.0), target, Some(50.0)), target);
}

#[test]
fn stream_integrates_velocity_and_stops_on_request() {
  let input = recording();
  let control = ScrollStreamControl::new();
  control.set_velocity(ScrollVelocity::new(0.0, 1_000.0)).unwrap();
  let remote = control.clone();
  let stopper = std::thread::spawn(move || {
    std::thread::sleep(Duration::from_millis(200));
    remote.stop();
  });
  let (result, progress) = run(&input, &control, options(None, 5_000));
  stopper.join().unwrap();

  assert_eq!(result.reason, ScrollStreamStopReason::Stopped);
  // About 200 ms at 1000 px/s; allow scheduler jitter.
  assert!((150.0..=320.0).contains(&result.delivered.delta_y), "{result:?}");
  let total: f64 = input.calls.borrow().iter().map(|(scroll, _)| scroll.delta_y).sum();
  assert_eq!(total, result.delivered.delta_y);
  for (_, options) in &input.calls.borrow()[1..] {
    assert_eq!(options.delivery_strategy.candidates, vec![ScrollDeliveryCandidate::WindowTargetedWheel]);
  }
  assert_eq!(progress.last().unwrap().velocity, ScrollVelocity::ZERO);
  assert_eq!(result.action.unwrap().selected_path, InputDeliveryPath::WindowTargetedWheel);
}

#[test]
fn stream_ramps_with_max_acceleration_and_ramps_down_before_completing() {
  let input = recording();
  let control = ScrollStreamControl::new();
  control.set_velocity(ScrollVelocity::new(0.0, 2_000.0)).unwrap();
  let remote = control.clone();
  let stopper = std::thread::spawn(move || {
    std::thread::sleep(Duration::from_millis(150));
    remote.stop();
  });
  // 4000 px/s²: reaching 2000 px/s takes 500 ms, so at 150 ms speed is ~600.
  let (result, progress) = run(&input, &control, options(Some(4_000.0), 5_000));
  stopper.join().unwrap();

  let peak = progress.iter().map(|update| update.velocity.delta_y_per_second).fold(0.0, f64::max);
  assert!(peak < 1_000.0, "peak {peak}");
  assert!(progress[0].velocity.delta_y_per_second <= 4_000.0 * 0.02, "{:?}", progress[0]);
  assert_eq!(result.reason, ScrollStreamStopReason::Stopped);
  // Ramping down from ~600 px/s at 4000 px/s² takes ~150 ms more.
  assert!(result.elapsed >= Duration::from_millis(250), "{result:?}");
}

#[test]
fn stream_completes_when_the_lease_expires_without_renewal() {
  let input = recording();
  let control = ScrollStreamControl::new();
  control.set_velocity(ScrollVelocity::new(500.0, 0.0)).unwrap();
  let (result, _) = run(&input, &control, options(None, 100));
  assert_eq!(result.reason, ScrollStreamStopReason::LeaseExpired);
  assert!(result.elapsed < Duration::from_millis(400), "{result:?}");
  assert!(result.delivered.delta_x > 0.0 && result.delivered.delta_x <= 150.0, "{result:?}");
}

#[test]
fn cancel_ends_without_ramping_and_an_idle_stream_reports_no_action() {
  let input = recording();
  let control = ScrollStreamControl::new();
  control.cancel();
  let (result, _) = run(&input, &control, options(Some(10.0), 5_000));
  assert_eq!(result.reason, ScrollStreamStopReason::Cancelled);
  assert!(result.action.is_none());
  assert_eq!(result.delivered, Scroll::new(0.0, 0.0));
  assert!(input.calls.borrow().is_empty());
}
