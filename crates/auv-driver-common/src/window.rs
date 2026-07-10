use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{
  geometry::{CoordinateSpace, Point, Rect, Size},
  input::DisturbanceLevel,
};

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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowMutationOptions {
  pub policy: WindowMutationPolicy,
  pub strategy: WindowMutationStrategy,
  pub settle: Duration,
  pub verification: WindowMutationVerification,
}

impl Default for WindowMutationOptions {
  fn default() -> Self {
    Self {
      policy: WindowMutationPolicy::NativePreferred,
      strategy: WindowMutationStrategy::default(),
      settle: Duration::from_millis(100),
      verification: WindowMutationVerification::default(),
    }
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowMutationPolicy {
  NativeOnly,
  #[default]
  NativePreferred,
  ForegroundPreferred,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowMutationStrategy {
  pub candidates: Vec<WindowMutationCandidate>,
}

impl Default for WindowMutationStrategy {
  fn default() -> Self {
    Self {
      candidates: vec![
        WindowMutationCandidate::AxWindowAttribute,
        WindowMutationCandidate::AxWindowAction,
      ],
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowMutationCandidate {
  AxWindowAttribute,
  AxWindowAction,
  PlatformNative,
  ForegroundSystemEvents,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowMutationVerification {
  FrameTolerance { points: f64 },
  BestEffortState,
}

impl Default for WindowMutationVerification {
  fn default() -> Self {
    Self::FrameTolerance { points: 2.0 }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowMutationPath {
  AxWindowAttribute,
  AxWindowAction,
  PlatformNative,
  ForegroundSystemEvents,
  Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowMutationAttempt {
  pub path: WindowMutationPath,
  pub succeeded: bool,
  pub message: Option<String>,
}

impl WindowMutationAttempt {
  pub fn success(path: WindowMutationPath, message: impl Into<String>) -> Self {
    Self {
      path,
      succeeded: true,
      message: Some(message.into()),
    }
  }

  pub fn failure(path: WindowMutationPath, message: impl Into<String>) -> Self {
    Self {
      path,
      succeeded: false,
      message: Some(message.into()),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowMutationResult {
  pub selected_path: WindowMutationPath,
  pub attempts: Vec<WindowMutationAttempt>,
  pub fallback_reason: Option<String>,
  pub before_frame: Option<Rect>,
  pub after_frame: Option<Rect>,
  pub before_state: Option<WindowState>,
  pub after_state: Option<WindowState>,
  pub focus_disturbance: DisturbanceLevel,
  pub mouse_disturbance: DisturbanceLevel,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowState {
  pub is_minimized: Option<bool>,
  pub is_visible: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowMutationKind {
  MoveTo { point: Point },
  Resize { size: Size },
  SetFrame { frame: Rect },
  Minimize,
  Restore,
  Zoom,
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::time::Duration;

  #[test]
  fn window_mutation_options_default_to_native_preferred_ax_candidates() {
    let options = WindowMutationOptions::default();

    assert_eq!(options.policy, WindowMutationPolicy::NativePreferred);
    assert_eq!(
      options.strategy,
      WindowMutationStrategy {
        candidates: vec![
          WindowMutationCandidate::AxWindowAttribute,
          WindowMutationCandidate::AxWindowAction,
        ],
      }
    );
    assert_eq!(options.settle, Duration::from_millis(100));
    assert_eq!(options.verification, WindowMutationVerification::FrameTolerance { points: 2.0 });
  }

  #[test]
  fn window_mutation_types_serde_as_snake_case() {
    let result = WindowMutationResult {
      selected_path: WindowMutationPath::AxWindowAttribute,
      attempts: vec![WindowMutationAttempt {
        path: WindowMutationPath::AxWindowAttribute,
        succeeded: true,
        message: Some("set AXPosition".to_string()),
      }],
      fallback_reason: None,
      before_frame: Some(Rect::new(0.0, 0.0, 400.0, 300.0)),
      after_frame: Some(Rect::new(10.0, 20.0, 400.0, 300.0)),
      before_state: Some(WindowState {
        is_minimized: Some(false),
        is_visible: Some(true),
      }),
      after_state: Some(WindowState {
        is_minimized: Some(false),
        is_visible: Some(true),
      }),
      focus_disturbance: DisturbanceLevel::None,
      mouse_disturbance: DisturbanceLevel::None,
    };

    let encoded = serde_json::to_value(&result).expect("serialize");
    assert_eq!(encoded["selected_path"], "ax_window_attribute");
    assert_eq!(encoded["attempts"][0]["path"], "ax_window_attribute");

    let decoded: WindowMutationResult = serde_json::from_value(encoded).expect("deserialize");
    assert_eq!(decoded, result);
  }
}
