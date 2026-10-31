use super::*;

#[test]
fn rect_from_edges_converts_inclusive_exclusive_edges_to_origin_size() {
  let rect = rect_from_edges(100, 200, 340, 500);

  assert_eq!(rect, Rect::new(100.0, 200.0, 240.0, 300.0));
}

#[test]
fn rect_from_edges_handles_zero_area() {
  let rect = rect_from_edges(10, 20, 10, 20);

  assert_eq!(rect, Rect::new(10.0, 20.0, 0.0, 0.0));
}

#[test]
fn child_indices_parse_snapshot_path() {
  assert_eq!(child_indices("0/2/0/0/1/3/0").unwrap(), [2, 0, 0, 1, 3, 0]);
}

#[test]
fn child_indices_reject_invalid_root_and_child() {
  assert!(child_indices("1/2").is_err());
  assert!(child_indices("0/search").is_err());
}

// Live smoke test: snapshot the first enumerated top-level window and prove
// the UIA COM walk produces at least the root node with the expected root
// path. Skips cleanly when no windows are present (headless session).
#[cfg(target_os = "windows")]
#[test]
fn snapshot_window_captures_root_node_for_a_live_window() {
  let windows = crate::window::list_windows().expect("list windows");
  let Some(window) = windows.into_iter().next() else {
    return;
  };

  let snapshot = snapshot_window(&window).expect("snapshot window ax tree");

  assert_eq!(snapshot.window_ref, window.reference.id);
  assert!(!snapshot.nodes.is_empty());
  assert_eq!(snapshot.nodes[0].depth, 0);
  assert_eq!(snapshot.nodes[0].path, "0");
}

// --- focus-target resolution kernel: synthetic fixtures only. ---

use auv_driver_common::accessibility::{AxTextSelector, FocusTextOptions};
use auv_driver_common::error::DriverError;

fn fixture_node(path: &str, depth: usize, control_type: &str, name: &str, value: Option<&str>, automation_id: &str) -> AxNode {
  AxNode {
    depth,
    path: path.to_string(),
    control_type: control_type.to_string(),
    role: control_type.to_string(),
    name: name.to_string(),
    value: value.map(str::to_string),
    automation_id: automation_id.to_string(),
    class_name: String::new(),
    focused: false,
    enabled: true,
    keyboard_focusable: true,
    offscreen: false,
    bounds: Rect::new(0.0, 0.0, 100.0, 20.0),
  }
}

fn fixture_snapshot(nodes: Vec<AxNode>) -> AxTreeSnapshot {
  AxTreeSnapshot {
    window_ref: "fixture-window".to_string(),
    nodes,
    truncated: false,
  }
}

fn query_options(query: &str) -> FocusTextOptions {
  FocusTextOptions {
    app: "fixture.exe".into(),
    selector: AxTextSelector::Query(query.to_string()),
    expected_role: None,
  }
}

fn path_selector(path: &str) -> FocusTextOptions {
  FocusTextOptions {
    app: "fixture.exe".into(),
    selector: AxTextSelector::Path(path.to_string()),
    expected_role: None,
  }
}

#[test]
fn an_empty_or_whitespace_query_is_invalid_input() {
  let snapshot = fixture_snapshot(vec![fixture_node("0", 0, "Edit", "Search", None, "id")]);
  for query in ["", "   "] {
    let error = resolve_focus_target(&snapshot, &query_options(query)).unwrap_err();
    assert!(matches!(error, DriverError::InvalidInput { .. }), "{query:?}: {error:?}");
  }
  let error = resolve_focus_target(&snapshot, &path_selector("")).unwrap_err();
  assert!(matches!(error, DriverError::InvalidInput { .. }), "{error:?}");
}

