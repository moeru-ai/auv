use super::*;

#[test]
fn display_selector_matches_native_id_and_compat_display_ref() {
  let targets = vec![display_target(0, "100", "display_0", false)];

  let by_native = resolve_display_target(&targets, Some("100")).unwrap();
  let by_ref = resolve_display_target(&targets, Some("display_0")).unwrap();

  assert_eq!(by_native.display.id, "100");
  assert_eq!(by_ref.display.name.as_deref(), Some("display_0"));
}

#[test]
fn display_selector_defaults_to_primary_display() {
  let targets = vec![
    display_target(0, "100", "display_0", false),
    display_target(1, "200", "display_1", true),
  ];

  let resolved = resolve_display_target(&targets, None).unwrap();

  assert_eq!(resolved.display.id, "200");
}

#[test]
fn display_region_resolution_requires_contained_global_region() {
  let targets = vec![display_target(0, "100", "display_0", true)];

  let resolved = resolve_display_for_global_region(&targets, None, Rect::new(10.0, 20.0, 40.0, 50.0)).unwrap();
  let outside = resolve_display_for_global_region(&targets, None, Rect::new(10.0, 20.0, 2000.0, 50.0));

  assert_eq!(resolved.display.id, "100");
  assert!(matches!(outside, Err(DriverError::NotFound { .. })));
}

fn display_target(index: usize, native_id: &str, display_ref: &str, is_primary: bool) -> MacosDisplayTarget {
  MacosDisplayTarget {
    index,
    display: Display {
      id: native_id.to_string(),
      name: Some(display_ref.to_string()),
      frame: Rect::new(0.0, 0.0, 1000.0, 800.0),
      coordinate_space: CoordinateSpace::Screen,
      scale_factor: 2.0,
      is_primary,
      is_builtin: Some(false),
    },
  }
}

#[cfg(all(target_os = "macos", feature = "macos-xcap"))]
#[test]
fn empty_display_monitor_list_reports_permission_context() {
  let error = display_targets_from_monitors(&[]).expect_err("empty xcap monitor list should explain the likely permission boundary");

  match error {
    DriverError::PermissionDenied {
      permission,
      message,
      recovery,
    } => {
      assert_eq!(permission, "screen_recording");
      assert_eq!(message, None);
      assert!(recovery.as_deref().is_some_and(|message| message.contains("auv doctor --json")));
    }
    other => panic!("unexpected error: {other}"),
  }
}

// ROOT CAUSE:
//
// If Mission Control showed the target window, ScreenCaptureKit captured it
// at its shrunk frame (955x558 pt for a 1644x960 pt window) and the driver
// derived `scale_factor` from the resolved frame instead, returning a black
// image with scale 1.17.
//
// The fix compares the captured frame with the resolved one, and asks the
// application (AX) whether a mismatch is a real resize.
#[test]
fn captured_frame_is_transformed_while_the_application_keeps_its_size() {
  let ax = || {
    Some(WindowAxSize {
      size: Size::new(1644.0, 960.0),
      minimized: false,
    })
  };

  assert_eq!(
    classify_captured_frame(Size::new(1644.0, 960.0), Size::new(955.0, 558.0), ax),
    CapturedWindowFrame::Transformed { minimized: false }
  );
}

#[test]
fn captured_frame_is_a_resize_when_the_application_agrees_with_it() {
  let ax = || {
    Some(WindowAxSize {
      size: Size::new(1056.0, 752.0),
      minimized: false,
    })
  };

  assert_eq!(classify_captured_frame(Size::new(1644.0, 960.0), Size::new(1056.0, 752.0), ax), CapturedWindowFrame::Resized);
}

#[test]
fn captured_frame_of_a_minimized_window_is_transformed() {
  let ax = || {
    Some(WindowAxSize {
      size: Size::new(1644.0, 965.0),
      minimized: true,
    })
  };

  assert_eq!(
    classify_captured_frame(Size::new(1644.0, 965.0), Size::new(101.0, 45.0), ax),
    CapturedWindowFrame::Transformed { minimized: true }
  );
}

#[test]
fn captured_frame_matches_up_to_rounding_without_asking_the_application() {
  let ax = || -> Option<WindowAxSize> { panic!("AX is only consulted on a mismatch") };

  assert_eq!(classify_captured_frame(Size::new(1644.0, 960.0), Size::new(1644.5, 959.5), ax), CapturedWindowFrame::Resolved);
  assert_eq!(classify_captured_frame(Size::new(0.0, 0.0), Size::new(1644.0, 960.0), ax), CapturedWindowFrame::Resolved);
}

#[test]
fn captured_frame_without_application_size_is_treated_as_transformed() {
  assert_eq!(
    classify_captured_frame(Size::new(1644.0, 960.0), Size::new(1056.0, 752.0), || None),
    CapturedWindowFrame::Transformed { minimized: false }
  );
}
