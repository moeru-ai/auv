// File: src/inspect.rs
//! Human-readable run inspection helpers.
//!
//! This module renders stored run snapshots (`CanonicalRun`) into a simple text
//! form (useful for CLI/debug output). It does not provide a live viewer or any
//! runtime execution logic; see `inspect_server` for the HTTP/WebSocket UI.

use crate::contract::{
  FailureLayer, ObservationSnapshot, ObservationSource, VerificationMethod, VerificationResult,
};
use crate::model::AuvResult;
use crate::run_read::{
  CandidateActionDecisionLineage, CandidateActionDecisionLineageStatus,
  CandidateActionExecutionClosureState, CandidateActionExecutionLineage,
  CandidateActionExecutionLineageStatus, CandidatePromotionLineage,
  CandidatePromotionLineageStatus, DetectorRecognitionLineage,
  MinecraftTelemetrySampleArtifactLineage, list_minecraft_projection_artifacts,
  list_minecraft_telemetry_sample_artifacts,
};
use auv_tracing_driver::store::{CanonicalRun, LocalStore};

pub fn read_run(store: &LocalStore, run_id: &str) -> AuvResult<CanonicalRun> {
  crate::run_read::read_run(store, run_id)
}

pub fn list_verifications(store: &LocalStore, run_id: &str) -> AuvResult<Vec<VerificationResult>> {
  crate::run_read::list_verifications(store, run_id)
}

