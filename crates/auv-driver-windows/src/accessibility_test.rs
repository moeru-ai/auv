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
