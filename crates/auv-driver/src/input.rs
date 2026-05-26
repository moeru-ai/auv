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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputPolicy {
  BackgroundOnly,
  #[default]
  BackgroundPreferred,
  ForegroundPreferred,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationPolicy {
  NoChange,
  Background,
  FocusWithoutRaise,
  Foreground { settle: Duration },
}

impl Default for ActivationPolicy {
  fn default() -> Self {
    Self::NoChange
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepareForInputOptions {
  pub activation: ActivationPolicy,
  pub preserve_frontmost: bool,
  pub install_focus_guard: bool,
  pub settle: Duration,
}

impl Default for PrepareForInputOptions {
  fn default() -> Self {
    Self {
      activation: ActivationPolicy::NoChange,
      preserve_frontmost: true,
      install_focus_guard: false,
      settle: Duration::ZERO,
    }
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputPreparationLease {
  pub restored: bool,
}

impl InputPreparationLease {
  pub const fn noop() -> Self {
    Self { restored: false }
  }

  pub fn mark_restored(&mut self) {
    self.restored = true;
  }

  pub const fn is_restored(self) -> bool {
    self.restored
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClickOptions {
  pub policy: InputPolicy,
  pub click: Click,
}

impl Default for ClickOptions {
  fn default() -> Self {
    Self {
      policy: InputPolicy::BackgroundPreferred,
      click: Click::Single,
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeTextOptions {
  pub policy: InputPolicy,
  pub replace_existing: bool,
  pub submit: TextSubmit,
  pub inter_char_delay: Duration,
  pub allow_clipboard_fallback: bool,
  pub settle: Duration,
}

impl Default for TypeTextOptions {
  fn default() -> Self {
    Self {
      policy: InputPolicy::BackgroundPreferred,
      replace_existing: false,
      submit: TextSubmit::No,
      inter_char_delay: Duration::from_millis(8),
      allow_clipboard_fallback: false,
      settle: Duration::ZERO,
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputDeliveryPath {
  Noop,
  AxPress,
  AxFocus,
  AxSetValue,
  AxSelectedText,
  WindowTargetedMouse,
  WindowTargetedKeyboard,
  ClipboardPaste,
  ForegroundSystemEvents,
  Unsupported,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisturbanceLevel {
  #[default]
  None,
  Temporary,
  Foreground,
  Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputAttempt {
  pub path: InputDeliveryPath,
  pub succeeded: bool,
  pub message: Option<String>,
}

impl InputAttempt {
  pub fn success(path: InputDeliveryPath) -> Self {
    Self {
      path,
      succeeded: true,
      message: None,
    }
  }

  pub fn failure(path: InputDeliveryPath, message: impl Into<String>) -> Self {
    Self {
      path,
      succeeded: false,
      message: Some(message.into()),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputActionResult {
  pub selected_path: InputDeliveryPath,
  pub attempts: Vec<InputAttempt>,
  pub fallback_reason: Option<String>,
  pub mouse_disturbance: DisturbanceLevel,
  pub focus_disturbance: DisturbanceLevel,
  pub clipboard_disturbance: DisturbanceLevel,
}

impl InputActionResult {
  pub fn single_success(path: InputDeliveryPath) -> Self {
    Self {
      selected_path: path,
      attempts: vec![InputAttempt::success(path)],
      fallback_reason: None,
      mouse_disturbance: DisturbanceLevel::None,
      focus_disturbance: DisturbanceLevel::None,
      clipboard_disturbance: DisturbanceLevel::None,
    }
  }
}
