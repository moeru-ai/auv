use std::fs;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::candidate_promotion::{
  ActionPermission, CandidatePromotion, PromotionContext, PromotionFreshness, PromotionProjection,
  promote_recognition_to_candidates,
};
use crate::contract::{ArtifactRef, RecognitionResult};
use crate::model::{AuvResult, now_millis};
use crate::recorded_operation::RecordedOperationContext;
use crate::stability::{StabilityAssessment, StabilityPolicy, assess_stability};

const CANDIDATE_PROMOTION_ARTIFACT_ROLE: &str = "candidate-promotion";
const CANDIDATE_PROMOTION_ARTIFACT_VERSION: &str = "candidate_promotion_artifact_v0";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CandidatePromotionArtifactRequest {
  pub promotion_id: String,
  pub source_recognition_artifact: Option<ArtifactRef>,
  pub stability_policy: StabilityPolicy,
  pub projection: PromotionProjection,
  pub freshness: Option<PromotionFreshness>,
  pub permission: Option<ActionPermission>,
  pub checked_at_millis: u64,
  pub artifact_role: String,
  pub artifact_label: String,
  pub artifact_note: String,
}

impl CandidatePromotionArtifactRequest {
  pub fn new(promotion_id: impl Into<String>, artifact_label: impl Into<String>) -> Self {
    let promotion_id = promotion_id.into();
    Self {
      promotion_id: promotion_id.clone(),
      source_recognition_artifact: None,
      stability_policy: StabilityPolicy {
        min_frames: 1,
        max_centroid_drift_px: 0.0,
        require_stable_text: false,
      },
      projection: PromotionProjection::Unavailable {
        reason: "projection context not provided".to_string(),
      },
      freshness: None,
      permission: None,
      checked_at_millis: now_millis(),
      artifact_role: CANDIDATE_PROMOTION_ARTIFACT_ROLE.to_string(),
      artifact_label: artifact_label.into(),
      artifact_note: "Candidate-promotion gate decision runtime artifact.".to_string(),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CandidatePromotionArtifact {
  pub artifact_version: String,
  pub promotion_id: String,
  pub source_recognition_artifact: Option<ArtifactRef>,
  pub observed_recognition_ids: Vec<String>,
  pub promotion_input_recognition_id: String,
  pub promotion_input_frame_index: usize,
  pub stability_policy: StabilityPolicy,
  pub stability_assessment: StabilityAssessment,
  pub promotion_context: PromotionContext,
  pub decision: CandidatePromotion,
  pub recognition: RecognitionResult,
  pub detail: serde_json::Value,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CandidatePromotionArtifactError {
  NoRecognitionFrames,
}

impl std::fmt::Display for CandidatePromotionArtifactError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::NoRecognitionFrames => {
        write!(
          f,
          "candidate promotion recording requires at least one RecognitionResult"
        )
      }
    }
  }
}

impl std::error::Error for CandidatePromotionArtifactError {}

pub fn build_candidate_promotion_artifact(
  observations: &[RecognitionResult],
  request: &CandidatePromotionArtifactRequest,
) -> Result<CandidatePromotionArtifact, CandidatePromotionArtifactError> {
  let Some((promotion_input_frame_index, recognition)) = observations.iter().enumerate().last()
  else {
    return Err(CandidatePromotionArtifactError::NoRecognitionFrames);
  };

  let stability_assessment = assess_stability(observations, &request.stability_policy);
  let promotion_context = PromotionContext {
    projection: request.projection.clone(),
    stability: stability_assessment.to_promotion_stability_input(),
    freshness: request.freshness.clone(),
    permission: request.permission.clone(),
    checked_at_millis: request.checked_at_millis,
  };
  let decision = promote_recognition_to_candidates(recognition, &promotion_context);

  Ok(CandidatePromotionArtifact {
    artifact_version: CANDIDATE_PROMOTION_ARTIFACT_VERSION.to_string(),
    promotion_id: request.promotion_id.clone(),
    source_recognition_artifact: request.source_recognition_artifact.clone(),
    observed_recognition_ids: observations
      .iter()
      .map(|recognition| recognition.recognition_id.clone())
      .collect(),
    promotion_input_recognition_id: recognition.recognition_id.clone(),
    promotion_input_frame_index,
    stability_policy: request.stability_policy.clone(),
    stability_assessment,
    promotion_context,
    decision: decision.clone(),
    recognition: recognition.clone(),
    detail: json!({
      "artifact_version": CANDIDATE_PROMOTION_ARTIFACT_VERSION,
      "frame_selection_strategy": "latest_observation",
      "producer": "candidate_promotion_recording",
      "observed_frame_count": observations.len(),
      "decision_kind": decision_kind(&decision),
      "source_recognition_artifact_present": request.source_recognition_artifact.is_some(),
    }),
    known_limits: artifact_known_limits(recognition, &decision),
  })
}

#[cfg(target_os = "macos")]
pub fn build_candidate_promotion_artifact_with_recognition_projection(
  observations: &[RecognitionResult],
  request: &CandidatePromotionArtifactRequest,
) -> Result<CandidatePromotionArtifact, CandidatePromotionArtifactError> {
  let Some((_, recognition)) = observations.iter().enumerate().last() else {
    return Err(CandidatePromotionArtifactError::NoRecognitionFrames);
  };
  let mut request = request.clone();
  request.projection = crate::ax_recognition::promotion_projection_for_recognition(recognition);
  build_candidate_promotion_artifact(observations, &request)
}

pub fn record_candidate_promotion_artifact(
  context: &mut RecordedOperationContext<'_>,
  observations: &[RecognitionResult],
  request: &CandidatePromotionArtifactRequest,
) -> AuvResult<(ArtifactRef, CandidatePromotionArtifact)> {
  let artifact = build_candidate_promotion_artifact(observations, request)
    .map_err(|error| format!("failed to build candidate-promotion artifact: {error}"))?;
  record_built_candidate_promotion_artifact(context, artifact, request)
}

#[cfg(target_os = "macos")]
pub fn record_candidate_promotion_artifact_with_recognition_projection(
  context: &mut RecordedOperationContext<'_>,
  observations: &[RecognitionResult],
  request: &CandidatePromotionArtifactRequest,
) -> AuvResult<(ArtifactRef, CandidatePromotionArtifact)> {
  let artifact =
    build_candidate_promotion_artifact_with_recognition_projection(observations, request)
      .map_err(|error| format!("failed to build candidate-promotion artifact: {error}"))?;
  record_built_candidate_promotion_artifact(context, artifact, request)
}

fn record_built_candidate_promotion_artifact(
  context: &mut RecordedOperationContext<'_>,
  artifact: CandidatePromotionArtifact,
  request: &CandidatePromotionArtifactRequest,
) -> AuvResult<(ArtifactRef, CandidatePromotionArtifact)> {
  let rendered = serde_json::to_string_pretty(&artifact)
    .map(|mut rendered| {
      rendered.push('\n');
      rendered
    })
    .map_err(|error| format!("failed to encode candidate-promotion artifact JSON: {error}"))?;
  let artifact_source_path = candidate_promotion_temp_json_path(&request.artifact_label);
  fs::write(&artifact_source_path, rendered).map_err(|error| {
    format!(
      "failed to write candidate-promotion temp artifact {}: {error}",
      artifact_source_path.display()
    )
  })?;

  let (_, artifact_ref) = context.stage_artifact_file_with_ref(
    &request.artifact_role,
    &artifact_source_path,
    format!("{}.json", sanitize_artifact_label(&request.artifact_label)),
    Some(request.artifact_note.clone()),
  )?;
  let _ = fs::remove_file(&artifact_source_path);

  context.record_event(
    "candidate.promotion.artifact_recorded",
    Some(format!(
      "recorded {} from recognition {}",
      artifact_ref.artifact_id, artifact.promotion_input_recognition_id
    )),
  );

  Ok((artifact_ref, artifact))
}

fn decision_kind(decision: &CandidatePromotion) -> &'static str {
  match decision {
    CandidatePromotion::Refused { .. } => "refused",
    CandidatePromotion::Promoted { .. } => "promoted",
  }
}

