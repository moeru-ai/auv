use crate::SidebarScrollbarBoundary;
use crate::view_parsers::sidebar::ax::vertical_scrollbar_boundary_from_nodes;

#[cfg(target_os = "macos")]
#[test]
fn vertical_scrollbar_boundary_prefers_page_button_height() {
  let nodes = vec![
    observed_node("0.0.1", "AXScrollBar", "", 480),
    observed_node("0.0.1.3", "AXButton", "AXIncrementPage", 0),
    observed_node("0.0.1.4", "AXButton", "AXDecrementPage", 24),
  ];

  assert_eq!(vertical_scrollbar_boundary_from_nodes(&nodes, &nodes[0]), Some(SidebarScrollbarBoundary::Bottom));
}

#[cfg(target_os = "macos")]
fn observed_node(path: &str, role: &str, subrole: &str, height: i64) -> auv_driver_macos::types::ObservedAxNode {
  auv_driver_macos::types::ObservedAxNode {
    depth: path.split('.').count(),
    path: path.to_string(),
    role: role.to_string(),
    subrole: subrole.to_string(),
    title: String::new(),
    description: String::new(),
    help: String::new(),
    identifier: String::new(),
    placeholder: String::new(),
    value: String::new(),
    focused: false,
    bounds: auv_driver_macos::types::ObservedRect {
      x: 272,
      y: 260,
      width: 18,
      height,
    },
  }
}
