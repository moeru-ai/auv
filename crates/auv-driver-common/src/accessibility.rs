use serde::{Deserialize, Serialize};

use crate::{DriverError, DriverResult, InputActionResult};

/// Stable selector for one text-capable accessibility node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AxTextSelector {
  Query(String),
  Path(String),
}

/// Inputs owned by the accessibility focus operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FocusTextOptions {
  pub app: String,
  pub selector: AxTextSelector,
  pub expected_role: Option<String>,
}

impl FocusTextOptions {
  /// Rejects malformed selectors before the macOS adapter captures an AX tree.
  pub fn validate(&self) -> DriverResult<()> {
    if self.app.trim().is_empty() {
      return Err(DriverError::InvalidInput {
        message: "accessibility.focus_text requires a non-empty app".to_string(),
      });
    }
    let selector = match &self.selector {
      AxTextSelector::Query(query) => query,
      AxTextSelector::Path(path) => path,
    };
    if selector.trim().is_empty() {
      return Err(DriverError::InvalidInput {
        message: "accessibility.focus_text requires a non-empty query or AX path".to_string(),
      });
    }
    if self.expected_role.as_ref().is_some_and(|role| role.trim().is_empty()) {
      return Err(DriverError::InvalidInput {
        message: "accessibility.focus_text expected_role must be non-empty when supplied".to_string(),
      });
    }
    Ok(())
  }
}

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
