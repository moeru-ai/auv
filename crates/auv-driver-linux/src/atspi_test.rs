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
    active: true,
    bounds: Rect::new(1.0, 2.0, 300.0, 400.0),
  };

  let window = window_from_accessible(&app, &accessible, true);

  assert_eq!(window.title.as_deref(), Some("Settings"));
  assert_eq!(window.app_name.as_deref(), Some("gnome-control-center"));
  assert_eq!(window.application_id.as_deref(), Some("org.gnome.Settings"));
  assert!(window.reference.id.starts_with(WINDOW_REF_PREFIX));
}

fn listed_window(app_name: &str, frame: Rect) -> Window {
  Window {
    reference: WindowRef {
      id: format!("{WINDOW_REF_PREFIX}:1.1/window"),
    },
    title: Some("window".to_string()),
    app_name: Some(app_name.to_string()),
    application_id: None,
    process_id: None,
    frame,
    coordinate_space: CoordinateSpace::Screen,
    is_main: false,
    is_visible: rect_has_area(frame),
  }
}

// ROOT CAUSE:
//
// `list_windows` walked the whole GNOME Shell "Main stage" tree on every call,
// even when no listed window needed its origin repaired: 2,000 nodes at about
// nine D-Bus calls each (17,289 messages, 8 to 10 seconds on GNOME Shell 50.4).
//
// The fix collects the frames that need repair first and skips the walk when
// none do; when a repair is needed it walks only the shell's window containers
// (64 nodes) instead of the whole stage. This test pins the gate.
// ROOT CAUSE:
//
// `find_window_groups` can return a partial list: the search budget can run
// out, a container can be missing, or a name read can fail. Only a complete
// pair may narrow the candidate set; a partial one can turn an ambiguous match
// into a wrong one.
#[test]
fn stage_repair_roots_require_both_window_containers() {
  let stage = ObjectRef {
    dest: "org.gnome.Shell".to_string(),
    path: "/stage".to_string(),
  };
  let window_group = ObjectRef {
    dest: "org.gnome.Shell".to_string(),
    path: "/window-group".to_string(),
  };
  let top_group = ObjectRef {
    dest: "org.gnome.Shell".to_string(),
    path: "/top-window-group".to_string(),
  };

  assert_eq!(stage_repair_roots(&stage, Vec::new()), vec![stage.clone()]);
  assert_eq!(stage_repair_roots(&stage, vec![window_group.clone()]), vec![stage.clone()]);
  assert_eq!(stage_repair_roots(&stage, vec![window_group.clone(), top_group.clone()]), vec![window_group, top_group]);
}

#[test]
fn stage_origin_repair_is_skipped_when_every_window_has_a_usable_origin() {
  let windows = [
    listed_window("gnome-shell", Rect::new(0.0, 55.0, 133.0, 56.0)),
    listed_window("ghostty", Rect::new(5192.0, 55.0, 800.0, 1077.0)),
  ];

  assert!(stage_repair_frames(&windows).is_empty());
}

#[test]
fn stage_origin_repair_collects_only_zero_origin_app_windows() {
  let repair = listed_window("ghostty", Rect::new(0.0, 0.0, 800.0, 1077.0));
  let shell = listed_window("gnome-shell", Rect::new(0.0, 0.0, 2752.0, 1152.0));
  let collapsed = listed_window("ghostty", Rect::new(0.0, 0.0, 0.0, 0.0));

  assert_eq!(stage_repair_frames(&[repair.clone(), shell, collapsed]), vec![repair.frame]);
}

// ROOT CAUSE:
//
// The origin repair walks the shell's window containers instead of the whole
// stage. gnome-shell keeps window actors in two actors: "Window group" and
// "Top window group" (override-redirect windows). Discovering only the first
// one would drop override-redirect windows from the candidate set.
//
// This live test pins both names and the discovery contract; it needs a GNOME
// session and is ignored by default.
#[test]
#[ignore = "requires a live GNOME Shell Wayland desktop"]
fn live_gnome_shell_exposes_both_window_groups() {
  let connection = connect().expect("a11y bus connects");
  let root = ObjectRef {
    dest: REGISTRY_DEST.to_string(),
    path: ROOT_PATH.to_string(),
  };
  let shell = children(&connection, &root)
    .expect("registry children")
    .into_iter()
    .filter_map(|reference| application(&connection, reference).transpose())
    .collect::<DriverResult<Vec<_>>>()
    .expect("applications")
    .into_iter()
    .find(|application| application.name == "gnome-shell")
    .expect("gnome-shell exposes an AT-SPI application");
  let stage = children(&connection, &shell.reference)
    .expect("shell children")
    .into_iter()
    .find(|child| accessible(&connection, child.clone()).is_ok_and(|node| node.name == "Main stage"))
    .expect("Main stage is exposed");

  let groups = find_window_groups(&connection, &stage).expect("window group discovery");

  assert_eq!(groups.len(), 2, "Window group and Top window group are both exposed");
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

// ROOT CAUSE:
//
// If a caller asked whether an AT-SPI node was active or focused, the state
// check looked for the state number inside `GetState`'s reply, but that reply
// is a bitfield of two u32 words. ACTIVE (1) or FOCUSED (12) matched only by
// accident, so the focused terminal was not recognized for Ctrl+Shift+V.
//
// The fix tests bit `n % 32` of word `n / 32`.
#[test]
fn state_set_is_a_bitfield_of_two_words() {
  let active_and_focused = [(1 << 1) | (1 << 12), 0];
  assert!(state_set_contains(&active_and_focused, STATE_ACTIVE));
  assert!(state_set_contains(&active_and_focused, STATE_FOCUSED));
  assert!(!state_set_contains(&[1, 0], STATE_ACTIVE));
  assert!(state_set_contains(&[0, 1 << 2], 34));
}
