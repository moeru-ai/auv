use super::super::capture_store::CaptureStoreOptions;
use super::*;
use std::cell::Cell;

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
  CaptureState::new(target, CaptureResolution::Native, Some(windows.clone()), Some(windows)).unwrap()
}

fn stored() -> (CaptureStore, proto::CaptureRef, CaptureState) {
  let store = CaptureStore::new(CaptureStoreOptions::default());
  let capture_id = store.insert(capture());
  let state = state(CaptureTarget::Window(window()), vec![window()]);
  store.set_click_state(&capture_id, Some(state.clone()));
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
  current: Result<(Capture, Option<CaptureState>), Status>,
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
      calls.set(calls.get() + 1);
      Ok(())
    },
  )
}

fn reason(status: Status) -> Reason {
  assert_eq!(status.code(), tonic::Code::FailedPrecondition);
  assert!(status.message().contains("take a new capture"));
  proto::ClickPreconditionFailure::decode(status.details()).unwrap().reason()
}

#[test]
fn unchanged_capture_permits_exactly_one_delivery() {
  let (store, reference, state) = stored();
  let calls = Cell::new(0);
  attempt(&store, &reference, &window(), Ok((capture(), Some(state))), &calls).unwrap();
  assert_eq!(calls.get(), 1);
}

#[test]
fn window_movement_refuses_without_delivery() {
  let (store, reference, _) = stored();
  let mut moved = window();
  moved.frame.origin.x += 1.0;
  let current = state(CaptureTarget::Window(moved.clone()), vec![moved.clone()]);
  let calls = Cell::new(0);
  assert_eq!(reason(attempt(&store, &reference, &moved, Ok((capture(), Some(current))), &calls).unwrap_err()), Reason::StateChanged);
  assert_eq!(calls.get(), 0);
}

#[test]
fn window_resize_refuses_without_delivery() {
  let (store, reference, _) = stored();
  let mut resized = window();
  resized.frame.size.width += 1.0;
  let current = state(CaptureTarget::Window(resized.clone()), vec![resized.clone()]);
  let calls = Cell::new(0);
  assert_eq!(reason(attempt(&store, &reference, &resized, Ok((capture(), Some(current))), &calls).unwrap_err()), Reason::StateChanged);
  assert_eq!(calls.get(), 0);
}

#[test]
fn scroll_or_navigation_pixels_refuse_without_delivery() {
  let (store, reference, state) = stored();
  let mut changed = capture();
  changed.image.put_pixel(5, 5, image::Rgba([200, 0, 0, 255]));
  let calls = Cell::new(0);
  assert_eq!(reason(attempt(&store, &reference, &window(), Ok((changed, Some(state))), &calls).unwrap_err()), Reason::StateChanged);
  assert_eq!(calls.get(), 0);
}

#[test]
fn separate_dialog_refuses_even_when_window_pixels_are_unchanged() {
  let (store, reference, _) = stored();
  let mut dialog = window();
  dialog.reference.id = "dialog".into();
  dialog.title = Some("Confirm".into());
  let current = state(CaptureTarget::Window(window()), vec![dialog, window()]);
  let calls = Cell::new(0);
  assert_eq!(reason(attempt(&store, &reference, &window(), Ok((capture(), Some(current))), &calls).unwrap_err()), Reason::StateChanged);
  assert_eq!(calls.get(), 0);
}

#[test]
fn failed_recapture_and_unstable_state_refuse_without_delivery() {
  let (store, reference, _) = stored();
  let calls = Cell::new(0);
  let error = attempt(&store, &reference, &window(), Err(Status::permission_denied("capture denied")), &calls).unwrap_err();
  assert_eq!(reason(error), Reason::CheckUnavailable);
  let error = attempt(&store, &reference, &window(), Ok((capture(), None)), &calls).unwrap_err();
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
    reason(attempt(&store, &unknown, &window(), Ok((capture(), Some(state.clone()))), &calls).unwrap_err()),
    Reason::CaptureUnavailable
  );
  let mut wrong = window();
  wrong.reference.id = "other".into();
  assert_eq!(reason(attempt(&store, &reference, &wrong, Ok((capture(), Some(state))), &calls).unwrap_err()), Reason::TargetMismatch);
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
    reason(attempt(&store, &unbound, &window(), Ok((capture(), Some(state.clone()))), &calls).unwrap_err()),
    Reason::CheckUnavailable
  );
  assert_eq!(calls.get(), 0);
  attempt(&store, &reference, &window(), Ok((capture(), Some(state))), &calls).unwrap();
  assert_eq!(calls.get(), 1);
}

#[test]
fn capture_baseline_rejects_a_window_change_during_capture() {
  let mut moved = window();
  moved.frame.origin.y += 2.0;
  assert!(CaptureState::new(CaptureTarget::Window(window()), CaptureResolution::Native, Some(vec![window()]), Some(vec![moved])).is_none());
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
  store.set_click_state(&reference.capture_id, Some(state.clone()));
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
      |_| Ok((current, Some(state.clone()))),
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
  root.in_scope(|| attempt(&store, &reference, &window(), Ok((changed, Some(state))), &calls)).unwrap_err();
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
  assert_eq!(calls.get(), 0);
}