pub fn list_observation_snapshots(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<ObservationSnapshot>> {
  crate::run_read::list_observation_snapshots(store, run_id)
}

pub fn list_detector_recognition_lineage(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<DetectorRecognitionLineage>> {
  crate::run_read::list_detector_recognition_lineage(store, run_id)
}

pub fn list_candidate_promotion_lineage(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<CandidatePromotionLineage>> {
  crate::run_read::list_candidate_promotion_lineage(store, run_id)
}

pub fn list_candidate_action_decision_lineage(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<CandidateActionDecisionLineage>> {
  crate::run_read::list_candidate_action_decision_lineage(store, run_id)
}

pub fn list_candidate_action_execution_lineage(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<CandidateActionExecutionLineage>> {
  crate::run_read::list_candidate_action_execution_lineage(store, run_id)
}

pub fn inspect_run(store: &LocalStore, run_id: &str) -> AuvResult<String> {
  let canonical = read_run(store, run_id)?;
  let verifications = list_verifications(store, run_id)?;
  let observation_snapshots = list_observation_snapshots(store, run_id)?;
  let detector_recognition_lineage = list_detector_recognition_lineage(store, run_id)?;
  let candidate_promotion_lineage = list_candidate_promotion_lineage(store, run_id)?;
  let candidate_action_decision_lineage = list_candidate_action_decision_lineage(store, run_id)?;
  let candidate_action_execution_lineage = list_candidate_action_execution_lineage(store, run_id)?;
  let minecraft_projection_artifacts = list_minecraft_projection_artifacts(store, run_id)?;
  let minecraft_telemetry_sample_artifacts =
    list_minecraft_telemetry_sample_artifacts(store, run_id)?;
  Ok(render_run_text(
    &canonical,
    &verifications,
    &observation_snapshots,
    &detector_recognition_lineage,
    &candidate_promotion_lineage,
    &candidate_action_decision_lineage,
    &candidate_action_execution_lineage,
    &minecraft_projection_artifacts,
    &minecraft_telemetry_sample_artifacts,
  ))
}

pub fn render_run_text(
  run: &CanonicalRun,
  verifications: &[VerificationResult],
  observation_snapshots: &[ObservationSnapshot],
  detector_recognition_lineage: &[DetectorRecognitionLineage],
  candidate_promotion_lineage: &[CandidatePromotionLineage],
  candidate_action_decision_lineage: &[CandidateActionDecisionLineage],
  candidate_action_execution_lineage: &[CandidateActionExecutionLineage],
  minecraft_projection_artifacts: &[auv_game_minecraft::artifact::MinecraftProjectionArtifact],
  minecraft_telemetry_sample_artifacts: &[MinecraftTelemetrySampleArtifactLineage],
) -> String {
  let mut output = format!(
    "Run {}\nType: {}\nStatus: {}\nState: {}\n",
    run.run.run_id,
    run.run.run_type.as_str(),
    run.run.status_code.as_str(),
    run.run.state.as_str()
  );
  if let Some(summary) = &run.run.summary {
    output.push_str(&format!("Summary: {summary}\n"));
  }
  if let Some(failure) = &run.run.failure {
    output.push_str(&format!("Failure: {}\n", failure.message));
  }

  output.push_str(&format!("\nSpans: {}\n", run.spans.len()));
  for span in run.spans.iter().take(20) {
    output.push_str(&format!(
      "- {} name={} parent={} status={}\n",
      span.span_id,
      span.name,
      span
        .parent_span_id
        .as_ref()
        .map(|span_id| span_id.as_str())
        .unwrap_or("n/a"),
      span.status_code.as_str()
    ));
  }
  if run.spans.len() > 20 {
    output.push_str(&format!("- … {} more\n", run.spans.len() - 20));
  }

  output.push_str(&format!("\nEvents: {}\n", run.events.len()));
  for event in run.events.iter().take(20) {
    let message = event.message.as_deref().unwrap_or("");
    output.push_str(&format!(
      "- {} span={} name={} {}\n",
      event.event_id, event.span_id, event.name, message
    ));
  }
  if run.events.len() > 20 {
    output.push_str(&format!("- … {} more\n", run.events.len() - 20));
  }

  output.push_str(&format!("\nArtifacts: {}\n", run.artifacts.len()));
  for artifact in run.artifacts.iter().take(20) {
    output.push_str(&format!(
      "- {} span={} role={} path={}\n",
      artifact.artifact_id, artifact.span_id, artifact.role, artifact.path
    ));
  }
  if run.artifacts.len() > 20 {
    output.push_str(&format!("- … {} more\n", run.artifacts.len() - 20));
  }

  output.push_str("\nVerifications:\n");
  if verifications.is_empty() {
    output.push_str("- none\n");
  } else {
    for verification in verifications {
      output.push_str(&format!(
        "- method={} executed={} state_changed={} semantic_matched={} failure_layer={} evidence={} observed_label={}\n",
        render_verification_method(&verification.method),
        verification.executed,
        verification.state_changed,
        render_optional_bool(verification.semantic_matched),
        render_failure_layer(verification.failure_layer),
        verification.evidence.len(),
        verification.observed_label.as_deref().unwrap_or("n/a")
      ));
    }
  }

  output.push_str("\nObservations:\n");
  if observation_snapshots.is_empty() {
    output.push_str("- none\n");
  } else {
    for snapshot in observation_snapshots {
      output.push_str(&format!(
        "- {} span={} source={} nodes={} evidence={} limits={}\n",
        snapshot.snapshot_id,
        snapshot.span_id,
        render_observation_source(snapshot.source),
        snapshot.nodes.len(),
        snapshot.evidence.len(),
        snapshot.known_limits.len()
      ));
    }
  }

  output.push_str("\nDetector Recognition Lineage:\n");
  if detector_recognition_lineage.is_empty() {
    output.push_str("- none\n");
  } else {
    for lineage in detector_recognition_lineage {
      output.push_str(&format!(
        "- artifact={} status={} source={} model={} backend={} items={}/{} best={} projection={} capture={} limits={}\n",
        lineage.artifact.artifact_id,
        render_detector_status(&lineage.status),
        lineage
          .source
          .map(render_recognition_source)
          .unwrap_or("n/a"),
        lineage.model_id.as_deref().unwrap_or("n/a"),
        lineage.backend.as_deref().unwrap_or("n/a"),
        lineage.filtered_count.map(|value| value.to_string()).unwrap_or_else(|| "n/a".to_string()),
        lineage.all_count.map(|value| value.to_string()).unwrap_or_else(|| "n/a".to_string()),
        lineage.best_item_id.as_deref().unwrap_or("n/a"),
        lineage.runtime_projection_kind.as_deref().unwrap_or("n/a"),
        lineage
          .capture_artifact
          .as_ref()
          .and_then(|artifact| artifact.path.as_deref())
          .unwrap_or("n/a"),
        lineage.known_limits.len()
      ));
      output.push_str(&format!(
        "  evidence={} class_label_source={} provider={} issue={}\n",
        lineage.evidence_artifacts.len(),
        lineage.class_label_source_kind.as_deref().unwrap_or("n/a"),
        lineage.execution_provider.as_deref().unwrap_or("n/a"),
        lineage.issue.as_deref().unwrap_or("n/a")
      ));
      if !lineage.known_limits.is_empty() {
        output.push_str(&format!(
          "  known_limits={}\n",
          lineage.known_limits.join(" | ")
        ));
      }
    }
  }

  output.push_str("\nCandidate Promotion Lineage:\n");
  if candidate_promotion_lineage.is_empty() {
    output.push_str("- none\n");
  } else {
    for lineage in candidate_promotion_lineage {
      output.push_str(&format!(
        "- artifact={} status={} promotion_id={} decision={} stability={} projection={} source_recognition={} capture={} promoted={} refusals={}\n",
        lineage.artifact.artifact_id,
        render_candidate_promotion_status(&lineage.status),
        lineage.promotion_id.as_deref().unwrap_or("n/a"),
        lineage.decision_kind.as_deref().unwrap_or("n/a"),
        lineage.stability_kind.as_deref().unwrap_or("n/a"),
        lineage.projection_kind.as_deref().unwrap_or("n/a"),
        lineage
          .source_recognition_artifact
          .as_ref()
          .and_then(|artifact| artifact.path.as_deref())
          .unwrap_or("n/a"),
        lineage
          .capture_artifact
          .as_ref()
          .and_then(|artifact| artifact.path.as_deref())
          .unwrap_or("n/a"),
        if lineage.promoted_candidate_local_ids.is_empty() {
          "none".to_string()
        } else {
          lineage.promoted_candidate_local_ids.join(",")
        },
        if lineage.refusal_reasons.is_empty() {
          "none".to_string()
        } else {
          lineage.refusal_reasons.join(" | ")
        }
      ));
      output.push_str(&format!(
        "  recognition={} observed_frames={} freshness_present={} freshness_source={} permission_granted={} consent_id={} consent_provenance={} consent_grade={} consent_scope={} permission_by={} issue={}\n",
        lineage
          .promotion_input_recognition_id
          .as_deref()
          .unwrap_or("n/a"),
        lineage
          .stability_observed_frames
          .map(|value| value.to_string())
          .unwrap_or_else(|| "n/a".to_string()),
        lineage
          .freshness_present
          .map(|value| if value { "true" } else { "false" })
          .unwrap_or("n/a"),
        lineage
          .freshness_source_artifact
          .as_ref()
          .and_then(|artifact| artifact.path.as_deref())
          .unwrap_or("n/a"),
        lineage
          .permission_granted
          .map(|value| if value { "true" } else { "false" })
          .unwrap_or("n/a"),
        lineage.consent_id.as_deref().unwrap_or("n/a"),
        lineage.consent_provenance.as_deref().unwrap_or("n/a"),
        lineage.consent_grade.as_deref().unwrap_or("n/a"),
        lineage.consent_scope.as_deref().unwrap_or("n/a"),
        lineage.permission_granted_by.as_deref().unwrap_or("n/a"),
        lineage.issue.as_deref().unwrap_or("n/a")
      ));
      if let Some(stability_reason) = &lineage.stability_reason {
        output.push_str(&format!("  stability_reason={stability_reason}\n"));
      }
      if !lineage.known_limits.is_empty() {
        output.push_str(&format!(
          "  known_limits={}\n",
          lineage.known_limits.join(" | ")
        ));
      }
    }
  }

  output.push_str("\nCandidate Action Decision Lineage:\n");
  if candidate_action_decision_lineage.is_empty() {
    output.push_str("- none\n");
  } else {
    for lineage in candidate_action_decision_lineage {
      output.push_str(&format!(
        "- artifact={} status={} decision_id={} source_promotion={} candidate={} resolver={} selected={} side_effect={} input_delivery={} operation_result={} verification_result={}\n",
        lineage.artifact.artifact_id,
        render_candidate_action_decision_status(&lineage.status),
        lineage.decision_id.as_deref().unwrap_or("n/a"),
        lineage
          .source_candidate_promotion_artifact
          .as_ref()
          .and_then(|artifact| artifact.path.as_deref())
          .unwrap_or("n/a"),
        lineage.candidate_local_id.as_deref().unwrap_or("n/a"),
        lineage.resolver_operation.as_deref().unwrap_or("n/a"),
        lineage.selected_method.as_deref().unwrap_or("n/a"),
        lineage.side_effect.as_deref().unwrap_or("n/a"),
        lineage.input_delivery.as_deref().unwrap_or("n/a"),
        lineage.operation_result.as_deref().unwrap_or("n/a"),
        lineage.verification_result.as_deref().unwrap_or("n/a"),
      ));
      output.push_str(&format!(
        "  primary={} fallback_allowed={} fallback_used={} fallback_reason={} policy={} cursor={} press={} issue={}\n",
        lineage.primary_method.as_deref().unwrap_or("n/a"),
        lineage
          .fallback_allowed
          .map(|value| if value { "true" } else { "false" })
          .unwrap_or("n/a"),
        lineage
          .fallback_used
          .map(|value| if value { "true" } else { "false" })
          .unwrap_or("n/a"),
        lineage.fallback_reason.as_deref().unwrap_or("none"),
        lineage.policy.as_deref().unwrap_or("n/a"),
        lineage.cursor_disturbance.as_deref().unwrap_or("n/a"),
        lineage.press_mechanism.as_deref().unwrap_or("n/a"),
        lineage.issue.as_deref().unwrap_or("n/a"),
      ));
      if !lineage.known_limits.is_empty() {
        output.push_str(&format!(
          "  known_limits={}\n",
          lineage.known_limits.join(" | ")
        ));
      }
    }
  }

  output.push_str("\nMC-1 Telemetry Samples:\n");
  if minecraft_telemetry_sample_artifacts.is_empty() {
    output.push_str("- none\n");
  } else {
    for lineage in minecraft_telemetry_sample_artifacts {
      output.push_str(&format!(
        "- artifact={} line_count={} bytes={} path={} issue={}\n",
        lineage.artifact.artifact_id,
        lineage
          .line_count
          .map(|value| value.to_string())
          .unwrap_or_else(|| "n/a".to_string()),
        lineage
          .byte_size
          .map(|value| value.to_string())
          .unwrap_or_else(|| "n/a".to_string()),
        lineage.artifact.path.as_deref().unwrap_or("n/a"),
        lineage.issue.as_deref().unwrap_or("n/a"),
      ));
    }
  }

  output.push_str("\nMC-2 Projection Artifacts:\n");
  if minecraft_projection_artifacts.is_empty() {
    output.push_str("- none\n");
  } else {
    for artifact in minecraft_projection_artifacts {
      output.push_str(&format!(
        "- frame={} tick={} timestamp_ms={} screenshot_artifact_ref={} capture_skew_ms={} viewport={}x{}@{},{} visibility={} raycast={} screen_state={} refusal_reason={} verification_reference={} projected_point={}\n",
        artifact.spatial_frame_id,
        artifact.world_tick,
        artifact.monotonic_timestamp_ms,
        artifact
          .screenshot_artifact_ref
          .as_deref()
          .unwrap_or("n/a"),
        artifact
          .mc_capture_skew_ms
          .map(|value| value.to_string())
          .unwrap_or_else(|| "n/a".to_string()),
        artifact.viewport_bounds.width,
        artifact.viewport_bounds.height,
        artifact.viewport_bounds.x,
        artifact.viewport_bounds.y,
        render_projection_visibility(&artifact.visibility),
        artifact.raycast_block_id.as_deref().unwrap_or("n/a"),
        artifact.screen_state.as_deref().unwrap_or("n/a"),
        artifact
          .mismatch_refusal_reason
          .map(|reason| format!("{reason:?}"))
          .unwrap_or_else(|| "n/a".to_string()),
        artifact.verification_reference.as_deref().unwrap_or("n/a"),
        render_minecraft_projected_point(artifact.projected_point.as_ref()),
      ));
    }
  }

  output.push_str("\nCandidate Action Execution Lineage:\n");
  if candidate_action_execution_lineage.is_empty() {
    output.push_str("- none\n");
  } else {
    for lineage in candidate_action_execution_lineage {
      output.push_str(&format!(
        "- artifact={} status={} closure_state={} execution_id={} source_decision={} operation_result_artifact={} candidate={} resolver={} selected={} input_delivery={} selected_path={} operation_status={} verification={} semantic_matched={} readiness={} blocker={} side_effect={} consent={} by={} consent_provenance={} consent_grade={} issue={}\n",
        lineage.artifact.artifact_id,
        render_candidate_action_execution_status(&lineage.status),
        render_candidate_action_execution_closure_state(&lineage.closure_state),
        lineage.execution_id.as_deref().unwrap_or("n/a"),
        lineage
          .source_candidate_action_decision_artifact
          .as_ref()
          .and_then(|artifact| artifact.path.as_deref())
          .unwrap_or("n/a"),
        lineage
          .operation_result_artifact
          .as_ref()
          .and_then(|artifact| artifact.path.as_deref())
          .unwrap_or("n/a"),
        lineage.candidate_local_id.as_deref().unwrap_or("n/a"),
        lineage.resolver_operation.as_deref().unwrap_or("n/a"),
        lineage.selected_method.as_deref().unwrap_or("n/a"),
        lineage.input_delivery.as_deref().unwrap_or("n/a"),
        lineage.selected_path.as_deref().unwrap_or("n/a"),
        lineage.operation_status.as_deref().unwrap_or("n/a"),
        lineage.verification.as_deref().unwrap_or("n/a"),
        render_optional_bool(lineage.semantic_matched),
        lineage.readiness.as_deref().unwrap_or("n/a"),
        lineage.readiness_blocker.as_deref().unwrap_or("n/a"),
        lineage.side_effect.as_deref().unwrap_or("n/a"),
        lineage.consent_id.as_deref().unwrap_or("n/a"),
        lineage.consent_granted_by.as_deref().unwrap_or("n/a"),
        lineage.consent_provenance.as_deref().unwrap_or("n/a"),
        lineage.consent_grade.as_deref().unwrap_or("n/a"),
        lineage.issue.as_deref().unwrap_or("n/a"),
      ));
      output.push_str(&format!(
        "  attempts={} succeeded={}\n",
        lineage
          .attempts
          .map(|value| value.to_string())
          .unwrap_or_else(|| "n/a".to_string()),
        lineage
          .attempts_succeeded
          .map(|value| value.to_string())
          .unwrap_or_else(|| "n/a".to_string()),
      ));
      if !lineage.known_limits.is_empty() {
        output.push_str(&format!(
          "  known_limits={}\n",
          lineage.known_limits.join(" | ")
        ));
      }
    }
  }

  output
}

fn render_detector_status(
  status: &crate::run_read::DetectorRecognitionLineageStatus,
) -> &'static str {
  match status {
    crate::run_read::DetectorRecognitionLineageStatus::Ready => "ready",
    crate::run_read::DetectorRecognitionLineageStatus::MissingCaptureArtifact => {
      "missing_capture_artifact"
    }
    crate::run_read::DetectorRecognitionLineageStatus::MissingEvidence => "missing_evidence",
    crate::run_read::DetectorRecognitionLineageStatus::CaptureArtifactUnresolved => {
      "capture_artifact_unresolved"
    }
    crate::run_read::DetectorRecognitionLineageStatus::Malformed => "malformed",
  }
}

fn render_candidate_promotion_status(status: &CandidatePromotionLineageStatus) -> &'static str {
  match status {
    CandidatePromotionLineageStatus::Ready => "ready",
    CandidatePromotionLineageStatus::MissingSourceRecognitionArtifact => {
      "missing_source_recognition_artifact"
    }
    CandidatePromotionLineageStatus::SourceRecognitionArtifactUnresolved => {
      "source_recognition_artifact_unresolved"
    }
    CandidatePromotionLineageStatus::MissingCaptureArtifact => "missing_capture_artifact",
    CandidatePromotionLineageStatus::CaptureArtifactUnresolved => "capture_artifact_unresolved",
    CandidatePromotionLineageStatus::MissingRecognitionEvidence => "missing_recognition_evidence",
    CandidatePromotionLineageStatus::Malformed => "malformed",
  }
}

fn render_candidate_action_decision_status(
  status: &CandidateActionDecisionLineageStatus,
) -> &'static str {
  match status {
    CandidateActionDecisionLineageStatus::Ready => "ready",
    CandidateActionDecisionLineageStatus::MissingSourceCandidatePromotionArtifact => {
      "missing_source_candidate_promotion_artifact"
    }
    CandidateActionDecisionLineageStatus::SourceCandidatePromotionArtifactUnresolved => {
      "source_candidate_promotion_artifact_unresolved"
    }
    CandidateActionDecisionLineageStatus::Malformed => "malformed",
  }
}

fn render_candidate_action_execution_status(
  status: &CandidateActionExecutionLineageStatus,
) -> &'static str {
  match status {
    CandidateActionExecutionLineageStatus::Ready => "ready",
    CandidateActionExecutionLineageStatus::BlockedNotReady => "blocked_not_ready",
    CandidateActionExecutionLineageStatus::MissingSourceCandidateActionDecisionArtifact => {
      "missing_source_candidate_action_decision_artifact"
    }
    CandidateActionExecutionLineageStatus::SourceCandidateActionDecisionArtifactUnresolved => {
      "source_candidate_action_decision_artifact_unresolved"
    }
    CandidateActionExecutionLineageStatus::MissingOperationResultArtifact => {
      "missing_operation_result_artifact"
    }
    CandidateActionExecutionLineageStatus::OperationResultArtifactUnresolved => {
      "operation_result_artifact_unresolved"
    }
    CandidateActionExecutionLineageStatus::Malformed => "malformed",
  }
}

fn render_candidate_action_execution_closure_state(
  state: &CandidateActionExecutionClosureState,
) -> &'static str {
  match state {
    CandidateActionExecutionClosureState::EvidenceClosed => "evidence_closed",
    CandidateActionExecutionClosureState::SemanticOpen => "semantic_open",
    CandidateActionExecutionClosureState::BlockedByReadiness => "blocked_by_readiness",
  }
}