#[test]
fn an_unmatched_query_answers_not_found_without_relaxing_the_role_filter() {
  // macOS's ranker falls back to a role-agnostic rescan; this kernel must not:
  // the query only matches the group, so the Edit filter stays a NotFound.
  let snapshot = fixture_snapshot(vec![
    fixture_node("0", 0, "Group", "Settings panel", None, "panel"),
    fixture_node("0/0", 1, "Group", "Search field area", None, "area"),
  ]);
  let error = resolve_focus_target(
    &snapshot,
    &FocusTextOptions {
      app: "fixture.exe".into(),
      selector: AxTextSelector::Query("Search".to_string()),
      expected_role: Some("Edit".to_string()),
    },
  )
  .unwrap_err();
  assert!(matches!(error, DriverError::NotFound { .. }), "{error:?}");
}

#[test]
fn a_query_matches_the_name_case_insensitively_and_reports_the_identity() {
  let snapshot = fixture_snapshot(vec![fixture_node(
    "0/1",
    1,
    "Edit",
    "Recipient email",
    Some("drafts@auv.dev"),
    "to-field",
  )]);
  let resolved = resolve_focus_target(&snapshot, &query_options("recipient")).expect("the name carries the query");
  assert_eq!(resolved, &snapshot.nodes[0]);
}

#[test]
fn the_name_outranks_the_value_and_the_automation_id() {
  let snapshot = fixture_snapshot(vec![
    fixture_node("0/0", 1, "Edit", "matches the value only", Some("needle"), "second"),
    fixture_node("0/1", 1, "Edit", "needle", None, "first"),
    // The automation id carries the query too, at weight 40: it identifies a
    // control a caller saw before, but the visible name outranks it.
    fixture_node("0/2", 1, "Edit", "unrelated caption", None, "needle-by-id"),
  ]);
  let resolved = resolve_focus_target(&snapshot, &query_options("needle")).expect("all three match; the name weighs 80");
  assert_eq!(resolved.path, "0/1");
}

#[test]
fn an_exact_match_outranks_a_containing_match() {
  let snapshot = fixture_snapshot(vec![
    fixture_node("0/0", 1, "Edit", "Save all changes", None, "a"),
    fixture_node("0/1", 1, "Edit", "Save", None, "b"),
  ]);
  let resolved = resolve_focus_target(&snapshot, &query_options("save")).expect("both contain the query");
  assert_eq!(resolved.path, "0/1", "the exact match gains the +20 bonus");
}

#[test]
fn shallower_nodes_outrank_equally_scoring_deeper_ones() {
  let snapshot = fixture_snapshot(vec![
    fixture_node("0/0/0", 2, "Edit", "Save", None, "deep"),
    fixture_node("0/1", 1, "Edit", "Save", None, "shallow"),
  ]);
  let resolved = resolve_focus_target(&snapshot, &query_options("save")).expect("two equal-name candidates");
  assert_eq!(resolved.path, "0/1", "the ranker subtracts depth");
}

#[test]
fn equal_scores_refuse_independently_of_snapshot_order() {
  let snapshot = fixture_snapshot(vec![
    fixture_node("0/0", 1, "Edit", "Save", None, "first"),
    fixture_node("0/1", 1, "Edit", "Save", None, "second"),
  ]);
  assert!(matches!(resolve_focus_target(&snapshot, &query_options("save")), Err(DriverError::InvalidInput { .. })));
  let mut reversed = snapshot;
  reversed.nodes.reverse();
  assert!(matches!(resolve_focus_target(&reversed, &query_options("save")), Err(DriverError::InvalidInput { .. })));
}

#[test]
fn an_edit_control_outranks_an_equally_scoring_group() {
  let snapshot = fixture_snapshot(vec![
    fixture_node("0/0", 1, "Group", "Save", None, "container"),
    fixture_node("0/1", 1, "Edit", "Save", None, "field"),
  ]);
  let resolved = resolve_focus_target(&snapshot, &query_options("save")).expect("two name matches");
  assert_eq!(resolved.path, "0/1", "the UIA role bias favors the editable control");
}

