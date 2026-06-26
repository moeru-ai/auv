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
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use serde::de::DeserializeOwned;

use crate::candidate_action_decision::{
  CandidateActionDecisionArtifact, CandidateActionExecutionArtifact,
};
use crate::candidate_promotion::{CandidatePromotion, PromotionProjection, PromotionRefusal};
use crate::candidate_promotion_recording::CandidatePromotionArtifact;
use crate::contract::{
  ArtifactRef, ObservationSnapshot, OperationOutput, OperationResult, RecognitionResult,
  RecognitionSource, VerificationResult,
};
use crate::model::AuvResult;
use crate::scroll_scan::ScrollScanArtifact;
use crate::stability::{StabilityAssessment, StabilityRejection};
use auv_game_minecraft::artifact::MinecraftProjectionArtifact;
use auv_game_minecraft::dataset::{SourceRunSummary, SpatialBundleCounts};
use auv_game_minecraft::{
  TrainingCompatibilityViewReport, TrainingLaunchInspectReport, TrainingLaunchJobInspectReport,
  TrainingLaunchJobManifest, TrainingLaunchPlanManifest, TrainingPackageCounts,
  TrainingPackageInspectReport, TrainingPackageManifest, TrainingResultArtifactFetchInspectReport,
  TrainingResultArtifactFetchManifest, TrainingResultInspectReport, TrainingResultManifest,
};
use auv_tracing_driver::store::{CanonicalRun, LocalStore};
use auv_tracing_driver::trace::ArtifactRecordV1Alpha1;

pub struct MinecraftTelemetrySampleArtifactLineage {
  pub artifact: ArtifactRefLineage,
  pub line_count: Option<usize>,
  pub byte_size: Option<u64>,
  pub issue: Option<String>,
}