fn artifact_known_limits(
  recognition: &RecognitionResult,
  decision: &CandidatePromotion,
) -> Vec<String> {
  let mut known_limits = recognition.known_limits.clone();
  push_known_limit(
    &mut known_limits,
    "candidate promotion v0 selects the latest recognition frame after stability assessment",
  );
  push_known_limit(
    &mut known_limits,
    "candidate promotion artifact records gate decisions only; runtime action consumption remains deferred",
  );
  if let CandidatePromotion::Promoted {
    residual_known_limits,
    ..
  } = decision
  {
    for limit in residual_known_limits {
      push_known_limit(&mut known_limits, limit);
    }
  }
  known_limits
}

fn push_known_limit(known_limits: &mut Vec<String>, value: impl AsRef<str>) {
  let value = value.as_ref();
  if !known_limits.iter().any(|existing| existing == value) {
    known_limits.push(value.to_string());
  }
}

fn candidate_promotion_temp_json_path(label: &str) -> std::path::PathBuf {
  std::env::temp_dir().join(format!(
    "auv-candidate-promotion-{}-{}-{}.json",
    sanitize_artifact_label(label),
    now_millis(),
    std::process::id()
  ))
}

fn sanitize_artifact_label(raw: &str) -> String {
  let sanitized = raw
    .chars()
    .map(|character| match character {
      'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => character,
      _ => '-',
    })
    .collect::<String>()
    .trim_matches('-')
    .to_string();
  if sanitized.is_empty() {
    "artifact".to_string()
  } else {
    sanitized
  }
}