fn render_optional_bool(value: Option<bool>) -> &'static str {
  match value {
    Some(true) => "true",
    Some(false) => "false",
    None => "n/a",
  }
}

fn render_projection_visibility(
  visibility: &auv_game_minecraft::types::ProjectionVisibility,
) -> &'static str {
  match visibility {
    auv_game_minecraft::types::ProjectionVisibility::Visible => "visible",
    auv_game_minecraft::types::ProjectionVisibility::BehindCamera => "behind_camera",
    auv_game_minecraft::types::ProjectionVisibility::OutOfFrustum => "out_of_frustum",
    auv_game_minecraft::types::ProjectionVisibility::OutsideWindow => "outside_window",
  }
}

fn render_minecraft_projected_point(
  projected_point: Option<&auv_game_minecraft::types::MinecraftProjectedPoint>,
) -> String {
  match projected_point {
    Some(projected_point) => {
      let screen_point = projected_point
        .screen_point
        .as_ref()
        .map(|point| format!("{},{}", point.x, point.y))
        .unwrap_or_else(|| "n/a".to_string());
      format!(
        "screen={} visibility={} radius_px={} confidence={} basis={}",
        screen_point,
        render_projection_visibility(&projected_point.visibility),
        projected_point.match_radius_px,
        projected_point.confidence,
        projected_point.basis_frame_id,
      )
    }
    None => "n/a".to_string(),
  }
}

