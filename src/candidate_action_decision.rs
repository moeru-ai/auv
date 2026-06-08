use std::fs;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::action_resolver_decision::{ActionResolverDecision, ActionResolverDecisionInput};
use crate::candidate_promotion::CandidatePromotion;
use crate::candidate_promotion_recording::CandidatePromotionArtifact;
use crate::contract::{ArtifactRef, Candidate, TargetGrounding};
use crate::model::{AuvResult, now_millis};
use crate::recorded_operation::RecordedOperationContext;

pub const CANDIDATE_ACTION_DECISION_ARTIFACT_ROLE: &str = "candidate-action-decision";
const CANDIDATE_ACTION_DECISION_ARTIFACT_VERSION: &str = "candidate_action_decision_artifact_v0";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateActionDecisionRequest {
  pub(crate) decision_id: String,
  pub(crate) source_candidate_promotion_artifact: Option<ArtifactRef>,
  pub(crate) candidate_local_id: Option<String>,
  pub(crate) artifact_role: String,
  pub(crate) artifact_label: String,
  pub(crate) artifact_note: String,
}

impl CandidateActionDecisionRequest {
  pub fn new(decision_id: impl Into<String>, artifact_label: impl Into<String>) -> Self {
    let decision_id = decision_id.into();
    Self {
      decision_id: decision_id.clone(),
      source_candidate_promotion_artifact: None,
      candidate_local_id: None,
      artifact_role: CANDIDATE_ACTION_DECISION_ARTIFACT_ROLE.to_string(),
      artifact_label: artifact_label.into(),
      artifact_note: "Decide-only candidate action resolver decision artifact.".to_string(),
    }
  }

  pub fn with_source_candidate_promotion_artifact(mut self, artifact: ArtifactRef) -> Self {
    self.source_candidate_promotion_artifact = Some(artifact);
    self
  }