pub struct MinecraftSpatialBundleManifestLineage {
  pub artifact: ArtifactRefLineage,
  pub manifest: Option<MinecraftSpatialBundleManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct MinecraftTrainingPackageManifestLineage {
  pub artifact: ArtifactRefLineage,
  pub manifest: Option<MinecraftTrainingPackageManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct MinecraftTrainingLaunchManifestLineage {
  pub artifact: ArtifactRefLineage,
  pub manifest: Option<MinecraftTrainingLaunchManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct MinecraftTrainingLaunchInspectReportLineage {
  pub artifact: ArtifactRefLineage,
  pub report: Option<MinecraftTrainingLaunchInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct MinecraftTrainingJobManifestLineage {
  pub artifact: ArtifactRefLineage,
  pub manifest: Option<MinecraftTrainingJobManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct MinecraftTrainingJobInspectReportLineage {
  pub artifact: ArtifactRefLineage,
  pub report: Option<MinecraftTrainingJobInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct MinecraftTrainingResultManifestLineage {
  pub artifact: ArtifactRefLineage,
  pub manifest: Option<MinecraftTrainingResultManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct MinecraftTrainingResultInspectReportLineage {
  pub artifact: ArtifactRefLineage,
  pub report: Option<MinecraftTrainingResultInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct MinecraftTrainingResultArtifactFetchManifestLineage {
  pub artifact: ArtifactRefLineage,
  pub manifest: Option<MinecraftTrainingResultArtifactFetchManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct MinecraftTrainingResultArtifactFetchInspectReportLineage {
  pub artifact: ArtifactRefLineage,
  pub report: Option<MinecraftTrainingResultArtifactFetchInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct MinecraftTrainingPackageInspectReportLineage {
  pub artifact: ArtifactRefLineage,
  pub report: Option<MinecraftTrainingPackageInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MinecraftTrainingLaunchManifestSummary {
  pub schema_version: u32,
  pub source_training_package_manifest_path: String,
  pub source_training_package_inspect_report_path: String,
  pub source_scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub counts: TrainingPackageCounts,
  pub compatibility_view_name: String,
  pub trainer_backend: String,
  pub training_data_dir: String,
  pub transforms_path: Option<String>,
  pub export_report_path: String,
  pub suggested_output_dir: String,
  pub launch_command: String,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MinecraftTrainingLaunchInspectReportSummary {
  pub schema_version: u32,
  pub training_launch_manifest_path: String,
  pub source_training_package_manifest_path: String,
  pub source_scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub compatibility_status: String,
  pub trainer_readiness: String,
  pub readiness_blocker: Option<String>,
  pub probe_command: String,
  pub probe_succeeded: bool,
  pub exported_frame_count: usize,
  pub skipped_frame_count: usize,
  pub transforms_present: bool,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MinecraftTrainingJobManifestSummary {
  pub schema_version: u32,
  pub source_training_launch_plan_path: String,
  pub source_training_package_manifest_path: String,
  pub source_training_package_inspect_report_path: String,
  pub source_scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub counts: TrainingPackageCounts,
  pub compatibility_view_name: String,
  pub provider_backend: String,
  pub trainer_backend: String,
  pub job_backend: String,
  pub job_submission_endpoint: String,
  pub job_submission_command: String,
  pub training_data_dir: String,
  pub transforms_path: Option<String>,
  pub export_report_path: String,
  pub suggested_output_dir: String,
  pub launch_command: String,
  pub status: String,
  pub job_id: Option<String>,
  pub job_url: Option<String>,
  pub readiness_blocker: Option<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MinecraftTrainingJobInspectReportSummary {
  pub schema_version: u32,
  pub training_launch_manifest_path: String,
  pub source_training_launch_plan_path: String,
  pub source_training_package_manifest_path: String,
  pub source_scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub provider_backend: String,
  pub job_backend: String,
  pub trainer_backend: String,
  pub job_submission_endpoint: String,
  pub job_submission_command: String,
  pub status: String,
  pub job_id: Option<String>,
  pub job_url: Option<String>,
  pub readiness_blocker: Option<String>,
  pub probe_command: String,
  pub probe_succeeded: bool,
  pub exported_frame_count: usize,
  pub skipped_frame_count: usize,
  pub transforms_present: bool,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MinecraftTrainingResultManifestSummary {
  pub schema_version: u32,
  pub source_training_job_manifest_path: String,
  pub source_training_launch_plan_path: String,
  pub source_training_package_manifest_path: String,
  pub source_scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub trainer_backend: String,
  pub job_backend: String,
  pub job_submission_endpoint: String,
  pub source_job_status: String,
  pub status: String,
  pub job_id: String,
  pub job_url: Option<String>,
  pub result_dir: String,
  pub result_artifacts: Vec<MinecraftTrainingResultArtifactSummary>,
  pub exported_frame_count: usize,
  pub skipped_frame_count: usize,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MinecraftTrainingResultArtifactSummary {
  pub relative_path: String,
  pub absolute_path: String,
  pub readable: bool,
  pub byte_size: Option<u64>,
}

impl From<auv_game_minecraft::TrainingResultArtifactRecord>
  for MinecraftTrainingResultArtifactSummary
{
  fn from(value: auv_game_minecraft::TrainingResultArtifactRecord) -> Self {
    Self {
      relative_path: value.relative_path,
      absolute_path: value.absolute_path,
      readable: value.readable,
      byte_size: value.byte_size,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MinecraftTrainingResultInspectReportSummary {
  pub schema_version: u32,
  pub training_result_manifest_path: String,
  pub source_training_job_manifest_path: String,
  pub source_training_launch_plan_path: String,
  pub source_scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub trainer_backend: String,
  pub job_backend: String,
  pub job_submission_endpoint: String,
  pub source_job_status: String,
  pub status: String,
  pub status_reason: Option<String>,
  pub job_id: String,
  pub job_url: Option<String>,
  pub result_dir: String,
  pub result_dir_exists: bool,
  pub key_result_artifacts_present: bool,
  pub result_artifact_count: usize,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MinecraftTrainingResultArtifactFetchManifestSummary {
  pub schema_version: u32,
  pub source_training_result_manifest_path: String,
  pub source_training_job_manifest_path: String,
  pub source_training_launch_plan_path: String,
  pub source_training_package_manifest_path: String,
  pub source_scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub trainer_backend: String,
  pub job_backend: String,
  pub source_job_status: String,
  pub source_result_status: String,
  pub source_result_status_reason: Option<String>,
  pub source_result_dir: String,
  pub normalized_result_dir: String,
  pub normalized_artifacts: Vec<MinecraftTrainingResultNormalizedArtifactSummary>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MinecraftTrainingResultArtifactFetchInspectReportSummary {
  pub schema_version: u32,
  pub training_result_artifact_fetch_manifest_path: String,
  pub source_training_result_manifest_path: String,
  pub source_training_job_manifest_path: String,
  pub source_training_launch_plan_path: String,
  pub source_scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub trainer_backend: String,
  pub job_backend: String,
  pub source_job_status: String,
  pub source_result_status: String,
  pub source_result_status_reason: Option<String>,
  pub fetch_status: String,
  pub fetch_reason: Option<String>,
  pub source_result_dir: String,
  pub normalized_result_dir: String,
  pub source_result_dir_exists: bool,
  pub required_artifacts_present: bool,
  pub normalized_artifact_count: usize,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MinecraftTrainingResultNormalizedArtifactSummary {
  pub kind: String,
  pub relative_path: String,
  pub absolute_path: String,
  pub readable: bool,
  pub byte_size: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MinecraftSpatialBundleManifestSummary {
  pub schema_version: u32,
  pub source_run: SourceRunSummary,
  pub counts: SpatialBundleCounts,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MinecraftTrainingPackageManifestSummary {
  pub schema_version: u32,
  pub source_scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub counts: TrainingPackageCounts,
  pub compatibility_views: Vec<TrainingCompatibilityViewReport>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MinecraftTrainingPackageInspectReportSummary {
  pub schema_version: u32,
  pub training_package_manifest_path: String,
  pub scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub counts: TrainingPackageCounts,
  pub compatibility_views: Vec<TrainingCompatibilityViewReport>,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

pub fn read_run(store: &LocalStore, run_id: &str) -> AuvResult<CanonicalRun> {
  store.read_run(run_id)
}

const DETECTOR_RECOGNITION_ARTIFACT_ROLE: &str = "detector-recognition";
const CANDIDATE_PROMOTION_ARTIFACT_ROLE: &str = "candidate-promotion";
const CANDIDATE_ACTION_DECISION_ARTIFACT_ROLE: &str = "candidate-action-decision";
const CANDIDATE_ACTION_EXECUTION_ARTIFACT_ROLE: &str = "candidate-action-execution";

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
  pub run_id: auv_tracing_driver::trace::RunId,
  pub artifact_id: auv_tracing_driver::trace::ArtifactId,
  pub span_id: auv_tracing_driver::trace::SpanId,
  pub captured_event_id: Option<auv_tracing_driver::trace::EventId>,
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
  pub permission_granted_by: Option<String>,
  pub permission_scope_note: Option<String>,
  pub consent_id: Option<String>,
  pub consent_provenance: Option<String>,
  pub consent_grade: Option<String>,
  pub consent_scope: Option<String>,
  pub consent_approved_action: Option<String>,
  pub consent_recognition_id: Option<String>,
  pub decision_kind: Option<String>,
  pub refusal_reasons: Vec<String>,
  pub promoted_candidate_local_ids: Vec<String>,
  pub known_limits: Vec<String>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateActionDecisionLineageStatus {
  Ready,
  MissingSourceCandidatePromotionArtifact,
  SourceCandidatePromotionArtifactUnresolved,
  Malformed,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct CandidateActionDecisionLineage {
  pub artifact: ArtifactRefLineage,
  pub status: CandidateActionDecisionLineageStatus,
  pub decision_id: Option<String>,
  pub source_candidate_promotion_artifact: Option<ArtifactRefLineage>,
  pub source_promotion_id: Option<String>,
  pub candidate_local_id: Option<String>,
  pub resolver_operation: Option<String>,
  pub selected_method: Option<String>,
  pub primary_method: Option<String>,
  pub fallback_allowed: Option<bool>,
  pub fallback_used: Option<bool>,
  pub fallback_reason: Option<String>,
  pub policy: Option<String>,
  pub cursor_disturbance: Option<String>,
  pub press_mechanism: Option<String>,
  pub side_effect: Option<String>,
  pub input_delivery: Option<String>,
  pub operation_result: Option<String>,
  pub verification_result: Option<String>,
  pub known_limits: Vec<String>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateActionExecutionLineageStatus {
  Ready,
  BlockedNotReady,
  MissingSourceCandidateActionDecisionArtifact,
  SourceCandidateActionDecisionArtifactUnresolved,
  MissingOperationResultArtifact,
  OperationResultArtifactUnresolved,
  Malformed,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateActionExecutionClosureState {
  EvidenceClosed,
  SemanticOpen,
  BlockedByReadiness,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct CandidateActionExecutionLineage {
  pub artifact: ArtifactRefLineage,
  pub status: CandidateActionExecutionLineageStatus,
  pub execution_id: Option<String>,
  pub source_candidate_action_decision_artifact: Option<ArtifactRefLineage>,
  pub source_candidate_promotion_artifact: Option<ArtifactRefLineage>,
  pub operation_result_artifact: Option<ArtifactRefLineage>,
  pub source_promotion_id: Option<String>,
  pub source_decision_id: Option<String>,
  pub candidate_local_id: Option<String>,
  pub resolver_operation: Option<String>,
  pub selected_method: Option<String>,
  pub input_delivery: Option<String>,
  pub selected_path: Option<String>,
  pub attempts: Option<usize>,
  pub attempts_succeeded: Option<usize>,
  pub operation_status: Option<String>,
  pub verification: Option<String>,
  pub closure_state: CandidateActionExecutionClosureState,
  pub semantic_matched: Option<bool>,
  pub readiness: Option<String>,
  pub readiness_blocker: Option<String>,
  pub consent_id: Option<String>,
  pub consent_granted_by: Option<String>,
  pub consent_provenance: Option<String>,
  pub consent_grade: Option<String>,
  pub side_effect: Option<String>,
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

pub(crate) fn list_candidate_action_decision_lineage(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<CandidateActionDecisionLineage>> {
  let run = store.read_run(run_id)?;
  extract_candidate_action_decision_lineage(store, &run)
}

pub(crate) fn list_candidate_action_execution_lineage(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<CandidateActionExecutionLineage>> {
  let run = store.read_run(run_id)?;
  extract_candidate_action_execution_lineage(store, &run)
}

pub(crate) fn list_minecraft_projection_artifacts(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<MinecraftProjectionArtifact>> {
  let run = store.read_run(run_id)?;
  extract_minecraft_projection_artifacts(store, &run)
}

pub(crate) fn extract_minecraft_projection_artifacts(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<MinecraftProjectionArtifact>> {
  let mut artifacts = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::contract::MINECRAFT_PROJECTION_ARTIFACT_ROLE
      || !is_json_mime(&artifact.mime_type)
    {
      continue;
    }

    let parsed = read_artifact_json::<MinecraftProjectionArtifact>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::contract::MINECRAFT_PROJECTION_ARTIFACT_ROLE,
    );

    if let Ok(projection) = parsed {
      artifacts.push(projection);
    }
  }
  Ok(artifacts)
}

pub(crate) fn list_minecraft_telemetry_sample_artifacts(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<MinecraftTelemetrySampleArtifactLineage>> {
  let run = store.read_run(run_id)?;
  extract_minecraft_telemetry_sample_artifacts(store, &run)
}

pub(crate) fn list_minecraft_spatial_bundle_manifests(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<MinecraftSpatialBundleManifestLineage>> {
  let run = store.read_run(run_id)?;
  extract_minecraft_spatial_bundle_manifests(store, &run)
}

pub(crate) fn list_minecraft_training_launch_manifests(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<MinecraftTrainingLaunchManifestLineage>> {
  let run = store.read_run(run_id)?;
  extract_minecraft_training_launch_manifests(store, &run)
}

pub(crate) fn list_minecraft_training_launch_inspect_reports(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<MinecraftTrainingLaunchInspectReportLineage>> {
  let run = store.read_run(run_id)?;
  extract_minecraft_training_launch_inspect_reports(store, &run)
}

pub(crate) fn list_minecraft_training_job_manifests(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<MinecraftTrainingJobManifestLineage>> {
  let run = store.read_run(run_id)?;
  extract_minecraft_training_job_manifests(store, &run)
}

pub(crate) fn list_minecraft_training_job_inspect_reports(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<MinecraftTrainingJobInspectReportLineage>> {
  let run = store.read_run(run_id)?;
  extract_minecraft_training_job_inspect_reports(store, &run)
}

pub(crate) fn list_minecraft_training_result_manifests(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<MinecraftTrainingResultManifestLineage>> {
  let run = store.read_run(run_id)?;
  extract_minecraft_training_result_manifests(store, &run)
}

pub(crate) fn list_minecraft_training_result_inspect_reports(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<MinecraftTrainingResultInspectReportLineage>> {
  let run = store.read_run(run_id)?;
  extract_minecraft_training_result_inspect_reports(store, &run)
}

pub(crate) fn list_minecraft_training_result_artifact_fetch_manifests(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<MinecraftTrainingResultArtifactFetchManifestLineage>> {
  let run = store.read_run(run_id)?;
  extract_minecraft_training_result_artifact_fetch_manifests(store, &run)
}

pub(crate) fn list_minecraft_training_result_artifact_fetch_inspect_reports(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<MinecraftTrainingResultArtifactFetchInspectReportLineage>> {
  let run = store.read_run(run_id)?;
  extract_minecraft_training_result_artifact_fetch_inspect_reports(store, &run)
}

pub(crate) fn list_minecraft_training_package_manifests(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<MinecraftTrainingPackageManifestLineage>> {
  let run = store.read_run(run_id)?;
  extract_minecraft_training_package_manifests(store, &run)
}

pub(crate) fn list_minecraft_training_package_inspect_reports(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<MinecraftTrainingPackageInspectReportLineage>> {
  let run = store.read_run(run_id)?;
  extract_minecraft_training_package_inspect_reports(store, &run)
}

pub(crate) fn extract_minecraft_spatial_bundle_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<MinecraftSpatialBundleManifestLineage>> {
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::minecraft::MINECRAFT_SPATIAL_BUNDLE_ARTIFACT_ROLE {
      continue;
    }

    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(MinecraftSpatialBundleManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!(
          "minecraft spatial bundle artifact mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }

    let parsed = read_artifact_json::<MinecraftSpatialBundleManifestSummary>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::minecraft::MINECRAFT_SPATIAL_BUNDLE_ARTIFACT_ROLE,
    );

    match parsed {
      Ok(manifest) => manifests.push(MinecraftSpatialBundleManifestLineage {
        artifact: artifact_ref,
        manifest: Some(manifest),
        issue: None,
      }),
      Err(error) => manifests.push(MinecraftSpatialBundleManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(error),
      }),
    }
  }
  Ok(manifests)
}

pub(crate) fn extract_minecraft_training_launch_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<MinecraftTrainingLaunchManifestLineage>> {
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::minecraft::MINECRAFT_3DGS_TRAINING_LAUNCH_PLAN_ARTIFACT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(MinecraftTrainingLaunchManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!(
          "minecraft training launch artifact mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingLaunchPlanManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_LAUNCH_PLAN_ARTIFACT_ROLE,
    )
    .map(MinecraftTrainingLaunchManifestSummary::from);
    match parsed {
      Ok(manifest) => manifests.push(MinecraftTrainingLaunchManifestLineage {
        artifact: artifact_ref,
        manifest: Some(manifest),
        issue: None,
      }),
      Err(error) => manifests.push(MinecraftTrainingLaunchManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(error),
      }),
    }
  }
  Ok(manifests)
}

pub(crate) fn extract_minecraft_training_launch_inspect_reports(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<MinecraftTrainingLaunchInspectReportLineage>> {
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::minecraft::MINECRAFT_3DGS_TRAINING_LAUNCH_INSPECT_ARTIFACT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(MinecraftTrainingLaunchInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!(
          "minecraft training launch inspect artifact mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingLaunchInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_LAUNCH_INSPECT_ARTIFACT_ROLE,
    )
    .map(MinecraftTrainingLaunchInspectReportSummary::from);
    match parsed {
      Ok(report) => reports.push(MinecraftTrainingLaunchInspectReportLineage {
        artifact: artifact_ref,
        report: Some(report),
        issue: None,
      }),
      Err(error) => reports.push(MinecraftTrainingLaunchInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(error),
      }),
    }
  }
  Ok(reports)
}

pub(crate) fn extract_minecraft_training_job_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<MinecraftTrainingJobManifestLineage>> {
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::minecraft::MINECRAFT_3DGS_TRAINING_JOB_ARTIFACT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(MinecraftTrainingJobManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!(
          "minecraft training job artifact mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingLaunchJobManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_JOB_ARTIFACT_ROLE,
    )
    .map(MinecraftTrainingJobManifestSummary::from);
    match parsed {
      Ok(manifest) => manifests.push(MinecraftTrainingJobManifestLineage {
        artifact: artifact_ref,
        manifest: Some(manifest),
        issue: None,
      }),
      Err(error) => manifests.push(MinecraftTrainingJobManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(error),
      }),
    }
  }
  Ok(manifests)
}

pub(crate) fn extract_minecraft_training_job_inspect_reports(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<MinecraftTrainingJobInspectReportLineage>> {
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::minecraft::MINECRAFT_3DGS_TRAINING_JOB_INSPECT_ARTIFACT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(MinecraftTrainingJobInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!(
          "minecraft training job inspect artifact mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingLaunchJobInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_JOB_INSPECT_ARTIFACT_ROLE,
    )
    .map(MinecraftTrainingJobInspectReportSummary::from);
    match parsed {
      Ok(report) => reports.push(MinecraftTrainingJobInspectReportLineage {
        artifact: artifact_ref,
        report: Some(report),
        issue: None,
      }),
      Err(error) => reports.push(MinecraftTrainingJobInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(error),
      }),
    }
  }
  Ok(reports)
}

pub(crate) fn extract_minecraft_training_result_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<MinecraftTrainingResultManifestLineage>> {
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(MinecraftTrainingResultManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!(
          "minecraft training result artifact mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingResultManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_ROLE,
    )
    .map(MinecraftTrainingResultManifestSummary::from);
    match parsed {
      Ok(manifest) => manifests.push(MinecraftTrainingResultManifestLineage {
        artifact: artifact_ref,
        manifest: Some(manifest),
        issue: None,
      }),
      Err(error) => manifests.push(MinecraftTrainingResultManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(error),
      }),
    }
  }
  Ok(manifests)
}

pub(crate) fn extract_minecraft_training_result_inspect_reports(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<MinecraftTrainingResultInspectReportLineage>> {
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_INSPECT_ARTIFACT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(MinecraftTrainingResultInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!(
          "minecraft training result inspect artifact mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingResultInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_INSPECT_ARTIFACT_ROLE,
    )
    .map(MinecraftTrainingResultInspectReportSummary::from);
    match parsed {
      Ok(report) => reports.push(MinecraftTrainingResultInspectReportLineage {
        artifact: artifact_ref,
        report: Some(report),
        issue: None,
      }),
      Err(error) => reports.push(MinecraftTrainingResultInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(error),
      }),
    }
  }
  Ok(reports)
}

pub(crate) fn extract_minecraft_training_result_artifact_fetch_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<MinecraftTrainingResultArtifactFetchManifestLineage>> {
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_MANIFEST_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(MinecraftTrainingResultArtifactFetchManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!(
          "minecraft training result artifact fetch manifest mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingResultArtifactFetchManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_MANIFEST_ROLE,
    )
    .map(MinecraftTrainingResultArtifactFetchManifestSummary::from);
    match parsed {
      Ok(manifest) => manifests.push(MinecraftTrainingResultArtifactFetchManifestLineage {
        artifact: artifact_ref,
        manifest: Some(manifest),
        issue: None,
      }),
      Err(error) => manifests.push(MinecraftTrainingResultArtifactFetchManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(error),
      }),
    }
  }
  Ok(manifests)
}

pub(crate) fn extract_minecraft_training_result_artifact_fetch_inspect_reports(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<MinecraftTrainingResultArtifactFetchInspectReportLineage>> {
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_INSPECT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(MinecraftTrainingResultArtifactFetchInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!(
          "minecraft training result artifact fetch inspect mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingResultArtifactFetchInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_INSPECT_ROLE,
    )
    .map(MinecraftTrainingResultArtifactFetchInspectReportSummary::from);
    match parsed {
      Ok(report) => reports.push(MinecraftTrainingResultArtifactFetchInspectReportLineage {
        artifact: artifact_ref,
        report: Some(report),
        issue: None,
      }),
      Err(error) => reports.push(MinecraftTrainingResultArtifactFetchInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(error),
      }),
    }
  }
  Ok(reports)
}

pub(crate) fn extract_minecraft_training_package_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<MinecraftTrainingPackageManifestLineage>> {
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::minecraft::MINECRAFT_3DGS_TRAINING_PACKAGE_ARTIFACT_ROLE {
      continue;
    }

    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(MinecraftTrainingPackageManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!(
          "minecraft training package artifact mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }

    let parsed = read_artifact_json::<TrainingPackageManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_PACKAGE_ARTIFACT_ROLE,
    )
    .map(MinecraftTrainingPackageManifestSummary::from);

    match parsed {
      Ok(manifest) => manifests.push(MinecraftTrainingPackageManifestLineage {
        artifact: artifact_ref,
        manifest: Some(manifest),
        issue: None,
      }),
      Err(error) => manifests.push(MinecraftTrainingPackageManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(error),
      }),
    }
  }
  Ok(manifests)
}

pub(crate) fn extract_minecraft_training_package_inspect_reports(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<MinecraftTrainingPackageInspectReportLineage>> {
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::minecraft::MINECRAFT_3DGS_TRAINING_PACKAGE_INSPECT_ARTIFACT_ROLE {
      continue;
    }

    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(MinecraftTrainingPackageInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!(
          "minecraft training package inspect artifact mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }

    let parsed = read_artifact_json::<TrainingPackageInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_PACKAGE_INSPECT_ARTIFACT_ROLE,
    )
    .map(MinecraftTrainingPackageInspectReportSummary::from);

    match parsed {
      Ok(report) => reports.push(MinecraftTrainingPackageInspectReportLineage {
        artifact: artifact_ref,
        report: Some(report),
        issue: None,
      }),
      Err(error) => reports.push(MinecraftTrainingPackageInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(error),
      }),
    }
  }
  Ok(reports)
}

pub(crate) fn extract_minecraft_telemetry_sample_artifacts(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<MinecraftTelemetrySampleArtifactLineage>> {
  let mut artifacts = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::contract::TELEMETRY_SAMPLE_ARTIFACT_ROLE {
      continue;
    }

    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      artifacts.push(MinecraftTelemetrySampleArtifactLineage {
        artifact: artifact_ref,
        line_count: None,
        byte_size: None,
        issue: Some(format!(
          "minecraft telemetry sample artifact mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }

    let parsed = read_telemetry_artifact_summary(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::contract::TELEMETRY_SAMPLE_ARTIFACT_ROLE,
    );

    match parsed {
      Ok((artifact_path, line_count, byte_size)) => {
        artifacts.push(MinecraftTelemetrySampleArtifactLineage {
          artifact: ArtifactRefLineage {
            path: Some(artifact_path.display().to_string()),
            ..artifact_ref
          },
          line_count: Some(line_count),
          byte_size: Some(byte_size),
          issue: None,
        })
      }
      Err(error) => artifacts.push(MinecraftTelemetrySampleArtifactLineage {
        artifact: artifact_ref,
        line_count: None,
        byte_size: None,
        issue: Some(error),
      }),
    }
  }
  Ok(artifacts)
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

    let parsed = read_artifact_json::<RecognitionResult>(
      store,
      run.run.run_id.as_str(),
      artifact,
      DETECTOR_RECOGNITION_ARTIFACT_ROLE,
    );

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
        permission_granted_by: None,
        permission_scope_note: None,
        consent_id: None,
        consent_provenance: None,
        consent_grade: None,
        consent_scope: None,
        consent_approved_action: None,
        consent_recognition_id: None,
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

    let parsed = read_artifact_json::<CandidatePromotionArtifact>(
      store,
      run.run.run_id.as_str(),
      artifact,
      CANDIDATE_PROMOTION_ARTIFACT_ROLE,
    );

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
        permission_granted_by: None,
        permission_scope_note: None,
        consent_id: None,
        consent_provenance: None,
        consent_grade: None,
        consent_scope: None,
        consent_approved_action: None,
        consent_recognition_id: None,
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

pub(crate) fn extract_candidate_action_decision_lineage(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<CandidateActionDecisionLineage>> {
  let mut lineage = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != CANDIDATE_ACTION_DECISION_ARTIFACT_ROLE {
      continue;
    }

    let decision_artifact = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      lineage.push(malformed_candidate_action_decision_lineage(
        decision_artifact,
        format!(
          "candidate-action-decision artifact mime_type {} is not JSON",
          artifact.mime_type
        ),
      ));
      continue;
    }

    let parsed = read_artifact_json::<CandidateActionDecisionArtifact>(
      store,
      run.run.run_id.as_str(),
      artifact,
      CANDIDATE_ACTION_DECISION_ARTIFACT_ROLE,
    );

    match parsed {
      Ok(decision) => lineage.push(candidate_action_decision_lineage_entry(
        run, artifact, decision,
      )),
      Err(error) => lineage.push(malformed_candidate_action_decision_lineage(
        decision_artifact,
        error,
      )),
    }
  }
  Ok(lineage)
}

pub(crate) fn extract_candidate_action_execution_lineage(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<CandidateActionExecutionLineage>> {
  let mut lineage = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != CANDIDATE_ACTION_EXECUTION_ARTIFACT_ROLE {
      continue;
    }

    let execution_artifact = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      lineage.push(malformed_candidate_action_execution_lineage(
        execution_artifact,
        format!(
          "candidate-action-execution artifact mime_type {} is not JSON",
          artifact.mime_type
        ),
      ));
      continue;
    }

    let parsed = read_artifact_json::<CandidateActionExecutionArtifact>(
      store,
      run.run.run_id.as_str(),
      artifact,
      CANDIDATE_ACTION_EXECUTION_ARTIFACT_ROLE,
    );

    match parsed {
      Ok(execution) => lineage.push(candidate_action_execution_lineage_entry(
        run, artifact, execution,
      )),
      Err(error) => lineage.push(malformed_candidate_action_execution_lineage(
        execution_artifact,
        error,
      )),
    }
  }
  Ok(lineage)
}

fn is_json_mime(mime_type: &str) -> bool {
  mime_type == "application/json" || mime_type.ends_with("+json")
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
  let freshness_source_artifact = promotion
    .promotion_context
    .freshness
    .as_ref()
    .and_then(|freshness| freshness.source_artifact.as_ref())
    .map(|reference| resolve_artifact_ref(run, reference));
  let permission = promotion.promotion_context.permission.as_ref();
  let consent = permission.and_then(|permission| permission.consent.as_ref());
  let (status, issue) = classify_candidate_promotion_lineage(
    &promotion,
    source_recognition_artifact.as_ref(),
    capture_artifact.as_ref(),
  );
  let (decision_kind, refusal_reasons, promoted_candidate_local_ids) =
    promotion_decision_summary(&promotion.decision);
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
    freshness_source_operation_id: promotion
      .promotion_context
      .freshness
      .as_ref()
      .and_then(|freshness| freshness.source_operation_id.clone()),
    permission_granted: Some(promotion.promotion_context.permission.is_some()),
    permission_granted_by: permission.map(|permission| permission.granted_by.clone()),
    permission_scope_note: permission.map(|permission| permission.scope_note.clone()),
    consent_id: consent.map(|consent| consent.consent_id.clone()),
    consent_provenance: consent.map(|consent| consent_provenance_string(&consent.provenance)),
    consent_grade: consent.map(|consent| consent_grade_string(&consent.grade)),
    consent_scope: consent.map(|consent| consent_scope_string(&consent.scope)),
    consent_approved_action: consent.map(|consent| consent_action_string(&consent.approved_action)),
    consent_recognition_id: consent.map(|consent| consent.recognition_id.clone()),
    decision_kind: Some(decision_kind),
    refusal_reasons,
    promoted_candidate_local_ids,
    known_limits: promotion.known_limits,
    issue,
  }
}

fn candidate_action_decision_lineage_entry(
  run: &CanonicalRun,
  artifact: &ArtifactRecordV1Alpha1,
  decision: CandidateActionDecisionArtifact,
) -> CandidateActionDecisionLineage {
  let source_candidate_promotion_artifact = decision
    .source_candidate_promotion_artifact
    .as_ref()
    .map(|reference| resolve_artifact_ref(run, reference));
  let (status, issue) = classify_candidate_action_decision_lineage(
    &decision,
    source_candidate_promotion_artifact.as_ref(),
  );

  CandidateActionDecisionLineage {
    artifact: artifact_record_lineage(run.run.run_id.clone(), artifact),
    status,
    decision_id: Some(decision.decision_id),
    source_candidate_promotion_artifact,
    source_promotion_id: Some(decision.source_promotion_id),
    candidate_local_id: Some(decision.candidate_local_id),
    resolver_operation: Some(decision.action_resolver_decision.operation),
    selected_method: Some(decision.action_resolver_decision.selected_method),
    primary_method: Some(decision.action_resolver_decision.primary_method),
    fallback_allowed: Some(decision.action_resolver_decision.fallback_allowed),
    fallback_used: Some(decision.action_resolver_decision.fallback_used),
    fallback_reason: decision.action_resolver_decision.fallback_reason,
    policy: Some(decision.action_resolver_decision.policy),
    cursor_disturbance: Some(decision.action_resolver_decision.cursor_disturbance),
    press_mechanism: Some(decision.action_resolver_decision.press_mechanism),
    side_effect: Some(candidate_action_side_effect_string(&decision.side_effect)),
    input_delivery: detail_string(&decision.detail, &["input_delivery"]),
    operation_result: detail_string(&decision.detail, &["operation_result"]),
    verification_result: detail_string(&decision.detail, &["verification_result"]),
    known_limits: decision.known_limits,
    issue,
  }
}

fn malformed_candidate_action_decision_lineage(
  artifact: ArtifactRefLineage,
  issue: String,
) -> CandidateActionDecisionLineage {
  CandidateActionDecisionLineage {
    artifact,
    status: CandidateActionDecisionLineageStatus::Malformed,
    decision_id: None,
    source_candidate_promotion_artifact: None,
    source_promotion_id: None,
    candidate_local_id: None,
    resolver_operation: None,
    selected_method: None,
    primary_method: None,
    fallback_allowed: None,
    fallback_used: None,
    fallback_reason: None,
    policy: None,
    cursor_disturbance: None,
    press_mechanism: None,
    side_effect: None,
    input_delivery: None,
    operation_result: None,
    verification_result: None,
    known_limits: Vec::new(),
    issue: Some(issue),
  }
}

fn candidate_action_execution_lineage_entry(
  run: &CanonicalRun,
  artifact: &ArtifactRecordV1Alpha1,
  execution: CandidateActionExecutionArtifact,
) -> CandidateActionExecutionLineage {
  let source_candidate_action_decision_artifact = Some(resolve_artifact_ref(
    run,
    &execution.source_candidate_action_decision_artifact,
  ));
  let source_candidate_promotion_artifact = execution
    .source_candidate_promotion_artifact
    .as_ref()
    .map(|reference| resolve_artifact_ref(run, reference));
  let operation_result_artifact = execution
    .operation_result_artifact
    .as_ref()
    .map(|reference| resolve_artifact_ref(run, reference));
  let (status, issue) = classify_candidate_action_execution_lineage(
    &execution,
    source_candidate_action_decision_artifact.as_ref(),
    operation_result_artifact.as_ref(),
  );
  let semantic_matched = execution
    .operation_result
    .verifications
    .iter()
    .find(|verification| verification.semantic_matched.is_some())
    .or_else(|| execution.operation_result.verifications.first())
    .and_then(|verification| verification.semantic_matched);
  let closure_state = candidate_action_execution_closure_state(&execution);

  CandidateActionExecutionLineage {
    artifact: artifact_record_lineage(run.run.run_id.clone(), artifact),
    status,
    execution_id: Some(execution.execution_id),
    source_candidate_action_decision_artifact,
    source_candidate_promotion_artifact,
    operation_result_artifact,
    source_promotion_id: Some(execution.source_promotion_id),
    source_decision_id: Some(execution.source_decision_id),
    candidate_local_id: Some(execution.candidate_local_id),
    resolver_operation: Some(execution.action_resolver_decision.operation),
    selected_method: Some(execution.action_resolver_decision.selected_method),
    input_delivery: detail_string(&execution.detail, &["input_delivery"]),
    selected_path: detail_string(&execution.detail, &["selected_path"]),
    attempts: execution
      .detail
      .get("attempt_count")
      .and_then(|value| value.as_u64().and_then(|count| usize::try_from(count).ok())),
    attempts_succeeded: execution
      .detail
      .get("attempts_succeeded")
      .and_then(|value| value.as_u64().and_then(|count| usize::try_from(count).ok())),
    operation_status: detail_string(&execution.detail, &["operation_status"]),
    verification: detail_string(&execution.detail, &["verification"]),
    closure_state,
    semantic_matched,
    readiness: detail_string(&execution.detail, &["readiness"]),
    readiness_blocker: detail_string(&execution.detail, &["readiness_blocker"]),
    consent_id: Some(execution.consent.consent_id),
    consent_granted_by: Some(execution.consent.granted_by),
    consent_provenance: Some(consent_provenance_string(&execution.consent.provenance)),
    consent_grade: Some(consent_grade_string(&execution.consent.grade)),
    side_effect: Some(candidate_action_execution_side_effect_string(
      &execution.side_effect,
    )),
    known_limits: execution.known_limits,
    issue,
  }
}

fn malformed_candidate_action_execution_lineage(
  artifact: ArtifactRefLineage,
  issue: String,
) -> CandidateActionExecutionLineage {
  CandidateActionExecutionLineage {
    artifact,
    status: CandidateActionExecutionLineageStatus::Malformed,
    execution_id: None,
    source_candidate_action_decision_artifact: None,
    source_candidate_promotion_artifact: None,
    operation_result_artifact: None,
    source_promotion_id: None,
    source_decision_id: None,
    candidate_local_id: None,
    resolver_operation: None,
    selected_method: None,
    input_delivery: None,
    selected_path: None,
    attempts: None,
    attempts_succeeded: None,
    operation_status: None,
    verification: None,
    closure_state: CandidateActionExecutionClosureState::SemanticOpen,
    semantic_matched: None,
    readiness: None,
    readiness_blocker: None,
    consent_id: None,
    consent_granted_by: None,
    consent_provenance: None,
    consent_grade: None,
    side_effect: None,
    known_limits: Vec::new(),
    issue: Some(issue),
  }
}

fn candidate_action_execution_closure_state(
  execution: &CandidateActionExecutionArtifact,
) -> CandidateActionExecutionClosureState {
  if execution
    .detail
    .get("readiness")
    .and_then(|value| value.as_str())
    == Some("not_ready")
  {
    return CandidateActionExecutionClosureState::BlockedByReadiness;
  }

  let semantic_matched = execution
    .operation_result
    .verifications
    .iter()
    .find(|verification| verification.semantic_matched.is_some())
    .or_else(|| execution.operation_result.verifications.first())
    .and_then(|verification| verification.semantic_matched);

  match semantic_matched {
    Some(false) | None => CandidateActionExecutionClosureState::SemanticOpen,
    Some(true) => CandidateActionExecutionClosureState::EvidenceClosed,
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

fn classify_candidate_action_decision_lineage(
  decision: &CandidateActionDecisionArtifact,
  source_candidate_promotion_artifact: Option<&ArtifactRefLineage>,
) -> (CandidateActionDecisionLineageStatus, Option<String>) {
  if decision.source_candidate_promotion_artifact.is_none() {
    return (
      CandidateActionDecisionLineageStatus::MissingSourceCandidatePromotionArtifact,
      Some("source_candidate_promotion_artifact is missing".to_string()),
    );
  }
  if let Some(source_candidate_promotion_artifact) = source_candidate_promotion_artifact
    && !source_candidate_promotion_artifact.resolved
  {
    return (
      CandidateActionDecisionLineageStatus::SourceCandidatePromotionArtifactUnresolved,
      Some(
        "source_candidate_promotion_artifact could not be resolved from recorded run artifacts"
          .to_string(),
      ),
    );
  }
  (CandidateActionDecisionLineageStatus::Ready, None)
}

fn classify_candidate_action_execution_lineage(
  execution: &CandidateActionExecutionArtifact,
  source_candidate_action_decision_artifact: Option<&ArtifactRefLineage>,
  operation_result_artifact: Option<&ArtifactRefLineage>,
) -> (CandidateActionExecutionLineageStatus, Option<String>) {
  if !source_candidate_action_decision_artifact.is_some_and(|artifact| artifact.resolved) {
    return (
      CandidateActionExecutionLineageStatus::SourceCandidateActionDecisionArtifactUnresolved,
      Some(
        "source_candidate_action_decision_artifact could not be resolved from recorded run artifacts"
          .to_string(),
      ),
    );
  }
  if execution.operation_result_artifact.is_none() {
    return (
      CandidateActionExecutionLineageStatus::MissingOperationResultArtifact,
      Some("operation_result_artifact is missing".to_string()),
    );
  }
  if let Some(operation_result_artifact) = operation_result_artifact
    && !operation_result_artifact.resolved
  {
    return (
      CandidateActionExecutionLineageStatus::OperationResultArtifactUnresolved,
      Some(
        "operation_result_artifact could not be resolved from recorded run artifacts".to_string(),
      ),
    );
  }
  if execution
    .detail
    .get("readiness")
    .and_then(|value| value.as_str())
    == Some("not_ready")
  {
    return (
      CandidateActionExecutionLineageStatus::BlockedNotReady,
      execution
        .detail
        .get("readiness_blocker")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .or_else(|| Some("candidate action execution blocked by readiness".to_string())),
    );
  }
  (CandidateActionExecutionLineageStatus::Ready, None)
}

fn artifact_record_lineage(
  run_id: auv_tracing_driver::trace::RunId,
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

fn consent_scope_string(scope: &crate::candidate_promotion::ConsentScope) -> String {
  match scope {
    crate::candidate_promotion::ConsentScope::CandidatePromotionOnly => {
      "candidate_promotion_only".to_string()
    }
  }
}

fn consent_provenance_string(provenance: &crate::candidate_promotion::ConsentProvenance) -> String {
  match provenance {
    crate::candidate_promotion::ConsentProvenance::DevSelfMinted => "dev_self_minted".to_string(),
    crate::candidate_promotion::ConsentProvenance::HumanGesture => "human_gesture".to_string(),
  }
}

fn consent_grade_string(grade: &crate::candidate_promotion::ConsentGrade) -> String {
  match grade {
    crate::candidate_promotion::ConsentGrade::DevOnly => "dev_only".to_string(),
    crate::candidate_promotion::ConsentGrade::HumanApproved => "human_approved".to_string(),
  }
}

fn consent_action_string(action: &crate::candidate_promotion::ConsentAction) -> String {
  match action {
    crate::candidate_promotion::ConsentAction::PromoteRecognitionToCandidate => {
      "promote_recognition_to_candidate".to_string()
    }
  }
}

fn candidate_action_side_effect_string(
  side_effect: &crate::candidate_action_decision::CandidateActionSideEffect,
) -> String {
  match side_effect {
    crate::candidate_action_decision::CandidateActionSideEffect::NoneDecideOnly => {
      "none_decide_only".to_string()
    }
  }
}

fn candidate_action_execution_side_effect_string(
  side_effect: &crate::candidate_action_decision::CandidateActionExecutionSideEffect,
) -> String {
  match side_effect {
    crate::candidate_action_decision::CandidateActionExecutionSideEffect::SingleInputDelivered => {
      "single_input_delivered".to_string()
    }
    crate::candidate_action_decision::CandidateActionExecutionSideEffect::BlockedNotReady => {
      "blocked_not_ready".to_string()
    }
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
    PromotionRefusal::PermissionMissing => "permission_missing".to_string(),
  }
}

fn read_artifact_json<T: DeserializeOwned>(
  store: &LocalStore,
  run_id: &str,
  artifact: &ArtifactRecordV1Alpha1,
  artifact_role: &str,
) -> AuvResult<T> {
  let (file, artifact_path) = open_artifact_file(store, run_id, artifact, artifact_role)?;
  serde_json::from_reader(BufReader::new(file)).map_err(|error| {
    format!(
      "failed to parse {artifact_role} artifact {} for run {run_id} from {}: {error}",
      artifact.artifact_id,
      artifact_path.display()
    )
  })
}

fn open_artifact_file(
  store: &LocalStore,
  run_id: &str,
  artifact: &ArtifactRecordV1Alpha1,
  artifact_role: &str,
) -> AuvResult<(fs::File, PathBuf)> {
  let (_, artifact_path) = store.artifact_file_scoped(
    run_id,
    artifact.artifact_id.as_str(),
    Some(artifact.span_id.as_str()),
  )?;
  let file = fs::File::open(&artifact_path).map_err(|error| {
    format!(
      "failed to open {artifact_role} artifact {} for run {run_id} from {}: {error}",
      artifact.artifact_id,
      artifact_path.display()
    )
  })?;
  Ok((file, artifact_path))
}

fn read_telemetry_artifact_summary(
  store: &LocalStore,
  run_id: &str,
  artifact: &ArtifactRecordV1Alpha1,
  artifact_role: &str,
) -> AuvResult<(PathBuf, usize, u64)> {
  let (_, artifact_path) = store.artifact_file_scoped(
    run_id,
    artifact.artifact_id.as_str(),
    Some(artifact.span_id.as_str()),
  )?;
  let metadata = fs::metadata(&artifact_path).map_err(|error| {
    format!(
      "failed to stat {artifact_role} artifact {} for run {run_id} from {}: {error}",
      artifact.artifact_id,
      artifact_path.display()
    )
  })?;
  let (file, _) = open_artifact_file(store, run_id, artifact, artifact_role)?;
  let line_count = BufReader::new(file)
    .lines()
    .try_fold(0usize, |count, line| {
      let line = line.map_err(|error| {
        format!(
          "failed to read {artifact_role} artifact {} for run {run_id} from {}: {error}",
          artifact.artifact_id,
          artifact_path.display()
        )
      })?;
      Ok::<_, String>(count + usize::from(!line.trim().is_empty()))
    })?;
  Ok((artifact_path, line_count, metadata.len()))
}

impl From<TrainingPackageManifest> for MinecraftTrainingPackageManifestSummary {
  fn from(value: TrainingPackageManifest) -> Self {
    Self {
      schema_version: value.schema_version,
      source_scene_packet_manifest_path: value.source_scene_packet_manifest_path,
      source_bundle_manifest_paths: value.source_bundle_manifest_paths,
      source_run_ids: value.source_run_ids,
      counts: value.counts,
      compatibility_views: value.compatibility_views,
      known_limits: value.known_limits,
    }
  }
}

impl From<TrainingLaunchPlanManifest> for MinecraftTrainingLaunchManifestSummary {
  fn from(value: TrainingLaunchPlanManifest) -> Self {
    Self {
      schema_version: value.schema_version,
      source_training_package_manifest_path: value.source_training_package_manifest_path,
      source_training_package_inspect_report_path: value
        .source_training_package_inspect_report_path,
      source_scene_packet_manifest_path: value.source_scene_packet_manifest_path,
      source_bundle_manifest_paths: value.source_bundle_manifest_paths,
      source_run_ids: value.source_run_ids,
      counts: value.counts,
      compatibility_view_name: value.compatibility_view_name,
      trainer_backend: value.trainer_backend,
      training_data_dir: value.training_data_dir,
      transforms_path: value.transforms_path,
      export_report_path: value.export_report_path,
      suggested_output_dir: value.suggested_output_dir,
      launch_command: value.launch_command,
      known_limits: value.known_limits,
    }
  }
}

impl From<TrainingLaunchInspectReport> for MinecraftTrainingLaunchInspectReportSummary {
  fn from(value: TrainingLaunchInspectReport) -> Self {
    Self {
      schema_version: value.schema_version,
      training_launch_manifest_path: value.training_launch_manifest_path,
      source_training_package_manifest_path: value.source_training_package_manifest_path,
      source_scene_packet_manifest_path: value.source_scene_packet_manifest_path,
      source_bundle_manifest_paths: value.source_bundle_manifest_paths,
      source_run_ids: value.source_run_ids,
      compatibility_status: format!("{:?}", value.compatibility_status),
      trainer_readiness: format!("{:?}", value.trainer_readiness),
      readiness_blocker: value
        .readiness_blocker
        .map(|blocker| format!("{blocker:?}")),
      probe_command: value.probe_command,
      probe_succeeded: value.probe_succeeded,
      exported_frame_count: value.exported_frame_count,
      skipped_frame_count: value.skipped_frame_count,
      transforms_present: value.transforms_present,
      warnings: value.warnings,
      known_limits: value.known_limits,
    }
  }
}

impl From<TrainingLaunchJobManifest> for MinecraftTrainingJobManifestSummary {
  fn from(value: TrainingLaunchJobManifest) -> Self {
    let counts = TrainingPackageCounts {
      frames: value.counts.frames,
      images: value.counts.images,
      compatibility_exported_frames: value.counts.compatibility_exported_frames,
      compatibility_skipped_frames: value.counts.compatibility_skipped_frames,
    };
    Self {
      schema_version: value.schema_version,
      source_training_launch_plan_path: value.source_training_launch_plan_path,
      source_training_package_manifest_path: value.source_training_package_manifest_path,
      source_training_package_inspect_report_path: value
        .source_training_package_inspect_report_path,
      source_scene_packet_manifest_path: value.source_scene_packet_manifest_path,
      source_bundle_manifest_paths: value.source_bundle_manifest_paths,
      source_run_ids: value.source_run_ids,
      counts,
      compatibility_view_name: value.compatibility_view_name,
      provider_backend: value.provider_backend,
      trainer_backend: value.trainer_backend,
      job_backend: value.job_backend,
      job_submission_endpoint: value.job_submission_endpoint,
      job_submission_command: value.job_submission_command,
      training_data_dir: value.training_data_dir,
      transforms_path: value.transforms_path,
      export_report_path: value.export_report_path,
      suggested_output_dir: value.suggested_output_dir,
      launch_command: value.launch_command,
      status: value.status.as_str().to_string(),
      job_id: value.job_id,
      job_url: value.job_url,
      readiness_blocker: value
        .readiness_blocker
        .map(|blocker| format!("{blocker:?}")),
      known_limits: value.known_limits,
    }
  }
}

impl From<TrainingLaunchJobInspectReport> for MinecraftTrainingJobInspectReportSummary {
  fn from(value: TrainingLaunchJobInspectReport) -> Self {
    Self {
      schema_version: value.schema_version,
      training_launch_manifest_path: value.training_launch_manifest_path,
      source_training_launch_plan_path: value.source_training_launch_plan_path,
      source_training_package_manifest_path: value.source_training_package_manifest_path,
      source_scene_packet_manifest_path: value.source_scene_packet_manifest_path,
      source_bundle_manifest_paths: value.source_bundle_manifest_paths,
      source_run_ids: value.source_run_ids,
      provider_backend: value.provider_backend,
      job_backend: value.job_backend,
      trainer_backend: value.trainer_backend,
      job_submission_endpoint: value.job_submission_endpoint,
      job_submission_command: value.job_submission_command,
      status: value.status.as_str().to_string(),
      job_id: value.job_id,
      job_url: value.job_url,
      readiness_blocker: value
        .readiness_blocker
        .map(|blocker| format!("{blocker:?}")),
      probe_command: value.probe_command,
      probe_succeeded: value.probe_succeeded,
      exported_frame_count: value.exported_frame_count,
      skipped_frame_count: value.skipped_frame_count,
      transforms_present: value.transforms_present,
      warnings: value.warnings,
      known_limits: value.known_limits,
    }
  }
}

impl From<TrainingResultManifest> for MinecraftTrainingResultManifestSummary {
  fn from(value: TrainingResultManifest) -> Self {
    Self {
      schema_version: value.schema_version,
      source_training_job_manifest_path: value.source_training_job_manifest_path,
      source_training_launch_plan_path: value.source_training_launch_plan_path,
      source_training_package_manifest_path: value.source_training_package_manifest_path,
      source_scene_packet_manifest_path: value.source_scene_packet_manifest_path,
      source_bundle_manifest_paths: value.source_bundle_manifest_paths,
      source_run_ids: value.source_run_ids,
      trainer_backend: value.trainer_backend,
      job_backend: value.job_backend,
      job_submission_endpoint: value.job_submission_endpoint,
      source_job_status: value.source_job_status.as_str().to_string(),
      status: value.status.as_str().to_string(),
      job_id: value.job_id,
      job_url: value.job_url,
      result_dir: value.result_dir,
      exported_frame_count: value.exported_frame_count,
      skipped_frame_count: value.skipped_frame_count,
      result_artifacts: value
        .result_artifacts
        .into_iter()
        .map(MinecraftTrainingResultArtifactSummary::from)
        .collect(),
      known_limits: value.known_limits,
    }
  }
}

impl From<TrainingResultInspectReport> for MinecraftTrainingResultInspectReportSummary {
  fn from(value: TrainingResultInspectReport) -> Self {
    Self {
      schema_version: value.schema_version,
      training_result_manifest_path: value.training_result_manifest_path,
      source_training_job_manifest_path: value.source_training_job_manifest_path,
      source_training_launch_plan_path: value.source_training_launch_plan_path,
      source_scene_packet_manifest_path: value.source_scene_packet_manifest_path,
      source_bundle_manifest_paths: value.source_bundle_manifest_paths,
      source_run_ids: value.source_run_ids,
      trainer_backend: value.trainer_backend,
      job_backend: value.job_backend,
      job_submission_endpoint: value.job_submission_endpoint,
      source_job_status: value.source_job_status.as_str().to_string(),
      status: value.status.as_str().to_string(),
      status_reason: value
        .status_reason
        .map(|reason| reason.as_str().to_string()),
      job_id: value.job_id,
      job_url: value.job_url,
      result_dir: value.result_dir,
      result_dir_exists: value.result_dir_exists,
      key_result_artifacts_present: value.key_result_artifacts_present,
      result_artifact_count: value.result_artifact_count,
      warnings: value.warnings,
      known_limits: value.known_limits,
    }
  }
}

impl From<TrainingResultArtifactFetchManifest>
  for MinecraftTrainingResultArtifactFetchManifestSummary
{
  fn from(value: TrainingResultArtifactFetchManifest) -> Self {
    Self {
      schema_version: value.schema_version,
      source_training_result_manifest_path: value.source_training_result_manifest_path,
      source_training_job_manifest_path: value.source_training_job_manifest_path,
      source_training_launch_plan_path: value.source_training_launch_plan_path,
      source_training_package_manifest_path: value.source_training_package_manifest_path,
      source_scene_packet_manifest_path: value.source_scene_packet_manifest_path,
      source_bundle_manifest_paths: value.source_bundle_manifest_paths,
      source_run_ids: value.source_run_ids,
      trainer_backend: value.trainer_backend,
      job_backend: value.job_backend,
      source_job_status: value.source_job_status.as_str().to_string(),
      source_result_status: value.source_result_status.as_str().to_string(),
      source_result_status_reason: value.source_result_status_reason,
      source_result_dir: value.source_result_dir,
      normalized_result_dir: value.normalized_result_dir,
      normalized_artifacts: value
        .normalized_artifacts
        .into_iter()
        .map(MinecraftTrainingResultNormalizedArtifactSummary::from)
        .collect(),
      known_limits: value.known_limits,
    }
  }
}

impl From<TrainingResultArtifactFetchInspectReport>
  for MinecraftTrainingResultArtifactFetchInspectReportSummary
{
  fn from(value: TrainingResultArtifactFetchInspectReport) -> Self {
    Self {
      schema_version: value.schema_version,
      training_result_artifact_fetch_manifest_path: value
        .training_result_artifact_fetch_manifest_path,
      source_training_result_manifest_path: value.source_training_result_manifest_path,
      source_training_job_manifest_path: value.source_training_job_manifest_path,
      source_training_launch_plan_path: value.source_training_launch_plan_path,
      source_scene_packet_manifest_path: value.source_scene_packet_manifest_path,
      source_bundle_manifest_paths: value.source_bundle_manifest_paths,
      source_run_ids: value.source_run_ids,
      trainer_backend: value.trainer_backend,
      job_backend: value.job_backend,
      source_job_status: value.source_job_status.as_str().to_string(),
      source_result_status: value.source_result_status.as_str().to_string(),
      source_result_status_reason: value.source_result_status_reason,
      fetch_status: value.fetch_status.as_str().to_string(),
      fetch_reason: value.fetch_reason.map(|reason| reason.as_str().to_string()),
      source_result_dir: value.source_result_dir,
      normalized_result_dir: value.normalized_result_dir,
      source_result_dir_exists: value.source_result_dir_exists,
      required_artifacts_present: value.required_artifacts_present,
      normalized_artifact_count: value.normalized_artifact_count,
      warnings: value.warnings,
      known_limits: value.known_limits,
    }
  }
}

impl From<auv_game_minecraft::TrainingResultNormalizedArtifactRecord>
  for MinecraftTrainingResultNormalizedArtifactSummary
{
  fn from(value: auv_game_minecraft::TrainingResultNormalizedArtifactRecord) -> Self {
    Self {
      kind: value.kind.as_str().to_string(),
      relative_path: value.relative_path,
      absolute_path: value.absolute_path,
      readable: value.readable,
      byte_size: value.byte_size,
    }
  }
}

impl From<TrainingPackageInspectReport> for MinecraftTrainingPackageInspectReportSummary {
  fn from(value: TrainingPackageInspectReport) -> Self {
    Self {
      schema_version: value.schema_version,
      training_package_manifest_path: value.training_package_manifest_path,
      scene_packet_manifest_path: value.scene_packet_manifest_path,
      source_bundle_manifest_paths: value.source_bundle_manifest_paths,
      source_run_ids: value.source_run_ids,
      counts: value.counts,
      compatibility_views: value.compatibility_views,
      warnings: value.warnings,
      known_limits: value.known_limits,
    }
  }
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
    CANDIDATE_ACTION_DECISION_ARTIFACT_ROLE, CANDIDATE_ACTION_EXECUTION_ARTIFACT_ROLE,
    CANDIDATE_PROMOTION_ARTIFACT_ROLE, CandidateActionDecisionLineageStatus,
    CandidateActionExecutionClosureState, CandidateActionExecutionLineageStatus,
    CandidatePromotionLineageStatus, DETECTOR_RECOGNITION_ARTIFACT_ROLE,
    DetectorRecognitionLineageStatus, MinecraftSpatialBundleManifestSummary,
    extract_candidate_action_decision_lineage, extract_candidate_action_execution_lineage,
    extract_candidate_promotion_lineage, extract_detector_recognition_lineage,
    extract_minecraft_training_job_inspect_reports, extract_minecraft_training_job_manifests,
    extract_minecraft_training_launch_inspect_reports, extract_minecraft_training_launch_manifests,
    extract_minecraft_training_package_inspect_reports,
    extract_minecraft_training_package_manifests,
    extract_minecraft_training_result_artifact_fetch_inspect_reports,
    extract_minecraft_training_result_artifact_fetch_manifests,
    extract_minecraft_training_result_inspect_reports, extract_minecraft_training_result_manifests,
    extract_observation_snapshots, extract_verifications, list_candidate_action_decision_lineage,
    list_candidate_action_execution_lineage, list_candidate_promotion_lineage,
    list_detector_recognition_lineage, list_minecraft_spatial_bundle_manifests,
    list_minecraft_training_job_inspect_reports, list_minecraft_training_job_manifests,
    list_minecraft_training_launch_inspect_reports, list_minecraft_training_launch_manifests,
    list_minecraft_training_package_inspect_reports, list_minecraft_training_package_manifests,
    list_minecraft_training_result_artifact_fetch_inspect_reports,
    list_minecraft_training_result_artifact_fetch_manifests,
    list_minecraft_training_result_inspect_reports, list_minecraft_training_result_manifests,
    list_observation_snapshots, list_verifications,
  };
  use crate::action_resolver_decision::{ActionResolverDecision, ActionResolverDecisionInput};
  use crate::candidate_action_decision::{
    CandidateActionDecisionArtifact, CandidateActionExecutionArtifact,
    CandidateActionExecutionConsent, CandidateActionExecutionConsentAction,
    CandidateActionExecutionSideEffect, CandidateActionSideEffect,
  };
  use crate::candidate_promotion::{
    ActionConsentRecord, ActionPermission, CandidatePromotion, ConsentAction, ConsentScope,
    PromotionContext, PromotionProjection, PromotionRefusal, StabilityInput,
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
  use auv_game_minecraft::dataset::{SourceRunSummary, SpatialBundleCounts};
  use auv_game_minecraft::{
    TrainingCompatibilityStatus, TrainingCompatibilityViewReport, TrainingLaunchInspectReport,
    TrainingLaunchJobBlocker, TrainingLaunchJobCounts, TrainingLaunchJobInspectReport,
    TrainingLaunchJobManifest, TrainingLaunchJobStatus, TrainingLaunchPlanManifest,
    TrainingLaunchReadiness, TrainingLaunchReadinessBlocker, TrainingPackageCounts,
    TrainingPackageInspectReport, TrainingPackageManifest,
    TrainingResultArtifactFetchInspectReport, TrainingResultArtifactFetchManifest,
    TrainingResultArtifactFetchReason, TrainingResultArtifactFetchStatus,
    TrainingResultArtifactRecord, TrainingResultInspectReport, TrainingResultManifest,
    TrainingResultNormalizedArtifactKind, TrainingResultReason, TrainingResultStatus,
  };
  use auv_tracing_driver::ArtifactFileSource;
  use auv_tracing_driver::store::{CanonicalRun, LocalStore};
  use auv_tracing_driver::trace::{
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

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn training_result_artifact_fetch_manifest_extracts_normalized_artifact_rows() {
    let root = temp_dir("run-read-mc7-d11-fetch-manifest");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_mc7_d11_fetch_manifest");
    let span = dummy_span(&run.root_span_id);

    let manifest = TrainingResultArtifactFetchManifest {
      schema_version: 1,
      generated_at_millis: 1,
      source_training_result_manifest_path: "/tmp/result/minecraft-3dgs-training-result.json"
        .to_string(),
      source_training_job_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json".to_string(),
      source_training_launch_plan_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json"
        .to_string(),
      source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
      source_run_ids: vec!["run-a".to_string()],
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      source_job_status: TrainingResultStatus::Submitted,
      source_result_status: TrainingResultStatus::Succeeded,
      source_result_status_reason: None,
      source_result_dir: "/tmp/result/trainer-output".to_string(),
      normalized_result_dir: "/tmp/result/normalized-result".to_string(),
      normalized_artifacts: vec![
        auv_game_minecraft::TrainingResultNormalizedArtifactRecord {
          kind: TrainingResultNormalizedArtifactKind::Config,
          relative_path: "config.yml".to_string(),
          absolute_path: "/tmp/result/normalized-result/config.yml".to_string(),
          readable: true,
          byte_size: Some(128),
        },
        auv_game_minecraft::TrainingResultNormalizedArtifactRecord {
          kind: TrainingResultNormalizedArtifactKind::ModelsDirectory,
          relative_path: "nerfstudio_models".to_string(),
          absolute_path: "/tmp/result/normalized-result/nerfstudio_models".to_string(),
          readable: true,
          byte_size: None,
        },
        auv_game_minecraft::TrainingResultNormalizedArtifactRecord {
          kind: TrainingResultNormalizedArtifactKind::StatusSnapshot,
          relative_path: "job_status.json".to_string(),
          absolute_path: "/tmp/result/normalized-result/job_status.json".to_string(),
          readable: true,
          byte_size: Some(32),
        },
      ],
      known_limits: vec!["normalized artifacts only".to_string()],
    };

    let artifacts = vec![stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_MANIFEST_ROLE,
      "minecraft-3dgs-training-result-artifact-manifest.json",
      &manifest,
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let canonical = store
      .read_run("run_read_mc7_d11_fetch_manifest")
      .expect("run should read back");

    let extracted = extract_minecraft_training_result_artifact_fetch_manifests(&store, &canonical)
      .expect("manifest should extract");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0]
      .manifest
      .as_ref()
      .expect("summary should be present");
    assert_eq!(summary.normalized_artifacts.len(), 3);
    assert_eq!(summary.normalized_artifacts[0].kind, "config");
    assert_eq!(summary.normalized_artifacts[0].relative_path, "config.yml");
    assert_eq!(
      summary.normalized_artifacts[0].absolute_path,
      "/tmp/result/normalized-result/config.yml"
    );
    assert!(summary.normalized_artifacts[0].readable);
    assert_eq!(summary.normalized_artifacts[0].byte_size, Some(128));
    assert_eq!(summary.normalized_artifacts[1].kind, "models_directory");
    assert_eq!(summary.normalized_artifacts[1].byte_size, None);
    assert_eq!(summary.normalized_artifacts[2].kind, "status_snapshot");

    let listed = list_minecraft_training_result_artifact_fetch_manifests(
      &store,
      "run_read_mc7_d11_fetch_manifest",
    )
    .expect("manifest should list");
    assert_eq!(listed, extracted);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn training_result_artifact_fetch_inspect_extracts_blocked_summary_fields() {
    let root = temp_dir("run-read-mc7-d11-fetch-inspect");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_mc7_d11_fetch_inspect");
    let span = dummy_span(&run.root_span_id);

    let report = TrainingResultArtifactFetchInspectReport {
      schema_version: 1,
      generated_at_millis: 1,
      training_result_artifact_fetch_manifest_path:
        "/tmp/result/minecraft-3dgs-training-result-artifact-manifest.json".to_string(),
      source_training_result_manifest_path: "/tmp/result/minecraft-3dgs-training-result.json"
        .to_string(),
      source_training_job_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json".to_string(),
      source_training_launch_plan_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json"
        .to_string(),
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
      source_run_ids: vec!["run-a".to_string()],
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      source_job_status: TrainingResultStatus::Submitted,
      source_result_status: TrainingResultStatus::Blocked,
      source_result_status_reason: Some(
        TrainingResultReason::RemoteStatusUnavailable
          .as_str()
          .to_string(),
      ),
      fetch_status: TrainingResultArtifactFetchStatus::Blocked,
      fetch_reason: Some(TrainingResultArtifactFetchReason::SourceResultBlocked),
      source_result_dir: "/tmp/result/trainer-output".to_string(),
      normalized_result_dir: "/tmp/result/normalized-result".to_string(),
      source_result_dir_exists: false,
      required_artifacts_present: false,
      normalized_artifact_count: 0,
      warnings: vec!["remote status probe unavailable".to_string()],
      known_limits: vec!["blocked evidence only".to_string()],
    };

    let artifacts = vec![stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_INSPECT_ROLE,
      "minecraft-3dgs-training-result-artifact-inspect.json",
      &report,
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let canonical = store
      .read_run("run_read_mc7_d11_fetch_inspect")
      .expect("run should read back");

    let extracted =
      extract_minecraft_training_result_artifact_fetch_inspect_reports(&store, &canonical)
        .expect("report should extract");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0]
      .report
      .as_ref()
      .expect("summary should be present");
    assert_eq!(summary.fetch_status, "blocked");
    assert_eq!(
      summary.fetch_reason.as_deref(),
      Some("source_result_blocked")
    );
    assert!(!summary.source_result_dir_exists);
    assert!(!summary.required_artifacts_present);
    assert_eq!(summary.normalized_artifact_count, 0);
    assert_eq!(
      summary.source_result_status_reason.as_deref(),
      Some("remote_status_unavailable")
    );

    let listed = list_minecraft_training_result_artifact_fetch_inspect_reports(
      &store,
      "run_read_mc7_d11_fetch_inspect",
    )
    .expect("report should list");
    assert_eq!(listed, extracted);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn training_result_artifact_fetch_extractors_report_json_issues() {
    let root = temp_dir("run-read-mc7-d11-fetch-issues");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_mc7_d11_fetch_issues");
    let span = dummy_span(&run.root_span_id);

    let manifest_artifact = stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_MANIFEST_ROLE,
      "minecraft-3dgs-training-result-artifact-manifest.txt",
      "not json",
    );
    let mut inspect_artifact = stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      1,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_INSPECT_ROLE,
      "minecraft-3dgs-training-result-artifact-inspect.json",
      "{ malformed",
    );
    inspect_artifact.mime_type = "application/json".to_string();
    let artifacts = vec![manifest_artifact, inspect_artifact];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let canonical = store
      .read_run("run_read_mc7_d11_fetch_issues")
      .expect("run should read back");

    let manifest_lineage =
      extract_minecraft_training_result_artifact_fetch_manifests(&store, &canonical)
        .expect("manifest lineage should extract");
    assert_eq!(manifest_lineage.len(), 1);
    assert!(manifest_lineage[0].manifest.is_none());
    assert!(
      manifest_lineage[0]
        .issue
        .as_deref()
        .is_some_and(|issue| issue.contains("mime_type text/plain is not JSON"))
    );

    let report_lineage =
      extract_minecraft_training_result_artifact_fetch_inspect_reports(&store, &canonical)
        .expect("report lineage should extract");
    assert_eq!(report_lineage.len(), 1);
    assert!(report_lineage[0].report.is_none());
    assert!(
      report_lineage[0]
        .issue
        .as_deref()
        .unwrap_or_default()
        .contains("failed to parse")
    );

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
  fn minecraft_spatial_bundle_manifest_lineage_reads_summary_without_artifact_payload() {
    let root = temp_dir("run-read-minecraft-spatial-bundle");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_spatial_bundle");
    let span = dummy_span(&run.root_span_id);

    let bundle_manifest = MinecraftSpatialBundleManifestSummary {
      schema_version: 1,
      source_run: SourceRunSummary {
        source_run_id: "source_run_1".to_string(),
        source_operation: "auv.minecraft.bridge".to_string(),
        source_run_type: "execute".to_string(),
        source_status: "ok".to_string(),
        generated_at_millis: 1,
        auv_git_commit: None,
        exporter_git_commit: None,
      },
      counts: SpatialBundleCounts {
        screenshots: 2,
        spatial_frames: 3,
        actions: 4,
        verification: 5,
        overlays: 6,
        skipped: 7,
      },
    };

    let artifacts = vec![stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_SPATIAL_BUNDLE_ARTIFACT_ROLE,
      "bundle-run.json",
      &serde_json::json!({
        "schema_version": bundle_manifest.schema_version,
        "source_run": bundle_manifest.source_run,
        "counts": bundle_manifest.counts,
        "artifacts": [
          {
            "artifact_id": "artifact_0001",
            "role": "minecraft-spatial-frame",
            "source_path": "artifacts/frame.json",
            "bundle_path": "spatial_frames/artifact_0001-frame.json",
            "directory": "spatial_frames",
            "summary": "big payload should be ignored by read-side summary"
          }
        ],
        "known_limits": ["fixture"]
      }),
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let listed =
      list_minecraft_spatial_bundle_manifests(&store, "run_read_minecraft_spatial_bundle")
        .expect("spatial bundle manifests should list");
    assert_eq!(listed.len(), 1);
    let manifest = listed[0].manifest.as_ref().expect("summary should parse");
    assert_eq!(manifest.source_run.source_run_id, "source_run_1");
    assert_eq!(manifest.counts.spatial_frames, 3);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_package_manifest_lineage_reads_summary() {
    let root = temp_dir("run-read-minecraft-training-package-manifest");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_package_manifest");
    let span = dummy_span(&run.root_span_id);

    let manifest = TrainingPackageManifest {
      schema_version: 1,
      generated_at_millis: 1,
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec![
        "/tmp/bundle-a/run.json".to_string(),
        "/tmp/bundle-b/run.json".to_string(),
      ],
      source_run_ids: vec!["run_a".to_string(), "run_b".to_string()],
      counts: TrainingPackageCounts {
        frames: 6,
        images: 6,
        compatibility_exported_frames: 4,
        compatibility_skipped_frames: 2,
      },
      frames: Vec::new(),
      compatibility_views: vec![TrainingCompatibilityViewReport {
        view_name: "nerfstudio".to_string(),
        status: TrainingCompatibilityStatus::Partial,
        exported_frame_count: 4,
        skipped_frame_count: 2,
        transforms_path: Some("compat/nerfstudio/transforms.json".to_string()),
        export_report_path: "compat/nerfstudio/export_report.json".to_string(),
        exported_frame_indices: vec![1, 2, 3, 4],
        frame_decisions: Vec::new(),
        skip_reason_counts: Vec::new(),
        warnings: vec!["missing screenshot on frame 6".to_string()],
        used_legacy_view_translation_fallback_frame_indices: vec![2],
        known_limits: vec!["legacy translation fallback used".to_string()],
      }],
      known_limits: vec!["canonical package only; no trainer output".to_string()],
    };

    let artifacts = vec![stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_PACKAGE_ARTIFACT_ROLE,
      "minecraft-3dgs-training-package-run.json",
      &manifest,
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = extract_minecraft_training_package_manifests(
      &store,
      &store
        .read_run("run_read_minecraft_training_package_manifest")
        .expect("run should read back"),
    )
    .expect("training package manifests should extract");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0]
      .manifest
      .as_ref()
      .expect("summary should parse");
    assert_eq!(
      summary.source_scene_packet_manifest_path,
      "/tmp/scene-packet/run.json"
    );
    assert_eq!(summary.counts.frames, 6);
    assert_eq!(summary.compatibility_views[0].view_name, "nerfstudio");

    let listed = list_minecraft_training_package_manifests(
      &store,
      "run_read_minecraft_training_package_manifest",
    )
    .expect("training package manifests should list");
    assert_eq!(listed, extracted);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_package_inspect_report_lineage_reads_summary() {
    let root = temp_dir("run-read-minecraft-training-package-inspect");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_package_inspect");
    let span = dummy_span(&run.root_span_id);

    let report = TrainingPackageInspectReport {
      schema_version: 1,
      generated_at_millis: 1,
      training_package_manifest_path: "/tmp/package/run.json".to_string(),
      scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
      source_run_ids: vec!["run_a".to_string()],
      counts: TrainingPackageCounts {
        frames: 3,
        images: 2,
        compatibility_exported_frames: 2,
        compatibility_skipped_frames: 1,
      },
      compatibility_views: vec![TrainingCompatibilityViewReport {
        view_name: "nerfstudio".to_string(),
        status: TrainingCompatibilityStatus::Ready,
        exported_frame_count: 2,
        skipped_frame_count: 1,
        transforms_path: Some("compat/nerfstudio/transforms.json".to_string()),
        export_report_path: "compat/nerfstudio/export_report.json".to_string(),
        exported_frame_indices: vec![1, 2],
        frame_decisions: Vec::new(),
        skip_reason_counts: Vec::new(),
        warnings: Vec::new(),
        used_legacy_view_translation_fallback_frame_indices: Vec::new(),
        known_limits: vec!["manual smoke required".to_string()],
      }],
      warnings: vec!["frame 3 skipped: missing_screenshot".to_string()],
      known_limits: vec!["synthetic validation only".to_string()],
    };

    let artifacts = vec![stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_PACKAGE_INSPECT_ARTIFACT_ROLE,
      "minecraft-3dgs-training-package-inspect.json",
      &report,
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = extract_minecraft_training_package_inspect_reports(
      &store,
      &store
        .read_run("run_read_minecraft_training_package_inspect")
        .expect("run should read back"),
    )
    .expect("training package inspect reports should extract");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0].report.as_ref().expect("summary should parse");
    assert_eq!(
      summary.training_package_manifest_path,
      "/tmp/package/run.json"
    );
    assert_eq!(summary.counts.compatibility_exported_frames, 2);
    assert_eq!(summary.warnings.len(), 1);

    let listed = list_minecraft_training_package_inspect_reports(
      &store,
      "run_read_minecraft_training_package_inspect",
    )
    .expect("training package inspect reports should list");
    assert_eq!(listed, extracted);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_package_manifest_lineage_reports_non_json_mime() {
    let root = temp_dir("run-read-minecraft-training-package-manifest-non-json");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_package_manifest_non_json");
    let span = dummy_span(&run.root_span_id);

    let artifacts = vec![stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_PACKAGE_ARTIFACT_ROLE,
      "minecraft-3dgs-training-package.txt",
      "plain text payload",
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = list_minecraft_training_package_manifests(
      &store,
      "run_read_minecraft_training_package_manifest_non_json",
    )
    .expect("training package manifests should list");
    assert_eq!(extracted.len(), 1);
    assert!(extracted[0].manifest.is_none());
    assert!(
      extracted[0]
        .issue
        .as_deref()
        .unwrap_or_default()
        .contains("mime_type")
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_package_inspect_report_lineage_reports_parse_failure() {
    let root = temp_dir("run-read-minecraft-training-package-inspect-malformed");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_package_inspect_malformed");
    let span = dummy_span(&run.root_span_id);

    let artifacts = vec![stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_PACKAGE_INSPECT_ARTIFACT_ROLE,
      "minecraft-3dgs-training-package-inspect.json",
      "{ not valid json",
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = list_minecraft_training_package_inspect_reports(
      &store,
      "run_read_minecraft_training_package_inspect_malformed",
    )
    .expect("training package inspect reports should list");
    assert_eq!(extracted.len(), 1);
    assert!(extracted[0].report.is_none());
    assert!(
      extracted[0]
        .issue
        .as_deref()
        .unwrap_or_default()
        .contains("failed to parse minecraft-3dgs-training-package-inspect artifact")
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_launch_manifest_lineage_reads_summary() {
    let root = temp_dir("run-read-minecraft-training-launch-manifest");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_launch_manifest");
    let span = dummy_span(&run.root_span_id);

    let manifest = TrainingLaunchPlanManifest {
      schema_version: 1,
      generated_at_millis: 1,
      source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
      source_training_package_inspect_report_path: "/tmp/package/inspect_report.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec![
        "/tmp/bundle-a/run.json".to_string(),
        "/tmp/bundle-b/run.json".to_string(),
      ],
      source_run_ids: vec!["run_a".to_string(), "run_b".to_string()],
      counts: TrainingPackageCounts {
        frames: 6,
        images: 6,
        compatibility_exported_frames: 4,
        compatibility_skipped_frames: 2,
      },
      compatibility_view_name: "nerfstudio".to_string(),
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      training_data_dir: "/tmp/package/compat/nerfstudio".to_string(),
      transforms_path: Some("compat/nerfstudio/transforms.json".to_string()),
      export_report_path: "compat/nerfstudio/export_report.json".to_string(),
      suggested_output_dir: "/tmp/output/trainer-output/nerfstudio-splatfacto".to_string(),
      launch_command: "ns-train splatfacto --data /tmp/package/compat/nerfstudio --output-dir /tmp/output/trainer-output/nerfstudio-splatfacto".to_string(),
      known_limits: vec!["launch prep only".to_string()],
    };

    let artifacts = vec![stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_LAUNCH_PLAN_ARTIFACT_ROLE,
      "minecraft-3dgs-training-launch-plan.json",
      &manifest,
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = extract_minecraft_training_launch_manifests(
      &store,
      &store
        .read_run("run_read_minecraft_training_launch_manifest")
        .expect("run should read back"),
    )
    .expect("training launch manifests should extract");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0]
      .manifest
      .as_ref()
      .expect("summary should parse");
    assert_eq!(
      summary.source_training_package_manifest_path,
      "/tmp/package/run.json"
    );
    assert_eq!(summary.counts.frames, 6);
    assert_eq!(summary.compatibility_view_name, "nerfstudio");
    assert_eq!(summary.trainer_backend, "nerfstudio.splatfacto");

    let listed = list_minecraft_training_launch_manifests(
      &store,
      "run_read_minecraft_training_launch_manifest",
    )
    .expect("training launch manifests should list");
    assert_eq!(listed, extracted);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_launch_inspect_report_lineage_reads_summary() {
    let root = temp_dir("run-read-minecraft-training-launch-inspect");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_launch_inspect");
    let span = dummy_span(&run.root_span_id);

    let report = TrainingLaunchInspectReport {
      schema_version: 1,
      generated_at_millis: 1,
      training_launch_manifest_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json"
        .to_string(),
      source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
      source_run_ids: vec!["run_a".to_string()],
      compatibility_status: TrainingCompatibilityStatus::Partial,
      trainer_readiness: TrainingLaunchReadiness::Blocked,
      readiness_blocker: Some(TrainingLaunchReadinessBlocker::TrainerCommandUnavailable),
      probe_command: "ns-train --help".to_string(),
      probe_succeeded: false,
      exported_frame_count: 2,
      skipped_frame_count: 1,
      transforms_present: true,
      warnings: vec!["ns-train unavailable".to_string()],
      known_limits: vec!["synthetic validation only".to_string()],
    };

    let artifacts = vec![stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_LAUNCH_INSPECT_ARTIFACT_ROLE,
      "minecraft-3dgs-training-launch-inspect.json",
      &report,
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = extract_minecraft_training_launch_inspect_reports(
      &store,
      &store
        .read_run("run_read_minecraft_training_launch_inspect")
        .expect("run should read back"),
    )
    .expect("training launch inspect reports should extract");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0].report.as_ref().expect("summary should parse");
    assert_eq!(
      summary.training_launch_manifest_path,
      "/tmp/launch/minecraft-3dgs-training-launch-plan.json"
    );
    assert_eq!(summary.compatibility_status, "Partial");
    assert_eq!(summary.trainer_readiness, "Blocked");
    assert_eq!(
      summary.readiness_blocker.as_deref(),
      Some("TrainerCommandUnavailable")
    );

    let listed = list_minecraft_training_launch_inspect_reports(
      &store,
      "run_read_minecraft_training_launch_inspect",
    )
    .expect("training launch inspect reports should list");
    assert_eq!(listed, extracted);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_job_manifest_lineage_reads_summary() {
    let root = temp_dir("run-read-minecraft-training-job-manifest");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_job_manifest");
    let span = dummy_span(&run.root_span_id);

    let manifest = TrainingLaunchJobManifest {
      schema_version: 1,
      generated_at_millis: 1,
      source_training_launch_plan_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json"
        .to_string(),
      source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
      source_training_package_inspect_report_path: "/tmp/package/inspect_report.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
      source_run_ids: vec!["run_a".to_string()],
      counts: TrainingLaunchJobCounts {
        frames: 6,
        images: 6,
        compatibility_exported_frames: 4,
        compatibility_skipped_frames: 2,
      },
      compatibility_view_name: "nerfstudio".to_string(),
      provider_backend: "remote-command-provider".to_string(),
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      job_submission_endpoint: "https://jobs.example/api".to_string(),
      job_submission_command: "submit-training-job".to_string(),
      training_data_dir: "/tmp/package/compat/nerfstudio".to_string(),
      transforms_path: Some("compat/nerfstudio/transforms.json".to_string()),
      export_report_path: "compat/nerfstudio/export_report.json".to_string(),
      suggested_output_dir: "/tmp/job/trainer-output/nerfstudio-splatfacto".to_string(),
      launch_command: "ns-train splatfacto --data /tmp/package/compat/nerfstudio --output-dir /tmp/job/trainer-output/nerfstudio-splatfacto".to_string(),
      status: TrainingLaunchJobStatus::Submitted,
      job_id: Some("job-123".to_string()),
      job_url: Some("https://jobs.example/job-123".to_string()),
      readiness_blocker: None,
      known_limits: vec!["remote submission only".to_string()],
    };

    let artifacts = vec![stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_JOB_ARTIFACT_ROLE,
      "minecraft-3dgs-training-job.json",
      &manifest,
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = extract_minecraft_training_job_manifests(
      &store,
      &store
        .read_run("run_read_minecraft_training_job_manifest")
        .expect("run should read back"),
    )
    .expect("training job manifests should extract");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0]
      .manifest
      .as_ref()
      .expect("summary should parse");
    assert_eq!(
      summary.source_training_launch_plan_path,
      "/tmp/launch/minecraft-3dgs-training-launch-plan.json"
    );
    assert_eq!(summary.status, "submitted");
    assert_eq!(summary.job_backend, "remote");
    assert_eq!(summary.counts.compatibility_exported_frames, 4);

    let listed =
      list_minecraft_training_job_manifests(&store, "run_read_minecraft_training_job_manifest")
        .expect("training job manifests should list");
    assert_eq!(listed, extracted);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_job_inspect_report_lineage_reads_summary() {
    let root = temp_dir("run-read-minecraft-training-job-inspect");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_job_inspect");
    let span = dummy_span(&run.root_span_id);

    let report = TrainingLaunchJobInspectReport {
      schema_version: 1,
      generated_at_millis: 1,
      training_launch_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json".to_string(),
      source_training_launch_plan_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json"
        .to_string(),
      source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
      source_run_ids: vec!["run_a".to_string()],
      provider_backend: "remote-command-provider".to_string(),
      job_backend: "remote".to_string(),
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_submission_endpoint: "https://jobs.example/api".to_string(),
      job_submission_command: "submit-training-job".to_string(),
      status: TrainingLaunchJobStatus::Blocked,
      job_id: None,
      job_url: None,
      readiness_blocker: Some(TrainingLaunchJobBlocker::MissingAuthentication),
      probe_command: "submit-training-job --help".to_string(),
      probe_succeeded: true,
      exported_frame_count: 4,
      skipped_frame_count: 2,
      transforms_present: true,
      warnings: vec!["token missing".to_string()],
      known_limits: vec!["job execution not consumed here".to_string()],
    };

    let artifacts = vec![stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_JOB_INSPECT_ARTIFACT_ROLE,
      "minecraft-3dgs-training-job-inspect.json",
      &report,
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = extract_minecraft_training_job_inspect_reports(
      &store,
      &store
        .read_run("run_read_minecraft_training_job_inspect")
        .expect("run should read back"),
    )
    .expect("training job inspect reports should extract");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0].report.as_ref().expect("summary should parse");
    assert_eq!(summary.status, "blocked");
    assert_eq!(
      summary.readiness_blocker.as_deref(),
      Some("MissingAuthentication")
    );
    assert_eq!(summary.exported_frame_count, 4);

    let listed = list_minecraft_training_job_inspect_reports(
      &store,
      "run_read_minecraft_training_job_inspect",
    )
    .expect("training job inspect reports should list");
    assert_eq!(listed, extracted);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_job_manifest_lineage_reports_non_json_mime() {
    let root = temp_dir("run-read-minecraft-training-job-manifest-non-json");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_job_manifest_non_json");
    let span = dummy_span(&run.root_span_id);

    let artifacts = vec![stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_JOB_ARTIFACT_ROLE,
      "minecraft-3dgs-training-job.txt",
      "plain text payload",
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = list_minecraft_training_job_manifests(
      &store,
      "run_read_minecraft_training_job_manifest_non_json",
    )
    .expect("training job manifests should list");
    assert_eq!(extracted.len(), 1);
    assert!(extracted[0].manifest.is_none());
    assert!(
      extracted[0]
        .issue
        .as_deref()
        .unwrap_or_default()
        .contains("mime_type")
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_job_inspect_report_lineage_reports_parse_failure() {
    let root = temp_dir("run-read-minecraft-training-job-inspect-malformed");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_job_inspect_malformed");
    let span = dummy_span(&run.root_span_id);

    let artifacts = vec![stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_JOB_INSPECT_ARTIFACT_ROLE,
      "minecraft-3dgs-training-job-inspect.json",
      "{ not valid json",
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = list_minecraft_training_job_inspect_reports(
      &store,
      "run_read_minecraft_training_job_inspect_malformed",
    )
    .expect("training job inspect reports should list");
    assert_eq!(extracted.len(), 1);
    assert!(extracted[0].report.is_none());
    assert!(
      extracted[0]
        .issue
        .as_deref()
        .unwrap_or_default()
        .contains("failed to parse minecraft-3dgs-training-job-inspect artifact")
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_result_manifest_lineage_reads_summary() {
    let root = temp_dir("run-read-minecraft-training-result-manifest");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_result_manifest");
    let span = dummy_span(&run.root_span_id);

    let manifest = TrainingResultManifest {
      schema_version: 1,
      generated_at_millis: 1,
      source_training_job_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json".to_string(),
      source_training_launch_plan_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json"
        .to_string(),
      source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
      source_run_ids: vec!["run_a".to_string()],
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      job_submission_endpoint: "https://jobs.example/api".to_string(),
      source_job_status: TrainingLaunchJobStatus::Submitted,
      status: TrainingResultStatus::Succeeded,
      job_id: "job-123".to_string(),
      job_url: Some("https://jobs.example/job-123".to_string()),
      result_dir: "/tmp/job/trainer-output/nerfstudio-splatfacto".to_string(),
      exported_frame_count: 4,
      skipped_frame_count: 2,
      result_artifacts: vec![TrainingResultArtifactRecord {
        relative_path: "config.yml".to_string(),
        absolute_path: "/tmp/job/trainer-output/nerfstudio-splatfacto/config.yml".to_string(),
        readable: true,
        byte_size: Some(128),
      }],
      known_limits: vec!["quality not graded".to_string()],
    };

    let artifacts = vec![stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_ROLE,
      "minecraft-3dgs-training-result.json",
      &manifest,
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = extract_minecraft_training_result_manifests(
      &store,
      &store
        .read_run("run_read_minecraft_training_result_manifest")
        .expect("run should read back"),
    )
    .expect("training result manifests should extract");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0]
      .manifest
      .as_ref()
      .expect("summary should parse");
    assert_eq!(summary.status, "succeeded");
    assert_eq!(summary.source_job_status, "submitted");
    assert_eq!(summary.result_artifacts.len(), 1);

    let listed = list_minecraft_training_result_manifests(
      &store,
      "run_read_minecraft_training_result_manifest",
    )
    .expect("training result manifests should list");
    assert_eq!(listed, extracted);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_result_inspect_report_lineage_reads_summary() {
    let root = temp_dir("run-read-minecraft-training-result-inspect");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_result_inspect");
    let span = dummy_span(&run.root_span_id);

    let report = TrainingResultInspectReport {
      schema_version: 1,
      generated_at_millis: 1,
      training_result_manifest_path: "/tmp/result/minecraft-3dgs-training-result.json".to_string(),
      source_training_job_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json".to_string(),
      source_training_launch_plan_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json"
        .to_string(),
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
      source_run_ids: vec!["run_a".to_string()],
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      job_submission_endpoint: "https://jobs.example/api".to_string(),
      source_job_status: TrainingLaunchJobStatus::Submitted,
      status: TrainingResultStatus::Failed,
      status_reason: Some(TrainingResultReason::ResultArtifactsMissing),
      job_id: "job-123".to_string(),
      job_url: Some("https://jobs.example/job-123".to_string()),
      result_dir: "/tmp/job/trainer-output/nerfstudio-splatfacto".to_string(),
      result_dir_exists: true,
      key_result_artifacts_present: false,
      result_artifact_count: 0,
      warnings: vec!["models directory missing".to_string()],
      known_limits: vec!["quality not graded".to_string()],
    };

    let artifacts = vec![stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_INSPECT_ARTIFACT_ROLE,
      "minecraft-3dgs-training-result-inspect.json",
      &report,
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = extract_minecraft_training_result_inspect_reports(
      &store,
      &store
        .read_run("run_read_minecraft_training_result_inspect")
        .expect("run should read back"),
    )
    .expect("training result inspect reports should extract");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0].report.as_ref().expect("summary should parse");
    assert_eq!(summary.status, "failed");
    assert_eq!(
      summary.status_reason.as_deref(),
      Some("result_artifacts_missing")
    );
    assert!(!summary.key_result_artifacts_present);

    let listed = list_minecraft_training_result_inspect_reports(
      &store,
      "run_read_minecraft_training_result_inspect",
    )
    .expect("training result inspect reports should list");
    assert_eq!(listed, extracted);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_result_manifest_lineage_reports_non_json_mime() {
    let root = temp_dir("run-read-minecraft-training-result-manifest-non-json");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_result_manifest_non_json");
    let span = dummy_span(&run.root_span_id);

    let artifacts = vec![stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_ROLE,
      "minecraft-3dgs-training-result.txt",
      "plain text payload",
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = list_minecraft_training_result_manifests(
      &store,
      "run_read_minecraft_training_result_manifest_non_json",
    )
    .expect("training result manifests should list");
    assert_eq!(extracted.len(), 1);
    assert!(extracted[0].manifest.is_none());
    assert!(
      extracted[0]
        .issue
        .as_deref()
        .unwrap_or_default()
        .contains("mime_type")
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_launch_manifest_lineage_reports_non_json_mime() {
    let root = temp_dir("run-read-minecraft-training-launch-manifest-non-json");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_launch_manifest_non_json");
    let span = dummy_span(&run.root_span_id);

    let artifacts = vec![stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_LAUNCH_PLAN_ARTIFACT_ROLE,
      "minecraft-3dgs-training-launch-plan.txt",
      "plain text payload",
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = list_minecraft_training_launch_manifests(
      &store,
      "run_read_minecraft_training_launch_manifest_non_json",
    )
    .expect("training launch manifests should list");
    assert_eq!(extracted.len(), 1);
    assert!(extracted[0].manifest.is_none());
    assert!(
      extracted[0]
        .issue
        .as_deref()
        .unwrap_or_default()
        .contains("mime_type")
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_launch_inspect_report_lineage_reports_parse_failure() {
    let root = temp_dir("run-read-minecraft-training-launch-inspect-malformed");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_launch_inspect_malformed");
    let span = dummy_span(&run.root_span_id);

    let artifacts = vec![stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_LAUNCH_INSPECT_ARTIFACT_ROLE,
      "minecraft-3dgs-training-launch-inspect.json",
      "{ not valid json",
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = list_minecraft_training_launch_inspect_reports(
      &store,
      "run_read_minecraft_training_launch_inspect_malformed",
    )
    .expect("training launch inspect reports should list");
    assert_eq!(extracted.len(), 1);
    assert!(extracted[0].report.is_none());
    assert!(
      extracted[0]
        .issue
        .as_deref()
        .unwrap_or_default()
        .contains("failed to parse minecraft-3dgs-training-launch-inspect artifact")
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_result_inspect_report_lineage_reports_parse_failure() {
    let root = temp_dir("run-read-minecraft-training-result-inspect-malformed");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_result_inspect_malformed");
    let span = dummy_span(&run.root_span_id);

    let artifacts = vec![stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_INSPECT_ARTIFACT_ROLE,
      "minecraft-3dgs-training-result-inspect.json",
      "{ not valid json",
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = list_minecraft_training_result_inspect_reports(
      &store,
      "run_read_minecraft_training_result_inspect_malformed",
    )
    .expect("training result inspect reports should list");
    assert_eq!(extracted.len(), 1);
    assert!(extracted[0].report.is_none());
    assert!(
      extracted[0]
        .issue
        .as_deref()
        .unwrap_or_default()
        .contains("failed to parse minecraft-3dgs-training-result-inspect artifact")
    );

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
      extracted[0].freshness_source_operation_id.as_deref(),
      Some("observe.window.capture")
    );
    assert_eq!(
      extracted[0].consent_scope.as_deref(),
      Some("candidate_promotion_only")
    );
    assert_eq!(
      extracted[0].consent_approved_action.as_deref(),
      Some("promote_recognition_to_candidate")
    );
    assert_eq!(
      extracted[0].consent_recognition_id.as_deref(),
      Some("promotion_ready_frame_1")
    );
    assert_eq!(
      extracted[0].promoted_candidate_local_ids,
      vec!["promoted-item_end_turn".to_string()]
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

  #[test]
  fn candidate_action_decision_lineage_extracts_decide_only_and_error_states() {
    let root = temp_dir("run-read-candidate-action-decision");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_candidate_action_decision");
    let span = dummy_span(&run.root_span_id);
    let source_promotion_artifact = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      CANDIDATE_PROMOTION_ARTIFACT_ROLE,
      "candidate-promotion-source.json",
      &serde_json::json!({"fixture": "candidate-promotion"}),
    );

    let ready_decision = candidate_action_decision_artifact(
      Some(ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: source_promotion_artifact.artifact_id.clone(),
        span_id: span.span_id.clone(),
        captured_event_id: source_promotion_artifact.event_id.clone(),
      }),
      "decision_ready",
      "promotion_ready",
      "promoted-item_end_turn",
    );
    let missing_source_decision = candidate_action_decision_artifact(
      None,
      "decision_missing_source",
      "promotion_ready",
      "promoted-item_end_turn",
    );
    let unresolved_source_decision = candidate_action_decision_artifact(
      Some(ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: ArtifactId::new("artifact_missing_promotion"),
        span_id: span.span_id.clone(),
        captured_event_id: Some(EventId::new("event_missing_promotion")),
      }),
      "decision_unresolved_source",
      "promotion_ready",
      "promoted-item_end_turn",
    );

    let artifacts = vec![
      source_promotion_artifact,
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        1,
        CANDIDATE_ACTION_DECISION_ARTIFACT_ROLE,
        "candidate-action-decision-ready.json",
        &ready_decision,
      ),
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        2,
        CANDIDATE_ACTION_DECISION_ARTIFACT_ROLE,
        "candidate-action-decision-missing-source.json",
        &missing_source_decision,
      ),
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        3,
        CANDIDATE_ACTION_DECISION_ARTIFACT_ROLE,
        "candidate-action-decision-unresolved-source.json",
        &unresolved_source_decision,
      ),
      stage_text_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        4,
        CANDIDATE_ACTION_DECISION_ARTIFACT_ROLE,
        "candidate-action-decision-malformed.json",
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
      .read_run("run_read_candidate_action_decision")
      .expect("run should read back");
    let extracted = extract_candidate_action_decision_lineage(&store, &canonical)
      .expect("candidate action decision lineage should extract");
    assert_eq!(extracted.len(), 4);
    assert_eq!(
      extracted[0].status,
      CandidateActionDecisionLineageStatus::Ready
    );
    assert_eq!(extracted[0].decision_id.as_deref(), Some("decision_ready"));
    assert_eq!(
      extracted[0].resolver_operation.as_deref(),
      Some("candidate.action.decide_only")
    );
    assert_eq!(
      extracted[0].selected_method.as_deref(),
      Some("pointer-click")
    );
    assert_eq!(
      extracted[0].side_effect.as_deref(),
      Some("none_decide_only")
    );
    assert_eq!(
      extracted[0].input_delivery.as_deref(),
      Some("not_attempted")
    );
    assert_eq!(
      extracted[0].operation_result.as_deref(),
      Some("not_produced")
    );
    assert_eq!(
      extracted[0].verification_result.as_deref(),
      Some("not_produced")
    );
    assert_eq!(
      extracted[1].status,
      CandidateActionDecisionLineageStatus::MissingSourceCandidatePromotionArtifact
    );
    assert_eq!(
      extracted[2].status,
      CandidateActionDecisionLineageStatus::SourceCandidatePromotionArtifactUnresolved
    );
    assert_eq!(
      extracted[3].status,
      CandidateActionDecisionLineageStatus::Malformed
    );
    assert!(
      extracted[3]
        .issue
        .as_deref()
        .unwrap_or_default()
        .contains("failed to parse candidate-action-decision artifact")
    );

    let listed =
      list_candidate_action_decision_lineage(&store, "run_read_candidate_action_decision")
        .expect("candidate action decision lineage should list");
    assert_eq!(listed, extracted);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn candidate_action_execution_lineage_extracts_activation_only_and_error_states() {
    let root = temp_dir("run-read-candidate-action-execution");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_candidate_action_execution");
    let span = dummy_span(&run.root_span_id);
    let source_decision_artifact = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      CANDIDATE_ACTION_DECISION_ARTIFACT_ROLE,
      "candidate-action-decision-source.json",
      &serde_json::json!({"fixture": "candidate-action-decision"}),
    );
    let operation_result_artifact = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      1,
      "operation-result",
      "candidate-action-operation-result.json",
      &serde_json::json!({"fixture": "operation-result"}),
    );

    let ready_execution = candidate_action_execution_artifact(
      ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: source_decision_artifact.artifact_id.clone(),
        span_id: span.span_id.clone(),
        captured_event_id: source_decision_artifact.event_id.clone(),
      },
      Some(ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: operation_result_artifact.artifact_id.clone(),
        span_id: span.span_id.clone(),
        captured_event_id: operation_result_artifact.event_id.clone(),
      }),
      "execution_ready",
    );
    let missing_operation_result_execution = candidate_action_execution_artifact(
      ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: source_decision_artifact.artifact_id.clone(),
        span_id: span.span_id.clone(),
        captured_event_id: source_decision_artifact.event_id.clone(),
      },
      None,
      "execution_missing_operation",
    );
    let unresolved_source_execution = candidate_action_execution_artifact(
      ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: ArtifactId::new("artifact_missing_decision"),
        span_id: span.span_id.clone(),
        captured_event_id: Some(EventId::new("event_missing_decision")),
      },
      Some(ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: operation_result_artifact.artifact_id.clone(),
        span_id: span.span_id.clone(),
        captured_event_id: operation_result_artifact.event_id.clone(),
      }),
      "execution_unresolved_source",
    );
    let semantic_execution = candidate_action_execution_with_semantic_artifact(
      ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: source_decision_artifact.artifact_id.clone(),
        span_id: span.span_id.clone(),
        captured_event_id: source_decision_artifact.event_id.clone(),
      },
      Some(ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: operation_result_artifact.artifact_id.clone(),
        span_id: span.span_id.clone(),
        captured_event_id: operation_result_artifact.event_id.clone(),
      }),
      "execution_semantic",
    );

    let artifacts = vec![
      source_decision_artifact,
      operation_result_artifact,
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        2,
        CANDIDATE_ACTION_EXECUTION_ARTIFACT_ROLE,
        "candidate-action-execution-ready.json",
        &ready_execution,
      ),
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        3,
        CANDIDATE_ACTION_EXECUTION_ARTIFACT_ROLE,
        "candidate-action-execution-missing-operation.json",
        &missing_operation_result_execution,
      ),
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        4,
        CANDIDATE_ACTION_EXECUTION_ARTIFACT_ROLE,
        "candidate-action-execution-unresolved-source.json",
        &unresolved_source_execution,
      ),
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        5,
        CANDIDATE_ACTION_EXECUTION_ARTIFACT_ROLE,
        "candidate-action-execution-semantic.json",
        &semantic_execution,
      ),
      stage_text_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        6,
        CANDIDATE_ACTION_EXECUTION_ARTIFACT_ROLE,
        "candidate-action-execution-malformed.json",
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
      .read_run("run_read_candidate_action_execution")
      .expect("run should read back");
    let extracted = extract_candidate_action_execution_lineage(&store, &canonical)
      .expect("candidate action execution lineage should extract");
    assert_eq!(extracted.len(), 5);
    assert_eq!(
      extracted[0].status,
      CandidateActionExecutionLineageStatus::Ready
    );
    assert_eq!(
      extracted[0].execution_id.as_deref(),
      Some("execution_ready")
    );
    assert_eq!(extracted[0].input_delivery.as_deref(), Some("attempted"));
    assert_eq!(
      extracted[0].selected_path.as_deref(),
      Some("window_targeted_mouse")
    );
    assert_eq!(extracted[0].operation_status.as_deref(), Some("completed"));
    assert_eq!(
      extracted[0].verification.as_deref(),
      Some("activation_only")
    );
    assert_eq!(
      extracted[0].closure_state,
      CandidateActionExecutionClosureState::SemanticOpen
    );
    assert_eq!(extracted[0].semantic_matched, None);
    assert_eq!(extracted[0].attempts, Some(1));
    assert_eq!(extracted[0].attempts_succeeded, Some(1));
    assert_eq!(
      extracted[1].status,
      CandidateActionExecutionLineageStatus::MissingOperationResultArtifact
    );
    assert_eq!(
      extracted[2].status,
      CandidateActionExecutionLineageStatus::SourceCandidateActionDecisionArtifactUnresolved
    );
    assert_eq!(
      extracted[3].status,
      CandidateActionExecutionLineageStatus::Ready
    );
    assert_eq!(
      extracted[3].verification.as_deref(),
      Some("activation_only+post_action:semantic_match")
    );
    assert_eq!(
      extracted[3].closure_state,
      CandidateActionExecutionClosureState::EvidenceClosed
    );
    assert_eq!(extracted[3].semantic_matched, Some(true));
    assert_eq!(
      extracted[4].status,
      CandidateActionExecutionLineageStatus::Malformed
    );

    let listed =
      list_candidate_action_execution_lineage(&store, "run_read_candidate_action_execution")
        .expect("candidate action execution lineage should list");
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

  fn candidate_action_decision_artifact(
    source_candidate_promotion_artifact: Option<ArtifactRef>,
    decision_id: &str,
    source_promotion_id: &str,
    candidate_local_id: &str,
  ) -> CandidateActionDecisionArtifact {
    CandidateActionDecisionArtifact {
      artifact_version: "candidate_action_decision_artifact_v0".to_string(),
      decision_id: decision_id.to_string(),
      source_candidate_promotion_artifact,
      source_promotion_id: source_promotion_id.to_string(),
      candidate_local_id: candidate_local_id.to_string(),
      action_resolver_decision: ActionResolverDecision::new(ActionResolverDecisionInput {
        operation: "candidate.action.decide_only",
        target_query: "End Turn",
        primary_method: "pointer-click",
        selected_method: "pointer-click",
        fallback_allowed: false,
        fallback_used: false,
        fallback_reason: None,
        policy: "candidate-coordinate-pointer",
        cursor_disturbance: "warp-visible",
        press_mechanism: "pointer-click",
      }),
      side_effect: CandidateActionSideEffect::NoneDecideOnly,
      detail: json!({
        "input_delivery": "not_attempted",
        "operation_result": "not_produced",
        "verification_result": "not_produced",
      }),
      known_limits: vec![
        "L8a records an ActionResolverDecision only; it does not call auv-driver or produce InputActionResult".to_string(),
      ],
    }
  }

  fn candidate_action_execution_artifact(
    source_candidate_action_decision_artifact: ArtifactRef,
    operation_result_artifact: Option<ArtifactRef>,
    execution_id: &str,
  ) -> CandidateActionExecutionArtifact {
    CandidateActionExecutionArtifact {
      artifact_version: "candidate_action_execution_artifact_v0".to_string(),
      execution_id: execution_id.to_string(),
      source_candidate_action_decision_artifact,
      source_candidate_promotion_artifact: None,
      operation_result_artifact,
      source_promotion_id: "promotion_ready".to_string(),
      source_decision_id: "decision_ready".to_string(),
      candidate_local_id: "promoted-item_end_turn".to_string(),
      action_resolver_decision: ActionResolverDecision::new(ActionResolverDecisionInput {
        operation: "candidate.action.decide_only",
        target_query: "End Turn",
        primary_method: "pointer-click",
        selected_method: "pointer-click",
        fallback_allowed: false,
        fallback_used: false,
        fallback_reason: None,
        policy: "candidate-coordinate-pointer",
        cursor_disturbance: "warp-visible",
        press_mechanism: "pointer-click",
      }),
      consent: CandidateActionExecutionConsent {
        consent_id: "consent_execute_end_turn".to_string(),
        execution_id: "execution_end_turn".to_string(),
        granted_by: "human-review".to_string(),
        scope_note: "execute exactly one approved candidate action".to_string(),
        run_id: "run_read_candidate_action_execution".to_string(),
        source_promotion_id: "promotion_ready".to_string(),
        source_decision_id: "decision_ready".to_string(),
        candidate_local_id: "promoted-item_end_turn".to_string(),
        approved_action: CandidateActionExecutionConsentAction::ExecuteSingleCandidateAction,
        provenance: crate::candidate_promotion::ConsentProvenance::HumanGesture,
        grade: crate::candidate_promotion::ConsentGrade::HumanApproved,
        approved_at_millis: 2,
        evidence_note: "unit test execution consent".to_string(),
      },
      readiness: auv_driver::ReadinessReport::ready(vec![auv_driver::ReadinessCheck::pass(
        "target_window_present",
        "target window present",
      )]),
      input_action_result: auv_driver::InputActionResult::single_success(
        auv_driver::InputDeliveryPath::WindowTargetedMouse,
      ),
      operation_result: OperationResult {
        api_version: OPERATION_RESULT_API_VERSION.to_string(),
        run_id: RunId::new("run_read_candidate_action_execution"),
        status: OperationStatus::Completed,
        operation_id: "candidate.action.execute_single".to_string(),
        evidence_artifacts: Vec::new(),
        output: OperationOutput::Acknowledged {
          message: Some(
            "single candidate action activated; semantic verification remains activation_only"
              .to_string(),
          ),
        },
        verifications: vec![candidate_action_activation_verification(
          VerificationMethod::Custom {
            name: "activation_only".to_string(),
          },
          Vec::new(),
          Some("End Turn"),
        )],
        freshness_basis: None,
        known_limits: vec![
          "activation_only verification records input delivery, not semantic success".to_string(),
        ],
      },
      verification_result: candidate_action_activation_verification(
        VerificationMethod::Custom {
          name: "activation_only".to_string(),
        },
        Vec::new(),
        Some("End Turn"),
      ),
      side_effect: CandidateActionExecutionSideEffect::SingleInputDelivered,
      detail: json!({
        "input_delivery": "attempted",
        "selected_path": "window_targeted_mouse",
        "attempt_count": 1,
        "attempts_succeeded": 1,
        "operation_status": "completed",
        "verification": "activation_only",
        "semantic_matched": null,
        "readiness": "ready",
        "readiness_blocker": null,
      }),
      known_limits: vec![
        "activation_only verification records input delivery, not semantic success".to_string(),
      ],
    }
  }

  fn candidate_action_execution_with_semantic_artifact(
    source_candidate_action_decision_artifact: ArtifactRef,
    operation_result_artifact: Option<ArtifactRef>,
    execution_id: &str,
  ) -> CandidateActionExecutionArtifact {
    let mut artifact = candidate_action_execution_artifact(
      source_candidate_action_decision_artifact,
      operation_result_artifact,
      execution_id,
    );
    let semantic = candidate_action_semantic_verification();
    artifact
      .operation_result
      .verifications
      .push(semantic.clone());
    artifact.verification_result = semantic;
    artifact.detail = json!({
      "input_delivery": "attempted",
      "selected_path": "window_targeted_mouse",
      "attempt_count": 1,
      "attempts_succeeded": 1,
      "operation_status": "completed",
      "verification": "activation_only+post_action:semantic_match",
      "verification_count": 2,
      "post_action_verification_count": 1,
      "semantic_matched": true,
    });
    artifact
  }

  fn candidate_action_activation_verification(
    method: VerificationMethod,
    evidence: Vec<ArtifactRef>,
    observed_label: Option<&str>,
  ) -> VerificationResult {
    VerificationResult {
      api_version: VERIFICATION_RESULT_API_VERSION.to_string(),
      method,
      executed: true,
      state_changed: false,
      semantic_matched: None,
      failure_layer: None,
      evidence,
      consumed_candidate_ref: None,
      consumed_node_ref: None,
      consumed_recognition_artifact_ref: None,
      consumed_recognition_id: None,
      consumed_recognized_item_id: Some("item_end_turn".to_string()),
      observed_label: observed_label.map(str::to_string),
    }
  }

  fn candidate_action_semantic_verification() -> VerificationResult {
    VerificationResult {
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
      consumed_recognition_id: Some("recognition_after_action".to_string()),
      consumed_recognized_item_id: Some("item_end_turn".to_string()),
      observed_label: Some("End Turn activated".to_string()),
    }
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
        freshness: Some(crate::contract::FreshnessBasis {
          source_artifact: capture_artifact.clone(),
          source_operation_id: Some("observe.window.capture".to_string()),
          notes: vec!["fixture freshness".to_string()],
        }),
        permission: Some(ActionPermission {
          granted_by: "human-review".to_string(),
          scope_note: "fixture promotion".to_string(),
          consent: Some(ActionConsentRecord {
            consent_id: format!("consent_{promotion_id}"),
            recognition_id: format!("{promotion_id}_frame_1"),
            run_id: run_id.as_str().to_string(),
            scope: ConsentScope::CandidatePromotionOnly,
            approved_action: ConsentAction::PromoteRecognitionToCandidate,
            provenance: crate::candidate_promotion::ConsentProvenance::HumanGesture,
            grade: crate::candidate_promotion::ConsentGrade::HumanApproved,
            approved_at_millis: 1,
            evidence_note: "fixture consent".to_string(),
          }),
        }),
        allow_dev_self_minted_consent: false,
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
