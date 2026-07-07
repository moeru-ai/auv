//! L6 recognition stability producer(v0)。
//!
//! 多帧 RecognitionResult 一致性 -> StabilityAssessment
//!   -> candidate_promotion::StabilityInput。
//! 纯函数,无副作用,不落盘,不碰 runtime/driver。
//! 这是 L7 闸门 `StabilityUnproven` 那条 refusal 的合法 producer。

use serde::{Deserialize, Serialize};

use crate::candidate_promotion::StabilityInput;
use crate::contract::{RecognitionBox, RecognitionResult};

/// 判稳策略。阈值全部显式,无隐藏默认。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StabilityPolicy {
  /// 至少多少帧才考虑判稳;不足直接 Unstable。
  pub min_frames: u32,
  /// 相邻帧 best 目标中心点允许的最大漂移(像素)。
  pub max_centroid_drift_px: f64,
  /// 是否要求 best.text 跨帧完全一致。
  pub require_stable_text: bool,
}

/// 判稳结果(自描述,带原因)。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StabilityAssessment {
  Stable {
    observed_frames: u32,
    max_observed_drift_px: f64,
  },
  Unstable {
    reason: StabilityRejection,
  },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StabilityRejection {
  NoFrames,
  InsufficientFrames {
    have: u32,
    need: u32,
  },
  TargetMissingInFrame {
    frame_index: usize,
  },
  UnstableKind {
    first: String,
    offending_frame: usize,
  },
  UnstableText {
    offending_frame: usize,
  },
  DriftExceeded {
    observed_px: f64,
    allowed_px: f64,
    between_frames: (usize, usize),
  },
}

/// 主入口。约定 `observations` 按时间顺序、且都指同一个目标。
pub fn assess_stability(observations: &[RecognitionResult], policy: &StabilityPolicy) -> StabilityAssessment {
  if observations.is_empty() {
    return StabilityAssessment::Unstable {
      reason: StabilityRejection::NoFrames,
    };
  }
  let frame_count = observations.len() as u32;
  if frame_count < policy.min_frames {
    return StabilityAssessment::Unstable {
      reason: StabilityRejection::InsufficientFrames {
        have: frame_count,
        need: policy.min_frames,
      },
    };
  }

  let mut bests = Vec::with_capacity(observations.len());
  for (i, obs) in observations.iter().enumerate() {
    match obs.best.as_ref() {
      Some(item) => bests.push(item),
      None => {
        return StabilityAssessment::Unstable {
          reason: StabilityRejection::TargetMissingInFrame { frame_index: i },
        };
      }
    }
  }

  let first_kind = bests[0].kind.clone();
  let first_text = bests[0].text.clone();
  for (i, item) in bests.iter().enumerate().skip(1) {
    if item.kind != first_kind {
      return StabilityAssessment::Unstable {
        reason: StabilityRejection::UnstableKind {
          first: first_kind,
          offending_frame: i,
        },
      };
    }
    if policy.require_stable_text && item.text != first_text {
      return StabilityAssessment::Unstable {
        reason: StabilityRejection::UnstableText { offending_frame: i },
      };
    }
  }

  let mut max_drift = 0.0_f64;
  for i in 1..bests.len() {
    let drift = euclidean(centroid(&bests[i - 1].box_), centroid(&bests[i].box_));
    if drift > max_drift {
      max_drift = drift;
    }
    if drift > policy.max_centroid_drift_px {
      return StabilityAssessment::Unstable {
        reason: StabilityRejection::DriftExceeded {
          observed_px: drift,
          allowed_px: policy.max_centroid_drift_px,
          between_frames: (i - 1, i),
        },
      };
    }
  }

  StabilityAssessment::Stable {
    observed_frames: frame_count,
    max_observed_drift_px: max_drift,
  }
}

impl StabilityAssessment {
  /// 适配到 L7 闸门入参。Stable -> Proven; Unstable -> Unproven{reason}。
  pub fn to_promotion_stability_input(&self) -> StabilityInput {
    match self {
      StabilityAssessment::Stable {
        observed_frames, ..
      } => StabilityInput::Proven {
        observed_frames: *observed_frames,
      },
      StabilityAssessment::Unstable { reason } => StabilityInput::Unproven {
        reason: format!("{reason:?}"),
      },
    }
  }
}

fn centroid(b: &RecognitionBox) -> (f64, f64) {
  (b.x as f64 + b.width as f64 / 2.0, b.y as f64 + b.height as f64 / 2.0)
}

