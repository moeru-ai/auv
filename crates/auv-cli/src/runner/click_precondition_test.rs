use super::super::capture_store::CaptureStoreOptions;
use super::*;
use std::cell::Cell;

// https://github.com/moeru-ai/auv/pull/429#discussion_r4386288123
// ROOT CAUSE:
//
// Cancellation during recapture was missed because admission checked it only
// while acquiring the desktop reservation. Delivery must check it again.
#[test]
fn cancellation_during_recapture_stops_delivery_pr_429() {
  use auv_driver::input_cancellation::{InputCancellation, with_input_cancellation};
  let cancellation = std::sync::Arc::new(InputCancellation::default());
  let (store, reference, state) = stored();
  let calls = Cell::new(0);
  let result = with_input_cancellation(cancellation.clone(), || {
    deliver(
      &store,
      Some(&reference),
      &Position {
        point: auv_driver::Point::new(5.0, 5.0),
        coordinate_space: window().coordinate_space,
      },
      Some(&window()),
      ScreenPoint::new(15.0, 25.0),
      |_| {
        cancellation.cancel();
        Ok((capture(), Ok(state)))
      },
      || {
        calls.set(calls.get() + 1);
        Ok(())
      },
    )
  });
  assert_eq!(calls.get(), 0, "cancelled admission must not invoke native input");
  assert_eq!(result.unwrap_err().code(), tonic::Code::Cancelled);
}

// https://github.com/moeru-ai/auv/pull/429
// ROOT CAUSE:
//
// A guarded click checked pixels before waiting for the driver's desktop lock.
// Another input could change the UI during that wait, leaving admitted input stale.
// Admission must wait for the desktop before recapture and retain it through delivery.
#[test]
fn guarded_click_checks_after_desktop_queue_wait_pr_429() {
  use std::sync::atomic::{AtomicBool, Ordering};
  use std::sync::{Arc, mpsc};
  use std::time::Duration;

  let desktop = auv_driver::mouse_input::reserve_desktop_input().unwrap();
  let changed = Arc::new(AtomicBool::new(false));
  let worker_changed = changed.clone();
  let (started_tx, started_rx) = mpsc::channel();
  let (captured_tx, captured_rx) = mpsc::channel();
  let worker = std::thread::spawn(move || {
    let (store, reference, state) = stored();
    let calls = Cell::new(0);
    started_tx.send(()).unwrap();
    let result = deliver(
      &store,
      Some(&reference),
      &Position {
        point: auv_driver::Point::new(5.0, 5.0),
        coordinate_space: window().coordinate_space,
      },
      Some(&window()),
      ScreenPoint::new(15.0, 25.0),
      |_| {
        let mut pixels = capture();
        if worker_changed.load(Ordering::SeqCst) {
          pixels.image.put_pixel(5, 5, image::Rgba([200, 0, 0, 255]));
        }
        captured_tx.send(()).unwrap();
        Ok((pixels, Ok(state)))
      },
      || {
        // Use the real shared reservation, but never call a native input backend.
        let _desktop = auv_driver::mouse_input::reserve_desktop_input().unwrap();
        calls.set(calls.get() + 1);
        Ok(())
      },
    );
    (result, calls.get())
  });
  started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
  // Give the worker a chance to expose the old check-before-lock ordering.
  let captured_while_occupied = captured_rx.recv_timeout(Duration::from_millis(100)).is_ok();
  changed.store(true, Ordering::SeqCst);
  drop(desktop);
  let (result, calls) = worker.join().unwrap();
  assert_eq!(calls, 0, "a queued click must not deliver from stale evidence");
  assert!(!captured_while_occupied, "recapture must wait for desktop admission");
  assert_eq!(reason(result.unwrap_err()), Reason::StateChanged);
}

fn window() -> Window {
  Window {
    reference: auv_driver::WindowRef {
      id: "window-1".into(),
    },
    title: Some("Editor".into()),
    app_name: Some("Editor".into()),
    application_id: None,
    process_id: Some(42),
    frame: auv_driver::Rect::new(10.0, 20.0, 40.0, 30.0),
    coordinate_space: CoordinateSpace::Window("window-1".into()),
    is_main: true,
    is_visible: true,
  }
}

