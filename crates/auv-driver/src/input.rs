use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
  Left,
  Right,
  Middle,
}

impl Default for MouseButton {
  fn default() -> Self {
    Self::Left
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Click {
  Single,
  Double { interval: Duration },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PasteTextOptions {
  pub text: String,
  pub replace_existing: bool,
  pub submit: TextSubmit,
  pub settle: Duration,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextSubmit {
  #[default]
  No,
  Return,
  Search,
  Done,
  Go,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaitOptions {
  pub timeout: Duration,
  pub poll_interval: Duration,
}

impl Default for WaitOptions {
  fn default() -> Self {
    Self {
      timeout: Duration::from_secs(5),
      poll_interval: Duration::from_millis(100),
    }
  }
}
