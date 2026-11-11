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

fn display_target(index: usize, native_id: &str, display_ref: &str, is_primary: bool) -> DisplayTarget {
  DisplayTarget {
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
