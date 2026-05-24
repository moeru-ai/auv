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
pub struct RecognitionResult {
  pub recognition_id: String,
  pub source: RecognitionSource,
  pub scope: RecognitionScope,
  pub best: Option<RecognizedItem>,
  pub filtered: Vec<RecognizedItem>,
  pub all: Vec<RecognizedItem>,
  pub detail: serde_json::Value,
  pub evidence: Vec<ArtifactRef>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecognizedItem {
  pub item_id: String,
  pub kind: String,
  #[serde(rename = "box")]
  pub box_: RecognitionBox,
  pub text: Option<String>,
  pub provider_score: Option<f64>,
  pub detail: serde_json::Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecognitionBox {
  pub x: i64,
  pub y: i64,
  pub width: i64,
  pub height: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecognitionScope {
  pub surface: RecognitionSurface,
  pub display_ref: Option<String>,
  pub native_display_id: Option<String>,
  pub app_bundle_id: Option<String>,
  pub window_title: Option<String>,
  pub window_number: Option<i64>,
  pub region_hint: Option<RatioRegion>,
  pub capture_artifact: Option<ArtifactRef>,
  pub capture_contract_artifact: Option<ArtifactRef>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecognitionSource {
  OcrText,
  OcrRow,
  VisualRow,
  SegmentedRegion,
  IconMatch,
  Custom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecognitionSurface {
  Screen,
  Display,
  Window,
  Region,
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
pub struct CandidateQuery {
  pub query_id: String,
  pub selector: SurfaceSelector,
  pub output_kind: Option<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SurfaceSelector {
  pub any_of: Vec<SurfaceSelectorClause>,
  pub within: SelectorScope,
  pub require_visible: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum SurfaceSelectorClause {
  Ax {
    role: Option<String>,
    label: Option<String>,
    path: Option<String>,
    enabled: Option<bool>,
    visible: Option<bool>,
  },
  Ocr {
    text: String,
    region_hint: Option<RatioRegion>,
    min_provider_score: Option<f64>,
  },
  Row {
    row_index: Option<usize>,
    contains_text: Option<String>,
    region_hint: Option<RatioRegion>,
  },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectorScope {
  ActiveWindow,
  TargetWindow,
  CaptureRegion,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TargetSpec {
  pub grounding: TargetGrounding,
  pub anchor_text: Option<String>,
  pub region_hint: Option<RatioRegion>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub row_index: Option<usize>,
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
  fn candidate_query_round_trips_minimal_cross_surface_selector() {
    let query = CandidateQuery {
      query_id: "play-control".to_string(),
      selector: SurfaceSelector {
        any_of: vec![
          SurfaceSelectorClause::Ax {
            role: Some("AXButton".to_string()),
            label: Some("播放".to_string()),
            path: None,
            enabled: Some(true),
            visible: Some(true),
          },
          SurfaceSelectorClause::Ocr {
            text: "播放".to_string(),
            region_hint: Some(RatioRegion {
              left: 0.18,
              top: 0.28,
              right: 0.60,
              bottom: 0.42,
            }),
            min_provider_score: Some(0.75),
          },
          SurfaceSelectorClause::Row {
            row_index: Some(1),
            contains_text: None,
            region_hint: None,
          },
        ],
        within: SelectorScope::TargetWindow,
        require_visible: true,
      },
      output_kind: Some("button".to_string()),
      known_limits: vec!["dom and visual-icon backends are not part of v0".to_string()],
    };

    let value = serde_json::to_value(&query).expect("candidate query should serialize");
    assert_eq!(value["selector"]["within"], json!("target_window"));
    assert_eq!(value["selector"]["any_of"][0]["source"], json!("ax"));
    assert_eq!(value["selector"]["any_of"][1]["source"], json!("ocr"));
    assert_eq!(value["selector"]["any_of"][2]["source"], json!("row"));
    assert_eq!(
      value["selector"]["any_of"][1]["min_provider_score"],
      json!(0.75)
    );
    assert!(value["selector"]["any_of"][1].get("confidence").is_none());

    let parsed: CandidateQuery =
      serde_json::from_value(value).expect("candidate query should deserialize");
    assert_eq!(parsed, query);
  }

  #[test]
  fn recognition_result_round_trips_populated_best_filtered_and_all() {
    let capture_artifact = artifact_ref();
    let contract_artifact = ArtifactRef {
      run_id: RunId::new("run_123"),
      artifact_id: ArtifactId::new("artifact_contract"),
      span_id: SpanId::new("span_01"),
      captured_event_id: Some(EventId::new("event_02")),
    };
    let best = RecognizedItem {
      item_id: "item_best".to_string(),
      kind: "ocr_text".to_string(),
      box_: RecognitionBox {
        x: 2155,
        y: 1402,
        width: 170,
        height: 24,
      },
      text: Some("Cure For Me".to_string()),
      provider_score: Some(0.97),
      detail: json!({
        "provider": "vision_ocr",
        "fragments": ["Cure", "For", "Me"],
      }),
    };
    let filtered = RecognizedItem {
      item_id: "item_filtered".to_string(),
      kind: "ocr_text".to_string(),
      box_: RecognitionBox {
        x: 2140,
        y: 1440,
        width: 196,
        height: 22,
      },
      text: Some("A Temporary High".to_string()),
      provider_score: Some(0.84),
      detail: json!({
        "provider": "vision_ocr",
        "fragments": ["A", "Temporary", "High"],
      }),
    };
    let rejected = RecognizedItem {
      item_id: "item_rejected".to_string(),
      kind: "ocr_text".to_string(),
      box_: RecognitionBox {
        x: 1980,
        y: 1328,
        width: 140,
        height: 19,
      },
      text: None,
      provider_score: Some(0.31),
      detail: json!({
        "provider": "vision_ocr",
        "reject_reason": "below_min_provider_score",
      }),
    };
    let result = RecognitionResult {
      recognition_id: "recognition_window_rows_01".to_string(),
      source: RecognitionSource::OcrRow,
      scope: RecognitionScope {
        surface: RecognitionSurface::Window,
        display_ref: Some("display-main".to_string()),
        native_display_id: Some("69733248".to_string()),
        app_bundle_id: Some("com.tencent.QQMusicMac".to_string()),
        window_title: Some("QQ音乐".to_string()),
        window_number: Some(42),
        region_hint: Some(RatioRegion {
          left: 0.18,
          top: 0.28,
          right: 0.82,
          bottom: 0.92,
        }),
        capture_artifact: Some(capture_artifact.clone()),
        capture_contract_artifact: Some(contract_artifact.clone()),
      },
      best: Some(best.clone()),
      filtered: vec![best.clone(), filtered.clone()],
      all: vec![best.clone(), filtered.clone(), rejected.clone()],
      detail: json!({
        "provider": "vision_ocr.window_rows",
        "strategy": "ocr-first",
        "raw_match_count": 3,
      }),
      evidence: vec![capture_artifact.clone(), contract_artifact.clone()],
      known_limits: vec![
        "provider score is detector-local, not semantic truth".to_string(),
        "window scope depends on the capture contract".to_string(),
      ],
    };

    let value = serde_json::to_value(&result).expect("recognition result should serialize");
    assert_eq!(value["source"], json!("ocr_row"));
    assert_eq!(value["scope"]["surface"], json!("window"));
    assert_eq!(value["best"]["box"]["x"], json!(2155));
    assert_eq!(value["filtered"][1]["box"]["width"], json!(196));
    assert_eq!(
      value["all"][2]["detail"]["reject_reason"],
      json!("below_min_provider_score")
    );
    assert_eq!(value["best"]["provider_score"], json!(0.97));
    assert!(value["best"].get("box_").is_none());
    assert!(value.get("confidence").is_none());

    let parsed: RecognitionResult =
      serde_json::from_value(value).expect("recognition result should deserialize");
    assert_eq!(parsed, result);
  }

  #[test]
  fn recognition_result_round_trips_with_empty_filtered_and_all() {
    let result = RecognitionResult {
      recognition_id: "recognition_empty".to_string(),
      source: RecognitionSource::VisualRow,
      scope: RecognitionScope {
        surface: RecognitionSurface::Region,
        display_ref: None,
        native_display_id: None,
        app_bundle_id: Some("com.tencent.QQMusicMac".to_string()),
        window_title: None,
        window_number: None,
        region_hint: Some(RatioRegion {
          left: 0.22,
          top: 0.30,
          right: 0.88,
          bottom: 0.76,
        }),
        capture_artifact: None,
        capture_contract_artifact: None,
      },
      best: None,
      filtered: Vec::new(),
      all: Vec::new(),
      detail: json!({
        "provider": "visual_rows",
        "strategy": "visual-bands",
      }),
      evidence: Vec::new(),
      known_limits: vec!["no rows detected on this page".to_string()],
    };

    let value = serde_json::to_value(&result).expect("empty recognition result should serialize");
    assert_eq!(value["source"], json!("visual_row"));
    assert_eq!(value["scope"]["surface"], json!("region"));
    assert_eq!(value["best"], serde_json::Value::Null);
    assert_eq!(value["filtered"], json!([]));
    assert_eq!(value["all"], json!([]));

    let parsed: RecognitionResult =
      serde_json::from_value(value).expect("empty recognition result should deserialize");
    assert_eq!(parsed, result);
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
            row_index: None,
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
  fn visual_row_candidate_serializes_row_index_without_anchor_recheck() {
    let artifact = artifact_ref();
    let candidate = Candidate {
      candidate_local_id: "row#2".to_string(),
      kind: "search_result_row".to_string(),
      label: Some("Visual row 2".to_string()),
      target_spec: TargetSpec {
        grounding: TargetGrounding::VisualRow,
        anchor_text: None,
        region_hint: Some(RatioRegion {
          left: 0.1,
          top: 0.2,
          right: 0.9,
          bottom: 0.3,
        }),
        row_index: Some(2),
      },
      evidence: CandidateEvidence {
        artifact_ref: artifact,
        observation: json!({
          "provider": "vision_ocr.window_rows",
          "source": "visual-bands"
        }),
      },
      liveness: CandidateLiveness {
        preconditions: LivenessPreconditions {
          window_ref: Some(WindowRefPrecondition {
            app_bundle_id: "com.tencent.QQMusicMac".to_string(),
            window_title_substring: None,
            window_number: None,
          }),
          anchor_recheck: None,
        },
        ttl_hint_ms: Some(5000),
      },
      control: ControlRequirements {
        requires_app_frontmost: true,
        requires_window_focus: true,
      },
      known_limits: vec!["visual row index may drift after scrolling".to_string()],
    };

    let value = serde_json::to_value(&candidate).expect("candidate should serialize");
    assert_eq!(value["target_spec"]["grounding"], json!("visual_row"));
    assert_eq!(value["target_spec"]["row_index"], json!(2));
    assert_eq!(
      value["liveness"]["preconditions"]["anchor_recheck"],
      serde_json::Value::Null
    );

    let parsed: Candidate = serde_json::from_value(value).expect("candidate should deserialize");
    assert_eq!(parsed, candidate);
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