fn render_failure_layer(layer: Option<FailureLayer>) -> &'static str {
  match layer {
    Some(FailureLayer::GroundingFailed) => "grounding_failed",
    Some(FailureLayer::CandidateExpired) => "candidate_expired",
    Some(FailureLayer::ControlFailed) => "control_failed",
    Some(FailureLayer::VerificationUnreliable) => "verification_unreliable",
    Some(FailureLayer::StateChangedNoMatch) => "state_changed_no_match",
    Some(FailureLayer::SemanticMismatch) => "semantic_mismatch",
    None => "n/a",
  }
}

fn render_verification_method(method: &VerificationMethod) -> String {
  match method {
    VerificationMethod::TextVisible => "text_visible".to_string(),
    VerificationMethod::AxText => "ax_text".to_string(),
    VerificationMethod::StateChanged => "state_changed".to_string(),
    VerificationMethod::CandidateAlive => "candidate_alive".to_string(),
    VerificationMethod::SemanticMatch => "semantic_match".to_string(),
    VerificationMethod::NoProgressBoundary => "no_progress_boundary".to_string(),
    VerificationMethod::Custom { name } => format!("custom:{name}"),
  }
}

fn render_observation_source(source: ObservationSource) -> &'static str {
  match source {
    ObservationSource::Ax => "ax",
    ObservationSource::Ocr => "ocr",
    ObservationSource::Visual => "visual",
    ObservationSource::Merged => "merged",
  }
}