#[test]
fn the_expected_role_filter_is_case_insensitive_and_independent_of_scoring() {
  let snapshot = fixture_snapshot(vec![
    fixture_node("0/0", 1, "edit", "Save", None, "field"),
    fixture_node("0/1", 1, "Button", "Save changes", None, "button"),
  ]);
  let resolved = resolve_focus_target(
    &snapshot,
    &FocusTextOptions {
      app: "fixture.exe".into(),
      selector: AxTextSelector::Query("save".to_string()),
      expected_role: Some(" EDIT ".to_string()),
    },
  )
  .expect("the role filter runs before scoring");
  assert_eq!(resolved.path, "0/0");
  assert_eq!(resolved.control_type, "edit");
}

#[test]
fn zero_area_nodes_are_not_candidates() {
  let snapshot = fixture_snapshot(vec![AxNode {
    bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
    ..fixture_node("0/0", 1, "Edit", "Save", None, "hidden")
  }]);
  let error = resolve_focus_target(&snapshot, &query_options("save")).unwrap_err();
  assert!(matches!(error, DriverError::NotFound { .. }), "{error:?}");
}

#[test]
fn a_path_selector_resolves_exactly_and_fails_closed() {
  // The miss doubles as the stale-node case of the determinism contract: a
  // path an earlier snapshot returned, absent from the current capture,
  // answers NotFound — never a silent substitute.
  let snapshot = fixture_snapshot(vec![fixture_node(
    "0/2",
    1,
    "Edit",
    "Unrelated caption",
    None,
    "field",
  )]);
  let resolved = resolve_focus_target(&snapshot, &path_selector("0/2")).expect("the path exists");
  assert_eq!(resolved.name, "Unrelated caption");
  let error = resolve_focus_target(&snapshot, &path_selector("0/9")).unwrap_err();
  assert!(matches!(error, DriverError::NotFound { .. }), "{error:?}");
  assert!(error.to_string().contains("UIA candidate path 0/9"), "{error}");
}

// ROOT CAUSE:
//
// Track B selected the first equal-score node, so tree order could choose a
// different control. A semantic focus query must reject an ambiguous winner.
#[test]
fn ambiguous_focus_query_is_rejected_before_delivery() {
  let snapshot = fixture_snapshot(vec![
    fixture_node("0/0", 1, "Edit", "Search", None, "first"),
    fixture_node("0/1", 1, "Edit", "Search", None, "second"),
  ]);
  assert!(matches!(resolve_focus_target(&snapshot, &query_options("Search")), Err(DriverError::InvalidInput { .. })));
}

// ROOT CAUSE:
//
// Track B applied expected_role only to queries. An explicit path could
// therefore select a different role after the UI tree changed.
#[test]
fn candidate_path_preserves_the_expected_role() {
  let snapshot = fixture_snapshot(vec![fixture_node("0/0", 1, "Button", "Search", None, "button")]);
  let options = FocusTextOptions {
    app: "fixture.exe".into(),
    selector: AxTextSelector::Path("0/0".into()),
    expected_role: Some("Edit".into()),
  };
  assert!(matches!(resolve_focus_target(&snapshot, &options), Err(DriverError::RoleMismatch { .. })));
}

// ROOT CAUSE:
//
// Track B filtered empty bounds only for queries. A path to a hidden node
// bypassed that condition and could enter native focus delivery.
#[test]
fn candidate_path_rejects_unusable_bounds() {
  let snapshot = fixture_snapshot(vec![AxNode {
    bounds: Rect::default(),
    ..fixture_node("0/0", 1, "Edit", "Search", None, "field")
  }]);
  assert!(matches!(resolve_focus_target(&snapshot, &path_selector("0/0")), Err(DriverError::NotFound { .. })));
}

