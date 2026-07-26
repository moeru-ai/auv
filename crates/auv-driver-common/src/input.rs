use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
  #[default]
  Left,
  Right,
  Middle,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationPolicy {
  #[default]
  NoChange,
  Background,
  FocusWithoutRaise,
  Foreground {
    settle: Duration,
  },
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

#[derive(Debug, Default, PartialEq, Eq)]
pub struct InputPreparationLease {
  restored: bool,
}

impl InputPreparationLease {
  pub const fn noop() -> Self {
    Self { restored: false }
  }

  pub fn mark_restored(&mut self) {
    self.restored = true;
  }

  pub const fn is_restored(&self) -> bool {
    self.restored
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClickOptions {
  pub policy: InputPolicy,
  pub click: Click,
  pub window_strategy: WindowClickStrategy,
}

impl Default for ClickOptions {
  fn default() -> Self {
    Self {
      policy: InputPolicy::BackgroundPreferred,
      click: Click::Single,
      window_strategy: WindowClickStrategy::default(),
    }
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowClickStrategy {
  /// Use the Chromium-compatible background window click route.
  ///
  /// This stamps extra window-routing fields and sends a CUA-derived synthetic
  /// event sequence for Chromium/WebView/Catalyst-style targets that ignore
  /// the narrower pid-targeted route.
  #[default]
  ChromiumCompatible,
  /// Use a direct pid-targeted mouse pair with window-local routing fields.
  PidTargeted,
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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyPressOptions {
  pub key: String,
  pub settle: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scroll {
  pub delta_x: f64,
  pub delta_y: f64,
}

impl Scroll {
  pub const fn new(delta_x: f64, delta_y: f64) -> Self {
    Self { delta_x, delta_y }
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScrollOptions {
  pub policy: InputPolicy,
  pub delivery_strategy: ScrollDeliveryStrategy,
  pub settle: Duration,
}

impl Default for ScrollOptions {
  fn default() -> Self {
    Self {
      policy: InputPolicy::BackgroundPreferred,
      delivery_strategy: ScrollDeliveryStrategy::default(),
      settle: Duration::ZERO,
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollDeliveryCandidate {
  AxScroll,
  WindowTargetedWheel,
  WindowTargetedKeyboardScroll,
  ForegroundHid,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScrollDeliveryStrategy {
  pub candidates: Vec<ScrollDeliveryCandidate>,
}

impl Default for ScrollDeliveryStrategy {
  fn default() -> Self {
    Self {
      candidates: vec![
        ScrollDeliveryCandidate::AxScroll,
        ScrollDeliveryCandidate::WindowTargetedWheel,
        ScrollDeliveryCandidate::ForegroundHid,
      ],
    }
  }
}

impl ScrollDeliveryStrategy {
  pub fn foreground_hid() -> Self {
    Self {
      candidates: vec![ScrollDeliveryCandidate::ForegroundHid],
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
  AxScroll,
  AxSelectedText,
  WindowTargetedMouse,
  WindowTargetedWheel,
  WindowTargetedKeyboard,
  WindowTargetedKeyboardScroll,
  ClipboardPaste,
  ForegroundSystemEvents,
  Unsupported,
}

impl InputDeliveryPath {
  pub const fn as_str(self) -> &'static str {
    match self {
      Self::Noop => "noop",
      Self::AxPress => "ax_press",
      Self::AxFocus => "ax_focus",
      Self::AxSetValue => "ax_set_value",
      Self::AxScroll => "ax_scroll",
      Self::AxSelectedText => "ax_selected_text",
      Self::WindowTargetedMouse => "window_targeted_mouse",
      Self::WindowTargetedWheel => "window_targeted_wheel",
      Self::WindowTargetedKeyboard => "window_targeted_keyboard",
      Self::WindowTargetedKeyboardScroll => "window_targeted_keyboard_scroll",
      Self::ClipboardPaste => "clipboard_paste",
      Self::ForegroundSystemEvents => "foreground_system_events",
      Self::Unsupported => "unsupported",
    }
  }
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

impl DisturbanceLevel {
  pub const fn as_str(self) -> &'static str {
    match self {
      Self::None => "none",
      Self::Temporary => "temporary",
      Self::Foreground => "foreground",
      Self::Unknown => "unknown",
    }
  }
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

/// Canonical artifact purpose used by recording adapters for
/// [`InputActionResult`] JSON.
pub const INPUT_ACTION_RESULT_PURPOSE: &str = "auv.driver.input_action_result";

/// Persisted record of one driver input delivery — clicks, scrolls,
/// text submission, etc. Captures the attempt sequence, the path that
/// ultimately succeeded (or the failure mode), and the disturbance
/// levels the delivery caused on user-visible state (mouse, focus,
/// clipboard).
///
/// # Seam role
///
/// Current "what actually happened" input delivery evidence for action-
/// bearing operations. The archived candidate-action `ActionResolverDecision`
/// peer schema was removed; method-selection details should now be represented
/// through current operation records, verification records, and delivery
/// evidence instead of a separate resolver-decision artifact.
///
/// - **Upstream**: AUV's macOS smart-press path produces an
///   `InputActionResult` when it attempts typed delivery. Direct driver-
///   API consumers (recipes, typed commands invoking driver primitives)
///   construct `InputActionResult` the same way.
/// - **Downstream**: persisted as a standalone `input-action-result` JSON
///   artifact — **not** embedded in `OperationResult`. Read-side seam:
///   see the owning runtime or frontend's purpose-specific reader.
///
/// Do not introduce a new action-result schema beside `InputActionResult`
/// without owner approval.
///
/// TODO(operation-result-iar-ref): whether `OperationResult.evidence_artifacts`
/// should explicitly cite the standalone artifact is a separate slice.
///
/// TODO(input-action-result-api-version): this existing wire shape has no
/// version discriminator. Add producer stamping and reader validation together
/// only when an owner-approved versioning slice defines the compatibility rule.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputActionResult {
  pub selected_path: InputDeliveryPath,
  pub attempts: Vec<InputAttempt>,
  pub mouse_disturbance: DisturbanceLevel,
  pub focus_disturbance: DisturbanceLevel,
  pub clipboard_disturbance: DisturbanceLevel,
}

impl InputActionResult {
  pub fn single_success(path: InputDeliveryPath) -> Self {
    Self {
      selected_path: path,
      attempts: vec![InputAttempt::success(path)],
      mouse_disturbance: DisturbanceLevel::None,
      focus_disturbance: DisturbanceLevel::None,
      clipboard_disturbance: DisturbanceLevel::None,
    }
  }

  /// Returns the first failed delivery attempt's diagnostic.
  pub fn fallback_reason(&self) -> Option<&str> {
    self.attempts.iter().find(|attempt| !attempt.succeeded).and_then(|attempt| attempt.message.as_deref())
  }
}

#[cfg(test)]
#[path = "input_test.rs"]
mod tests;
