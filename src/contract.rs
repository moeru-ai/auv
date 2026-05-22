use serde::{Deserialize, Serialize};

use crate::trace::{ArtifactId, EventId, RunId, SpanId};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRef {
  pub run_id: RunId,
  pub artifact_id: ArtifactId,
  pub span_id: SpanId,
  pub captured_event_id: Option<EventId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateRef {
  pub source_run_id: RunId,
  pub source_span_id: SpanId,
  pub source_operation_id: String,
  pub source_artifact_id: ArtifactId,
  pub candidate_local_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
  Completed,
  Failed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OperationResult {
  pub run_id: RunId,
  pub status: OperationStatus,
  pub operation_id: String,
  pub evidence_artifacts: Vec<ArtifactRef>,
  pub output: OperationOutput,
  pub freshness_basis: Option<FreshnessBasis>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OperationOutput {
  Candidates { candidates: Vec<Candidate> },
  Verification { verification: VerificationResult },
  Acknowledged { message: Option<String> },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FreshnessBasis {
  pub source_artifact: Option<ArtifactRef>,
  pub source_operation_id: Option<String>,
  pub notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
  pub candidate_local_id: String,
  pub kind: String,
  pub label: Option<String>,
  pub target_spec: TargetSpec,
  pub evidence: CandidateEvidence,
  pub liveness: CandidateLiveness,
  pub control: ControlRequirements,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TargetSpec {
  pub grounding: TargetGrounding,
  pub anchor_text: Option<String>,
  pub region_hint: Option<RatioRegion>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetGrounding {
  OcrAnchor,
  VisualRow,
  AxNode,
  Coordinate,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RatioRegion {
  pub left: f64,
  pub top: f64,
  pub right: f64,
  pub bottom: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CandidateEvidence {
  pub artifact_ref: ArtifactRef,
  pub observation: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CandidateLiveness {
  pub preconditions: LivenessPreconditions,
  pub ttl_hint_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LivenessPreconditions {
  pub window_ref: Option<WindowRefPrecondition>,
  pub anchor_recheck: Option<AnchorRecheckPrecondition>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowRefPrecondition {
  pub app_bundle_id: String,
  pub window_title_substring: Option<String>,
  pub window_number: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnchorRecheckPrecondition {
  pub text: String,
  pub region_hint: Option<RatioRegion>,
  pub expected_min_confidence: f64,
  pub max_pixel_distance: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlRequirements {
  pub requires_app_frontmost: bool,
  pub requires_window_focus: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VerificationResult {
  pub executed: bool,
  pub state_changed: bool,
  pub semantic_matched: Option<bool>,
  pub failure_layer: Option<FailureLayer>,
  pub evidence: Vec<ArtifactRef>,
  pub observed_label: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureLayer {
  GroundingFailed,
  CandidateExpired,
  ControlFailed,
  VerificationUnreliable,
  StateChangedNoMatch,
  SemanticMismatch,
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::json;

  fn artifact_ref() -> ArtifactRef {
    ArtifactRef {
      run_id: RunId::new("run_123"),
      artifact_id: ArtifactId::new("artifact_01"),
      span_id: SpanId::new("span_01"),
      captured_event_id: Some(EventId::new("event_01")),
    }
  }

  #[test]
  fn artifact_ref_round_trips_without_inline_timestamp() {
    let value = serde_json::to_value(artifact_ref()).expect("artifact ref should serialize");

    assert_eq!(value["run_id"], json!("run_123"));
    assert_eq!(value["artifact_id"], json!("artifact_01"));
    assert_eq!(value["span_id"], json!("span_01"));
    assert_eq!(value["captured_event_id"], json!("event_01"));
    assert!(value.get("captured_at_millis").is_none());

    let parsed: ArtifactRef =
      serde_json::from_value(value).expect("artifact ref should deserialize");
    assert_eq!(parsed, artifact_ref());
  }

  #[test]
  fn candidate_ref_round_trips_as_cross_operation_handle() {
    let reference = CandidateRef {
      source_run_id: RunId::new("run_getter"),
      source_span_id: SpanId::new("span_getter"),
      source_operation_id: "music.search.results".to_string(),
      source_artifact_id: ArtifactId::new("artifact_candidates"),
      candidate_local_id: "row#1".to_string(),
    };

    let value = serde_json::to_value(&reference).expect("candidate ref should serialize");
    assert_eq!(value["source_run_id"], json!("run_getter"));
    assert_eq!(value["source_span_id"], json!("span_getter"));
    assert_eq!(value["source_operation_id"], json!("music.search.results"));
    assert_eq!(value["source_artifact_id"], json!("artifact_candidates"));
    assert_eq!(value["candidate_local_id"], json!("row#1"));
    assert!(value.get("candidate_id").is_none());

    let parsed: CandidateRef =
      serde_json::from_value(value).expect("candidate ref should deserialize");
    assert_eq!(parsed, reference);
  }

  #[test]
  fn operation_result_with_candidate_round_trips() {
    let artifact = artifact_ref();
    let result = OperationResult {
      run_id: RunId::new("run_123"),
      status: OperationStatus::Completed,
      operation_id: "music.search.results".to_string(),
      evidence_artifacts: vec![artifact.clone()],
      output: OperationOutput::Candidates {
        candidates: vec![Candidate {
          candidate_local_id: "row#1".to_string(),
          kind: "search_result_row".to_string(),
          label: Some("Cure For Me".to_string()),
          target_spec: TargetSpec {
            grounding: TargetGrounding::OcrAnchor,
            anchor_text: Some("Cure For Me".to_string()),
            region_hint: Some(RatioRegion {
              left: 0.2,
              top: 0.3,
              right: 0.8,
              bottom: 0.9,
            }),
          },
          evidence: CandidateEvidence {
            artifact_ref: artifact.clone(),
            observation: json!({
              "provider": "vision_ocr",
              "text": "Cure For Me",
              "bounds": { "x": 2155, "y": 1402, "width": 170, "height": 24 }
            }),
          },
          liveness: CandidateLiveness {
            preconditions: LivenessPreconditions {
              window_ref: Some(WindowRefPrecondition {
                app_bundle_id: "com.tencent.QQMusicMac".to_string(),
                window_title_substring: None,
                window_number: Some(42),
              }),
              anchor_recheck: Some(AnchorRecheckPrecondition {
                text: "Cure For Me".to_string(),
                region_hint: None,
                expected_min_confidence: 0.5,
                max_pixel_distance: 32.0,
              }),
            },
            ttl_hint_ms: Some(5000),
          },
          control: ControlRequirements {
            requires_app_frontmost: true,
            requires_window_focus: true,
          },
          known_limits: vec!["validated only for visible ASCII anchors".to_string()],
        }],
      },
      freshness_basis: Some(FreshnessBasis {
        source_artifact: Some(artifact),
        source_operation_id: Some("debug.findWindowRows".to_string()),
        notes: vec!["window-scoped OCR rows".to_string()],
      }),
      known_limits: Vec::new(),
    };

    let value = serde_json::to_value(&result).expect("operation result should serialize");
    assert_eq!(value["status"], json!("completed"));
    assert_eq!(value["output"]["kind"], json!("candidates"));
    assert_eq!(
      value["output"]["candidates"][0]["target_spec"]["grounding"],
      json!("ocr_anchor")
    );

    let parsed: OperationResult =
      serde_json::from_value(value).expect("operation result should deserialize");
    assert_eq!(parsed, result);
  }

  #[test]
  fn verification_result_failure_layer_uses_snake_case_contract() {
    let result = VerificationResult {
      executed: true,
      state_changed: true,
      semantic_matched: Some(false),
      failure_layer: Some(FailureLayer::StateChangedNoMatch),
      evidence: vec![artifact_ref()],
      observed_label: Some("天空仍灿烂".to_string()),
    };

    let value = serde_json::to_value(&result).expect("verification result should serialize");
    assert_eq!(value["failure_layer"], json!("state_changed_no_match"));

    let parsed: VerificationResult =
      serde_json::from_value(value).expect("verification result should deserialize");
    assert_eq!(parsed, result);
  }
}
