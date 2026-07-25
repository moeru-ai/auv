use serde::{Deserialize, Serialize};

use crate::InputActionResult;

/// Evidence returned after focusing a selected accessibility node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AxFocusResult {
  pub app: String,
  pub pid: i32,
  pub path: String,
  pub role: String,
  pub title: String,
  pub value: String,
  pub query: String,
  pub input_action_result: InputActionResult,
}

/// Observed accessibility facts returned after reading text from a node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AxTextRead {
  pub app: String,
  pub pid: i32,
  pub path: String,
  pub role: String,
  pub matched_text: String,
}