fn capture() -> Capture {
  Capture {
    origin: Some(Position {
      point: auv_driver::Point::new(0.0, 0.0),
      coordinate_space: window().coordinate_space,
    }),
    image: image::RgbaImage::from_pixel(40, 30, image::Rgba([30, 40, 50, 255])),
    bounds: window().frame,
    scale_factor: 1.0,
    backend: "fixture".into(),
    fallback_reason: None,
  }
}

fn state(target: CaptureTarget, windows: Vec<Window>) -> CaptureState {
  CaptureState::new(target, CaptureResolution::Native, Ok(windows.clone()), Ok(windows)).unwrap()
}

fn stored() -> (CaptureStore, proto::CaptureRef, CaptureState) {
  let store = CaptureStore::new(CaptureStoreOptions::default());
  let capture_id = store.insert(capture());
  let state = state(CaptureTarget::Window(window()), vec![window()]);
  store.set_click_state(&capture_id, Ok(state.clone()));
  (store, proto::CaptureRef { capture_id }, state)
}

// ROOT CAUSE:
//
// Input used earlier pixels without any state comparison. Window coordinate
// conversion could not detect resizing, navigation or an intervening dialog.
// The admission boundary now completes every check before it calls delivery.
fn attempt(
  store: &CaptureStore,
  reference: &proto::CaptureRef,
  target: &Window,
  current: Result<(Capture, Result<CaptureState, String>), Status>,
  calls: &Cell<usize>,
) -> Result<(), Status> {
  deliver(
    store,
    Some(reference),
    &Position {
      point: auv_driver::Point::new(5.0, 5.0),
      coordinate_space: target.coordinate_space.clone(),
    },
    Some(target),
    ScreenPoint::new(target.frame.origin.x + 5.0, target.frame.origin.y + 5.0),
    |_| current,
    || {
      // Native click implementations share the admission reservation on this thread.
      let _desktop = auv_driver::mouse_input::reserve_desktop_input().unwrap();
      calls.set(calls.get() + 1);
      Ok(())
    },
  )
}

fn reason(status: Status) -> Reason {
  assert_eq!(status.code(), tonic::Code::FailedPrecondition);
  assert!(status.message().contains("take a new capture"));
  auv_api_proto::click_precondition::failure(&status).unwrap().reason()
}

#[test]
fn unchanged_capture_permits_exactly_one_delivery() {
  let (store, reference, state) = stored();
  let calls = Cell::new(0);
  attempt(&store, &reference, &window(), Ok((capture(), Ok(state))), &calls).unwrap();
  assert_eq!(calls.get(), 1);
}

// https://github.com/moeru-ai/auv/pull/429
#[test]
fn cancelled_guard_stops_before_recapture_or_delivery_pr_429() {
  use auv_driver::input_cancellation::{InputCancellation, with_input_cancellation};
  let (store, reference, _) = stored();
  let cancellation = std::sync::Arc::new(InputCancellation::default());
  cancellation.cancel();
  let error = with_input_cancellation(cancellation, || {
    deliver(
      &store,
      Some(&reference),
      &Position {
        point: auv_driver::Point::new(5.0, 5.0),
        coordinate_space: window().coordinate_space,
      },
      Some(&window()),
      ScreenPoint::new(15.0, 25.0),
      |_| panic!("cancelled admission must not recapture"),
      || -> Result<(), Status> { panic!("cancelled admission must not deliver input") },
    )
  })
  .unwrap_err();
  assert!(error.message().contains("cancelled"), "{error}");
}

// https://github.com/moeru-ai/auv/pull/429#discussion_r4382693014
// Capture indicators outside the target must not invalidate its baseline.
#[test]
fn unrelated_window_churn_does_not_refuse_target_clicks() {
  let (store, reference, _) = stored();
  let mut indicator = window();
  indicator.reference.id = "recording-indicator".into();
  indicator.frame = auv_driver::Rect::new(900.0, 0.0, 20.0, 20.0);
  let current =
    CaptureState::new(CaptureTarget::Window(window()), CaptureResolution::Native, Ok(vec![window()]), Ok(vec![indicator, window()]));
  let calls = Cell::new(0);
  attempt(&store, &reference, &window(), Ok((capture(), current)), &calls).unwrap();
  assert_eq!(calls.get(), 1);
}