#[cfg(test)]
mod tests {
  use std::fs;
  use std::path::PathBuf;

  use serde_json::json;

  use super::{
    CandidatePromotionArtifactError, CandidatePromotionArtifactRequest,
    build_candidate_promotion_artifact, record_candidate_promotion_artifact,
  };
  use crate::build_runtime_with_store_root;
  use crate::candidate_promotion::{
    ActionConsentAction, ActionConsentRecord, ActionConsentScope, ActionPermission,
    CandidatePromotion, PromotionFreshness, PromotionProjection,
  };
  use crate::contract::{
    ArtifactRef, RecognitionBox, RecognitionResult, RecognitionScope, RecognitionSource,
    RecognitionSurface, RecognizedItem,
  };
  use crate::run_builder::RunSpec;
  use crate::stability::StabilityPolicy;
  use crate::trace::{ArtifactId, EventId, RunId, RunType, SpanId, TraceStatusCode};

  fn sample_artifact_ref() -> ArtifactRef {
    ArtifactRef {
      run_id: RunId::new("run_candidate_promotion_source"),
      artifact_id: ArtifactId::new("artifact_capture"),
      span_id: SpanId::new("span_candidate_promotion_source"),
      captured_event_id: Some(EventId::new("event_capture")),
    }
  }

  fn sample_frame(recognition_id: &str, x: i64, y: i64) -> RecognitionResult {
    let capture_artifact = sample_artifact_ref();
    RecognitionResult {
      recognition_id: recognition_id.to_string(),
      source: RecognitionSource::Custom,
      scope: RecognitionScope {
        surface: RecognitionSurface::Window,
        display_ref: Some("display-main".to_string()),
        native_display_id: Some("69733248".to_string()),
        app_bundle_id: Some("com.megacrit.cardcrawl".to_string()),
        window_title: Some("Slay the Spire".to_string()),
        window_number: Some(7),
        region_hint: None,
        capture_artifact: Some(capture_artifact.clone()),
        capture_contract_artifact: Some(ArtifactRef {
          run_id: RunId::new("run_candidate_promotion_source"),
          artifact_id: ArtifactId::new("artifact_capture_contract"),
          span_id: SpanId::new("span_candidate_promotion_source"),
          captured_event_id: Some(EventId::new("event_capture_contract")),
        }),
      },
      best: Some(RecognizedItem {
        item_id: "item_end_turn".to_string(),
        kind: "button".to_string(),
        box_: RecognitionBox {
          x,
          y,
          width: 228,
          height: 178,
        },
        text: Some("End Turn".to_string()),
        provider_score: Some(0.99),
        detail: json!({
          "backend": "manual-fixture"
        }),
      }),
      filtered: vec![RecognizedItem {
        item_id: "item_end_turn".to_string(),
        kind: "button".to_string(),
        box_: RecognitionBox {
          x,
          y,
          width: 228,
          height: 178,
        },
        text: Some("End Turn".to_string()),
        provider_score: Some(0.99),
        detail: json!({
          "backend": "manual-fixture"
        }),
      }],
      all: vec![RecognizedItem {
        item_id: "item_end_turn".to_string(),
        kind: "button".to_string(),
        box_: RecognitionBox {
          x,
          y,
          width: 228,
          height: 178,
        },
        text: Some("End Turn".to_string()),
        provider_score: Some(0.99),
        detail: json!({
          "backend": "manual-fixture"
        }),
      }],
      detail: json!({
        "backend": "manual-fixture",
        "model_id": "slay-the-spire-observe-only",
      }),
      evidence: vec![
        capture_artifact,
        ArtifactRef {
          run_id: RunId::new("run_candidate_promotion_source"),
          artifact_id: ArtifactId::new("artifact_capture_contract"),
          span_id: SpanId::new("span_candidate_promotion_source"),
          captured_event_id: Some(EventId::new("event_capture_contract")),
        },
      ],
      known_limits: vec!["fixture-backed recognition only".to_string()],
    }
  }