fn render_recognition_source(source: crate::contract::RecognitionSource) -> &'static str {
  match source {
    crate::contract::RecognitionSource::OcrText => "ocr_text",
    crate::contract::RecognitionSource::OcrRow => "ocr_row",
    crate::contract::RecognitionSource::VisualRow => "visual_row",
    crate::contract::RecognitionSource::SegmentedRegion => "segmented_region",
    crate::contract::RecognitionSource::IconMatch => "icon_match",
    crate::contract::RecognitionSource::Custom => "custom",
  }
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;

  use super::render_run_text;
  use crate::contract::{
    OBSERVATION_SNAPSHOT_API_VERSION, ObservationSnapshot, ObservationSource, RecognitionScope,
    RecognitionSource, RecognitionSurface, VERIFICATION_RESULT_API_VERSION, VerificationMethod,
    VerificationResult,
  };
  use crate::run_read::{
    ArtifactRefLineage, CandidateActionDecisionLineage, CandidateActionDecisionLineageStatus,
    CandidateActionExecutionClosureState, CandidateActionExecutionLineage,
    CandidateActionExecutionLineageStatus, CandidatePromotionLineage,
    CandidatePromotionLineageStatus, DetectorRecognitionArtifactRefLineage,
    DetectorRecognitionLineage, DetectorRecognitionLineageStatus,
    MinecraftTelemetrySampleArtifactLineage,
  };
  use auv_tracing_driver::store::CanonicalRun;
  use auv_tracing_driver::trace::{
    ARTIFACT_API_VERSION, ArtifactId, ArtifactRecordV1Alpha1, EVENT_API_VERSION, EventId,
    EventRecordV1Alpha1, RUN_API_VERSION, RunId, RunRecordV1Alpha1, RunType, SPAN_API_VERSION,
    SpanId, SpanRecordV1Alpha1, TraceId, TraceState, TraceStatusCode,
  };

  #[test]
  fn render_run_text_includes_run_span_event_artifact_verification_and_observation_records() {
    let run_id = RunId::new("run_inspect_test");
    let root_span_id = SpanId::new("span_root");
    let event_id = EventId::new("event_test");
    let artifact_id = ArtifactId::new("artifact_test");
    let run = CanonicalRun {
      run: RunRecordV1Alpha1 {
        api_version: RUN_API_VERSION.to_string(),
        run_id: run_id.clone(),
        trace_id: TraceId::new("trace_test"),
        run_type: RunType::Command,
        state: TraceState::Ended,
        status_code: TraceStatusCode::Ok,
        started_at_millis: 1,
        finished_at_millis: Some(2),
        root_span_id: root_span_id.clone(),
        attributes: BTreeMap::new(),
        summary: Some("inspection summary".to_string()),
        failure: None,
      },
      spans: vec![SpanRecordV1Alpha1 {
        api_version: SPAN_API_VERSION.to_string(),
        span_id: root_span_id.clone(),
        parent_span_id: None,
        name: "auv.inspect.span".to_string(),
        state: TraceState::Ended,
        status_code: TraceStatusCode::Ok,
        started_at_millis: 1,
        finished_at_millis: Some(2),
        attributes: BTreeMap::new(),
        summary: None,
        failure: None,
      }],
      events: vec![EventRecordV1Alpha1 {
        api_version: EVENT_API_VERSION.to_string(),
        event_id,
        span_id: root_span_id.clone(),
        name: "inspect.event".to_string(),
        timestamp_millis: 1,
        attributes: BTreeMap::new(),
        message: Some("event message".to_string()),
        artifact_ids: vec![artifact_id.clone()],
      }],
      artifacts: vec![ArtifactRecordV1Alpha1 {
        api_version: ARTIFACT_API_VERSION.to_string(),
        artifact_id: artifact_id.clone(),
        span_id: root_span_id,
        event_id: None,
        role: "driver.output".to_string(),
        mime_type: "text/plain".to_string(),
        path: "artifacts/output.txt".to_string(),
        sha256: None,
        attributes: BTreeMap::new(),
        summary: Some("output".to_string()),
      }],
    };
    let verifications = vec![VerificationResult {
      api_version: VERIFICATION_RESULT_API_VERSION.to_string(),
      method: VerificationMethod::SemanticMatch,
      executed: true,
      state_changed: true,
      semantic_matched: Some(true),
      failure_layer: None,
      evidence: Vec::new(),
      consumed_candidate_ref: None,
      consumed_node_ref: None,
      consumed_recognition_artifact_ref: None,
      consumed_recognition_id: None,
      consumed_recognized_item_id: None,
      observed_label: Some("Now Playing".to_string()),
    }];
    let observation_snapshots = vec![ObservationSnapshot {
      api_version: OBSERVATION_SNAPSHOT_API_VERSION.to_string(),
      snapshot_id: "snapshot_1".to_string(),
      run_id: run_id.clone(),
      span_id: SpanId::new("span_root"),
      captured_at_millis: 1,
      source: ObservationSource::Visual,
      scope: RecognitionScope {
        surface: RecognitionSurface::Window,
        display_ref: None,
        native_display_id: None,
        app_bundle_id: Some("com.example.music".to_string()),
        window_title: Some("Example Music".to_string()),
        window_number: None,
        region_hint: None,
        capture_artifact: None,
        capture_contract_artifact: None,
      },
      capture_contract_ref: None,
      evidence: Vec::new(),
      nodes: Vec::new(),
      detail: serde_json::json!({"producer": "scroll_scan"}),
      known_limits: vec!["visual only".to_string()],
    }];
    let detector_recognition_lineage = vec![DetectorRecognitionLineage {
      artifact: DetectorRecognitionArtifactRefLineage {
        run_id: run_id.clone(),
        artifact_id: ArtifactId::new("artifact_detector_recognition"),
        span_id: SpanId::new("span_root"),
        captured_event_id: Some(EventId::new("event_detector_recognition")),
        role: Some("detector-recognition".to_string()),
        path: Some("artifacts/detector-recognition.json".to_string()),
        summary: Some("detector recognition".to_string()),
        resolved: true,
      },
      status: DetectorRecognitionLineageStatus::Ready,
      recognition_id: Some("recognition_detector_1".to_string()),
      source: Some(RecognitionSource::Custom),
      backend: Some("ultralytics-inference".to_string()),
      model_id: Some("games-balatro-ui".to_string()),
      execution_provider: Some("cpu".to_string()),
      class_label_source_kind: Some("override_file".to_string()),
      runtime_projection_kind: Some("identity_source_image_pixels".to_string()),
      capture_artifact: Some(DetectorRecognitionArtifactRefLineage {
        run_id: run_id.clone(),
        artifact_id: ArtifactId::new("artifact_capture"),
        span_id: SpanId::new("span_root"),
        captured_event_id: Some(EventId::new("event_capture")),
        role: Some("capture-image".to_string()),
        path: Some("artifacts/capture.png".to_string()),
        summary: Some("capture".to_string()),
        resolved: true,
      }),
      capture_contract_artifact: Some(DetectorRecognitionArtifactRefLineage {
        run_id: run_id.clone(),
        artifact_id: ArtifactId::new("artifact_contract"),
        span_id: SpanId::new("span_root"),
        captured_event_id: Some(EventId::new("event_contract")),
        role: Some("capture-contract".to_string()),
        path: Some("artifacts/capture-contract.json".to_string()),
        summary: Some("contract".to_string()),
        resolved: true,
      }),
      evidence_artifacts: vec![
        DetectorRecognitionArtifactRefLineage {
          run_id: run_id.clone(),
          artifact_id: ArtifactId::new("artifact_capture"),
          span_id: SpanId::new("span_root"),
          captured_event_id: Some(EventId::new("event_capture")),
          role: Some("capture-image".to_string()),
          path: Some("artifacts/capture.png".to_string()),
          summary: Some("capture".to_string()),
          resolved: true,
        },
        DetectorRecognitionArtifactRefLineage {
          run_id: run_id.clone(),
          artifact_id: ArtifactId::new("artifact_contract"),
          span_id: SpanId::new("span_root"),
          captured_event_id: Some(EventId::new("event_contract")),
          role: Some("capture-contract".to_string()),
          path: Some("artifacts/capture-contract.json".to_string()),
          summary: Some("contract".to_string()),
          resolved: true,
        },
      ],
      all_count: Some(2),
      filtered_count: Some(1),
      best_item_id: None,
      known_limits: vec![
        "projection basis is unavailable outside capture-integrated runtime".to_string(),
        "detector RecognitionResult is recognition evidence only, not candidate-ready output"
          .to_string(),
      ],
      issue: None,
    }];
    let candidate_promotion_lineage = vec![CandidatePromotionLineage {
      artifact: ArtifactRefLineage {
        run_id: run_id.clone(),
        artifact_id: ArtifactId::new("artifact_candidate_promotion"),
        span_id: SpanId::new("span_root"),
        captured_event_id: Some(EventId::new("event_candidate_promotion")),
        role: Some("candidate-promotion".to_string()),
        path: Some("artifacts/candidate-promotion.json".to_string()),
        summary: Some("candidate promotion".to_string()),
        resolved: true,
      },
      status: CandidatePromotionLineageStatus::Ready,
      promotion_id: Some("promotion_end_turn".to_string()),
      source_recognition_artifact: Some(ArtifactRefLineage {
        run_id: run_id.clone(),
        artifact_id: ArtifactId::new("artifact_detector_recognition"),
        span_id: SpanId::new("span_root"),
        captured_event_id: Some(EventId::new("event_detector_recognition")),
        role: Some("detector-recognition".to_string()),
        path: Some("artifacts/detector-recognition.json".to_string()),
        summary: Some("detector recognition".to_string()),
        resolved: true,
      }),
      capture_artifact: Some(ArtifactRefLineage {
        run_id: run_id.clone(),
        artifact_id: ArtifactId::new("artifact_capture"),
        span_id: SpanId::new("span_root"),
        captured_event_id: Some(EventId::new("event_capture")),
        role: Some("capture-image".to_string()),
        path: Some("artifacts/capture.png".to_string()),
        summary: Some("capture".to_string()),
        resolved: true,
      }),
      promotion_input_recognition_id: Some("recognition_detector_1".to_string()),
      observed_recognition_ids: vec![
        "recognition_detector_0".to_string(),
        "recognition_detector_1".to_string(),
      ],
      recognition_source: Some(RecognitionSource::Custom),
      projection_kind: Some("identity_window_addressable".to_string()),
      stability_kind: Some("stable".to_string()),
      stability_observed_frames: Some(2),
      stability_reason: None,
      freshness_present: Some(true),
      freshness_source_artifact: Some(ArtifactRefLineage {
        run_id: run_id.clone(),
        artifact_id: ArtifactId::new("artifact_capture"),
        span_id: SpanId::new("span_root"),
        captured_event_id: Some(EventId::new("event_capture")),
        role: Some("capture-image".to_string()),
        path: Some("artifacts/capture.png".to_string()),
        summary: Some("capture".to_string()),
        resolved: true,
      }),
      freshness_source_operation_id: Some("observe.window.capture".to_string()),
      permission_granted: Some(true),
      permission_granted_by: Some("human-review".to_string()),
      permission_scope_note: Some("fixture promotion".to_string()),
      consent_id: Some("consent_promotion_end_turn".to_string()),
      consent_provenance: Some("human_gesture".to_string()),
      consent_grade: Some("human_approved".to_string()),
      consent_scope: Some("candidate_promotion_only".to_string()),
      consent_approved_action: Some("promote_recognition_to_candidate".to_string()),
      consent_recognition_id: Some("recognition_detector_1".to_string()),
      decision_kind: Some("promoted".to_string()),
      refusal_reasons: Vec::new(),
      promoted_candidate_local_ids: vec!["promoted-item_end_turn".to_string()],
      known_limits: vec![
        "candidate promotion artifact records gate decisions only; runtime action consumption remains deferred".to_string(),
      ],
      issue: None,
    }];
    let candidate_action_decision_lineage = vec![CandidateActionDecisionLineage {
      artifact: ArtifactRefLineage {
        run_id: run_id.clone(),
        artifact_id: ArtifactId::new("artifact_candidate_action_decision"),
        span_id: SpanId::new("span_root"),
        captured_event_id: Some(EventId::new("event_candidate_action_decision")),
        role: Some("candidate-action-decision".to_string()),
        path: Some("artifacts/candidate-action-decision.json".to_string()),
        summary: Some("candidate action decision".to_string()),
        resolved: true,
      },
      status: CandidateActionDecisionLineageStatus::Ready,
      decision_id: Some("decision_end_turn".to_string()),
      source_candidate_promotion_artifact: Some(ArtifactRefLineage {
        run_id: run_id.clone(),
        artifact_id: ArtifactId::new("artifact_candidate_promotion"),
        span_id: SpanId::new("span_root"),
        captured_event_id: Some(EventId::new("event_candidate_promotion")),
        role: Some("candidate-promotion".to_string()),
        path: Some("artifacts/candidate-promotion.json".to_string()),
        summary: Some("candidate promotion".to_string()),
        resolved: true,
      }),
      source_promotion_id: Some("promotion_end_turn".to_string()),
      candidate_local_id: Some("promoted-item_end_turn".to_string()),
      resolver_operation: Some("candidate.action.decide_only".to_string()),
      selected_method: Some("pointer-click".to_string()),
      primary_method: Some("pointer-click".to_string()),
      fallback_allowed: Some(false),
      fallback_used: Some(false),
      fallback_reason: None,
      policy: Some("candidate-coordinate-pointer".to_string()),
      cursor_disturbance: Some("warp-visible".to_string()),
      press_mechanism: Some("pointer-click".to_string()),
      side_effect: Some("none_decide_only".to_string()),
      input_delivery: Some("not_attempted".to_string()),
      operation_result: Some("not_produced".to_string()),
      verification_result: Some("not_produced".to_string()),
      known_limits: vec![
        "L8a records an ActionResolverDecision only; it does not call auv-driver or produce InputActionResult".to_string(),
      ],
      issue: None,
    }];
    let candidate_action_execution_lineage = vec![CandidateActionExecutionLineage {
      artifact: ArtifactRefLineage {
        run_id: run_id.clone(),
        artifact_id: ArtifactId::new("artifact_candidate_action_execution"),
        span_id: SpanId::new("span_root"),
        captured_event_id: Some(EventId::new("event_candidate_action_execution")),
        role: Some("candidate-action-execution".to_string()),
        path: Some("artifacts/candidate-action-execution.json".to_string()),
        summary: Some("candidate action execution".to_string()),
        resolved: true,
      },
      status: CandidateActionExecutionLineageStatus::Ready,
      execution_id: Some("execution_end_turn".to_string()),
      source_candidate_action_decision_artifact: Some(ArtifactRefLineage {
        run_id: run_id.clone(),
        artifact_id: ArtifactId::new("artifact_candidate_action_decision"),
        span_id: SpanId::new("span_root"),
        captured_event_id: Some(EventId::new("event_candidate_action_decision")),
        role: Some("candidate-action-decision".to_string()),
        path: Some("artifacts/candidate-action-decision.json".to_string()),
        summary: Some("candidate action decision".to_string()),
        resolved: true,
      }),
      source_candidate_promotion_artifact: None,
      operation_result_artifact: Some(ArtifactRefLineage {
        run_id: run_id.clone(),
        artifact_id: ArtifactId::new("artifact_operation_result"),
        span_id: SpanId::new("span_root"),
        captured_event_id: Some(EventId::new("event_operation_result")),
        role: Some("operation-result".to_string()),
        path: Some("artifacts/operation-result.json".to_string()),
        summary: Some("operation result".to_string()),
        resolved: true,
      }),
      source_promotion_id: Some("promotion_end_turn".to_string()),
      source_decision_id: Some("decision_end_turn".to_string()),
      candidate_local_id: Some("promoted-item_end_turn".to_string()),
      resolver_operation: Some("candidate.action.decide_only".to_string()),
      selected_method: Some("pointer-click".to_string()),
      input_delivery: Some("attempted".to_string()),
      selected_path: Some("window_targeted_mouse".to_string()),
      attempts: Some(1),
      attempts_succeeded: Some(1),
      operation_status: Some("completed".to_string()),
      verification: Some("activation_only".to_string()),
      closure_state: CandidateActionExecutionClosureState::SemanticOpen,
      semantic_matched: None,
      readiness: Some("ready".to_string()),
      readiness_blocker: None,
      consent_id: Some("consent_execute_end_turn".to_string()),
      consent_granted_by: Some("human-review".to_string()),
      consent_provenance: Some("human_gesture".to_string()),
      consent_grade: Some("human_approved".to_string()),
      side_effect: Some("single_input_delivered".to_string()),
      known_limits: vec![
        "activation_only verification records input delivery, not semantic success".to_string(),
      ],
      issue: None,
    }];

    let minecraft_projection_artifacts =
      vec![auv_game_minecraft::artifact::MinecraftProjectionArtifact {
        spatial_frame_id: "frame-1".to_string(),
        world_tick: 42,
        monotonic_timestamp_ms: 1_000,
        viewport_bounds: auv_game_minecraft::artifact::ProjectionViewportBounds {
          x: 0.0,
          y: 0.0,
          width: 800.0,
          height: 600.0,
        },
        projected_point: Some(auv_game_minecraft::types::MinecraftProjectedPoint {
          screen_point: Some(auv_driver::geometry::Point::new(320.0, 240.0)),
          visibility: auv_game_minecraft::types::ProjectionVisibility::Visible,
          match_radius_px: 12.0,
          basis_frame_id: "frame-1".to_string(),
          confidence: 1.0,
        }),
        screenshot_artifact_ref: Some("artifact://screenshot-1".to_string()),
        mc_capture_skew_ms: Some(180),
        visibility: auv_game_minecraft::types::ProjectionVisibility::Visible,
        raycast_block_id: Some("minecraft:stone".to_string()),
        screen_state: Some("menu".to_string()),
        mismatch_refusal_reason: Some(
          auv_game_minecraft::verify::MismatchRefusalReason::MenuLoadingScreen,
        ),
        verification_reference: Some("verification-1".to_string()),
      }];
    let minecraft_telemetry_sample_artifacts = vec![MinecraftTelemetrySampleArtifactLineage {
      artifact: crate::run_read::ArtifactRefLineage {
        run_id: run.run.run_id.clone(),
        artifact_id: auv_tracing_driver::trace::ArtifactId::new("artifact_mc1".to_string()),
        span_id: auv_tracing_driver::trace::SpanId::new("span_mc1".to_string()),
        captured_event_id: None,
        role: Some("telemetry-sample".to_string()),
        path: Some("artifacts/telemetry.jsonl".to_string()),
        summary: Some("durable minecraft telemetry sample".to_string()),
        resolved: true,
      },
      line_count: Some(1),
      byte_size: Some(16),
      issue: None,
    }];

    let output = render_run_text(
      &run,
      &verifications,
      &observation_snapshots,
      &detector_recognition_lineage,
      &candidate_promotion_lineage,
      &candidate_action_decision_lineage,
      &candidate_action_execution_lineage,
      &minecraft_projection_artifacts,
      &minecraft_telemetry_sample_artifacts,
    );

    assert!(output.contains("Run run_inspect_test"));
    assert!(output.contains("Type: command"));
    assert!(output.contains("Status: ok"));
    assert!(output.contains("auv.inspect.span"));
    assert!(output.contains("inspect.event"));
    assert!(output.contains("artifact_test"));
    assert!(output.contains("Verifications:"));
    assert!(output.contains("method=semantic_match"));
    assert!(output.contains("Observations:"));
    assert!(output.contains("snapshot_1"));
    assert!(output.contains("Detector Recognition Lineage:"));
    assert!(output.contains("artifact=artifact_detector_recognition"));
    assert!(output.contains("status=ready"));
    assert!(output.contains("model=games-balatro-ui"));
    assert!(output.contains("backend=ultralytics-inference"));
    assert!(output.contains("capture=artifacts/capture.png"));
    assert!(output.contains("known_limits=projection basis is unavailable outside capture-integrated runtime | detector RecognitionResult is recognition evidence only, not candidate-ready output"));
    assert!(output.contains("Candidate Promotion Lineage:"));
    assert!(output.contains("artifact=artifact_candidate_promotion"));
    assert!(output.contains("promotion_id=promotion_end_turn"));
    assert!(output.contains("decision=promoted"));
    assert!(output.contains("projection=identity_window_addressable"));
    assert!(output.contains("source_recognition=artifacts/detector-recognition.json"));
    assert!(output.contains("freshness_source=artifacts/capture.png"));
    assert!(output.contains("consent_scope=candidate_promotion_only"));
    assert!(output.contains("consent_provenance=human_gesture"));
    assert!(output.contains("consent_grade=human_approved"));
    assert!(output.contains("permission_by=human-review"));
    assert!(output.contains("Candidate Action Decision Lineage:"));
    assert!(output.contains("artifact=artifact_candidate_action_decision"));
    assert!(output.contains("decision_id=decision_end_turn"));
    assert!(output.contains("resolver=candidate.action.decide_only"));
    assert!(output.contains("selected=pointer-click"));
    assert!(output.contains("side_effect=none_decide_only"));
    assert!(output.contains("input_delivery=not_attempted"));
    assert!(output.contains("operation_result=not_produced"));
    assert!(output.contains("verification_result=not_produced"));
    assert!(output.contains("cursor=warp-visible"));
    assert!(output.contains("MC-2 Projection Artifacts:"));
    assert!(output.contains("frame=frame-1"));
    assert!(output.contains("screenshot_artifact_ref=artifact://screenshot-1"));
    assert!(output.contains("capture_skew_ms=180"));
    assert!(output.contains("verification_reference=verification-1"));
    assert!(output.contains(
      "projected_point=screen=320,240 visibility=visible radius_px=12 confidence=1 basis=frame-1"
    ));
    assert!(output.contains("Candidate Action Execution Lineage:"));
    assert!(output.contains("artifact=artifact_candidate_action_execution"));
    assert!(output.contains("execution_id=execution_end_turn"));
    assert!(output.contains("input_delivery=attempted"));
    assert!(output.contains("selected_path=window_targeted_mouse"));
    assert!(output.contains("operation_status=completed"));
    assert!(output.contains("verification=activation_only"));
    assert!(output.contains("closure_state=semantic_open"));
    assert!(output.contains("semantic_matched=n/a"));
    assert!(output.contains("side_effect=single_input_delivered"));
    assert!(output.contains("consent=consent_execute_end_turn"));
    assert!(output.contains("consent_provenance=human_gesture"));
    assert!(output.contains("consent_grade=human_approved"));
  }
}
