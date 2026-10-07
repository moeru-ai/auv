use std::cell::RefCell;
use std::time::Duration;

use super::*;
use crate::geometry::{CoordinateSpace, Rect};
use crate::input::{ClickOptions, DisturbanceLevel, InputAttempt, MouseButton};
use crate::window::WindowRef;

fn motion(total: Scroll, duration_ms: u64, function: TimingFunction, sample_rate_hz: u32) -> ScrollMotion {
  ScrollMotion {
    total,
    timing: MotionTiming::FixedDuration {
      duration: Duration::from_millis(duration_ms),
      function,
    },
    sample_rate_hz,
  }
}

#[test]
fn timing_functions_start_at_zero_end_at_one_and_shape_the_middle() {
  let functions = [
    TimingFunction::Linear,
    TimingFunction::EaseInCubic,
    TimingFunction::EaseOutCubic,
    TimingFunction::EaseInOutCubic,
    TimingFunction::CubicBezier {
      x1: 0.25,
      y1: 0.1,
      x2: 0.25,
      y2: 1.0,
    },
  ];
  for function in functions {
    assert!(function.progress(0.0).abs() < 1e-6, "{function:?}");
    assert!((function.progress(1.0) - 1.0).abs() < 1e-6, "{function:?}");
  }
  assert!((TimingFunction::Linear.progress(0.25) - 0.25).abs() < 1e-12);
  assert!(TimingFunction::EaseInCubic.progress(0.5) < 0.5);
  assert!(TimingFunction::EaseOutCubic.progress(0.5) > 0.5);
  assert!((TimingFunction::EaseInOutCubic.progress(0.5) - 0.5).abs() < 1e-12);
  // cubic-bezier(0, 0, 1, 1) is linear; CSS `ease` front-loads progress.
  let linear_bezier = TimingFunction::CubicBezier {
    x1: 0.0,
    y1: 0.0,
    x2: 1.0,
    y2: 1.0,
  };
  assert!((linear_bezier.progress(0.3) - 0.3).abs() < 1e-5);
  let ease = TimingFunction::CubicBezier {
    x1: 0.25,
    y1: 0.1,
    x2: 0.25,
    y2: 1.0,
  };
  assert!((ease.progress(0.5) - 0.8024).abs() < 1e-3);
}

#[test]
fn cubic_bezier_rejects_x_controls_outside_unit_interval() {
  for function in [
    TimingFunction::CubicBezier {
      x1: -0.1,
      y1: 0.0,
      x2: 1.0,
      y2: 1.0,
    },
    TimingFunction::CubicBezier {
      x1: 0.0,
      y1: f64::NAN,
      x2: 1.0,
      y2: 1.0,
    },
  ] {
    assert!(matches!(function.validate(), Err(DriverError::InvalidInput { .. })));
  }
  // Overshooting y values are valid in CSS and produce a back-and-forth scroll.
  assert!(
    TimingFunction::CubicBezier {
      x1: 0.5,
      y1: -0.5,
      x2: 0.5,
      y2: 1.5,
    }
    .validate()
    .is_ok()
  );
}

#[test]
fn schedule_rejects_invalid_motion_before_delivery() {
  assert!(motion(Scroll::new(0.0, 0.0), 100, TimingFunction::Linear, 60).schedule().is_err());
  assert!(motion(Scroll::new(f64::NAN, 1.0), 100, TimingFunction::Linear, 60).schedule().is_err());
  assert!(motion(Scroll::new(0.0, 100.0), 100, TimingFunction::Linear, 0).schedule().is_err());
  // A zero duration is one immediate sample and needs no sample rate.
  let immediate = motion(Scroll::new(0.0, 100.0), 0, TimingFunction::Linear, 0).schedule().unwrap();
  assert_eq!(immediate.len(), 1);
  assert_eq!(immediate.at(0), (Duration::ZERO, 1.0));
}

#[test]
fn schedule_samples_at_rate_and_ends_exactly_at_duration() {
  let schedule = motion(Scroll::new(0.0, 300.0), 500, TimingFunction::Linear, 60).schedule().unwrap();
  assert_eq!(schedule.len(), 31);
  assert_eq!(schedule.at(0), (Duration::ZERO, 0.0));
  let (elapsed, progress) = schedule.at(15);
  assert_eq!(elapsed, Duration::from_millis(250));
  assert!((progress - 0.5).abs() < 1e-9);
  assert_eq!(schedule.at(30), (Duration::from_millis(500), 1.0));
  assert_eq!(schedule.latest_due(3, Duration::from_millis(100)), 6);
  assert_eq!(schedule.latest_due(10, Duration::from_millis(100)), 10);
  assert_eq!(schedule.latest_due(3, Duration::from_secs(9)), 30);
}

