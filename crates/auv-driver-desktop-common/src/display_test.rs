use super::*;
use auv_driver_common::CoordinateSpace;

#[test]
fn display_resolution_prefers_primary() {
  let targets = vec![
    DisplayTarget {
      index: 0,
      display: display("left", false),
    },
    DisplayTarget {
      index: 0,
      display: display("primary", true),
    },
  ];

  let selected = resolve_target(&targets, None).expect("display resolves");

  assert_eq!(selected.display.id, "primary");
}

fn display(id: &str, is_primary: bool) -> Display {
  Display {
    id: id.to_string(),
    name: None,
    frame: Rect::new(0.0, 0.0, 100.0, 100.0),
    coordinate_space: CoordinateSpace::Screen,
    scale_factor: 1.0,
    is_primary,
    is_builtin: None,
  }
}

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
fn resolve_target_prefers_primary_without_selector() {
  let targets = vec![
    target(0, "a", Rect::new(0.0, 0.0, 100.0, 100.0), false),
    target(1, "b", Rect::new(100.0, 0.0, 100.0, 100.0), true),
  ];

  let resolved = resolve_target(&targets, None).expect("primary should resolve");

  assert_eq!(resolved.index, 1);
}

#[test]
fn resolve_target_matches_selector_by_id_or_name() {
  let targets = vec![
    target(0, "a", Rect::new(0.0, 0.0, 100.0, 100.0), true),
    target(1, "b", Rect::new(100.0, 0.0, 100.0, 100.0), false),
  ];

  assert_eq!(resolve_target(&targets, Some("b")).unwrap().index, 1);
  assert_eq!(resolve_target(&targets, Some("display_1")).unwrap().index, 1);
  assert!(resolve_target(&targets, Some("missing")).is_err());
}

#[test]
fn resolve_for_region_selects_containing_display() {
  let targets = vec![
    target(0, "a", Rect::new(0.0, 0.0, 100.0, 100.0), true),
    target(1, "b", Rect::new(100.0, 0.0, 100.0, 100.0), false),
  ];

  let region = Rect::new(110.0, 10.0, 20.0, 20.0);
  let resolved = resolve_for_region(&targets, None, region).expect("region is within display b");

  assert_eq!(resolved.index, 1);
}

#[test]
fn resolve_for_region_rejects_region_spanning_displays() {
  let targets = vec![target(0, "a", Rect::new(0.0, 0.0, 100.0, 100.0), true)];

  let region = Rect::new(50.0, 50.0, 100.0, 10.0);

  assert!(resolve_for_region(&targets, None, region).is_err());
}
