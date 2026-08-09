use auv_driver_common::window::WindowRef;
use auv_driver_common::{CoordinateSpace, Point, ReadinessStatus, Rect, Size};

use super::*;

fn permissions() -> WindowsPermissionProbe {
  WindowsPermissionProbe {
    elevated: PermissionStatus::Missing,
    ui_access: PermissionStatus::Missing,
    interactive_session: PermissionStatus::Granted,
  }
}

fn window(id: &str, title: &str, frame: Rect, is_main: bool) -> Window {
  Window {
    reference: WindowRef { id: id.to_string() },
    title: Some(title.to_string()),
    app_name: Some("notepad.exe".to_string()),
    app_bundle_id: None,
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
    app_bundle_id: None,
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
fn readiness_passes_when_session_window_and_frontmost_match() {
  let target = window("11", "Untitled", input().expected_window_frame.unwrap(), true);

  let report = assess_readiness(&permissions(), std::slice::from_ref(&target), Some(&target), &input());

  assert!(report.is_ready());
  assert_eq!(report.target_window_ref.as_deref(), Some("11"));
  assert!(report.selected_blocker.is_none());
}

#[test]
fn readiness_blocks_non_interactive_session_before_input_delivery() {
  let target = window("11", "Untitled", input().expected_window_frame.unwrap(), true);
  let mut permissions = permissions();
  permissions.interactive_session = PermissionStatus::Missing;

  let report = assess_readiness(&permissions, std::slice::from_ref(&target), Some(&target), &input());

  assert!(!report.is_ready());
  assert_eq!(report.status, ReadinessStatus::NotReady);
  assert!(report.selected_blocker.as_deref().is_some_and(|reason| reason.contains("Session 0")));
}

#[test]
fn readiness_does_not_block_on_non_elevated_or_missing_ui_access() {
  // ROOT CAUSE: elevation and UIAccess are diagnostic, not blockers; a
  // standard non-elevated process without UIAccess is the normal automation
  // posture and must still report ready.
  let target = window("11", "Untitled", input().expected_window_frame.unwrap(), true);

  let report = assess_readiness(&permissions(), std::slice::from_ref(&target), Some(&target), &input());

  assert!(report.is_ready());
}

#[test]
fn readiness_blocks_window_drift() {
  let mut actual_frame = input().expected_window_frame.unwrap();
  actual_frame.origin.x += 20.0;
  let target = window("11", "Untitled", actual_frame, true);

  let report = assess_readiness(&permissions(), &[target.clone()], Some(&target), &input());

  assert!(!report.is_ready());
  assert!(report.selected_blocker.as_deref().is_some_and(|reason| reason.contains("drift")));
}

#[test]
fn readiness_blocks_missing_expected_window_frame() {
  let target = window(
    "11",
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
fn readiness_blocks_frontmost_mismatch() {
  let target = window("11", "Untitled", input().expected_window_frame.unwrap(), false);
  let other = window("12", "Other", Rect::new(0.0, 0.0, 200.0, 200.0), true);

  let report = assess_readiness(&permissions(), &[target.clone(), other.clone()], Some(&other), &input());

  assert!(!report.is_ready());
  assert!(report.selected_blocker.as_deref().is_some_and(|reason| reason.contains("does not match target window")));
}

#[test]
fn readiness_resolves_window_number_without_a_title_filter() {
  let target = window("11", "Some Other Title", input().expected_window_frame.unwrap(), true);
  let mut input = input();
  input.window_title = None;

  let report = assess_readiness(&permissions(), std::slice::from_ref(&target), Some(&target), &input);

  assert!(report.is_ready());
  assert_eq!(report.target_window_ref.as_deref(), Some("11"));
}