#[test]
fn incomplete_tree_cannot_establish_an_unambiguous_focus_target() {
  let mut snapshot = fixture_snapshot(vec![fixture_node("0/0", 1, "Edit", "Search", None, "field")]);
  snapshot.truncated = true;
  assert!(matches!(resolve_focus_target(&snapshot, &query_options("Search")), Err(DriverError::StaleUiReference { .. })));
}

#[test]
fn disabled_nonfocusable_and_offscreen_nodes_are_not_selected() {
  let node = fixture_node("0/0", 1, "Edit", "Search", None, "field");
  for unusable in [
    AxNode {
      enabled: false,
      ..node.clone()
    },
    AxNode {
      keyboard_focusable: false,
      ..node.clone()
    },
    AxNode {
      offscreen: true,
      ..node.clone()
    },
    AxNode {
      bounds: Rect::new(f64::NAN, 0.0, 100.0, 20.0),
      ..node.clone()
    },
    AxNode {
      bounds: Rect::new(0.0, 0.0, f64::INFINITY, 20.0),
      ..node
    },
  ] {
    let snapshot = fixture_snapshot(vec![unusable]);
    assert!(matches!(resolve_focus_target(&snapshot, &query_options("Search")), Err(DriverError::NotFound { .. })));
    assert!(matches!(resolve_focus_target(&snapshot, &path_selector("0/0")), Err(DriverError::NotFound { .. })));
  }
}

#[test]
fn changed_target_refuses_focus_without_delivery() {
  let selected = fixture_node("0/0", 1, "Edit", "Search", None, "original");
  let mut changed = selected.clone();
  changed.automation_id = "replacement".into();
  let delivered = std::cell::Cell::new(0);
  let result = focus_selected(
    &selected,
    || Ok(changed.clone()),
    || {
      delivered.set(delivered.get() + 1);
      Ok(())
    },
  );
  assert!(matches!(result, Err(DriverError::StaleUiReference { .. })));
  assert_eq!(delivered.get(), 0);
}

#[test]
fn native_identity_failure_refuses_focus_without_delivery() {
  let selected = fixture_node("0/0", 1, "Edit", "Search", None, "field");
  let delivered = std::cell::Cell::new(0);
  let result = focus_selected(
    &selected,
    || {
      Err(DriverError::StaleUiReference {
        message: "provider identity changed".into(),
        recovery: None,
      })
    },
    || {
      delivered.set(delivered.get() + 1);
      Ok(())
    },
  );
  assert!(matches!(&result, Err(DriverError::StaleUiReference { .. })));
  assert_eq!(delivered.get(), 0);
}

#[test]
fn focus_readback_accepts_mutable_ui_properties_when_identity_and_focus_hold() {
  let selected = fixture_node("0/0", 1, "Edit", "Search", None, "field");
  let mut after = selected.clone();
  after.focused = true;
  after.name = "Search results".into();
  after.value = Some("updated on focus".into());
  after.bounds = Rect::new(0.0, 40.0, 100.0, 20.0);
  let reads = std::cell::Cell::new(0);
  let delivered = std::cell::Cell::new(0);
  // Both reads model a native callback that has already confirmed the same
  // UIA element identity. Only presentation attributes change after SetFocus.
  let result = focus_selected(
    &selected,
    || {
      let previous = reads.replace(reads.get() + 1);
      Ok(if previous == 0 {
        selected.clone()
      } else {
        after.clone()
      })
    },
    || {
      delivered.set(delivered.get() + 1);
      Ok(())
    },
  )
  .expect("focus readback must not require frozen presentation attributes");
  assert!(result.verified);
  assert_eq!(reads.get(), 2);
  assert_eq!(delivered.get(), 1);
}

