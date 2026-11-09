use super::*;

#[test]
fn display_resolution_prefers_primary() {
  let targets = vec![
    DisplayTarget {
      display: display("left", false),
    },
    DisplayTarget {
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
