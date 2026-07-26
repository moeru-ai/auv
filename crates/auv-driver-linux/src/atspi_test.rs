use super::*;

#[test]
fn atspi_window_ref_roundtrips() {
  let reference = ObjectRef {
    dest: ":1.75".to_string(),
    path: "/org/gnome/Settings/a11y/window".to_string(),
  };

  assert_eq!(ObjectRef::decode(&reference.encode()).unwrap(), reference);
}

#[test]
fn atspi_window_ref_rejects_non_atspi_reference() {
  assert!(ObjectRef::decode("42").is_err());
}

#[test]
fn child_indices_parse_snapshot_paths() {
  let parsed = child_indices("0/12/3").expect("path parses");

  assert_eq!(parsed, vec![12, 3]);
}

#[test]
fn child_indices_reject_paths_outside_root() {
  let error = child_indices("1/2").expect_err("invalid root is rejected");

  assert!(error.to_string().contains("must start at root 0"));
}

#[test]
fn preferred_action_uses_semantic_action_before_first_action() {
  let actions = vec![
    Action {
      index: 0,
      name: "show-menu".to_string(),
    },
    Action {
      index: 1,
      name: "click".to_string(),
    },
  ];

  let selected = preferred_action(&actions).expect("action selected");

  assert_eq!(selected.index, 1);
}

#[test]
fn preferred_action_falls_back_to_first_action() {
  let actions = vec![Action {
    index: 3,
    name: "show-menu".to_string(),
  }];

  let selected = preferred_action(&actions).expect("action selected");

  assert_eq!(selected.index, 3);
}

#[test]
fn window_from_accessible_projects_linux_app_id() {
  let app = Application {
    reference: ObjectRef {
      dest: ":1.1".to_string(),
      path: ROOT_PATH.to_string(),
    },
    name: "gnome-control-center".to_string(),
    accessible_id: "org.gnome.Settings".to_string(),
  };
  let accessible = Accessible {
    reference: ObjectRef {
      dest: ":1.1".to_string(),
      path: "/window".to_string(),
    },
    name: "Settings".to_string(),
    description: String::new(),
    accessible_id: "CcWindow".to_string(),
    role: "window".to_string(),
    child_count: 0,
    focused: true,
    bounds: Rect::new(1.0, 2.0, 300.0, 400.0),
  };

  let window = window_from_accessible(&app, &accessible, true);

  assert_eq!(window.title.as_deref(), Some("Settings"));
  assert_eq!(window.app_name.as_deref(), Some("gnome-control-center"));
  assert_eq!(window.app_bundle_id.as_deref(), Some("org.gnome.Settings"));
  assert!(window.reference.id.starts_with(WINDOW_REF_PREFIX));
}

#[test]
fn matching_stage_origin_uses_unique_same_size_stage_rect() {
  let origin = matching_stage_origin(
    Rect::new(0.0, 0.0, 980.0, 1077.0),
    &[
      Rect::new(1727.0, 30.0, 1030.0, 1127.0),
      Rect::new(1752.0, 55.0, 980.0, 1077.0),
    ],
  );

  assert_eq!(origin, Some(Point::new(1752.0, 55.0)));
}

#[test]
fn matching_stage_origin_rejects_ambiguous_same_size_stage_rects() {
  let origin = matching_stage_origin(
    Rect::new(0.0, 0.0, 980.0, 1077.0),
    &[
      Rect::new(1752.0, 55.0, 980.0, 1077.0),
      Rect::new(20.0, 40.0, 980.0, 1077.0),
    ],
  );

  assert_eq!(origin, None);
}

#[test]
fn matching_stage_origin_deduplicates_nested_same_origin_rects() {
  let origin = matching_stage_origin(
    Rect::new(0.0, 0.0, 980.0, 1077.0),
    &[
      Rect::new(1752.0, 55.0, 980.0, 1077.0),
      Rect::new(1752.1, 55.0, 980.0, 1077.0),
    ],
  );

  assert_eq!(origin, Some(Point::new(1752.0, 55.0)));
}