// https://github.com/moeru-ai/auv/pull/429#discussion_r4382693049
#[test]
fn recapture_failure_preserves_the_backend_cause() {
  let (store, reference, _) = stored();
  let calls = Cell::new(0);
  let error = attempt(&store, &reference, &window(), Err(Status::permission_denied("capture permission denied")), &calls).unwrap_err();
  assert!(error.message().contains("capture permission denied"));
  assert_eq!(calls.get(), 0);
}

// https://github.com/moeru-ai/auv/pull/429#discussion_r4382693034
#[test]
fn atspi_crop_cannot_prove_window_location() {
  let (store, _, state) = stored();
  let mut pixels = capture();
  pixels.backend = "atspi.extents+pipewire.crop".into();
  let reference = proto::CaptureRef {
    capture_id: store.insert(pixels.clone()),
  };
  store.set_click_state(&reference.capture_id, Ok(state.clone()));
  let calls = Cell::new(0);
  let error = attempt(&store, &reference, &window(), Ok((pixels, Ok(state))), &calls).unwrap_err();
  assert_eq!(reason(error), Reason::CheckUnavailable);
  assert_eq!(calls.get(), 0);
}

#[test]
fn window_movement_refuses_without_delivery() {
  let (store, reference, _) = stored();
  let mut moved = window();
  moved.frame.origin.x += 1.0;
  let current = state(CaptureTarget::Window(moved.clone()), vec![moved.clone()]);
  let calls = Cell::new(0);
  assert_eq!(reason(attempt(&store, &reference, &moved, Ok((capture(), Ok(current))), &calls).unwrap_err()), Reason::StateChanged);
  assert_eq!(calls.get(), 0);
}

#[test]
fn window_resize_refuses_without_delivery() {
  let (store, reference, _) = stored();
  let mut resized = window();
  resized.frame.size.width += 1.0;
  let current = state(CaptureTarget::Window(resized.clone()), vec![resized.clone()]);
  let calls = Cell::new(0);
  assert_eq!(reason(attempt(&store, &reference, &resized, Ok((capture(), Ok(current))), &calls).unwrap_err()), Reason::StateChanged);
  assert_eq!(calls.get(), 0);
}

#[test]
fn scroll_or_navigation_pixels_refuse_without_delivery() {
  let (store, reference, state) = stored();
  let mut changed = capture();
  changed.image.put_pixel(5, 5, image::Rgba([200, 0, 0, 255]));
  let calls = Cell::new(0);
  assert_eq!(reason(attempt(&store, &reference, &window(), Ok((changed, Ok(state))), &calls).unwrap_err()), Reason::StateChanged);
  assert_eq!(calls.get(), 0);
}

// https://github.com/moeru-ai/auv/pull/429#discussion_r4386560659
#[test]
fn separate_dialog_refusal_names_the_added_window_pr_429() {
  let (store, reference, _) = stored();
  let mut dialog = window();
  dialog.reference.id = "dialog".into();
  dialog.title = Some("Confirm".into());
  let current = state(CaptureTarget::Window(window()), vec![dialog, window()]);
  let calls = Cell::new(0);
  let error = attempt(&store, &reference, &window(), Ok((capture(), Ok(current))), &calls).unwrap_err();
  assert!(error.message().contains("added=1, removed=0, modified=0, order_changed=false"), "{error}");
  assert!(error.message().contains("first difference: id=\"dialog\", app=\"Editor\""), "{error}");
  assert_eq!(reason(error), Reason::StateChanged);
  assert_eq!(calls.get(), 0);
}