fn euclidean(a: (f64, f64), b: (f64, f64)) -> f64 {
  ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
  use serde_json::Value;

  use super::{StabilityAssessment, StabilityPolicy, StabilityRejection, assess_stability};
  use crate::candidate_promotion::StabilityInput;
  use crate::contract::{RecognitionBox, RecognitionResult, RecognitionScope, RecognitionSource, RecognitionSurface, RecognizedItem};

  fn frame(kind: &str, text: Option<&str>, x: i64, y: i64) -> RecognitionResult {
    let best = RecognizedItem {
      item_id: "i".to_string(),
      kind: kind.to_string(),
      box_: RecognitionBox {
        x,
        y,
        width: 40,
        height: 20,
      },
      text: text.map(str::to_string),
      provider_score: None,
      detail: Value::Null,
    };
    RecognitionResult {
      recognition_id: format!("recognition-{x}-{y}"),
      source: RecognitionSource::Custom,
      scope: RecognitionScope {
        surface: RecognitionSurface::Window,
        display_ref: None,
        native_display_id: None,
        app_bundle_id: None,
        window_title: None,
        window_number: None,
        region_hint: None,
        capture_artifact: None,
        capture_contract_artifact: None,
      },
      best: Some(best.clone()),
      filtered: vec![],
      all: vec![best],
      detail: Value::Null,
      evidence: vec![],
      known_limits: vec![],
    }
  }

  fn policy(min_frames: u32, max_centroid_drift_px: f64, require_stable_text: bool) -> StabilityPolicy {
    StabilityPolicy {
      min_frames,
      max_centroid_drift_px,
      require_stable_text,
    }
  }

  #[test]
  fn no_frames_is_unstable() {
    let assessment = assess_stability(&[], &policy(2, 10.0, false));
    assert_eq!(
      assessment,
      StabilityAssessment::Unstable {
        reason: StabilityRejection::NoFrames
      }
    );
  }

  #[test]
  fn below_min_frames_is_unstable() {
    let assessment = assess_stability(&[frame("button", Some("OK"), 10, 20)], &policy(3, 10.0, false));
    assert_eq!(
      assessment,
      StabilityAssessment::Unstable {
        reason: StabilityRejection::InsufficientFrames { have: 1, need: 3 }
      }
    );
  }

  #[test]
  fn stable_frames_map_to_proven() {
    let frames = vec![
      frame("button", Some("OK"), 10, 20),
      frame("button", Some("OK"), 12, 21),
      frame("button", Some("OK"), 14, 22),
    ];

    let assessment = assess_stability(&frames, &policy(3, 5.0, true));
    match assessment {
      StabilityAssessment::Stable {
        observed_frames,
        max_observed_drift_px,
      } => {
        assert_eq!(observed_frames, 3);
        assert!(max_observed_drift_px > 0.0);
      }
      other => panic!("expected Stable, got {other:?}"),
    }
    assert_eq!(
      assess_stability(&frames, &policy(3, 5.0, true)).to_promotion_stability_input(),
      StabilityInput::Proven { observed_frames: 3 }
    );
  }

  #[test]
  fn target_missing_in_frame_is_unstable() {
    let mut frames = vec![
      frame("button", Some("OK"), 10, 20),
      frame("button", Some("OK"), 12, 21),
      frame("button", Some("OK"), 14, 22),
    ];
    frames[1].best = None;

    let assessment = assess_stability(&frames, &policy(3, 5.0, true));
    assert_eq!(
      assessment,
      StabilityAssessment::Unstable {
        reason: StabilityRejection::TargetMissingInFrame { frame_index: 1 }
      }
    );
  }

  #[test]
  fn kind_change_is_unstable() {
    let frames = vec![
      frame("button", Some("OK"), 10, 20),
      frame("dialog", Some("OK"), 12, 21),
      frame("button", Some("OK"), 14, 22),
    ];

    let assessment = assess_stability(&frames, &policy(3, 5.0, true));
    assert!(matches!(
      assessment,
      StabilityAssessment::Unstable {
        reason: StabilityRejection::UnstableKind { .. }
      }
    ));
  }

  #[test]
  fn drift_exceeded_is_unstable() {
    let frames = vec![
      frame("button", Some("OK"), 10, 20),
      frame("button", Some("OK"), 100, 200),
      frame("button", Some("OK"), 102, 202),
    ];

    let assessment = assess_stability(&frames, &policy(3, 5.0, true));
    assert!(matches!(
      assessment,
      StabilityAssessment::Unstable {
        reason: StabilityRejection::DriftExceeded { .. }
      }
    ));
  }
}
