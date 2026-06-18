use std::time::Duration;

use image::{RgbaImage, SubImage};

use crate::display::Display;
use crate::geometry::Rect;
use crate::window::WindowRef;

pub type ImageView<'a> = SubImage<&'a RgbaImage>;

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Activation {
  #[default]
  KeepCurrent,
  ActivateFirst {
    settle: Duration,
  },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CaptureOptions {
  pub activation: Activation,
  pub display: Option<String>,
  pub window: Option<WindowRef>,
  pub region: Option<Rect>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Capture {
  pub image: RgbaImage,
  pub bounds: Rect,
  pub scale_factor: f64,
  pub backend: String,
  pub fallback_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DisplayCapture {
  pub display: Display,
  pub capture: Capture,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RegionCapture {
  pub display: Display,
  pub capture: Capture,
}

/// Generic same-instant binding between a structured source observation and a
/// captured image/artifact.
///
/// The binding records provenance only. It does not define freshness thresholds,
/// refusal policy, or source-specific capture mechanics.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CaptureBinding {
  pub source_observation_id: String,
  pub capture_ref: String,
  pub capture_skew_ms: i64,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub source_timestamp_millis: Option<u64>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub capture_timestamp_millis: Option<u64>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub known_limits: Vec<String>,
}

impl CaptureBinding {
  pub fn new(
    source_observation_id: impl Into<String>,
    capture_ref: impl Into<String>,
    capture_skew_ms: i64,
  ) -> Self {
    Self {
      source_observation_id: source_observation_id.into(),
      capture_ref: capture_ref.into(),
      capture_skew_ms,
      source_timestamp_millis: None,
      capture_timestamp_millis: None,
      known_limits: Vec::new(),
    }
  }

  pub fn with_source_timestamp_millis(mut self, timestamp_millis: u64) -> Self {
    self.source_timestamp_millis = Some(timestamp_millis);
    self
  }

  pub fn with_capture_timestamp_millis(mut self, timestamp_millis: u64) -> Self {
    self.capture_timestamp_millis = Some(timestamp_millis);
    self
  }

  pub fn with_known_limit(mut self, known_limit: impl Into<String>) -> Self {
    self.known_limits.push(known_limit.into());
    self
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn capture_binding_serializes_same_instant_fact() {
    let binding = CaptureBinding::new("frame-1", "artifact://capture-1", 12)
      .with_source_timestamp_millis(1_000)
      .with_capture_timestamp_millis(988)
      .with_known_limit("clock bases aligned by caller");

    let value = serde_json::to_value(&binding).expect("serialize capture binding");

    assert_eq!(value["source_observation_id"], serde_json::json!("frame-1"));
    assert_eq!(
      value["capture_ref"],
      serde_json::json!("artifact://capture-1")
    );
    assert_eq!(value["capture_skew_ms"], serde_json::json!(12));
    assert_eq!(value["source_timestamp_millis"], serde_json::json!(1_000));
    assert_eq!(value["capture_timestamp_millis"], serde_json::json!(988));
  }
}