  pub fn with_candidate_local_id(mut self, candidate_local_id: impl Into<String>) -> Self {
    self.candidate_local_id = Some(candidate_local_id.into());
    self
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateActionDecisionArtifact {
  pub(crate) artifact_version: String,
  pub(crate) decision_id: String,
  pub(crate) source_candidate_promotion_artifact: Option<ArtifactRef>,
  pub(crate) source_promotion_id: String,
  pub(crate) candidate_local_id: String,
  pub(crate) action_resolver_decision: ActionResolverDecision,
  pub(crate) side_effect: CandidateActionSideEffect,
  pub(crate) detail: serde_json::Value,
  pub(crate) known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateActionSideEffect {
  NoneDecideOnly,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CandidateActionDecisionError {
  PromotionDidNotPromote,
  NoPromotedCandidates,
  CandidateNotFound { candidate_local_id: String },
  UnsupportedTargetGrounding { grounding: TargetGrounding },
}

impl std::fmt::Display for CandidateActionDecisionError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::PromotionDidNotPromote => write!(
        f,
        "candidate action decide-only requires a promoted CandidatePromotion"
      ),
      Self::NoPromotedCandidates => write!(
        f,
        "candidate action decide-only requires at least one promoted Candidate"
      ),
      Self::CandidateNotFound { candidate_local_id } => {
        write!(
          f,
          "candidate {candidate_local_id} was not promoted by the source artifact"
        )
      }
      Self::UnsupportedTargetGrounding { grounding } => {
        write!(
          f,
          "candidate target grounding {grounding:?} is not supported by L8a decide-only"
        )
      }
    }
  }
}

impl std::error::Error for CandidateActionDecisionError {}

pub fn build_candidate_action_decision_artifact(
  promotion: &CandidatePromotionArtifact,
  request: &CandidateActionDecisionRequest,
) -> Result<CandidateActionDecisionArtifact, CandidateActionDecisionError> {
  let candidates = match &promotion.decision {
    CandidatePromotion::Promoted { candidates, .. } => candidates,
    CandidatePromotion::Refused { .. } => {
      return Err(CandidateActionDecisionError::PromotionDidNotPromote);
    }
  };
  let candidate = select_candidate(candidates, request)?;
  let action_resolver_decision = decide_candidate_action(candidate)?;
  let known_limits = artifact_known_limits(promotion, candidate);

  Ok(CandidateActionDecisionArtifact {
    artifact_version: CANDIDATE_ACTION_DECISION_ARTIFACT_VERSION.to_string(),
    decision_id: request.decision_id.clone(),
    source_candidate_promotion_artifact: request.source_candidate_promotion_artifact.clone(),
    source_promotion_id: promotion.promotion_id.clone(),
    candidate_local_id: candidate.candidate_local_id.clone(),
    action_resolver_decision,
    side_effect: CandidateActionSideEffect::NoneDecideOnly,
    detail: json!({
      "artifact_version": CANDIDATE_ACTION_DECISION_ARTIFACT_VERSION,
      "producer": "candidate_action_decision",
      "mode": "decide_only",
      "input_delivery": "not_attempted",
      "operation_result": "not_produced",
      "verification_result": "not_produced",
      "source_promotion_decision": "promoted",
      "source_promotion_id": promotion.promotion_id,
      "candidate_local_id": candidate.candidate_local_id,
      "target_grounding": target_grounding_kind(candidate.target_spec.grounding),
    }),
    known_limits,
  })
}

pub fn record_candidate_action_decision_artifact(
  context: &mut RecordedOperationContext<'_>,
  promotion: &CandidatePromotionArtifact,
  request: &CandidateActionDecisionRequest,
) -> AuvResult<(ArtifactRef, CandidateActionDecisionArtifact)> {
  let artifact = build_candidate_action_decision_artifact(promotion, request)
    .map_err(|error| format!("failed to build candidate action decision artifact: {error}"))?;
  let rendered = serde_json::to_string_pretty(&artifact)
    .map(|mut rendered| {
      rendered.push('\n');
      rendered
    })
    .map_err(|error| {
      format!("failed to encode candidate action decision artifact JSON: {error}")
    })?;
  let artifact_source_path = temp_json_path(&request.artifact_label);
  fs::write(&artifact_source_path, rendered).map_err(|error| {
    format!(
      "failed to write candidate action decision temp artifact {}: {error}",
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
    "candidate.action.decision.artifact_recorded",
    Some(format!(
      "recorded decide-only action decision {} for candidate {}",
      artifact_ref.artifact_id, artifact.candidate_local_id
    )),
  );

  Ok((artifact_ref, artifact))
}

fn select_candidate<'a>(
  candidates: &'a [Candidate],
  request: &CandidateActionDecisionRequest,
) -> Result<&'a Candidate, CandidateActionDecisionError> {
  if candidates.is_empty() {
    return Err(CandidateActionDecisionError::NoPromotedCandidates);
  }
  if let Some(candidate_local_id) = request.candidate_local_id.as_deref() {
    return candidates
      .iter()
      .find(|candidate| candidate.candidate_local_id == candidate_local_id)
      .ok_or_else(|| CandidateActionDecisionError::CandidateNotFound {
        candidate_local_id: candidate_local_id.to_string(),
      });
  }
  Ok(&candidates[0])
}

fn decide_candidate_action(
  candidate: &Candidate,
) -> Result<ActionResolverDecision, CandidateActionDecisionError> {
  match candidate.target_spec.grounding {
    TargetGrounding::AxNode => Ok(ActionResolverDecision::new(ActionResolverDecisionInput {
      operation: "candidate.action.decide_only",
      target_query: &target_query(candidate),
      primary_method: "ax-action",
      selected_method: "ax-action",
      fallback_allowed: false,
      fallback_used: false,
      fallback_reason: None,
      policy: "candidate-ax-node",
      cursor_disturbance: "none",
      press_mechanism: "ax-action",
    })),
    TargetGrounding::Coordinate => Ok(ActionResolverDecision::new(ActionResolverDecisionInput {
      operation: "candidate.action.decide_only",
      target_query: &target_query(candidate),
      primary_method: "pointer-click",
      selected_method: "pointer-click",
      fallback_allowed: false,
      fallback_used: false,
      fallback_reason: None,
      policy: "candidate-coordinate-pointer",
      cursor_disturbance: "warp-visible",
      press_mechanism: "pointer-click",
    })),
    TargetGrounding::OcrAnchor | TargetGrounding::VisualRow => {
      Err(CandidateActionDecisionError::UnsupportedTargetGrounding {
        grounding: candidate.target_spec.grounding,
      })
    }
  }
}

fn target_query(candidate: &Candidate) -> String {
  candidate
    .target_spec
    .anchor_text
    .clone()
    .or_else(|| candidate.label.clone())
    .unwrap_or_else(|| candidate.candidate_local_id.clone())
}

fn artifact_known_limits(
  promotion: &CandidatePromotionArtifact,
  candidate: &Candidate,
) -> Vec<String> {
  let mut known_limits = promotion.known_limits.clone();
  for limit in &candidate.known_limits {
    push_known_limit(&mut known_limits, limit);
  }
  push_known_limit(
    &mut known_limits,
    "L8a records an ActionResolverDecision only; it does not call auv-driver or produce InputActionResult",
  );
  push_known_limit(
    &mut known_limits,
    "L8a does not prove semantic success; verification remains deferred to L8b",
  );
  known_limits
}

fn push_known_limit(known_limits: &mut Vec<String>, value: impl AsRef<str>) {
  let value = value.as_ref();
  if !known_limits.iter().any(|existing| existing == value) {
    known_limits.push(value.to_string());
  }
}

fn target_grounding_kind(grounding: TargetGrounding) -> &'static str {
  match grounding {
    TargetGrounding::OcrAnchor => "ocr_anchor",
    TargetGrounding::VisualRow => "visual_row",
    TargetGrounding::AxNode => "ax_node",
    TargetGrounding::Coordinate => "coordinate",
  }
}

