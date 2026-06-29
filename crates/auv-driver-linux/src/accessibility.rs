//! Window accessibility tree snapshots via AT-SPI.
//!
//! This mirrors the Windows UIA tree surface and the macOS AX tree direction:
//! AUV captures a flattened, read-only tree for one top-level window. Acting on
//! arbitrary nodes remains a separate delivery-policy slice because AT-SPI
//! actions vary by role and toolkit.

use auv_driver::error::{DriverError, DriverResult};
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
  if node_path != "0" {
    return Err(DriverError::unsupported("accessibility.focus_node"));
  }
  atspi::focus_window(window)?;
  Ok(InputActionResult {
    selected_path: InputDeliveryPath::AxFocus,
    attempts: vec![InputAttempt::success(InputDeliveryPath::AxFocus)],
    fallback_reason: None,
    mouse_disturbance: DisturbanceLevel::None,
    focus_disturbance: DisturbanceLevel::Foreground,
    clipboard_disturbance: DisturbanceLevel::None,
  })
}

pub fn select_node(_window: &Window, _node_path: &str) -> DriverResult<InputActionResult> {
  // TODO(linux-atspi-actions): node action dispatch is deferred until an
  // owner-approved action resolver slice defines per-role AT-SPI action policy.
  Err(DriverError::unsupported("accessibility.select_node"))
}

#[cfg(test)]
mod tests {
  use auv_driver::geometry::{CoordinateSpace, Rect};
  use auv_driver::window::WindowRef;

  use super::*;

  #[test]
  fn focus_node_only_accepts_root_window_boundary_for_now() {
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

    assert!(focus_node(&window, "0/1").is_err());
  }
}