#[test]
fn quantizer_emits_whole_native_units_whose_sum_is_exact() {
  // Windows: 100 px per notch, 120 units per notch.
  let quantum = 100.0 / 120.0;
  let mut quantizer = CumulativeQuantizer::new(Scroll::new(-50.0, 301.0), quantum).unwrap();
  let schedule = motion(Scroll::new(-50.0, 301.0), 1000, TimingFunction::EaseInOutCubic, 97).schedule().unwrap();
  let mut sum = (0.0, 0.0);
  for index in 0..schedule.len() {
    let delta = quantizer.step(schedule.at(index).1);
    let units = (delta.delta_x / quantum, delta.delta_y / quantum);
    assert!((units.0 - units.0.round()).abs() < 1e-9 && (units.1 - units.1.round()).abs() < 1e-9);
    sum = (sum.0 + delta.delta_x, sum.1 + delta.delta_y);
  }
  let expected = ((-50.0 / quantum).round() * quantum, (301.0 / quantum).round() * quantum);
  assert!((sum.0 - expected.0).abs() < 1e-6 && (sum.1 - expected.1).abs() < 1e-6);
  assert_eq!(quantizer.delivered(), Scroll::new(expected.0, expected.1));
}

struct RecordingInput {
  quantum: f64,
  calls: RefCell<Vec<(Scroll, ScrollOptions)>>,
  fail_on_call: Option<usize>,
}

impl WindowInput for RecordingInput {
  fn click(&self, _: &Window, _: WindowPoint, _: ClickOptions) -> DriverResult<InputActionResult> {
    unreachable!("scroll motion never clicks")
  }

  fn scroll(&self, _: &Window, _: WindowPoint, scroll: Scroll, options: ScrollOptions) -> DriverResult<InputActionResult> {
    let mut calls = self.calls.borrow_mut();
    calls.push((scroll, options));
    if self.fail_on_call == Some(calls.len()) {
      return Err(DriverError::Backend {
        message: "native wheel failed".to_string(),
      });
    }
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

  fn scroll_quantum(&self) -> f64 {
    self.quantum
  }

  fn drag(&self, _: &Window, _: crate::MoveMouseRequest, _: MouseButton, _: InputPolicy) -> DriverResult<(crate::Point, InputActionResult)> {
    unreachable!("scroll motion never drags")
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

#[test]
fn motion_delivers_exact_total_and_pins_the_first_selected_path() {
  let input = RecordingInput {
    quantum: 1.0,
    calls: RefCell::new(Vec::new()),
    fail_on_call: None,
  };
  let mut progress = Vec::new();
  let result = input
    .scroll_motion(
      &test_window(),
      WindowPoint::new(10.0, 20.0),
      // NOTICE(scroll-motion-test-timing): late samples are coalesced, so a
      // 50 ms motion became one call when a loaded CI runner stalled. 500 ms
      // keeps several calls unless the runner stalls for the whole motion.
      &motion(Scroll::new(0.0, 240.0), 500, TimingFunction::EaseOutCubic, 200),
      ScrollOptions::default(),
      &mut |update| progress.push(update),
    )
    .unwrap();

  let calls = input.calls.borrow();
  assert!(calls.len() > 1);
  assert_eq!(
    calls[0].1,
    ScrollOptions {
      settle: Duration::ZERO,
      ..ScrollOptions::default()
    }
  );
  for (_, options) in &calls[1..] {
    assert_eq!(options.policy, InputPolicy::BackgroundOnly);
    assert_eq!(options.delivery_strategy.candidates, vec![ScrollDeliveryCandidate::WindowTargetedWheel]);
  }
  let total: f64 = calls.iter().map(|(scroll, _)| scroll.delta_y).sum();
  assert_eq!(total, 240.0);
  assert_eq!(result.delivered, Scroll::new(0.0, 240.0));
  assert_eq!(result.action.selected_path, InputDeliveryPath::WindowTargetedWheel);
  assert_eq!(progress.last().unwrap().delivered, Scroll::new(0.0, 240.0));
}

#[test]
fn failed_later_sample_reports_partial_delivery() {
  let input = RecordingInput {
    quantum: 1.0,
    calls: RefCell::new(Vec::new()),
    fail_on_call: Some(2),
  };
  let error = input
    .scroll_motion(
      &test_window(),
      WindowPoint::new(10.0, 20.0),
      // See NOTICE(scroll-motion-test-timing): the failure needs a second call.
      &motion(Scroll::new(0.0, 120.0), 500, TimingFunction::Linear, 200),
      ScrollOptions::default(),
      &mut |_| {},
    )
    .unwrap_err();
  let message = error.to_string();
  assert!(message.contains("scroll motion stopped after delivering"), "{message}");
  assert!(message.contains("native wheel failed"), "{message}");
}

#[test]
fn motion_cancellation_stops_before_the_next_sample() {
  let input = RecordingInput {
    quantum: 1.0,
    calls: RefCell::new(Vec::new()),
    fail_on_call: None,
  };
  let cancellation = std::sync::Arc::new(crate::input_cancellation::InputCancellation::default());
  cancellation.cancel();
  let error = crate::input_cancellation::with_input_cancellation(cancellation, || {
    input.scroll_motion(
      &test_window(),
      WindowPoint::new(10.0, 20.0),
      &motion(Scroll::new(0.0, 120.0), 500, TimingFunction::Linear, 60),
      ScrollOptions::default(),
      &mut |_| {},
    )
  })
  .unwrap_err();
  assert!(error.to_string().contains("cancelled"));
  assert!(input.calls.borrow().is_empty());
}
