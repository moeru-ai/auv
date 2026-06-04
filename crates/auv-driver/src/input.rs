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

/// Persisted record of one driver input delivery — clicks, scrolls,
/// text submission, etc. Captures the attempt sequence, the path that
/// ultimately succeeded (or the failure mode), and the disturbance
/// levels the delivery caused on user-visible state (mouse, focus,
/// clipboard).
///
/// # Seam role
///
/// Lower / "what actually happened" half of the v0 action-result pair
/// (per CLAUDE.md). Sibling: `ActionResolverDecision` in
/// `src/driver/macos/control/action_resolver.rs` (`pub(crate)`), which
/// records the upstream method-selection decision.
///
/// - **Upstream**: AUV's macOS smart-press path produces an
///   `ActionResolverDecision` alongside this struct. Direct driver-
///   API consumers (recipes, typed commands invoking driver primitives
///   without a resolver) construct `InputActionResult` without a peer
///   decision record.
/// - **Downstream**: action-bearing operations attach this (and any
///   peer `ActionResolverDecision`) to the resulting `OperationResult`
///   artifact (`src/contract.rs`) as delivery evidence — typically
///   through evidence artifacts or signal flattening.
///
/// Per CLAUDE.md, this is one of the two action-result schemas in v0;
/// `ActionResolverDecision` is the other. Do not introduce a third
/// action-result schema beside these two.
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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn click_and_click_options_serde_roundtrip() {
    let clicks = [
      Click::Single,
      Click::Double {
        interval: Duration::from_millis(42),
      },
    ];

    for click in clicks {
      let encoded = serde_json::to_string(&click).expect("serialize click");
      let decoded: Click = serde_json::from_str(&encoded).expect("deserialize click");
      assert_eq!(decoded, click);
    }

    let options = ClickOptions {
      policy: InputPolicy::ForegroundPreferred,
      click: Click::Double {
        interval: Duration::from_millis(100),
      },
      window_strategy: WindowClickStrategy::PidTargeted,
    };

    let encoded = serde_json::to_string(&options).expect("serialize click options");
    let decoded: ClickOptions = serde_json::from_str(&encoded).expect("deserialize click options");
    assert_eq!(decoded, options);
  }

  #[test]
  fn scroll_serde_roundtrip() {
    let scroll = Scroll::new(12.5, -42.0);

    let encoded = serde_json::to_string(&scroll).expect("serialize scroll");
    let decoded: Scroll = serde_json::from_str(&encoded).expect("deserialize scroll");

    assert_eq!(decoded, scroll);
  }

  #[test]
  fn scroll_options_serde_uses_public_snake_case_contract() {
    let options = ScrollOptions {
      policy: InputPolicy::BackgroundPreferred,
      delivery_strategy: ScrollDeliveryStrategy {
        candidates: vec![
          ScrollDeliveryCandidate::AxScroll,
          ScrollDeliveryCandidate::WindowTargetedWheel,
          ScrollDeliveryCandidate::ForegroundHid,
        ],
      },
      settle: Duration::from_millis(25),
    };

    let encoded = serde_json::to_value(&options).expect("serialize scroll options");

    assert_eq!(
      encoded,
      serde_json::json!({
        "policy": "background_preferred",
        "delivery_strategy": {
          "candidates": [
            "ax_scroll",
            "window_targeted_wheel",
            "foreground_hid",
          ],
        },
        "settle": {
          "secs": 0,
          "nanos": 25_000_000,
        },
      })
    );
    let decoded: ScrollOptions =
      serde_json::from_value(encoded).expect("deserialize scroll options");
    assert_eq!(decoded, options);
  }

  #[test]
  fn scroll_delivery_path_variants_serde_as_snake_case() {
    assert_eq!(
      serde_json::to_string(&InputDeliveryPath::AxScroll).expect("serialize ax scroll"),
      "\"ax_scroll\""
    );
    assert_eq!(
      serde_json::to_string(&InputDeliveryPath::WindowTargetedWheel)
        .expect("serialize window wheel"),
      "\"window_targeted_wheel\""
    );
    assert_eq!(
      serde_json::to_string(&InputDeliveryPath::WindowTargetedKeyboardScroll)
        .expect("serialize keyboard scroll"),
      "\"window_targeted_keyboard_scroll\""
    );
  }

  #[test]
  fn scroll_options_default_to_background_preferred() {
    let options = ScrollOptions::default();

    assert_eq!(options.policy, InputPolicy::BackgroundPreferred);
    assert_eq!(options.settle, Duration::ZERO);
  }

  #[test]
  fn scroll_delivery_strategy_defaults_to_background_first_without_keyboard() {
    let strategy = ScrollDeliveryStrategy::default();

    assert_eq!(
      strategy.candidates,
      vec![
        ScrollDeliveryCandidate::AxScroll,
        ScrollDeliveryCandidate::WindowTargetedWheel,
        ScrollDeliveryCandidate::ForegroundHid,
      ]
    );
  }

  #[test]
  fn scroll_options_default_include_delivery_strategy() {
    let options = ScrollOptions::default();

    assert_eq!(options.policy, InputPolicy::BackgroundPreferred);
    assert_eq!(options.delivery_strategy, ScrollDeliveryStrategy::default());
    assert_eq!(options.settle, Duration::ZERO);
  }

  #[test]
  fn scroll_specific_delivery_paths_are_distinct_from_mouse_and_keyboard() {
    assert_ne!(InputDeliveryPath::AxScroll, InputDeliveryPath::AxSetValue);
    assert_ne!(
      InputDeliveryPath::WindowTargetedWheel,
      InputDeliveryPath::WindowTargetedMouse
    );
    assert_ne!(
      InputDeliveryPath::WindowTargetedKeyboardScroll,
      InputDeliveryPath::WindowTargetedKeyboard
    );
  }

  #[test]
  fn input_preparation_lease_tracks_restoration() {
    let mut lease = InputPreparationLease::noop();
    assert!(!lease.is_restored());

    lease.mark_restored();

    assert!(lease.is_restored());
  }
}