// https://github.com/moeru-ai/auv/pull/429#discussion_r4386560659
// ROOT CAUSE:
//
// Window-list differences refused input without identifying the changed windows.
// The cause must summarize the relevant snapshots without changing admission.
#[test]
fn unstable_capture_reports_window_difference_counts_pr_429() {
  let (store, reference, mut baseline) = stored();
  let mut removed = window();
  removed.reference.id = "temporary-script".into();
  removed.app_name = Some("osascript".into());
  let mut modified = window();
  modified.reference.id = "utility".into();
  baseline.before.extend([removed, modified.clone()]);
  modified.title = Some("New title".into());
  let mut added = window();
  added.reference.id = "dialog".into();
  baseline.after.extend([added, modified]);
  store.set_click_state(&reference.capture_id, Ok(baseline));
  let calls = Cell::new(0);
  let error = attempt(&store, &reference, &window(), Err(Status::internal("recapture must not run")), &calls).unwrap_err();
  assert!(error.message().contains("windows intersecting the target changed during capture"), "{error}");
  assert!(error.message().contains("added=1, removed=1, modified=1, order_changed=false"), "{error}");
  assert!(error.message().contains("first difference: id=\"temporary-script\", app=\"osascript\""), "{error}");
  assert_eq!(reason(error), Reason::CheckUnavailable);
  assert_eq!(calls.get(), 0);
}

// https://github.com/moeru-ai/auv/pull/429#discussion_r4386560659
#[test]
fn window_order_only_refusal_reports_order_change_pr_429() {
  let (store, reference, _) = stored();
  let mut other = window();
  other.reference.id = "utility".into();
  let baseline = state(CaptureTarget::Window(window()), vec![window(), other.clone()]);
  store.set_click_state(&reference.capture_id, Ok(baseline));
  let current = state(CaptureTarget::Window(window()), vec![other, window()]);
  let calls = Cell::new(0);
  let error = attempt(&store, &reference, &window(), Ok((capture(), Ok(current))), &calls).unwrap_err();
  assert!(error.message().contains("added=0, removed=0, modified=0, order_changed=true"), "{error}");
  assert!(error.message().contains("first difference: id=\"window-1\""), "{error}");
  assert_eq!(reason(error), Reason::StateChanged);
  assert_eq!(calls.get(), 0);
}

// https://github.com/moeru-ai/auv/pull/429#discussion_r4386560659
#[tokio::test]
async fn recapture_window_diagnostic_is_bounded_and_recorded_pr_429() {
  use auv_tracing::{Context, MemoryTracingStore, RunId, TraceRecord, configure, dispatcher};
  let records = std::sync::Arc::new(MemoryTracingStore::new());
  let dispatch = configure().tracing_store(records.clone()).build().unwrap();
  let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
  let (store, reference, mut current) = stored();
  let mut added = window();
  added.reference.id = "界".repeat(4096);
  added.app_name = Some(format!("Utility\n{}", "应".repeat(4096)));
  added.title = Some("private document title".into());
  current.after.push(added);
  let calls = Cell::new(0);
  let error = root.in_scope(|| attempt(&store, &reference, &window(), Ok((capture(), Ok(current))), &calls)).unwrap_err();
  dispatch.flush().await.unwrap();
  let payload = records
    .records()
    .into_iter()
    .find_map(|record| match record {
      TraceRecord::Event {
        schema, payload, ..
      } if schema.name().as_str() == "auv.input.click_refused" => Some(serde_json::from_str::<serde_json::Value>(payload.get()).unwrap()),
      _ => None,
    })
    .expect("refusal event");
  let cause = payload["cause"].as_str().unwrap();
  assert!(cause.contains("added=1, removed=0, modified=0, order_changed=false"), "{cause}");
  assert!(cause.contains(&format!("id=\"{}…\"", "界".repeat(80))), "{cause}");
  assert!(cause.contains("app=\"Utility\\n"), "{cause}");
  assert!(!cause.contains('\n'));
  assert!(!cause.contains("private document title"));
  assert!(cause.chars().count() <= 512);
  assert!(error.message().contains(cause));
  assert_eq!(payload["input_delivered"], false);
  assert_eq!(reason(error), Reason::CheckUnavailable);
  assert_eq!(calls.get(), 0);
}

