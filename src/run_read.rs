//! Read-side helpers for stored operation results and observation snapshots.
//!
//! These helpers intentionally sit below `runtime` and `inspect_server` so
//! both call sites reuse one artifact scan / compatibility policy:
//!
//! - verification claims come from `operation-result` JSON artifacts
//! - observation snapshots come from `scroll-scan` JSON artifacts
//! - legacy `OperationOutput::Verification` remains readable without
//!   double-counting artifacts that also populate `OperationResult.verifications`

use std::fs;
use std::path::PathBuf;

use serde::de::DeserializeOwned;

use crate::app::AppValidation;
use crate::candidate_promotion::{
  ActionConsentScope, CandidatePromotion, PromotionProjection, PromotionRefusal,
};
use crate::candidate_promotion_recording::CandidatePromotionArtifact;
use crate::contract::{
  ArtifactRef, ObservationSnapshot, OperationOutput, OperationResult, RecognitionResult,
  RecognitionSource, VerificationResult,
};
use crate::model::AuvResult;
use crate::scroll_scan::ScrollScanArtifact;
use crate::stability::{StabilityAssessment, StabilityRejection};
use crate::store::{CanonicalRun, LocalStore};
use crate::trace::ArtifactRecordV1Alpha1;

const NATIVE_TEXT_CANONICAL_TAXONOMY_ID: &str =
  "native-text.ax-text.ax-perform-action-clipboard-paste.verify-ax-text";
const NATIVE_TEXT_LEGACY_TAXONOMY_ID: &str =
  "native-text.ax-text.pointer-focus-clipboard-paste.verify-ax-text";
