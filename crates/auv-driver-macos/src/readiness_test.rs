use super::*;
use auv_driver_common::{CoordinateSpace, Point, ReadinessStatus, Size, WindowRef};

fn permissions() -> PermissionProbe {
  PermissionProbe {
    screen_recording: PermissionStatus::Granted,
    screen_capture_kit: PermissionStatus::Granted,
    accessibility: PermissionStatus::Granted,
    automation_to_system_events: PermissionStatus::Granted,
  }
}

fn window(id: &str, bundle: &str, title: &str, frame: Rect, is_main: bool) -> Window {
  Window {
    reference: WindowRef { id: id.to_string() },
    title: Some(title.to_string()),
    app_name: Some("TextEdit".to_string()),
    app_bundle_id: Some(bundle.to_string()),
    process_id: Some(42),
    frame,
    coordinate_space: CoordinateSpace::Screen,
    is_main,
    is_visible: true,
  }
}

fn input() -> ReadinessProbeInput {
  ReadinessProbeInput {
    window_number: Some(11),
    window_title: Some("Untitled".to_string()),
    app_bundle_id: Some("com.apple.TextEdit".to_string()),
    expected_window_frame: Some(Rect {
      origin: Point { x: 100.0, y: 80.0 },
      size: Size {
        width: 500.0,
        height: 300.0,
      },
    }),
    max_window_frame_drift_px: 2.0,
    require_frontmost: true,
    target_window_x: 10.0,
    target_window_y: 20.0,
  }
}

#[test]
fn readiness_passes_when_permissions_window_and_frontmost_match() {
  let target = window("11", "com.apple.TextEdit", "Untitled", input().expected_window_frame.unwrap(), true);

  let report = assess_readiness(&permissions(), std::slice::from_ref(&target), Some(&target), &input());

  assert!(report.is_ready());
  assert_eq!(report.target_window_ref.as_deref(), Some("11"));
  assert!(report.selected_blocker.is_none());
}

#[test]
fn readiness_blocks_missing_accessibility_before_input_delivery() {
  let target = window("11", "com.apple.TextEdit", "Untitled", input().expected_window_frame.unwrap(), true);
  let mut permissions = permissions();
  permissions.accessibility = PermissionStatus::Missing;

  let report = assess_readiness(&permissions, std::slice::from_ref(&target), Some(&target), &input());

  assert!(!report.is_ready());
  assert_eq!(report.status, ReadinessStatus::NotReady);
  assert!(report.selected_blocker.as_deref().is_some_and(|reason| reason.contains("accessibility")));
}

#[test]
fn readiness_blocks_window_drift() {
  let mut actual_frame = input().expected_window_frame.unwrap();
  actual_frame.origin.x += 20.0;
  let target = window("11", "com.apple.TextEdit", "Untitled", actual_frame, true);

  let report = assess_readiness(&permissions(), &[target.clone()], Some(&target), &input());

  assert!(!report.is_ready());
  assert!(report.selected_blocker.as_deref().is_some_and(|reason| reason.contains("drift")));
}

#[test]
fn readiness_blocks_missing_expected_window_frame() {
  let target = window(
    "11",
    "com.apple.TextEdit",
    "Untitled",
    Rect {
      origin: Point { x: 100.0, y: 80.0 },
      size: Size {
        width: 500.0,
        height: 300.0,
      },
    },
    true,
  );
  let mut input = input();
  input.expected_window_frame = None;

  let report = assess_readiness(&permissions(), &[target.clone()], Some(&target), &input);

  assert!(!report.is_ready());
  assert!(report.selected_blocker.as_deref().is_some_and(|reason| reason.contains("expected window frame")));
}

#[test]
fn readiness_resolves_window_number_even_when_bundle_metadata_is_missing() {
  let mut target = window("11", "com.apple.TextEdit", "Untitled", input().expected_window_frame.unwrap(), true);
  target.app_bundle_id = None;

  let report = assess_readiness(&permissions(), std::slice::from_ref(&target), Some(&target), &input());

  assert!(report.is_ready());
  assert_eq!(report.target_window_ref.as_deref(), Some("11"));
}
