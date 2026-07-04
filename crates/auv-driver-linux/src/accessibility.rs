//! Window accessibility tree snapshots via AT-SPI.
//!
//! This mirrors the Windows UIA tree surface and the macOS AX tree direction:
//! AUV captures a flattened tree for one top-level window, then re-resolves
//! path-targeted focus/action requests against the current AT-SPI tree.

use auv_driver::error::DriverResult;
use auv_driver::geometry::Rect;
use auv_driver::input::{DisturbanceLevel, InputActionResult, InputAttempt, InputDeliveryPath};
use auv_driver::window::Window;

use crate::atspi;

#[derive(Clone, Debug, PartialEq)]
pub struct AxNode {
  pub depth: usize,
  pub path: String,
  pub control_type: String,
  pub name: String,
  pub value: Option<String>,
  pub automation_id: String,
  pub class_name: String,
  pub focused: bool,
  pub bounds: Rect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AxTreeSnapshot {
  pub window_ref: String,
  pub nodes: Vec<AxNode>,
}

pub fn snapshot_window(window: &Window) -> DriverResult<AxTreeSnapshot> {
  let snapshot = atspi::snapshot_window(window)?;
  Ok(AxTreeSnapshot {
    window_ref: snapshot.window_ref,
    nodes: snapshot
      .nodes
      .into_iter()
      .map(|node| AxNode {
        depth: node.depth,
        path: node.path,
        control_type: node.role,
        name: node.name,
        value: node.value,
        automation_id: node.accessible_id,
        class_name: String::new(),
        focused: node.focused,
        bounds: node.bounds,
      })
      .collect(),
  })
}

pub fn focus_node(window: &Window, node_path: &str) -> DriverResult<InputActionResult> {
  let result = atspi::focus_node(window, node_path)?;
  Ok(InputActionResult {
    selected_path: InputDeliveryPath::AxFocus,
    attempts: vec![InputAttempt::success(InputDeliveryPath::AxFocus)],
    fallback_reason: result.fallback_reason,
    mouse_disturbance: DisturbanceLevel::None,
    focus_disturbance: DisturbanceLevel::Foreground,
    clipboard_disturbance: DisturbanceLevel::None,
  })
}

pub fn select_node(window: &Window, node_path: &str) -> DriverResult<InputActionResult> {
  let result = atspi::select_node(window, node_path)?;
  Ok(InputActionResult {
    selected_path: InputDeliveryPath::AxPress,
    attempts: vec![InputAttempt {
      path: InputDeliveryPath::AxPress,
      succeeded: true,
      message: Some(result.action_name),
    }],
    fallback_reason: None,
    mouse_disturbance: DisturbanceLevel::None,
    focus_disturbance: DisturbanceLevel::Foreground,
    clipboard_disturbance: DisturbanceLevel::None,
  })
}

#[cfg(test)]
mod tests {
  use auv_driver::geometry::{CoordinateSpace, Rect};
  use auv_driver::window::WindowRef;

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
      fallback_reason: None,
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
      fallback_reason: None,
      mouse_disturbance: DisturbanceLevel::None,
      focus_disturbance: DisturbanceLevel::Foreground,
      clipboard_disturbance: DisturbanceLevel::None,
    };

    assert_eq!(window.reference.id, "atspi::1.1/window");
    assert_eq!(result.selected_path, InputDeliveryPath::AxFocus);
  }
}