#[test]
fn post_focus_native_identity_failure_is_not_accepted_or_retried() {
  let selected = fixture_node("0/0", 1, "Edit", "Search", None, "field");
  let reads = std::cell::Cell::new(0);
  let delivered = std::cell::Cell::new(0);
  let result = focus_selected(
    &selected,
    || {
      let previous = reads.replace(reads.get() + 1);
      if previous == 0 {
        Ok(selected.clone())
      } else {
        // Native CompareElements rejected a replacement at the same path.
        Err(DriverError::StaleUiReference {
          message: "UIA focus path now identifies a different control".into(),
          recovery: None,
        })
      }
    },
    || {
      delivered.set(delivered.get() + 1);
      Ok(())
    },
  );
  assert!(matches!(result, Err(DriverError::StaleUiReference { .. })));
  let error = result.unwrap_err().to_string();
  assert!(error.contains("Focus delivery was attempted. The final state is uncertain."), "{error}");
  assert!(error.contains("Inspect focus before retrying"), "{error}");
  assert_eq!(reads.get(), 2);
  assert_eq!(delivered.get(), 1);
}

#[test]
fn setfocus_success_without_focus_readback_is_not_verified_or_retried() {
  let selected = fixture_node("0/0", 1, "Edit", "Search", None, "field");
  let delivered = std::cell::Cell::new(0);
  let result = focus_selected(
    &selected,
    || Ok(selected.clone()),
    || {
      delivered.set(delivered.get() + 1);
      Ok(())
    },
  );
  assert!(matches!(result, Err(DriverError::Backend { .. })));
  assert_eq!(delivered.get(), 1);
}

#[test]
fn focus_delivery_error_preserves_the_category_without_retry() {
  let selected = fixture_node("0/0", 1, "Edit", "Search", None, "field");
  let reads = std::cell::Cell::new(0);
  let delivered = std::cell::Cell::new(0);
  let result = focus_selected(
    &selected,
    || {
      reads.set(reads.get() + 1);
      Ok(selected.clone())
    },
    || {
      delivered.set(delivered.get() + 1);
      Err(DriverError::PermissionDenied {
        permission: "UI Automation",
        message: None,
        recovery: None,
      })
    },
  );
  assert!(matches!(result, Err(DriverError::PermissionDenied { .. })));
  assert_eq!(delivered.get(), 1);
  assert_eq!(reads.get(), 1);
}

#[test]
fn confirmed_focus_reports_one_ax_delivery_with_no_pointer_or_clipboard_effect() {
  let selected = fixture_node("0/0", 1, "Edit", "Search", None, "field");
  let delivered = std::cell::Cell::new(0);
  let action = focus_selected(
    &selected,
    || {
      Ok(AxNode {
        focused: delivered.get() > 0,
        ..selected.clone()
      })
    },
    || {
      delivered.set(delivered.get() + 1);
      Ok(())
    },
  )
  .expect("native focus readback agrees");
  assert_eq!(delivered.get(), 1);
  assert!(action.verified);
  assert_eq!(action.selected_path, InputDeliveryPath::AxFocus);
  assert_eq!(action.attempts, vec![InputAttempt::success(InputDeliveryPath::AxFocus)]);
  assert_eq!(action.mouse_disturbance, DisturbanceLevel::None);
  assert_eq!(action.clipboard_disturbance, DisturbanceLevel::None);
}

#[test]
fn raw_candidate_paths_are_refused_before_any_native_window_lookup() {
  let result = focus_text(FocusTextOptions {
    app: "this-process-must-not-be-looked-up.exe".into(),
    selector: AxTextSelector::Path("0/0".into()),
    expected_role: None,
  });
  assert!(matches!(result, Err(DriverError::Unsupported { .. })));
}

#[test]
fn focus_roles_are_independent_of_localized_tree_labels() {
  let mut node = fixture_node("0/0", 1, "Edit", "Search", None, "field");
  node.control_type = "编辑".into();
  let snapshot = fixture_snapshot(vec![node]);
  let mut options = query_options("Search");
  options.expected_role = Some("Edit".into());
  let selected = resolve_focus_target(&snapshot, &options).expect("programmatic role matches on any display language");
  assert_eq!(selected.role, "Edit");
  assert_eq!(selected.control_type, "编辑");
}