fn temp_json_path(label: &str) -> std::path::PathBuf {
  std::env::temp_dir().join(format!(
    "auv-candidate-action-decision-{}-{}-{}.json",
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
    CandidateActionDecisionError, CandidateActionDecisionRequest,
    build_candidate_action_decision_artifact, record_candidate_action_decision_artifact,
  };
  use crate::build_runtime_with_store_root;
  use crate::candidate_promotion::CandidatePromotion;
  use crate::candidate_promotion_recording::{
    CandidatePromotionArtifactRequest, CandidatePromotionConsentInput,
    build_candidate_promotion_artifact, explicit_consent_for_candidate_promotion,
    freshness_from_capture_backed_recognition,
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
      run_id: RunId::new("run_candidate_action_decision_source"),
      artifact_id: ArtifactId::new("artifact_capture"),
      span_id: SpanId::new("span_candidate_action_decision_source"),
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
        app_bundle_id: Some("com.apple.TextEdit".to_string()),
        window_title: Some("Untitled".to_string()),
        window_number: Some(11),
        region_hint: None,
        capture_artifact: Some(capture_artifact.clone()),
        capture_contract_artifact: None,
      },
      best: Some(RecognizedItem {
        item_id: "item_text_area".to_string(),
        kind: "text_area".to_string(),
        box_: RecognitionBox {
          x,
          y,
          width: 300,
          height: 80,
        },
        text: Some("Text Area".to_string()),
        provider_score: Some(0.99),
        detail: json!({"backend": "ax-fixture"}),
      }),
      filtered: vec![RecognizedItem {
        item_id: "item_text_area".to_string(),
        kind: "text_area".to_string(),
        box_: RecognitionBox {
          x,
          y,
          width: 300,
          height: 80,
        },
        text: Some("Text Area".to_string()),
        provider_score: Some(0.99),
        detail: json!({"backend": "ax-fixture"}),
      }],
      all: vec![RecognizedItem {
        item_id: "item_text_area".to_string(),
        kind: "text_area".to_string(),
        box_: RecognitionBox {
          x,
          y,
          width: 300,
          height: 80,
        },
        text: Some("Text Area".to_string()),
        provider_score: Some(0.99),
        detail: json!({"backend": "ax-fixture"}),
      }],
      detail: json!({"backend": "ax-fixture"}),
      evidence: vec![capture_artifact],
      known_limits: vec!["fixture-backed recognition only".to_string()],
    }
  }

  fn promoted_artifact() -> crate::candidate_promotion_recording::CandidatePromotionArtifact {
    let observations = vec![
      sample_frame("recognition_frame_1", 10, 20),
      sample_frame("recognition_frame_2", 11, 20),
    ];
    let latest = observations.last().expect("latest frame exists");
    let mut request =
      CandidatePromotionArtifactRequest::new("promotion_text_area", "promotion-text-area");
    request.source_recognition_artifact = Some(ArtifactRef {
      run_id: RunId::new("run_candidate_action_decision_source"),
      artifact_id: ArtifactId::new("artifact_recognition"),
      span_id: SpanId::new("span_candidate_action_decision_source"),
      captured_event_id: Some(EventId::new("event_recognition")),
    });
    request.stability_policy = StabilityPolicy {
      min_frames: 2,
      max_centroid_drift_px: 4.0,
      require_stable_text: true,
    };
    request.projection = crate::candidate_promotion::PromotionProjection::IdentityWindowAddressable;
    request.freshness = Some(
      freshness_from_capture_backed_recognition(latest, "debug.captureAxTree", "fresh")
        .expect("latest recognition is capture-backed"),
    );
    request.permission = Some(
      explicit_consent_for_candidate_promotion(
        &request.promotion_id,
        latest,
        CandidatePromotionConsentInput {
          granted_by: "human-review".to_string(),
          scope_note: "candidate promotion only, no action execution".to_string(),
          evidence_note: "unit test consent".to_string(),
          approved_at_millis: 1,
        },
      )
      .expect("latest recognition is capture-backed"),
    );
    build_candidate_promotion_artifact(&observations, &request)
      .expect("promotion artifact should build")
  }

  #[test]
  fn build_decide_only_artifact_from_promoted_candidate_without_input_delivery() {
    let promotion = promoted_artifact();
    let request =
      CandidateActionDecisionRequest::new("decision_text_area", "text-area-action-decision");
    let artifact = build_candidate_action_decision_artifact(&promotion, &request)
      .expect("promoted candidate should produce decide-only artifact");

    assert_eq!(artifact.source_promotion_id, "promotion_text_area");
    assert_eq!(artifact.candidate_local_id, "promoted-item_text_area");
    assert_eq!(
      artifact.action_resolver_decision.operation,
      "candidate.action.decide_only"
    );
    assert_eq!(
      artifact.action_resolver_decision.selected_method,
      "pointer-click"
    );
    assert_eq!(
      artifact.action_resolver_decision.cursor_disturbance,
      "warp-visible"
    );
    assert_eq!(artifact.detail["input_delivery"], json!("not_attempted"));
    assert_eq!(artifact.detail["operation_result"], json!("not_produced"));
    assert_eq!(
      artifact.detail["verification_result"],
      json!("not_produced")
    );
    let rendered = serde_json::to_value(&artifact).expect("artifact should serialize");
    assert!(rendered.get("input_action_result").is_none());
    assert!(rendered.get("operation_result").is_none());
    assert!(rendered.get("verification_result").is_none());
    assert!(
      artifact
        .known_limits
        .iter()
        .any(|limit| limit.contains("does not call auv-driver"))
    );
  }

  #[test]
  fn refused_promotion_does_not_produce_action_decision() {
    let mut promotion = promoted_artifact();
    promotion.decision = CandidatePromotion::Refused {
      reasons: vec![crate::candidate_promotion::PromotionRefusal::PermissionMissing],
    };
    let request =
      CandidateActionDecisionRequest::new("decision_text_area", "text-area-action-decision");

    let error = build_candidate_action_decision_artifact(&promotion, &request)
      .expect_err("refused promotion must not produce action decision");

    assert_eq!(error, CandidateActionDecisionError::PromotionDidNotPromote);
  }

  #[test]
  fn recorded_operation_persists_decide_only_action_decision_artifact() {
    let project_root = temp_dir("candidate-action-decision-record-project");
    let store_root = temp_dir("candidate-action-decision-record-store");
    fs::create_dir_all(&project_root).expect("project root should exist");
    let runtime = build_runtime_with_store_root(project_root.clone(), store_root.clone())
      .expect("runtime should build");
    let promotion = promoted_artifact();

    let output = runtime
      .run_recorded_operation(
        RunSpec::new(RunType::Execute, "auv.candidate.action.decide_only"),
        "Candidate action decide-only artifact recording",
        |context| {
          let source_path = project_root.join("candidate-promotion-source.json");
          fs::write(&source_path, "{\"fixture\":true}\n").expect("promotion source should write");
          let (_, source_promotion_ref) = context
            .stage_artifact_file_with_ref(
              "candidate-promotion",
              &source_path,
              "candidate-promotion-source.json",
              Some("Recorded source promotion artifact.".to_string()),
            )
            .expect("source promotion artifact should stage");

          let mut request =
            CandidateActionDecisionRequest::new("decision_text_area", "text-area-action-decision");
          request.source_candidate_promotion_artifact = Some(source_promotion_ref);
          record_candidate_action_decision_artifact(context, &promotion, &request)
        },
      )
      .expect("recorded decide-only action decision operation should succeed");

    let run = runtime
      .read_run(output.run_id.as_str())
      .expect("recorded run should persist");
    assert_eq!(run.run.status_code, TraceStatusCode::Ok);
    assert_eq!(run.artifacts.len(), 2);
    assert_eq!(run.artifacts[0].role, "candidate-promotion");
    assert_eq!(run.artifacts[1].role, "candidate-action-decision");
    assert!(
      run
        .events
        .iter()
        .any(|event| event.name == "candidate.action.decision.artifact_recorded")
    );
    let (_artifact_ref, artifact) = output.value;
    assert_eq!(
      artifact.action_resolver_decision.operation,
      "candidate.action.decide_only"
    );
    assert_eq!(artifact.detail["input_delivery"], json!("not_attempted"));
    let inspect = runtime
      .inspect(output.run_id.as_str())
      .expect("recorded decide-only run should inspect");
    assert!(inspect.contains("Candidate Action Decision Lineage:"));
    assert!(inspect.contains("resolver=candidate.action.decide_only"));
    assert!(inspect.contains("side_effect=none_decide_only"));
    assert!(inspect.contains("input_delivery=not_attempted"));
    assert!(inspect.contains("operation_result=not_produced"));
    assert!(inspect.contains("verification_result=not_produced"));

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  fn temp_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("auv-{}-{}", label, crate::model::now_millis()))
  }
}