  fn sample_request() -> CandidatePromotionArtifactRequest {
    CandidatePromotionArtifactRequest {
      promotion_id: "promotion_end_turn".to_string(),
      source_recognition_artifact: Some(ArtifactRef {
        run_id: RunId::new("run_candidate_promotion_source"),
        artifact_id: ArtifactId::new("artifact_recognition_source"),
        span_id: SpanId::new("span_candidate_promotion_source"),
        captured_event_id: Some(EventId::new("event_recognition_source")),
      }),
      stability_policy: StabilityPolicy {
        min_frames: 3,
        max_centroid_drift_px: 10.0,
        require_stable_text: true,
      },
      projection: PromotionProjection::IdentityWindowAddressable,
      freshness: Some(PromotionFreshness {
        source_artifact: Some(sample_artifact_ref()),
        source_operation_id: Some("observe.window.capture".to_string()),
        observed_at_millis: Some(2_000),
        max_age_ms: Some(500),
        notes: vec!["fixture freshness seed".to_string()],
      }),
      permission: Some(ActionPermission {
        granted_by: "human-review".to_string(),
        scope_note: "single end-turn action".to_string(),
        consent: ActionConsentRecord {
          consent_id: "consent_end_turn".to_string(),
          recognition_id: "recognition_frame_3".to_string(),
          run_id: sample_artifact_ref().run_id,
          scope: ActionConsentScope {
            surface: RecognitionSurface::Window,
            app_bundle_id: Some("com.megacrit.cardcrawl".to_string()),
            window_title: Some("Slay the Spire".to_string()),
            window_number: Some(7),
          },
          approved_action: ActionConsentAction::CandidatePromotion,
          target_item_id: "item_end_turn".to_string(),
          approved_at_millis: 2_000,
          expires_at_millis: Some(2_500),
          evidence_note: "fixture approval".to_string(),
        },
      }),
      checked_at_millis: 2_100,
      artifact_role: "candidate-promotion".to_string(),
      artifact_label: "slay-the-spire-end-turn-promotion".to_string(),
      artifact_note: "Candidate-promotion gate decision for Slay the Spire fixture.".to_string(),
    }
  }

