use super::*;

fn target(index: usize, id: &str, frame: Rect, is_primary: bool) -> DisplayTarget {
  DisplayTarget {
    index,
    display: Display {
      id: id.to_string(),
      name: Some(format!("display_{index}")),
      frame,
      coordinate_space: CoordinateSpace::Screen,
      scale_factor: 1.0,
      is_primary,
      is_builtin: None,
    },
  }
}

#[test]
fn resolve_display_target_prefers_primary_without_selector() {
  let targets = vec![
    target(0, "a", Rect::new(0.0, 0.0, 100.0, 100.0), false),
    target(1, "b", Rect::new(100.0, 0.0, 100.0, 100.0), true),
  ];

  let resolved = resolve_display_target(&targets, None).expect("primary should resolve");

  assert_eq!(resolved.index, 1);
}

#[test]
fn resolve_display_target_matches_selector_by_id_or_name() {
  let targets = vec![
    target(0, "a", Rect::new(0.0, 0.0, 100.0, 100.0), true),
    target(1, "b", Rect::new(100.0, 0.0, 100.0, 100.0), false),
  ];

  assert_eq!(resolve_display_target(&targets, Some("b")).unwrap().index, 1);
  assert_eq!(resolve_display_target(&targets, Some("display_1")).unwrap().index, 1);
  assert!(resolve_display_target(&targets, Some("missing")).is_err());
}

#[test]
fn resolve_display_for_region_selects_containing_display() {
  let targets = vec![
    target(0, "a", Rect::new(0.0, 0.0, 100.0, 100.0), true),
    target(1, "b", Rect::new(100.0, 0.0, 100.0, 100.0), false),
  ];

  let region = Rect::new(110.0, 10.0, 20.0, 20.0);
  let resolved = resolve_display_for_region(&targets, None, region).expect("region is within display b");

  assert_eq!(resolved.index, 1);
}

#[test]
fn resolve_display_for_region_rejects_region_spanning_displays() {
  let targets = vec![target(0, "a", Rect::new(0.0, 0.0, 100.0, 100.0), true)];

  let region = Rect::new(50.0, 50.0, 100.0, 10.0);

  assert!(resolve_display_for_region(&targets, None, region).is_err());
}

#[test]
fn integral_capture_dimension_rejects_fractional_and_negative() {
  assert!(integral_capture_dimension("x", 10.5).is_err());
  assert!(integral_capture_dimension("x", -1.0).is_err());
  assert_eq!(integral_capture_dimension("x", 12.0).unwrap(), 12);
}

#[test]
fn integral_positive_capture_dimension_rejects_zero() {
  assert!(integral_positive_capture_dimension("width", 0.0).is_err());
  assert_eq!(integral_positive_capture_dimension("width", 4.0).unwrap(), 4);
}
