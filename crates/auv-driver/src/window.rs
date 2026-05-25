use serde::{Deserialize, Serialize};

use crate::geometry::{CoordinateSpace, Rect};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WindowRef {
  pub id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Window {
  pub reference: WindowRef,
  pub title: Option<String>,
  pub app_name: Option<String>,
  pub app_bundle_id: Option<String>,
  pub process_id: Option<u32>,
  pub frame: Rect,
  pub coordinate_space: CoordinateSpace,
  pub is_main: bool,
  pub is_visible: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ObservedWindows {
  pub windows: Vec<Window>,
}