#[test]
fn failed_recapture_and_unstable_state_refuse_without_delivery() {
  let (store, reference, _) = stored();
  let calls = Cell::new(0);
  let error = attempt(&store, &reference, &window(), Err(Status::permission_denied("capture denied")), &calls).unwrap_err();
  assert_eq!(reason(error), Reason::CheckUnavailable);
  let error = attempt(&store, &reference, &window(), Ok((capture(), Err("baseline unavailable".into()))), &calls).unwrap_err();
  assert_eq!(reason(error), Reason::CheckUnavailable);
  assert_eq!(calls.get(), 0);
}

#[test]
fn unknown_capture_and_wrong_window_refuse_without_delivery() {
  let (store, reference, state) = stored();
  let calls = Cell::new(0);
  let unknown = proto::CaptureRef {
    capture_id: "another-runner".into(),
  };
  assert_eq!(
    reason(attempt(&store, &unknown, &window(), Ok((capture(), Ok(state.clone()))), &calls).unwrap_err()),
    Reason::CaptureUnavailable
  );
  let mut wrong = window();
  wrong.reference.id = "other".into();
  assert_eq!(reason(attempt(&store, &reference, &wrong, Ok((capture(), Ok(state))), &calls).unwrap_err()), Reason::TargetMismatch);
  assert_eq!(calls.get(), 0);
}

#[test]
fn provenance_is_per_reference_even_when_pixels_are_deduplicated() {
  let (store, reference, state) = stored();
  let unbound = proto::CaptureRef {
    capture_id: store.insert(capture()),
  };
  let calls = Cell::new(0);
  assert_eq!(
    reason(attempt(&store, &unbound, &window(), Ok((capture(), Ok(state.clone()))), &calls).unwrap_err()),
    Reason::CheckUnavailable
  );
  assert_eq!(calls.get(), 0);
  attempt(&store, &reference, &window(), Ok((capture(), Ok(state))), &calls).unwrap();
  assert_eq!(calls.get(), 1);
}

#[test]
fn capture_baseline_rejects_a_window_change_during_capture() {
  let mut moved = window();
  moved.frame.origin.y += 2.0;
  assert!(CaptureState::new(CaptureTarget::Window(window()), CaptureResolution::Native, Ok(vec![window()]), Ok(vec![moved])).is_err());
}

#[test]
fn display_capture_checks_pixels_and_the_capture_extent() {
  let store = CaptureStore::new(CaptureStoreOptions::default());
  let mut pixels = capture();
  pixels.origin = Some(Position {
    point: auv_driver::Point::new(0.0, 0.0),
    coordinate_space: CoordinateSpace::Display("display-1".into()),
  });
  let display = Display {
    id: "display-1".into(),
    name: None,
    frame: pixels.bounds,
    coordinate_space: CoordinateSpace::Display("display-1".into()),
    scale_factor: 1.0,
    is_primary: true,
    is_builtin: None,
  };
  let state = state(CaptureTarget::Display(display), vec![window()]);
  let reference = proto::CaptureRef {
    capture_id: store.insert(pixels.clone()),
  };
  store.set_click_state(&reference.capture_id, Ok(state.clone()));
  let calls = Cell::new(0);
  let position = Position {
    point: auv_driver::Point::new(15.0, 25.0),
    coordinate_space: CoordinateSpace::Screen,
  };
  let check = |current: Capture, point: ScreenPoint| {
    deliver(
      &store,
      Some(&reference),
      &position,
      None,
      point,
      |_| Ok((current, Ok(state.clone()))),
      || {
        calls.set(calls.get() + 1);
        Ok(())
      },
    )
  };
  check(pixels.clone(), ScreenPoint::new(15.0, 25.0)).unwrap();
  assert_eq!(calls.get(), 1);
  pixels.image.put_pixel(5, 5, image::Rgba([0, 0, 0, 255]));
  assert_eq!(reason(check(pixels.clone(), ScreenPoint::new(15.0, 25.0)).unwrap_err()), Reason::StateChanged);
  assert_eq!(reason(check(pixels, ScreenPoint::new(500.0, 25.0)).unwrap_err()), Reason::TargetMismatch);
  assert_eq!(calls.get(), 1);
}

