use crate::view_parsers::sidebar::ax::sidebar_ax_scrollbar_boundary;
use crate::{SidebarScrollbarBoundary, ViewBounds};

#[cfg(target_os = "macos")]
#[test]
fn vertical_scrollbar_boundary_prefers_page_button_height_over_plain_scrollbar_geometry() {
  let window = auv_driver::Window {
    reference: auv_driver::WindowRef {
      id: "42".to_string(),
    },
    title: Some("网易云音乐".to_string()),
    app_name: Some("网易云音乐".to_string()),
    app_bundle_id: Some("com.netease.163music".to_string()),
    process_id: Some(42),
    frame: auv_driver::Rect::new(100.0, 200.0, 400.0, 600.0),
    coordinate_space: auv_driver::geometry::CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
  };
  let sidebar_bounds = ViewBounds::new(20.0, 30.0, 160.0, 520.0);
  let nodes = vec![
    auv_driver_macos::types::ObservedAxNode {
      depth: 2,
      path: "0.0.1".to_string(),
      role: "AXScrollBar".to_string(),
      subrole: String::new(),
      title: String::new(),
      description: String::new(),
      help: String::new(),
      identifier: "_NS:1".to_string(),
      placeholder: String::new(),
      value: "0.6".to_string(),
      focused: false,
      bounds: auv_driver_macos::types::ObservedRect {
        x: 272,
        y: 260,
        width: 18,
        height: 480,
      },
    },
    auv_driver_macos::types::ObservedAxNode {
      depth: 3,
      path: "0.0.1.3".to_string(),
      role: "AXButton".to_string(),
      subrole: "AXIncrementPage".to_string(),
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
        height: 0,
      },
    },
    auv_driver_macos::types::ObservedAxNode {
      depth: 3,
      path: "0.0.1.4".to_string(),
      role: "AXButton".to_string(),
      subrole: "AXDecrementPage".to_string(),
      title: String::new(),
      description: String::new(),
      help: String::new(),
      identifier: String::new(),
      placeholder: String::new(),
      value: String::new(),
      focused: false,
      bounds: auv_driver_macos::types::ObservedRect {
        x: 272,
        y: 740,
        width: 18,
        height: 24,
      },
    },
  ];

  assert_eq!(sidebar_ax_scrollbar_boundary(&nodes, &window, sidebar_bounds), Some(SidebarScrollbarBoundary::Bottom));
}
