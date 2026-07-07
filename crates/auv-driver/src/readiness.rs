use serde::{Deserialize, Serialize};

use crate::Rect;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReadinessProbeInput {
  pub window_number: Option<i64>,
  pub window_title: Option<String>,
  pub app_bundle_id: Option<String>,
  pub expected_window_frame: Option<Rect>,
  pub max_window_frame_drift_px: f64,
  pub require_frontmost: bool,
  pub target_window_x: f64,
  pub target_window_y: f64,
}

impl ReadinessProbeInput {
  pub fn for_window_target(
    window_number: Option<i64>,
    window_title: Option<String>,
    app_bundle_id: Option<String>,
    target_window_x: f64,
    target_window_y: f64,
  ) -> Self {
    Self {
      window_number,
      window_title,
      app_bundle_id,
      expected_window_frame: None,
      max_window_frame_drift_px: 2.0,
      require_frontmost: true,
      target_window_x,
      target_window_y,
    }
  }

  pub fn with_expected_window_frame(mut self, frame: Option<Rect>) -> Self {
    self.expected_window_frame = frame;
    self
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessStatus {
  Ready,
  NotReady,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessCheckStatus {
  Pass,
  Fail,
  Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadinessCheck {
  pub name: String,
  pub status: ReadinessCheckStatus,
  pub reason: Option<String>,
}

impl ReadinessCheck {
  pub fn pass(name: impl Into<String>, reason: impl Into<String>) -> Self {
    Self {
      name: name.into(),
      status: ReadinessCheckStatus::Pass,
      reason: Some(reason.into()),
    }
  }

  pub fn fail(name: impl Into<String>, reason: impl Into<String>) -> Self {
    Self {
      name: name.into(),
      status: ReadinessCheckStatus::Fail,
      reason: Some(reason.into()),
    }
  }

  pub fn unknown(name: impl Into<String>, reason: impl Into<String>) -> Self {
    Self {
      name: name.into(),
      status: ReadinessCheckStatus::Unknown,
      reason: Some(reason.into()),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReadinessReport {
  pub status: ReadinessStatus,
  pub checks: Vec<ReadinessCheck>,
  pub target_window_ref: Option<String>,
  pub target_window_frame: Option<Rect>,
  pub selected_blocker: Option<String>,
}

impl ReadinessReport {
  pub fn from_checks(
    checks: Vec<ReadinessCheck>,
    target_window_ref: Option<String>,
    target_window_frame: Option<Rect>,
    selected_blocker: Option<String>,
  ) -> Self {
    let selected_blocker = selected_blocker.or_else(|| first_blocker(&checks));
    Self {
      status: if selected_blocker.is_some() {
        ReadinessStatus::NotReady
      } else {
        ReadinessStatus::Ready
      },
      checks,
      target_window_ref,
      target_window_frame,
      selected_blocker,
    }
  }

  pub fn ready(checks: Vec<ReadinessCheck>) -> Self {
    Self::from_checks(checks, None, None, None)
  }

  pub fn blocked(reason: impl Into<String>) -> Self {
    let reason = reason.into();
    Self::from_checks(vec![ReadinessCheck::fail("readiness", reason.clone())], None, None, Some(reason))
  }

  pub fn is_ready(&self) -> bool {
    self.status == ReadinessStatus::Ready
  }
}

fn first_blocker(checks: &[ReadinessCheck]) -> Option<String> {
  checks.iter().find(|check| check.status == ReadinessCheckStatus::Fail).and_then(|check| check.reason.clone())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn readiness_report_selects_first_failed_check_as_blocker() {
    let report = ReadinessReport::from_checks(
      vec![
        ReadinessCheck::pass("accessibility", "ok"),
        ReadinessCheck::fail("target_window_present", "missing window"),
      ],
      Some("11".to_string()),
      None,
      None,
    );

    assert_eq!(report.status, ReadinessStatus::NotReady);
    assert_eq!(report.selected_blocker.as_deref(), Some("missing window"));
  }
}