const DETECTOR_RECOGNITION_ARTIFACT_ROLE: &str = "detector-recognition";
const CANDIDATE_PROMOTION_ARTIFACT_ROLE: &str = "candidate-promotion";

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct AppValidationLineage {
  pub recipe_id: String,
  pub taxonomy_id: String,
  pub canonical_taxonomy_id: String,
  pub legacy_taxonomy_alias: bool,
  pub observed_consumer: Option<String>,
  pub observed_candidate_local_id: Option<String>,
  pub candidate_source: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectorRecognitionLineageStatus {
  Ready,
  MissingCaptureArtifact,
  MissingEvidence,
  CaptureArtifactUnresolved,
  Malformed,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DetectorRecognitionArtifactRefLineage {
  pub run_id: crate::trace::RunId,
  pub artifact_id: crate::trace::ArtifactId,
  pub span_id: crate::trace::SpanId,
  pub captured_event_id: Option<crate::trace::EventId>,
  pub role: Option<String>,
  pub path: Option<String>,
  pub summary: Option<String>,
  pub resolved: bool,
}

pub type ArtifactRefLineage = DetectorRecognitionArtifactRefLineage;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DetectorRecognitionLineage {
  pub artifact: DetectorRecognitionArtifactRefLineage,
  pub status: DetectorRecognitionLineageStatus,
  pub recognition_id: Option<String>,
  pub source: Option<RecognitionSource>,
  pub backend: Option<String>,
  pub model_id: Option<String>,
  pub execution_provider: Option<String>,
  pub class_label_source_kind: Option<String>,
  pub runtime_projection_kind: Option<String>,
  pub capture_artifact: Option<DetectorRecognitionArtifactRefLineage>,
  pub capture_contract_artifact: Option<DetectorRecognitionArtifactRefLineage>,
  pub evidence_artifacts: Vec<DetectorRecognitionArtifactRefLineage>,
  pub all_count: Option<usize>,
  pub filtered_count: Option<usize>,
  pub best_item_id: Option<String>,
  pub known_limits: Vec<String>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidatePromotionLineageStatus {
  Ready,
  MissingSourceRecognitionArtifact,
  SourceRecognitionArtifactUnresolved,
  MissingCaptureArtifact,
  CaptureArtifactUnresolved,
  MissingRecognitionEvidence,
  Malformed,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct CandidatePromotionLineage {
  pub artifact: ArtifactRefLineage,
  pub status: CandidatePromotionLineageStatus,
  pub promotion_id: Option<String>,
  pub source_recognition_artifact: Option<ArtifactRefLineage>,
  pub capture_artifact: Option<ArtifactRefLineage>,
  pub promotion_input_recognition_id: Option<String>,
  pub observed_recognition_ids: Vec<String>,
  pub recognition_source: Option<RecognitionSource>,
  pub projection_kind: Option<String>,
  pub stability_kind: Option<String>,
  pub stability_observed_frames: Option<u32>,
  pub stability_reason: Option<String>,
  pub freshness_present: Option<bool>,
  pub freshness_source_artifact: Option<ArtifactRefLineage>,
  pub freshness_source_operation_id: Option<String>,
  pub permission_granted: Option<bool>,
  pub consent_id: Option<String>,
  pub consent_scope: Option<ActionConsentScope>,
  pub consent_granted_by: Option<String>,
  pub decision_kind: Option<String>,
  pub refusal_reasons: Vec<String>,
  pub promoted_candidate_local_ids: Vec<String>,
  pub known_limits: Vec<String>,
  pub issue: Option<String>,
}

pub(crate) fn list_verifications(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<VerificationResult>> {
  let run = store.read_run(run_id)?;
  extract_verifications(store, &run)
}

pub(crate) fn extract_verifications(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<VerificationResult>> {
  let mut verifications = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != "operation-result" || !is_json_mime(&artifact.mime_type) {
      continue;
    }
    let operation_result: OperationResult =
      read_artifact_json(store, run.run.run_id.as_str(), artifact, "operation-result")?;
    if !operation_result.verifications.is_empty() {
      verifications.extend(operation_result.verifications);
      continue;
    }
    if let OperationOutput::Verification { verification } = operation_result.output {
      verifications.push(*verification);
    }
  }
  Ok(verifications)
}

pub(crate) fn list_observation_snapshots(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<ObservationSnapshot>> {
  let run = store.read_run(run_id)?;
  extract_observation_snapshots(store, &run)
}

pub(crate) fn extract_observation_snapshots(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<ObservationSnapshot>> {
  let mut snapshots = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != "scroll-scan" || !is_json_mime(&artifact.mime_type) {
      continue;
    }
    let scroll_scan_artifact: ScrollScanArtifact =
      read_artifact_json(store, run.run.run_id.as_str(), artifact, "scroll-scan")?;
    snapshots.extend(scroll_scan_artifact.snapshots);
  }
  Ok(snapshots)
}

pub(crate) fn list_app_validation_lineage(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<AppValidationLineage>> {
  let run = store.read_run(run_id)?;
  extract_app_validation_lineage(store, &run)
}

pub(crate) fn list_detector_recognition_lineage(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<DetectorRecognitionLineage>> {
  let run = store.read_run(run_id)?;
  extract_detector_recognition_lineage(store, &run)
}

pub(crate) fn list_candidate_promotion_lineage(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<CandidatePromotionLineage>> {
  let run = store.read_run(run_id)?;
  extract_candidate_promotion_lineage(store, &run)
}

pub(crate) fn extract_app_validation_lineage(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<AppValidationLineage>> {
  let mut lineage = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != "validation.output" || !is_json_mime(&artifact.mime_type) {
      continue;
    }
    let validation: AppValidation = read_artifact_json(
      store,
      run.run.run_id.as_str(),
      artifact,
      "validation.output",
    )?;
    lineage.extend(validation.candidates.into_iter().map(|candidate| {
      let canonical_taxonomy_id = canonicalize_taxonomy_id(&candidate.taxonomy_id).to_string();
      let legacy_taxonomy_alias = candidate.taxonomy_id.trim() != canonical_taxonomy_id;
      AppValidationLineage {
        recipe_id: candidate.recipe_id,
        taxonomy_id: candidate.taxonomy_id,
        canonical_taxonomy_id,
        legacy_taxonomy_alias,
        observed_consumer: candidate.observed_consumer,
        observed_candidate_local_id: candidate.observed_candidate_local_id,
        candidate_source: candidate.candidate_source,
      }
    }));
  }
  Ok(lineage)
}

pub(crate) fn extract_detector_recognition_lineage(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<DetectorRecognitionLineage>> {
  let mut lineage = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != DETECTOR_RECOGNITION_ARTIFACT_ROLE {
      continue;
    }

    let detector_artifact = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      lineage.push(DetectorRecognitionLineage {
        artifact: detector_artifact,
        status: DetectorRecognitionLineageStatus::Malformed,
        recognition_id: None,
        source: None,
        backend: None,
        model_id: None,
        execution_provider: None,
        class_label_source_kind: None,
        runtime_projection_kind: None,
        capture_artifact: None,
        capture_contract_artifact: None,
        evidence_artifacts: Vec::new(),
        all_count: None,
        filtered_count: None,
        best_item_id: None,
        known_limits: Vec::new(),
        issue: Some(format!(
          "detector-recognition artifact mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }

    let parsed = read_artifact_bytes(
      store,
      run.run.run_id.as_str(),
      artifact,
      DETECTOR_RECOGNITION_ARTIFACT_ROLE,
    )
    .and_then(|(bytes, artifact_path)| {
      serde_json::from_slice::<RecognitionResult>(&bytes).map_err(|error| {
        format!(
          "failed to parse detector-recognition artifact {} for run {} from {}: {error}",
          artifact.artifact_id,
          run.run.run_id,
          artifact_path.display()
        )
      })
    });

    match parsed {
      Ok(recognition) => lineage.push(detector_recognition_lineage_entry(
        run,
        artifact,
        recognition,
      )),
      Err(error) => lineage.push(DetectorRecognitionLineage {
        artifact: detector_artifact,
        status: DetectorRecognitionLineageStatus::Malformed,
        recognition_id: None,
        source: None,
        backend: None,
        model_id: None,
        execution_provider: None,
        class_label_source_kind: None,
        runtime_projection_kind: None,
        capture_artifact: None,
        capture_contract_artifact: None,
        evidence_artifacts: Vec::new(),
        all_count: None,
        filtered_count: None,
        best_item_id: None,
        known_limits: Vec::new(),
        issue: Some(error),
      }),
    }
  }
  Ok(lineage)
}

pub(crate) fn extract_candidate_promotion_lineage(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<CandidatePromotionLineage>> {
  let mut lineage = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != CANDIDATE_PROMOTION_ARTIFACT_ROLE {
      continue;
    }

    let promotion_artifact = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      lineage.push(CandidatePromotionLineage {
        artifact: promotion_artifact,
        status: CandidatePromotionLineageStatus::Malformed,
        promotion_id: None,
        source_recognition_artifact: None,
        capture_artifact: None,
        promotion_input_recognition_id: None,
        observed_recognition_ids: Vec::new(),
        recognition_source: None,
        projection_kind: None,
        stability_kind: None,
        stability_observed_frames: None,
        stability_reason: None,
        freshness_present: None,
        freshness_source_artifact: None,
        freshness_source_operation_id: None,
        permission_granted: None,
        consent_id: None,
        consent_scope: None,
        consent_granted_by: None,
        decision_kind: None,
        refusal_reasons: Vec::new(),
        promoted_candidate_local_ids: Vec::new(),
        known_limits: Vec::new(),
        issue: Some(format!(
          "candidate-promotion artifact mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }

    let parsed = read_artifact_bytes(
      store,
      run.run.run_id.as_str(),
      artifact,
      CANDIDATE_PROMOTION_ARTIFACT_ROLE,
    )
    .and_then(|(bytes, artifact_path)| {
      serde_json::from_slice::<CandidatePromotionArtifact>(&bytes).map_err(|error| {
        format!(
          "failed to parse candidate-promotion artifact {} for run {} from {}: {error}",
          artifact.artifact_id,
          run.run.run_id,
          artifact_path.display()
        )
      })
    });

    match parsed {
      Ok(promotion) => lineage.push(candidate_promotion_lineage_entry(run, artifact, promotion)),
      Err(error) => lineage.push(CandidatePromotionLineage {
        artifact: promotion_artifact,
        status: CandidatePromotionLineageStatus::Malformed,
        promotion_id: None,
        source_recognition_artifact: None,
        capture_artifact: None,
        promotion_input_recognition_id: None,
        observed_recognition_ids: Vec::new(),
        recognition_source: None,
        projection_kind: None,
        stability_kind: None,
        stability_observed_frames: None,
        stability_reason: None,
        freshness_present: None,
        freshness_source_artifact: None,
        freshness_source_operation_id: None,
        permission_granted: None,
        consent_id: None,
        consent_scope: None,
        consent_granted_by: None,
        decision_kind: None,
        refusal_reasons: Vec::new(),
        promoted_candidate_local_ids: Vec::new(),
        known_limits: Vec::new(),
        issue: Some(error),
      }),
    }
  }
  Ok(lineage)
}

fn is_json_mime(mime_type: &str) -> bool {
  mime_type == "application/json" || mime_type.ends_with("+json")
}

fn canonicalize_taxonomy_id(raw: &str) -> &str {
  match raw.trim() {
    NATIVE_TEXT_LEGACY_TAXONOMY_ID | NATIVE_TEXT_CANONICAL_TAXONOMY_ID => {
      NATIVE_TEXT_CANONICAL_TAXONOMY_ID
    }
    other => other,
  }
}

fn detector_recognition_lineage_entry(
  run: &CanonicalRun,
  artifact: &ArtifactRecordV1Alpha1,
  recognition: RecognitionResult,
) -> DetectorRecognitionLineage {
  let capture_artifact = recognition
    .scope
    .capture_artifact
    .as_ref()
    .map(|reference| resolve_artifact_ref(run, reference));
  let capture_contract_artifact = recognition
    .scope
    .capture_contract_artifact
    .as_ref()
    .map(|reference| resolve_artifact_ref(run, reference));
  let evidence_artifacts = recognition
    .evidence
    .iter()
    .map(|reference| resolve_artifact_ref(run, reference))
    .collect::<Vec<_>>();
  let (status, issue) =
    classify_detector_recognition_lineage(&recognition, capture_artifact.as_ref());

  DetectorRecognitionLineage {
    artifact: artifact_record_lineage(run.run.run_id.clone(), artifact),
    status,
    recognition_id: Some(recognition.recognition_id.clone()),
    source: Some(recognition.source),
    backend: detail_string(&recognition.detail, &["backend"]),
    model_id: detail_string(&recognition.detail, &["model_id"]),
    execution_provider: detail_string(&recognition.detail, &["execution_provider"]),
    class_label_source_kind: detail_string(&recognition.detail, &["class_label_source", "kind"]),
    runtime_projection_kind: detail_string(&recognition.detail, &["runtime_projection", "kind"]),
    capture_artifact,
    capture_contract_artifact,
    evidence_artifacts,
    all_count: Some(recognition.all.len()),
    filtered_count: Some(recognition.filtered.len()),
    best_item_id: recognition.best.as_ref().map(|item| item.item_id.clone()),
    known_limits: recognition.known_limits,
    issue,
  }
}

fn candidate_promotion_lineage_entry(
  run: &CanonicalRun,
  artifact: &ArtifactRecordV1Alpha1,
  promotion: CandidatePromotionArtifact,
) -> CandidatePromotionLineage {
  let source_recognition_artifact = promotion
    .source_recognition_artifact
    .as_ref()
    .map(|reference| resolve_artifact_ref(run, reference));
  let capture_artifact = promotion
    .recognition
    .scope
    .capture_artifact
    .as_ref()
    .map(|reference| resolve_artifact_ref(run, reference));
  let (status, issue) = classify_candidate_promotion_lineage(
    &promotion,
    source_recognition_artifact.as_ref(),
    capture_artifact.as_ref(),
  );
  let (decision_kind, refusal_reasons, promoted_candidate_local_ids) =
    promotion_decision_summary(&promotion.decision);
  let (
    freshness_source_artifact,
    freshness_source_operation_id,
    consent_id,
    consent_scope,
    consent_granted_by,
  ) = promotion_audit_summary(run, &promotion.decision);
  let (stability_kind, stability_observed_frames, stability_reason) =
    stability_summary(&promotion.stability_assessment);

  CandidatePromotionLineage {
    artifact: artifact_record_lineage(run.run.run_id.clone(), artifact),
    status,
    promotion_id: Some(promotion.promotion_id),
    source_recognition_artifact,
    capture_artifact,
    promotion_input_recognition_id: Some(promotion.promotion_input_recognition_id),
    observed_recognition_ids: promotion.observed_recognition_ids,
    recognition_source: Some(promotion.recognition.source),
    projection_kind: Some(projection_kind(&promotion.promotion_context.projection)),
    stability_kind: Some(stability_kind),
    stability_observed_frames,
    stability_reason,
    freshness_present: Some(promotion.promotion_context.freshness.is_some()),
    freshness_source_artifact,
    freshness_source_operation_id,
    permission_granted: Some(promotion.promotion_context.permission.is_some()),
    consent_id,
    consent_scope,
    consent_granted_by,
    decision_kind: Some(decision_kind),
    refusal_reasons,
    promoted_candidate_local_ids,
    known_limits: promotion.known_limits,
    issue,
  }
}

fn classify_detector_recognition_lineage(
  recognition: &RecognitionResult,
  capture_artifact: Option<&DetectorRecognitionArtifactRefLineage>,
) -> (DetectorRecognitionLineageStatus, Option<String>) {
  if recognition.scope.capture_artifact.is_none() {
    return (
      DetectorRecognitionLineageStatus::MissingCaptureArtifact,
      Some("scope.capture_artifact is missing".to_string()),
    );
  }
  if let Some(capture_artifact) = capture_artifact
    && !capture_artifact.resolved
  {
    return (
      DetectorRecognitionLineageStatus::CaptureArtifactUnresolved,
      Some("scope.capture_artifact could not be resolved from recorded run artifacts".to_string()),
    );
  }
  if recognition.evidence.is_empty() {
    return (
      DetectorRecognitionLineageStatus::MissingEvidence,
      Some("recognition evidence list is empty".to_string()),
    );
  }
  (DetectorRecognitionLineageStatus::Ready, None)
}

fn classify_candidate_promotion_lineage(
  promotion: &CandidatePromotionArtifact,
  source_recognition_artifact: Option<&ArtifactRefLineage>,
  capture_artifact: Option<&ArtifactRefLineage>,
) -> (CandidatePromotionLineageStatus, Option<String>) {
  if promotion.source_recognition_artifact.is_none() {
    return (
      CandidatePromotionLineageStatus::MissingSourceRecognitionArtifact,
      Some("source_recognition_artifact is missing".to_string()),
    );
  }
  if let Some(source_recognition_artifact) = source_recognition_artifact
    && !source_recognition_artifact.resolved
  {
    return (
      CandidatePromotionLineageStatus::SourceRecognitionArtifactUnresolved,
      Some(
        "source_recognition_artifact could not be resolved from recorded run artifacts".to_string(),
      ),
    );
  }
  if promotion.recognition.scope.capture_artifact.is_none() {
    return (
      CandidatePromotionLineageStatus::MissingCaptureArtifact,
      Some("recognition.scope.capture_artifact is missing".to_string()),
    );
  }
  if let Some(capture_artifact) = capture_artifact
    && !capture_artifact.resolved
  {
    return (
      CandidatePromotionLineageStatus::CaptureArtifactUnresolved,
      Some(
        "recognition.scope.capture_artifact could not be resolved from recorded run artifacts"
          .to_string(),
      ),
    );
  }
  if promotion.recognition.evidence.is_empty() {
    return (
      CandidatePromotionLineageStatus::MissingRecognitionEvidence,
      Some("embedded recognition evidence list is empty".to_string()),
    );
  }
  (CandidatePromotionLineageStatus::Ready, None)
}

fn artifact_record_lineage(
  run_id: crate::trace::RunId,
  artifact: &ArtifactRecordV1Alpha1,
) -> DetectorRecognitionArtifactRefLineage {
  DetectorRecognitionArtifactRefLineage {
    run_id,
    artifact_id: artifact.artifact_id.clone(),
    span_id: artifact.span_id.clone(),
    captured_event_id: artifact.event_id.clone(),
    role: Some(artifact.role.clone()),
    path: Some(artifact.path.clone()),
    summary: artifact.summary.clone(),
    resolved: true,
  }
}

fn resolve_artifact_ref(
  run: &CanonicalRun,
  reference: &ArtifactRef,
) -> DetectorRecognitionArtifactRefLineage {
  let resolved = if reference.run_id == run.run.run_id {
    run.artifacts.iter().find(|artifact| {
      artifact.artifact_id == reference.artifact_id && artifact.span_id == reference.span_id
    })
  } else {
    None
  };

  DetectorRecognitionArtifactRefLineage {
    run_id: reference.run_id.clone(),
    artifact_id: reference.artifact_id.clone(),
    span_id: reference.span_id.clone(),
    captured_event_id: reference.captured_event_id.clone(),
    role: resolved.map(|artifact| artifact.role.clone()),
    path: resolved.map(|artifact| artifact.path.clone()),
    summary: resolved.and_then(|artifact| artifact.summary.clone()),
    resolved: resolved.is_some(),
  }
}

fn detail_string(value: &serde_json::Value, path: &[&str]) -> Option<String> {
  let mut cursor = value;
  for key in path {
    cursor = cursor.get(*key)?;
  }
  cursor.as_str().map(str::to_string)
}

fn projection_kind(projection: &PromotionProjection) -> String {
  match projection {
    PromotionProjection::Unavailable { .. } => "unavailable".to_string(),
    PromotionProjection::IdentityWindowAddressable => "identity_window_addressable".to_string(),
  }
}

fn stability_summary(assessment: &StabilityAssessment) -> (String, Option<u32>, Option<String>) {
  match assessment {
    StabilityAssessment::Stable {
      observed_frames, ..
    } => ("stable".to_string(), Some(*observed_frames), None),
    StabilityAssessment::Unstable { reason } => (
      "unstable".to_string(),
      None,
      Some(stability_rejection_string(reason)),
    ),
  }
}

fn stability_rejection_string(reason: &StabilityRejection) -> String {
  match reason {
    StabilityRejection::NoFrames => "no_frames".to_string(),
    StabilityRejection::InsufficientFrames { have, need } => {
      format!("insufficient_frames: have={have} need={need}")
    }
    StabilityRejection::TargetMissingInFrame { frame_index } => {
      format!("target_missing_in_frame: frame_index={frame_index}")
    }
    StabilityRejection::UnstableKind {
      first,
      offending_frame,
    } => format!("unstable_kind: first={first} offending_frame={offending_frame}"),
    StabilityRejection::UnstableText { offending_frame } => {
      format!("unstable_text: offending_frame={offending_frame}")
    }
    StabilityRejection::DriftExceeded {
      observed_px,
      allowed_px,
      between_frames,
    } => format!(
      "drift_exceeded: observed_px={observed_px:.3} allowed_px={allowed_px:.3} between_frames={}..{}",
      between_frames.0, between_frames.1
    ),
  }
}

fn promotion_audit_summary(
  run: &CanonicalRun,
  decision: &CandidatePromotion,
) -> (
  Option<ArtifactRefLineage>,
  Option<String>,
  Option<String>,
  Option<ActionConsentScope>,
  Option<String>,
) {
  match decision {
    CandidatePromotion::Promoted { audit, .. } => (
      audit
        .freshness_source_artifact
        .as_ref()
        .map(|reference| resolve_artifact_ref(run, reference)),
      audit.freshness_source_operation_id.clone(),
      Some(audit.consent_id.clone()),
      Some(audit.consent_scope.clone()),
      Some(audit.consent_granted_by.clone()),
    ),
    CandidatePromotion::Refused { .. } => (None, None, None, None, None),
  }
}

fn promotion_decision_summary(decision: &CandidatePromotion) -> (String, Vec<String>, Vec<String>) {
  match decision {
    CandidatePromotion::Refused { reasons } => (
      "refused".to_string(),
      reasons.iter().map(promotion_refusal_string).collect(),
      Vec::new(),
    ),
    CandidatePromotion::Promoted { candidates, .. } => (
      "promoted".to_string(),
      Vec::new(),
      candidates
        .iter()
        .map(|candidate| candidate.candidate_local_id.clone())
        .collect(),
    ),
  }
}

fn promotion_refusal_string(reason: &PromotionRefusal) -> String {
  match reason {
    PromotionRefusal::EmptyRecognition => "empty_recognition".to_string(),
    PromotionRefusal::NoUnambiguousTarget => "no_unambiguous_target".to_string(),
    PromotionRefusal::NoRuntimeEvidence => "no_runtime_evidence".to_string(),
    PromotionRefusal::MissingCaptureArtifact => "missing_capture_artifact".to_string(),
    PromotionRefusal::ProjectionUnavailable { reason } => {
      format!("projection_unavailable: {reason}")
    }
    PromotionRefusal::StabilityUnproven { reason } => {
      format!("stability_unproven: {reason}")
    }
    PromotionRefusal::FreshnessUnknown => "freshness_unknown".to_string(),
    PromotionRefusal::FreshnessStale { reason } => format!("freshness_stale: {reason}"),
    PromotionRefusal::PermissionMissing => "permission_missing".to_string(),
    PromotionRefusal::PermissionInvalid { reason } => format!("permission_invalid: {reason}"),
  }
}

fn read_artifact_json<T: DeserializeOwned>(
  store: &LocalStore,
  run_id: &str,
  artifact: &ArtifactRecordV1Alpha1,
  artifact_role: &str,
) -> AuvResult<T> {
  let (bytes, artifact_path) = read_artifact_bytes(store, run_id, artifact, artifact_role)?;
  serde_json::from_slice(&bytes).map_err(|error| {
    format!(
      "failed to parse {artifact_role} artifact {} for run {run_id} from {}: {error}",
      artifact.artifact_id,
      artifact_path.display()
    )
  })
}

fn read_artifact_bytes(
  store: &LocalStore,
  run_id: &str,
  artifact: &ArtifactRecordV1Alpha1,
  artifact_role: &str,
) -> AuvResult<(Vec<u8>, PathBuf)> {
  let (_, artifact_path) = store.artifact_file_scoped(
    run_id,
    artifact.artifact_id.as_str(),
    Some(artifact.span_id.as_str()),
  )?;
  let bytes = fs::read(&artifact_path).map_err(|error| {
    format!(
      "failed to read {artifact_role} artifact {} for run {run_id} from {}: {error}",
      artifact.artifact_id,
      artifact_path.display()
    )
  })?;
  Ok((bytes, artifact_path))
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;
  use std::env;
  use std::fs;
  use std::path::{Path, PathBuf};

  use serde::Serialize;
  use serde_json::json;

  use super::{
    CANDIDATE_PROMOTION_ARTIFACT_ROLE, CandidatePromotionLineageStatus,
    DETECTOR_RECOGNITION_ARTIFACT_ROLE, DetectorRecognitionLineageStatus,
    NATIVE_TEXT_CANONICAL_TAXONOMY_ID, NATIVE_TEXT_LEGACY_TAXONOMY_ID,
    extract_app_validation_lineage, extract_candidate_promotion_lineage,
    extract_detector_recognition_lineage, extract_observation_snapshots, extract_verifications,
    list_app_validation_lineage, list_candidate_promotion_lineage,
    list_detector_recognition_lineage, list_observation_snapshots, list_verifications,
  };
  use crate::app::{
    AppIdentity, AppValidatedCandidate, AppValidation, AppValidationStatus, AppVerificationMode,
  };
  use crate::candidate_promotion::{
    ActionConsentAction, ActionConsentRecord, ActionConsentScope, ActionPermission,
    CandidatePromotion, PromotionAudit, PromotionContext, PromotionFreshness, PromotionProjection,
    PromotionRefusal, StabilityInput,
  };
  use crate::candidate_promotion_recording::CandidatePromotionArtifact;
  use crate::contract::{
    ArtifactRef, OBSERVATION_SNAPSHOT_API_VERSION, OPERATION_RESULT_API_VERSION,
    ObservationSnapshot, ObservationSource, OperationOutput, OperationResult, OperationStatus,
    RecognitionResult, RecognitionScope, RecognitionSource, RecognitionSurface, RecognizedItem,
    TargetGrounding, TargetSpec, VERIFICATION_RESULT_API_VERSION, VerificationMethod,
    VerificationResult,
  };
  use crate::scroll_scan::{
    CollectionObservation, CompletenessClaim, HookDecisionRecord, ObservationCluster,
    ScanPageRecord, ScanRegion, ScanTarget, ScrollBoundaryCandidate, ScrollScanArtifact,
    SectionCandidate, StopEvidence, StopPolicy, StopReason,
  };
  use crate::stability::{StabilityAssessment, StabilityPolicy, StabilityRejection};
  use crate::store::{ArtifactFileSource, CanonicalRun, LocalStore};
  use crate::trace::{
    ArtifactId, ArtifactRecordV1Alpha1, EventId, RUN_API_VERSION, RunId, RunRecordV1Alpha1,
    RunType, SPAN_API_VERSION, SpanId, SpanRecordV1Alpha1, TraceId, TraceState, TraceStatusCode,
  };

  #[test]
  fn read_side_extractors_collect_verifications_and_snapshots_from_json_artifacts() {
    let root = temp_dir("run-read-contracts");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_contracts");
    let span = dummy_span(&run.root_span_id);

    let legacy_verification = verification(
      VerificationMethod::TextVisible,
      Some("legacy verification".to_string()),
    );
    let top_level_verification = verification(
      VerificationMethod::SemanticMatch,
      Some("top-level verification".to_string()),
    );
    let duplicate_legacy_verification = verification(
      VerificationMethod::StateChanged,
      Some("legacy duplicate should be ignored".to_string()),
    );
    let observation_snapshot = dummy_observation_snapshot(&run.run_id, &span.span_id);

    let operation_legacy = OperationResult {
      api_version: OPERATION_RESULT_API_VERSION.to_string(),
      run_id: run.run_id.clone(),
      status: OperationStatus::Completed,
      operation_id: "verify.legacy".to_string(),
      evidence_artifacts: Vec::new(),
      output: OperationOutput::Verification {
        verification: Box::new(legacy_verification.clone()),
      },
      verifications: Vec::new(),
      freshness_basis: None,
      known_limits: Vec::new(),
    };
    let operation_top_level = OperationResult {
      api_version: OPERATION_RESULT_API_VERSION.to_string(),
      run_id: run.run_id.clone(),
      status: OperationStatus::Completed,
      operation_id: "music.result.play".to_string(),
      evidence_artifacts: Vec::new(),
      output: OperationOutput::Verification {
        verification: Box::new(duplicate_legacy_verification),
      },
      verifications: vec![top_level_verification.clone()],
      freshness_basis: None,
      known_limits: Vec::new(),
    };
    let scroll_scan_artifact = ScrollScanArtifact {
      scan_id: "scan_contracts".to_string(),
      target: ScanTarget {
        application_id: Some("com.example.music".to_string()),
        window_title: Some("Example Music".to_string()),
        region: ScanRegion {
          left_ratio: 0.1,
          top_ratio: 0.2,
          right_ratio: 0.9,
          bottom_ratio: 0.8,
        },
      },
      stop_policy: StopPolicy::Bounded {
        max_pages: 1,
        max_scrolls: 0,
      },
      pages: Vec::<ScanPageRecord>::new(),
      observations: Vec::<CollectionObservation>::new(),
      nodes: Vec::new(),
      snapshots: vec![observation_snapshot.clone()],
      clusters: Vec::<ObservationCluster>::new(),
      section_candidates: Vec::<SectionCandidate>::new(),
      scroll_boundary_candidates: Vec::<ScrollBoundaryCandidate>::new(),
      hook_decisions: Vec::<HookDecisionRecord>::new(),
      stop_evidence: StopEvidence {
        reason: StopReason::MaxPages,
        message: "bounded for test".to_string(),
        page_index: 0,
      },
      completeness_claim: CompletenessClaim::PartialMaxPages,
      warnings: Vec::new(),
    };

    let artifacts = vec![
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        0,
        "operation-result",
        "verify-legacy.json",
        &operation_legacy,
      ),
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        1,
        "operation-result",
        "music-result-play.json",
        &operation_top_level,
      ),
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        2,
        "scroll-scan",
        "scroll-scan.json",
        &scroll_scan_artifact,
      ),
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        3,
        "validation.output",
        "validation.json",
        &AppValidation {
          validate_version: "v0".to_string(),
          created_at_millis: 0,
          source_distillation_path: PathBuf::from("/tmp/distillation.json"),
          source_analysis_path: PathBuf::from("/tmp/analysis.json"),
          app_identity: AppIdentity {
            bundle_id: "com.example.music".to_string(),
            app_name: "Example Music".to_string(),
            app_path: None,
            main_executable_path: None,
            version: "1.0".to_string(),
            build_version: "100".to_string(),
            url_schemes: Vec::new(),
            apple_script_addressable: false,
            launch_services_resolved: true,
            resolution_notes: Vec::new(),
          },
          candidates: vec![AppValidatedCandidate {
            recipe_id: "macos.textedit.native_text_candidate.v0".to_string(),
            taxonomy_id: NATIVE_TEXT_LEGACY_TAXONOMY_ID.to_string(),
            status: AppValidationStatus::Validated,
            verification_mode: AppVerificationMode::MachineAsserted,
            rationale: "test".to_string(),
            used_annotation_ids: Vec::new(),
            recipe_path: PathBuf::from("/tmp/native-text.recipe.json"),
            case_matrix_path: PathBuf::from("/tmp/native-text.cases.json"),
            selected_case_count: 1,
            observed_consumer: Some("contract-candidate".to_string()),
            observed_candidate_local_id: Some("native-text-focus-ax".to_string()),
            candidate_source: Some("promoted_candidate".to_string()),
            unresolved_inputs: Vec::new(),
            failure_message: None,
            resolved_inputs: BTreeMap::new(),
          }],
          known_boundaries: Vec::new(),
        },
      ),
    ];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let canonical = store
      .read_run("run_read_contracts")
      .expect("run should read back");

    let extracted_verifications =
      extract_verifications(&store, &canonical).expect("verifications should extract");
    assert_eq!(
      extracted_verifications,
      vec![legacy_verification.clone(), top_level_verification.clone()]
    );
    let listed_verifications =
      list_verifications(&store, "run_read_contracts").expect("verifications should list");
    assert_eq!(listed_verifications, extracted_verifications);

    let extracted_snapshots = extract_observation_snapshots(&store, &canonical)
      .expect("observation snapshots should extract");
    assert_eq!(extracted_snapshots, vec![observation_snapshot.clone()]);
    let listed_snapshots = list_observation_snapshots(&store, "run_read_contracts")
      .expect("observation snapshots should list");
    assert_eq!(listed_snapshots, extracted_snapshots);

    let extracted_lineage = extract_app_validation_lineage(&store, &canonical)
      .expect("validation lineage should extract");
    assert_eq!(extracted_lineage.len(), 1);
    assert_eq!(
      extracted_lineage[0].canonical_taxonomy_id,
      NATIVE_TEXT_CANONICAL_TAXONOMY_ID
    );
    assert!(extracted_lineage[0].legacy_taxonomy_alias);
    assert_eq!(
      extracted_lineage[0].observed_consumer.as_deref(),
      Some("contract-candidate")
    );
    assert_eq!(
      extracted_lineage[0].candidate_source.as_deref(),
      Some("promoted_candidate")
    );
    let listed_lineage = list_app_validation_lineage(&store, "run_read_contracts")
      .expect("validation lineage should list");
    assert_eq!(listed_lineage, extracted_lineage);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn detector_recognition_lineage_extracts_ready_and_error_states() {
    let root = temp_dir("run-read-detector-recognition");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_detector_recognition");
    let span = dummy_span(&run.root_span_id);

    let capture_artifact = stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      "capture-image",
      "capture.png",
      "fake capture body",
    );
    let ready_recognition = detector_recognition_result(
      &run.run_id,
      &span.span_id,
      Some(ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: capture_artifact.artifact_id.clone(),
        span_id: span.span_id.clone(),
        captured_event_id: capture_artifact.event_id.clone(),
      }),
      vec![ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: capture_artifact.artifact_id.clone(),
        span_id: span.span_id.clone(),
        captured_event_id: capture_artifact.event_id.clone(),
      }],
      "recognition_ready",
    );
    let missing_capture = detector_recognition_result(
      &run.run_id,
      &span.span_id,
      None,
      vec![ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: capture_artifact.artifact_id.clone(),
        span_id: span.span_id.clone(),
        captured_event_id: capture_artifact.event_id.clone(),
      }],
      "recognition_missing_capture",
    );
    let missing_evidence = detector_recognition_result(
      &run.run_id,
      &span.span_id,
      Some(ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: capture_artifact.artifact_id.clone(),
        span_id: span.span_id.clone(),
        captured_event_id: capture_artifact.event_id.clone(),
      }),
      Vec::new(),
      "recognition_missing_evidence",
    );
    let unresolved_capture = detector_recognition_result(
      &run.run_id,
      &span.span_id,
      Some(ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: ArtifactId::new("artifact_missing_capture"),
        span_id: span.span_id.clone(),
        captured_event_id: Some(EventId::new("event_missing_capture")),
      }),
      vec![ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: ArtifactId::new("artifact_missing_capture"),
        span_id: span.span_id.clone(),
        captured_event_id: Some(EventId::new("event_missing_capture")),
      }],
      "recognition_unresolved_capture",
    );

    let artifacts = vec![
      capture_artifact,
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        1,
        DETECTOR_RECOGNITION_ARTIFACT_ROLE,
        "detector-ready.json",
        &ready_recognition,
      ),
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        2,
        DETECTOR_RECOGNITION_ARTIFACT_ROLE,
        "detector-missing-capture.json",
        &missing_capture,
      ),
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        3,
        DETECTOR_RECOGNITION_ARTIFACT_ROLE,
        "detector-missing-evidence.json",
        &missing_evidence,
      ),
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        4,
        DETECTOR_RECOGNITION_ARTIFACT_ROLE,
        "detector-unresolved-capture.json",
        &unresolved_capture,
      ),
      stage_text_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        5,
        DETECTOR_RECOGNITION_ARTIFACT_ROLE,
        "detector-malformed.json",
        "{ not valid json",
      ),
    ];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let canonical = store
      .read_run("run_read_detector_recognition")
      .expect("run should read back");
    let extracted = extract_detector_recognition_lineage(&store, &canonical)
      .expect("detector recognition lineage should extract");
    assert_eq!(extracted.len(), 5);
    assert_eq!(extracted[0].status, DetectorRecognitionLineageStatus::Ready);
    assert_eq!(
      extracted[0].backend.as_deref(),
      Some("ultralytics-inference")
    );
    assert_eq!(extracted[0].model_id.as_deref(), Some("games-balatro-ui"));
    assert_eq!(extracted[0].all_count, Some(2));
    assert_eq!(extracted[0].filtered_count, Some(1));
    assert_eq!(
      extracted[0]
        .capture_artifact
        .as_ref()
        .and_then(|artifact| artifact.role.as_deref()),
      Some("capture-image")
    );
    assert_eq!(
      extracted[1].status,
      DetectorRecognitionLineageStatus::MissingCaptureArtifact
    );
    assert_eq!(
      extracted[2].status,
      DetectorRecognitionLineageStatus::MissingEvidence
    );
    assert_eq!(
      extracted[3].status,
      DetectorRecognitionLineageStatus::CaptureArtifactUnresolved
    );
    assert_eq!(
      extracted[4].status,
      DetectorRecognitionLineageStatus::Malformed
    );
    assert!(
      extracted[4]
        .issue
        .as_deref()
        .unwrap_or_default()
        .contains("failed to parse detector-recognition artifact")
    );

    let listed = list_detector_recognition_lineage(&store, "run_read_detector_recognition")
      .expect("detector recognition lineage should list");
    assert_eq!(listed, extracted);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn candidate_promotion_lineage_extracts_ready_and_error_states() {
    let root = temp_dir("run-read-candidate-promotion");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_candidate_promotion");
    let span = dummy_span(&run.root_span_id);

    let capture_artifact = stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      "capture-image",
      "capture.png",
      "fake capture body",
    );
    let source_recognition_artifact = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      1,
      "detector-recognition",
      "detector-recognition.json",
      &detector_recognition_result(
        &run.run_id,
        &span.span_id,
        Some(ArtifactRef {
          run_id: run.run_id.clone(),
          artifact_id: capture_artifact.artifact_id.clone(),
          span_id: span.span_id.clone(),
          captured_event_id: capture_artifact.event_id.clone(),
        }),
        vec![ArtifactRef {
          run_id: run.run_id.clone(),
          artifact_id: capture_artifact.artifact_id.clone(),
          span_id: span.span_id.clone(),
          captured_event_id: capture_artifact.event_id.clone(),
        }],
        "recognition_ready",
      ),
    );

    let ready_promotion = candidate_promotion_artifact(
      &run.run_id,
      &span.span_id,
      Some(ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: source_recognition_artifact.artifact_id.clone(),
        span_id: span.span_id.clone(),
        captured_event_id: source_recognition_artifact.event_id.clone(),
      }),
      Some(ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: capture_artifact.artifact_id.clone(),
        span_id: span.span_id.clone(),
        captured_event_id: capture_artifact.event_id.clone(),
      }),
      vec![ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: capture_artifact.artifact_id.clone(),
        span_id: span.span_id.clone(),
        captured_event_id: capture_artifact.event_id.clone(),
      }],
      CandidatePromotion::Promoted {
        candidates: vec![sample_candidate(
          &run.run_id,
          &span.span_id,
          &capture_artifact,
          "promoted-item_end_turn",
        )],
        residual_known_limits: vec!["fixture-backed candidate".to_string()],
        audit: PromotionAudit {
          freshness_source_artifact: Some(ArtifactRef {
            run_id: run.run_id.clone(),
            artifact_id: capture_artifact.artifact_id.clone(),
            span_id: span.span_id.clone(),
            captured_event_id: capture_artifact.event_id.clone(),
          }),
          freshness_source_operation_id: Some("observe.window.capture".to_string()),
          consent_id: "consent_promotion_ready".to_string(),
          consent_scope: ActionConsentScope {
            surface: RecognitionSurface::Window,
            app_bundle_id: Some("com.megacrit.cardcrawl".to_string()),
            window_title: Some("Slay the Spire".to_string()),
            window_number: Some(7),
          },
          consent_granted_by: "human-review".to_string(),
          projection_kind: "identity_window_addressable".to_string(),
          stability_observed_frames: Some(2),
        },
      },
      "promotion_ready",
    );
    let refused_promotion = candidate_promotion_artifact(
      &run.run_id,
      &span.span_id,
      Some(ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: source_recognition_artifact.artifact_id.clone(),
        span_id: span.span_id.clone(),
        captured_event_id: source_recognition_artifact.event_id.clone(),
      }),
      Some(ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: capture_artifact.artifact_id.clone(),
        span_id: span.span_id.clone(),
        captured_event_id: capture_artifact.event_id.clone(),
      }),
      vec![ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: capture_artifact.artifact_id.clone(),
        span_id: span.span_id.clone(),
        captured_event_id: capture_artifact.event_id.clone(),
      }],
      CandidatePromotion::Refused {
        reasons: vec![PromotionRefusal::StabilityUnproven {
          reason: "InsufficientFrames { have: 1, need: 3 }".to_string(),
        }],
      },
      "promotion_refused",
    );
    let missing_source_promotion = candidate_promotion_artifact(
      &run.run_id,
      &span.span_id,
      None,
      Some(ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: capture_artifact.artifact_id.clone(),
        span_id: span.span_id.clone(),
        captured_event_id: capture_artifact.event_id.clone(),
      }),
      vec![ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: capture_artifact.artifact_id.clone(),
        span_id: span.span_id.clone(),
        captured_event_id: capture_artifact.event_id.clone(),
      }],
      CandidatePromotion::Refused {
        reasons: vec![PromotionRefusal::PermissionMissing],
      },
      "promotion_missing_source",
    );
    let unresolved_capture_promotion = candidate_promotion_artifact(
      &run.run_id,
      &span.span_id,
      Some(ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: source_recognition_artifact.artifact_id.clone(),
        span_id: span.span_id.clone(),
        captured_event_id: source_recognition_artifact.event_id.clone(),
      }),
      Some(ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: ArtifactId::new("artifact_missing_capture"),
        span_id: span.span_id.clone(),
        captured_event_id: Some(EventId::new("event_missing_capture")),
      }),
      vec![ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: ArtifactId::new("artifact_missing_capture"),
        span_id: span.span_id.clone(),
        captured_event_id: Some(EventId::new("event_missing_capture")),
      }],
      CandidatePromotion::Refused {
        reasons: vec![PromotionRefusal::ProjectionUnavailable {
          reason: "projection missing".to_string(),
        }],
      },
      "promotion_unresolved_capture",
    );

    let artifacts = vec![
      capture_artifact,
      source_recognition_artifact,
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        2,
        CANDIDATE_PROMOTION_ARTIFACT_ROLE,
        "candidate-promotion-ready.json",
        &ready_promotion,
      ),
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        3,
        CANDIDATE_PROMOTION_ARTIFACT_ROLE,
        "candidate-promotion-refused.json",
        &refused_promotion,
      ),
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        4,
        CANDIDATE_PROMOTION_ARTIFACT_ROLE,
        "candidate-promotion-missing-source.json",
        &missing_source_promotion,
      ),
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        5,
        CANDIDATE_PROMOTION_ARTIFACT_ROLE,
        "candidate-promotion-unresolved-capture.json",
        &unresolved_capture_promotion,
      ),
      stage_text_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        6,
        CANDIDATE_PROMOTION_ARTIFACT_ROLE,
        "candidate-promotion-malformed.json",
        "{ not valid json",
      ),
    ];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let canonical = store
      .read_run("run_read_candidate_promotion")
      .expect("run should read back");
    let extracted = extract_candidate_promotion_lineage(&store, &canonical)
      .expect("candidate promotion lineage should extract");
    assert_eq!(extracted.len(), 5);
    assert_eq!(extracted[0].status, CandidatePromotionLineageStatus::Ready);
    assert_eq!(extracted[0].decision_kind.as_deref(), Some("promoted"));
    assert_eq!(
      extracted[0].promoted_candidate_local_ids,
      vec!["promoted-item_end_turn".to_string()]
    );
    assert_eq!(
      extracted[0].freshness_source_operation_id.as_deref(),
      Some("observe.window.capture")
    );
    assert_eq!(
      extracted[0]
        .freshness_source_artifact
        .as_ref()
        .and_then(|artifact| artifact.role.as_deref()),
      Some("capture-image")
    );
    assert_eq!(
      extracted[0].consent_id.as_deref(),
      Some("consent_promotion_ready")
    );
    assert_eq!(
      extracted[0].consent_granted_by.as_deref(),
      Some("human-review")
    );
    assert_eq!(
      extracted[0]
        .consent_scope
        .as_ref()
        .and_then(|scope| scope.window_title.as_deref()),
      Some("Slay the Spire")
    );
    assert_eq!(extracted[1].status, CandidatePromotionLineageStatus::Ready);
    assert_eq!(extracted[1].decision_kind.as_deref(), Some("refused"));
    assert_eq!(
      extracted[1].refusal_reasons,
      vec!["stability_unproven: InsufficientFrames { have: 1, need: 3 }".to_string()]
    );
    assert_eq!(
      extracted[2].status,
      CandidatePromotionLineageStatus::MissingSourceRecognitionArtifact
    );
    assert_eq!(
      extracted[3].status,
      CandidatePromotionLineageStatus::CaptureArtifactUnresolved
    );
    assert_eq!(
      extracted[4].status,
      CandidatePromotionLineageStatus::Malformed
    );
    assert!(
      extracted[4]
        .issue
        .as_deref()
        .unwrap_or_default()
        .contains("failed to parse candidate-promotion artifact")
    );

    let listed = list_candidate_promotion_lineage(&store, "run_read_candidate_promotion")
      .expect("candidate promotion lineage should list");
    assert_eq!(listed, extracted);

    let _ = fs::remove_dir_all(root);
  }

  fn temp_dir(label: &str) -> PathBuf {
    let path = env::temp_dir().join(format!("auv-{}-{}", label, crate::model::now_millis()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("temp dir should be creatable");
    path
  }

  fn dummy_run(run_id: &str) -> RunRecordV1Alpha1 {
    let root_span_id = SpanId::new("0000000000000001");
    RunRecordV1Alpha1 {
      api_version: RUN_API_VERSION.to_string(),
      run_id: RunId::new(run_id),
      trace_id: TraceId::new("00000000000000000000000000000001"),
      run_type: RunType::Execute,
      state: TraceState::Ended,
      status_code: TraceStatusCode::Ok,
      started_at_millis: 100,
      finished_at_millis: Some(200),
      root_span_id,
      attributes: BTreeMap::new(),
      summary: Some("done".to_string()),
      failure: None,
    }
  }

  fn dummy_span(span_id: &SpanId) -> SpanRecordV1Alpha1 {
    SpanRecordV1Alpha1 {
      api_version: SPAN_API_VERSION.to_string(),
      span_id: span_id.clone(),
      parent_span_id: None,
      name: "auv.run.read".to_string(),
      state: TraceState::Ended,
      status_code: TraceStatusCode::Ok,
      started_at_millis: 100,
      finished_at_millis: Some(200),
      attributes: BTreeMap::new(),
      summary: None,
      failure: None,
    }
  }

  fn verification(
    method: VerificationMethod,
    observed_label: Option<String>,
  ) -> VerificationResult {
    VerificationResult {
      api_version: VERIFICATION_RESULT_API_VERSION.to_string(),
      method,
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
      observed_label,
    }
  }

  fn dummy_observation_snapshot(run_id: &RunId, span_id: &SpanId) -> ObservationSnapshot {
    ObservationSnapshot {
      api_version: OBSERVATION_SNAPSHOT_API_VERSION.to_string(),
      snapshot_id: "snapshot_contracts".to_string(),
      run_id: run_id.clone(),
      span_id: span_id.clone(),
      captured_at_millis: 150,
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
      detail: json!({"producer": "scroll_scan"}),
      known_limits: vec!["visual only".to_string()],
    }
  }

  fn stage_json_artifact<T: Serialize>(
    store: &LocalStore,
    root: &Path,
    run_id: &RunId,
    span_id: &SpanId,
    index: usize,
    role: &str,
    preferred_name: &str,
    value: &T,
  ) -> ArtifactRecordV1Alpha1 {
    let source_path = root.join(format!("source-{index}-{preferred_name}"));
    let rendered =
      serde_json::to_string_pretty(value).expect("artifact json should serialize") + "\n";
    fs::write(&source_path, rendered).expect("artifact source should write");
    store
      .stage_artifact_file(
        run_id,
        index,
        span_id,
        None,
        ArtifactFileSource {
          role: role.to_string(),
          source_path,
          preferred_name: preferred_name.to_string(),
          summary: None,
        },
      )
      .expect("artifact should stage")
  }

  fn stage_text_artifact(
    store: &LocalStore,
    root: &Path,
    run_id: &RunId,
    span_id: &SpanId,
    index: usize,
    role: &str,
    preferred_name: &str,
    content: &str,
  ) -> ArtifactRecordV1Alpha1 {
    let source_path = root.join(format!("source-{index}-{preferred_name}"));
    fs::write(&source_path, content).expect("artifact source should write");
    store
      .stage_artifact_file(
        run_id,
        index,
        span_id,
        None,
        ArtifactFileSource {
          role: role.to_string(),
          source_path,
          preferred_name: preferred_name.to_string(),
          summary: None,
        },
      )
      .expect("artifact should stage")
  }

  fn detector_recognition_result(
    run_id: &RunId,
    span_id: &SpanId,
    capture_artifact: Option<ArtifactRef>,
    evidence: Vec<ArtifactRef>,
    recognition_id: &str,
  ) -> RecognitionResult {
    RecognitionResult {
      recognition_id: recognition_id.to_string(),
      source: RecognitionSource::Custom,
      scope: RecognitionScope {
        surface: RecognitionSurface::Region,
        display_ref: Some("display-main".to_string()),
        native_display_id: Some("69733248".to_string()),
        app_bundle_id: Some("com.playstack.balatro".to_string()),
        window_title: Some("Balatro".to_string()),
        window_number: Some(7),
        region_hint: None,
        capture_artifact,
        capture_contract_artifact: Some(ArtifactRef {
          run_id: run_id.clone(),
          artifact_id: ArtifactId::new("artifact_contract"),
          span_id: span_id.clone(),
          captured_event_id: Some(EventId::new("event_contract")),
        }),
      },
      best: None,
      filtered: vec![RecognizedItem {
        item_id: "detector:games-balatro-ui:0".to_string(),
        kind: "ui_button_play".to_string(),
        box_: crate::contract::RecognitionBox {
          x: 10,
          y: 20,
          width: 30,
          height: 40,
        },
        text: None,
        provider_score: Some(0.98),
        detail: json!({}),
      }],
      all: vec![
        RecognizedItem {
          item_id: "detector:games-balatro-ui:0".to_string(),
          kind: "ui_button_play".to_string(),
          box_: crate::contract::RecognitionBox {
            x: 10,
            y: 20,
            width: 30,
            height: 40,
          },
          text: None,
          provider_score: Some(0.98),
          detail: json!({}),
        },
        RecognizedItem {
          item_id: "detector:games-balatro-ui:1".to_string(),
          kind: "ui_score".to_string(),
          box_: crate::contract::RecognitionBox {
            x: 50,
            y: 60,
            width: 70,
            height: 80,
          },
          text: None,
          provider_score: Some(0.87),
          detail: json!({}),
        },
      ],
      detail: json!({
        "backend": "ultralytics-inference",
        "model_id": "games-balatro-ui",
        "execution_provider": "cpu",
        "class_label_source": { "kind": "override_file" },
        "runtime_projection": { "kind": "identity_source_image_pixels" },
      }),
      evidence,
      known_limits: vec![
        "projection basis is unavailable outside capture-integrated runtime".to_string(),
        "detector RecognitionResult is recognition evidence only, not candidate-ready output"
          .to_string(),
      ],
    }
  }

  fn sample_candidate(
    run_id: &RunId,
    span_id: &SpanId,
    capture_artifact: &ArtifactRecordV1Alpha1,
    candidate_local_id: &str,
  ) -> crate::contract::Candidate {
    crate::contract::Candidate {
      candidate_local_id: candidate_local_id.to_string(),
      kind: "button".to_string(),
      label: Some("End Turn".to_string()),
      target_spec: TargetSpec {
        grounding: TargetGrounding::Coordinate,
        anchor_text: Some("End Turn".to_string()),
        region_hint: None,
        row_index: None,
      },
      evidence: crate::contract::CandidateEvidence {
        artifact_ref: ArtifactRef {
          run_id: run_id.clone(),
          artifact_id: capture_artifact.artifact_id.clone(),
          span_id: span_id.clone(),
          captured_event_id: capture_artifact.event_id.clone(),
        },
        observation: json!({"item_id": "item_end_turn"}),
      },
      liveness: crate::contract::CandidateLiveness {
        preconditions: crate::contract::LivenessPreconditions {
          window_ref: Some(crate::contract::WindowRefPrecondition {
            app_bundle_id: "com.megacrit.cardcrawl".to_string(),
            window_title_substring: Some("Slay the Spire".to_string()),
            window_number: Some(7),
          }),
          anchor_recheck: None,
        },
        ttl_hint_ms: None,
      },
      control: crate::contract::ControlRequirements {
        requires_app_frontmost: true,
        requires_window_focus: true,
      },
      known_limits: vec!["fixture-backed candidate".to_string()],
    }
  }

  fn candidate_promotion_artifact(
    run_id: &RunId,
    span_id: &SpanId,
    source_recognition_artifact: Option<ArtifactRef>,
    capture_artifact: Option<ArtifactRef>,
    evidence: Vec<ArtifactRef>,
    decision: CandidatePromotion,
    promotion_id: &str,
  ) -> CandidatePromotionArtifact {
    let stability_assessment = match &decision {
      CandidatePromotion::Promoted { .. } => StabilityAssessment::Stable {
        observed_frames: 2,
        max_observed_drift_px: 2.0,
      },
      CandidatePromotion::Refused { reasons }
        if reasons
          .iter()
          .any(|reason| matches!(reason, PromotionRefusal::StabilityUnproven { .. })) =>
      {
        StabilityAssessment::Unstable {
          reason: StabilityRejection::InsufficientFrames { have: 1, need: 3 },
        }
      }
      CandidatePromotion::Refused { .. } => StabilityAssessment::Stable {
        observed_frames: 2,
        max_observed_drift_px: 2.0,
      },
    };
    let projection = match &decision {
      CandidatePromotion::Refused { reasons }
        if reasons
          .iter()
          .any(|reason| matches!(reason, PromotionRefusal::ProjectionUnavailable { .. })) =>
      {
        PromotionProjection::Unavailable {
          reason: "projection missing".to_string(),
        }
      }
      _ => PromotionProjection::IdentityWindowAddressable,
    };
    let stability_input = match &decision {
      CandidatePromotion::Refused { reasons }
        if reasons
          .iter()
          .any(|reason| matches!(reason, PromotionRefusal::StabilityUnproven { .. })) =>
      {
        StabilityInput::Unproven {
          reason: "InsufficientFrames { have: 1, need: 3 }".to_string(),
        }
      }
      _ => StabilityInput::Proven { observed_frames: 2 },
    };

    CandidatePromotionArtifact {
      artifact_version: "candidate_promotion_artifact_v0".to_string(),
      promotion_id: promotion_id.to_string(),
      source_recognition_artifact,
      observed_recognition_ids: vec![
        format!("{promotion_id}_frame_0"),
        format!("{promotion_id}_frame_1"),
      ],
      promotion_input_recognition_id: format!("{promotion_id}_frame_1"),
      promotion_input_frame_index: 1,
      stability_policy: StabilityPolicy {
        min_frames: 2,
        max_centroid_drift_px: 8.0,
        require_stable_text: true,
      },
      stability_assessment,
      promotion_context: PromotionContext {
        projection,
        stability: stability_input,
        freshness: Some(PromotionFreshness {
          source_artifact: capture_artifact.clone(),
          source_operation_id: Some("observe.window.capture".to_string()),
          observed_at_millis: Some(2_000),
          max_age_ms: Some(500),
          notes: vec!["fixture freshness".to_string()],
        }),
        permission: Some(ActionPermission {
          granted_by: "human-review".to_string(),
          scope_note: "fixture promotion".to_string(),
          consent: ActionConsentRecord {
            consent_id: format!("consent_{promotion_id}"),
            recognition_id: format!("{promotion_id}_frame_1"),
            run_id: run_id.clone(),
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
            evidence_note: "fixture consent".to_string(),
          },
        }),
        checked_at_millis: 2_100,
      },
      decision,
      recognition: RecognitionResult {
        recognition_id: format!("{promotion_id}_frame_1"),
        source: RecognitionSource::Custom,
        scope: RecognitionScope {
          surface: RecognitionSurface::Window,
          display_ref: Some("display-main".to_string()),
          native_display_id: Some("69733248".to_string()),
          app_bundle_id: Some("com.megacrit.cardcrawl".to_string()),
          window_title: Some("Slay the Spire".to_string()),
          window_number: Some(7),
          region_hint: None,
          capture_artifact,
          capture_contract_artifact: Some(ArtifactRef {
            run_id: run_id.clone(),
            artifact_id: ArtifactId::new("artifact_contract"),
            span_id: span_id.clone(),
            captured_event_id: Some(EventId::new("event_contract")),
          }),
        },
        best: Some(RecognizedItem {
          item_id: "item_end_turn".to_string(),
          kind: "button".to_string(),
          box_: crate::contract::RecognitionBox {
            x: 1638,
            y: 792,
            width: 228,
            height: 178,
          },
          text: Some("End Turn".to_string()),
          provider_score: Some(0.99),
          detail: json!({"backend": "fixture"}),
        }),
        filtered: vec![RecognizedItem {
          item_id: "item_end_turn".to_string(),
          kind: "button".to_string(),
          box_: crate::contract::RecognitionBox {
            x: 1638,
            y: 792,
            width: 228,
            height: 178,
          },
          text: Some("End Turn".to_string()),
          provider_score: Some(0.99),
          detail: json!({"backend": "fixture"}),
        }],
        all: vec![RecognizedItem {
          item_id: "item_end_turn".to_string(),
          kind: "button".to_string(),
          box_: crate::contract::RecognitionBox {
            x: 1638,
            y: 792,
            width: 228,
            height: 178,
          },
          text: Some("End Turn".to_string()),
          provider_score: Some(0.99),
          detail: json!({"backend": "fixture"}),
        }],
        detail: json!({
          "backend": "fixture",
          "model_id": "slay-the-spire-observe-only",
        }),
        evidence,
        known_limits: vec![
          "candidate promotion artifact records gate decisions only; runtime action consumption remains deferred".to_string(),
        ],
      },
      detail: json!({
        "artifact_version": "candidate_promotion_artifact_v0",
        "decision_kind": "fixture",
      }),
      known_limits: vec![
        "candidate promotion artifact records gate decisions only; runtime action consumption remains deferred".to_string(),
      ],
    }
  }
}
