use auv_driver_common::Driver;
use auv_driver_common::capture::{Activation, CaptureOptions};
#[cfg(target_os = "windows")]
use auv_driver_common::geometry::RelativeRect;
use auv_driver_common::geometry::{CoordinateSpace, Rect, ScreenPoint, WindowPoint};
use auv_driver_common::input::{ClickOptions, InputPolicy, Scroll, ScrollDeliveryCandidate, ScrollOptions, WaitOptions, WindowInput};
use auv_driver_common::window::{Window, WindowRef};
use auv_driver_common::{InputTarget, KeyboardInput, PressKeysOptions};

use super::{screen_point_for_window_point, window_point_for_screen_point};
use crate::WindowsDriver;

fn session() -> crate::WindowsDriverSession {
  WindowsDriver::new().open_local().expect("session opens")
}

#[test]
fn keyboard_batch_rejects_invalid_tail_before_any_delivery() {
  let actions = vec![
    KeyboardInput::PressKeys {
      options: PressKeysOptions {
        keys: vec!["a".into()],
        ..Default::default()
      },
      policy: InputPolicy::ForegroundPreferred,
    },
    KeyboardInput::PressKeys {
      options: PressKeysOptions {
        keys: vec!["control".into(), "a".into()],
        count: 0,
        ..Default::default()
      },
      policy: InputPolicy::ForegroundPreferred,
    },
  ];

  let error = session().input().input_keyboard(&InputTarget::Foreground, actions, false).unwrap_err();

  assert_eq!(error.progress.action_index, 1);
  assert!(error.progress.completed.is_empty());
  assert_eq!(error.progress.completed_presses, 0);
}

#[test]
fn keyboard_dry_run_accepts_foreground_press_and_text_without_delivery() {
  let actions = vec![
    KeyboardInput::PressKeys {
      options: PressKeysOptions {
        keys: vec!["control".into(), "a".into()],
        ..Default::default()
      },
      policy: InputPolicy::ForegroundPreferred,
    },
    KeyboardInput::TypeText {
      text: "AUV probe".into(),
      options: auv_driver_common::TypeTextOptions {
        policy: InputPolicy::ForegroundPreferred,
        ..Default::default()
      },
    },
  ];

  assert_eq!(session().input().input_keyboard(&InputTarget::Foreground, actions, true).unwrap(), None);
}