#[tokio::test]
async fn refusal_is_recorded_in_the_run_without_delivery() {
  use auv_tracing::{Context, MemoryTracingStore, RunId, TraceRecord, configure, dispatcher};
  let records = std::sync::Arc::new(MemoryTracingStore::new());
  let dispatch = configure().tracing_store(records.clone()).build().unwrap();
  let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
  let (store, reference, state) = stored();
  let mut changed = capture();
  changed.image.put_pixel(0, 0, image::Rgba([0, 0, 0, 255]));
  let calls = Cell::new(0);
  root.in_scope(|| attempt(&store, &reference, &window(), Ok((changed, Ok(state))), &calls)).unwrap_err();
  dispatch.flush().await.unwrap();
  let payload = records
    .records()
    .into_iter()
    .find_map(|record| match record {
      TraceRecord::Event {
        schema, payload, ..
      } if schema.name().as_str() == "auv.input.click_refused" => Some(serde_json::from_str::<serde_json::Value>(payload.get()).unwrap()),
      _ => None,
    })
    .expect("refusal event");
  assert_eq!(payload["capture_id"], reference.capture_id);
  assert_eq!(payload["reason"], "CLICK_PRECONDITION_FAILURE_REASON_STATE_CHANGED");
  assert_eq!(payload["recovery"], "new_capture");
  assert_eq!(payload["input_delivered"], false);
  assert_eq!(payload["cause"], "captured pixels or capture metadata changed (exact comparison)");
  assert_eq!(calls.get(), 0);
}

// https://github.com/moeru-ai/auv/pull/429#discussion_r4382693049
#[test]
fn missing_baseline_preserves_the_original_window_list_failure() {
  let (store, reference, _) = stored();
  let baseline = CaptureState::new(
    CaptureTarget::Window(window()),
    CaptureResolution::Native,
    Err("accessibility permission denied".into()),
    Ok(vec![window()]),
  );
  store.set_click_state(&reference.capture_id, baseline);
  let calls = Cell::new(0);
  let error = attempt(&store, &reference, &window(), Err(Status::internal("recapture must not run")), &calls).unwrap_err();
  assert!(error.message().contains("window list before capture: accessibility permission denied"));
  assert_eq!(calls.get(), 0);
}

// https://github.com/moeru-ai/auv/pull/429#discussion_r4382693014
#[test]
fn display_baseline_ignores_metadata_churn_away_from_the_click_but_refuses_an_occluder() {
  let store = CaptureStore::new(CaptureStoreOptions::default());
  let pixels = capture();
  let display = CaptureTarget::Display(Display {
    id: "display".into(),
    name: None,
    frame: pixels.bounds,
    coordinate_space: CoordinateSpace::Display("display".into()),
    scale_factor: 1.0,
    is_primary: true,
    is_builtin: None,
  });
  let mut other = window();
  other.reference.id = "indicator".into();
  other.frame = auv_driver::Rect::new(40.0, 40.0, 5.0, 5.0);
  let baseline = CaptureState::new(display.clone(), CaptureResolution::Native, Ok(vec![window()]), Ok(vec![other.clone(), window()]));
  let reference = proto::CaptureRef {
    capture_id: store.insert(pixels.clone()),
  };
  store.set_click_state(&reference.capture_id, baseline);
  let calls = Cell::new(0);
  let position = Position {
    point: auv_driver::Point::new(15.0, 25.0),
    coordinate_space: CoordinateSpace::Screen,
  };
  let check = |windows| {
    deliver(
      &store,
      Some(&reference),
      &position,
      None,
      ScreenPoint::new(15.0, 25.0),
      |_| Ok((pixels.clone(), Ok(state(display.clone(), windows)))),
      || {
        calls.set(calls.get() + 1);
        Ok(())
      },
    )
  };
  check(vec![window()]).unwrap();
  assert_eq!(calls.get(), 1);
  other.frame = window().frame;
  assert_eq!(reason(check(vec![other, window()]).unwrap_err()), Reason::StateChanged);
  assert_eq!(calls.get(), 1, "the occluded click sent no input");
}
