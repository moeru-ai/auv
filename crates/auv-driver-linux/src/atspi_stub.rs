use auv_driver::error::{DriverError, DriverResult};
use auv_driver::geometry::Rect;
use auv_driver::window::Window;

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
  pub depth: usize,
  pub path: String,
  pub role: String,
  pub name: String,
  pub description: String,
  pub accessible_id: String,
  pub value: Option<String>,
  pub focused: bool,
  pub bounds: Rect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TreeSnapshot {
  pub window_ref: String,
  pub nodes: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionResult {
  pub action_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FocusResult {
  pub fallback_reason: Option<String>,
}

pub fn snapshot_window(_window: &Window) -> DriverResult<TreeSnapshot> {
  Err(DriverError::unsupported("window.capture_ax_tree"))
}

pub fn focus_window(_window: &Window) -> DriverResult<()> {
  Err(DriverError::unsupported("accessibility.focus_node"))
}

pub fn focus_node(_window: &Window, _node_path: &str) -> DriverResult<FocusResult> {
  Err(DriverError::unsupported("accessibility.focus_node"))
}

pub fn select_node(_window: &Window, _node_path: &str) -> DriverResult<ActionResult> {
  Err(DriverError::unsupported("accessibility.select_node"))
}