  #[test]
  fn empty_observation_list_is_rejected() {
    let error = build_candidate_promotion_artifact(&[], &sample_request())
      .expect_err("empty promotion input should be rejected");

    assert_eq!(error, CandidatePromotionArtifactError::NoRecognitionFrames);
  }

  #[test]
  fn builder_assesses_stability_and_promotes_latest_frame() {
    let observations = vec![
      sample_frame("recognition_frame_1", 1638, 792),
      sample_frame("recognition_frame_2", 1641, 794),
      sample_frame("recognition_frame_3", 1643, 796),
    ];
    let artifact = build_candidate_promotion_artifact(&observations, &sample_request())
      .expect("stable observations should build candidate-promotion artifact");

    assert_eq!(artifact.promotion_input_frame_index, 2);
    assert_eq!(
      artifact.promotion_input_recognition_id,
      "recognition_frame_3"
    );
    assert_eq!(artifact.observed_recognition_ids.len(), 3);
    assert_eq!(
      artifact
        .promotion_context
        .permission
        .as_ref()
        .map(|permission| permission.granted_by.as_str()),
      Some("human-review")
    );
    match artifact.decision {
      CandidatePromotion::Promoted { candidates, .. } => {
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].candidate_local_id, "promoted-item_end_turn");
      }
      CandidatePromotion::Refused { reasons } => {
        panic!("expected promoted decision, got refusal reasons: {reasons:?}");
      }
    }
    assert_eq!(
      artifact.detail["frame_selection_strategy"],
      json!("latest_observation")
    );
    assert!(
      artifact
        .known_limits
        .contains(&"candidate promotion artifact records gate decisions only; runtime action consumption remains deferred".to_string())
    );
  }

  #[test]
  fn recorded_operation_persists_candidate_promotion_artifact() {
    let project_root = temp_dir("candidate-promotion-record-project");
    let store_root = temp_dir("candidate-promotion-record-store");
    fs::create_dir_all(&project_root).expect("project root should exist");
    let runtime = build_runtime_with_store_root(project_root.clone(), store_root.clone())
      .expect("runtime should build");
    let observations = vec![
      sample_frame("recognition_frame_1", 1638, 792),
      sample_frame("recognition_frame_2", 1641, 794),
      sample_frame("recognition_frame_3", 1643, 796),
    ];
    let request = sample_request();

    let output = runtime
      .run_recorded_operation(
        RunSpec::new(RunType::Execute, "auv.candidate.promotion"),
        "Candidate promotion artifact recording",
        |context| {
          let source_path = project_root.join("recognition-source.json");
          fs::write(&source_path, "{\"fixture\":true}\n").expect("recognition source should write");
          let (_, source_artifact_ref) = context
            .stage_artifact_file_with_ref(
              "detector-recognition",
              &source_path,
              "recognition-source.json",
              Some("Recorded source recognition artifact.".to_string()),
            )
            .expect("source recognition artifact should stage");

          let mut request = request.clone();
          request.source_recognition_artifact = Some(source_artifact_ref);
          record_candidate_promotion_artifact(context, &observations, &request)
        },
      )
      .expect("recorded candidate promotion operation should succeed");

    let run = runtime
      .read_run(output.run_id.as_str())
      .expect("recorded run should persist");
    assert_eq!(run.run.status_code, TraceStatusCode::Ok);
    assert_eq!(run.artifacts.len(), 2);
    assert_eq!(run.artifacts[0].role, "detector-recognition");
    assert_eq!(run.artifacts[1].role, "candidate-promotion");

    let (artifact_ref, artifact) = output.value;
    assert_eq!(artifact_ref.run_id, output.run_id);
    assert_eq!(
      artifact.promotion_input_recognition_id,
      "recognition_frame_3"
    );
    assert_eq!(
      artifact
        .source_recognition_artifact
        .as_ref()
        .map(|reference| reference.artifact_id.as_str()),
      Some("artifact_0001")
    );

    let promotion_artifact = run
      .artifacts
      .iter()
      .find(|artifact_record| artifact_record.artifact_id == artifact_ref.artifact_id)
      .expect("candidate-promotion artifact should exist in recorded run");
    let promotion_path = output.run_dir.join(&promotion_artifact.path);
    let recorded_artifact: super::CandidatePromotionArtifact = serde_json::from_slice(
      &fs::read(&promotion_path).expect("promotion artifact bytes should read"),
    )
    .expect("promotion artifact JSON should decode");
    assert_eq!(recorded_artifact.promotion_id, "promotion_end_turn");
    assert!(matches!(
      recorded_artifact.decision,
      CandidatePromotion::Promoted { .. }
    ));
    assert!(
      run
        .events
        .iter()
        .any(|event| event.name == "candidate.promotion.artifact_recorded")
    );

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[cfg(target_os = "macos")]
  #[test]
  fn ax_backed_recognition_satisfies_projection_without_manual_context() {
    use super::build_candidate_promotion_artifact_with_recognition_projection;
    use crate::ax_recognition::{
      AxBestSelectionStrategy, AxRecognitionPolicy, AxRecognitionRuntimeContext,
      map_ax_tree_to_recognition_result,
    };
    use auv_driver_macos::types::{ObservedAxNode, ObservedAxTreeSnapshot, ObservedRect};

    fn ax_node(path: &str, title: &str, x: i64, y: i64) -> ObservedAxNode {
      ObservedAxNode {
        depth: path.matches('.').count(),
        path: path.to_string(),
        role: "AXButton".to_string(),
        subrole: String::new(),
        title: title.to_string(),
        description: String::new(),
        help: String::new(),
        identifier: String::new(),
        placeholder: String::new(),
        value: String::new(),
        bounds: ObservedRect {
          x,
          y,
          width: 120,
          height: 40,
        },
      }
    }

    let snapshot = ObservedAxTreeSnapshot {
      observed_at: "2026-06-07T10:00:00Z".to_string(),
      app_name: "Notes".to_string(),
      bundle_id: "com.apple.Notes".to_string(),
      pid: 42,
      window_title: "Notes".to_string(),
      nodes: vec![ax_node("0.0", "Done", 100, 200)],
    };
    let ax_recognition = map_ax_tree_to_recognition_result(
      &snapshot,
      &AxRecognitionRuntimeContext {
        recognition_id: "recognition_ax_done".to_string(),
        source_artifact: sample_artifact_ref(),
      },
      &AxRecognitionPolicy {
        query: Some("Done".to_string()),
        role: Some("AXButton".to_string()),
        require_bounds: true,
        best_selection: AxBestSelectionStrategy::SingleFilteredItem,
      },
    )
    .expect("AX snapshot should map to addressable RecognitionResult");

    let mut request = sample_request();
    request.projection = PromotionProjection::Unavailable {
      reason: "caller did not provide projection".to_string(),
    };
    request.freshness = None;
    request.permission = None;

    let artifact =
      build_candidate_promotion_artifact_with_recognition_projection(&[ax_recognition], &request)
        .expect("AX-backed recognition should build promotion artifact");

    assert_eq!(
      artifact.promotion_context.projection,
      PromotionProjection::IdentityWindowAddressable
    );
    match artifact.decision {
      CandidatePromotion::Refused { reasons } => {
        assert!(
          !reasons.iter().any(|reason| matches!(
            reason,
            crate::candidate_promotion::PromotionRefusal::ProjectionUnavailable { .. }
          )),
          "AX projection should not remain refused: {reasons:?}"
        );
        assert!(
          reasons.iter().any(|reason| matches!(
            reason,
            crate::candidate_promotion::PromotionRefusal::FreshnessUnknown
          )),
          "freshness remains intentionally deferred for the next slice"
        );
        assert!(
          reasons.iter().any(|reason| matches!(
            reason,
            crate::candidate_promotion::PromotionRefusal::PermissionMissing
          )),
          "permission remains intentionally deferred for the next slice"
        );
      }
      CandidatePromotion::Promoted { .. } => {
        panic!("freshness and permission are intentionally absent in this slice");
      }
    }
  }

  #[cfg(target_os = "macos")]
  #[test]
  fn gated_ax_report_records_projection_satisfied_candidate_promotion_lineage() {
    use super::record_candidate_promotion_artifact_with_recognition_projection;
    use crate::ax_recognition::{
      AxBestSelectionStrategy, AxRecognitionArtifactRequest, AxRecognitionPolicy,
      record_ax_tree_recognition_artifact,
    };
    use auv_driver_macos::support::parse_observed_ax_tree;

    let Ok(ax_report_path) = std::env::var("AUV_AX_TREE_REPORT") else {
      eprintln!("skipping gated AX projection smoke: AUV_AX_TREE_REPORT is not set");
      return;
    };
    let ax_report_path = PathBuf::from(ax_report_path);
    let report = fs::read_to_string(&ax_report_path)
      .expect("AUV_AX_TREE_REPORT should point at a readable AX tree report");
    let snapshot = parse_observed_ax_tree(&report).expect("AX tree report should parse");
    let query = std::env::var("AUV_AX_QUERY").unwrap_or_else(|_| "First Text View".to_string());
    let role = std::env::var("AUV_AX_ROLE").unwrap_or_else(|_| "AXTextArea".to_string());
    let project_root = temp_dir("ax-projection-live-smoke-project");
    let store_root = temp_dir("ax-projection-live-smoke-store");
    fs::create_dir_all(&project_root).expect("project root should exist");
    let runtime = build_runtime_with_store_root(project_root.clone(), store_root.clone())
      .expect("runtime should build");

    let output = runtime
      .run_recorded_operation(
        RunSpec::new(RunType::Execute, "auv.ax.projection.smoke"),
        "AX projection candidate-promotion smoke",
        |context| {
          let (_, recognition_ref, recognition) = record_ax_tree_recognition_artifact(
            context,
            &snapshot,
            &ax_report_path,
            "ax-tree",
            "ax-tree.txt",
            Some("Source AX tree artifact for projection smoke.".to_string()),
            &AxRecognitionArtifactRequest {
              recognition_id: "recognition_ax_projection_smoke".to_string(),
              policy: AxRecognitionPolicy {
                query: Some(query.clone()),
                role: Some(role.clone()),
                require_bounds: true,
                best_selection: AxBestSelectionStrategy::SingleFilteredItem,
              },
              artifact_role: "ax-recognition".to_string(),
              artifact_label: "ax-projection-smoke-recognition".to_string(),
              artifact_note: "AX-backed RecognitionResult for projection smoke.".to_string(),
            },
          )?;

          let mut request = CandidatePromotionArtifactRequest::new(
            "promotion_ax_projection_smoke",
            "ax-projection-smoke-promotion",
          );
          request.source_recognition_artifact = Some(recognition_ref);
          request.stability_policy = StabilityPolicy {
            min_frames: 1,
            max_centroid_drift_px: 0.0,
            require_stable_text: false,
          };
          request.projection = PromotionProjection::Unavailable {
            reason: "smoke starts without caller-supplied projection".to_string(),
          };

          record_candidate_promotion_artifact_with_recognition_projection(
            context,
            &[recognition],
            &request,
          )
        },
      )
      .expect("gated AX projection smoke should record artifacts");

    let (_promotion_ref, artifact) = output.value;
    assert_eq!(
      artifact.promotion_context.projection,
      PromotionProjection::IdentityWindowAddressable
    );
    let inspect = runtime
      .inspect(output.run_id.as_str())
      .expect("recorded smoke run should inspect");
    assert!(inspect.contains("Candidate Promotion Lineage:"));
    assert!(inspect.contains("projection=identity_window_addressable"));
    assert!(!inspect.contains("projection_unavailable"));

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  fn temp_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("auv-{}-{}", label, crate::model::now_millis()))
  }
}
