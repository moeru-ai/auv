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

use crate::action_resolver_decision::ActionResolverDecision;
use crate::candidate_action_decision::{
  CandidateActionDecisionArtifact, CandidateActionExecutionArtifact,
};
use crate::candidate_promotion::{CandidatePromotion, PromotionProjection, PromotionRefusal};
use crate::candidate_promotion_recording::CandidatePromotionArtifact;
use crate::contract::{
  ArtifactRef, FailureLayer, ObservationSnapshot, OperationOutput, OperationResult,
  OperationStatus, RecognitionResult, RecognitionSource, VerificationMethod, VerificationResult,
};
use crate::minecraft_query_live_action::QUERY_WIRED_LIVE_ACTION_OPERATION_ID;
use crate::model::AuvResult;
use crate::osu_query_live_action::QUERY_WIRED_LIVE_ACTION_OPERATION_ID as OSU_QUERY_WIRED_LIVE_ACTION_OPERATION_ID;
use crate::scroll_scan::ScrollScanArtifact;
use crate::stability::{StabilityAssessment, StabilityRejection};
use auv_game_minecraft::artifact::MinecraftProjectionArtifact;
use auv_game_minecraft::dataset::{SourceRunSummary, SpatialBundleCounts};
use auv_game_minecraft::{
  TrainingCompatibilityViewReport, TrainingLaunchInspectReport, TrainingLaunchJobInspectReport,
  TrainingLaunchJobManifest, TrainingLaunchPlanManifest, TrainingPackageCounts,
  TrainingPackageInspectReport, TrainingPackageManifest, TrainingResultArtifactFetchInspectReport,
  TrainingResultArtifactFetchManifest, TrainingResultHoldoutPreviewInspectReport,
  TrainingResultHoldoutPreviewManifest, TrainingResultHoldoutRenderQualityInspectReport,
  TrainingResultHoldoutRenderQualityManifest, TrainingResultInspectReport, TrainingResultManifest,
  TrainingResultSemanticCheckpointRecord, TrainingResultSemanticInspectReport,
  TrainingResultSemanticManifest, TrainingResultSpatialQueryInspectReport,
  TrainingResultSpatialQueryManifest, derive_action_readiness,
};
use auv_game_osu::{
  DetectionEvalQualityInspectReport, DetectionEvalQualityManifest,
  DetectionEvalWitnessInspectReport, DetectionEvalWitnessManifest,
  VisualTruthSemanticInspectReport, VisualTruthSemanticManifest,
  VisualTruthSpatialQueryInspectReport, VisualTruthSpatialQueryManifest,
  derive_visual_truth_spatial_query_action_readiness,
};
use auv_tracing_driver::store::{CanonicalRun, LocalStore};
use auv_tracing_driver::trace::ArtifactRecordV1Alpha1;
use serde::de::DeserializeOwned;

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
pub struct MinecraftTrainingResultSemanticManifestLineage {
  pub artifact: ArtifactRefLineage,
  pub manifest: Option<MinecraftTrainingResultSemanticManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct MinecraftTrainingResultSemanticInspectReportLineage {
  pub artifact: ArtifactRefLineage,
  pub report: Option<MinecraftTrainingResultSemanticInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct MinecraftTrainingResultSpatialQueryManifestLineage {
  pub artifact: ArtifactRefLineage,
  pub manifest: Option<MinecraftTrainingResultSpatialQueryManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct OsuVisualTruthSemanticManifestLineage {
  pub artifact: ArtifactRefLineage,
  pub manifest: Option<OsuVisualTruthSemanticManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct OsuVisualTruthSemanticInspectReportLineage {
  pub artifact: ArtifactRefLineage,
  pub report: Option<OsuVisualTruthSemanticInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct OsuVisualTruthSpatialQueryManifestLineage {
  pub artifact: ArtifactRefLineage,
  pub manifest: Option<OsuVisualTruthSpatialQueryManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct OsuVisualTruthSpatialQueryInspectReportLineage {
  pub artifact: ArtifactRefLineage,
  pub report: Option<OsuVisualTruthSpatialQueryInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct OsuVisualTruthSpatialQueryActionReadinessSummary {
  pub action_eligibility: String,
  pub pixel_point: Option<String>,
  pub refusal_reason: Option<String>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OsuVisualTruthSemanticManifestSummary {
  pub schema_version: u32,
  pub source_run_artifact_dir: String,
  pub source_visual_truth_manifest_path: String,
  pub source_projection_path: String,
  pub beatmap_path: String,
  pub frame_count: usize,
  pub semantic_status: String,
  pub semantic_reason: Option<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OsuVisualTruthSemanticInspectReportSummary {
  pub schema_version: u32,
  pub visual_truth_semantic_manifest_path: String,
  pub source_run_artifact_dir: String,
  pub semantic_status: String,
  pub semantic_reason: Option<String>,
  pub visual_truth_manifest_readable: bool,
  pub projection_readable: bool,
  pub projection_eval_ready: bool,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OsuVisualTruthSpatialQueryManifestSummary {
  pub schema_version: u32,
  pub visual_truth_semantic_manifest_path: String,
  pub source_run_artifact_dir: String,
  pub object_index: usize,
  pub capture_phase: String,
  pub object_kind: Option<String>,
  pub query_backend: String,
  pub status: String,
  pub reason: Option<String>,
  pub pixel_visibility: Option<String>,
  pub pixel_x: Option<f32>,
  pub pixel_y: Option<f32>,
  pub match_radius_px: Option<f32>,
  pub capture_width: Option<u32>,
  pub capture_height: Option<u32>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OsuVisualTruthSpatialQueryInspectReportSummary {
  pub schema_version: u32,
  pub visual_truth_spatial_query_manifest_path: String,
  pub visual_truth_semantic_manifest_path: String,
  pub object_index: usize,
  pub capture_phase: String,
  pub query_backend: String,
  pub status: String,
  pub reason: Option<String>,
  pub pixel_visibility: Option<String>,
  pub semantic_status: String,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct MinecraftTrainingResultSpatialQueryActionReadinessSummary {
  pub action_eligibility: String,
  pub readiness_class: Option<String>,
  pub window_point: Option<String>,
  pub refusal_reason: Option<String>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct OsuQueryWiredLiveActionSummary {
  pub operation_result_artifact_id: Option<String>,
  pub query_artifact_id: Option<String>,
  pub attempted: bool,
  pub action_eligibility: String,
  pub pixel_point: Option<String>,
  pub window_point: Option<String>,
  pub refusal_reason: Option<String>,
  pub operation_status: Option<String>,
  pub operation_message: Option<String>,
  pub target_app: Option<String>,
  pub target_title: Option<String>,
  pub dispatch_command: Option<String>,
  pub dispatch_outcome: Option<String>,
  pub readiness_class: Option<String>,
  pub source_readiness_ref: Option<String>,
  pub verification_outcome: String,
  pub verification_source: Option<String>,
  pub verification_reason: Option<String>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct OsuDetectionEvalWitnessManifestLineage {
  pub artifact: ArtifactRefLineage,
  pub manifest: Option<OsuDetectionEvalWitnessManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct OsuDetectionEvalWitnessInspectReportLineage {
  pub artifact: ArtifactRefLineage,
  pub report: Option<OsuDetectionEvalWitnessInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct OsuDetectionEvalQualityManifestLineage {
  pub artifact: ArtifactRefLineage,
  pub manifest: Option<OsuDetectionEvalQualityManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct OsuDetectionEvalQualityInspectReportLineage {
  pub artifact: ArtifactRefLineage,
  pub report: Option<OsuDetectionEvalQualityInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OsuDetectionEvalWitnessManifestSummary {
  pub schema_version: u32,
  pub source_visual_eval_report_path: String,
  pub source_run_artifact_dir: String,
  pub detector_model_id: Option<String>,
  pub total_frames: usize,
  pub label_matched_frames: usize,
  pub spatial_matched_frames: usize,
  pub spatial_unscored_frames: usize,
  pub spurious_detection_count: usize,
  pub projection_kind: String,
  pub frame_witness_count: usize,
  pub status: String,
  pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OsuDetectionEvalWitnessInspectReportSummary {
  pub schema_version: u32,
  pub detection_eval_witness_manifest_path: String,
  pub total_frames: usize,
  pub frame_witness_count: usize,
  pub status: String,
  pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OsuDetectionEvalQualityManifestSummary {
  pub schema_version: u32,
  pub detection_eval_witness_manifest_path: String,
  pub source_visual_eval_report_path: String,
  pub witness_status: String,
  pub status: String,
  pub verdict: String,
  pub label_recall: Option<f32>,
  pub spatial_recall: Option<f32>,
  pub spurious_detection_count: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OsuDetectionEvalQualityInspectReportSummary {
  pub schema_version: u32,
  pub detection_eval_quality_manifest_path: String,
  pub witness_status: String,
  pub status: String,
  pub verdict: String,
  pub label_recall_available: bool,
  pub spatial_recall_available: bool,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct BalatroCardDetectionSemanticManifestLineage {
  pub artifact: ArtifactRefLineage,
  pub manifest: Option<BalatroCardDetectionSemanticManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct BalatroCardDetectionSemanticInspectReportLineage {
  pub artifact: ArtifactRefLineage,
  pub report: Option<BalatroCardDetectionSemanticInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct BalatroCardDetectionSpatialQueryManifestLineage {
  pub artifact: ArtifactRefLineage,
  pub manifest: Option<BalatroCardDetectionSpatialQueryManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct BalatroCardDetectionSpatialQueryInspectReportLineage {
  pub artifact: ArtifactRefLineage,
  pub report: Option<BalatroCardDetectionSpatialQueryInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct BalatroCardDetectionEvalWitnessManifestLineage {
  pub artifact: ArtifactRefLineage,
  pub manifest: Option<BalatroCardDetectionEvalWitnessManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct BalatroCardDetectionEvalWitnessInspectReportLineage {
  pub artifact: ArtifactRefLineage,
  pub report: Option<BalatroCardDetectionEvalWitnessInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct BalatroCardDetectionQualityManifestLineage {
  pub artifact: ArtifactRefLineage,
  pub manifest: Option<BalatroCardDetectionQualityManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct BalatroCardDetectionQualityInspectReportLineage {
  pub artifact: ArtifactRefLineage,
  pub report: Option<BalatroCardDetectionQualityInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BalatroCardDetectionSemanticManifestSummary {
  pub schema_version: u32,
  pub source_detection_bundle_dir: String,
  pub frame_source: String,
  pub image_width: u32,
  pub image_height: u32,
  pub ui_detection_count: usize,
  pub entities_detection_count: usize,
  pub semantic_status: String,
  pub semantic_reason: Option<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BalatroCardDetectionSemanticInspectReportSummary {
  pub schema_version: u32,
  pub card_detection_semantic_manifest_path: String,
  pub semantic_status: String,
  pub semantic_reason: Option<String>,
  pub detection_bundle_readable: bool,
  pub detection_sets_non_empty: bool,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BalatroCardDetectionSpatialQueryManifestSummary {
  pub schema_version: u32,
  pub card_detection_semantic_manifest_path: String,
  pub target_zone: String,
  pub target_index: u32,
  pub query_backend: String,
  pub status: String,
  pub reason: Option<String>,
  pub pixel_x: Option<f32>,
  pub pixel_y: Option<f32>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BalatroCardDetectionSpatialQueryInspectReportSummary {
  pub schema_version: u32,
  pub card_detection_spatial_query_manifest_path: String,
  pub target_zone: String,
  pub target_index: u32,
  pub query_backend: String,
  pub status: String,
  pub reason: Option<String>,
  pub semantic_status: String,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BalatroCardDetectionEvalWitnessManifestSummary {
  pub schema_version: u32,
  pub card_detection_semantic_manifest_path: String,
  pub card_detection_spatial_query_manifest_path: String,
  pub expected_slots_path: String,
  pub source_detection_bundle_dir: String,
  pub expected_slot_count: usize,
  pub scored_slot_count: usize,
  pub unscored_slot_count: usize,
  pub below_confidence_slot_count: usize,
  pub quality_backend: String,
  pub detector_model_id: Option<String>,
  pub slot_score_count: usize,
  pub status: String,
  pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BalatroCardDetectionEvalWitnessInspectReportSummary {
  pub schema_version: u32,
  pub card_detection_eval_witness_manifest_path: String,
  pub card_detection_semantic_manifest_path: String,
  pub card_detection_spatial_query_manifest_path: String,
  pub expected_slots_path: String,
  pub source_detection_bundle_dir: String,
  pub expected_slot_count: usize,
  pub scored_slot_count: usize,
  pub unscored_slot_count: usize,
  pub below_confidence_slot_count: usize,
  pub quality_backend: String,
  pub detector_model_id: Option<String>,
  pub slot_score_count: usize,
  pub semantic_manifest_readable: bool,
  pub spatial_query_manifest_readable: bool,
  pub expected_slots_readable: bool,
  pub status: String,
  pub reason: Option<String>,
  pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BalatroCardDetectionQualityManifestSummary {
  pub schema_version: u32,
  pub card_detection_eval_witness_manifest_path: String,
  pub witness_status: String,
  pub status: String,
  pub verdict: String,
  pub quality_backend: Option<String>,
  pub expected_slot_count: Option<usize>,
  pub scored_slot_count: Option<usize>,
  pub unscored_slot_count: Option<usize>,
  pub slot_coverage_ratio: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BalatroCardDetectionQualityInspectReportSummary {
  pub schema_version: u32,
  pub card_detection_quality_manifest_path: String,
  pub card_detection_eval_witness_manifest_path: String,
  pub witness_status: String,
  pub status: String,
  pub verdict: String,
  pub quality_backend: Option<String>,
  pub slot_coverage_ratio_available: bool,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct OsuDetectionEvalQualityVerdictSummary {
  pub verdict: String,
  pub derived_from_witness_status: String,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct MinecraftQueryWiredLiveActionSummary {
  pub operation_result_artifact_id: Option<String>,
  pub query_artifact_id: Option<String>,
  pub attempted: bool,
  pub action_eligibility: String,
  pub window_point: Option<String>,
  pub refusal_reason: Option<String>,
  pub operation_status: Option<String>,
  pub operation_message: Option<String>,
  pub target_app: Option<String>,
  pub target_title: Option<String>,
  pub dispatch_command: Option<String>,
  pub dispatch_outcome: Option<String>,
  pub mc14_action_eligibility: Option<String>,
  pub readiness_class: Option<String>,
  pub source_readiness_ref: Option<String>,
  pub verification_outcome: String,
  pub verification_source: Option<String>,
  pub verification_reason: Option<String>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct MinecraftTrainingResultSpatialQueryInspectReportLineage {
  pub artifact: ArtifactRefLineage,
  pub report: Option<MinecraftTrainingResultSpatialQueryInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MinecraftTrainingResultHoldoutPreviewFrameWitnessSummary {
  pub frame_index: usize,
  pub spatial_frame_id: String,
  pub screenshot_path: String,
  pub frame_json_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MinecraftTrainingResultHoldoutPreviewManifestSummary {
  pub schema_version: u32,
  pub training_result_semantic_manifest_path: String,
  pub source_training_result_artifact_manifest_path: String,
  pub source_training_result_manifest_path: String,
  pub source_training_job_manifest_path: String,
  pub source_training_launch_plan_path: String,
  pub source_training_package_manifest_path: String,
  pub source_scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub trainer_backend: String,
  pub job_backend: String,
  pub normalized_result_dir: String,
  pub holdout_frame_index: usize,
  pub holdout_frame: Option<MinecraftTrainingResultHoldoutPreviewFrameWitnessSummary>,
  pub basis_checkpoint_path: Option<String>,
  pub holdout_screenshot_path: Option<String>,
  pub reference_overlay_path: Option<String>,
  pub status: String,
  pub reason: Option<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MinecraftTrainingResultHoldoutPreviewInspectReportSummary {
  pub schema_version: u32,
  pub training_result_holdout_preview_manifest_path: String,
  pub training_result_semantic_manifest_path: String,
  pub source_training_result_artifact_manifest_path: String,
  pub source_training_result_manifest_path: String,
  pub source_training_job_manifest_path: String,
  pub source_training_launch_plan_path: String,
  pub source_training_package_manifest_path: String,
  pub source_scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub trainer_backend: String,
  pub job_backend: String,
  pub normalized_result_dir: String,
  pub holdout_frame_index: usize,
  pub holdout_frame: Option<MinecraftTrainingResultHoldoutPreviewFrameWitnessSummary>,
  pub basis_checkpoint_path: Option<String>,
  pub holdout_screenshot_path: Option<String>,
  pub reference_overlay_path: Option<String>,
  pub status: String,
  pub reason: Option<String>,
  pub holdout_frame_selection: String,
  pub checkpoint_count: usize,
  pub scene_packet_frame_count: usize,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct MinecraftTrainingResultHoldoutPreviewManifestLineage {
  pub artifact: ArtifactRefLineage,
  pub manifest: Option<MinecraftTrainingResultHoldoutPreviewManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct MinecraftTrainingResultHoldoutPreviewInspectReportLineage {
  pub artifact: ArtifactRefLineage,
  pub report: Option<MinecraftTrainingResultHoldoutPreviewInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MinecraftHoldoutRenderQualityMetricsSummary {
  pub l1_mean: Option<f64>,
  pub mse: Option<f64>,
  pub psnr: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MinecraftHoldoutRenderQualityManifestSummary {
  pub schema_version: u32,
  pub training_result_semantic_manifest_path: String,
  pub holdout_preview_manifest_path: String,
  pub source_training_result_artifact_manifest_path: String,
  pub source_training_result_manifest_path: String,
  pub source_training_job_manifest_path: String,
  pub source_scene_packet_manifest_path: String,
  pub source_run_ids: Vec<String>,
  pub holdout_frame_index: usize,
  pub basis_checkpoint_path: Option<String>,
  pub rendered_image_path: Option<String>,
  pub image_size_match: bool,
  pub metrics: Option<MinecraftHoldoutRenderQualityMetricsSummary>,
  pub status: String,
  pub reason: Option<String>,
  pub verdict: String,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MinecraftHoldoutRenderQualityInspectReportSummary {
  pub schema_version: u32,
  pub training_result_holdout_render_quality_manifest_path: String,
  pub training_result_semantic_manifest_path: String,
  pub holdout_preview_manifest_path: String,
  pub source_training_result_artifact_manifest_path: String,
  pub source_training_result_manifest_path: String,
  pub source_training_job_manifest_path: String,
  pub source_scene_packet_manifest_path: String,
  pub source_run_ids: Vec<String>,
  pub holdout_frame_index: usize,
  pub basis_checkpoint_path: Option<String>,
  pub rendered_image_path: Option<String>,
  pub image_size_match: bool,
  pub metrics: Option<MinecraftHoldoutRenderQualityMetricsSummary>,
  pub status: String,
  pub reason: Option<String>,
  pub verdict: String,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct MinecraftHoldoutRenderQualityManifestLineage {
  pub artifact: ArtifactRefLineage,
  pub manifest: Option<MinecraftHoldoutRenderQualityManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct MinecraftHoldoutRenderQualityInspectReportLineage {
  pub artifact: ArtifactRefLineage,
  pub report: Option<MinecraftHoldoutRenderQualityInspectReportSummary>,
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
  pub submission_recorded_at_millis: Option<u64>,
  pub accepted_by_provider: bool,
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
  pub submission_recorded_at_millis: Option<u64>,
  pub accepted_by_provider: bool,
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
  #[serde(default)]
  pub status_message: Option<String>,
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
  #[serde(default)]
  pub status_message: Option<String>,
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
pub struct MinecraftTrainingResultSemanticCheckpointSummary {
  pub relative_path: String,
  pub byte_size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MinecraftTrainingResultSemanticManifestSummary {
  pub schema_version: u32,
  pub source_training_result_artifact_manifest_path: String,
  pub source_training_result_manifest_path: String,
  pub source_training_job_manifest_path: String,
  pub source_training_launch_plan_path: String,
  pub source_training_package_manifest_path: String,
  pub source_scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub trainer_backend: String,
  pub job_backend: String,
  pub source_result_status: String,
  pub normalized_result_dir: String,
  pub semantic_status: String,
  pub semantic_reason: Option<String>,
  pub config_path: String,
  pub models_dir_path: String,
  pub status_snapshot_path: Option<String>,
  pub config_trainer: Option<String>,
  pub checkpoint_files: Vec<MinecraftTrainingResultSemanticCheckpointSummary>,
  pub checkpoint_count: usize,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MinecraftTrainingResultSemanticInspectReportSummary {
  pub schema_version: u32,
  pub training_result_semantic_manifest_path: String,
  pub source_training_result_artifact_manifest_path: String,
  pub source_training_result_manifest_path: String,
  pub source_training_job_manifest_path: String,
  pub source_training_launch_plan_path: String,
  pub source_training_package_manifest_path: String,
  pub source_scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub trainer_backend: String,
  pub job_backend: String,
  pub source_result_status: String,
  pub normalized_result_dir: String,
  pub semantic_status: String,
  pub semantic_reason: Option<String>,
  pub config_yaml_parsed: bool,
  pub config_trainer: Option<String>,
  pub config_backend_matches: bool,
  pub models_dir_readable: bool,
  pub status_snapshot_present: bool,
  pub checkpoint_count: usize,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MinecraftTrainingResultSpatialQueryManifestSummary {
  pub schema_version: u32,
  pub training_result_semantic_manifest_path: String,
  pub source_training_result_artifact_manifest_path: String,
  pub source_training_result_manifest_path: String,
  pub source_training_job_manifest_path: String,
  pub source_training_launch_plan_path: String,
  pub source_training_package_manifest_path: String,
  pub source_scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub trainer_backend: String,
  pub job_backend: String,
  pub normalized_result_dir: String,
  pub query_kind: String,
  pub target_block: String,
  pub target_face: Option<String>,
  pub target_semantics: String,
  pub selected_backend: Option<String>,
  pub status: String,
  pub reason: Option<String>,
  pub visibility: Option<String>,
  pub screen_point: Option<String>,
  pub match_radius_px: Option<f64>,
  pub confidence: Option<f64>,
  pub basis_frame_id: Option<String>,
  pub comparison_verdict: Option<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MinecraftTrainingResultSpatialQueryInspectReportSummary {
  pub schema_version: u32,
  pub training_result_spatial_query_manifest_path: String,
  pub training_result_semantic_manifest_path: String,
  pub source_training_result_artifact_manifest_path: String,
  pub source_training_result_manifest_path: String,
  pub source_training_job_manifest_path: String,
  pub source_training_launch_plan_path: String,
  pub source_training_package_manifest_path: String,
  pub source_scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub trainer_backend: String,
  pub job_backend: String,
  pub normalized_result_dir: String,
  pub query_kind: String,
  pub target_block: String,
  pub target_face: Option<String>,
  pub target_semantics: String,
  pub selected_backend: Option<String>,
  pub status: String,
  pub reason: Option<String>,
  pub visibility: Option<String>,
  pub screen_point: Option<String>,
  pub match_radius_px: Option<f64>,
  pub confidence: Option<f64>,
  pub basis_frame_id: Option<String>,
  pub comparison_verdict: Option<String>,
  pub provider_status: String,
  pub provider_reason: Option<String>,
  pub provider_message: Option<String>,
  pub reference_status: String,
  pub reference_reason: Option<String>,
  pub reference_basis_frame_id: Option<String>,
  pub reference_source_frame_json_path: Option<String>,
  pub reference_screenshot_path: Option<String>,
  pub scene_packet_frame_count: usize,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
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

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionTransitionLineageStatus {
  Ready,
  Partial,
  Malformed,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ActionResolverDecisionProjection {
  pub version: String,
  pub operation: String,
  pub target_query: String,
  pub primary_method: String,
  pub selected_method: String,
  pub fallback_allowed: bool,
  pub fallback_used: bool,
  pub fallback_reason: Option<String>,
  pub policy: String,
  pub cursor_disturbance: String,
  pub press_mechanism: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ActionTransitionPreState {
  pub source_candidate_promotion_artifact: Option<ArtifactRefLineage>,
  pub source_promotion_id: Option<String>,
  pub candidate_local_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ActionTransitionPostState {
  pub operation_result_artifact: Option<ArtifactRefLineage>,
  pub operation_status: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ActionTransitionVerificationProjection {
  pub verification_outcome: String,
  pub verification_reason: Option<String>,
  pub semantic_matched: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ActionTransitionLineage {
  pub artifact: ArtifactRefLineage,
  pub status: ActionTransitionLineageStatus,
  pub execution_id: Option<String>,
  pub pre_state: ActionTransitionPreState,
  pub effective_decision: Option<ActionResolverDecisionProjection>,
  pub planned_decision: Option<ActionResolverDecisionProjection>,
  pub driver_result: Option<auv_driver::InputActionResult>,
  pub post_state: ActionTransitionPostState,
  pub verification: ActionTransitionVerificationProjection,
  pub known_limits: Vec<String>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
struct LegacyCandidateActionExecutionArtifact {
  pub artifact_version: Option<String>,
  pub execution_id: Option<String>,
  pub source_candidate_action_decision_artifact: Option<ArtifactRef>,
  pub source_candidate_promotion_artifact: Option<ArtifactRef>,
  pub operation_result_artifact: Option<ArtifactRef>,
  pub source_promotion_id: Option<String>,
  pub source_decision_id: Option<String>,
  pub candidate_local_id: Option<String>,
  pub action_resolver_decision: Option<ActionResolverDecision>,
  pub input_action_result: Option<auv_driver::InputActionResult>,
  pub operation_result: Option<OperationResult>,
  pub verification_result: Option<VerificationResult>,
  pub detail: Option<serde_json::Value>,
  #[serde(default)]
  pub known_limits: Vec<String>,
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

/// Read the persisted `OperationResult` for a run, if one was recorded.
///
/// Scans the run's artifacts for the first `operation-result` JSON record,
/// mirroring the role/mime filter used by [`extract_verifications`]. Returns
/// `Ok(None)` when the run exists but recorded no operation result.
///
/// This is the storage-side half of the API-P4 `GetOperation` read path; the
/// two-source join with the runtime summary lives in
/// `crate::api::session_service::summary`.
pub fn read_operation_result(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Option<OperationResult>> {
  let run = store.read_run(run_id)?;
  for artifact in &run.artifacts {
    if artifact.role != "operation-result" || !is_json_mime(&artifact.mime_type) {
      continue;
    }
    let operation_result: OperationResult =
      read_artifact_json(store, run.run.run_id.as_str(), artifact, "operation-result")?;
    return Ok(Some(operation_result));
  }
  Ok(None)
}

/// Read the persisted `OperationSummary` for a run, if one was recorded (API-P11).
///
/// Scans the run's artifacts for the first `operation-summary` JSON record,
/// mirroring [`read_operation_result`]. Returns `Ok(None)` when the run exists
/// but recorded no operation summary artifact.
pub fn read_operation_summary(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Option<auv_cli_invoke::OperationSummary>> {
  use auv_cli_invoke::OperationSummaryRecord;

  let run = store.read_run(run_id)?;
  for artifact in &run.artifacts {
    if artifact.role != crate::contract::OPERATION_SUMMARY_ARTIFACT_ROLE
      || !is_json_mime(&artifact.mime_type)
    {
      continue;
    }
    let record: OperationSummaryRecord = read_artifact_json(
      store,
      run.run.run_id.as_str(),
      artifact,
      "operation-summary",
    )?;
    return Ok(Some(auv_cli_invoke::OperationSummary::from_record(record)));
  }
  Ok(None)
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

pub fn list_action_transition_lineage(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<ActionTransitionLineage>> {
  let run = store.read_run(run_id)?;
  extract_action_transition_lineage(store, &run)
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

pub(crate) fn list_minecraft_training_result_semantic_manifests(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<MinecraftTrainingResultSemanticManifestLineage>> {
  let run = store.read_run(run_id)?;
  extract_minecraft_training_result_semantic_manifests(store, &run)
}

pub(crate) fn list_minecraft_training_result_semantic_inspect_reports(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<MinecraftTrainingResultSemanticInspectReportLineage>> {
  let run = store.read_run(run_id)?;
  extract_minecraft_training_result_semantic_inspect_reports(store, &run)
}

pub(crate) fn list_minecraft_holdout_render_quality_manifests(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<MinecraftHoldoutRenderQualityManifestLineage>> {
  let run = store.read_run(run_id)?;
  extract_minecraft_holdout_render_quality_manifests(store, &run)
}

pub(crate) fn list_minecraft_holdout_render_quality_inspect_reports(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<MinecraftHoldoutRenderQualityInspectReportLineage>> {
  let run = store.read_run(run_id)?;
  extract_minecraft_holdout_render_quality_inspect_reports(store, &run)
}

pub(crate) fn extract_minecraft_holdout_render_quality_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<MinecraftHoldoutRenderQualityManifestLineage>> {
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::minecraft::MINECRAFT_3DGS_HOLDOUT_RENDER_QUALITY_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(MinecraftHoldoutRenderQualityManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!(
          "minecraft holdout render quality manifest mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingResultHoldoutRenderQualityManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::minecraft::MINECRAFT_3DGS_HOLDOUT_RENDER_QUALITY_ROLE,
    )
    .map(MinecraftHoldoutRenderQualityManifestSummary::from);
    match parsed {
      Ok(manifest) => manifests.push(MinecraftHoldoutRenderQualityManifestLineage {
        artifact: artifact_ref,
        manifest: Some(manifest),
        issue: None,
      }),
      Err(error) => manifests.push(MinecraftHoldoutRenderQualityManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(error),
      }),
    }
  }
  Ok(manifests)
}

pub(crate) fn extract_minecraft_holdout_render_quality_inspect_reports(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<MinecraftHoldoutRenderQualityInspectReportLineage>> {
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::minecraft::MINECRAFT_3DGS_HOLDOUT_RENDER_QUALITY_INSPECT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(MinecraftHoldoutRenderQualityInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!(
          "minecraft holdout render quality inspect mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingResultHoldoutRenderQualityInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::minecraft::MINECRAFT_3DGS_HOLDOUT_RENDER_QUALITY_INSPECT_ROLE,
    )
    .map(MinecraftHoldoutRenderQualityInspectReportSummary::from);
    match parsed {
      Ok(report) => reports.push(MinecraftHoldoutRenderQualityInspectReportLineage {
        artifact: artifact_ref,
        report: Some(report),
        issue: None,
      }),
      Err(error) => reports.push(MinecraftHoldoutRenderQualityInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(error),
      }),
    }
  }
  Ok(reports)
}

pub(crate) fn list_minecraft_training_result_spatial_query_manifests(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<MinecraftTrainingResultSpatialQueryManifestLineage>> {
  let run = store.read_run(run_id)?;
  extract_minecraft_training_result_spatial_query_manifests(store, &run)
}

pub(crate) fn list_osu_visual_truth_semantic_manifests(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<OsuVisualTruthSemanticManifestLineage>> {
  let run = store.read_run(run_id)?;
  extract_osu_visual_truth_semantic_manifests(store, &run)
}

pub(crate) fn list_osu_visual_truth_semantic_inspect_reports(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<OsuVisualTruthSemanticInspectReportLineage>> {
  let run = store.read_run(run_id)?;
  extract_osu_visual_truth_semantic_inspect_reports(store, &run)
}

pub(crate) fn list_osu_visual_truth_spatial_query_manifests(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<OsuVisualTruthSpatialQueryManifestLineage>> {
  let run = store.read_run(run_id)?;
  extract_osu_visual_truth_spatial_query_manifests(store, &run)
}

pub(crate) fn list_osu_visual_truth_spatial_query_inspect_reports(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<OsuVisualTruthSpatialQueryInspectReportLineage>> {
  let run = store.read_run(run_id)?;
  extract_osu_visual_truth_spatial_query_inspect_reports(store, &run)
}

pub(crate) fn extract_osu_visual_truth_semantic_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<OsuVisualTruthSemanticManifestLineage>> {
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::osu::OSU_VISUAL_TRUTH_SEMANTIC_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(OsuVisualTruthSemanticManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!(
          "osu visual truth semantic manifest mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<VisualTruthSemanticManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::osu::OSU_VISUAL_TRUTH_SEMANTIC_ROLE,
    )
    .map(|manifest| OsuVisualTruthSemanticManifestSummary::from(&manifest));
    match parsed {
      Ok(manifest) => manifests.push(OsuVisualTruthSemanticManifestLineage {
        artifact: artifact_ref,
        manifest: Some(manifest),
        issue: None,
      }),
      Err(error) => manifests.push(OsuVisualTruthSemanticManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(error),
      }),
    }
  }
  Ok(manifests)
}

pub(crate) fn extract_osu_visual_truth_semantic_inspect_reports(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<OsuVisualTruthSemanticInspectReportLineage>> {
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::osu::OSU_VISUAL_TRUTH_SEMANTIC_INSPECT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(OsuVisualTruthSemanticInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!(
          "osu visual truth semantic inspect mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<VisualTruthSemanticInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::osu::OSU_VISUAL_TRUTH_SEMANTIC_INSPECT_ROLE,
    )
    .map(|report| OsuVisualTruthSemanticInspectReportSummary::from(&report));
    match parsed {
      Ok(report) => reports.push(OsuVisualTruthSemanticInspectReportLineage {
        artifact: artifact_ref,
        report: Some(report),
        issue: None,
      }),
      Err(error) => reports.push(OsuVisualTruthSemanticInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(error),
      }),
    }
  }
  Ok(reports)
}

pub(crate) fn extract_osu_visual_truth_spatial_query_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<OsuVisualTruthSpatialQueryManifestLineage>> {
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::osu::OSU_VISUAL_TRUTH_SPATIAL_QUERY_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(OsuVisualTruthSpatialQueryManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!(
          "osu visual truth spatial query manifest mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<VisualTruthSpatialQueryManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::osu::OSU_VISUAL_TRUTH_SPATIAL_QUERY_ROLE,
    )
    .map(|manifest| OsuVisualTruthSpatialQueryManifestSummary::from(&manifest));
    match parsed {
      Ok(manifest) => manifests.push(OsuVisualTruthSpatialQueryManifestLineage {
        artifact: artifact_ref,
        manifest: Some(manifest),
        issue: None,
      }),
      Err(error) => manifests.push(OsuVisualTruthSpatialQueryManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(error),
      }),
    }
  }
  Ok(manifests)
}

pub(crate) fn extract_osu_visual_truth_spatial_query_inspect_reports(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<OsuVisualTruthSpatialQueryInspectReportLineage>> {
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::osu::OSU_VISUAL_TRUTH_SPATIAL_QUERY_INSPECT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(OsuVisualTruthSpatialQueryInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!(
          "osu visual truth spatial query inspect mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<VisualTruthSpatialQueryInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::osu::OSU_VISUAL_TRUTH_SPATIAL_QUERY_INSPECT_ROLE,
    )
    .map(|report| OsuVisualTruthSpatialQueryInspectReportSummary::from(&report));
    match parsed {
      Ok(report) => reports.push(OsuVisualTruthSpatialQueryInspectReportLineage {
        artifact: artifact_ref,
        report: Some(report),
        issue: None,
      }),
      Err(error) => reports.push(OsuVisualTruthSpatialQueryInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(error),
      }),
    }
  }
  Ok(reports)
}

pub(crate) fn list_osu_detection_eval_witness_manifests(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<OsuDetectionEvalWitnessManifestLineage>> {
  let run = store.read_run(run_id)?;
  extract_osu_detection_eval_witness_manifests(store, &run)
}

pub(crate) fn list_osu_detection_eval_witness_inspect_reports(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<OsuDetectionEvalWitnessInspectReportLineage>> {
  let run = store.read_run(run_id)?;
  extract_osu_detection_eval_witness_inspect_reports(store, &run)
}

pub(crate) fn list_osu_detection_eval_quality_manifests(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<OsuDetectionEvalQualityManifestLineage>> {
  let run = store.read_run(run_id)?;
  extract_osu_detection_eval_quality_manifests(store, &run)
}

pub(crate) fn list_osu_detection_eval_quality_inspect_reports(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<OsuDetectionEvalQualityInspectReportLineage>> {
  let run = store.read_run(run_id)?;
  extract_osu_detection_eval_quality_inspect_reports(store, &run)
}

pub(crate) fn extract_osu_detection_eval_witness_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<OsuDetectionEvalWitnessManifestLineage>> {
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::osu::OSU_DETECTION_EVAL_WITNESS_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(OsuDetectionEvalWitnessManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!(
          "osu detection eval witness manifest mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<DetectionEvalWitnessManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::osu::OSU_DETECTION_EVAL_WITNESS_ROLE,
    )
    .map(|manifest| OsuDetectionEvalWitnessManifestSummary::from(&manifest));
    match parsed {
      Ok(manifest) => manifests.push(OsuDetectionEvalWitnessManifestLineage {
        artifact: artifact_ref,
        manifest: Some(manifest),
        issue: None,
      }),
      Err(error) => manifests.push(OsuDetectionEvalWitnessManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(error),
      }),
    }
  }
  Ok(manifests)
}

pub(crate) fn extract_osu_detection_eval_witness_inspect_reports(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<OsuDetectionEvalWitnessInspectReportLineage>> {
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::osu::OSU_DETECTION_EVAL_WITNESS_INSPECT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(OsuDetectionEvalWitnessInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!(
          "osu detection eval witness inspect mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<DetectionEvalWitnessInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::osu::OSU_DETECTION_EVAL_WITNESS_INSPECT_ROLE,
    )
    .map(|report| OsuDetectionEvalWitnessInspectReportSummary::from(&report));
    match parsed {
      Ok(report) => reports.push(OsuDetectionEvalWitnessInspectReportLineage {
        artifact: artifact_ref,
        report: Some(report),
        issue: None,
      }),
      Err(error) => reports.push(OsuDetectionEvalWitnessInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(error),
      }),
    }
  }
  Ok(reports)
}

pub(crate) fn extract_osu_detection_eval_quality_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<OsuDetectionEvalQualityManifestLineage>> {
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::osu::OSU_DETECTION_EVAL_QUALITY_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(OsuDetectionEvalQualityManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!(
          "osu detection eval quality manifest mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<DetectionEvalQualityManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::osu::OSU_DETECTION_EVAL_QUALITY_ROLE,
    )
    .map(|manifest| OsuDetectionEvalQualityManifestSummary::from(&manifest));
    match parsed {
      Ok(manifest) => manifests.push(OsuDetectionEvalQualityManifestLineage {
        artifact: artifact_ref,
        manifest: Some(manifest),
        issue: None,
      }),
      Err(error) => manifests.push(OsuDetectionEvalQualityManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(error),
      }),
    }
  }
  Ok(manifests)
}

pub(crate) fn extract_osu_detection_eval_quality_inspect_reports(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<OsuDetectionEvalQualityInspectReportLineage>> {
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::osu::OSU_DETECTION_EVAL_QUALITY_INSPECT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(OsuDetectionEvalQualityInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!(
          "osu detection eval quality inspect mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<DetectionEvalQualityInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::osu::OSU_DETECTION_EVAL_QUALITY_INSPECT_ROLE,
    )
    .map(|report| OsuDetectionEvalQualityInspectReportSummary::from(&report));
    match parsed {
      Ok(report) => reports.push(OsuDetectionEvalQualityInspectReportLineage {
        artifact: artifact_ref,
        report: Some(report),
        issue: None,
      }),
      Err(error) => reports.push(OsuDetectionEvalQualityInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(error),
      }),
    }
  }
  Ok(reports)
}

pub(crate) fn list_balatro_card_detection_semantic_manifests(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<BalatroCardDetectionSemanticManifestLineage>> {
  let run = store.read_run(run_id)?;
  extract_balatro_card_detection_semantic_manifests(store, &run)
}

pub(crate) fn list_balatro_card_detection_semantic_inspect_reports(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<BalatroCardDetectionSemanticInspectReportLineage>> {
  let run = store.read_run(run_id)?;
  extract_balatro_card_detection_semantic_inspect_reports(store, &run)
}

pub(crate) fn list_balatro_card_detection_spatial_query_manifests(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<BalatroCardDetectionSpatialQueryManifestLineage>> {
  let run = store.read_run(run_id)?;
  extract_balatro_card_detection_spatial_query_manifests(store, &run)
}

pub(crate) fn list_balatro_card_detection_spatial_query_inspect_reports(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<BalatroCardDetectionSpatialQueryInspectReportLineage>> {
  let run = store.read_run(run_id)?;
  extract_balatro_card_detection_spatial_query_inspect_reports(store, &run)
}

pub(crate) fn list_balatro_card_detection_eval_witness_manifests(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<BalatroCardDetectionEvalWitnessManifestLineage>> {
  let run = store.read_run(run_id)?;
  extract_balatro_card_detection_eval_witness_manifests(store, &run)
}

pub(crate) fn list_balatro_card_detection_eval_witness_inspect_reports(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<BalatroCardDetectionEvalWitnessInspectReportLineage>> {
  let run = store.read_run(run_id)?;
  extract_balatro_card_detection_eval_witness_inspect_reports(store, &run)
}

pub(crate) fn list_balatro_card_detection_quality_manifests(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<BalatroCardDetectionQualityManifestLineage>> {
  let run = store.read_run(run_id)?;
  extract_balatro_card_detection_quality_manifests(store, &run)
}

pub(crate) fn list_balatro_card_detection_quality_inspect_reports(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<BalatroCardDetectionQualityInspectReportLineage>> {
  let run = store.read_run(run_id)?;
  extract_balatro_card_detection_quality_inspect_reports(store, &run)
}

pub(crate) fn extract_balatro_card_detection_semantic_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<BalatroCardDetectionSemanticManifestLineage>> {
  use auv_game_balatro::CardDetectionSemanticManifest;
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::balatro::BALATRO_CARD_DETECTION_SEMANTIC_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(BalatroCardDetectionSemanticManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!(
          "balatro card detection semantic manifest mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<CardDetectionSemanticManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::balatro::BALATRO_CARD_DETECTION_SEMANTIC_ROLE,
    )
    .map(|manifest| BalatroCardDetectionSemanticManifestSummary::from(&manifest));
    match parsed {
      Ok(manifest) => manifests.push(BalatroCardDetectionSemanticManifestLineage {
        artifact: artifact_ref,
        manifest: Some(manifest),
        issue: None,
      }),
      Err(error) => manifests.push(BalatroCardDetectionSemanticManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(error),
      }),
    }
  }
  Ok(manifests)
}

pub(crate) fn extract_balatro_card_detection_semantic_inspect_reports(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<BalatroCardDetectionSemanticInspectReportLineage>> {
  use auv_game_balatro::CardDetectionSemanticInspectReport;
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::balatro::BALATRO_CARD_DETECTION_SEMANTIC_INSPECT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(BalatroCardDetectionSemanticInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!(
          "balatro card detection semantic inspect mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<CardDetectionSemanticInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::balatro::BALATRO_CARD_DETECTION_SEMANTIC_INSPECT_ROLE,
    )
    .map(|report| BalatroCardDetectionSemanticInspectReportSummary::from(&report));
    match parsed {
      Ok(report) => reports.push(BalatroCardDetectionSemanticInspectReportLineage {
        artifact: artifact_ref,
        report: Some(report),
        issue: None,
      }),
      Err(error) => reports.push(BalatroCardDetectionSemanticInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(error),
      }),
    }
  }
  Ok(reports)
}

pub(crate) fn extract_balatro_card_detection_spatial_query_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<BalatroCardDetectionSpatialQueryManifestLineage>> {
  use auv_game_balatro::CardDetectionSpatialQueryManifest;
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::balatro::BALATRO_CARD_DETECTION_SPATIAL_QUERY_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(BalatroCardDetectionSpatialQueryManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!(
          "balatro card detection spatial query manifest mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<CardDetectionSpatialQueryManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::balatro::BALATRO_CARD_DETECTION_SPATIAL_QUERY_ROLE,
    )
    .map(|manifest| BalatroCardDetectionSpatialQueryManifestSummary::from(&manifest));
    match parsed {
      Ok(manifest) => manifests.push(BalatroCardDetectionSpatialQueryManifestLineage {
        artifact: artifact_ref,
        manifest: Some(manifest),
        issue: None,
      }),
      Err(error) => manifests.push(BalatroCardDetectionSpatialQueryManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(error),
      }),
    }
  }
  Ok(manifests)
}

pub(crate) fn extract_balatro_card_detection_spatial_query_inspect_reports(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<BalatroCardDetectionSpatialQueryInspectReportLineage>> {
  use auv_game_balatro::CardDetectionSpatialQueryInspectReport;
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::balatro::BALATRO_CARD_DETECTION_SPATIAL_QUERY_INSPECT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(BalatroCardDetectionSpatialQueryInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!(
          "balatro card detection spatial query inspect mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<CardDetectionSpatialQueryInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::balatro::BALATRO_CARD_DETECTION_SPATIAL_QUERY_INSPECT_ROLE,
    )
    .map(|report| BalatroCardDetectionSpatialQueryInspectReportSummary::from(&report));
    match parsed {
      Ok(report) => reports.push(BalatroCardDetectionSpatialQueryInspectReportLineage {
        artifact: artifact_ref,
        report: Some(report),
        issue: None,
      }),
      Err(error) => reports.push(BalatroCardDetectionSpatialQueryInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(error),
      }),
    }
  }
  Ok(reports)
}

pub(crate) fn extract_balatro_card_detection_eval_witness_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<BalatroCardDetectionEvalWitnessManifestLineage>> {
  use auv_game_balatro::CardDetectionEvalWitnessManifest;
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::balatro::BALATRO_CARD_DETECTION_EVAL_WITNESS_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(BalatroCardDetectionEvalWitnessManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!(
          "balatro card detection eval witness manifest mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<CardDetectionEvalWitnessManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::balatro::BALATRO_CARD_DETECTION_EVAL_WITNESS_ROLE,
    )
    .map(|manifest| BalatroCardDetectionEvalWitnessManifestSummary::from(&manifest));
    match parsed {
      Ok(manifest) => manifests.push(BalatroCardDetectionEvalWitnessManifestLineage {
        artifact: artifact_ref,
        manifest: Some(manifest),
        issue: None,
      }),
      Err(error) => manifests.push(BalatroCardDetectionEvalWitnessManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(error),
      }),
    }
  }
  Ok(manifests)
}

pub(crate) fn extract_balatro_card_detection_eval_witness_inspect_reports(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<BalatroCardDetectionEvalWitnessInspectReportLineage>> {
  use auv_game_balatro::CardDetectionEvalWitnessInspectReport;
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::balatro::BALATRO_CARD_DETECTION_EVAL_WITNESS_INSPECT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(BalatroCardDetectionEvalWitnessInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!(
          "balatro card detection eval witness inspect mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<CardDetectionEvalWitnessInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::balatro::BALATRO_CARD_DETECTION_EVAL_WITNESS_INSPECT_ROLE,
    )
    .map(|report| BalatroCardDetectionEvalWitnessInspectReportSummary::from(&report));
    match parsed {
      Ok(report) => reports.push(BalatroCardDetectionEvalWitnessInspectReportLineage {
        artifact: artifact_ref,
        report: Some(report),
        issue: None,
      }),
      Err(error) => reports.push(BalatroCardDetectionEvalWitnessInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(error),
      }),
    }
  }
  Ok(reports)
}

pub(crate) fn extract_balatro_card_detection_quality_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<BalatroCardDetectionQualityManifestLineage>> {
  use auv_game_balatro::CardDetectionQualityManifest;
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::balatro::BALATRO_CARD_DETECTION_QUALITY_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(BalatroCardDetectionQualityManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!(
          "balatro card detection quality manifest mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<CardDetectionQualityManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::balatro::BALATRO_CARD_DETECTION_QUALITY_ROLE,
    )
    .map(|manifest| BalatroCardDetectionQualityManifestSummary::from(&manifest));
    match parsed {
      Ok(manifest) => manifests.push(BalatroCardDetectionQualityManifestLineage {
        artifact: artifact_ref,
        manifest: Some(manifest),
        issue: None,
      }),
      Err(error) => manifests.push(BalatroCardDetectionQualityManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(error),
      }),
    }
  }
  Ok(manifests)
}

pub(crate) fn extract_balatro_card_detection_quality_inspect_reports(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<BalatroCardDetectionQualityInspectReportLineage>> {
  use auv_game_balatro::CardDetectionQualityInspectReport;
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::balatro::BALATRO_CARD_DETECTION_QUALITY_INSPECT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(BalatroCardDetectionQualityInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!(
          "balatro card detection quality inspect mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<CardDetectionQualityInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::balatro::BALATRO_CARD_DETECTION_QUALITY_INSPECT_ROLE,
    )
    .map(|report| BalatroCardDetectionQualityInspectReportSummary::from(&report));
    match parsed {
      Ok(report) => reports.push(BalatroCardDetectionQualityInspectReportLineage {
        artifact: artifact_ref,
        report: Some(report),
        issue: None,
      }),
      Err(error) => reports.push(BalatroCardDetectionQualityInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(error),
      }),
    }
  }
  Ok(reports)
}

pub fn derive_osu_detection_eval_quality_verdict_summary(
  lineage: &OsuDetectionEvalQualityManifestLineage,
) -> OsuDetectionEvalQualityVerdictSummary {
  if let Some(issue) = &lineage.issue {
    return OsuDetectionEvalQualityVerdictSummary {
      verdict: "n/a".to_string(),
      derived_from_witness_status: "n/a".to_string(),
      issue: Some(issue.clone()),
    };
  }
  let Some(summary) = &lineage.manifest else {
    return OsuDetectionEvalQualityVerdictSummary {
      verdict: "n/a".to_string(),
      derived_from_witness_status: "n/a".to_string(),
      issue: Some("osu detection eval quality manifest summary missing".to_string()),
    };
  };
  OsuDetectionEvalQualityVerdictSummary {
    verdict: summary.verdict.clone(),
    derived_from_witness_status: summary.witness_status.clone(),
    issue: None,
  }
}

pub fn derive_osu_visual_truth_spatial_query_action_readiness(
  lineage: &OsuVisualTruthSpatialQueryManifestLineage,
) -> OsuVisualTruthSpatialQueryActionReadinessSummary {
  if let Some(issue) = &lineage.issue {
    return OsuVisualTruthSpatialQueryActionReadinessSummary {
      action_eligibility: "n/a".to_string(),
      pixel_point: None,
      refusal_reason: None,
      issue: Some(issue.clone()),
    };
  }
  let Some(summary) = &lineage.manifest else {
    return OsuVisualTruthSpatialQueryActionReadinessSummary {
      action_eligibility: "n/a".to_string(),
      pixel_point: None,
      refusal_reason: None,
      issue: Some("osu visual truth spatial query manifest summary missing".to_string()),
    };
  };
  let manifest = match osu_spatial_query_manifest_summary_for_action_readiness(summary) {
    Ok(manifest) => manifest,
    Err(error) => {
      return OsuVisualTruthSpatialQueryActionReadinessSummary {
        action_eligibility: "n/a".to_string(),
        pixel_point: None,
        refusal_reason: None,
        issue: Some(error),
      };
    }
  };
  let readiness = derive_visual_truth_spatial_query_action_readiness(&manifest);
  OsuVisualTruthSpatialQueryActionReadinessSummary {
    action_eligibility: readiness.eligibility.as_str().to_string(),
    pixel_point: readiness.pixel_point.map(|(x, y)| format!("{x},{y}")),
    refusal_reason: readiness.refusal_reason,
    issue: None,
  }
}

fn osu_spatial_query_manifest_summary_for_action_readiness(
  summary: &OsuVisualTruthSpatialQueryManifestSummary,
) -> Result<VisualTruthSpatialQueryManifest, String> {
  use auv_game_osu::{
    CapturePhase, ObjectKind, VisualTruthPixelVisibility, VisualTruthSpatialQueryBackend,
    VisualTruthSpatialQueryReason, VisualTruthSpatialQueryStatus,
  };
  let capture_phase = match summary.capture_phase.as_str() {
    "before_dispatch" => CapturePhase::BeforeDispatch,
    "after_dispatch" => CapturePhase::AfterDispatch,
    other => return Err(format!("unknown capture_phase {other}")),
  };
  let object_kind = match summary.object_kind.as_deref() {
    None => None,
    Some(kind) => Some(match kind {
      "circle" => ObjectKind::Circle,
      "slider" => ObjectKind::Slider,
      "spinner" => ObjectKind::Spinner,
      "hold" => ObjectKind::Hold,
      other => return Err(format!("unknown object_kind {other}")),
    }),
  };
  let status = match summary.status.as_str() {
    "answered" => VisualTruthSpatialQueryStatus::Answered,
    "blocked" => VisualTruthSpatialQueryStatus::Blocked,
    "failed" => VisualTruthSpatialQueryStatus::Failed,
    other => return Err(format!("unknown query status {other}")),
  };
  let reason = match summary.reason.as_deref() {
    None => None,
    Some(reason) => Some(match reason {
      "semantic_source_not_ready" => VisualTruthSpatialQueryReason::SemanticSourceNotReady,
      "target_absent_from_visual_truth" => {
        VisualTruthSpatialQueryReason::TargetAbsentFromVisualTruth
      }
      "projection_unavailable" => VisualTruthSpatialQueryReason::ProjectionUnavailable,
      other => return Err(format!("unknown query reason {other}")),
    }),
  };
  let pixel_visibility = match summary.pixel_visibility.as_deref() {
    None => None,
    Some(value) => Some(match value {
      "inside_capture" => VisualTruthPixelVisibility::InsideCapture,
      "outside_capture" => VisualTruthPixelVisibility::OutsideCapture,
      other => return Err(format!("unknown pixel_visibility {other}")),
    }),
  };
  let query_backend = match summary.query_backend.as_str() {
    "playfield_projection_reference" => {
      VisualTruthSpatialQueryBackend::PlayfieldProjectionReference
    }
    other => return Err(format!("unknown query backend {other}")),
  };
  Ok(VisualTruthSpatialQueryManifest {
    schema_version: summary.schema_version,
    generated_at_millis: 0,
    visual_truth_semantic_manifest_path: summary.visual_truth_semantic_manifest_path.clone(),
    source_run_artifact_dir: summary.source_run_artifact_dir.clone(),
    source_visual_truth_manifest_path: String::new(),
    source_projection_path: String::new(),
    object_index: summary.object_index,
    capture_phase,
    object_kind,
    query_backend,
    status,
    reason,
    pixel_visibility,
    pixel_x: summary.pixel_x,
    pixel_y: summary.pixel_y,
    match_radius_px: summary.match_radius_px,
    capture_width: summary.capture_width,
    capture_height: summary.capture_height,
    known_limits: summary.known_limits.clone(),
  })
}

pub(crate) fn list_minecraft_training_result_spatial_query_inspect_reports(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<MinecraftTrainingResultSpatialQueryInspectReportLineage>> {
  let run = store.read_run(run_id)?;
  extract_minecraft_training_result_spatial_query_inspect_reports(store, &run)
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

pub(crate) fn extract_minecraft_training_result_semantic_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<MinecraftTrainingResultSemanticManifestLineage>> {
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_SEMANTIC_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(MinecraftTrainingResultSemanticManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!(
          "minecraft training result semantic manifest mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingResultSemanticManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_SEMANTIC_ROLE,
    )
    .map(MinecraftTrainingResultSemanticManifestSummary::from);
    match parsed {
      Ok(manifest) => manifests.push(MinecraftTrainingResultSemanticManifestLineage {
        artifact: artifact_ref,
        manifest: Some(manifest),
        issue: None,
      }),
      Err(error) => manifests.push(MinecraftTrainingResultSemanticManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(error),
      }),
    }
  }
  Ok(manifests)
}

pub(crate) fn extract_minecraft_training_result_semantic_inspect_reports(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<MinecraftTrainingResultSemanticInspectReportLineage>> {
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_SEMANTIC_INSPECT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(MinecraftTrainingResultSemanticInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!(
          "minecraft training result semantic inspect mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingResultSemanticInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_SEMANTIC_INSPECT_ROLE,
    )
    .map(MinecraftTrainingResultSemanticInspectReportSummary::from);
    match parsed {
      Ok(report) => reports.push(MinecraftTrainingResultSemanticInspectReportLineage {
        artifact: artifact_ref,
        report: Some(report),
        issue: None,
      }),
      Err(error) => reports.push(MinecraftTrainingResultSemanticInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(error),
      }),
    }
  }
  Ok(reports)
}

pub(crate) fn extract_minecraft_training_result_spatial_query_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<MinecraftTrainingResultSpatialQueryManifestLineage>> {
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_QUERY_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(MinecraftTrainingResultSpatialQueryManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!(
          "minecraft training result spatial query manifest mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingResultSpatialQueryManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_QUERY_ROLE,
    )
    .map(MinecraftTrainingResultSpatialQueryManifestSummary::from);
    match parsed {
      Ok(manifest) => manifests.push(MinecraftTrainingResultSpatialQueryManifestLineage {
        artifact: artifact_ref,
        manifest: Some(manifest),
        issue: None,
      }),
      Err(error) => manifests.push(MinecraftTrainingResultSpatialQueryManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(error),
      }),
    }
  }
  Ok(manifests)
}

pub(crate) fn extract_minecraft_training_result_spatial_query_inspect_reports(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<MinecraftTrainingResultSpatialQueryInspectReportLineage>> {
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_QUERY_INSPECT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(MinecraftTrainingResultSpatialQueryInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!(
          "minecraft training result spatial query inspect mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingResultSpatialQueryInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_QUERY_INSPECT_ROLE,
    )
    .map(MinecraftTrainingResultSpatialQueryInspectReportSummary::from);
    match parsed {
      Ok(report) => reports.push(MinecraftTrainingResultSpatialQueryInspectReportLineage {
        artifact: artifact_ref,
        report: Some(report),
        issue: None,
      }),
      Err(error) => reports.push(MinecraftTrainingResultSpatialQueryInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(error),
      }),
    }
  }
  Ok(reports)
}

pub(crate) fn list_minecraft_training_result_holdout_preview_manifests(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<MinecraftTrainingResultHoldoutPreviewManifestLineage>> {
  let run = store.read_run(run_id)?;
  extract_minecraft_training_result_holdout_preview_manifests(store, &run)
}

pub(crate) fn list_minecraft_training_result_holdout_preview_inspect_reports(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<MinecraftTrainingResultHoldoutPreviewInspectReportLineage>> {
  let run = store.read_run(run_id)?;
  extract_minecraft_training_result_holdout_preview_inspect_reports(store, &run)
}

pub(crate) fn extract_minecraft_training_result_holdout_preview_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<MinecraftTrainingResultHoldoutPreviewManifestLineage>> {
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_HOLDOUT_PREVIEW_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(MinecraftTrainingResultHoldoutPreviewManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!(
          "minecraft training result holdout preview manifest mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingResultHoldoutPreviewManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_HOLDOUT_PREVIEW_ROLE,
    )
    .map(MinecraftTrainingResultHoldoutPreviewManifestSummary::from);
    match parsed {
      Ok(manifest) => manifests.push(MinecraftTrainingResultHoldoutPreviewManifestLineage {
        artifact: artifact_ref,
        manifest: Some(manifest),
        issue: None,
      }),
      Err(error) => manifests.push(MinecraftTrainingResultHoldoutPreviewManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(error),
      }),
    }
  }
  Ok(manifests)
}

pub(crate) fn extract_minecraft_training_result_holdout_preview_inspect_reports(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<MinecraftTrainingResultHoldoutPreviewInspectReportLineage>> {
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role
      != crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_HOLDOUT_PREVIEW_INSPECT_ROLE
    {
      continue;
    }
    let artifact_ref = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(MinecraftTrainingResultHoldoutPreviewInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!(
          "minecraft training result holdout preview inspect mime_type {} is not JSON",
          artifact.mime_type
        )),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingResultHoldoutPreviewInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_HOLDOUT_PREVIEW_INSPECT_ROLE,
    )
    .map(MinecraftTrainingResultHoldoutPreviewInspectReportSummary::from);
    match parsed {
      Ok(report) => reports.push(MinecraftTrainingResultHoldoutPreviewInspectReportLineage {
        artifact: artifact_ref,
        report: Some(report),
        issue: None,
      }),
      Err(error) => reports.push(MinecraftTrainingResultHoldoutPreviewInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(error),
      }),
    }
  }
  Ok(reports)
}

pub fn derive_minecraft_training_result_spatial_query_action_readiness(
  lineage: &MinecraftTrainingResultSpatialQueryManifestLineage,
) -> MinecraftTrainingResultSpatialQueryActionReadinessSummary {
  if let Some(issue) = &lineage.issue {
    return MinecraftTrainingResultSpatialQueryActionReadinessSummary {
      action_eligibility: "n/a".to_string(),
      readiness_class: None,
      window_point: None,
      refusal_reason: None,
      issue: Some(issue.clone()),
    };
  }
  let Some(manifest_summary) = &lineage.manifest else {
    return MinecraftTrainingResultSpatialQueryActionReadinessSummary {
      action_eligibility: "n/a".to_string(),
      readiness_class: None,
      window_point: None,
      refusal_reason: None,
      issue: Some("minecraft training result spatial query manifest summary missing".to_string()),
    };
  };
  let manifest = match spatial_query_manifest_summary_for_action_readiness(manifest_summary) {
    Ok(manifest) => manifest,
    Err(error) => {
      return MinecraftTrainingResultSpatialQueryActionReadinessSummary {
        action_eligibility: "n/a".to_string(),
        readiness_class: None,
        window_point: None,
        refusal_reason: None,
        issue: Some(error),
      };
    }
  };
  let readiness = derive_action_readiness(&manifest);
  let action_eligibility = readiness.eligibility.as_str().to_string();
  MinecraftTrainingResultSpatialQueryActionReadinessSummary {
    readiness_class: map_action_eligibility_to_readiness_class(&action_eligibility),
    action_eligibility,
    window_point: readiness.window_point.map(|point| {
      let point = auv_driver::geometry::Point::from(point);
      format!("{},{}", point.x, point.y)
    }),
    refusal_reason: readiness.refusal_reason,
    issue: None,
  }
}

fn parse_event_message_field(message: &str, key: &str) -> Option<String> {
  if key == "refusal_reason" {
    return parse_event_message_field_until(message, key, &["query_manifest_path"]);
  }
  let prefix = format!("{key}=");
  for token in message.split_whitespace() {
    if let Some(value) = token.strip_prefix(&prefix) {
      return Some(value.to_string());
    }
  }
  None
}

fn parse_event_message_field_until(message: &str, key: &str, stop_keys: &[&str]) -> Option<String> {
  let prefix = format!("{key}=");
  let start = message.find(&prefix)?;
  let rest = &message[start + prefix.len()..];
  let mut end = rest.len();
  for stop in stop_keys {
    if let Some(idx) = rest.find(&format!(" {stop}=")) {
      end = end.min(idx);
    }
  }
  let value = rest[..end].trim();
  if value.is_empty() {
    None
  } else {
    Some(value.to_string())
  }
}

fn operation_status_label(status: OperationStatus) -> &'static str {
  match status {
    OperationStatus::Completed => "completed",
    OperationStatus::Failed => "failed",
  }
}

fn operation_acknowledged_message(output: &OperationOutput) -> Option<String> {
  match output {
    OperationOutput::Acknowledged { message } => message.clone(),
    _ => None,
  }
}

fn query_artifact_id_from_operation_result(operation_result: &OperationResult) -> Option<String> {
  operation_result
    .evidence_artifacts
    .first()
    .map(|artifact| artifact.artifact_id.as_str().to_string())
    .or_else(|| {
      operation_result
        .freshness_basis
        .as_ref()
        .and_then(|basis| basis.source_artifact.as_ref())
        .map(|artifact| artifact.artifact_id.as_str().to_string())
    })
}

fn find_query_wired_live_action_operation_result(
  store: &LocalStore,
  run: &CanonicalRun,
) -> Option<(ArtifactRefLineage, OperationResult)> {
  for artifact in &run.artifacts {
    if artifact.role != "operation-result" || !is_json_mime(&artifact.mime_type) {
      continue;
    }
    let parsed = read_artifact_json::<OperationResult>(
      store,
      run.run.run_id.as_str(),
      artifact,
      "operation-result",
    )
    .ok()?;
    if parsed.operation_id == QUERY_WIRED_LIVE_ACTION_OPERATION_ID {
      return Some((
        artifact_record_lineage(run.run.run_id.clone(), artifact),
        parsed,
      ));
    }
  }
  None
}

fn derive_dispatch_evidence_from_events(run: &CanonicalRun) -> (Option<String>, Option<String>) {
  let mut dispatch_command = None;
  let mut dispatch_outcome = None;
  for event in &run.events {
    if event.name == "command.resolved"
      && event.message.as_deref() == Some("resolved input.clickWindowPoint")
    {
      dispatch_command = Some("input.clickWindowPoint".to_string());
      dispatch_outcome = Some("resolved".to_string());
    }
    if event.name == "command.failed" && dispatch_command.is_some() {
      if let Some(message) = event.message.as_deref() {
        dispatch_outcome = Some(format!("failed: {message}"));
      }
    }
  }
  (dispatch_command, dispatch_outcome)
}

// NOTICE(core-c2-d1): reader-side vocabulary only — Core-C1 table in
// docs/ai/references/2026-06-28-auv-core-c1-action-attempt-admission-design.md.
// Do not promote to shared crate/module in D1.
fn map_action_eligibility_to_readiness_class(donor: &str) -> Option<String> {
  match donor {
    "click_ready" => Some("ready".to_string()),
    "answer_non_clickable" => Some("non_actionable".to_string()),
    "not_consumable" => Some("not_consumable".to_string()),
    _ => None,
  }
}

// NOTICE(core-c2-d2): reader-side provenance only — Core-C1 source_readiness_ref.
fn format_source_readiness_ref(parts: &[(&str, &str)]) -> String {
  parts
    .iter()
    .filter(|(_, value)| !value.is_empty())
    .map(|(key, value)| format!("{key}={value}"))
    .collect::<Vec<_>>()
    .join(" ")
}

fn format_query_manifest_source_readiness_ref(artifact_id: &str, run_id: &str) -> String {
  format_source_readiness_ref(&[
    ("kind", "query_manifest"),
    ("artifact_id", artifact_id),
    ("run_id", run_id),
  ])
}

fn format_derived_readiness_source_readiness_ref(query_artifact_id: &str, run_id: &str) -> String {
  format_source_readiness_ref(&[
    ("kind", "derived_readiness"),
    ("query_artifact_id", query_artifact_id),
    ("run_id", run_id),
  ])
}

fn format_outcome_event_source_readiness_ref(
  event_name: &str,
  operation_result_artifact_id: Option<&str>,
) -> String {
  let mut parts = vec![("kind", "outcome_event"), ("event", event_name)];
  if let Some(operation_result_artifact_id) =
    operation_result_artifact_id.filter(|artifact_id| !artifact_id.is_empty())
  {
    parts.push(("operation_result_artifact_id", operation_result_artifact_id));
  }
  format_source_readiness_ref(&parts)
}

enum SourceReadinessManifestLookup {
  MatchedValidManifest { artifact_id: String },
  CleanMiss,
  MatchedParseFailure,
}

fn classify_minecraft_manifest_source_readiness_lookup(
  query_id: &str,
  extract_result: &AuvResult<Vec<MinecraftTrainingResultSpatialQueryManifestLineage>>,
) -> Option<SourceReadinessManifestLookup> {
  match extract_result {
    Err(_) => None,
    Ok(manifests) => {
      let matching = manifests
        .iter()
        .find(|manifest| manifest.artifact.artifact_id.as_str() == query_id);
      Some(match matching {
        None => SourceReadinessManifestLookup::CleanMiss,
        Some(lineage) if lineage.manifest.is_some() => {
          SourceReadinessManifestLookup::MatchedValidManifest {
            artifact_id: lineage.artifact.artifact_id.as_str().to_string(),
          }
        }
        Some(_) => SourceReadinessManifestLookup::MatchedParseFailure,
      })
    }
  }
}

fn classify_osu_manifest_source_readiness_lookup(
  query_id: &str,
  extract_result: &AuvResult<Vec<OsuVisualTruthSpatialQueryManifestLineage>>,
) -> Option<SourceReadinessManifestLookup> {
  match extract_result {
    Err(_) => None,
    Ok(manifests) => {
      let matching = manifests
        .iter()
        .find(|manifest| manifest.artifact.artifact_id.as_str() == query_id);
      Some(match matching {
        None => SourceReadinessManifestLookup::CleanMiss,
        Some(lineage) if lineage.manifest.is_some() => {
          SourceReadinessManifestLookup::MatchedValidManifest {
            artifact_id: lineage.artifact.artifact_id.as_str().to_string(),
          }
        }
        Some(_) => SourceReadinessManifestLookup::MatchedParseFailure,
      })
    }
  }
}

fn resolve_query_wired_live_action_source_readiness_ref(
  run_id: &str,
  query_artifact_id: Option<&str>,
  operation_result_artifact_id: Option<&str>,
  outcome_event_name: &str,
  has_outcome_event: bool,
  manifest_lookup: Option<SourceReadinessManifestLookup>,
) -> Option<String> {
  if let Some(query_id) = query_artifact_id {
    return match manifest_lookup? {
      SourceReadinessManifestLookup::MatchedValidManifest { artifact_id } => Some(
        format_query_manifest_source_readiness_ref(artifact_id.as_str(), run_id),
      ),
      SourceReadinessManifestLookup::CleanMiss => Some(
        format_derived_readiness_source_readiness_ref(query_id, run_id),
      ),
      SourceReadinessManifestLookup::MatchedParseFailure => None,
    };
  }
  if has_outcome_event {
    return Some(format_outcome_event_source_readiness_ref(
      outcome_event_name,
      operation_result_artifact_id,
    ));
  }
  None
}

// NOTICE(core-c3-d2): reader-side Layer 3 summary only — verification_outcome projection.
#[derive(Clone, Debug, PartialEq, Eq)]
struct QueryWiredLiveActionVerificationProjection {
  verification_outcome: String,
  verification_source: Option<String>,
  verification_reason: Option<String>,
}

fn format_operation_result_verification_source(artifact_id: &str, run_id: &str) -> String {
  format_source_readiness_ref(&[
    ("kind", "operation_result"),
    ("artifact_id", artifact_id),
    ("run_id", run_id),
  ])
}

fn operation_result_verification_claims(
  operation_result: &OperationResult,
) -> Vec<&VerificationResult> {
  if !operation_result.verifications.is_empty() {
    return operation_result.verifications.iter().collect();
  }
  if let OperationOutput::Verification { verification } = &operation_result.output {
    return vec![verification];
  }
  Vec::new()
}

fn is_activation_only_verification(verification: &VerificationResult) -> bool {
  matches!(
    &verification.method,
    VerificationMethod::Custom { name } if name == "activation_only"
  )
}

fn verification_failure_layer_label(layer: FailureLayer) -> &'static str {
  match layer {
    FailureLayer::GroundingFailed => "grounding_failed",
    FailureLayer::CandidateExpired => "candidate_expired",
    FailureLayer::ControlFailed => "control_failed",
    FailureLayer::VerificationUnreliable => "verification_unreliable",
    FailureLayer::StateChangedNoMatch => "state_changed_no_match",
    FailureLayer::SemanticMismatch => "semantic_mismatch",
  }
}

fn verification_claim_reason_snippet(verification: &VerificationResult) -> Option<String> {
  if let Some(observed_label) = verification
    .observed_label
    .as_deref()
    .filter(|label| !label.is_empty())
  {
    return Some(observed_label.to_string());
  }
  verification
    .failure_layer
    .map(verification_failure_layer_label)
    .map(str::to_string)
}

fn build_verification_reason_from_claims(claims: &[&VerificationResult]) -> Option<String> {
  let mut parts = claims
    .iter()
    .filter_map(|claim| verification_claim_reason_snippet(claim))
    .collect::<Vec<_>>();
  parts.dedup();
  if parts.is_empty() {
    None
  } else {
    Some(parts.join("; "))
  }
}

fn project_verification_outcome_from_claims(
  claims: &[&VerificationResult],
) -> (String, Option<String>) {
  let semantic_claims = claims
    .iter()
    .copied()
    .filter(|claim| !is_activation_only_verification(claim))
    .collect::<Vec<_>>();
  let focus: &[&VerificationResult] = if semantic_claims.is_empty() {
    claims
  } else {
    &semantic_claims
  };

  for claim in focus {
    if matches!(
      claim.failure_layer,
      Some(FailureLayer::VerificationUnreliable)
    ) {
      return (
        "unreliable".to_string(),
        build_verification_reason_from_claims(focus),
      );
    }
  }

  for claim in focus {
    if matches!(
      claim.failure_layer,
      Some(FailureLayer::SemanticMismatch | FailureLayer::StateChangedNoMatch)
    ) || claim.semantic_matched == Some(false)
    {
      return (
        "failed".to_string(),
        build_verification_reason_from_claims(focus),
      );
    }
  }

  if focus
    .iter()
    .all(|claim| is_activation_only_verification(claim))
  {
    return (
      "activation_only".to_string(),
      build_verification_reason_from_claims(focus)
        .or_else(|| Some("input delivery recorded; no semantic post-action assertion".to_string())),
    );
  }

  if focus
    .iter()
    .any(|claim| claim.semantic_matched == Some(true))
  {
    return (
      "passed".to_string(),
      build_verification_reason_from_claims(focus),
    );
  }

  if focus
    .iter()
    .any(|claim| claim.state_changed && claim.semantic_matched.is_none())
  {
    return (
      "inconclusive".to_string(),
      build_verification_reason_from_claims(focus),
    );
  }

  (
    "absent".to_string(),
    Some("verification claims present but not mappable to a read-side outcome".to_string()),
  )
}

fn resolve_query_wired_live_action_verification_projection(
  attempted: bool,
  operation_result_artifact_id: Option<&str>,
  operation_result: Option<&OperationResult>,
  run_id: &str,
  refusal_reason: Option<&str>,
) -> QueryWiredLiveActionVerificationProjection {
  if !attempted {
    return QueryWiredLiveActionVerificationProjection {
      verification_outcome: "not_attempted".to_string(),
      verification_source: Some(format_source_readiness_ref(&[(
        "kind",
        "layer1_no_dispatch",
      )])),
      verification_reason: refusal_reason
        .filter(|reason| !reason.is_empty() && *reason != "none")
        .map(str::to_string)
        .or_else(|| Some("post-action verification N/A; action not dispatched".to_string())),
    };
  }

  let Some(operation_result) = operation_result else {
    return QueryWiredLiveActionVerificationProjection {
      verification_outcome: "absent".to_string(),
      verification_source: None,
      verification_reason: Some(
        "attempted=true but operation-result artifact missing on read path".to_string(),
      ),
    };
  };

  let verification_source = operation_result_artifact_id
    .map(|artifact_id| format_operation_result_verification_source(artifact_id, run_id));
  let claims = operation_result_verification_claims(operation_result);
  if claims.is_empty() {
    return QueryWiredLiveActionVerificationProjection {
      verification_outcome: "absent".to_string(),
      verification_source,
      verification_reason: operation_result.known_limits.first().cloned().or_else(|| {
        Some("no VerificationResult on operation-result; Layer 3 evidence absent".to_string())
      }),
    };
  }

  let (verification_outcome, verification_reason) =
    project_verification_outcome_from_claims(&claims);
  QueryWiredLiveActionVerificationProjection {
    verification_outcome,
    verification_source,
    verification_reason,
  }
}

pub fn derive_minecraft_query_wired_live_action_summary(
  store: &LocalStore,
  run: &CanonicalRun,
) -> Option<MinecraftQueryWiredLiveActionSummary> {
  let outcome_event = run
    .events
    .iter()
    .find(|event| event.name == "minecraft.query_wired_live_action.outcome");
  let operation_result_pair = find_query_wired_live_action_operation_result(store, run);
  if outcome_event.is_none() && operation_result_pair.is_none() {
    return None;
  }

  let mut issue = None;
  let (attempted, action_eligibility, refusal_reason) = if let Some(event) = outcome_event {
    let message = event.message.as_deref().unwrap_or("");
    let attempted =
      parse_event_message_field(message, "attempted").is_some_and(|value| value == "true");
    let action_eligibility =
      parse_event_message_field(message, "action_eligibility").unwrap_or_else(|| "n/a".to_string());
    let refusal_reason =
      parse_event_message_field(message, "refusal_reason").filter(|value| value != "none");
    (attempted, action_eligibility, refusal_reason)
  } else {
    (false, "n/a".to_string(), None)
  };

  let inputs_event = run
    .events
    .iter()
    .find(|event| event.name == "minecraft.query_wired_live_action.inputs");
  let target_app = inputs_event
    .and_then(|event| event.message.as_deref())
    .and_then(|message| parse_event_message_field(message, "target_app"));
  let target_title = inputs_event
    .and_then(|event| event.message.as_deref())
    .and_then(|message| parse_event_message_field(message, "target_title"));

  let (operation_result_artifact_id, query_artifact_id, operation_status, operation_message) =
    if let Some((ref artifact_ref, ref operation_result)) = operation_result_pair {
      (
        Some(artifact_ref.artifact_id.as_str().to_string()),
        query_artifact_id_from_operation_result(operation_result),
        Some(operation_status_label(operation_result.status).to_string()),
        operation_acknowledged_message(&operation_result.output),
      )
    } else {
      (None, None, None, None)
    };

  let (dispatch_command, dispatch_outcome) = derive_dispatch_evidence_from_events(run);

  let run_id = run.run.run_id.as_str();
  let manifest_extract = query_artifact_id
    .as_deref()
    .map(|_| extract_minecraft_training_result_spatial_query_manifests(store, run));

  let mut window_point = None;
  let mut mc14_action_eligibility = None;
  if let Some(query_id) = query_artifact_id.as_deref() {
    if let Some(Ok(ref manifests)) = manifest_extract {
      if let Some(lineage) = manifests
        .iter()
        .find(|manifest| manifest.artifact.artifact_id.as_str() == query_id)
      {
        let readiness = derive_minecraft_training_result_spatial_query_action_readiness(lineage);
        mc14_action_eligibility = Some(readiness.action_eligibility);
        window_point = readiness.window_point;
        if readiness.issue.is_some() {
          issue = readiness.issue;
        }
      }
    }
  }

  let readiness_donor = mc14_action_eligibility
    .as_deref()
    .unwrap_or(action_eligibility.as_str());
  let readiness_class = map_action_eligibility_to_readiness_class(readiness_donor);
  let manifest_lookup = query_artifact_id.as_deref().and_then(|query_id| {
    manifest_extract.as_ref().and_then(|extract_result| {
      classify_minecraft_manifest_source_readiness_lookup(query_id, extract_result)
    })
  });
  let source_readiness_ref = resolve_query_wired_live_action_source_readiness_ref(
    run_id,
    query_artifact_id.as_deref(),
    operation_result_artifact_id.as_deref(),
    "minecraft.query_wired_live_action.outcome",
    outcome_event.is_some(),
    manifest_lookup,
  );
  let operation_result_ref = operation_result_pair.as_ref().map(|(_, result)| result);
  let verification_projection = resolve_query_wired_live_action_verification_projection(
    attempted,
    operation_result_artifact_id.as_deref(),
    operation_result_ref,
    run_id,
    refusal_reason.as_deref(),
  );

  Some(MinecraftQueryWiredLiveActionSummary {
    operation_result_artifact_id,
    query_artifact_id,
    attempted,
    action_eligibility,
    window_point,
    refusal_reason,
    operation_status,
    operation_message,
    target_app,
    target_title,
    dispatch_command,
    dispatch_outcome,
    mc14_action_eligibility,
    readiness_class,
    source_readiness_ref,
    verification_outcome: verification_projection.verification_outcome,
    verification_source: verification_projection.verification_source,
    verification_reason: verification_projection.verification_reason,
    issue,
  })
}

fn find_osu_query_wired_live_action_operation_result(
  store: &LocalStore,
  run: &CanonicalRun,
) -> Option<(ArtifactRefLineage, OperationResult)> {
  for artifact in &run.artifacts {
    if artifact.role != "operation-result" || !is_json_mime(&artifact.mime_type) {
      continue;
    }
    let parsed = read_artifact_json::<OperationResult>(
      store,
      run.run.run_id.as_str(),
      artifact,
      "operation-result",
    )
    .ok()?;
    if parsed.operation_id == OSU_QUERY_WIRED_LIVE_ACTION_OPERATION_ID {
      return Some((
        artifact_record_lineage(run.run.run_id.clone(), artifact),
        parsed,
      ));
    }
  }
  None
}

pub fn derive_osu_query_wired_live_action_summary(
  store: &LocalStore,
  run: &CanonicalRun,
) -> Option<OsuQueryWiredLiveActionSummary> {
  let outcome_event = run
    .events
    .iter()
    .find(|event| event.name == "osu.query_wired_live_action.outcome");
  let operation_result_pair = find_osu_query_wired_live_action_operation_result(store, run);
  if outcome_event.is_none() && operation_result_pair.is_none() {
    return None;
  }

  let mut issue = None;
  let (attempted, action_eligibility, refusal_reason, pixel_point, window_point) =
    if let Some(event) = outcome_event {
      let message = event.message.as_deref().unwrap_or("");
      let attempted =
        parse_event_message_field(message, "attempted").is_some_and(|value| value == "true");
      let action_eligibility = parse_event_message_field(message, "action_eligibility")
        .unwrap_or_else(|| "n/a".to_string());
      let refusal_reason =
        parse_event_message_field(message, "refusal_reason").filter(|value| value != "none");
      let pixel_point =
        parse_event_message_field(message, "pixel_point").filter(|value| value != "none");
      let window_point =
        parse_event_message_field(message, "window_point").filter(|value| value != "none");
      (
        attempted,
        action_eligibility,
        refusal_reason,
        pixel_point,
        window_point,
      )
    } else {
      (false, "n/a".to_string(), None, None, None)
    };

  let inputs_event = run
    .events
    .iter()
    .find(|event| event.name == "osu.query_wired_live_action.inputs");
  let target_app = inputs_event
    .and_then(|event| event.message.as_deref())
    .and_then(|message| parse_event_message_field(message, "target_app"));
  let target_title = inputs_event
    .and_then(|event| event.message.as_deref())
    .and_then(|message| parse_event_message_field(message, "target_title"));

  let (operation_result_artifact_id, query_artifact_id, operation_status, operation_message) =
    if let Some((ref artifact_ref, ref operation_result)) = operation_result_pair {
      (
        Some(artifact_ref.artifact_id.as_str().to_string()),
        query_artifact_id_from_operation_result(operation_result),
        Some(operation_status_label(operation_result.status).to_string()),
        operation_acknowledged_message(&operation_result.output),
      )
    } else {
      (None, None, None, None)
    };

  let (dispatch_command, dispatch_outcome) = derive_dispatch_evidence_from_events(run);

  let run_id = run.run.run_id.as_str();
  let manifest_extract = query_artifact_id
    .as_deref()
    .map(|_| extract_osu_visual_truth_spatial_query_manifests(store, run));

  let mut readiness_class = map_action_eligibility_to_readiness_class(&action_eligibility);
  if let Some(query_id) = query_artifact_id.as_deref() {
    if let Some(Ok(ref manifests)) = manifest_extract {
      if let Some(lineage) = manifests
        .iter()
        .find(|manifest| manifest.artifact.artifact_id.as_str() == query_id)
      {
        let readiness = derive_osu_visual_truth_spatial_query_action_readiness(lineage);
        readiness_class = map_action_eligibility_to_readiness_class(&readiness.action_eligibility);
        if readiness.issue.is_some() {
          issue = readiness.issue;
        }
      }
    }
  }

  let manifest_lookup = query_artifact_id.as_deref().and_then(|query_id| {
    manifest_extract.as_ref().and_then(|extract_result| {
      classify_osu_manifest_source_readiness_lookup(query_id, extract_result)
    })
  });
  let source_readiness_ref = resolve_query_wired_live_action_source_readiness_ref(
    run_id,
    query_artifact_id.as_deref(),
    operation_result_artifact_id.as_deref(),
    "osu.query_wired_live_action.outcome",
    outcome_event.is_some(),
    manifest_lookup,
  );
  let operation_result_ref = operation_result_pair.as_ref().map(|(_, result)| result);
  let verification_projection = resolve_query_wired_live_action_verification_projection(
    attempted,
    operation_result_artifact_id.as_deref(),
    operation_result_ref,
    run_id,
    refusal_reason.as_deref(),
  );

  Some(OsuQueryWiredLiveActionSummary {
    operation_result_artifact_id,
    query_artifact_id,
    attempted,
    action_eligibility,
    pixel_point,
    window_point,
    refusal_reason,
    operation_status,
    operation_message,
    target_app,
    target_title,
    dispatch_command,
    dispatch_outcome,
    readiness_class,
    source_readiness_ref,
    verification_outcome: verification_projection.verification_outcome,
    verification_source: verification_projection.verification_source,
    verification_reason: verification_projection.verification_reason,
    issue,
  })
}

pub(crate) fn list_osu_query_wired_live_action_summaries(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<OsuQueryWiredLiveActionSummary>> {
  let run = store.read_run(run_id)?;
  Ok(
    derive_osu_query_wired_live_action_summary(store, &run)
      .into_iter()
      .collect(),
  )
}

pub(crate) fn list_minecraft_query_wired_live_action_summaries(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<MinecraftQueryWiredLiveActionSummary>> {
  let run = store.read_run(run_id)?;
  Ok(
    derive_minecraft_query_wired_live_action_summary(store, &run)
      .into_iter()
      .collect(),
  )
}

pub const QUALITY_BASELINE_PROFILE_V1_JSON: &str =
  include_str!("../crates/auv-game-minecraft/tests/fixtures/mc17-d2/baseline-profile-v1.json");

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct QualityBaselineProfile {
  pub profile_id: String,
  pub training_result_semantic_manifest_path: String,
  pub query_target_block: String,
  pub query_target_face: Option<String>,
  pub query_target_semantics: String,
  pub holdout_frame_index: usize,
  pub basis_checkpoint_suffix: String,
  #[serde(default)]
  pub recorded_run_ids: Option<QualityBaselineRecordedRunIds>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub struct QualityBaselineRecordedRunIds {
  pub mc12: Option<String>,
  pub mc16: Option<String>,
  pub mc17: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QualityBaselineSpatialQueryEvidence {
  pub status: String,
  pub visibility: Option<String>,
  pub screen_point: Option<String>,
  pub selected_backend: Option<String>,
  pub comparison_verdict: Option<String>,
  pub basis_frame_id: Option<String>,
  pub target_block: String,
  pub target_face: Option<String>,
  pub target_semantics: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct QualityBaselineHoldoutWitnessEvidence {
  pub status: String,
  pub holdout_frame_index: usize,
  pub basis_checkpoint_path: Option<String>,
  pub holdout_screenshot_path: Option<String>,
  pub spatial_frame_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QualityBaselineRenderQualityEvidence {
  pub status: String,
  pub verdict: String,
  pub image_size_match: bool,
  pub l1_mean: Option<f64>,
  pub mse: Option<f64>,
  pub psnr: Option<f64>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MinecraftTrainingResultQualityBaselineReportSummary {
  pub profile_id: String,
  pub training_result_semantic_manifest_path: String,
  pub evidence_coverage: String,
  pub spatial_query: Option<QualityBaselineSpatialQueryEvidence>,
  pub holdout_witness: Option<QualityBaselineHoldoutWitnessEvidence>,
  pub render_quality: Option<QualityBaselineRenderQualityEvidence>,
  pub trust_notes: Vec<String>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct QualityBaselineEvidenceBundle {
  pub spatial_query: Option<MinecraftTrainingResultSpatialQueryManifestSummary>,
  pub holdout_preview: Option<MinecraftTrainingResultHoldoutPreviewManifestSummary>,
  pub render_quality: Option<MinecraftHoldoutRenderQualityManifestSummary>,
  pub collection_issues: Vec<String>,
}

pub fn quality_baseline_report_for_run(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<MinecraftTrainingResultQualityBaselineReportSummary> {
  let profile = quality_baseline_profile_v1()?;
  let bundle = collect_quality_baseline_evidence_for_run(store, run_id, &profile)?;
  Ok(derive_minecraft_training_result_quality_baseline_report(
    &profile,
    bundle.spatial_query.as_ref(),
    bundle.holdout_preview.as_ref(),
    bundle.render_quality.as_ref(),
    &bundle.collection_issues,
  ))
}

pub fn quality_baseline_profile_v1() -> Result<QualityBaselineProfile, String> {
  serde_json::from_str(QUALITY_BASELINE_PROFILE_V1_JSON)
    .map_err(|error| format!("parse quality baseline profile v1 fixture: {error}"))
}

fn spatial_query_matches_profile(
  manifest: &MinecraftTrainingResultSpatialQueryManifestSummary,
  profile: &QualityBaselineProfile,
) -> bool {
  manifest.training_result_semantic_manifest_path == profile.training_result_semantic_manifest_path
    && manifest.target_block == profile.query_target_block
    && manifest.target_face.as_deref() == profile.query_target_face.as_deref()
    && manifest.target_semantics == profile.query_target_semantics
}

fn holdout_preview_matches_profile(
  manifest: &MinecraftTrainingResultHoldoutPreviewManifestSummary,
  profile: &QualityBaselineProfile,
) -> bool {
  manifest.training_result_semantic_manifest_path == profile.training_result_semantic_manifest_path
    && manifest.holdout_frame_index == profile.holdout_frame_index
    && manifest
      .basis_checkpoint_path
      .as_deref()
      .is_some_and(|path| path.ends_with(&profile.basis_checkpoint_suffix))
}

fn holdout_render_quality_matches_profile(
  manifest: &MinecraftHoldoutRenderQualityManifestSummary,
  profile: &QualityBaselineProfile,
) -> bool {
  manifest.training_result_semantic_manifest_path == profile.training_result_semantic_manifest_path
    && manifest.holdout_frame_index == profile.holdout_frame_index
    && manifest
      .basis_checkpoint_path
      .as_deref()
      .is_some_and(|path| path.ends_with(&profile.basis_checkpoint_suffix))
}

fn spatial_query_evidence_from_summary(
  summary: &MinecraftTrainingResultSpatialQueryManifestSummary,
) -> QualityBaselineSpatialQueryEvidence {
  QualityBaselineSpatialQueryEvidence {
    status: summary.status.clone(),
    visibility: summary.visibility.clone(),
    screen_point: summary.screen_point.clone(),
    selected_backend: summary.selected_backend.clone(),
    comparison_verdict: summary.comparison_verdict.clone(),
    basis_frame_id: summary.basis_frame_id.clone(),
    target_block: summary.target_block.clone(),
    target_face: summary.target_face.clone(),
    target_semantics: summary.target_semantics.clone(),
  }
}

fn holdout_witness_evidence_from_summary(
  summary: &MinecraftTrainingResultHoldoutPreviewManifestSummary,
) -> QualityBaselineHoldoutWitnessEvidence {
  QualityBaselineHoldoutWitnessEvidence {
    status: summary.status.clone(),
    holdout_frame_index: summary.holdout_frame_index,
    basis_checkpoint_path: summary.basis_checkpoint_path.clone(),
    holdout_screenshot_path: summary.holdout_screenshot_path.clone(),
    spatial_frame_id: summary
      .holdout_frame
      .as_ref()
      .map(|frame| frame.spatial_frame_id.clone()),
  }
}

fn render_quality_evidence_from_summary(
  summary: &MinecraftHoldoutRenderQualityManifestSummary,
) -> QualityBaselineRenderQualityEvidence {
  let (l1_mean, mse, psnr) = summary
    .metrics
    .as_ref()
    .map(|metrics| (metrics.l1_mean, metrics.mse, metrics.psnr))
    .unwrap_or((None, None, None));
  QualityBaselineRenderQualityEvidence {
    status: summary.status.clone(),
    verdict: summary.verdict.clone(),
    image_size_match: summary.image_size_match,
    l1_mean,
    mse,
    psnr,
    known_limits: summary.known_limits.clone(),
  }
}

fn build_quality_baseline_trust_notes(
  render_quality: Option<&QualityBaselineRenderQualityEvidence>,
) -> Vec<String> {
  let mut notes = vec![
    "MC-12 projection_reference answers are scene-packet reference geometry only; they are not Gaussian-native inference".to_string(),
    "MC-17 screenshot-copy render probe measures pipeline comparability only; it is not trained-splat usefulness evidence".to_string(),
  ];
  if let Some(render) = render_quality {
    for limit in &render.known_limits {
      if !notes.iter().any(|note| note == limit) {
        notes.push(limit.clone());
      }
    }
  }
  notes
}

pub fn derive_minecraft_training_result_quality_baseline_report(
  profile: &QualityBaselineProfile,
  spatial_query: Option<&MinecraftTrainingResultSpatialQueryManifestSummary>,
  holdout_preview: Option<&MinecraftTrainingResultHoldoutPreviewManifestSummary>,
  render_quality: Option<&MinecraftHoldoutRenderQualityManifestSummary>,
  collection_issues: &[String],
) -> MinecraftTrainingResultQualityBaselineReportSummary {
  let mut issues = collection_issues.to_vec();
  let spatial_query_evidence = spatial_query.map(spatial_query_evidence_from_summary);
  let holdout_witness_evidence = holdout_preview.map(holdout_witness_evidence_from_summary);
  let render_quality_evidence = render_quality.map(render_quality_evidence_from_summary);

  if let Some(manifest) = spatial_query {
    if !spatial_query_matches_profile(manifest, profile) {
      issues.push("spatial query manifest does not match baseline profile pins".to_string());
    }
  }
  if let Some(manifest) = holdout_preview {
    if !holdout_preview_matches_profile(manifest, profile) {
      issues.push("holdout preview manifest does not match baseline profile pins".to_string());
    }
  }
  if let Some(manifest) = render_quality {
    if !holdout_render_quality_matches_profile(manifest, profile) {
      issues
        .push("holdout render quality manifest does not match baseline profile pins".to_string());
    }
  }

  let stage_count = [
    spatial_query_evidence.is_some(),
    holdout_witness_evidence.is_some(),
    render_quality_evidence.is_some(),
  ]
  .into_iter()
  .filter(|present| *present)
  .count();

  let evidence_coverage = if stage_count == 0 {
    "missing_stage".to_string()
  } else if stage_count == 3 && issues.is_empty() {
    "complete".to_string()
  } else {
    "partial".to_string()
  };

  let trust_notes = build_quality_baseline_trust_notes(render_quality_evidence.as_ref());
  let issue = if issues.is_empty() {
    None
  } else {
    Some(issues.join(" | "))
  };

  MinecraftTrainingResultQualityBaselineReportSummary {
    profile_id: profile.profile_id.clone(),
    training_result_semantic_manifest_path: profile.training_result_semantic_manifest_path.clone(),
    evidence_coverage,
    spatial_query: spatial_query_evidence,
    holdout_witness: holdout_witness_evidence,
    render_quality: render_quality_evidence,
    trust_notes,
    issue,
  }
}

fn read_holdout_preview_summary_from_path(
  path: &str,
) -> Result<MinecraftTrainingResultHoldoutPreviewManifestSummary, String> {
  let bytes = fs::read_to_string(path)
    .map_err(|error| format!("read holdout preview manifest at {path}: {error}"))?;
  let manifest: TrainingResultHoldoutPreviewManifest = serde_json::from_str(&bytes)
    .map_err(|error| format!("parse holdout preview manifest at {path}: {error}"))?;
  Ok(MinecraftTrainingResultHoldoutPreviewManifestSummary::from(
    manifest,
  ))
}

fn select_matching_spatial_query_manifest(
  manifests: &[MinecraftTrainingResultSpatialQueryManifestLineage],
  profile: &QualityBaselineProfile,
) -> Option<MinecraftTrainingResultSpatialQueryManifestSummary> {
  manifests
    .iter()
    .filter_map(|lineage| lineage.manifest.as_ref())
    .find(|manifest| spatial_query_matches_profile(manifest, profile))
    .cloned()
}

fn select_matching_holdout_preview_manifest(
  manifests: &[MinecraftTrainingResultHoldoutPreviewManifestLineage],
  profile: &QualityBaselineProfile,
) -> Option<MinecraftTrainingResultHoldoutPreviewManifestSummary> {
  manifests
    .iter()
    .filter_map(|lineage| lineage.manifest.as_ref())
    .find(|manifest| holdout_preview_matches_profile(manifest, profile))
    .cloned()
}

fn select_matching_holdout_render_quality_manifest(
  manifests: &[MinecraftHoldoutRenderQualityManifestLineage],
  profile: &QualityBaselineProfile,
) -> Option<MinecraftHoldoutRenderQualityManifestSummary> {
  manifests
    .iter()
    .filter_map(|lineage| lineage.manifest.as_ref())
    .find(|manifest| holdout_render_quality_matches_profile(manifest, profile))
    .cloned()
    .or_else(|| {
      manifests
        .iter()
        .filter_map(|lineage| lineage.manifest.as_ref())
        .find(|manifest| {
          manifest.training_result_semantic_manifest_path
            == profile.training_result_semantic_manifest_path
        })
        .cloned()
    })
}

fn find_spatial_query_manifest_in_run(
  store: &LocalStore,
  run_id: &str,
  profile: &QualityBaselineProfile,
) -> Option<MinecraftTrainingResultSpatialQueryManifestSummary> {
  let manifests = list_minecraft_training_result_spatial_query_manifests(store, run_id).ok()?;
  select_matching_spatial_query_manifest(&manifests, profile)
}

fn find_holdout_preview_manifest_in_run(
  store: &LocalStore,
  run_id: &str,
  profile: &QualityBaselineProfile,
) -> Option<MinecraftTrainingResultHoldoutPreviewManifestSummary> {
  let manifests = list_minecraft_training_result_holdout_preview_manifests(store, run_id).ok()?;
  select_matching_holdout_preview_manifest(&manifests, profile)
}

fn find_spatial_query_manifest_in_store(
  store: &LocalStore,
  profile: &QualityBaselineProfile,
) -> Option<MinecraftTrainingResultSpatialQueryManifestSummary> {
  if let Some(run_id) = profile
    .recorded_run_ids
    .as_ref()
    .and_then(|ids| ids.mc12.as_deref())
  {
    if let Some(manifest) = find_spatial_query_manifest_in_run(store, run_id, profile) {
      return Some(manifest);
    }
  }
  let runs = store.list_runs().ok()?;
  for run in runs.iter().rev() {
    if let Some(manifest) = find_spatial_query_manifest_in_run(store, run.run_id.as_str(), profile)
    {
      return Some(manifest);
    }
  }
  None
}

fn find_holdout_preview_manifest_in_store(
  store: &LocalStore,
  profile: &QualityBaselineProfile,
) -> Option<MinecraftTrainingResultHoldoutPreviewManifestSummary> {
  if let Some(run_id) = profile
    .recorded_run_ids
    .as_ref()
    .and_then(|ids| ids.mc16.as_deref())
  {
    if let Some(manifest) = find_holdout_preview_manifest_in_run(store, run_id, profile) {
      return Some(manifest);
    }
  }
  let runs = store.list_runs().ok()?;
  for run in runs.iter().rev() {
    if let Some(manifest) =
      find_holdout_preview_manifest_in_run(store, run.run_id.as_str(), profile)
    {
      return Some(manifest);
    }
  }
  None
}

pub fn collect_quality_baseline_evidence_for_run(
  store: &LocalStore,
  run_id: &str,
  profile: &QualityBaselineProfile,
) -> AuvResult<QualityBaselineEvidenceBundle> {
  let spatial_query_manifests =
    list_minecraft_training_result_spatial_query_manifests(store, run_id)?;
  let holdout_preview_manifests =
    list_minecraft_training_result_holdout_preview_manifests(store, run_id)?;
  let render_quality_manifests = list_minecraft_holdout_render_quality_manifests(store, run_id)?;

  let render_quality =
    select_matching_holdout_render_quality_manifest(&render_quality_manifests, profile);

  let mut collection_issues = Vec::new();

  let mut holdout_preview =
    select_matching_holdout_preview_manifest(&holdout_preview_manifests, profile);
  if holdout_preview.is_none() {
    holdout_preview = find_holdout_preview_manifest_in_store(store, profile);
  }
  if holdout_preview.is_none() {
    if let Some(render) = &render_quality {
      match read_holdout_preview_summary_from_path(render.holdout_preview_manifest_path.as_str()) {
        Ok(summary) => holdout_preview = Some(summary),
        Err(error) => collection_issues.push(error),
      }
    }
  }

  let mut spatial_query = select_matching_spatial_query_manifest(&spatial_query_manifests, profile);
  if spatial_query.is_none() {
    spatial_query = find_spatial_query_manifest_in_store(store, profile);
  }

  Ok(QualityBaselineEvidenceBundle {
    spatial_query,
    holdout_preview,
    render_quality,
    collection_issues,
  })
}

pub const QUALITY_BASELINE_VERDICT_THRESHOLDS_PROBE_V1_JSON: &str = include_str!(
  "../crates/auv-game-minecraft/tests/fixtures/mc17-d3/baseline-verdict-thresholds-v1-probe.json"
);
pub const QUALITY_BASELINE_VERDICT_THRESHOLDS_TRAINED_RENDER_V1_JSON: &str = include_str!(
  "../crates/auv-game-minecraft/tests/fixtures/mc17-d3/baseline-verdict-thresholds-v1-trained-render.json"
);

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QualityBaselineSpatialQueryThresholds {
  pub required_status: String,
  #[serde(default)]
  pub required_visibility: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct QualityBaselineHoldoutWitnessThresholds {
  pub required_status: String,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QualityBaselineRenderQualityThresholds {
  pub required_status: String,
  pub required_verdict: String,
  pub require_image_size_match: bool,
  #[serde(default)]
  pub l1_mean_max: Option<f64>,
  #[serde(default)]
  pub mse_max: Option<f64>,
  #[serde(default)]
  pub psnr_min: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QualityBaselineVerdictThresholdSet {
  pub spatial_query: QualityBaselineSpatialQueryThresholds,
  pub holdout_witness: QualityBaselineHoldoutWitnessThresholds,
  pub render_quality: QualityBaselineRenderQualityThresholds,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QualityBaselineVerdictThresholds {
  pub profile_id: String,
  pub render_evidence_mode: String,
  pub thresholds: QualityBaselineVerdictThresholdSet,
  #[serde(default)]
  pub trust_notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct QualityBaselineStageCheck {
  pub stage: String,
  pub outcome: String,
  pub reasons: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MinecraftTrainingResultQualityVerdictSummary {
  pub profile_id: String,
  pub render_evidence_mode: String,
  pub evidence_coverage: String,
  pub quality_verdict: String,
  pub stage_checks: Vec<QualityBaselineStageCheck>,
  pub trust_notes: Vec<String>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MinecraftQualityBaselineDualVerdicts {
  pub probe: MinecraftTrainingResultQualityVerdictSummary,
  pub trained_render: MinecraftTrainingResultQualityVerdictSummary,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MinecraftQualityBaselineReportWithVerdicts {
  #[serde(flatten)]
  pub report: MinecraftTrainingResultQualityBaselineReportSummary,
  pub verdicts: MinecraftQualityBaselineDualVerdicts,
}

pub fn quality_baseline_verdict_thresholds_probe_v1()
-> Result<QualityBaselineVerdictThresholds, String> {
  serde_json::from_str(QUALITY_BASELINE_VERDICT_THRESHOLDS_PROBE_V1_JSON)
    .map_err(|error| format!("parse quality baseline verdict thresholds probe v1 fixture: {error}"))
}

pub fn quality_baseline_verdict_thresholds_trained_render_v1()
-> Result<QualityBaselineVerdictThresholds, String> {
  serde_json::from_str(QUALITY_BASELINE_VERDICT_THRESHOLDS_TRAINED_RENDER_V1_JSON).map_err(
    |error| format!("parse quality baseline verdict thresholds trained_render v1 fixture: {error}"),
  )
}

fn check_spatial_query_stage(
  evidence: Option<&QualityBaselineSpatialQueryEvidence>,
  thresholds: &QualityBaselineSpatialQueryThresholds,
) -> QualityBaselineStageCheck {
  let Some(evidence) = evidence else {
    return QualityBaselineStageCheck {
      stage: "spatial_query".to_string(),
      outcome: "blocked".to_string(),
      reasons: vec!["spatial query evidence missing".to_string()],
    };
  };
  if evidence.status == "blocked" {
    return QualityBaselineStageCheck {
      stage: "spatial_query".to_string(),
      outcome: "blocked".to_string(),
      reasons: vec![format!(
        "status={} blocks threshold evaluation",
        evidence.status
      )],
    };
  }
  let mut reasons = Vec::new();
  if evidence.status != thresholds.required_status {
    reasons.push(format!(
      "status={} expected required_status={}",
      evidence.status, thresholds.required_status
    ));
  }
  if let Some(required_visibility) = thresholds.required_visibility.as_deref() {
    match evidence.visibility.as_deref() {
      Some(visibility) if visibility == required_visibility => {}
      Some(visibility) => reasons.push(format!(
        "visibility={visibility} expected required_visibility={required_visibility}"
      )),
      None => reasons.push(format!(
        "visibility missing expected required_visibility={required_visibility}"
      )),
    }
  }
  let outcome = if reasons.is_empty() {
    "pass".to_string()
  } else {
    "fail".to_string()
  };
  QualityBaselineStageCheck {
    stage: "spatial_query".to_string(),
    outcome,
    reasons,
  }
}

fn check_holdout_witness_stage(
  evidence: Option<&QualityBaselineHoldoutWitnessEvidence>,
  thresholds: &QualityBaselineHoldoutWitnessThresholds,
) -> QualityBaselineStageCheck {
  let Some(evidence) = evidence else {
    return QualityBaselineStageCheck {
      stage: "holdout_witness".to_string(),
      outcome: "blocked".to_string(),
      reasons: vec!["holdout witness evidence missing".to_string()],
    };
  };
  if evidence.status == "blocked" {
    return QualityBaselineStageCheck {
      stage: "holdout_witness".to_string(),
      outcome: "blocked".to_string(),
      reasons: vec![format!(
        "status={} blocks threshold evaluation",
        evidence.status
      )],
    };
  }
  if evidence.status != thresholds.required_status {
    return QualityBaselineStageCheck {
      stage: "holdout_witness".to_string(),
      outcome: "fail".to_string(),
      reasons: vec![format!(
        "status={} expected required_status={}",
        evidence.status, thresholds.required_status
      )],
    };
  }
  QualityBaselineStageCheck {
    stage: "holdout_witness".to_string(),
    outcome: "pass".to_string(),
    reasons: vec![],
  }
}

fn check_render_quality_stage(
  evidence: Option<&QualityBaselineRenderQualityEvidence>,
  thresholds: &QualityBaselineRenderQualityThresholds,
) -> QualityBaselineStageCheck {
  let Some(evidence) = evidence else {
    return QualityBaselineStageCheck {
      stage: "render_quality".to_string(),
      outcome: "blocked".to_string(),
      reasons: vec!["render quality evidence missing".to_string()],
    };
  };
  if evidence.status == "blocked" {
    return QualityBaselineStageCheck {
      stage: "render_quality".to_string(),
      outcome: "blocked".to_string(),
      reasons: vec![format!(
        "status={} blocks threshold evaluation",
        evidence.status
      )],
    };
  }
  if evidence.verdict == "metric_partial" {
    return QualityBaselineStageCheck {
      stage: "render_quality".to_string(),
      outcome: "partial".to_string(),
      reasons: vec!["verdict=metric_partial records incomplete photometric evidence".to_string()],
    };
  }
  let mut reasons = Vec::new();
  if evidence.status != thresholds.required_status {
    reasons.push(format!(
      "status={} expected required_status={}",
      evidence.status, thresholds.required_status
    ));
  }
  if evidence.verdict != thresholds.required_verdict {
    reasons.push(format!(
      "verdict={} expected required_verdict={}",
      evidence.verdict, thresholds.required_verdict
    ));
  }
  if thresholds.require_image_size_match && !evidence.image_size_match {
    reasons.push("image_size_match=false expected true".to_string());
  }
  if let Some(max) = thresholds.l1_mean_max {
    match evidence.l1_mean {
      Some(value) if value <= max => {}
      Some(value) => reasons.push(format!("l1_mean={value} exceeds l1_mean_max={max}")),
      None => reasons.push("l1_mean missing for threshold evaluation".to_string()),
    }
  }
  if let Some(max) = thresholds.mse_max {
    match evidence.mse {
      Some(value) if value <= max => {}
      Some(value) => reasons.push(format!("mse={value} exceeds mse_max={max}")),
      None => reasons.push("mse missing for threshold evaluation".to_string()),
    }
  }
  if let Some(min) = thresholds.psnr_min {
    match evidence.psnr {
      Some(value) if value >= min => {}
      Some(value) => reasons.push(format!("psnr={value} below psnr_min={min}")),
      None => reasons.push("psnr missing for threshold evaluation".to_string()),
    }
  }
  let has_metric_threshold_miss = reasons.iter().any(|reason| {
    reason.contains("exceeds l1_mean_max=")
      || reason.contains("exceeds mse_max=")
      || reason.contains("below psnr_min=")
  });
  let outcome = if reasons.is_empty() {
    "pass".to_string()
  } else if has_metric_threshold_miss {
    "fail".to_string()
  } else {
    "fail".to_string()
  };
  QualityBaselineStageCheck {
    stage: "render_quality".to_string(),
    outcome,
    reasons,
  }
}

fn aggregate_quality_verdict(
  baseline: &MinecraftTrainingResultQualityBaselineReportSummary,
  stage_checks: &[QualityBaselineStageCheck],
) -> String {
  if baseline.evidence_coverage != "complete" {
    return "blocked".to_string();
  }
  if baseline.issue.is_some() {
    return "blocked".to_string();
  }
  if stage_checks.iter().any(|check| check.outcome == "blocked") {
    return "blocked".to_string();
  }
  if stage_checks.iter().all(|check| check.outcome == "pass") {
    return "pass".to_string();
  }
  if stage_checks.iter().any(|check| {
    check.outcome == "fail"
      && check.reasons.iter().any(|reason| {
        reason.contains("exceeds l1_mean_max=")
          || reason.contains("exceeds mse_max=")
          || reason.contains("below psnr_min=")
      })
  }) {
    return "fail".to_string();
  }
  if stage_checks
    .iter()
    .any(|check| check.outcome == "partial" || check.outcome == "fail")
  {
    return "partial".to_string();
  }
  "blocked".to_string()
}

fn build_quality_verdict_trust_notes(
  baseline: &MinecraftTrainingResultQualityBaselineReportSummary,
  thresholds: &QualityBaselineVerdictThresholds,
) -> Vec<String> {
  let mut notes = build_quality_baseline_trust_notes(baseline.render_quality.as_ref());
  for note in &thresholds.trust_notes {
    if !notes.iter().any(|existing| existing == note) {
      notes.push(note.clone());
    }
  }
  notes
}

pub fn derive_minecraft_training_result_quality_verdict(
  baseline: &MinecraftTrainingResultQualityBaselineReportSummary,
  thresholds: &QualityBaselineVerdictThresholds,
) -> MinecraftTrainingResultQualityVerdictSummary {
  let spatial_check = check_spatial_query_stage(
    baseline.spatial_query.as_ref(),
    &thresholds.thresholds.spatial_query,
  );
  let holdout_check = check_holdout_witness_stage(
    baseline.holdout_witness.as_ref(),
    &thresholds.thresholds.holdout_witness,
  );
  let render_check = check_render_quality_stage(
    baseline.render_quality.as_ref(),
    &thresholds.thresholds.render_quality,
  );
  let stage_checks = vec![spatial_check, holdout_check, render_check];
  let quality_verdict = aggregate_quality_verdict(baseline, &stage_checks);
  let trust_notes = build_quality_verdict_trust_notes(baseline, thresholds);
  let issue = if baseline.evidence_coverage != "complete" {
    baseline.issue.clone().or_else(|| {
      Some(format!(
        "evidence_coverage={} blocks threshold verdict evaluation",
        baseline.evidence_coverage
      ))
    })
  } else {
    baseline.issue.clone()
  };

  MinecraftTrainingResultQualityVerdictSummary {
    profile_id: thresholds.profile_id.clone(),
    render_evidence_mode: thresholds.render_evidence_mode.clone(),
    evidence_coverage: baseline.evidence_coverage.clone(),
    quality_verdict,
    stage_checks,
    trust_notes,
    issue,
  }
}

pub fn quality_baseline_verdict_for_run(
  store: &LocalStore,
  run_id: &str,
  thresholds: &QualityBaselineVerdictThresholds,
) -> AuvResult<MinecraftTrainingResultQualityVerdictSummary> {
  let baseline = quality_baseline_report_for_run(store, run_id)?;
  Ok(derive_minecraft_training_result_quality_verdict(
    &baseline, thresholds,
  ))
}

pub fn quality_baseline_report_with_verdicts_for_run(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<MinecraftQualityBaselineReportWithVerdicts> {
  let report = quality_baseline_report_for_run(store, run_id)?;
  let probe = quality_baseline_verdict_thresholds_probe_v1()?;
  let trained_render = quality_baseline_verdict_thresholds_trained_render_v1()?;
  Ok(MinecraftQualityBaselineReportWithVerdicts {
    report: report.clone(),
    verdicts: MinecraftQualityBaselineDualVerdicts {
      probe: derive_minecraft_training_result_quality_verdict(&report, &probe),
      trained_render: derive_minecraft_training_result_quality_verdict(&report, &trained_render),
    },
  })
}

fn spatial_query_manifest_summary_for_action_readiness(
  summary: &MinecraftTrainingResultSpatialQueryManifestSummary,
) -> Result<TrainingResultSpatialQueryManifest, String> {
  let target_block = parse_spatial_query_target_block(&summary.target_block)?;
  let target_face = summary
    .target_face
    .as_deref()
    .map(parse_spatial_query_target_face)
    .transpose()?;
  let target_semantics = parse_spatial_query_target_semantics(&summary.target_semantics)?;
  let query_kind = parse_spatial_query_kind(&summary.query_kind)?;
  let status = parse_spatial_query_status(&summary.status)?;
  let reason = summary
    .reason
    .as_deref()
    .map(parse_spatial_query_reason)
    .transpose()?;
  let visibility = summary
    .visibility
    .as_deref()
    .map(parse_spatial_query_visibility)
    .transpose()?;
  let screen_point = summary
    .screen_point
    .as_deref()
    .map(parse_spatial_query_screen_point)
    .transpose()?;
  let selected_backend = summary
    .selected_backend
    .as_deref()
    .map(parse_spatial_query_backend)
    .transpose()?;
  let comparison_verdict = summary
    .comparison_verdict
    .as_deref()
    .map(parse_spatial_query_comparison_verdict)
    .transpose()?;

  Ok(TrainingResultSpatialQueryManifest {
    schema_version: summary.schema_version,
    generated_at_millis: 0,
    training_result_semantic_manifest_path: summary.training_result_semantic_manifest_path.clone(),
    source_training_result_artifact_manifest_path: summary
      .source_training_result_artifact_manifest_path
      .clone(),
    source_training_result_manifest_path: summary.source_training_result_manifest_path.clone(),
    source_training_job_manifest_path: summary.source_training_job_manifest_path.clone(),
    source_training_launch_plan_path: summary.source_training_launch_plan_path.clone(),
    source_training_package_manifest_path: summary.source_training_package_manifest_path.clone(),
    source_scene_packet_manifest_path: summary.source_scene_packet_manifest_path.clone(),
    source_bundle_manifest_paths: summary.source_bundle_manifest_paths.clone(),
    source_run_ids: summary.source_run_ids.clone(),
    trainer_backend: summary.trainer_backend.clone(),
    job_backend: summary.job_backend.clone(),
    normalized_result_dir: summary.normalized_result_dir.clone(),
    query_kind,
    target_block,
    target_face,
    target_semantics,
    selected_backend,
    status,
    reason,
    visibility,
    screen_point,
    match_radius_px: summary.match_radius_px,
    confidence: summary.confidence,
    basis_frame_id: summary.basis_frame_id.clone(),
    comparison_verdict,
    known_limits: summary.known_limits.clone(),
  })
}

fn parse_spatial_query_target_block(
  label: &str,
) -> Result<auv_game_minecraft::BlockPosition, String> {
  let parts: Vec<&str> = label.split(',').collect();
  if parts.len() != 3 {
    return Err(format!(
      "invalid spatial query target_block label `{label}`"
    ));
  }
  let x = parts[0]
    .parse::<i32>()
    .map_err(|error| format!("invalid spatial query target_block x `{label}`: {error}"))?;
  let y = parts[1]
    .parse::<i32>()
    .map_err(|error| format!("invalid spatial query target_block y `{label}`: {error}"))?;
  let z = parts[2]
    .parse::<i32>()
    .map_err(|error| format!("invalid spatial query target_block z `{label}`: {error}"))?;
  Ok(auv_game_minecraft::BlockPosition::new(x, y, z))
}

fn parse_spatial_query_target_face(label: &str) -> Result<auv_game_minecraft::BlockFace, String> {
  match label {
    "up" => Ok(auv_game_minecraft::BlockFace::Up),
    "down" => Ok(auv_game_minecraft::BlockFace::Down),
    "north" => Ok(auv_game_minecraft::BlockFace::North),
    "south" => Ok(auv_game_minecraft::BlockFace::South),
    "east" => Ok(auv_game_minecraft::BlockFace::East),
    "west" => Ok(auv_game_minecraft::BlockFace::West),
    other => Err(format!("invalid spatial query target_face label `{other}`")),
  }
}

fn parse_spatial_query_target_semantics(
  label: &str,
) -> Result<auv_game_minecraft::MinecraftTargetSemantics, String> {
  match label {
    "hit_face_center" => Ok(auv_game_minecraft::MinecraftTargetSemantics::HitFaceCenter),
    "block_center" => Ok(auv_game_minecraft::MinecraftTargetSemantics::BlockCenter),
    other => Err(format!(
      "invalid spatial query target_semantics label `{other}`"
    )),
  }
}

fn parse_spatial_query_kind(
  label: &str,
) -> Result<auv_game_minecraft::TrainingResultSpatialQueryKind, String> {
  match label {
    "block_projection" => Ok(auv_game_minecraft::TrainingResultSpatialQueryKind::BlockProjection),
    other => Err(format!("invalid spatial query query_kind label `{other}`")),
  }
}

fn parse_spatial_query_status(
  label: &str,
) -> Result<auv_game_minecraft::TrainingResultSpatialQueryStatus, String> {
  match label {
    "answered" => Ok(auv_game_minecraft::TrainingResultSpatialQueryStatus::Answered),
    "blocked" => Ok(auv_game_minecraft::TrainingResultSpatialQueryStatus::Blocked),
    "failed" => Ok(auv_game_minecraft::TrainingResultSpatialQueryStatus::Failed),
    other => Err(format!("invalid spatial query status label `{other}`")),
  }
}

fn parse_spatial_query_reason(
  label: &str,
) -> Result<auv_game_minecraft::TrainingResultSpatialQueryReason, String> {
  match label {
    "semantic_source_not_ready" => {
      Ok(auv_game_minecraft::TrainingResultSpatialQueryReason::SemanticSourceNotReady)
    }
    "target_block_absent_from_scene_packet" => {
      Ok(auv_game_minecraft::TrainingResultSpatialQueryReason::TargetBlockAbsentFromScenePacket)
    }
    "reference_projection_failed" => {
      Ok(auv_game_minecraft::TrainingResultSpatialQueryReason::ReferenceProjectionFailed)
    }
    "provider_command_failed" => {
      Ok(auv_game_minecraft::TrainingResultSpatialQueryReason::ProviderCommandFailed)
    }
    "provider_output_invalid" => {
      Ok(auv_game_minecraft::TrainingResultSpatialQueryReason::ProviderOutputInvalid)
    }
    other => Err(format!("invalid spatial query reason label `{other}`")),
  }
}

fn parse_spatial_query_visibility(
  label: &str,
) -> Result<auv_game_minecraft::ProjectionVisibility, String> {
  match label {
    "visible" => Ok(auv_game_minecraft::ProjectionVisibility::Visible),
    "behind_camera" => Ok(auv_game_minecraft::ProjectionVisibility::BehindCamera),
    "out_of_frustum" => Ok(auv_game_minecraft::ProjectionVisibility::OutOfFrustum),
    "outside_window" => Ok(auv_game_minecraft::ProjectionVisibility::OutsideWindow),
    other => Err(format!("invalid spatial query visibility label `{other}`")),
  }
}

fn parse_spatial_query_screen_point(label: &str) -> Result<auv_driver::geometry::Point, String> {
  let parts: Vec<&str> = label.split(',').collect();
  if parts.len() != 2 {
    return Err(format!(
      "invalid spatial query screen_point label `{label}`"
    ));
  }
  let x = parts[0]
    .parse::<f64>()
    .map_err(|error| format!("invalid spatial query screen_point x `{label}`: {error}"))?;
  let y = parts[1]
    .parse::<f64>()
    .map_err(|error| format!("invalid spatial query screen_point y `{label}`: {error}"))?;
  Ok(auv_driver::geometry::Point::new(x, y))
}

fn parse_spatial_query_backend(
  label: &str,
) -> Result<auv_game_minecraft::TrainingResultSpatialQueryBackend, String> {
  match label {
    "command_provider" => {
      Ok(auv_game_minecraft::TrainingResultSpatialQueryBackend::CommandProvider)
    }
    "checkpoint_native" => {
      Ok(auv_game_minecraft::TrainingResultSpatialQueryBackend::CheckpointNative)
    }
    "closed_scene_toy" => Ok(auv_game_minecraft::TrainingResultSpatialQueryBackend::ClosedSceneToy),
    "projection_reference" => {
      Ok(auv_game_minecraft::TrainingResultSpatialQueryBackend::ProjectionReference)
    }
    other => Err(format!(
      "invalid spatial query selected_backend label `{other}`"
    )),
  }
}

fn parse_spatial_query_comparison_verdict(
  label: &str,
) -> Result<auv_game_minecraft::TrainingResultSpatialQueryComparisonVerdict, String> {
  match label {
    "match" => Ok(auv_game_minecraft::TrainingResultSpatialQueryComparisonVerdict::Match),
    "divergent" => Ok(auv_game_minecraft::TrainingResultSpatialQueryComparisonVerdict::Divergent),
    "provider_only" => {
      Ok(auv_game_minecraft::TrainingResultSpatialQueryComparisonVerdict::ProviderOnly)
    }
    "reference_only" => {
      Ok(auv_game_minecraft::TrainingResultSpatialQueryComparisonVerdict::ReferenceOnly)
    }
    "not_comparable" => {
      Ok(auv_game_minecraft::TrainingResultSpatialQueryComparisonVerdict::NotComparable)
    }
    other => Err(format!(
      "invalid spatial query comparison_verdict label `{other}`"
    )),
  }
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

pub(crate) fn extract_action_transition_lineage(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<ActionTransitionLineage>> {
  let mut lineage = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != CANDIDATE_ACTION_EXECUTION_ARTIFACT_ROLE {
      continue;
    }

    let execution_artifact = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      lineage.push(malformed_action_transition_lineage(
        execution_artifact,
        format!(
          "candidate-action-execution artifact mime_type {} is not JSON",
          artifact.mime_type
        ),
      ));
      continue;
    }

    match read_candidate_action_execution_for_transition(store, run.run.run_id.as_str(), artifact) {
      Ok(ActionTransitionExecutionRead::Canonical(execution)) => lineage.push(
        action_transition_lineage_entry(store, run, artifact, execution),
      ),
      Ok(ActionTransitionExecutionRead::Legacy(legacy)) => lineage.push(
        legacy_action_transition_lineage_entry(run, artifact, legacy),
      ),
      Err(error) => lineage.push(malformed_action_transition_lineage(
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

fn project_action_resolver_decision(
  decision: &ActionResolverDecision,
) -> ActionResolverDecisionProjection {
  ActionResolverDecisionProjection {
    version: decision.version.clone(),
    operation: decision.operation.clone(),
    target_query: decision.target_query.clone(),
    primary_method: decision.primary_method.clone(),
    selected_method: decision.selected_method.clone(),
    fallback_allowed: decision.fallback_allowed,
    fallback_used: decision.fallback_used,
    fallback_reason: decision.fallback_reason.clone(),
    policy: decision.policy.clone(),
    cursor_disturbance: decision.cursor_disturbance.clone(),
    press_mechanism: decision.press_mechanism.clone(),
  }
}

enum ActionTransitionExecutionRead {
  Canonical(CandidateActionExecutionArtifact),
  Legacy(LegacyCandidateActionExecutionArtifact),
}

fn read_candidate_action_execution_for_transition(
  store: &LocalStore,
  run_id: &str,
  artifact: &ArtifactRecordV1Alpha1,
) -> AuvResult<ActionTransitionExecutionRead> {
  let (file, artifact_path) = open_artifact_file(
    store,
    run_id,
    artifact,
    CANDIDATE_ACTION_EXECUTION_ARTIFACT_ROLE,
  )?;
  let value: serde_json::Value =
    serde_json::from_reader(BufReader::new(file)).map_err(|error| {
      format!(
        "failed to parse {} artifact {} for run {} from {}: {}",
        CANDIDATE_ACTION_EXECUTION_ARTIFACT_ROLE,
        artifact.artifact_id,
        run_id,
        artifact_path.display(),
        error
      )
    })?;

  if let Ok(execution) = serde_json::from_value::<CandidateActionExecutionArtifact>(value.clone()) {
    return Ok(ActionTransitionExecutionRead::Canonical(execution));
  }

  serde_json::from_value::<LegacyCandidateActionExecutionArtifact>(value)
    .map(ActionTransitionExecutionRead::Legacy)
    .map_err(|error| {
      format!(
        "failed to parse {} artifact {} for run {} from {}: {}",
        CANDIDATE_ACTION_EXECUTION_ARTIFACT_ROLE,
        artifact.artifact_id,
        run_id,
        artifact_path.display(),
        error
      )
    })
}

fn read_candidate_action_decision_artifact(
  store: &LocalStore,
  run: &CanonicalRun,
  reference: &ArtifactRef,
) -> Option<CandidateActionDecisionArtifact> {
  let resolved = resolve_artifact_ref(run, reference);
  if !resolved.resolved {
    return None;
  }
  let record = run.artifacts.iter().find(|artifact| {
    artifact.artifact_id == reference.artifact_id && artifact.span_id == reference.span_id
  })?;
  if !is_json_mime(&record.mime_type) {
    return None;
  }
  read_artifact_json::<CandidateActionDecisionArtifact>(
    store,
    run.run.run_id.as_str(),
    record,
    CANDIDATE_ACTION_DECISION_ARTIFACT_ROLE,
  )
  .ok()
}

fn action_transition_verification_projection(
  execution: &CandidateActionExecutionArtifact,
) -> ActionTransitionVerificationProjection {
  let claims = operation_result_verification_claims(&execution.operation_result);
  let focus: Vec<&VerificationResult> = if claims.is_empty() {
    vec![&execution.verification_result]
  } else {
    claims
  };

  if focus.is_empty() {
    return ActionTransitionVerificationProjection {
      verification_outcome: "absent".to_string(),
      verification_reason: execution
        .operation_result
        .known_limits
        .first()
        .cloned()
        .or_else(|| {
          Some("no VerificationResult on operation-result; Layer 3 evidence absent".to_string())
        }),
      semantic_matched: None,
    };
  }

  let (verification_outcome, verification_reason) =
    project_verification_outcome_from_claims(&focus);
  let semantic_matched = focus
    .iter()
    .find(|claim| claim.semantic_matched.is_some())
    .or_else(|| focus.first())
    .and_then(|claim| claim.semantic_matched);

  ActionTransitionVerificationProjection {
    verification_outcome,
    verification_reason,
    semantic_matched,
  }
}

fn legacy_action_transition_verification_projection(
  operation_result: Option<&OperationResult>,
  verification_result: Option<&VerificationResult>,
) -> ActionTransitionVerificationProjection {
  if let Some(operation_result) = operation_result {
    let claims = operation_result_verification_claims(operation_result);
    let focus: Vec<&VerificationResult> = if claims.is_empty() {
      verification_result.into_iter().collect()
    } else {
      claims
    };

    if !focus.is_empty() {
      let (verification_outcome, verification_reason) =
        project_verification_outcome_from_claims(&focus);
      let semantic_matched = focus
        .iter()
        .find(|claim| claim.semantic_matched.is_some())
        .or_else(|| focus.first())
        .and_then(|claim| claim.semantic_matched);
      return ActionTransitionVerificationProjection {
        verification_outcome,
        verification_reason,
        semantic_matched,
      };
    }
  }

  if let Some(verification_result) = verification_result {
    let focus = vec![verification_result];
    let (verification_outcome, verification_reason) =
      project_verification_outcome_from_claims(&focus);
    return ActionTransitionVerificationProjection {
      verification_outcome,
      verification_reason,
      semantic_matched: verification_result.semantic_matched,
    };
  }

  ActionTransitionVerificationProjection {
    verification_outcome: "absent".to_string(),
    verification_reason: Some(
      "no VerificationResult on operation-result; Layer 3 evidence absent".to_string(),
    ),
    semantic_matched: None,
  }
}

fn classify_action_transition_lineage(
  execution: &CandidateActionExecutionArtifact,
  effective_decision: Option<&ActionResolverDecisionProjection>,
  planned_decision: Option<&ActionResolverDecisionProjection>,
  known_limits: &[String],
) -> (ActionTransitionLineageStatus, Option<String>) {
  let mut issue = None;
  let mut status = ActionTransitionLineageStatus::Ready;

  if effective_decision.is_none() {
    return (
      ActionTransitionLineageStatus::Partial,
      Some("missing_action_resolver_decision".to_string()),
    );
  }

  let has_plan_delivery_mismatch = known_limits
    .iter()
    .any(|limit| limit.starts_with("plan_delivery_mismatch:"));

  if has_plan_delivery_mismatch {
    status = ActionTransitionLineageStatus::Partial;
  }

  if let (Some(planned), Some(effective)) = (planned_decision, effective_decision) {
    if planned.selected_method != effective.selected_method && !has_plan_delivery_mismatch {
      status = ActionTransitionLineageStatus::Partial;
      issue = Some(format!(
        "plan_effective_method_divergence: planned={} effective={}",
        planned.selected_method, effective.selected_method
      ));
    }
  }

  if execution.input_action_result.attempts.is_empty()
    && detail_string(&execution.detail, &["input_delivery"]) != Some("not_attempted".to_string())
  {
    status = ActionTransitionLineageStatus::Partial;
    issue = issue.or(Some("missing_input_action_result".to_string()));
  }

  (status, issue)
}

fn action_transition_lineage_entry(
  store: &LocalStore,
  run: &CanonicalRun,
  artifact: &ArtifactRecordV1Alpha1,
  execution: CandidateActionExecutionArtifact,
) -> ActionTransitionLineage {
  let source_candidate_promotion_artifact = execution
    .source_candidate_promotion_artifact
    .as_ref()
    .map(|reference| resolve_artifact_ref(run, reference));
  let operation_result_artifact = execution
    .operation_result_artifact
    .as_ref()
    .map(|reference| resolve_artifact_ref(run, reference));
  let planned_decision = read_candidate_action_decision_artifact(
    store,
    run,
    &execution.source_candidate_action_decision_artifact,
  )
  .map(|decision| project_action_resolver_decision(&decision.action_resolver_decision));
  let effective_decision = Some(project_action_resolver_decision(
    &execution.action_resolver_decision,
  ));
  let driver_result = Some(execution.input_action_result.clone());
  let known_limits = execution.known_limits.clone();
  let (status, issue) = classify_action_transition_lineage(
    &execution,
    effective_decision.as_ref(),
    planned_decision.as_ref(),
    &known_limits,
  );
  let verification = action_transition_verification_projection(&execution);

  ActionTransitionLineage {
    artifact: artifact_record_lineage(run.run.run_id.clone(), artifact),
    status,
    execution_id: Some(execution.execution_id),
    pre_state: ActionTransitionPreState {
      source_candidate_promotion_artifact,
      source_promotion_id: Some(execution.source_promotion_id),
      candidate_local_id: Some(execution.candidate_local_id),
    },
    effective_decision,
    planned_decision,
    driver_result,
    post_state: ActionTransitionPostState {
      operation_result_artifact,
      operation_status: detail_string(&execution.detail, &["operation_status"]),
    },
    verification,
    known_limits,
    issue,
  }
}

fn malformed_action_transition_lineage(
  artifact: ArtifactRefLineage,
  issue: String,
) -> ActionTransitionLineage {
  ActionTransitionLineage {
    artifact,
    status: ActionTransitionLineageStatus::Malformed,
    execution_id: None,
    pre_state: ActionTransitionPreState {
      source_candidate_promotion_artifact: None,
      source_promotion_id: None,
      candidate_local_id: None,
    },
    effective_decision: None,
    planned_decision: None,
    driver_result: None,
    post_state: ActionTransitionPostState {
      operation_result_artifact: None,
      operation_status: None,
    },
    verification: ActionTransitionVerificationProjection {
      verification_outcome: "absent".to_string(),
      verification_reason: None,
      semantic_matched: None,
    },
    known_limits: Vec::new(),
    issue: Some(issue),
  }
}

fn legacy_action_transition_lineage_entry(
  run: &CanonicalRun,
  artifact: &ArtifactRecordV1Alpha1,
  legacy: LegacyCandidateActionExecutionArtifact,
) -> ActionTransitionLineage {
  let source_candidate_promotion_artifact = legacy
    .source_candidate_promotion_artifact
    .as_ref()
    .map(|reference| resolve_artifact_ref(run, reference));
  let operation_result_artifact = legacy
    .operation_result_artifact
    .as_ref()
    .map(|reference| resolve_artifact_ref(run, reference));
  let effective_decision = legacy
    .action_resolver_decision
    .as_ref()
    .map(project_action_resolver_decision);
  let verification = legacy_action_transition_verification_projection(
    legacy.operation_result.as_ref(),
    legacy.verification_result.as_ref(),
  );
  let issue = if effective_decision.is_none() {
    Some("missing_action_resolver_decision".to_string())
  } else if legacy.input_action_result.is_none() {
    Some("missing_input_action_result".to_string())
  } else {
    None
  };

  ActionTransitionLineage {
    artifact: artifact_record_lineage(run.run.run_id.clone(), artifact),
    status: ActionTransitionLineageStatus::Partial,
    execution_id: legacy.execution_id,
    pre_state: ActionTransitionPreState {
      source_candidate_promotion_artifact,
      source_promotion_id: legacy.source_promotion_id,
      candidate_local_id: legacy.candidate_local_id,
    },
    effective_decision,
    planned_decision: None,
    driver_result: legacy.input_action_result,
    post_state: ActionTransitionPostState {
      operation_result_artifact,
      operation_status: legacy
        .detail
        .as_ref()
        .and_then(|detail| detail_string(detail, &["operation_status"])),
    },
    verification,
    known_limits: legacy.known_limits,
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
      submission_recorded_at_millis: value.submission_recorded_at_millis,
      accepted_by_provider: value.accepted_by_provider,
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
      submission_recorded_at_millis: value.submission_recorded_at_millis,
      accepted_by_provider: value.accepted_by_provider,
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
      status_message: value.status_message,
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
      status_message: value.status_message,
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

fn spatial_query_block_label(block: auv_game_minecraft::BlockPosition) -> String {
  format!("{},{},{}", block.x, block.y, block.z)
}

fn spatial_query_optional_block_face_label(
  face: Option<auv_game_minecraft::BlockFace>,
) -> Option<String> {
  face.map(|value| match value {
    auv_game_minecraft::BlockFace::Up => "up".to_string(),
    auv_game_minecraft::BlockFace::Down => "down".to_string(),
    auv_game_minecraft::BlockFace::North => "north".to_string(),
    auv_game_minecraft::BlockFace::South => "south".to_string(),
    auv_game_minecraft::BlockFace::East => "east".to_string(),
    auv_game_minecraft::BlockFace::West => "west".to_string(),
  })
}

fn spatial_query_target_semantics_label(
  semantics: auv_game_minecraft::MinecraftTargetSemantics,
) -> String {
  match semantics {
    auv_game_minecraft::MinecraftTargetSemantics::HitFaceCenter => "hit_face_center".to_string(),
    auv_game_minecraft::MinecraftTargetSemantics::BlockCenter => "block_center".to_string(),
  }
}

fn spatial_query_kind_label(kind: auv_game_minecraft::TrainingResultSpatialQueryKind) -> String {
  match kind {
    auv_game_minecraft::TrainingResultSpatialQueryKind::BlockProjection => {
      "block_projection".to_string()
    }
  }
}

fn spatial_query_visibility_label(visibility: auv_game_minecraft::ProjectionVisibility) -> String {
  match visibility {
    auv_game_minecraft::ProjectionVisibility::Visible => "visible".to_string(),
    auv_game_minecraft::ProjectionVisibility::BehindCamera => "behind_camera".to_string(),
    auv_game_minecraft::ProjectionVisibility::OutOfFrustum => "out_of_frustum".to_string(),
    auv_game_minecraft::ProjectionVisibility::OutsideWindow => "outside_window".to_string(),
  }
}

fn spatial_query_screen_point_label(point: auv_driver::geometry::Point) -> String {
  format!("{},{}", point.x, point.y)
}

fn spatial_query_manifest_fields(
  value: &TrainingResultSpatialQueryManifest,
) -> MinecraftTrainingResultSpatialQueryManifestSummary {
  MinecraftTrainingResultSpatialQueryManifestSummary {
    schema_version: value.schema_version,
    training_result_semantic_manifest_path: value.training_result_semantic_manifest_path.clone(),
    source_training_result_artifact_manifest_path: value
      .source_training_result_artifact_manifest_path
      .clone(),
    source_training_result_manifest_path: value.source_training_result_manifest_path.clone(),
    source_training_job_manifest_path: value.source_training_job_manifest_path.clone(),
    source_training_launch_plan_path: value.source_training_launch_plan_path.clone(),
    source_training_package_manifest_path: value.source_training_package_manifest_path.clone(),
    source_scene_packet_manifest_path: value.source_scene_packet_manifest_path.clone(),
    source_bundle_manifest_paths: value.source_bundle_manifest_paths.clone(),
    source_run_ids: value.source_run_ids.clone(),
    trainer_backend: value.trainer_backend.clone(),
    job_backend: value.job_backend.clone(),
    normalized_result_dir: value.normalized_result_dir.clone(),
    query_kind: spatial_query_kind_label(value.query_kind),
    target_block: spatial_query_block_label(value.target_block),
    target_face: spatial_query_optional_block_face_label(value.target_face),
    target_semantics: spatial_query_target_semantics_label(value.target_semantics),
    selected_backend: value
      .selected_backend
      .map(|backend| backend.as_str().to_string()),
    status: value.status.as_str().to_string(),
    reason: value.reason.map(|reason| reason.as_str().to_string()),
    visibility: value.visibility.map(spatial_query_visibility_label),
    screen_point: value.screen_point.map(spatial_query_screen_point_label),
    match_radius_px: value.match_radius_px,
    confidence: value.confidence,
    basis_frame_id: value.basis_frame_id.clone(),
    comparison_verdict: value
      .comparison_verdict
      .map(|verdict| verdict.as_str().to_string()),
    known_limits: value.known_limits.clone(),
  }
}

impl From<auv_game_minecraft::HoldoutRenderQualityMetrics>
  for MinecraftHoldoutRenderQualityMetricsSummary
{
  fn from(value: auv_game_minecraft::HoldoutRenderQualityMetrics) -> Self {
    Self {
      l1_mean: value.l1_mean,
      mse: value.mse,
      psnr: value.psnr,
    }
  }
}

impl From<TrainingResultHoldoutRenderQualityManifest>
  for MinecraftHoldoutRenderQualityManifestSummary
{
  fn from(value: TrainingResultHoldoutRenderQualityManifest) -> Self {
    Self {
      schema_version: value.schema_version,
      training_result_semantic_manifest_path: value.training_result_semantic_manifest_path,
      holdout_preview_manifest_path: value.holdout_preview_manifest_path,
      source_training_result_artifact_manifest_path: value
        .source_training_result_artifact_manifest_path,
      source_training_result_manifest_path: value.source_training_result_manifest_path,
      source_training_job_manifest_path: value.source_training_job_manifest_path,
      source_scene_packet_manifest_path: value.source_scene_packet_manifest_path,
      source_run_ids: value.source_run_ids,
      holdout_frame_index: value.holdout_frame_index,
      basis_checkpoint_path: value.basis_checkpoint_path,
      rendered_image_path: value.rendered_image_path,
      image_size_match: value.image_size_match,
      metrics: value
        .metrics
        .map(MinecraftHoldoutRenderQualityMetricsSummary::from),
      status: value.status.as_str().to_string(),
      reason: value.reason.map(|reason| reason.as_str().to_string()),
      verdict: value.verdict.as_str().to_string(),
      known_limits: value.known_limits,
    }
  }
}

impl From<TrainingResultHoldoutRenderQualityInspectReport>
  for MinecraftHoldoutRenderQualityInspectReportSummary
{
  fn from(value: TrainingResultHoldoutRenderQualityInspectReport) -> Self {
    Self {
      schema_version: value.schema_version,
      training_result_holdout_render_quality_manifest_path: value
        .training_result_holdout_render_quality_manifest_path,
      training_result_semantic_manifest_path: value.training_result_semantic_manifest_path,
      holdout_preview_manifest_path: value.holdout_preview_manifest_path,
      source_training_result_artifact_manifest_path: value
        .source_training_result_artifact_manifest_path,
      source_training_result_manifest_path: value.source_training_result_manifest_path,
      source_training_job_manifest_path: value.source_training_job_manifest_path,
      source_scene_packet_manifest_path: value.source_scene_packet_manifest_path,
      source_run_ids: value.source_run_ids,
      holdout_frame_index: value.holdout_frame_index,
      basis_checkpoint_path: value.basis_checkpoint_path,
      rendered_image_path: value.rendered_image_path,
      image_size_match: value.image_size_match,
      metrics: value
        .metrics
        .map(MinecraftHoldoutRenderQualityMetricsSummary::from),
      status: value.status.as_str().to_string(),
      reason: value.reason.map(|reason| reason.as_str().to_string()),
      verdict: value.verdict.as_str().to_string(),
      warnings: value.warnings,
      known_limits: value.known_limits,
    }
  }
}

impl From<TrainingResultSpatialQueryManifest>
  for MinecraftTrainingResultSpatialQueryManifestSummary
{
  fn from(value: TrainingResultSpatialQueryManifest) -> Self {
    spatial_query_manifest_fields(&value)
  }
}

impl From<TrainingResultSpatialQueryInspectReport>
  for MinecraftTrainingResultSpatialQueryInspectReportSummary
{
  fn from(value: TrainingResultSpatialQueryInspectReport) -> Self {
    let manifest_fields = spatial_query_manifest_fields(&TrainingResultSpatialQueryManifest {
      schema_version: value.schema_version,
      generated_at_millis: value.generated_at_millis,
      training_result_semantic_manifest_path: value.training_result_semantic_manifest_path.clone(),
      source_training_result_artifact_manifest_path: value
        .source_training_result_artifact_manifest_path
        .clone(),
      source_training_result_manifest_path: value.source_training_result_manifest_path.clone(),
      source_training_job_manifest_path: value.source_training_job_manifest_path.clone(),
      source_training_launch_plan_path: value.source_training_launch_plan_path.clone(),
      source_training_package_manifest_path: value.source_training_package_manifest_path.clone(),
      source_scene_packet_manifest_path: value.source_scene_packet_manifest_path.clone(),
      source_bundle_manifest_paths: value.source_bundle_manifest_paths.clone(),
      source_run_ids: value.source_run_ids.clone(),
      trainer_backend: value.trainer_backend.clone(),
      job_backend: value.job_backend.clone(),
      normalized_result_dir: value.normalized_result_dir.clone(),
      query_kind: value.query_kind,
      target_block: value.target_block,
      target_face: value.target_face,
      target_semantics: value.target_semantics,
      selected_backend: value.selected_backend,
      status: value.status,
      reason: value.reason,
      visibility: value.visibility,
      screen_point: value.screen_point,
      match_radius_px: value.match_radius_px,
      confidence: value.confidence,
      basis_frame_id: value.basis_frame_id.clone(),
      comparison_verdict: value.comparison_verdict,
      known_limits: value.known_limits.clone(),
    });
    Self {
      schema_version: manifest_fields.schema_version,
      training_result_spatial_query_manifest_path: value
        .training_result_spatial_query_manifest_path
        .clone(),
      training_result_semantic_manifest_path: manifest_fields
        .training_result_semantic_manifest_path
        .clone(),
      source_training_result_artifact_manifest_path: manifest_fields
        .source_training_result_artifact_manifest_path
        .clone(),
      source_training_result_manifest_path: manifest_fields
        .source_training_result_manifest_path
        .clone(),
      source_training_job_manifest_path: manifest_fields.source_training_job_manifest_path.clone(),
      source_training_launch_plan_path: manifest_fields.source_training_launch_plan_path.clone(),
      source_training_package_manifest_path: manifest_fields
        .source_training_package_manifest_path
        .clone(),
      source_scene_packet_manifest_path: manifest_fields.source_scene_packet_manifest_path.clone(),
      source_bundle_manifest_paths: manifest_fields.source_bundle_manifest_paths.clone(),
      source_run_ids: manifest_fields.source_run_ids.clone(),
      trainer_backend: manifest_fields.trainer_backend.clone(),
      job_backend: manifest_fields.job_backend.clone(),
      normalized_result_dir: manifest_fields.normalized_result_dir.clone(),
      query_kind: manifest_fields.query_kind.clone(),
      target_block: manifest_fields.target_block.clone(),
      target_face: manifest_fields.target_face.clone(),
      target_semantics: manifest_fields.target_semantics.clone(),
      selected_backend: manifest_fields.selected_backend.clone(),
      status: manifest_fields.status.clone(),
      reason: manifest_fields.reason.clone(),
      visibility: manifest_fields.visibility.clone(),
      screen_point: manifest_fields.screen_point.clone(),
      match_radius_px: manifest_fields.match_radius_px,
      confidence: manifest_fields.confidence,
      basis_frame_id: manifest_fields.basis_frame_id.clone(),
      comparison_verdict: manifest_fields.comparison_verdict.clone(),
      provider_status: value.provider_status.as_str().to_string(),
      provider_reason: value
        .provider_reason
        .map(|reason| reason.as_str().to_string()),
      provider_message: value.provider_message.clone(),
      reference_status: value.reference_status.as_str().to_string(),
      reference_reason: value
        .reference_reason
        .map(|reason| reason.as_str().to_string()),
      reference_basis_frame_id: value.reference_basis_frame_id.clone(),
      reference_source_frame_json_path: value.reference_source_frame_json_path.clone(),
      reference_screenshot_path: value.reference_screenshot_path.clone(),
      scene_packet_frame_count: value.scene_packet_frame_count,
      warnings: value.warnings.clone(),
      known_limits: manifest_fields.known_limits.clone(),
    }
  }
}

impl From<TrainingResultSemanticCheckpointRecord>
  for MinecraftTrainingResultSemanticCheckpointSummary
{
  fn from(value: TrainingResultSemanticCheckpointRecord) -> Self {
    Self {
      relative_path: value.relative_path,
      byte_size: value.byte_size,
    }
  }
}

impl From<auv_game_minecraft::HoldoutFrameWitness>
  for MinecraftTrainingResultHoldoutPreviewFrameWitnessSummary
{
  fn from(value: auv_game_minecraft::HoldoutFrameWitness) -> Self {
    Self {
      frame_index: value.frame_index,
      spatial_frame_id: value.spatial_frame_id,
      screenshot_path: value.screenshot_path,
      frame_json_path: value.frame_json_path,
    }
  }
}

impl From<TrainingResultHoldoutPreviewManifest>
  for MinecraftTrainingResultHoldoutPreviewManifestSummary
{
  fn from(value: TrainingResultHoldoutPreviewManifest) -> Self {
    Self {
      schema_version: value.schema_version,
      training_result_semantic_manifest_path: value.training_result_semantic_manifest_path,
      source_training_result_artifact_manifest_path: value
        .source_training_result_artifact_manifest_path,
      source_training_result_manifest_path: value.source_training_result_manifest_path,
      source_training_job_manifest_path: value.source_training_job_manifest_path,
      source_training_launch_plan_path: value.source_training_launch_plan_path,
      source_training_package_manifest_path: value.source_training_package_manifest_path,
      source_scene_packet_manifest_path: value.source_scene_packet_manifest_path,
      source_bundle_manifest_paths: value.source_bundle_manifest_paths,
      source_run_ids: value.source_run_ids,
      trainer_backend: value.trainer_backend,
      job_backend: value.job_backend,
      normalized_result_dir: value.normalized_result_dir,
      holdout_frame_index: value.holdout_frame_index,
      holdout_frame: value
        .holdout_frame
        .map(MinecraftTrainingResultHoldoutPreviewFrameWitnessSummary::from),
      basis_checkpoint_path: value.basis_checkpoint_path,
      holdout_screenshot_path: value.holdout_screenshot_path,
      reference_overlay_path: value.reference_overlay_path,
      status: value.status.as_str().to_string(),
      reason: value.reason.map(|reason| reason.as_str().to_string()),
      known_limits: value.known_limits,
    }
  }
}

impl From<TrainingResultHoldoutPreviewInspectReport>
  for MinecraftTrainingResultHoldoutPreviewInspectReportSummary
{
  fn from(value: TrainingResultHoldoutPreviewInspectReport) -> Self {
    Self {
      schema_version: value.schema_version,
      training_result_holdout_preview_manifest_path: value
        .training_result_holdout_preview_manifest_path,
      training_result_semantic_manifest_path: value.training_result_semantic_manifest_path,
      source_training_result_artifact_manifest_path: value
        .source_training_result_artifact_manifest_path,
      source_training_result_manifest_path: value.source_training_result_manifest_path,
      source_training_job_manifest_path: value.source_training_job_manifest_path,
      source_training_launch_plan_path: value.source_training_launch_plan_path,
      source_training_package_manifest_path: value.source_training_package_manifest_path,
      source_scene_packet_manifest_path: value.source_scene_packet_manifest_path,
      source_bundle_manifest_paths: value.source_bundle_manifest_paths,
      source_run_ids: value.source_run_ids,
      trainer_backend: value.trainer_backend,
      job_backend: value.job_backend,
      normalized_result_dir: value.normalized_result_dir,
      holdout_frame_index: value.holdout_frame_index,
      holdout_frame: value
        .holdout_frame
        .map(MinecraftTrainingResultHoldoutPreviewFrameWitnessSummary::from),
      basis_checkpoint_path: value.basis_checkpoint_path,
      holdout_screenshot_path: value.holdout_screenshot_path,
      reference_overlay_path: value.reference_overlay_path,
      status: value.status.as_str().to_string(),
      reason: value.reason.map(|reason| reason.as_str().to_string()),
      holdout_frame_selection: value.holdout_frame_selection.as_str().to_string(),
      checkpoint_count: value.checkpoint_count,
      scene_packet_frame_count: value.scene_packet_frame_count,
      warnings: value.warnings,
      known_limits: value.known_limits,
    }
  }
}

impl From<TrainingResultSemanticManifest> for MinecraftTrainingResultSemanticManifestSummary {
  fn from(value: TrainingResultSemanticManifest) -> Self {
    Self {
      schema_version: value.schema_version,
      source_training_result_artifact_manifest_path: value
        .source_training_result_artifact_manifest_path,
      source_training_result_manifest_path: value.source_training_result_manifest_path,
      source_training_job_manifest_path: value.source_training_job_manifest_path,
      source_training_launch_plan_path: value.source_training_launch_plan_path,
      source_training_package_manifest_path: value.source_training_package_manifest_path,
      source_scene_packet_manifest_path: value.source_scene_packet_manifest_path,
      source_bundle_manifest_paths: value.source_bundle_manifest_paths,
      source_run_ids: value.source_run_ids,
      trainer_backend: value.trainer_backend,
      job_backend: value.job_backend,
      source_result_status: value.source_result_status.as_str().to_string(),
      normalized_result_dir: value.normalized_result_dir,
      semantic_status: value.semantic_status.as_str().to_string(),
      semantic_reason: value
        .semantic_reason
        .map(|reason| reason.as_str().to_string()),
      config_path: value.config_path,
      models_dir_path: value.models_dir_path,
      status_snapshot_path: value.status_snapshot_path,
      config_trainer: value.config_trainer,
      checkpoint_files: value
        .checkpoint_files
        .into_iter()
        .map(MinecraftTrainingResultSemanticCheckpointSummary::from)
        .collect(),
      checkpoint_count: value.checkpoint_count,
      known_limits: value.known_limits,
    }
  }
}

impl From<TrainingResultSemanticInspectReport>
  for MinecraftTrainingResultSemanticInspectReportSummary
{
  fn from(value: TrainingResultSemanticInspectReport) -> Self {
    Self {
      schema_version: value.schema_version,
      training_result_semantic_manifest_path: value.training_result_semantic_manifest_path,
      source_training_result_artifact_manifest_path: value
        .source_training_result_artifact_manifest_path,
      source_training_result_manifest_path: value.source_training_result_manifest_path,
      source_training_job_manifest_path: value.source_training_job_manifest_path,
      source_training_launch_plan_path: value.source_training_launch_plan_path,
      source_training_package_manifest_path: value.source_training_package_manifest_path,
      source_scene_packet_manifest_path: value.source_scene_packet_manifest_path,
      source_bundle_manifest_paths: value.source_bundle_manifest_paths,
      source_run_ids: value.source_run_ids,
      trainer_backend: value.trainer_backend,
      job_backend: value.job_backend,
      source_result_status: value.source_result_status.as_str().to_string(),
      normalized_result_dir: value.normalized_result_dir,
      semantic_status: value.semantic_status.as_str().to_string(),
      semantic_reason: value
        .semantic_reason
        .map(|reason| reason.as_str().to_string()),
      config_yaml_parsed: value.config_yaml_parsed,
      config_trainer: value.config_trainer,
      config_backend_matches: value.config_backend_matches,
      models_dir_readable: value.models_dir_readable,
      status_snapshot_present: value.status_snapshot_present,
      checkpoint_count: value.checkpoint_count,
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

impl From<&DetectionEvalWitnessManifest> for OsuDetectionEvalWitnessManifestSummary {
  fn from(manifest: &DetectionEvalWitnessManifest) -> Self {
    Self {
      schema_version: manifest.schema_version,
      source_visual_eval_report_path: manifest.source_visual_eval_report_path.clone(),
      source_run_artifact_dir: manifest.source_run_artifact_dir.clone(),
      detector_model_id: manifest.detector_model_id.clone(),
      total_frames: manifest.total_frames,
      label_matched_frames: manifest.label_matched_frames,
      spatial_matched_frames: manifest.spatial_matched_frames,
      spatial_unscored_frames: manifest.spatial_unscored_frames,
      spurious_detection_count: manifest.spurious_detection_count,
      projection_kind: manifest.projection_kind.clone(),
      frame_witness_count: manifest.frame_witnesses.len(),
      status: manifest.status.as_str().to_string(),
      reason: manifest.reason.map(|reason| reason.as_str().to_string()),
    }
  }
}

impl From<&DetectionEvalWitnessInspectReport> for OsuDetectionEvalWitnessInspectReportSummary {
  fn from(report: &DetectionEvalWitnessInspectReport) -> Self {
    Self {
      schema_version: report.schema_version,
      detection_eval_witness_manifest_path: report.detection_eval_witness_manifest_path.clone(),
      total_frames: report.total_frames,
      frame_witness_count: report.frame_witness_count,
      status: report.status.as_str().to_string(),
      warnings: report.warnings.clone(),
    }
  }
}

impl From<&DetectionEvalQualityManifest> for OsuDetectionEvalQualityManifestSummary {
  fn from(manifest: &DetectionEvalQualityManifest) -> Self {
    Self {
      schema_version: manifest.schema_version,
      detection_eval_witness_manifest_path: manifest.detection_eval_witness_manifest_path.clone(),
      source_visual_eval_report_path: manifest.source_visual_eval_report_path.clone(),
      witness_status: manifest.witness_status.as_str().to_string(),
      status: manifest.status.as_str().to_string(),
      verdict: manifest.verdict.as_str().to_string(),
      label_recall: manifest.metrics.as_ref().and_then(|m| m.label_recall),
      spatial_recall: manifest.metrics.as_ref().and_then(|m| m.spatial_recall),
      spurious_detection_count: manifest
        .metrics
        .as_ref()
        .map(|m| m.spurious_detection_count),
    }
  }
}

impl From<&DetectionEvalQualityInspectReport> for OsuDetectionEvalQualityInspectReportSummary {
  fn from(report: &DetectionEvalQualityInspectReport) -> Self {
    Self {
      schema_version: report.schema_version,
      detection_eval_quality_manifest_path: report.detection_eval_quality_manifest_path.clone(),
      witness_status: report.witness_status.as_str().to_string(),
      status: report.status.as_str().to_string(),
      verdict: report.verdict.as_str().to_string(),
      label_recall_available: report.label_recall_available,
      spatial_recall_available: report.spatial_recall_available,
    }
  }
}

impl From<&VisualTruthSemanticManifest> for OsuVisualTruthSemanticManifestSummary {
  fn from(manifest: &VisualTruthSemanticManifest) -> Self {
    Self {
      schema_version: manifest.schema_version,
      source_run_artifact_dir: manifest.source_run_artifact_dir.clone(),
      source_visual_truth_manifest_path: manifest.source_visual_truth_manifest_path.clone(),
      source_projection_path: manifest.source_projection_path.clone(),
      beatmap_path: manifest.beatmap_path.clone(),
      frame_count: manifest.frame_count,
      semantic_status: manifest.semantic_status.as_str().to_string(),
      semantic_reason: manifest
        .semantic_reason
        .map(|reason| reason.as_str().to_string()),
      known_limits: manifest.known_limits.clone(),
    }
  }
}

impl From<&VisualTruthSemanticInspectReport> for OsuVisualTruthSemanticInspectReportSummary {
  fn from(report: &VisualTruthSemanticInspectReport) -> Self {
    Self {
      schema_version: report.schema_version,
      visual_truth_semantic_manifest_path: report.visual_truth_semantic_manifest_path.clone(),
      source_run_artifact_dir: report.source_run_artifact_dir.clone(),
      semantic_status: report.semantic_status.as_str().to_string(),
      semantic_reason: report
        .semantic_reason
        .map(|reason| reason.as_str().to_string()),
      visual_truth_manifest_readable: report.visual_truth_manifest_readable,
      projection_readable: report.projection_readable,
      projection_eval_ready: report.projection_eval_ready,
      warnings: report.warnings.clone(),
      known_limits: report.known_limits.clone(),
    }
  }
}

impl From<&auv_game_balatro::CardDetectionSemanticManifest>
  for BalatroCardDetectionSemanticManifestSummary
{
  fn from(manifest: &auv_game_balatro::CardDetectionSemanticManifest) -> Self {
    Self {
      schema_version: manifest.schema_version,
      source_detection_bundle_dir: manifest.source_detection_bundle_dir.clone(),
      frame_source: manifest.frame_source.clone(),
      image_width: manifest.image_width,
      image_height: manifest.image_height,
      ui_detection_count: manifest.ui_detection_count,
      entities_detection_count: manifest.entities_detection_count,
      semantic_status: manifest.semantic_status.as_str().to_string(),
      semantic_reason: manifest
        .semantic_reason
        .map(|reason| reason.as_str().to_string()),
      known_limits: manifest.known_limits.clone(),
    }
  }
}

impl From<&auv_game_balatro::CardDetectionSemanticInspectReport>
  for BalatroCardDetectionSemanticInspectReportSummary
{
  fn from(report: &auv_game_balatro::CardDetectionSemanticInspectReport) -> Self {
    Self {
      schema_version: report.schema_version,
      card_detection_semantic_manifest_path: report.card_detection_semantic_manifest_path.clone(),
      semantic_status: report.semantic_status.as_str().to_string(),
      semantic_reason: report
        .semantic_reason
        .map(|reason| reason.as_str().to_string()),
      detection_bundle_readable: report.detection_bundle_readable,
      detection_sets_non_empty: report.detection_sets_non_empty,
      warnings: report.warnings.clone(),
      known_limits: report.known_limits.clone(),
    }
  }
}

impl From<&auv_game_balatro::CardDetectionSpatialQueryManifest>
  for BalatroCardDetectionSpatialQueryManifestSummary
{
  fn from(manifest: &auv_game_balatro::CardDetectionSpatialQueryManifest) -> Self {
    Self {
      schema_version: manifest.schema_version,
      card_detection_semantic_manifest_path: manifest.card_detection_semantic_manifest_path.clone(),
      target_zone: manifest.target_zone.clone(),
      target_index: manifest.target_index,
      query_backend: manifest.query_backend.as_str().to_string(),
      status: manifest.status.as_str().to_string(),
      reason: manifest.reason.map(|reason| reason.as_str().to_string()),
      pixel_x: manifest.pixel_x,
      pixel_y: manifest.pixel_y,
      known_limits: manifest.known_limits.clone(),
    }
  }
}

impl From<&auv_game_balatro::CardDetectionSpatialQueryInspectReport>
  for BalatroCardDetectionSpatialQueryInspectReportSummary
{
  fn from(report: &auv_game_balatro::CardDetectionSpatialQueryInspectReport) -> Self {
    Self {
      schema_version: report.schema_version,
      card_detection_spatial_query_manifest_path: report
        .card_detection_spatial_query_manifest_path
        .clone(),
      target_zone: report.target_zone.clone(),
      target_index: report.target_index,
      query_backend: report.query_backend.as_str().to_string(),
      status: report.status.as_str().to_string(),
      reason: report.reason.map(|reason| reason.as_str().to_string()),
      semantic_status: report.semantic_status.as_str().to_string(),
      warnings: report.warnings.clone(),
      known_limits: report.known_limits.clone(),
    }
  }
}

impl From<&auv_game_balatro::CardDetectionEvalWitnessManifest>
  for BalatroCardDetectionEvalWitnessManifestSummary
{
  fn from(manifest: &auv_game_balatro::CardDetectionEvalWitnessManifest) -> Self {
    Self {
      schema_version: manifest.schema_version,
      card_detection_semantic_manifest_path: manifest.card_detection_semantic_manifest_path.clone(),
      card_detection_spatial_query_manifest_path: manifest
        .card_detection_spatial_query_manifest_path
        .clone(),
      expected_slots_path: manifest.expected_slots_path.clone(),
      source_detection_bundle_dir: manifest.source_detection_bundle_dir.clone(),
      expected_slot_count: manifest.expected_slot_count,
      scored_slot_count: manifest.scored_slot_count,
      unscored_slot_count: manifest.unscored_slot_count,
      below_confidence_slot_count: manifest.below_confidence_slot_count,
      quality_backend: manifest.quality_backend.as_str().to_string(),
      detector_model_id: manifest.detector_model_id.clone(),
      slot_score_count: manifest.slot_scores.len(),
      status: manifest.status.as_str().to_string(),
      reason: manifest.reason.map(|reason| reason.as_str().to_string()),
    }
  }
}

impl From<&auv_game_balatro::CardDetectionEvalWitnessInspectReport>
  for BalatroCardDetectionEvalWitnessInspectReportSummary
{
  fn from(report: &auv_game_balatro::CardDetectionEvalWitnessInspectReport) -> Self {
    Self {
      schema_version: report.schema_version,
      card_detection_eval_witness_manifest_path: report
        .card_detection_eval_witness_manifest_path
        .clone(),
      card_detection_semantic_manifest_path: report.card_detection_semantic_manifest_path.clone(),
      card_detection_spatial_query_manifest_path: report
        .card_detection_spatial_query_manifest_path
        .clone(),
      expected_slots_path: report.expected_slots_path.clone(),
      source_detection_bundle_dir: report.source_detection_bundle_dir.clone(),
      expected_slot_count: report.expected_slot_count,
      scored_slot_count: report.scored_slot_count,
      unscored_slot_count: report.unscored_slot_count,
      below_confidence_slot_count: report.below_confidence_slot_count,
      quality_backend: report.quality_backend.as_str().to_string(),
      detector_model_id: report.detector_model_id.clone(),
      slot_score_count: report.slot_score_count,
      semantic_manifest_readable: report.semantic_manifest_readable,
      spatial_query_manifest_readable: report.spatial_query_manifest_readable,
      expected_slots_readable: report.expected_slots_readable,
      status: report.status.as_str().to_string(),
      reason: report.reason.map(|reason| reason.as_str().to_string()),
      warnings: report.warnings.clone(),
    }
  }
}

impl From<&auv_game_balatro::CardDetectionQualityManifest>
  for BalatroCardDetectionQualityManifestSummary
{
  fn from(manifest: &auv_game_balatro::CardDetectionQualityManifest) -> Self {
    Self {
      schema_version: manifest.schema_version,
      card_detection_eval_witness_manifest_path: manifest
        .card_detection_eval_witness_manifest_path
        .clone(),
      witness_status: manifest.witness_status.as_str().to_string(),
      status: manifest.status.as_str().to_string(),
      verdict: manifest.verdict.as_str().to_string(),
      quality_backend: manifest
        .quality_backend
        .map(|backend| backend.as_str().to_string()),
      expected_slot_count: manifest.metrics.as_ref().map(|m| m.expected_slot_count),
      scored_slot_count: manifest.metrics.as_ref().map(|m| m.scored_slot_count),
      unscored_slot_count: manifest.metrics.as_ref().map(|m| m.unscored_slot_count),
      slot_coverage_ratio: manifest
        .metrics
        .as_ref()
        .and_then(|m| m.slot_coverage_ratio),
    }
  }
}

impl From<&auv_game_balatro::CardDetectionQualityInspectReport>
  for BalatroCardDetectionQualityInspectReportSummary
{
  fn from(report: &auv_game_balatro::CardDetectionQualityInspectReport) -> Self {
    Self {
      schema_version: report.schema_version,
      card_detection_quality_manifest_path: report.card_detection_quality_manifest_path.clone(),
      card_detection_eval_witness_manifest_path: report
        .card_detection_eval_witness_manifest_path
        .clone(),
      witness_status: report.witness_status.as_str().to_string(),
      status: report.status.as_str().to_string(),
      verdict: report.verdict.as_str().to_string(),
      quality_backend: report
        .quality_backend
        .map(|backend| backend.as_str().to_string()),
      slot_coverage_ratio_available: report.slot_coverage_ratio_available,
    }
  }
}

impl From<&VisualTruthSpatialQueryManifest> for OsuVisualTruthSpatialQueryManifestSummary {
  fn from(manifest: &VisualTruthSpatialQueryManifest) -> Self {
    Self {
      schema_version: manifest.schema_version,
      visual_truth_semantic_manifest_path: manifest.visual_truth_semantic_manifest_path.clone(),
      source_run_artifact_dir: manifest.source_run_artifact_dir.clone(),
      object_index: manifest.object_index,
      capture_phase: match manifest.capture_phase {
        auv_game_osu::CapturePhase::BeforeDispatch => "before_dispatch".to_string(),
        auv_game_osu::CapturePhase::AfterDispatch => "after_dispatch".to_string(),
      },
      object_kind: manifest.object_kind.as_ref().map(|kind| match kind {
        auv_game_osu::ObjectKind::Circle => "circle".to_string(),
        auv_game_osu::ObjectKind::Slider => "slider".to_string(),
        auv_game_osu::ObjectKind::Spinner => "spinner".to_string(),
        auv_game_osu::ObjectKind::Hold => "hold".to_string(),
      }),
      query_backend: manifest.query_backend.as_str().to_string(),
      status: manifest.status.as_str().to_string(),
      reason: manifest.reason.map(|reason| reason.as_str().to_string()),
      pixel_visibility: manifest
        .pixel_visibility
        .map(|visibility| visibility.as_str().to_string()),
      pixel_x: manifest.pixel_x,
      pixel_y: manifest.pixel_y,
      match_radius_px: manifest.match_radius_px,
      capture_width: manifest.capture_width,
      capture_height: manifest.capture_height,
      known_limits: manifest.known_limits.clone(),
    }
  }
}

impl From<&VisualTruthSpatialQueryInspectReport>
  for OsuVisualTruthSpatialQueryInspectReportSummary
{
  fn from(report: &VisualTruthSpatialQueryInspectReport) -> Self {
    Self {
      schema_version: report.schema_version,
      visual_truth_spatial_query_manifest_path: report
        .visual_truth_spatial_query_manifest_path
        .clone(),
      visual_truth_semantic_manifest_path: report.visual_truth_semantic_manifest_path.clone(),
      object_index: report.object_index,
      capture_phase: match report.capture_phase {
        auv_game_osu::CapturePhase::BeforeDispatch => "before_dispatch".to_string(),
        auv_game_osu::CapturePhase::AfterDispatch => "after_dispatch".to_string(),
      },
      query_backend: report.query_backend.as_str().to_string(),
      status: report.status.as_str().to_string(),
      reason: report.reason.map(|reason| reason.as_str().to_string()),
      pixel_visibility: report
        .pixel_visibility
        .map(|visibility| visibility.as_str().to_string()),
      semantic_status: report.semantic_status.as_str().to_string(),
      warnings: report.warnings.clone(),
      known_limits: report.known_limits.clone(),
    }
  }
}

pub use crate::view_parser_read::{
  build_view_parser_inspect, build_view_resolution_summary, extract_playlist_select_result_wires,
  extract_reacquisition_records, extract_view_memory_writes, list_view_memory_writes,
};

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;
  use std::env;
  use std::fs;
  use std::path::{Path, PathBuf};

  use serde::Serialize;
  use serde_json::json;

  use super::{
    ActionTransitionLineageStatus, ArtifactRefLineage, CANDIDATE_ACTION_DECISION_ARTIFACT_ROLE,
    CANDIDATE_ACTION_EXECUTION_ARTIFACT_ROLE, CANDIDATE_PROMOTION_ARTIFACT_ROLE,
    CandidateActionDecisionLineageStatus, CandidateActionExecutionClosureState,
    CandidateActionExecutionLineageStatus, CandidatePromotionLineageStatus,
    DETECTOR_RECOGNITION_ARTIFACT_ROLE, DetectorRecognitionLineageStatus,
    MinecraftHoldoutRenderQualityManifestSummary, MinecraftHoldoutRenderQualityMetricsSummary,
    MinecraftSpatialBundleManifestSummary,
    MinecraftTrainingResultHoldoutPreviewFrameWitnessSummary,
    MinecraftTrainingResultHoldoutPreviewManifestSummary,
    MinecraftTrainingResultQualityBaselineReportSummary,
    MinecraftTrainingResultSpatialQueryManifestLineage,
    MinecraftTrainingResultSpatialQueryManifestSummary,
    derive_minecraft_query_wired_live_action_summary,
    derive_minecraft_training_result_quality_baseline_report,
    derive_minecraft_training_result_quality_verdict,
    derive_minecraft_training_result_spatial_query_action_readiness,
    derive_osu_query_wired_live_action_summary, extract_action_transition_lineage,
    extract_candidate_action_decision_lineage, extract_candidate_action_execution_lineage,
    extract_candidate_promotion_lineage, extract_detector_recognition_lineage,
    extract_minecraft_holdout_render_quality_inspect_reports,
    extract_minecraft_holdout_render_quality_manifests,
    extract_minecraft_training_job_inspect_reports, extract_minecraft_training_job_manifests,
    extract_minecraft_training_launch_inspect_reports, extract_minecraft_training_launch_manifests,
    extract_minecraft_training_package_inspect_reports,
    extract_minecraft_training_package_manifests,
    extract_minecraft_training_result_artifact_fetch_inspect_reports,
    extract_minecraft_training_result_artifact_fetch_manifests,
    extract_minecraft_training_result_holdout_preview_inspect_reports,
    extract_minecraft_training_result_holdout_preview_manifests,
    extract_minecraft_training_result_inspect_reports, extract_minecraft_training_result_manifests,
    extract_minecraft_training_result_semantic_inspect_reports,
    extract_minecraft_training_result_semantic_manifests,
    extract_minecraft_training_result_spatial_query_manifests, extract_observation_snapshots,
    extract_verifications, list_action_transition_lineage, list_candidate_action_decision_lineage,
    list_candidate_action_execution_lineage, list_candidate_promotion_lineage,
    list_detector_recognition_lineage, list_minecraft_query_wired_live_action_summaries,
    list_minecraft_spatial_bundle_manifests, list_minecraft_training_job_inspect_reports,
    list_minecraft_training_job_manifests, list_minecraft_training_launch_inspect_reports,
    list_minecraft_training_launch_manifests, list_minecraft_training_package_inspect_reports,
    list_minecraft_training_package_manifests,
    list_minecraft_training_result_artifact_fetch_inspect_reports,
    list_minecraft_training_result_artifact_fetch_manifests,
    list_minecraft_training_result_inspect_reports, list_minecraft_training_result_manifests,
    list_minecraft_training_result_semantic_inspect_reports,
    list_minecraft_training_result_semantic_manifests,
    list_minecraft_training_result_spatial_query_inspect_reports,
    list_minecraft_training_result_spatial_query_manifests, list_observation_snapshots,
    list_verifications, quality_baseline_profile_v1, quality_baseline_verdict_thresholds_probe_v1,
    quality_baseline_verdict_thresholds_trained_render_v1,
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
    TrainingResultNormalizedArtifactKind, TrainingResultReason,
    TrainingResultSemanticInspectReport, TrainingResultSemanticManifest,
    TrainingResultSemanticStatus, TrainingResultStatus,
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
  fn minecraft_training_result_semantic_manifest_lineage_reads_summary() {
    let root = temp_dir("run-read-mc10-semantic-manifest");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_mc10_semantic_manifest");
    let span = dummy_span(&run.root_span_id);

    let manifest = TrainingResultSemanticManifest {
      schema_version: 1,
      generated_at_millis: 99,
      source_training_result_artifact_manifest_path:
        "/tmp/result/minecraft-3dgs-training-result-artifact-manifest.json".to_string(),
      source_training_result_manifest_path: "/tmp/result/minecraft-3dgs-training-result.json"
        .to_string(),
      source_training_job_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json".to_string(),
      source_training_launch_plan_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json"
        .to_string(),
      source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
      source_run_ids: vec!["run-a".to_string(), "run-b".to_string()],
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      source_result_status: TrainingResultStatus::Succeeded,
      normalized_result_dir: "/tmp/result/normalized-result".to_string(),
      semantic_status: TrainingResultSemanticStatus::Ready,
      semantic_reason: None,
      config_path: "/tmp/result/normalized-result/config.yml".to_string(),
      models_dir_path: "/tmp/result/normalized-result/nerfstudio_models".to_string(),
      status_snapshot_path: Some("/tmp/result/normalized-result/job_status.json".to_string()),
      config_trainer: Some("nerfstudio.splatfacto".to_string()),
      checkpoint_files: vec![auv_game_minecraft::TrainingResultSemanticCheckpointRecord {
        relative_path: "step-000001.ckpt".to_string(),
        byte_size: 32,
      }],
      checkpoint_count: 1,
      known_limits: vec!["semantic gate only".to_string()],
    };

    let artifacts = vec![stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_SEMANTIC_ROLE,
      "minecraft-3dgs-training-result-semantic.json",
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
      .read_run("run_read_mc10_semantic_manifest")
      .expect("run should read back");

    let extracted = extract_minecraft_training_result_semantic_manifests(&store, &canonical)
      .expect("manifest should extract");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0]
      .manifest
      .as_ref()
      .expect("summary should be present");
    assert_eq!(summary.semantic_status, "ready");
    assert_eq!(summary.checkpoint_count, 1);
    assert_eq!(summary.checkpoint_files.len(), 1);
    assert_eq!(
      summary.checkpoint_files[0].relative_path,
      "step-000001.ckpt"
    );
    let serialized = serde_json::to_string(summary).expect("summary should serialize");
    assert!(!serialized.contains("generated_at_millis"));

    let listed =
      list_minecraft_training_result_semantic_manifests(&store, "run_read_mc10_semantic_manifest")
        .expect("manifest should list");
    assert_eq!(listed, extracted);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_result_semantic_inspect_report_lineage_reads_summary() {
    let root = temp_dir("run-read-mc10-semantic-inspect");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_mc10_semantic_inspect");
    let span = dummy_span(&run.root_span_id);

    let report = TrainingResultSemanticInspectReport {
      schema_version: 1,
      generated_at_millis: 99,
      training_result_semantic_manifest_path:
        "/tmp/result/minecraft-3dgs-training-result-semantic.json".to_string(),
      source_training_result_artifact_manifest_path:
        "/tmp/result/minecraft-3dgs-training-result-artifact-manifest.json".to_string(),
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
      source_result_status: TrainingResultStatus::Succeeded,
      normalized_result_dir: "/tmp/result/normalized-result".to_string(),
      semantic_status: TrainingResultSemanticStatus::Ready,
      semantic_reason: None,
      config_yaml_parsed: true,
      config_trainer: Some("nerfstudio.splatfacto".to_string()),
      config_backend_matches: true,
      models_dir_readable: true,
      status_snapshot_present: true,
      checkpoint_count: 1,
      warnings: vec![],
      known_limits: vec!["semantic inspect only".to_string()],
    };

    let artifacts = vec![stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_SEMANTIC_INSPECT_ROLE,
      "minecraft-3dgs-training-result-semantic-inspect.json",
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
      .read_run("run_read_mc10_semantic_inspect")
      .expect("run should read back");

    let extracted = extract_minecraft_training_result_semantic_inspect_reports(&store, &canonical)
      .expect("report should extract");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0]
      .report
      .as_ref()
      .expect("summary should be present");
    assert!(summary.config_yaml_parsed);
    assert!(summary.config_backend_matches);
    assert_eq!(summary.checkpoint_count, 1);
    let serialized = serde_json::to_string(summary).expect("summary should serialize");
    assert!(!serialized.contains("generated_at_millis"));

    let listed = list_minecraft_training_result_semantic_inspect_reports(
      &store,
      "run_read_mc10_semantic_inspect",
    )
    .expect("report should list");
    assert_eq!(listed, extracted);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_result_semantic_manifest_lineage_reports_non_json_mime() {
    let root = temp_dir("run-read-mc10-semantic-manifest-non-json");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_mc10_semantic_manifest_non_json");
    let span = dummy_span(&run.root_span_id);

    let artifacts = vec![stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_SEMANTIC_ROLE,
      "minecraft-3dgs-training-result-semantic.txt",
      "not json",
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = list_minecraft_training_result_semantic_manifests(
      &store,
      "run_read_mc10_semantic_manifest_non_json",
    )
    .expect("manifest lineage should list");
    assert_eq!(extracted.len(), 1);
    assert!(extracted[0].manifest.is_none());
    assert!(
      extracted[0]
        .issue
        .as_deref()
        .is_some_and(|issue| issue.contains("mime_type text/plain is not JSON"))
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_result_semantic_inspect_report_lineage_reports_parse_failure() {
    let root = temp_dir("run-read-mc10-semantic-inspect-malformed");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_mc10_semantic_inspect_malformed");
    let span = dummy_span(&run.root_span_id);

    let mut inspect_artifact = stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_SEMANTIC_INSPECT_ROLE,
      "minecraft-3dgs-training-result-semantic-inspect.json",
      "{ malformed",
    );
    inspect_artifact.mime_type = "application/json".to_string();

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts: vec![inspect_artifact],
      })
      .expect("run snapshot should persist");

    let extracted = list_minecraft_training_result_semantic_inspect_reports(
      &store,
      "run_read_mc10_semantic_inspect_malformed",
    )
    .expect("report lineage should list");
    assert_eq!(extracted.len(), 1);
    assert!(extracted[0].report.is_none());
    assert!(
      extracted[0]
        .issue
        .as_deref()
        .unwrap_or_default()
        .contains("failed to parse")
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_result_holdout_preview_manifest_lineage_reads_summary() {
    use auv_game_minecraft::{
      HoldoutFrameSelection, HoldoutFrameWitness, HoldoutPreviewStatus,
      TrainingResultHoldoutPreviewInspectReport, TrainingResultHoldoutPreviewManifest,
    };
    let root = temp_dir("run-read-mc16-holdout-manifest");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_mc16_holdout_manifest");
    let span = dummy_span(&run.root_span_id);
    let witness = HoldoutFrameWitness {
      frame_index: 6,
      spatial_frame_id: "frame-355416".to_string(),
      screenshot_path: "/tmp/scene-packet/frames/frame_000006.png".to_string(),
      frame_json_path: "/tmp/scene-packet/frames/frame_000006.json".to_string(),
    };
    let manifest = TrainingResultHoldoutPreviewManifest {
      schema_version: 1,
      generated_at_millis: 1,
      training_result_semantic_manifest_path:
        "/tmp/result/minecraft-3dgs-training-result-semantic.json".to_string(),
      source_training_result_artifact_manifest_path:
        "/tmp/result/minecraft-3dgs-training-result-artifact-manifest.json".to_string(),
      source_training_result_manifest_path: "/tmp/result/minecraft-3dgs-training-result.json"
        .to_string(),
      source_training_job_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json".to_string(),
      source_training_launch_plan_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json"
        .to_string(),
      source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle.json".to_string()],
      source_run_ids: vec!["run-1".to_string()],
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      normalized_result_dir: "/tmp/normalized".to_string(),
      holdout_frame_index: 6,
      holdout_frame: Some(witness.clone()),
      basis_checkpoint_path: Some("/tmp/normalized/nerfstudio_models/step-000001.ckpt".to_string()),
      holdout_screenshot_path: Some(witness.screenshot_path.clone()),
      reference_overlay_path: Some("/tmp/holdout/holdout_overlay_frame_000006.png".to_string()),
      status: HoldoutPreviewStatus::Ready,
      reason: None,
      known_limits: vec!["holdout preview only".to_string()],
    };
    let inspect_report = TrainingResultHoldoutPreviewInspectReport {
      schema_version: 1,
      generated_at_millis: 1,
      training_result_holdout_preview_manifest_path:
        "/tmp/holdout/minecraft-3dgs-training-result-holdout-preview.json".to_string(),
      training_result_semantic_manifest_path: manifest
        .training_result_semantic_manifest_path
        .clone(),
      source_training_result_artifact_manifest_path: manifest
        .source_training_result_artifact_manifest_path
        .clone(),
      source_training_result_manifest_path: manifest.source_training_result_manifest_path.clone(),
      source_training_job_manifest_path: manifest.source_training_job_manifest_path.clone(),
      source_training_launch_plan_path: manifest.source_training_launch_plan_path.clone(),
      source_training_package_manifest_path: manifest.source_training_package_manifest_path.clone(),
      source_scene_packet_manifest_path: manifest.source_scene_packet_manifest_path.clone(),
      source_bundle_manifest_paths: manifest.source_bundle_manifest_paths.clone(),
      source_run_ids: manifest.source_run_ids.clone(),
      trainer_backend: manifest.trainer_backend.clone(),
      job_backend: manifest.job_backend.clone(),
      normalized_result_dir: manifest.normalized_result_dir.clone(),
      holdout_frame_index: 6,
      holdout_frame: Some(witness),
      basis_checkpoint_path: manifest.basis_checkpoint_path.clone(),
      holdout_screenshot_path: manifest.holdout_screenshot_path.clone(),
      reference_overlay_path: manifest.reference_overlay_path.clone(),
      status: HoldoutPreviewStatus::Ready,
      reason: None,
      holdout_frame_selection: HoldoutFrameSelection::LastInGame,
      checkpoint_count: 1,
      scene_packet_frame_count: 6,
      warnings: vec![],
      known_limits: vec!["holdout inspect only".to_string()],
    };

    let artifacts = vec![
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        0,
        crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_HOLDOUT_PREVIEW_ROLE,
        "minecraft-3dgs-training-result-holdout-preview.json",
        &manifest,
      ),
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        1,
        crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_HOLDOUT_PREVIEW_INSPECT_ROLE,
        "minecraft-3dgs-training-result-holdout-preview-inspect.json",
        &inspect_report,
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
      .read_run("run_read_mc16_holdout_manifest")
      .expect("run should read back");
    let extracted = extract_minecraft_training_result_holdout_preview_manifests(&store, &canonical)
      .expect("extract holdout manifests");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0].manifest.as_ref().expect("summary");
    assert_eq!(summary.status, "ready");
    assert_eq!(summary.holdout_frame_index, 6);
    assert_eq!(
      summary.basis_checkpoint_path.as_deref(),
      Some("/tmp/normalized/nerfstudio_models/step-000001.ckpt")
    );

    let reports =
      extract_minecraft_training_result_holdout_preview_inspect_reports(&store, &canonical)
        .expect("extract holdout inspect");
    assert_eq!(reports.len(), 1);
    assert_eq!(
      reports[0].report.as_ref().unwrap().holdout_frame_selection,
      "last_in_game"
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_holdout_render_quality_manifest_lineage_reads_summary() {
    use auv_game_minecraft::{
      HoldoutFrameWitness, HoldoutPreviewStatus, HoldoutRenderQualityBackend,
      HoldoutRenderQualityMetrics, HoldoutRenderQualityStatus, HoldoutRenderQualityVerdict,
      TrainingResultHoldoutPreviewManifest, TrainingResultHoldoutRenderQualityInspectReport,
      TrainingResultHoldoutRenderQualityManifest,
    };
    let root = temp_dir("run-read-mc17-holdout-quality-manifest");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_mc17_holdout_quality_manifest");
    let span = dummy_span(&run.root_span_id);
    let witness = HoldoutFrameWitness {
      frame_index: 6,
      spatial_frame_id: "frame-355416".to_string(),
      screenshot_path: "/tmp/scene-packet/frames/frame_000006.png".to_string(),
      frame_json_path: "/tmp/scene-packet/frames/frame_000006.json".to_string(),
    };
    let holdout_preview_manifest = TrainingResultHoldoutPreviewManifest {
      schema_version: 1,
      generated_at_millis: 1,
      training_result_semantic_manifest_path:
        "/tmp/result/minecraft-3dgs-training-result-semantic.json".to_string(),
      source_training_result_artifact_manifest_path:
        "/tmp/result/minecraft-3dgs-training-result-artifact-manifest.json".to_string(),
      source_training_result_manifest_path: "/tmp/result/minecraft-3dgs-training-result.json"
        .to_string(),
      source_training_job_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json".to_string(),
      source_training_launch_plan_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json"
        .to_string(),
      source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle.json".to_string()],
      source_run_ids: vec!["run-1".to_string()],
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      normalized_result_dir: "/tmp/normalized".to_string(),
      holdout_frame_index: 6,
      holdout_frame: Some(witness.clone()),
      basis_checkpoint_path: Some("/tmp/normalized/nerfstudio_models/step-000001.ckpt".to_string()),
      holdout_screenshot_path: Some(witness.screenshot_path.clone()),
      reference_overlay_path: Some("/tmp/holdout/holdout_overlay_frame_000006.png".to_string()),
      status: HoldoutPreviewStatus::Ready,
      reason: None,
      known_limits: vec!["holdout preview only".to_string()],
    };
    let manifest = TrainingResultHoldoutRenderQualityManifest {
      schema_version: 1,
      generated_at_millis: 1,
      training_result_semantic_manifest_path: holdout_preview_manifest
        .training_result_semantic_manifest_path
        .clone(),
      holdout_preview_manifest_path:
        "/tmp/holdout/minecraft-3dgs-training-result-holdout-preview.json".to_string(),
      source_training_result_artifact_manifest_path: holdout_preview_manifest
        .source_training_result_artifact_manifest_path
        .clone(),
      source_training_result_manifest_path: holdout_preview_manifest
        .source_training_result_manifest_path
        .clone(),
      source_training_job_manifest_path: holdout_preview_manifest
        .source_training_job_manifest_path
        .clone(),
      source_training_launch_plan_path: holdout_preview_manifest
        .source_training_launch_plan_path
        .clone(),
      source_training_package_manifest_path: holdout_preview_manifest
        .source_training_package_manifest_path
        .clone(),
      source_scene_packet_manifest_path: holdout_preview_manifest
        .source_scene_packet_manifest_path
        .clone(),
      source_bundle_manifest_paths: holdout_preview_manifest
        .source_bundle_manifest_paths
        .clone(),
      source_run_ids: holdout_preview_manifest.source_run_ids.clone(),
      trainer_backend: holdout_preview_manifest.trainer_backend.clone(),
      job_backend: holdout_preview_manifest.job_backend.clone(),
      normalized_result_dir: holdout_preview_manifest.normalized_result_dir.clone(),
      holdout_frame_index: 6,
      holdout_frame: Some(witness.clone()),
      basis_checkpoint_path: holdout_preview_manifest.basis_checkpoint_path.clone(),
      holdout_screenshot_path: holdout_preview_manifest.holdout_screenshot_path.clone(),
      rendered_image_path: Some("/tmp/holdout/rendered_frame_000006.png".to_string()),
      render_backend: HoldoutRenderQualityBackend::ExternalCommand,
      image_size_match: true,
      source_image_size: None,
      rendered_image_size: None,
      metrics: Some(HoldoutRenderQualityMetrics {
        l1_mean: Some(0.01),
        mse: Some(0.002),
        psnr: Some(27.0),
        ssim: None,
      }),
      status: HoldoutRenderQualityStatus::Ready,
      reason: None,
      verdict: HoldoutRenderQualityVerdict::MeasuredOnly,
      known_limits: vec!["render quality evidence only".to_string()],
    };
    let inspect_report = TrainingResultHoldoutRenderQualityInspectReport {
      schema_version: 1,
      generated_at_millis: 1,
      training_result_holdout_render_quality_manifest_path:
        "/tmp/holdout/minecraft-3dgs-holdout-render-quality.json".to_string(),
      training_result_semantic_manifest_path: manifest
        .training_result_semantic_manifest_path
        .clone(),
      holdout_preview_manifest_path: manifest.holdout_preview_manifest_path.clone(),
      source_training_result_artifact_manifest_path: manifest
        .source_training_result_artifact_manifest_path
        .clone(),
      source_training_result_manifest_path: manifest.source_training_result_manifest_path.clone(),
      source_training_job_manifest_path: manifest.source_training_job_manifest_path.clone(),
      source_training_launch_plan_path: manifest.source_training_launch_plan_path.clone(),
      source_training_package_manifest_path: manifest.source_training_package_manifest_path.clone(),
      source_scene_packet_manifest_path: manifest.source_scene_packet_manifest_path.clone(),
      source_bundle_manifest_paths: manifest.source_bundle_manifest_paths.clone(),
      source_run_ids: manifest.source_run_ids.clone(),
      trainer_backend: manifest.trainer_backend.clone(),
      job_backend: manifest.job_backend.clone(),
      normalized_result_dir: manifest.normalized_result_dir.clone(),
      holdout_frame_index: 6,
      holdout_frame: Some(witness),
      basis_checkpoint_path: manifest.basis_checkpoint_path.clone(),
      holdout_screenshot_path: manifest.holdout_screenshot_path.clone(),
      rendered_image_path: manifest.rendered_image_path.clone(),
      render_backend: manifest.render_backend.clone(),
      image_size_match: true,
      l1_mean_available: true,
      mse_available: true,
      psnr_available: true,
      ssim_available: false,
      metrics: manifest.metrics.clone(),
      status: HoldoutRenderQualityStatus::Ready,
      reason: None,
      verdict: HoldoutRenderQualityVerdict::MeasuredOnly,
      warnings: vec![],
      known_limits: vec!["render quality inspect only".to_string()],
    };

    let artifacts = vec![
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        0,
        crate::minecraft::MINECRAFT_3DGS_HOLDOUT_RENDER_QUALITY_ROLE,
        "minecraft-3dgs-holdout-render-quality.json",
        &manifest,
      ),
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        1,
        crate::minecraft::MINECRAFT_3DGS_HOLDOUT_RENDER_QUALITY_INSPECT_ROLE,
        "minecraft-3dgs-holdout-render-quality-inspect.json",
        &inspect_report,
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
      .read_run("run_read_mc17_holdout_quality_manifest")
      .expect("run should read back");
    let extracted = extract_minecraft_holdout_render_quality_manifests(&store, &canonical)
      .expect("extract holdout render quality manifests");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0].manifest.as_ref().expect("summary");
    assert_eq!(summary.status, "ready");
    assert_eq!(summary.verdict, "measured_only");
    assert_eq!(summary.image_size_match, true);
    assert_eq!(summary.metrics.as_ref().unwrap().l1_mean, Some(0.01));

    let reports = extract_minecraft_holdout_render_quality_inspect_reports(&store, &canonical)
      .expect("extract holdout render quality inspect");
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].report.as_ref().unwrap().holdout_frame_index, 6);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_result_spatial_query_manifest_lineage_reads_summary() {
    use auv_driver::geometry::Point;
    use auv_game_minecraft::{
      BlockPosition, TrainingResultSpatialQueryBackend,
      TrainingResultSpatialQueryComparisonVerdict, TrainingResultSpatialQueryKind,
      TrainingResultSpatialQueryManifest, TrainingResultSpatialQueryStatus,
    };
    let root = temp_dir("run-read-mc13-query-manifest");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_mc13_query_manifest");
    let span = dummy_span(&run.root_span_id);

    let manifest = TrainingResultSpatialQueryManifest {
      schema_version: 1,
      generated_at_millis: 99,
      training_result_semantic_manifest_path: "/tmp/semantic.json".to_string(),
      source_training_result_artifact_manifest_path:
        "/tmp/d11/minecraft-3dgs-training-result-artifact-manifest.json".to_string(),
      source_training_result_manifest_path: "/tmp/d7/minecraft-3dgs-training-result.json"
        .to_string(),
      source_training_job_manifest_path: "/tmp/d6/minecraft-3dgs-training-job.json".to_string(),
      source_training_launch_plan_path: "/tmp/d5/minecraft-3dgs-training-launch-plan.json"
        .to_string(),
      source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
      source_run_ids: vec!["run-a".to_string()],
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      normalized_result_dir: "/tmp/result/normalized-result".to_string(),
      query_kind: TrainingResultSpatialQueryKind::BlockProjection,
      target_block: BlockPosition::new(511, 73, 728),
      target_face: None,
      target_semantics: auv_game_minecraft::MinecraftTargetSemantics::HitFaceCenter,
      selected_backend: Some(TrainingResultSpatialQueryBackend::ProjectionReference),
      status: TrainingResultSpatialQueryStatus::Answered,
      reason: None,
      visibility: Some(auv_game_minecraft::ProjectionVisibility::Visible),
      screen_point: Some(Point { x: 854.0, y: 480.0 }),
      match_radius_px: Some(8.0),
      confidence: Some(0.9),
      basis_frame_id: Some("frame-355416".to_string()),
      comparison_verdict: Some(TrainingResultSpatialQueryComparisonVerdict::ReferenceOnly),
      known_limits: vec!["projection_reference only".to_string()],
    };

    let artifacts = vec![stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_QUERY_ROLE,
      "minecraft-3dgs-training-result-query.json",
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

    let extracted = extract_minecraft_training_result_spatial_query_manifests(
      &store,
      &store.read_run("run_read_mc13_query_manifest").expect("run"),
    )
    .expect("manifest should extract");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0]
      .manifest
      .as_ref()
      .expect("summary should be present");
    assert_eq!(summary.status, "answered");
    assert_eq!(summary.target_block, "511,73,728");
    assert_eq!(
      summary.selected_backend.as_deref(),
      Some("projection_reference")
    );
    assert_eq!(summary.visibility.as_deref(), Some("visible"));
    assert!(summary.screen_point.is_some());
    assert_eq!(
      list_minecraft_training_result_spatial_query_manifests(
        &store,
        "run_read_mc13_query_manifest"
      )
      .expect("list")
      .len(),
      1
    );

    let readiness = derive_minecraft_training_result_spatial_query_action_readiness(&extracted[0]);
    assert_eq!(readiness.action_eligibility, "click_ready");
    assert!(readiness.window_point.is_some());
    assert!(readiness.refusal_reason.is_none());
    assert!(readiness.issue.is_none());

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn osu_visual_truth_spatial_query_action_readiness_three_states() {
    use auv_tracing_driver::trace::{ArtifactId, RunId, SpanId};

    fn lineage(
      artifact_id: &str,
      summary: super::OsuVisualTruthSpatialQueryManifestSummary,
    ) -> super::OsuVisualTruthSpatialQueryManifestLineage {
      super::OsuVisualTruthSpatialQueryManifestLineage {
        artifact: ArtifactRefLineage {
          run_id: RunId::new("run_osu_readiness"),
          artifact_id: ArtifactId::new(artifact_id),
          span_id: SpanId::new("span_osu_readiness"),
          captured_event_id: None,
          role: Some(crate::osu::OSU_VISUAL_TRUTH_SPATIAL_QUERY_ROLE.to_string()),
          path: Some(format!("artifacts/{artifact_id}.json")),
          summary: Some("osu spatial query manifest".to_string()),
          resolved: true,
        },
        manifest: Some(summary),
        issue: None,
      }
    }

    fn base_summary() -> super::OsuVisualTruthSpatialQueryManifestSummary {
      super::OsuVisualTruthSpatialQueryManifestSummary {
        schema_version: 1,
        visual_truth_semantic_manifest_path: "/tmp/semantic.json".to_string(),
        source_run_artifact_dir: "/tmp/run".to_string(),
        object_index: 0,
        capture_phase: "before_dispatch".to_string(),
        object_kind: Some("circle".to_string()),
        query_backend: "playfield_projection_reference".to_string(),
        status: "answered".to_string(),
        reason: None,
        pixel_visibility: Some("inside_capture".to_string()),
        pixel_x: Some(400.0),
        pixel_y: Some(300.0),
        match_radius_px: Some(20.0),
        capture_width: Some(800),
        capture_height: Some(600),
        known_limits: vec![],
      }
    }

    let click_ready = super::derive_osu_visual_truth_spatial_query_action_readiness(&lineage(
      "artifact_click_ready",
      base_summary(),
    ));
    assert_eq!(click_ready.action_eligibility, "click_ready");
    assert!(click_ready.pixel_point.is_some());

    let mut outside = base_summary();
    outside.pixel_visibility = Some("outside_capture".to_string());
    let outside_capture = super::derive_osu_visual_truth_spatial_query_action_readiness(&lineage(
      "artifact_outside_capture",
      outside,
    ));
    assert_eq!(outside_capture.action_eligibility, "answer_non_clickable");

    let mut failed = base_summary();
    failed.status = "failed".to_string();
    failed.reason = Some("target_absent_from_visual_truth".to_string());
    failed.pixel_visibility = None;
    failed.pixel_x = None;
    failed.pixel_y = None;
    let not_consumable = super::derive_osu_visual_truth_spatial_query_action_readiness(&lineage(
      "artifact_failed",
      failed,
    ));
    assert_eq!(not_consumable.action_eligibility, "not_consumable");
  }

  #[test]
  fn minecraft_training_result_spatial_query_action_readiness_inherits_manifest_issue() {
    use auv_tracing_driver::trace::{ArtifactId, RunId, SpanId};

    let lineage = MinecraftTrainingResultSpatialQueryManifestLineage {
      artifact: ArtifactRefLineage {
        run_id: RunId::new("run_issue"),
        artifact_id: ArtifactId::new("artifact_issue"),
        span_id: SpanId::new("span_issue"),
        captured_event_id: None,
        role: Some(crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_QUERY_ROLE.to_string()),
        path: Some("artifacts/query.json".to_string()),
        summary: Some("spatial query manifest".to_string()),
        resolved: true,
      },
      manifest: None,
      issue: Some(
        "minecraft training result spatial query manifest mime_type text/plain is not JSON"
          .to_string(),
      ),
    };

    let readiness = derive_minecraft_training_result_spatial_query_action_readiness(&lineage);
    assert_eq!(readiness.action_eligibility, "n/a");
    assert!(readiness.window_point.is_none());
    assert!(
      readiness
        .issue
        .as_deref()
        .is_some_and(|issue| issue.contains("mime_type"))
    );
  }

  #[test]
  fn minecraft_training_result_spatial_query_inspect_report_lineage_reads_summary() {
    use auv_game_minecraft::{
      BlockPosition, TrainingResultSpatialQueryBackend,
      TrainingResultSpatialQueryComparisonVerdict, TrainingResultSpatialQueryInspectReport,
      TrainingResultSpatialQueryKind, TrainingResultSpatialQueryManifest,
      TrainingResultSpatialQueryStatus,
    };
    let root = temp_dir("run-read-mc13-query-inspect");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_mc13_query_inspect");
    let span = dummy_span(&run.root_span_id);

    let shared_manifest = TrainingResultSpatialQueryManifest {
      schema_version: 1,
      generated_at_millis: 99,
      training_result_semantic_manifest_path: "/tmp/semantic.json".to_string(),
      source_training_result_artifact_manifest_path:
        "/tmp/d11/minecraft-3dgs-training-result-artifact-manifest.json".to_string(),
      source_training_result_manifest_path: "/tmp/d7/minecraft-3dgs-training-result.json"
        .to_string(),
      source_training_job_manifest_path: "/tmp/d6/minecraft-3dgs-training-job.json".to_string(),
      source_training_launch_plan_path: "/tmp/d5/minecraft-3dgs-training-launch-plan.json"
        .to_string(),
      source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
      source_run_ids: vec!["run-a".to_string()],
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      normalized_result_dir: "/tmp/result/normalized-result".to_string(),
      query_kind: TrainingResultSpatialQueryKind::BlockProjection,
      target_block: BlockPosition::new(511, 73, 728),
      target_face: None,
      target_semantics: auv_game_minecraft::MinecraftTargetSemantics::HitFaceCenter,
      selected_backend: Some(TrainingResultSpatialQueryBackend::ProjectionReference),
      status: TrainingResultSpatialQueryStatus::Answered,
      reason: None,
      visibility: Some(auv_game_minecraft::ProjectionVisibility::Visible),
      screen_point: None,
      match_radius_px: None,
      confidence: None,
      basis_frame_id: Some("frame-355416".to_string()),
      comparison_verdict: Some(TrainingResultSpatialQueryComparisonVerdict::ReferenceOnly),
      known_limits: vec!["fixture".to_string()],
    };

    let inspect = TrainingResultSpatialQueryInspectReport {
      schema_version: shared_manifest.schema_version,
      generated_at_millis: shared_manifest.generated_at_millis,
      training_result_spatial_query_manifest_path:
        "/tmp/query/minecraft-3dgs-training-result-query.json".to_string(),
      training_result_semantic_manifest_path: shared_manifest
        .training_result_semantic_manifest_path
        .clone(),
      source_training_result_artifact_manifest_path: shared_manifest
        .source_training_result_artifact_manifest_path
        .clone(),
      source_training_result_manifest_path: shared_manifest
        .source_training_result_manifest_path
        .clone(),
      source_training_job_manifest_path: shared_manifest.source_training_job_manifest_path.clone(),
      source_training_launch_plan_path: shared_manifest.source_training_launch_plan_path.clone(),
      source_training_package_manifest_path: shared_manifest
        .source_training_package_manifest_path
        .clone(),
      source_scene_packet_manifest_path: shared_manifest.source_scene_packet_manifest_path.clone(),
      source_bundle_manifest_paths: shared_manifest.source_bundle_manifest_paths.clone(),
      source_run_ids: shared_manifest.source_run_ids.clone(),
      trainer_backend: shared_manifest.trainer_backend.clone(),
      job_backend: shared_manifest.job_backend.clone(),
      normalized_result_dir: shared_manifest.normalized_result_dir.clone(),
      query_kind: shared_manifest.query_kind,
      target_block: shared_manifest.target_block,
      target_face: shared_manifest.target_face,
      target_semantics: shared_manifest.target_semantics,
      selected_backend: shared_manifest.selected_backend,
      status: shared_manifest.status,
      reason: shared_manifest.reason,
      visibility: shared_manifest.visibility,
      screen_point: shared_manifest.screen_point,
      match_radius_px: shared_manifest.match_radius_px,
      confidence: shared_manifest.confidence,
      basis_frame_id: shared_manifest.basis_frame_id.clone(),
      comparison_verdict: shared_manifest.comparison_verdict,
      provider_status: TrainingResultSpatialQueryStatus::Blocked,
      provider_reason: None,
      provider_message: None,
      reference_status: TrainingResultSpatialQueryStatus::Answered,
      reference_reason: None,
      reference_basis_frame_id: Some("frame-355416".to_string()),
      reference_source_frame_json_path: Some(
        "/tmp/scene-packet/frames/frame_000001.json".to_string(),
      ),
      reference_screenshot_path: None,
      scene_packet_frame_count: 12,
      warnings: vec!["provider not configured".to_string()],
      known_limits: shared_manifest.known_limits.clone(),
    };

    let artifacts = vec![stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_QUERY_INSPECT_ROLE,
      "minecraft-3dgs-training-result-query-inspect.json",
      &inspect,
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = list_minecraft_training_result_spatial_query_inspect_reports(
      &store,
      "run_read_mc13_query_inspect",
    )
    .expect("inspect should list");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0]
      .report
      .as_ref()
      .expect("summary should be present");
    assert_eq!(summary.provider_status, "blocked");
    assert_eq!(summary.reference_status, "answered");
    assert_eq!(summary.scene_packet_frame_count, 12);
    assert_eq!(summary.target_block, "511,73,728");

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_result_spatial_query_manifest_lineage_reports_non_json_mime() {
    let root = temp_dir("run-read-mc13-query-manifest-mime");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_mc13_query_manifest_mime");
    let span = dummy_span(&run.root_span_id);

    let mut manifest_artifact = stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_QUERY_ROLE,
      "minecraft-3dgs-training-result-query.json",
      "{}",
    );
    manifest_artifact.mime_type = "text/plain".to_string();

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts: vec![manifest_artifact],
      })
      .expect("run snapshot should persist");

    let extracted = list_minecraft_training_result_spatial_query_manifests(
      &store,
      "run_read_mc13_query_manifest_mime",
    )
    .expect("manifest lineage should list");
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
  fn minecraft_training_result_spatial_query_inspect_report_lineage_reports_parse_failure() {
    let root = temp_dir("run-read-mc13-query-inspect-malformed");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_mc13_query_inspect_malformed");
    let span = dummy_span(&run.root_span_id);

    let mut inspect_artifact = stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_QUERY_INSPECT_ROLE,
      "minecraft-3dgs-training-result-query-inspect.json",
      "{ malformed",
    );
    inspect_artifact.mime_type = "application/json".to_string();

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts: vec![inspect_artifact],
      })
      .expect("run snapshot should persist");

    let extracted = list_minecraft_training_result_spatial_query_inspect_reports(
      &store,
      "run_read_mc13_query_inspect_malformed",
    )
    .expect("report lineage should list");
    assert_eq!(extracted.len(), 1);
    assert!(extracted[0].report.is_none());
    assert!(
      extracted[0]
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
      &json!({
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
      submission_recorded_at_millis: Some(1),
      accepted_by_provider: true,
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
      submission_recorded_at_millis: None,
      accepted_by_provider: false,
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
      status_message: Some("provider succeeded".to_string()),
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
    assert_eq!(
      summary.status_message.as_deref(),
      Some("provider succeeded")
    );
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
      status_message: Some("legacy adapter failure".to_string()),
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
      summary.status_message.as_deref(),
      Some("legacy adapter failure")
    );
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
      &json!({"fixture": "candidate-promotion"}),
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
      &json!({"fixture": "candidate-action-decision"}),
    );
    let operation_result_artifact = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      1,
      "operation-result",
      "candidate-action-operation-result.json",
      &json!({"fixture": "operation-result"}),
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

    let transitions = extract_action_transition_lineage(&store, &canonical)
      .expect("action transition lineage should extract");
    assert_eq!(transitions.len(), 5);
    assert_eq!(transitions[0].status, ActionTransitionLineageStatus::Ready);
    assert_eq!(
      transitions[0]
        .effective_decision
        .as_ref()
        .map(|decision| decision.selected_method.as_str()),
      Some("pointer-click")
    );
    assert!(transitions[0].driver_result.is_some());
    assert_eq!(
      transitions[0].verification.verification_outcome,
      "activation_only"
    );
    assert_eq!(transitions[3].verification.verification_outcome, "passed");
    assert_eq!(
      transitions[4].status,
      ActionTransitionLineageStatus::Malformed
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn action_transition_lineage_surfaces_plan_delivery_mismatch_from_l8b() {
    let root = temp_dir("run-read-action-transition-mismatch");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_action_transition_mismatch");
    let span = dummy_span(&run.root_span_id);

    let mut l8a = candidate_action_decision_artifact(
      None,
      "decision_ax_plan",
      "promotion_ready",
      "promoted-item_end_turn",
    );
    l8a.action_resolver_decision.selected_method = "ax-action".to_string();
    l8a.action_resolver_decision.primary_method = "ax-action".to_string();
    l8a.action_resolver_decision.policy = "candidate-ax-node".to_string();

    let decision_artifact = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      CANDIDATE_ACTION_DECISION_ARTIFACT_ROLE,
      "candidate-action-decision-ax-plan.json",
      &l8a,
    );
    let operation_result_artifact = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      1,
      "operation-result",
      "candidate-action-operation-result.json",
      &json!({"fixture": "operation-result"}),
    );

    let mut execution = candidate_action_execution_artifact(
      ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: decision_artifact.artifact_id.clone(),
        span_id: span.span_id.clone(),
        captured_event_id: decision_artifact.event_id.clone(),
      },
      Some(ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: operation_result_artifact.artifact_id.clone(),
        span_id: span.span_id.clone(),
        captured_event_id: operation_result_artifact.event_id.clone(),
      }),
      "execution_plan_delivery_mismatch",
    );
    execution.action_resolver_decision.selected_method = "pointer-click".to_string();
    execution
      .known_limits
      .push("plan_delivery_mismatch: l8a_selected=ax-action effective=pointer-click".to_string());

    let artifacts = vec![
      decision_artifact,
      operation_result_artifact,
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        2,
        CANDIDATE_ACTION_EXECUTION_ARTIFACT_ROLE,
        "candidate-action-execution-mismatch.json",
        &execution,
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
      .read_run("run_read_action_transition_mismatch")
      .expect("run should read back");
    let transitions = extract_action_transition_lineage(&store, &canonical)
      .expect("action transition lineage should extract");
    assert_eq!(transitions.len(), 1);
    assert_eq!(
      transitions[0].status,
      ActionTransitionLineageStatus::Partial
    );
    assert_eq!(
      transitions[0]
        .planned_decision
        .as_ref()
        .map(|decision| decision.selected_method.as_str()),
      Some("ax-action")
    );
    assert_eq!(
      transitions[0]
        .effective_decision
        .as_ref()
        .map(|decision| decision.selected_method.as_str()),
      Some("pointer-click")
    );
    assert!(
      transitions[0]
        .known_limits
        .iter()
        .any(|limit| limit.starts_with("plan_delivery_mismatch:"))
    );

    let listed = list_action_transition_lineage(&store, "run_read_action_transition_mismatch")
      .expect("action transition lineage should list");
    assert_eq!(listed, transitions);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn action_transition_lineage_marks_legacy_missing_decision_as_partial() {
    let root = temp_dir("run-read-action-transition-legacy-missing-decision");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_action_transition_legacy_missing_decision");
    let span = dummy_span(&run.root_span_id);
    let source_decision_artifact = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      CANDIDATE_ACTION_DECISION_ARTIFACT_ROLE,
      "candidate-action-decision-source.json",
      &json!({"fixture": "candidate-action-decision"}),
    );
    let full_execution = candidate_action_execution_artifact(
      ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: source_decision_artifact.artifact_id.clone(),
        span_id: span.span_id.clone(),
        captured_event_id: source_decision_artifact.event_id.clone(),
      },
      None,
      "execution_legacy_missing_decision",
    );
    let legacy = LegacyExecutionFixture {
      artifact_version: "candidate_action_execution_artifact_v0".to_string(),
      execution_id: "execution_legacy_missing_decision".to_string(),
      source_candidate_action_decision_artifact: ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: source_decision_artifact.artifact_id.clone(),
        span_id: span.span_id.clone(),
        captured_event_id: source_decision_artifact.event_id.clone(),
      },
      source_candidate_promotion_artifact: None,
      operation_result_artifact: None,
      source_promotion_id: "promotion_ready".to_string(),
      source_decision_id: "decision_ready".to_string(),
      candidate_local_id: "promoted-item_end_turn".to_string(),
      action_resolver_decision: None,
      operation_result: full_execution.operation_result.clone(),
      verification_result: full_execution.verification_result.clone(),
      detail: json!({
        "input_delivery": "attempted",
        "operation_status": "completed",
      }),
      known_limits: vec!["legacy fixture missing decision and driver result".to_string()],
    };

    let artifacts = vec![
      source_decision_artifact,
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        1,
        CANDIDATE_ACTION_EXECUTION_ARTIFACT_ROLE,
        "candidate-action-execution-legacy-missing-decision.json",
        &legacy,
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
      .read_run("run_read_action_transition_legacy_missing_decision")
      .expect("run should read back");
    let transitions = extract_action_transition_lineage(&store, &canonical)
      .expect("action transition lineage should extract");
    assert_eq!(transitions.len(), 1);
    assert_eq!(
      transitions[0].status,
      ActionTransitionLineageStatus::Partial
    );
    assert_eq!(
      transitions[0].issue.as_deref(),
      Some("missing_action_resolver_decision")
    );
    assert_eq!(
      transitions[0].verification.verification_outcome,
      "activation_only"
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn action_transition_lineage_marks_legacy_missing_driver_as_partial() {
    let root = temp_dir("run-read-action-transition-legacy-missing-driver");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_action_transition_legacy_missing_driver");
    let span = dummy_span(&run.root_span_id);
    let source_decision_artifact = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      CANDIDATE_ACTION_DECISION_ARTIFACT_ROLE,
      "candidate-action-decision-source.json",
      &json!({"fixture": "candidate-action-decision"}),
    );
    let full_execution = candidate_action_execution_artifact(
      ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: source_decision_artifact.artifact_id.clone(),
        span_id: span.span_id.clone(),
        captured_event_id: source_decision_artifact.event_id.clone(),
      },
      None,
      "execution_legacy_missing_driver",
    );
    let legacy = LegacyExecutionFixture {
      artifact_version: "candidate_action_execution_artifact_v0".to_string(),
      execution_id: "execution_legacy_missing_driver".to_string(),
      source_candidate_action_decision_artifact: ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: source_decision_artifact.artifact_id.clone(),
        span_id: span.span_id.clone(),
        captured_event_id: source_decision_artifact.event_id.clone(),
      },
      source_candidate_promotion_artifact: None,
      operation_result_artifact: None,
      source_promotion_id: "promotion_ready".to_string(),
      source_decision_id: "decision_ready".to_string(),
      candidate_local_id: "promoted-item_end_turn".to_string(),
      action_resolver_decision: Some(full_execution.action_resolver_decision.clone()),
      operation_result: full_execution.operation_result.clone(),
      verification_result: full_execution.verification_result.clone(),
      detail: json!({
        "input_delivery": "attempted",
        "operation_status": "completed",
      }),
      known_limits: vec!["legacy fixture missing input_action_result".to_string()],
    };

    let artifacts = vec![
      source_decision_artifact,
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        1,
        CANDIDATE_ACTION_EXECUTION_ARTIFACT_ROLE,
        "candidate-action-execution-legacy-missing-driver.json",
        &legacy,
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
      .read_run("run_read_action_transition_legacy_missing_driver")
      .expect("run should read back");
    let transitions = extract_action_transition_lineage(&store, &canonical)
      .expect("action transition lineage should extract");
    assert_eq!(transitions.len(), 1);
    assert_eq!(
      transitions[0].status,
      ActionTransitionLineageStatus::Partial
    );
    assert_eq!(
      transitions[0].issue.as_deref(),
      Some("missing_input_action_result")
    );
    assert!(transitions[0].effective_decision.is_some());
    assert!(transitions[0].driver_result.is_none());

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

  #[derive(Serialize)]
  struct LegacyExecutionFixture {
    artifact_version: String,
    execution_id: String,
    source_candidate_action_decision_artifact: ArtifactRef,
    source_candidate_promotion_artifact: Option<ArtifactRef>,
    operation_result_artifact: Option<ArtifactRef>,
    source_promotion_id: String,
    source_decision_id: String,
    candidate_local_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    action_resolver_decision: Option<ActionResolverDecision>,
    operation_result: OperationResult,
    verification_result: VerificationResult,
    detail: serde_json::Value,
    known_limits: Vec<String>,
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

  fn mc19_query_manifest_json(
    target_block: (i32, i32, i32),
    status: &str,
    visibility: Option<&str>,
    screen_point: Option<serde_json::Value>,
    reason: Option<&str>,
    selected_backend: Option<&str>,
  ) -> serde_json::Value {
    json!({
      "schema_version": 1,
      "generated_at_millis": 1,
      "training_result_semantic_manifest_path": "/tmp/semantic.json",
      "source_training_result_artifact_manifest_path": "/tmp/artifact.json",
      "source_training_result_manifest_path": "/tmp/result.json",
      "source_training_job_manifest_path": "/tmp/job.json",
      "source_training_launch_plan_path": "/tmp/launch.json",
      "source_training_package_manifest_path": "/tmp/package.json",
      "source_scene_packet_manifest_path": "/tmp/scene-packet.json",
      "source_bundle_manifest_paths": ["/tmp/bundle.json"],
      "source_run_ids": ["run-a"],
      "trainer_backend": "nerfstudio.splatfacto",
      "job_backend": "remote",
      "normalized_result_dir": "/tmp/normalized",
      "query_kind": "block_projection",
      "target_block": {"x": target_block.0, "y": target_block.1, "z": target_block.2},
      "target_face": null,
      "target_semantics": "hit_face_center",
      "selected_backend": selected_backend,
      "status": status,
      "reason": reason,
      "visibility": visibility,
      "screen_point": screen_point,
      "match_radius_px": 8.0,
      "confidence": 0.9,
      "basis_frame_id": "frame-1",
      "comparison_verdict": "reference_only",
      "known_limits": []
    })
  }

  fn mc19_operation_result(
    run_id: &RunId,
    query_artifact_id: &str,
    status: OperationStatus,
    message: &str,
  ) -> OperationResult {
    let query_ref = ArtifactRef {
      artifact_id: ArtifactId::new(query_artifact_id),
      run_id: run_id.clone(),
      span_id: SpanId::new("0000000000000001"),
      captured_event_id: None,
    };
    OperationResult {
      api_version: OPERATION_RESULT_API_VERSION.to_string(),
      run_id: run_id.clone(),
      status,
      operation_id: crate::minecraft_query_live_action::QUERY_WIRED_LIVE_ACTION_OPERATION_ID
        .to_string(),
      evidence_artifacts: vec![query_ref.clone()],
      output: OperationOutput::Acknowledged {
        message: Some(message.to_string()),
      },
      verifications: Vec::new(),
      freshness_basis: Some(crate::contract::FreshnessBasis {
        source_artifact: Some(query_ref),
        source_operation_id: Some("auv.minecraft.query_3dgs_training_result".to_string()),
        notes: vec!["MC-12 spatial query manifest staged in the same run".to_string()],
      }),
      known_limits: vec![
        "mc19_v1_d4_query_wired_live_action_non_stub_click_no_gameplay_verification".to_string(),
      ],
    }
  }

  fn dummy_mc19_event(
    span_id: &SpanId,
    name: &str,
    message: &str,
  ) -> auv_tracing_driver::trace::EventRecordV1Alpha1 {
    auv_tracing_driver::trace::EventRecordV1Alpha1 {
      api_version: auv_tracing_driver::trace::EVENT_API_VERSION.to_string(),
      event_id: EventId::new(format!("event_{name}")),
      span_id: span_id.clone(),
      name: name.to_string(),
      timestamp_millis: 150,
      attributes: BTreeMap::new(),
      message: Some(message.to_string()),
      artifact_ids: Vec::new(),
    }
  }

  fn write_mc19_run_snapshot(
    store: &LocalStore,
    root: &Path,
    run_id: &str,
    events: Vec<auv_tracing_driver::trace::EventRecordV1Alpha1>,
    artifacts: Vec<ArtifactRecordV1Alpha1>,
  ) {
    let run = dummy_run(run_id);
    let span = dummy_span(&run.root_span_id);
    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events,
        artifacts,
      })
      .expect("run snapshot should persist");
    let _ = root;
  }

  #[test]
  fn query_wired_live_action_verification_projection_maps_semantic_pass_and_absent() {
    use crate::contract::{
      OperationStatus, VERIFICATION_RESULT_API_VERSION, VerificationMethod, VerificationResult,
    };

    let run_id = RunId::new("run_verification_projection");
    let absent = super::resolve_query_wired_live_action_verification_projection(
      true,
      Some("artifact_op"),
      Some(&mc19_operation_result(
        &run_id,
        "artifact_query",
        OperationStatus::Completed,
        "dispatched",
      )),
      run_id.as_str(),
      None,
    );
    assert_eq!(absent.verification_outcome, "absent");
    assert!(
      absent
        .verification_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("mc19_v1_d4"))
    );

    let mut operation_result = mc19_operation_result(
      &run_id,
      "artifact_query",
      OperationStatus::Completed,
      "dispatched",
    );
    operation_result.verifications.push(VerificationResult {
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
      observed_label: Some("world diff matched".to_string()),
    });
    let passed = super::resolve_query_wired_live_action_verification_projection(
      true,
      Some("artifact_op"),
      Some(&operation_result),
      run_id.as_str(),
      None,
    );
    assert_eq!(passed.verification_outcome, "passed");
    let mut unreliable_operation_result = mc19_operation_result(
      &run_id,
      "artifact_query",
      OperationStatus::Completed,
      "dispatched",
    );
    unreliable_operation_result
      .verifications
      .push(VerificationResult {
        api_version: VERIFICATION_RESULT_API_VERSION.to_string(),
        method: VerificationMethod::SemanticMatch,
        executed: true,
        state_changed: false,
        semantic_matched: None,
        failure_layer: Some(crate::contract::FailureLayer::VerificationUnreliable),
        evidence: Vec::new(),
        consumed_candidate_ref: None,
        consumed_node_ref: None,
        consumed_recognition_artifact_ref: None,
        consumed_recognition_id: None,
        consumed_recognized_item_id: None,
        observed_label: None,
      });
    unreliable_operation_result.known_limits =
      vec![auv_game_minecraft::MC20_V1_QUERY_WIRED_WITNESS_ABSENT_KNOWN_LIMIT.to_string()];
    let unreliable = super::resolve_query_wired_live_action_verification_projection(
      true,
      Some("artifact_op"),
      Some(&unreliable_operation_result),
      run_id.as_str(),
      None,
    );
    assert_eq!(unreliable.verification_outcome, "unreliable");

    assert_eq!(
      passed.verification_reason.as_deref(),
      Some("world diff matched")
    );

    let mut inconclusive_operation_result = mc19_operation_result(
      &run_id,
      "artifact_query",
      OperationStatus::Completed,
      "dispatched",
    );
    inconclusive_operation_result
      .verifications
      .push(VerificationResult {
        api_version: VERIFICATION_RESULT_API_VERSION.to_string(),
        method: VerificationMethod::SemanticMatch,
        executed: true,
        state_changed: true,
        semantic_matched: None,
        failure_layer: None,
        evidence: Vec::new(),
        consumed_candidate_ref: None,
        consumed_node_ref: None,
        consumed_recognition_artifact_ref: None,
        consumed_recognition_id: None,
        consumed_recognized_item_id: None,
        observed_label: Some("tick advanced".to_string()),
      });
    let inconclusive = super::resolve_query_wired_live_action_verification_projection(
      true,
      Some("artifact_op"),
      Some(&inconclusive_operation_result),
      run_id.as_str(),
      None,
    );
    assert_eq!(inconclusive.verification_outcome, "inconclusive");
    assert_eq!(
      inconclusive.verification_reason.as_deref(),
      Some("tick advanced")
    );

    let not_attempted = super::resolve_query_wired_live_action_verification_projection(
      false,
      None,
      None,
      run_id.as_str(),
      Some("visibility=outside_window"),
    );
    assert_eq!(not_attempted.verification_outcome, "not_attempted");
    assert_eq!(
      not_attempted.verification_reason.as_deref(),
      Some("visibility=outside_window")
    );
  }

  #[test]
  fn minecraft_query_wired_live_action_summary_click_ready_gate() {
    let root = temp_dir("run-read-mc19-click-ready");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = "run_read_mc19_click_ready";
    let run = dummy_run(run_id);
    let span_id = run.root_span_id.clone();
    let query_artifact = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_QUERY_ROLE,
      "minecraft-3dgs-training-result-query.json",
      &mc19_query_manifest_json(
        (511, 73, 728),
        "answered",
        Some("visible"),
        Some(json!({"x": 854.0, "y": 480.0})),
        None,
        Some("projection_reference"),
      ),
    );
    let query_artifact_id = query_artifact.artifact_id.as_str().to_string();
    let operation_result = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      1,
      "operation-result",
      "operation-result.json",
      &mc19_operation_result(
        &run.run_id,
        query_artifact_id.as_str(),
        OperationStatus::Completed,
        "mock live click dispatched",
      ),
    );
    let events = vec![
      dummy_mc19_event(
        &span_id,
        "minecraft.query_wired_live_action.inputs",
        "training_result_semantic_manifest=/tmp/semantic.json target_block=511,73,728 target_app=net.minecraft.client target_title=Minecraft checkpoint_native_provider=false closed_scene_toy_provider=true closed_scene_fixture=/tmp/fixture.json output_dir=/tmp/out",
      ),
      dummy_mc19_event(
        &span_id,
        "minecraft.query_wired_live_action.outcome",
        "attempted=true action_eligibility=click_ready refusal_reason=none query_manifest_path=/tmp/query.json",
      ),
      dummy_mc19_event(
        &span_id,
        "command.resolved",
        "resolved input.clickWindowPoint",
      ),
      dummy_mc19_event(
        &span_id,
        "command.failed",
        "main visible window was not found",
      ),
    ];
    write_mc19_run_snapshot(
      &store,
      &root,
      run_id,
      events,
      vec![query_artifact, operation_result],
    );

    let summary = derive_minecraft_query_wired_live_action_summary(
      &store,
      &store.read_run(run_id).expect("run"),
    )
    .expect("summary should derive");
    assert!(summary.attempted);
    assert_eq!(summary.action_eligibility, "click_ready");
    assert_eq!(summary.readiness_class.as_deref(), Some("ready"));
    assert_eq!(
      summary.dispatch_command.as_deref(),
      Some("input.clickWindowPoint")
    );
    assert!(
      summary
        .dispatch_outcome
        .as_deref()
        .is_some_and(|v| v.starts_with("failed:"))
    );
    assert_eq!(
      summary.mc14_action_eligibility.as_deref(),
      Some("click_ready")
    );
    assert!(summary.window_point.is_some());
    assert_eq!(
      summary.source_readiness_ref.as_deref(),
      Some(
        format!(
          "kind=query_manifest artifact_id={} run_id={}",
          query_artifact_id.as_str(),
          run_id
        )
        .as_str()
      )
    );
    assert_eq!(
      list_minecraft_query_wired_live_action_summaries(&store, run_id)
        .expect("list")
        .len(),
      1
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_query_wired_live_action_summary_answer_non_clickable_gate() {
    let root = temp_dir("run-read-mc19-outside-window");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = "run_read_mc19_outside_window";
    let run = dummy_run(run_id);
    let span_id = run.root_span_id.clone();
    let query_artifact = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_QUERY_ROLE,
      "minecraft-3dgs-training-result-query.json",
      &mc19_query_manifest_json(
        (511, 73, 728),
        "answered",
        Some("outside_window"),
        None,
        None,
        Some("command_provider"),
      ),
    );
    let query_artifact_id = query_artifact.artifact_id.as_str().to_string();
    let operation_result = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      1,
      "operation-result",
      "operation-result.json",
      &mc19_operation_result(
        &run.run_id,
        query_artifact_id.as_str(),
        OperationStatus::Completed,
        "visibility=outside_window",
      ),
    );
    let events = vec![
      dummy_mc19_event(
        &span_id,
        "minecraft.query_wired_live_action.inputs",
        "target_app=net.minecraft.client target_title=Minecraft",
      ),
      dummy_mc19_event(
        &span_id,
        "minecraft.query_wired_live_action.outcome",
        "attempted=false action_eligibility=answer_non_clickable refusal_reason=visibility=outside_window query_manifest_path=/tmp/query.json",
      ),
    ];
    write_mc19_run_snapshot(
      &store,
      &root,
      run_id,
      events,
      vec![query_artifact, operation_result],
    );

    let summary = derive_minecraft_query_wired_live_action_summary(
      &store,
      &store.read_run(run_id).expect("run"),
    )
    .expect("summary should derive");
    assert!(!summary.attempted);
    assert_eq!(summary.action_eligibility, "answer_non_clickable");
    assert_eq!(summary.readiness_class.as_deref(), Some("non_actionable"));
    assert_eq!(
      summary.refusal_reason.as_deref(),
      Some("visibility=outside_window")
    );
    assert!(summary.dispatch_command.is_none());
    assert!(summary.dispatch_outcome.is_none());
    assert_eq!(
      summary.mc14_action_eligibility.as_deref(),
      Some("answer_non_clickable")
    );
    assert_eq!(
      summary.source_readiness_ref.as_deref(),
      Some(
        format!(
          "kind=query_manifest artifact_id={} run_id={}",
          query_artifact_id.as_str(),
          run_id
        )
        .as_str()
      )
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_query_wired_live_action_summary_not_consumable_gate() {
    let root = temp_dir("run-read-mc19-not-consumable");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = "run_read_mc19_not_consumable";
    let run = dummy_run(run_id);
    let span_id = run.root_span_id.clone();
    let query_artifact = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_QUERY_ROLE,
      "minecraft-3dgs-training-result-query.json",
      &mc19_query_manifest_json(
        (9, 9, 9),
        "failed",
        None,
        None,
        Some("target_block_absent_from_scene_packet"),
        None,
      ),
    );
    let query_artifact_id = query_artifact.artifact_id.as_str().to_string();
    let operation_result = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      1,
      "operation-result",
      "operation-result.json",
      &mc19_operation_result(
        &run.run_id,
        query_artifact_id.as_str(),
        OperationStatus::Completed,
        "status=failed reason=target_block_absent_from_scene_packet",
      ),
    );
    let events = vec![
      dummy_mc19_event(
        &span_id,
        "minecraft.query_wired_live_action.inputs",
        "target_app=net.minecraft.client target_title=Minecraft",
      ),
      dummy_mc19_event(
        &span_id,
        "minecraft.query_wired_live_action.outcome",
        "attempted=false action_eligibility=not_consumable refusal_reason=status=failed reason=target_block_absent_from_scene_packet query_manifest_path=/tmp/query.json",
      ),
    ];
    write_mc19_run_snapshot(
      &store,
      &root,
      run_id,
      events,
      vec![query_artifact, operation_result],
    );

    let summary = derive_minecraft_query_wired_live_action_summary(
      &store,
      &store.read_run(run_id).expect("run"),
    )
    .expect("summary should derive");
    assert!(!summary.attempted);
    assert_eq!(summary.action_eligibility, "not_consumable");
    assert_eq!(summary.readiness_class.as_deref(), Some("not_consumable"));
    assert_eq!(
      summary.refusal_reason.as_deref(),
      Some("status=failed reason=target_block_absent_from_scene_packet")
    );
    assert!(summary.dispatch_command.is_none());
    assert!(summary.dispatch_outcome.is_none());
    assert_eq!(
      summary.mc14_action_eligibility.as_deref(),
      Some("not_consumable")
    );
    assert_eq!(
      summary.source_readiness_ref.as_deref(),
      Some(
        format!(
          "kind=query_manifest artifact_id={} run_id={}",
          query_artifact_id.as_str(),
          run_id
        )
        .as_str()
      )
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_query_wired_live_action_summary_event_only_source_readiness_ref() {
    let root = temp_dir("run-read-mc19-event-only");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = "run_read_mc19_event_only";
    let run = dummy_run(run_id);
    let span_id = run.root_span_id.clone();
    let events = vec![dummy_mc19_event(
      &span_id,
      "minecraft.query_wired_live_action.outcome",
      "attempted=false action_eligibility=answer_non_clickable refusal_reason=visibility=outside_window",
    )];
    write_mc19_run_snapshot(&store, &root, run_id, events, vec![]);

    let summary = derive_minecraft_query_wired_live_action_summary(
      &store,
      &store.read_run(run_id).expect("run"),
    )
    .expect("summary should derive");
    assert_eq!(
      summary.source_readiness_ref.as_deref(),
      Some("kind=outcome_event event=minecraft.query_wired_live_action.outcome")
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_query_wired_live_action_summary_manifest_parse_failure_source_readiness_ref_none() {
    let root = temp_dir("run-read-mc19-manifest-parse-failure");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = "run_read_mc19_manifest_parse_failure";
    let run = dummy_run(run_id);
    let span_id = run.root_span_id.clone();
    let query_artifact = stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_QUERY_ROLE,
      "minecraft-3dgs-training-result-query.json",
      "{not valid json",
    );
    let operation_result = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      1,
      "operation-result",
      "operation-result.json",
      &mc19_operation_result(
        &run.run_id,
        query_artifact.artifact_id.as_str(),
        OperationStatus::Completed,
        "manifest unreadable",
      ),
    );
    let events = vec![dummy_mc19_event(
      &span_id,
      "minecraft.query_wired_live_action.outcome",
      "attempted=false action_eligibility=not_consumable refusal_reason=manifest unreadable",
    )];
    write_mc19_run_snapshot(
      &store,
      &root,
      run_id,
      events,
      vec![query_artifact, operation_result],
    );

    let summary = derive_minecraft_query_wired_live_action_summary(
      &store,
      &store.read_run(run_id).expect("run"),
    )
    .expect("summary should derive");
    assert!(summary.source_readiness_ref.is_none());
    assert!(
      summary
        .source_readiness_ref
        .as_deref()
        .is_none_or(|value| !value.contains("kind=derived_readiness"))
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_query_wired_live_action_summary_clean_miss_derived_readiness_ref() {
    let root = temp_dir("run-read-mc19-clean-miss");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = "run_read_mc19_clean_miss";
    let run = dummy_run(run_id);
    let span_id = run.root_span_id.clone();
    let missing_query_id = "artifact_missing_query_manifest";
    let operation_result = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      0,
      "operation-result",
      "operation-result.json",
      &mc19_operation_result(
        &run.run_id,
        missing_query_id,
        OperationStatus::Completed,
        "query manifest absent from run",
      ),
    );
    let events = vec![dummy_mc19_event(
      &span_id,
      "minecraft.query_wired_live_action.outcome",
      "attempted=false action_eligibility=not_consumable refusal_reason=query manifest absent from run",
    )];
    write_mc19_run_snapshot(&store, &root, run_id, events, vec![operation_result]);

    let summary = derive_minecraft_query_wired_live_action_summary(
      &store,
      &store.read_run(run_id).expect("run"),
    )
    .expect("summary should derive");
    assert_eq!(
      summary.source_readiness_ref.as_deref(),
      Some(
        format!("kind=derived_readiness query_artifact_id={missing_query_id} run_id={run_id}")
          .as_str()
      )
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_query_wired_live_action_summary_partial_manifest_source_readiness_ref_none() {
    let root = temp_dir("run-read-mc19-partial-manifest");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = "run_read_mc19_partial_manifest";
    let run = dummy_run(run_id);
    let span_id = run.root_span_id.clone();
    let query_artifact = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_QUERY_ROLE,
      "minecraft-3dgs-training-result-query.json",
      &json!({"status": "answered"}),
    );
    let operation_result = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      1,
      "operation-result",
      "operation-result.json",
      &mc19_operation_result(
        &run.run_id,
        query_artifact.artifact_id.as_str(),
        OperationStatus::Completed,
        "partial manifest",
      ),
    );
    let events = vec![dummy_mc19_event(
      &span_id,
      "minecraft.query_wired_live_action.outcome",
      "attempted=false action_eligibility=not_consumable refusal_reason=partial manifest",
    )];
    write_mc19_run_snapshot(
      &store,
      &root,
      run_id,
      events,
      vec![query_artifact, operation_result],
    );

    let summary = derive_minecraft_query_wired_live_action_summary(
      &store,
      &store.read_run(run_id).expect("run"),
    )
    .expect("summary should derive");
    assert!(summary.source_readiness_ref.is_none());

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_query_wired_live_action_summary_schema_status_only_source_readiness_ref_none() {
    let root = temp_dir("run-read-mc19-schema-status-only");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = "run_read_mc19_schema_status_only";
    let run = dummy_run(run_id);
    let span_id = run.root_span_id.clone();
    let query_artifact = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_QUERY_ROLE,
      "minecraft-3dgs-training-result-query.json",
      &json!({"schema_version": 1, "status": "answered"}),
    );
    let operation_result = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      1,
      "operation-result",
      "operation-result.json",
      &mc19_operation_result(
        &run.run_id,
        query_artifact.artifact_id.as_str(),
        OperationStatus::Completed,
        "schema status only manifest",
      ),
    );
    let events = vec![dummy_mc19_event(
      &span_id,
      "minecraft.query_wired_live_action.outcome",
      "attempted=false action_eligibility=not_consumable refusal_reason=schema status only manifest",
    )];
    write_mc19_run_snapshot(
      &store,
      &root,
      run_id,
      events,
      vec![query_artifact, operation_result],
    );

    let summary = derive_minecraft_query_wired_live_action_summary(
      &store,
      &store.read_run(run_id).expect("run"),
    )
    .expect("summary should derive");
    assert!(summary.source_readiness_ref.is_none());

    let _ = fs::remove_dir_all(root);
  }

  fn osu_query_manifest_json(status: &str, pixel_visibility: Option<&str>) -> serde_json::Value {
    let (pixel_x, pixel_y) = if pixel_visibility == Some("inside_capture") {
      (Some(400.0), Some(300.0))
    } else {
      (None, None)
    };
    json!({
      "schema_version": 1,
      "generated_at_millis": 1,
      "visual_truth_semantic_manifest_path": "/tmp/semantic.json",
      "source_run_artifact_dir": "/tmp/run",
      "source_visual_truth_manifest_path": "/tmp/vt.json",
      "source_projection_path": "/tmp/proj.json",
      "object_index": 0,
      "capture_phase": "before_dispatch",
      "object_kind": "circle",
      "query_backend": "playfield_projection_reference",
      "status": status,
      "pixel_visibility": pixel_visibility,
      "pixel_x": pixel_x,
      "pixel_y": pixel_y,
      "match_radius_px": 20.0,
      "capture_width": 800,
      "capture_height": 600,
      "known_limits": []
    })
  }

  fn osu_operation_result(
    run_id: &RunId,
    query_artifact_id: &str,
    status: OperationStatus,
    message: &str,
  ) -> OperationResult {
    let query_ref = ArtifactRef {
      artifact_id: ArtifactId::new(query_artifact_id),
      run_id: run_id.clone(),
      span_id: SpanId::new("0000000000000001"),
      captured_event_id: None,
    };
    OperationResult {
      api_version: OPERATION_RESULT_API_VERSION.to_string(),
      run_id: run_id.clone(),
      status,
      operation_id: crate::osu_query_live_action::QUERY_WIRED_LIVE_ACTION_OPERATION_ID.to_string(),
      evidence_artifacts: vec![query_ref.clone()],
      output: OperationOutput::Acknowledged {
        message: Some(message.to_string()),
      },
      verifications: Vec::new(),
      freshness_basis: Some(crate::contract::FreshnessBasis {
        source_artifact: Some(query_ref),
        source_operation_id: Some("auv.osu.query_visual_truth_spatial".to_string()),
        notes: vec!["osu visual truth spatial query manifest staged in the same run".to_string()],
      }),
      known_limits: vec![
        "osu_query_wired_live_action_capture_space_readiness_live_window_dispatch_no_gameplay_verification".to_string(),
      ],
    }
  }

  fn dummy_osu_event(
    span_id: &SpanId,
    name: &str,
    message: &str,
  ) -> auv_tracing_driver::trace::EventRecordV1Alpha1 {
    dummy_mc19_event(span_id, name, message)
  }

  #[test]
  fn osu_query_wired_live_action_summary_event_only_source_readiness_ref() {
    let root = temp_dir("run-read-osu-event-only");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = "run_read_osu_event_only";
    let run = dummy_run(run_id);
    let span_id = run.root_span_id.clone();
    let events = vec![dummy_osu_event(
      &span_id,
      "osu.query_wired_live_action.outcome",
      "attempted=false action_eligibility=answer_non_clickable refusal_reason=pixel_visibility=outside_capture",
    )];
    write_mc19_run_snapshot(&store, &root, run_id, events, vec![]);

    let summary =
      derive_osu_query_wired_live_action_summary(&store, &store.read_run(run_id).expect("run"))
        .expect("summary should derive");
    assert_eq!(
      summary.source_readiness_ref.as_deref(),
      Some("kind=outcome_event event=osu.query_wired_live_action.outcome")
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn osu_query_wired_live_action_summary_manifest_parse_failure_source_readiness_ref_none() {
    let root = temp_dir("run-read-osu-manifest-parse-failure");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = "run_read_osu_manifest_parse_failure";
    let run = dummy_run(run_id);
    let span_id = run.root_span_id.clone();
    let query_artifact = stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      0,
      crate::osu::OSU_VISUAL_TRUTH_SPATIAL_QUERY_ROLE,
      "osu-visual-truth-spatial-query.json",
      "{not valid json",
    );
    let operation_result = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      1,
      "operation-result",
      "operation-result.json",
      &osu_operation_result(
        &run.run_id,
        query_artifact.artifact_id.as_str(),
        OperationStatus::Completed,
        "manifest unreadable",
      ),
    );
    let events = vec![dummy_osu_event(
      &span_id,
      "osu.query_wired_live_action.outcome",
      "attempted=false action_eligibility=not_consumable refusal_reason=manifest unreadable",
    )];
    write_mc19_run_snapshot(
      &store,
      &root,
      run_id,
      events,
      vec![query_artifact, operation_result],
    );

    let summary =
      derive_osu_query_wired_live_action_summary(&store, &store.read_run(run_id).expect("run"))
        .expect("summary should derive");
    assert!(summary.source_readiness_ref.is_none());

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn osu_query_wired_live_action_summary_clean_miss_derived_readiness_ref() {
    let root = temp_dir("run-read-osu-clean-miss");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = "run_read_osu_clean_miss";
    let run = dummy_run(run_id);
    let span_id = run.root_span_id.clone();
    let missing_query_id = "artifact_missing_osu_query_manifest";
    let operation_result = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      0,
      "operation-result",
      "operation-result.json",
      &osu_operation_result(
        &run.run_id,
        missing_query_id,
        OperationStatus::Completed,
        "query manifest absent from run",
      ),
    );
    let events = vec![dummy_osu_event(
      &span_id,
      "osu.query_wired_live_action.outcome",
      "attempted=false action_eligibility=not_consumable refusal_reason=query manifest absent from run",
    )];
    write_mc19_run_snapshot(&store, &root, run_id, events, vec![operation_result]);

    let summary =
      derive_osu_query_wired_live_action_summary(&store, &store.read_run(run_id).expect("run"))
        .expect("summary should derive");
    assert_eq!(
      summary.source_readiness_ref.as_deref(),
      Some(
        format!("kind=derived_readiness query_artifact_id={missing_query_id} run_id={run_id}")
          .as_str()
      )
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn osu_query_wired_live_action_summary_query_manifest_source_readiness_ref() {
    let root = temp_dir("run-read-osu-query-manifest");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = "run_read_osu_query_manifest";
    let run = dummy_run(run_id);
    let span_id = run.root_span_id.clone();
    let query_artifact = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      0,
      crate::osu::OSU_VISUAL_TRUTH_SPATIAL_QUERY_ROLE,
      "osu-visual-truth-spatial-query.json",
      &osu_query_manifest_json("answered", Some("inside_capture")),
    );
    let query_artifact_id = query_artifact.artifact_id.as_str().to_string();
    let operation_result = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      1,
      "operation-result",
      "operation-result.json",
      &osu_operation_result(
        &run.run_id,
        query_artifact_id.as_str(),
        OperationStatus::Completed,
        "mock dispatch",
      ),
    );
    let events = vec![dummy_osu_event(
      &span_id,
      "osu.query_wired_live_action.outcome",
      "attempted=true action_eligibility=click_ready refusal_reason=none pixel_point=400,300",
    )];
    write_mc19_run_snapshot(
      &store,
      &root,
      run_id,
      events,
      vec![query_artifact, operation_result],
    );

    let summary =
      derive_osu_query_wired_live_action_summary(&store, &store.read_run(run_id).expect("run"))
        .expect("summary should derive");
    assert_eq!(
      summary.source_readiness_ref.as_deref(),
      Some(
        format!(
          "kind=query_manifest artifact_id={} run_id={}",
          query_artifact_id.as_str(),
          run_id
        )
        .as_str()
      )
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn quality_baseline_profile_v1_fixture_loads() {
    let profile = quality_baseline_profile_v1().expect("profile v1 fixture should parse");
    assert_eq!(profile.profile_id, "mc17-d2-primary-v1");
    assert_eq!(profile.query_target_block, "511,73,728");
    assert_eq!(profile.holdout_frame_index, 6);
  }

  #[test]
  fn quality_baseline_derive_complete_when_all_stages_match_profile() {
    let profile = quality_baseline_profile_v1().expect("profile");
    let semantic = profile.training_result_semantic_manifest_path.clone();
    let spatial = MinecraftTrainingResultSpatialQueryManifestSummary {
      schema_version: 1,
      training_result_semantic_manifest_path: semantic.clone(),
      source_training_result_artifact_manifest_path: "/tmp/artifact.json".to_string(),
      source_training_result_manifest_path: "/tmp/result.json".to_string(),
      source_training_job_manifest_path: "/tmp/job.json".to_string(),
      source_training_launch_plan_path: "/tmp/launch.json".to_string(),
      source_training_package_manifest_path: "/tmp/package.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene.json".to_string(),
      source_bundle_manifest_paths: vec![],
      source_run_ids: vec![],
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      normalized_result_dir: "/tmp/normalized".to_string(),
      query_kind: "block_projection".to_string(),
      target_block: profile.query_target_block.clone(),
      target_face: profile.query_target_face.clone(),
      target_semantics: profile.query_target_semantics.clone(),
      selected_backend: Some("projection_reference".to_string()),
      status: "answered".to_string(),
      reason: None,
      visibility: Some("visible".to_string()),
      screen_point: Some("854.0,480.0".to_string()),
      match_radius_px: Some(8.0),
      confidence: Some(0.9),
      basis_frame_id: Some("frame-355416".to_string()),
      comparison_verdict: Some("reference_only".to_string()),
      known_limits: vec![],
    };
    let holdout = MinecraftTrainingResultHoldoutPreviewManifestSummary {
      schema_version: 1,
      training_result_semantic_manifest_path: semantic.clone(),
      source_training_result_artifact_manifest_path: "/tmp/artifact.json".to_string(),
      source_training_result_manifest_path: "/tmp/result.json".to_string(),
      source_training_job_manifest_path: "/tmp/job.json".to_string(),
      source_training_launch_plan_path: "/tmp/launch.json".to_string(),
      source_training_package_manifest_path: "/tmp/package.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene.json".to_string(),
      source_bundle_manifest_paths: vec![],
      source_run_ids: vec![],
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      normalized_result_dir: "/tmp/normalized".to_string(),
      holdout_frame_index: profile.holdout_frame_index,
      holdout_frame: Some(MinecraftTrainingResultHoldoutPreviewFrameWitnessSummary {
        frame_index: 6,
        spatial_frame_id: "frame-355416".to_string(),
        screenshot_path: "/tmp/frame_000006.png".to_string(),
        frame_json_path: "/tmp/frame_000006.json".to_string(),
      }),
      basis_checkpoint_path: Some(format!(
        "/tmp/normalized/nerfstudio_models/{}",
        profile.basis_checkpoint_suffix
      )),
      holdout_screenshot_path: Some("/tmp/frame_000006.png".to_string()),
      reference_overlay_path: None,
      status: "ready".to_string(),
      reason: None,
      known_limits: vec![],
    };
    let render = MinecraftHoldoutRenderQualityManifestSummary {
      schema_version: 1,
      training_result_semantic_manifest_path: semantic,
      holdout_preview_manifest_path: "/tmp/holdout-preview.json".to_string(),
      source_training_result_artifact_manifest_path: "/tmp/artifact.json".to_string(),
      source_training_result_manifest_path: "/tmp/result.json".to_string(),
      source_training_job_manifest_path: "/tmp/job.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene.json".to_string(),
      source_run_ids: vec![],
      holdout_frame_index: profile.holdout_frame_index,
      basis_checkpoint_path: holdout.basis_checkpoint_path.clone(),
      rendered_image_path: Some("/tmp/rendered.png".to_string()),
      image_size_match: true,
      metrics: Some(MinecraftHoldoutRenderQualityMetricsSummary {
        l1_mean: Some(0.0),
        mse: Some(0.0),
        psnr: None,
      }),
      status: "ready".to_string(),
      reason: None,
      verdict: "measured_only".to_string(),
      known_limits: vec!["metrics evidence only".to_string()],
    };

    let report = derive_minecraft_training_result_quality_baseline_report(
      &profile,
      Some(&spatial),
      Some(&holdout),
      Some(&render),
      &[],
    );
    assert_eq!(report.evidence_coverage, "complete");
    assert!(report.issue.is_none());
    assert!(report.spatial_query.is_some());
    assert!(report.holdout_witness.is_some());
    assert!(report.render_quality.is_some());
    assert!(
      report
        .trust_notes
        .iter()
        .any(|note| note.contains("projection_reference"))
    );
  }

  #[test]
  fn quality_baseline_derive_partial_when_stage_missing() {
    let profile = quality_baseline_profile_v1().expect("profile");
    let report =
      derive_minecraft_training_result_quality_baseline_report(&profile, None, None, None, &[]);
    assert_eq!(report.evidence_coverage, "missing_stage");
  }

  #[test]
  fn quality_baseline_derive_partial_on_profile_mismatch() {
    let profile = quality_baseline_profile_v1().expect("profile");
    let spatial = MinecraftTrainingResultSpatialQueryManifestSummary {
      schema_version: 1,
      training_result_semantic_manifest_path: "/tmp/other-semantic.json".to_string(),
      source_training_result_artifact_manifest_path: "/tmp/artifact.json".to_string(),
      source_training_result_manifest_path: "/tmp/result.json".to_string(),
      source_training_job_manifest_path: "/tmp/job.json".to_string(),
      source_training_launch_plan_path: "/tmp/launch.json".to_string(),
      source_training_package_manifest_path: "/tmp/package.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene.json".to_string(),
      source_bundle_manifest_paths: vec![],
      source_run_ids: vec![],
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      normalized_result_dir: "/tmp/normalized".to_string(),
      query_kind: "block_projection".to_string(),
      target_block: "9,9,9".to_string(),
      target_face: None,
      target_semantics: "hit_face_center".to_string(),
      selected_backend: None,
      status: "failed".to_string(),
      reason: Some("target_block_absent_from_scene_packet".to_string()),
      visibility: None,
      screen_point: None,
      match_radius_px: None,
      confidence: None,
      basis_frame_id: None,
      comparison_verdict: None,
      known_limits: vec![],
    };
    let report = derive_minecraft_training_result_quality_baseline_report(
      &profile,
      Some(&spatial),
      None,
      None,
      &[],
    );
    assert_eq!(report.evidence_coverage, "partial");
    assert!(report.issue.is_some());
  }
  #[test]
  fn quality_baseline_derive_surfaces_collection_issues() {
    let profile = quality_baseline_profile_v1().expect("profile");
    let report = derive_minecraft_training_result_quality_baseline_report(
      &profile,
      None,
      None,
      None,
      &["read holdout preview manifest at /missing/path: no such file".to_string()],
    );
    assert_eq!(report.evidence_coverage, "missing_stage");
    assert!(
      report
        .issue
        .as_ref()
        .is_some_and(|issue| issue.contains("read holdout preview manifest"))
    );
  }

  fn sample_complete_quality_baseline_report(
    l1_mean: f64,
    mse: f64,
    render_verdict: &str,
    spatial_visibility: Option<&str>,
  ) -> MinecraftTrainingResultQualityBaselineReportSummary {
    let profile = quality_baseline_profile_v1().expect("profile");
    let semantic = profile.training_result_semantic_manifest_path.clone();
    let spatial = MinecraftTrainingResultSpatialQueryManifestSummary {
      schema_version: 1,
      training_result_semantic_manifest_path: semantic.clone(),
      source_training_result_artifact_manifest_path: "/tmp/artifact.json".to_string(),
      source_training_result_manifest_path: "/tmp/result.json".to_string(),
      source_training_job_manifest_path: "/tmp/job.json".to_string(),
      source_training_launch_plan_path: "/tmp/launch.json".to_string(),
      source_training_package_manifest_path: "/tmp/package.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene.json".to_string(),
      source_bundle_manifest_paths: vec![],
      source_run_ids: vec![],
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      normalized_result_dir: "/tmp/normalized".to_string(),
      query_kind: "block_projection".to_string(),
      target_block: profile.query_target_block.clone(),
      target_face: profile.query_target_face.clone(),
      target_semantics: profile.query_target_semantics.clone(),
      selected_backend: Some("projection_reference".to_string()),
      status: "answered".to_string(),
      reason: None,
      visibility: spatial_visibility.map(str::to_string),
      screen_point: Some("854.0,480.0".to_string()),
      match_radius_px: Some(8.0),
      confidence: Some(0.9),
      basis_frame_id: Some("frame-355416".to_string()),
      comparison_verdict: Some("reference_only".to_string()),
      known_limits: vec![],
    };
    let holdout = MinecraftTrainingResultHoldoutPreviewManifestSummary {
      schema_version: 1,
      training_result_semantic_manifest_path: semantic.clone(),
      source_training_result_artifact_manifest_path: "/tmp/artifact.json".to_string(),
      source_training_result_manifest_path: "/tmp/result.json".to_string(),
      source_training_job_manifest_path: "/tmp/job.json".to_string(),
      source_training_launch_plan_path: "/tmp/launch.json".to_string(),
      source_training_package_manifest_path: "/tmp/package.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene.json".to_string(),
      source_bundle_manifest_paths: vec![],
      source_run_ids: vec![],
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      normalized_result_dir: "/tmp/normalized".to_string(),
      holdout_frame_index: profile.holdout_frame_index,
      holdout_frame: Some(MinecraftTrainingResultHoldoutPreviewFrameWitnessSummary {
        frame_index: 6,
        spatial_frame_id: "frame-355416".to_string(),
        screenshot_path: "/tmp/frame_000006.png".to_string(),
        frame_json_path: "/tmp/frame_000006.json".to_string(),
      }),
      basis_checkpoint_path: Some(format!(
        "/tmp/normalized/nerfstudio_models/{}",
        profile.basis_checkpoint_suffix
      )),
      holdout_screenshot_path: Some("/tmp/frame_000006.png".to_string()),
      reference_overlay_path: None,
      status: "ready".to_string(),
      reason: None,
      known_limits: vec![],
    };
    let render = MinecraftHoldoutRenderQualityManifestSummary {
      schema_version: 1,
      training_result_semantic_manifest_path: semantic,
      holdout_preview_manifest_path: "/tmp/holdout-preview.json".to_string(),
      source_training_result_artifact_manifest_path: "/tmp/artifact.json".to_string(),
      source_training_result_manifest_path: "/tmp/result.json".to_string(),
      source_training_job_manifest_path: "/tmp/job.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene.json".to_string(),
      source_run_ids: vec![],
      holdout_frame_index: profile.holdout_frame_index,
      basis_checkpoint_path: holdout.basis_checkpoint_path.clone(),
      rendered_image_path: Some("/tmp/rendered.png".to_string()),
      image_size_match: true,
      metrics: Some(MinecraftHoldoutRenderQualityMetricsSummary {
        l1_mean: Some(l1_mean),
        mse: Some(mse),
        psnr: None,
      }),
      status: "ready".to_string(),
      reason: None,
      verdict: render_verdict.to_string(),
      known_limits: vec![],
    };
    derive_minecraft_training_result_quality_baseline_report(
      &profile,
      Some(&spatial),
      Some(&holdout),
      Some(&render),
      &[],
    )
  }

  #[test]
  fn quality_baseline_verdict_probe_passes_on_complete_zero_metric_baseline() {
    let baseline =
      sample_complete_quality_baseline_report(0.0, 0.0, "measured_only", Some("visible"));
    let thresholds = quality_baseline_verdict_thresholds_probe_v1().expect("probe thresholds");
    let verdict = derive_minecraft_training_result_quality_verdict(&baseline, &thresholds);
    assert_eq!(verdict.quality_verdict, "pass");
    assert_eq!(verdict.render_evidence_mode, "screenshot_copy_probe");
  }

  #[test]
  fn quality_baseline_verdict_fails_when_l1_exceeds_probe_max() {
    let baseline =
      sample_complete_quality_baseline_report(0.42, 0.0, "measured_only", Some("visible"));
    let thresholds = quality_baseline_verdict_thresholds_probe_v1().expect("probe thresholds");
    let verdict = derive_minecraft_training_result_quality_verdict(&baseline, &thresholds);
    assert_eq!(verdict.quality_verdict, "fail");
    assert!(
      verdict
        .stage_checks
        .iter()
        .any(|check| check.stage == "render_quality" && check.outcome == "fail")
    );
  }

  #[test]
  fn quality_baseline_verdict_blocks_on_partial_evidence_coverage() {
    let profile = quality_baseline_profile_v1().expect("profile");
    let baseline =
      derive_minecraft_training_result_quality_baseline_report(&profile, None, None, None, &[]);
    let thresholds = quality_baseline_verdict_thresholds_probe_v1().expect("probe thresholds");
    let verdict = derive_minecraft_training_result_quality_verdict(&baseline, &thresholds);
    assert_eq!(verdict.quality_verdict, "blocked");
  }

  #[test]
  fn quality_baseline_verdict_partial_on_metric_partial_render() {
    let baseline =
      sample_complete_quality_baseline_report(0.0, 0.0, "metric_partial", Some("visible"));
    let thresholds = quality_baseline_verdict_thresholds_probe_v1().expect("probe thresholds");
    let verdict = derive_minecraft_training_result_quality_verdict(&baseline, &thresholds);
    assert_eq!(verdict.quality_verdict, "partial");
  }

  #[test]
  fn quality_baseline_verdict_partial_on_spatial_outside_window() {
    let baseline =
      sample_complete_quality_baseline_report(0.0, 0.0, "measured_only", Some("outside_window"));
    let thresholds = quality_baseline_verdict_thresholds_probe_v1().expect("probe thresholds");
    let verdict = derive_minecraft_training_result_quality_verdict(&baseline, &thresholds);
    assert_eq!(verdict.quality_verdict, "partial");
    assert!(
      verdict
        .stage_checks
        .iter()
        .any(|check| check.stage == "spatial_query" && check.outcome == "fail")
    );
  }

  #[test]
  fn quality_baseline_verdict_threshold_fixtures_load() {
    let probe = quality_baseline_verdict_thresholds_probe_v1().expect("probe");
    let trained = quality_baseline_verdict_thresholds_trained_render_v1().expect("trained");
    assert_eq!(probe.render_evidence_mode, "screenshot_copy_probe");
    assert_eq!(trained.render_evidence_mode, "trained_render");
    assert_eq!(probe.profile_id, "mc17-d2-primary-v1");
  }
}
