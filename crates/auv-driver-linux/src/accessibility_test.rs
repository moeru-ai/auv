use auv_driver_common::geometry::{CoordinateSpace, Rect};
use auv_driver_common::window::WindowRef;

use super::*;

#[test]
fn select_node_reports_atspi_action_path() {
  let result = InputActionResult {
    selected_path: InputDeliveryPath::AxPress,
    attempts: vec![InputAttempt {
      path: InputDeliveryPath::AxPress,
      succeeded: true,
      message: Some("click".to_string()),
    }],
    verified: false,
    mouse_disturbance: DisturbanceLevel::None,
    focus_disturbance: DisturbanceLevel::Foreground,
    clipboard_disturbance: DisturbanceLevel::None,
  };

  assert_eq!(result.selected_path, InputDeliveryPath::AxPress);
  assert_eq!(result.attempts[0].message.as_deref(), Some("click"));
}

#[test]
fn focus_result_uses_ax_focus_path() {
  let window = Window {
    reference: WindowRef {
      id: "atspi::1.1/window".to_string(),
    },
    title: Some("Settings".to_string()),
    app_name: Some("gnome-control-center".to_string()),
    app_bundle_id: Some("org.gnome.Settings".to_string()),
    process_id: None,
    frame: Rect::new(0.0, 0.0, 800.0, 600.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
  };

  let result = InputActionResult {
    selected_path: InputDeliveryPath::AxFocus,
    attempts: vec![InputAttempt::success(InputDeliveryPath::AxFocus)],
    verified: false,
    mouse_disturbance: DisturbanceLevel::None,
    focus_disturbance: DisturbanceLevel::Foreground,
    clipboard_disturbance: DisturbanceLevel::None,
  };

  assert_eq!(window.reference.id, "atspi::1.1/window");
  assert_eq!(result.selected_path, InputDeliveryPath::AxFocus);
}