fn sample_window() -> Window {
  Window {
    reference: WindowRef {
      id: "42".to_string(),
    },
    title: None,
    app_name: None,
    application_id: None,
    process_id: Some(123),
    frame: Rect::new(100.0, 200.0, 800.0, 600.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
  }
}

#[test]
fn window_point_converts_to_screen_point() {
  let window = sample_window();

  let point = screen_point_for_window_point(&window, WindowPoint::new(25.0, 30.0));

  assert_eq!(point, ScreenPoint::new(125.0, 230.0));
}

#[test]
fn screen_point_converts_to_window_point() {
  let window = sample_window();

  let point = window_point_for_screen_point(&window, ScreenPoint::new(125.0, 230.0));

  assert_eq!(point, WindowPoint::new(25.0, 30.0));
}

#[test]
fn capture_rejects_region_option() {
  let options = CaptureOptions {
    region: Some(Rect::new(0.0, 0.0, 10.0, 10.0)),
    ..CaptureOptions::default()
  };

  assert!(session().display().capture(options).is_err());
}

#[test]
fn capture_rejects_activation_without_app_target() {
  let options = CaptureOptions {
    activation: Activation::ActivateFirst {
      settle: std::time::Duration::from_millis(0),
    },
    ..CaptureOptions::default()
  };

  assert!(session().display().capture(options).is_err());
}

#[test]
fn capture_region_requires_region() {
  assert!(session().display().capture_region(CaptureOptions::default()).is_err());
}

// The failure comes from the Win32 call; other platforms return `Unsupported`.
#[cfg(target_os = "windows")]
#[test]
fn window_click_background_only_fails_for_invalid_window_handle() {
  let window = sample_window();
  let error = session()
    .window()
    .click(
      &window,
      WindowPoint::new(1.0, 1.0),
      ClickOptions {
        policy: InputPolicy::BackgroundOnly,
        ..ClickOptions::default()
      },
    )
    .expect_err("background click posts window messages, so a fake test window handle fails at the Win32 call");

  assert!(error.to_string().contains("ScreenToClient"));
}

// ROOT CAUSE:
//
// If a window click was cancelled before admission, the outer desktop
// reservation returned before the foreground delivery boundary could stamp it.
// Before the fix, callers received a plain error despite zero native delivery.
// The fix keeps the structured not-delivered receipt at the public boundary.
#[test]
fn pre_cancelled_foreground_window_click_reports_not_delivered() {
  use auv_driver_common::input_cancellation::{InputCancellation, with_input_cancellation};
  use auv_driver_common::{DriverError, InputActionProgress};

  let cancellation = std::sync::Arc::new(InputCancellation::default());
  cancellation.cancel();
  let error = with_input_cancellation(cancellation, || {
    session().window().click(
      &sample_window(),
      WindowPoint::new(1.0, 1.0),
      ClickOptions {
        policy: InputPolicy::ForegroundPreferred,
        ..Default::default()
      },
    )
  })
  .unwrap_err();
  assert!(matches!(error, DriverError::InputAction { cause, progress: InputActionProgress::NotDelivered }
    if matches!(cause.as_ref(), DriverError::InvalidInput { message } if message.contains("cancelled"))));
}

// ROOT CAUSE:
//
// If a foreground scroll was already cancelled, desktop admission returned a
// plain error before any input. The caller could not distinguish this known
// zero-delivery refusal from a later failure.
// The fix retains NotDelivered at the public window-scroll boundary.
#[test]
fn pre_cancelled_foreground_window_scroll_reports_not_delivered() {
  use auv_driver_common::input_cancellation::{InputCancellation, with_input_cancellation};
  use auv_driver_common::{DriverError, InputActionProgress};

  let cancellation = std::sync::Arc::new(InputCancellation::default());
  cancellation.cancel();
  let error = with_input_cancellation(cancellation, || {
    session().window().scroll(
      &sample_window(),
      WindowPoint::new(1.0, 1.0),
      Scroll::new(0.0, 100.0),
      ScrollOptions {
        policy: InputPolicy::ForegroundPreferred,
        ..Default::default()
      },
    )
  })
  .unwrap_err();
  assert!(matches!(error, DriverError::InputAction { cause, progress: InputActionProgress::NotDelivered }
    if matches!(cause.as_ref(), DriverError::InvalidInput { message } if message.contains("cancelled"))));
}

#[test]
fn pre_cancelled_window_scroll_motion_reports_not_delivered() {
  use auv_driver_common::input_cancellation::{InputCancellation, with_input_cancellation};
  use auv_driver_common::{DriverError, InputActionProgress, MotionTiming, ScrollMotion, TimingFunction};

  let cancellation = std::sync::Arc::new(InputCancellation::default());
  cancellation.cancel();
  let error = with_input_cancellation(cancellation, || {
    session().window().scroll_motion(
      &sample_window(),
      WindowPoint::new(1.0, 1.0),
      &ScrollMotion {
        total: Scroll::new(0.0, 100.0),
        timing: MotionTiming::FixedDuration {
          duration: std::time::Duration::from_secs(1),
          function: TimingFunction::Linear,
        },
        sample_rate_hz: 4,
      },
      ScrollOptions {
        policy: InputPolicy::ForegroundPreferred,
        ..Default::default()
      },
      &mut |_| {},
    )
  })
  .unwrap_err();
  assert!(matches!(error, DriverError::InputAction { cause, progress: InputActionProgress::NotDelivered }
    if matches!(cause.as_ref(), DriverError::InvalidInput { message } if message.contains("cancelled"))));
}

#[test]
fn pre_cancelled_foreground_window_drag_reports_not_delivered() {
  use auv_driver_common::input_cancellation::{InputCancellation, with_input_cancellation};
  use auv_driver_common::{DriverError, InputActionProgress, MouseButton, MoveMouseRequest, Point};

  let cancellation = std::sync::Arc::new(InputCancellation::default());
  cancellation.cancel();
  let error = with_input_cancellation(cancellation, || {
    session().window().drag(
      &sample_window(),
      MoveMouseRequest::direct(Point::new(1.0, 1.0)),
      MouseButton::Left,
      InputPolicy::ForegroundPreferred,
    )
  })
  .unwrap_err();
  assert!(matches!(error, DriverError::InputAction { cause, progress: InputActionProgress::NotDelivered }
    if matches!(cause.as_ref(), DriverError::InvalidInput { message } if message.contains("cancelled"))));
}

#[test]
fn pre_cancelled_window_scroll_stream_reports_not_delivered() {
  use auv_driver_common::input_cancellation::{InputCancellation, with_input_cancellation};
  use auv_driver_common::{DriverError, InputActionProgress, ScrollStreamControl, ScrollStreamOptions};

  let cancellation = std::sync::Arc::new(InputCancellation::default());
  cancellation.cancel();
  let error = with_input_cancellation(cancellation, || {
    session().window().scroll_stream(
      &sample_window(),
      WindowPoint::new(1.0, 1.0),
      &ScrollStreamControl::new(),
      ScrollStreamOptions {
        sample_rate_hz: 4,
        max_acceleration: None,
        lease: std::time::Duration::from_secs(1),
      },
      ScrollOptions {
        policy: InputPolicy::ForegroundPreferred,
        ..Default::default()
      },
      &mut |_| {},
    )
  })
  .unwrap_err();
  assert!(matches!(error, DriverError::InputAction { cause, progress: InputActionProgress::NotDelivered }
    if matches!(cause.as_ref(), DriverError::InvalidInput { message } if message.contains("cancelled"))));
}

#[test]
fn window_scroll_background_only_fails_for_invalid_window_handle() {
  let window = sample_window();
  let error = session()
    .window()
    .scroll(
      &window,
      WindowPoint::new(1.0, 1.0),
      Scroll::new(0.0, 10.0),
      ScrollOptions {
        policy: InputPolicy::BackgroundOnly,
        ..ScrollOptions::default()
      },
    )
    .expect_err("background scroll posts window messages, so a fake test window handle fails at the Win32 call");

  assert_background_wheel_refusal(error);
}

#[test]
fn window_scroll_attempts_window_targeted_wheel_without_requiring_foreground_candidate() {
  let window = sample_window();
  let error = session()
    .window()
    .scroll(
      &window,
      WindowPoint::new(1.0, 1.0),
      Scroll::new(0.0, 10.0),
      ScrollOptions {
        delivery_strategy: auv_driver_common::input::ScrollDeliveryStrategy {
          candidates: vec![ScrollDeliveryCandidate::WindowTargetedWheel],
        },
        ..ScrollOptions::default()
      },
    )
    .expect_err("the fake test window handle fails the background wheel post, not a missing-ForegroundHid validation error");

  // Fails from the actual background wheel attempt (Win32 call against a fake
  // handle), not from an upfront "needs ForegroundHid" validation error.
  assert_background_wheel_refusal(error);
}

fn assert_background_wheel_refusal(error: auv_driver_common::DriverError) {
  // The candidate loop retains the real failure instead of replacing it with
  // generic Unsupported. These fake-handle tests never submit native input.
  #[cfg(target_os = "windows")]
  assert!(matches!(error,auv_driver_common::DriverError::Backend {message} if message.contains("ScreenToClient")));
  #[cfg(not(target_os = "windows"))]
  assert!(matches!(error,auv_driver_common::DriverError::InputAction {cause,progress:auv_driver_common::InputActionProgress::NotDelivered}
    if matches!(*cause,auv_driver_common::DriverError::Unsupported {..})));
}

fn posted_scroll_options() -> ScrollOptions {
  ScrollOptions {
    policy: InputPolicy::BackgroundPreferred,
    delivery_strategy: auv_driver_common::ScrollDeliveryStrategy {
      candidates: vec![
        ScrollDeliveryCandidate::WindowTargetedWheel,
        ScrollDeliveryCandidate::ForegroundHid,
      ],
    },
    ..Default::default()
  }
}

#[test]
fn posted_scroll_not_delivered_refusal_allows_foreground_candidate() {
  use auv_driver_common::{DriverError, InputActionResult, InputDeliveryPath};
  let calls = std::cell::Cell::new(0);
  let result = super::run_scroll_candidates(
    &posted_scroll_options(),
    || {
      Err(crate::input::not_delivered(DriverError::Backend {
        message: "queue full".into(),
      }))
    },
    || {
      calls.set(calls.get() + 1);
      Ok(InputActionResult::single_success(InputDeliveryPath::ForegroundSystemEvents))
    },
  )
  .unwrap();
  assert_eq!(calls.get(), 1);
  assert_eq!(result.selected_path, InputDeliveryPath::ForegroundSystemEvents);
  assert!(!result.attempts[0].succeeded);
  assert_eq!(result.attempts[0].path, InputDeliveryPath::WindowTargetedWheel);
  assert!(result.validate().is_ok());
}

#[test]
fn posted_scroll_unknown_failure_stops_without_fallback() {
  use auv_driver_common::DriverError;
  let result = super::run_scroll_candidates(
    &posted_scroll_options(),
    || {
      Err(DriverError::Backend {
        message: "missing receipt".into(),
      })
    },
    || panic!("unknown delivery must stop"),
  );
  assert!(matches!(result,Err(DriverError::Backend {message}) if message=="missing receipt"));
}

#[test]
fn posted_scroll_uncertain_failure_retains_original_progress() {
  use auv_driver_common::{DriverError, InputActionProgress, InputActionResult, InputDeliveryPath};
  let result = super::run_scroll_candidates(
    &posted_scroll_options(),
    || {
      Err(DriverError::InputAction {
        cause: Box::new(DriverError::Backend {
          message: "lost native reply".into(),
        }),
        progress: InputActionProgress::DeliveryUncertain {
          action: InputActionResult::single_success(InputDeliveryPath::WindowTargetedWheel),
        },
      })
    },
    || panic!("uncertain delivery must stop"),
  );
  assert!(matches!(result,Err(DriverError::InputAction {cause,progress:InputActionProgress::DeliveryUncertain {action}})
    if matches!(*cause,DriverError::Backend {ref message} if message=="lost native reply") && action.selected_path==InputDeliveryPath::WindowTargetedWheel));
}

#[test]
fn posted_scroll_invalidated_not_delivered_target_is_not_reactivated() {
  use auv_driver_common::{DriverError, InputActionProgress};
  let result = super::run_scroll_candidates(
    &posted_scroll_options(),
    || {
      Err(crate::input::not_delivered(DriverError::TargetLost {
        target: "original window".into(),
        message: "owner changed".into(),
      }))
    },
    || panic!("target loss requires new target selection"),
  );
  assert!(
    matches!(result,Err(DriverError::InputAction {cause,progress:InputActionProgress::NotDelivered}) if matches!(*cause,DriverError::TargetLost {..}))
  );
}

#[test]
fn posted_scroll_cancelled_not_delivered_action_stops() {
  use auv_driver_common::{DriverError, InputActionProgress};
  let result = super::run_scroll_candidates(
    &posted_scroll_options(),
    || {
      Err(crate::input::not_delivered(DriverError::InvalidInput {
        message: "input cancelled".into(),
      }))
    },
    || panic!("cancelled action must stop"),
  );
  assert!(matches!(result,Err(DriverError::InputAction {cause,progress:InputActionProgress::NotDelivered})
    if matches!(*cause,DriverError::InvalidInput {ref message} if message.contains("cancelled"))));
}

#[test]
fn pre_cancelled_background_window_scroll_reports_not_delivered() {
  use auv_driver_common::input_cancellation::{InputCancellation, with_input_cancellation};
  use auv_driver_common::{DriverError, InputActionProgress};
  let cancellation = std::sync::Arc::new(InputCancellation::default());
  cancellation.cancel();
  let error = with_input_cancellation(cancellation, || {
    session().window().scroll(
      &sample_window(),
      WindowPoint::new(1.0, 1.0),
      Scroll::new(100.0, 100.0),
      ScrollOptions {
        policy: InputPolicy::BackgroundOnly,
        ..Default::default()
      },
    )
  })
  .unwrap_err();
  assert!(matches!(error,DriverError::InputAction {cause,progress:InputActionProgress::NotDelivered}
    if matches!(*cause,DriverError::InvalidInput {ref message} if message.contains("cancelled"))));
}

#[test]
fn posted_scroll_refusal_without_fallback_keeps_native_error() {
  use auv_driver_common::{DriverError, InputActionProgress};
  let mut options = posted_scroll_options();
  options.delivery_strategy.candidates = vec![ScrollDeliveryCandidate::WindowTargetedWheel];
  let result = super::run_scroll_candidates(
    &options,
    || {
      Err(crate::input::not_delivered(DriverError::Backend {
        message: "queue quota 1816".into(),
      }))
    },
    || panic!("no foreground candidate"),
  );
  assert!(matches!(result,Err(DriverError::InputAction {cause,progress:InputActionProgress::NotDelivered})
    if matches!(*cause,DriverError::Backend {ref message} if message=="queue quota 1816")));
}

#[test]
fn posted_scroll_foreground_failure_retains_background_attempt() {
  use auv_driver_common::{DriverError, InputActionProgress, InputActionResult, InputDeliveryPath};
  let result = super::run_scroll_candidates(
    &posted_scroll_options(),
    || {
      Err(crate::input::not_delivered(DriverError::Backend {
        message: "queue full".into(),
      }))
    },
    || {
      Err(DriverError::InputAction {
        cause: Box::new(DriverError::Backend {
          message: "native foreground short count".into(),
        }),
        progress: InputActionProgress::DeliveryUncertain {
          action: InputActionResult::single_success(InputDeliveryPath::ForegroundSystemEvents),
        },
      })
    },
  );
  assert!(matches!(result,Err(DriverError::InputAction {cause,progress:InputActionProgress::DeliveryUncertain {action}})
    if matches!(*cause,DriverError::Backend {ref message} if message=="native foreground short count")
      && action.attempts[0].path==InputDeliveryPath::WindowTargetedWheel && !action.attempts[0].succeeded
      && action.selected_path==InputDeliveryPath::ForegroundSystemEvents));
}

#[test]
fn posted_scroll_partial_receipt_stops_before_foreground_fallback() {
  use super::run_scroll_candidates;
  use auv_driver_common::{DriverError, InputActionResult, InputDeliveryPath, ScrollDeliveryStrategy};
  let calls = std::cell::Cell::new(0);
  let options = ScrollOptions {
    policy: InputPolicy::BackgroundPreferred,
    delivery_strategy: ScrollDeliveryStrategy {
      candidates: vec![
        ScrollDeliveryCandidate::WindowTargetedWheel,
        ScrollDeliveryCandidate::ForegroundHid,
      ],
    },
    ..Default::default()
  };
  let result = run_scroll_candidates(
    &options,
    || {
      Err(DriverError::InputAction {
        cause: Box::new(DriverError::Backend {
          message: "horizontal queue refused after vertical acceptance".into(),
        }),
        progress: auv_driver_common::InputActionProgress::Submitted {
          action: InputActionResult::single_success(InputDeliveryPath::WindowTargetedWheel),
        },
      })
    },
    || {
      calls.set(calls.get() + 1);
      Ok(InputActionResult::single_success(InputDeliveryPath::ForegroundSystemEvents))
    },
  );
  assert_eq!(calls.get(), 0, "a submitted axis must never be resent through foreground fallback");
  assert!(matches!(
    result,
    Err(DriverError::InputAction {
      progress: auv_driver_common::InputActionProgress::Submitted { .. },
      ..
    })
  ));
}

// Live smoke tests: poll a real enumerated top-level window for text that can
// never appear, proving the polling loop terminates at the timeout instead of
// hanging, and that `wait_text` surfaces `NotFound` in that case. Skips
// cleanly when no windows are present (headless session).
fn short_wait() -> WaitOptions {
  WaitOptions {
    timeout: std::time::Duration::from_millis(50),
    poll_interval: std::time::Duration::from_millis(10),
  }
}

#[cfg(target_os = "windows")]
#[test]
fn find_text_returns_no_match_for_unmatchable_query_on_a_live_window() {
  let session = session();
  let Some(window) = session.window().list().expect("list windows").into_iter().next() else {
    return;
  };

  let matches = session
    .window()
    .find_text(&window, "auv-driver-windows-find-text-query-that-never-matches", RelativeRect::new(0.0, 0.0, 1.0, 1.0), short_wait())
    .expect("find_text succeeds even without a match");

  assert!(matches.matches.is_empty());
}

#[cfg(target_os = "windows")]
#[test]
fn wait_text_fails_with_not_found_for_unmatchable_query_on_a_live_window() {
  let session = session();
  let Some(window) = session.window().list().expect("list windows").into_iter().next() else {
    return;
  };

  let error = session
    .window()
    .wait_text(&window, "auv-driver-windows-wait-text-query-that-never-matches", RelativeRect::new(0.0, 0.0, 1.0, 1.0), short_wait())
    .expect_err("wait_text fails when the query never matches before the timeout");

  assert!(error.to_string().contains("before timeout"));
}

#[test]
fn background_click_rejects_alt_meta_before_target_activation() {
  let modifiers = auv_driver_common::ClickModifiers {
    meta: true,
    ..Default::default()
  };
  let driver = session();
  let window = sample_window();
  for policy in [
    InputPolicy::BackgroundOnly,
    InputPolicy::BackgroundPreferred,
  ] {
    let error = driver
      .window()
      .click(
        &window,
        WindowPoint::new(1.0, 1.0),
        ClickOptions {
          policy,
          modifiers,
          ..Default::default()
        },
      )
      .unwrap_err();
    assert!(error.to_string().contains("windows background click supports only shift/control"));
  }
}
