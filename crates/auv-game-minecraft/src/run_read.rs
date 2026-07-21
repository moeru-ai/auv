//! Minecraft ordinary run_read helpers for inspect composition.
//!
//! Depends on `auv-inspect-model` only (no `auv-cli`). Query-wired adapters stay in product (S3b).

use auv_inspect_model::legacy::{ArtifactRefView, artifact_record_view, is_json_mime, read_artifact_json, read_telemetry_artifact_summary};
use auv_tracing_driver::store::{CanonicalRun, LocalStore};

use crate::artifact::MinecraftProjectionArtifact;
use crate::dataset::{SourceRunSummary, SpatialBundleCounts};
use crate::{
  TrainingCompatibilityViewReport, TrainingLaunchInspectReport, TrainingLaunchJobInspectReport, TrainingLaunchJobManifest,
  TrainingLaunchPlanManifest, TrainingPackageCounts, TrainingPackageInspectReport, TrainingPackageManifest,
  TrainingResultArtifactFetchInspectReport, TrainingResultArtifactFetchManifest, TrainingResultHoldoutPreviewInspectReport,
  TrainingResultHoldoutPreviewManifest, TrainingResultHoldoutRenderQualityInspectReport, TrainingResultHoldoutRenderQualityManifest,
  TrainingResultInspectReport, TrainingResultManifest, TrainingResultSemanticCheckpointRecord, TrainingResultSemanticInspectReport,
  TrainingResultSemanticManifest, TrainingResultSpatialQueryInspectReport, TrainingResultSpatialQueryManifest, derive_action_readiness,
};
use std::fs;

pub(crate) struct MinecraftTelemetrySampleArtifactLineage {
  pub artifact: ArtifactRefView,
  pub line_count: Option<usize>,
  pub byte_size: Option<u64>,
  pub issue: Option<String>,
}

pub struct MinecraftSpatialBundleManifestLineage {
  pub artifact: ArtifactRefView,
  pub manifest: Option<MinecraftSpatialBundleManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct MinecraftTrainingPackageManifestLineage {
  pub artifact: ArtifactRefView,
  pub manifest: Option<MinecraftTrainingPackageManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct MinecraftTrainingLaunchManifestLineage {
  pub artifact: ArtifactRefView,
  pub manifest: Option<MinecraftTrainingLaunchManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct MinecraftTrainingLaunchInspectReportLineage {
  pub artifact: ArtifactRefView,
  pub report: Option<MinecraftTrainingLaunchInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct MinecraftTrainingJobManifestLineage {
  pub artifact: ArtifactRefView,
  pub manifest: Option<MinecraftTrainingJobManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct MinecraftTrainingJobInspectReportLineage {
  pub artifact: ArtifactRefView,
  pub report: Option<MinecraftTrainingJobInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct MinecraftTrainingResultManifestLineage {
  pub artifact: ArtifactRefView,
  pub manifest: Option<MinecraftTrainingResultManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct MinecraftTrainingResultInspectReportLineage {
  pub artifact: ArtifactRefView,
  pub report: Option<MinecraftTrainingResultInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct MinecraftTrainingResultArtifactFetchManifestLineage {
  pub artifact: ArtifactRefView,
  pub manifest: Option<MinecraftTrainingResultArtifactFetchManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct MinecraftTrainingResultArtifactFetchInspectReportLineage {
  pub artifact: ArtifactRefView,
  pub report: Option<MinecraftTrainingResultArtifactFetchInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct MinecraftTrainingResultSemanticManifestLineage {
  pub artifact: ArtifactRefView,
  pub manifest: Option<MinecraftTrainingResultSemanticManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct MinecraftTrainingResultSemanticInspectReportLineage {
  pub artifact: ArtifactRefView,
  pub report: Option<MinecraftTrainingResultSemanticInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct MinecraftTrainingResultSpatialQueryManifestLineage {
  pub artifact: ArtifactRefView,
  pub manifest: Option<MinecraftTrainingResultSpatialQueryManifestSummary>,
  pub issue: Option<String>,
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
pub(crate) struct MinecraftTrainingResultSpatialQueryInspectReportLineage {
  pub artifact: ArtifactRefView,
  pub report: Option<MinecraftTrainingResultSpatialQueryInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct MinecraftTrainingResultHoldoutPreviewFrameWitnessSummary {
  pub frame_index: usize,
  pub spatial_frame_id: String,
  pub screenshot_path: String,
  pub frame_json_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct MinecraftTrainingResultHoldoutPreviewManifestSummary {
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
pub(crate) struct MinecraftTrainingResultHoldoutPreviewInspectReportSummary {
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
pub(crate) struct MinecraftTrainingResultHoldoutPreviewManifestLineage {
  pub artifact: ArtifactRefView,
  pub manifest: Option<MinecraftTrainingResultHoldoutPreviewManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct MinecraftTrainingResultHoldoutPreviewInspectReportLineage {
  pub artifact: ArtifactRefView,
  pub report: Option<MinecraftTrainingResultHoldoutPreviewInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct MinecraftHoldoutRenderQualityMetricsSummary {
  pub l1_mean: Option<f64>,
  pub mse: Option<f64>,
  pub psnr: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct MinecraftHoldoutRenderQualityManifestSummary {
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
pub(crate) struct MinecraftHoldoutRenderQualityInspectReportSummary {
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
pub(crate) struct MinecraftHoldoutRenderQualityManifestLineage {
  pub artifact: ArtifactRefView,
  pub manifest: Option<MinecraftHoldoutRenderQualityManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct MinecraftHoldoutRenderQualityInspectReportLineage {
  pub artifact: ArtifactRefView,
  pub report: Option<MinecraftHoldoutRenderQualityInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct MinecraftTrainingPackageInspectReportLineage {
  pub artifact: ArtifactRefView,
  pub report: Option<MinecraftTrainingPackageInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct MinecraftTrainingLaunchManifestSummary {
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
pub(crate) struct MinecraftTrainingLaunchInspectReportSummary {
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
pub(crate) struct MinecraftTrainingJobManifestSummary {
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
pub(crate) struct MinecraftTrainingJobInspectReportSummary {
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
pub(crate) struct MinecraftTrainingResultManifestSummary {
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
pub(crate) struct MinecraftTrainingResultArtifactSummary {
  pub relative_path: String,
  pub absolute_path: String,
  pub readable: bool,
  pub byte_size: Option<u64>,
}

impl From<crate::TrainingResultArtifactRecord> for MinecraftTrainingResultArtifactSummary {
  fn from(value: crate::TrainingResultArtifactRecord) -> Self {
    Self {
      relative_path: value.relative_path,
      absolute_path: value.absolute_path,
      readable: value.readable,
      byte_size: value.byte_size,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct MinecraftTrainingResultInspectReportSummary {
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
pub(crate) struct MinecraftTrainingResultArtifactFetchManifestSummary {
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
pub(crate) struct MinecraftTrainingResultArtifactFetchInspectReportSummary {
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
pub(crate) struct MinecraftTrainingResultNormalizedArtifactSummary {
  pub kind: String,
  pub relative_path: String,
  pub absolute_path: String,
  pub readable: bool,
  pub byte_size: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct MinecraftTrainingResultSemanticCheckpointSummary {
  pub relative_path: String,
  pub byte_size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct MinecraftTrainingResultSemanticManifestSummary {
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
pub(crate) struct MinecraftTrainingResultSemanticInspectReportSummary {
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
pub(crate) struct MinecraftTrainingResultSpatialQueryInspectReportSummary {
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
pub(crate) struct MinecraftTrainingPackageManifestSummary {
  pub schema_version: u32,
  pub source_scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub counts: TrainingPackageCounts,
  pub compatibility_views: Vec<TrainingCompatibilityViewReport>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct MinecraftTrainingPackageInspectReportSummary {
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

pub(crate) fn extract_minecraft_projection_artifacts(
  store: &LocalStore,
  run: &CanonicalRun,
) -> Result<Vec<MinecraftProjectionArtifact>, String> {
  let mut artifacts = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::MINECRAFT_PROJECTION_ARTIFACT_ROLE || !is_json_mime(&artifact.mime_type) {
      continue;
    }

    let parsed =
      read_artifact_json::<MinecraftProjectionArtifact>(store, run.run.run_id.as_str(), artifact, crate::MINECRAFT_PROJECTION_ARTIFACT_ROLE);

    if let Ok(projection) = parsed {
      artifacts.push(projection);
    }
  }
  Ok(artifacts)
}

pub fn list_minecraft_spatial_bundle_manifests(
  store: &LocalStore,
  run_id: &str,
) -> Result<Vec<MinecraftSpatialBundleManifestLineage>, String> {
  let run = store.read_run(run_id)?;
  extract_minecraft_spatial_bundle_manifests(store, &run)
}

pub(crate) fn list_minecraft_holdout_render_quality_manifests(
  store: &LocalStore,
  run_id: &str,
) -> Result<Vec<MinecraftHoldoutRenderQualityManifestLineage>, String> {
  let run = store.read_run(run_id)?;
  extract_minecraft_holdout_render_quality_manifests(store, &run)
}

pub(crate) fn extract_minecraft_holdout_render_quality_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> Result<Vec<MinecraftHoldoutRenderQualityManifestLineage>, String> {
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::MINECRAFT_3DGS_HOLDOUT_RENDER_QUALITY_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(MinecraftHoldoutRenderQualityManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!("minecraft holdout render quality manifest mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingResultHoldoutRenderQualityManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::MINECRAFT_3DGS_HOLDOUT_RENDER_QUALITY_ROLE,
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
) -> Result<Vec<MinecraftHoldoutRenderQualityInspectReportLineage>, String> {
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::MINECRAFT_3DGS_HOLDOUT_RENDER_QUALITY_INSPECT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(MinecraftHoldoutRenderQualityInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!("minecraft holdout render quality inspect mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingResultHoldoutRenderQualityInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::MINECRAFT_3DGS_HOLDOUT_RENDER_QUALITY_INSPECT_ROLE,
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
) -> Result<Vec<MinecraftTrainingResultSpatialQueryManifestLineage>, String> {
  let run = store.read_run(run_id)?;
  extract_minecraft_training_result_spatial_query_manifests(store, &run)
}

pub(crate) fn extract_minecraft_spatial_bundle_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> Result<Vec<MinecraftSpatialBundleManifestLineage>, String> {
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::MINECRAFT_SPATIAL_BUNDLE_ARTIFACT_ROLE {
      continue;
    }

    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(MinecraftSpatialBundleManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!("minecraft spatial bundle artifact mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }

    let parsed = read_artifact_json::<MinecraftSpatialBundleManifestSummary>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::MINECRAFT_SPATIAL_BUNDLE_ARTIFACT_ROLE,
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
) -> Result<Vec<MinecraftTrainingLaunchManifestLineage>, String> {
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::MINECRAFT_3DGS_TRAINING_LAUNCH_PLAN_ARTIFACT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(MinecraftTrainingLaunchManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!("minecraft training launch artifact mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingLaunchPlanManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::MINECRAFT_3DGS_TRAINING_LAUNCH_PLAN_ARTIFACT_ROLE,
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
) -> Result<Vec<MinecraftTrainingLaunchInspectReportLineage>, String> {
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::MINECRAFT_3DGS_TRAINING_LAUNCH_INSPECT_ARTIFACT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(MinecraftTrainingLaunchInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!("minecraft training launch inspect artifact mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingLaunchInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::MINECRAFT_3DGS_TRAINING_LAUNCH_INSPECT_ARTIFACT_ROLE,
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
) -> Result<Vec<MinecraftTrainingJobManifestLineage>, String> {
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::MINECRAFT_3DGS_TRAINING_JOB_ARTIFACT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(MinecraftTrainingJobManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!("minecraft training job artifact mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingLaunchJobManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::MINECRAFT_3DGS_TRAINING_JOB_ARTIFACT_ROLE,
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
) -> Result<Vec<MinecraftTrainingJobInspectReportLineage>, String> {
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::MINECRAFT_3DGS_TRAINING_JOB_INSPECT_ARTIFACT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(MinecraftTrainingJobInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!("minecraft training job inspect artifact mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingLaunchJobInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::MINECRAFT_3DGS_TRAINING_JOB_INSPECT_ARTIFACT_ROLE,
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
) -> Result<Vec<MinecraftTrainingResultManifestLineage>, String> {
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(MinecraftTrainingResultManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!("minecraft training result artifact mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingResultManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_ROLE,
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
) -> Result<Vec<MinecraftTrainingResultInspectReportLineage>, String> {
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::MINECRAFT_3DGS_TRAINING_RESULT_INSPECT_ARTIFACT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(MinecraftTrainingResultInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!("minecraft training result inspect artifact mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingResultInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::MINECRAFT_3DGS_TRAINING_RESULT_INSPECT_ARTIFACT_ROLE,
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
) -> Result<Vec<MinecraftTrainingResultArtifactFetchManifestLineage>, String> {
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_MANIFEST_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(MinecraftTrainingResultArtifactFetchManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!("minecraft training result artifact fetch manifest mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingResultArtifactFetchManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_MANIFEST_ROLE,
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
) -> Result<Vec<MinecraftTrainingResultArtifactFetchInspectReportLineage>, String> {
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_INSPECT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(MinecraftTrainingResultArtifactFetchInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!("minecraft training result artifact fetch inspect mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingResultArtifactFetchInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_INSPECT_ROLE,
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
) -> Result<Vec<MinecraftTrainingResultSemanticManifestLineage>, String> {
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::MINECRAFT_3DGS_TRAINING_RESULT_SEMANTIC_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(MinecraftTrainingResultSemanticManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!("minecraft training result semantic manifest mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingResultSemanticManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::MINECRAFT_3DGS_TRAINING_RESULT_SEMANTIC_ROLE,
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
) -> Result<Vec<MinecraftTrainingResultSemanticInspectReportLineage>, String> {
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::MINECRAFT_3DGS_TRAINING_RESULT_SEMANTIC_INSPECT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(MinecraftTrainingResultSemanticInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!("minecraft training result semantic inspect mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingResultSemanticInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::MINECRAFT_3DGS_TRAINING_RESULT_SEMANTIC_INSPECT_ROLE,
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

pub fn extract_minecraft_training_result_spatial_query_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> Result<Vec<MinecraftTrainingResultSpatialQueryManifestLineage>, String> {
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::MINECRAFT_3DGS_TRAINING_RESULT_QUERY_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(MinecraftTrainingResultSpatialQueryManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!("minecraft training result spatial query manifest mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingResultSpatialQueryManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::MINECRAFT_3DGS_TRAINING_RESULT_QUERY_ROLE,
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
) -> Result<Vec<MinecraftTrainingResultSpatialQueryInspectReportLineage>, String> {
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::MINECRAFT_3DGS_TRAINING_RESULT_QUERY_INSPECT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(MinecraftTrainingResultSpatialQueryInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!("minecraft training result spatial query inspect mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingResultSpatialQueryInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::MINECRAFT_3DGS_TRAINING_RESULT_QUERY_INSPECT_ROLE,
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
) -> Result<Vec<MinecraftTrainingResultHoldoutPreviewManifestLineage>, String> {
  let run = store.read_run(run_id)?;
  extract_minecraft_training_result_holdout_preview_manifests(store, &run)
}

pub(crate) fn extract_minecraft_training_result_holdout_preview_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> Result<Vec<MinecraftTrainingResultHoldoutPreviewManifestLineage>, String> {
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::MINECRAFT_3DGS_TRAINING_RESULT_HOLDOUT_PREVIEW_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(MinecraftTrainingResultHoldoutPreviewManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!("minecraft training result holdout preview manifest mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingResultHoldoutPreviewManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::MINECRAFT_3DGS_TRAINING_RESULT_HOLDOUT_PREVIEW_ROLE,
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
) -> Result<Vec<MinecraftTrainingResultHoldoutPreviewInspectReportLineage>, String> {
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::MINECRAFT_3DGS_TRAINING_RESULT_HOLDOUT_PREVIEW_INSPECT_ROLE {
      continue;
    }
    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(MinecraftTrainingResultHoldoutPreviewInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!("minecraft training result holdout preview inspect mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }
    let parsed = read_artifact_json::<TrainingResultHoldoutPreviewInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::MINECRAFT_3DGS_TRAINING_RESULT_HOLDOUT_PREVIEW_INSPECT_ROLE,
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
    readiness_class: auv_query_readiness::map_action_eligibility_to_readiness_class(&action_eligibility),
    action_eligibility,
    window_point: readiness.window_point.map(|point| {
      let point = auv_driver::geometry::Point::from(point);
      format!("{},{}", point.x, point.y)
    }),
    refusal_reason: readiness.refusal_reason,
    issue: None,
  }
}

pub(crate) const QUALITY_BASELINE_PROFILE_V1_JSON: &str = include_str!("../tests/fixtures/mc17-d2/baseline-profile-v1.json");

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct QualityBaselineProfile {
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
pub(crate) struct QualityBaselineRecordedRunIds {
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
pub(crate) struct QualityBaselineEvidenceBundle {
  pub spatial_query: Option<MinecraftTrainingResultSpatialQueryManifestSummary>,
  pub holdout_preview: Option<MinecraftTrainingResultHoldoutPreviewManifestSummary>,
  pub render_quality: Option<MinecraftHoldoutRenderQualityManifestSummary>,
  pub collection_issues: Vec<String>,
}

pub(crate) fn quality_baseline_report_for_run(
  store: &LocalStore,
  run_id: &str,
) -> Result<MinecraftTrainingResultQualityBaselineReportSummary, String> {
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

pub(crate) fn quality_baseline_profile_v1() -> Result<QualityBaselineProfile, String> {
  serde_json::from_str(QUALITY_BASELINE_PROFILE_V1_JSON).map_err(|error| format!("parse quality baseline profile v1 fixture: {error}"))
}

fn spatial_query_matches_profile(manifest: &MinecraftTrainingResultSpatialQueryManifestSummary, profile: &QualityBaselineProfile) -> bool {
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
    && manifest.basis_checkpoint_path.as_deref().is_some_and(|path| path.ends_with(&profile.basis_checkpoint_suffix))
}

fn holdout_render_quality_matches_profile(
  manifest: &MinecraftHoldoutRenderQualityManifestSummary,
  profile: &QualityBaselineProfile,
) -> bool {
  manifest.training_result_semantic_manifest_path == profile.training_result_semantic_manifest_path
    && manifest.holdout_frame_index == profile.holdout_frame_index
    && manifest.basis_checkpoint_path.as_deref().is_some_and(|path| path.ends_with(&profile.basis_checkpoint_suffix))
}

fn spatial_query_evidence_from_summary(summary: &MinecraftTrainingResultSpatialQueryManifestSummary) -> QualityBaselineSpatialQueryEvidence {
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
    spatial_frame_id: summary.holdout_frame.as_ref().map(|frame| frame.spatial_frame_id.clone()),
  }
}

fn render_quality_evidence_from_summary(summary: &MinecraftHoldoutRenderQualityManifestSummary) -> QualityBaselineRenderQualityEvidence {
  let (l1_mean, mse, psnr) =
    summary.metrics.as_ref().map(|metrics| (metrics.l1_mean, metrics.mse, metrics.psnr)).unwrap_or((None, None, None));
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

fn build_quality_baseline_trust_notes(render_quality: Option<&QualityBaselineRenderQualityEvidence>) -> Vec<String> {
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

pub(crate) fn derive_minecraft_training_result_quality_baseline_report(
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
      issues.push("holdout render quality manifest does not match baseline profile pins".to_string());
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

fn read_holdout_preview_summary_from_path(path: &str) -> Result<MinecraftTrainingResultHoldoutPreviewManifestSummary, String> {
  let bytes = fs::read_to_string(path).map_err(|error| format!("read holdout preview manifest at {path}: {error}"))?;
  let manifest: TrainingResultHoldoutPreviewManifest =
    serde_json::from_str(&bytes).map_err(|error| format!("parse holdout preview manifest at {path}: {error}"))?;
  Ok(MinecraftTrainingResultHoldoutPreviewManifestSummary::from(manifest))
}

fn select_matching_spatial_query_manifest(
  manifests: &[MinecraftTrainingResultSpatialQueryManifestLineage],
  profile: &QualityBaselineProfile,
) -> Option<MinecraftTrainingResultSpatialQueryManifestSummary> {
  manifests.iter().filter_map(|lineage| lineage.manifest.as_ref()).find(|manifest| spatial_query_matches_profile(manifest, profile)).cloned()
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
        .find(|manifest| manifest.training_result_semantic_manifest_path == profile.training_result_semantic_manifest_path)
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
  if let Some(run_id) = profile.recorded_run_ids.as_ref().and_then(|ids| ids.mc12.as_deref()) {
    if let Some(manifest) = find_spatial_query_manifest_in_run(store, run_id, profile) {
      return Some(manifest);
    }
  }
  let runs = store.list_runs().ok()?;
  for run in runs.iter().rev() {
    if let Some(manifest) = find_spatial_query_manifest_in_run(store, run.run_id.as_str(), profile) {
      return Some(manifest);
    }
  }
  None
}

fn find_holdout_preview_manifest_in_store(
  store: &LocalStore,
  profile: &QualityBaselineProfile,
) -> Option<MinecraftTrainingResultHoldoutPreviewManifestSummary> {
  if let Some(run_id) = profile.recorded_run_ids.as_ref().and_then(|ids| ids.mc16.as_deref()) {
    if let Some(manifest) = find_holdout_preview_manifest_in_run(store, run_id, profile) {
      return Some(manifest);
    }
  }
  let runs = store.list_runs().ok()?;
  for run in runs.iter().rev() {
    if let Some(manifest) = find_holdout_preview_manifest_in_run(store, run.run_id.as_str(), profile) {
      return Some(manifest);
    }
  }
  None
}

pub(crate) fn collect_quality_baseline_evidence_for_run(
  store: &LocalStore,
  run_id: &str,
  profile: &QualityBaselineProfile,
) -> Result<QualityBaselineEvidenceBundle, String> {
  let spatial_query_manifests = list_minecraft_training_result_spatial_query_manifests(store, run_id)?;
  let holdout_preview_manifests = list_minecraft_training_result_holdout_preview_manifests(store, run_id)?;
  let render_quality_manifests = list_minecraft_holdout_render_quality_manifests(store, run_id)?;

  let render_quality = select_matching_holdout_render_quality_manifest(&render_quality_manifests, profile);

  let mut collection_issues = Vec::new();

  let mut holdout_preview = select_matching_holdout_preview_manifest(&holdout_preview_manifests, profile);
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

pub(crate) const QUALITY_BASELINE_VERDICT_THRESHOLDS_PROBE_V1_JSON: &str =
  include_str!("../tests/fixtures/mc17-d3/baseline-verdict-thresholds-v1-probe.json");
pub(crate) const QUALITY_BASELINE_VERDICT_THRESHOLDS_TRAINED_RENDER_V1_JSON: &str =
  include_str!("../tests/fixtures/mc17-d3/baseline-verdict-thresholds-v1-trained-render.json");

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct QualityBaselineSpatialQueryThresholds {
  pub required_status: String,
  #[serde(default)]
  pub required_visibility: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct QualityBaselineHoldoutWitnessThresholds {
  pub required_status: String,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct QualityBaselineRenderQualityThresholds {
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
pub(crate) struct QualityBaselineVerdictThresholdSet {
  pub spatial_query: QualityBaselineSpatialQueryThresholds,
  pub holdout_witness: QualityBaselineHoldoutWitnessThresholds,
  pub render_quality: QualityBaselineRenderQualityThresholds,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct QualityBaselineVerdictThresholds {
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

pub(crate) fn quality_baseline_verdict_thresholds_probe_v1() -> Result<QualityBaselineVerdictThresholds, String> {
  serde_json::from_str(QUALITY_BASELINE_VERDICT_THRESHOLDS_PROBE_V1_JSON)
    .map_err(|error| format!("parse quality baseline verdict thresholds probe v1 fixture: {error}"))
}

pub(crate) fn quality_baseline_verdict_thresholds_trained_render_v1() -> Result<QualityBaselineVerdictThresholds, String> {
  serde_json::from_str(QUALITY_BASELINE_VERDICT_THRESHOLDS_TRAINED_RENDER_V1_JSON)
    .map_err(|error| format!("parse quality baseline verdict thresholds trained_render v1 fixture: {error}"))
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
    reasons.push(format!("status={} expected required_status={}", evidence.status, thresholds.required_status));
  }
  if let Some(required_visibility) = thresholds.required_visibility.as_deref() {
    match evidence.visibility.as_deref() {
      Some(visibility) if visibility == required_visibility => {}
      Some(visibility) => reasons.push(format!("visibility={visibility} expected required_visibility={required_visibility}")),
      None => reasons.push(format!("visibility missing expected required_visibility={required_visibility}")),
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
    reasons.push(format!("status={} expected required_status={}", evidence.status, thresholds.required_status));
  }
  if evidence.verdict != thresholds.required_verdict {
    reasons.push(format!("verdict={} expected required_verdict={}", evidence.verdict, thresholds.required_verdict));
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
  let has_metric_threshold_miss = reasons
    .iter()
    .any(|reason| reason.contains("exceeds l1_mean_max=") || reason.contains("exceeds mse_max=") || reason.contains("below psnr_min="));
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
      && check
        .reasons
        .iter()
        .any(|reason| reason.contains("exceeds l1_mean_max=") || reason.contains("exceeds mse_max=") || reason.contains("below psnr_min="))
  }) {
    return "fail".to_string();
  }
  if stage_checks.iter().any(|check| check.outcome == "partial" || check.outcome == "fail") {
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

pub(crate) fn derive_minecraft_training_result_quality_verdict(
  baseline: &MinecraftTrainingResultQualityBaselineReportSummary,
  thresholds: &QualityBaselineVerdictThresholds,
) -> MinecraftTrainingResultQualityVerdictSummary {
  let spatial_check = check_spatial_query_stage(baseline.spatial_query.as_ref(), &thresholds.thresholds.spatial_query);
  let holdout_check = check_holdout_witness_stage(baseline.holdout_witness.as_ref(), &thresholds.thresholds.holdout_witness);
  let render_check = check_render_quality_stage(baseline.render_quality.as_ref(), &thresholds.thresholds.render_quality);
  let stage_checks = vec![spatial_check, holdout_check, render_check];
  let quality_verdict = aggregate_quality_verdict(baseline, &stage_checks);
  let trust_notes = build_quality_verdict_trust_notes(baseline, thresholds);
  let issue = if baseline.evidence_coverage != "complete" {
    baseline.issue.clone().or_else(|| Some(format!("evidence_coverage={} blocks threshold verdict evaluation", baseline.evidence_coverage)))
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

pub fn quality_baseline_report_with_verdicts_for_run(
  store: &LocalStore,
  run_id: &str,
) -> Result<MinecraftQualityBaselineReportWithVerdicts, String> {
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
  let target_face = summary.target_face.as_deref().map(parse_spatial_query_target_face).transpose()?;
  let target_semantics = parse_spatial_query_target_semantics(&summary.target_semantics)?;
  let query_kind = parse_spatial_query_kind(&summary.query_kind)?;
  let status = parse_spatial_query_status(&summary.status)?;
  let reason = summary.reason.as_deref().map(parse_spatial_query_reason).transpose()?;
  let visibility = summary.visibility.as_deref().map(parse_spatial_query_visibility).transpose()?;
  let screen_point = summary.screen_point.as_deref().map(parse_spatial_query_screen_point).transpose()?;
  let selected_backend = summary.selected_backend.as_deref().map(parse_spatial_query_backend).transpose()?;
  let comparison_verdict = summary.comparison_verdict.as_deref().map(parse_spatial_query_comparison_verdict).transpose()?;

  Ok(TrainingResultSpatialQueryManifest {
    schema_version: summary.schema_version,
    generated_at_millis: 0,
    training_result_semantic_manifest_path: summary.training_result_semantic_manifest_path.clone(),
    source_training_result_artifact_manifest_path: summary.source_training_result_artifact_manifest_path.clone(),
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

fn parse_spatial_query_target_block(label: &str) -> Result<crate::BlockPosition, String> {
  let parts: Vec<&str> = label.split(',').collect();
  if parts.len() != 3 {
    return Err(format!("invalid spatial query target_block label `{label}`"));
  }
  let x = parts[0].parse::<i32>().map_err(|error| format!("invalid spatial query target_block x `{label}`: {error}"))?;
  let y = parts[1].parse::<i32>().map_err(|error| format!("invalid spatial query target_block y `{label}`: {error}"))?;
  let z = parts[2].parse::<i32>().map_err(|error| format!("invalid spatial query target_block z `{label}`: {error}"))?;
  Ok(crate::BlockPosition::new(x, y, z))
}

fn parse_spatial_query_target_face(label: &str) -> Result<crate::BlockFace, String> {
  match label {
    "up" => Ok(crate::BlockFace::Up),
    "down" => Ok(crate::BlockFace::Down),
    "north" => Ok(crate::BlockFace::North),
    "south" => Ok(crate::BlockFace::South),
    "east" => Ok(crate::BlockFace::East),
    "west" => Ok(crate::BlockFace::West),
    other => Err(format!("invalid spatial query target_face label `{other}`")),
  }
}

fn parse_spatial_query_target_semantics(label: &str) -> Result<crate::MinecraftTargetSemantics, String> {
  match label {
    "hit_face_center" => Ok(crate::MinecraftTargetSemantics::HitFaceCenter),
    "block_center" => Ok(crate::MinecraftTargetSemantics::BlockCenter),
    other => Err(format!("invalid spatial query target_semantics label `{other}`")),
  }
}

fn parse_spatial_query_kind(label: &str) -> Result<crate::TrainingResultSpatialQueryKind, String> {
  match label {
    "block_projection" => Ok(crate::TrainingResultSpatialQueryKind::BlockProjection),
    other => Err(format!("invalid spatial query query_kind label `{other}`")),
  }
}

fn parse_spatial_query_status(label: &str) -> Result<crate::TrainingResultSpatialQueryStatus, String> {
  match label {
    "answered" => Ok(crate::TrainingResultSpatialQueryStatus::Answered),
    "blocked" => Ok(crate::TrainingResultSpatialQueryStatus::Blocked),
    "failed" => Ok(crate::TrainingResultSpatialQueryStatus::Failed),
    other => Err(format!("invalid spatial query status label `{other}`")),
  }
}

fn parse_spatial_query_reason(label: &str) -> Result<crate::TrainingResultSpatialQueryReason, String> {
  match label {
    "semantic_source_not_ready" => Ok(crate::TrainingResultSpatialQueryReason::SemanticSourceNotReady),
    "target_block_absent_from_scene_packet" => Ok(crate::TrainingResultSpatialQueryReason::TargetBlockAbsentFromScenePacket),
    "reference_projection_failed" => Ok(crate::TrainingResultSpatialQueryReason::ReferenceProjectionFailed),
    "provider_command_failed" => Ok(crate::TrainingResultSpatialQueryReason::ProviderCommandFailed),
    "provider_output_invalid" => Ok(crate::TrainingResultSpatialQueryReason::ProviderOutputInvalid),
    other => Err(format!("invalid spatial query reason label `{other}`")),
  }
}

fn parse_spatial_query_visibility(label: &str) -> Result<crate::ProjectionVisibility, String> {
  match label {
    "visible" => Ok(crate::ProjectionVisibility::Visible),
    "behind_camera" => Ok(crate::ProjectionVisibility::BehindCamera),
    "out_of_frustum" => Ok(crate::ProjectionVisibility::OutOfFrustum),
    "outside_window" => Ok(crate::ProjectionVisibility::OutsideWindow),
    other => Err(format!("invalid spatial query visibility label `{other}`")),
  }
}

fn parse_spatial_query_screen_point(label: &str) -> Result<auv_driver::geometry::Point, String> {
  let parts: Vec<&str> = label.split(',').collect();
  if parts.len() != 2 {
    return Err(format!("invalid spatial query screen_point label `{label}`"));
  }
  let x = parts[0].parse::<f64>().map_err(|error| format!("invalid spatial query screen_point x `{label}`: {error}"))?;
  let y = parts[1].parse::<f64>().map_err(|error| format!("invalid spatial query screen_point y `{label}`: {error}"))?;
  Ok(auv_driver::geometry::Point::new(x, y))
}

fn parse_spatial_query_backend(label: &str) -> Result<crate::TrainingResultSpatialQueryBackend, String> {
  match label {
    "command_provider" => Ok(crate::TrainingResultSpatialQueryBackend::CommandProvider),
    "checkpoint_native" => Ok(crate::TrainingResultSpatialQueryBackend::CheckpointNative),
    "closed_scene_toy" => Ok(crate::TrainingResultSpatialQueryBackend::ClosedSceneToy),
    "projection_reference" => Ok(crate::TrainingResultSpatialQueryBackend::ProjectionReference),
    other => Err(format!("invalid spatial query selected_backend label `{other}`")),
  }
}

fn parse_spatial_query_comparison_verdict(label: &str) -> Result<crate::TrainingResultSpatialQueryComparisonVerdict, String> {
  match label {
    "match" => Ok(crate::TrainingResultSpatialQueryComparisonVerdict::Match),
    "divergent" => Ok(crate::TrainingResultSpatialQueryComparisonVerdict::Divergent),
    "provider_only" => Ok(crate::TrainingResultSpatialQueryComparisonVerdict::ProviderOnly),
    "reference_only" => Ok(crate::TrainingResultSpatialQueryComparisonVerdict::ReferenceOnly),
    "not_comparable" => Ok(crate::TrainingResultSpatialQueryComparisonVerdict::NotComparable),
    other => Err(format!("invalid spatial query comparison_verdict label `{other}`")),
  }
}

pub(crate) fn extract_minecraft_training_package_manifests(
  store: &LocalStore,
  run: &CanonicalRun,
) -> Result<Vec<MinecraftTrainingPackageManifestLineage>, String> {
  let mut manifests = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::MINECRAFT_3DGS_TRAINING_PACKAGE_ARTIFACT_ROLE {
      continue;
    }

    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      manifests.push(MinecraftTrainingPackageManifestLineage {
        artifact: artifact_ref,
        manifest: None,
        issue: Some(format!("minecraft training package artifact mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }

    let parsed = read_artifact_json::<TrainingPackageManifest>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::MINECRAFT_3DGS_TRAINING_PACKAGE_ARTIFACT_ROLE,
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
) -> Result<Vec<MinecraftTrainingPackageInspectReportLineage>, String> {
  let mut reports = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::MINECRAFT_3DGS_TRAINING_PACKAGE_INSPECT_ARTIFACT_ROLE {
      continue;
    }

    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      reports.push(MinecraftTrainingPackageInspectReportLineage {
        artifact: artifact_ref,
        report: None,
        issue: Some(format!("minecraft training package inspect artifact mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }

    let parsed = read_artifact_json::<TrainingPackageInspectReport>(
      store,
      run.run.run_id.as_str(),
      artifact,
      crate::MINECRAFT_3DGS_TRAINING_PACKAGE_INSPECT_ARTIFACT_ROLE,
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
) -> Result<Vec<MinecraftTelemetrySampleArtifactLineage>, String> {
  let mut artifacts = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != crate::TELEMETRY_SAMPLE_ARTIFACT_ROLE {
      continue;
    }

    let artifact_ref = artifact_record_view(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      artifacts.push(MinecraftTelemetrySampleArtifactLineage {
        artifact: artifact_ref,
        line_count: None,
        byte_size: None,
        issue: Some(format!("minecraft telemetry sample artifact mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }

    let parsed = read_telemetry_artifact_summary(store, run.run.run_id.as_str(), artifact, crate::TELEMETRY_SAMPLE_ARTIFACT_ROLE);

    match parsed {
      Ok((artifact_path, line_count, byte_size)) => artifacts.push(MinecraftTelemetrySampleArtifactLineage {
        artifact: ArtifactRefView {
          path: Some(artifact_path.display().to_string()),
          ..artifact_ref
        },
        line_count: Some(line_count),
        byte_size: Some(byte_size),
        issue: None,
      }),
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
      source_training_package_inspect_report_path: value.source_training_package_inspect_report_path,
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
      readiness_blocker: value.readiness_blocker.map(|blocker| format!("{blocker:?}")),
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
      source_training_package_inspect_report_path: value.source_training_package_inspect_report_path,
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
      readiness_blocker: value.readiness_blocker.map(|blocker| format!("{blocker:?}")),
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
      readiness_blocker: value.readiness_blocker.map(|blocker| format!("{blocker:?}")),
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
      result_artifacts: value.result_artifacts.into_iter().map(MinecraftTrainingResultArtifactSummary::from).collect(),
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
      status_reason: value.status_reason.map(|reason| reason.as_str().to_string()),
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

impl From<TrainingResultArtifactFetchManifest> for MinecraftTrainingResultArtifactFetchManifestSummary {
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
      normalized_artifacts: value.normalized_artifacts.into_iter().map(MinecraftTrainingResultNormalizedArtifactSummary::from).collect(),
      known_limits: value.known_limits,
    }
  }
}

impl From<TrainingResultArtifactFetchInspectReport> for MinecraftTrainingResultArtifactFetchInspectReportSummary {
  fn from(value: TrainingResultArtifactFetchInspectReport) -> Self {
    Self {
      schema_version: value.schema_version,
      training_result_artifact_fetch_manifest_path: value.training_result_artifact_fetch_manifest_path,
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

fn spatial_query_block_label(block: crate::BlockPosition) -> String {
  format!("{},{},{}", block.x, block.y, block.z)
}

fn spatial_query_optional_block_face_label(face: Option<crate::BlockFace>) -> Option<String> {
  face.map(|value| match value {
    crate::BlockFace::Up => "up".to_string(),
    crate::BlockFace::Down => "down".to_string(),
    crate::BlockFace::North => "north".to_string(),
    crate::BlockFace::South => "south".to_string(),
    crate::BlockFace::East => "east".to_string(),
    crate::BlockFace::West => "west".to_string(),
  })
}

fn spatial_query_target_semantics_label(semantics: crate::MinecraftTargetSemantics) -> String {
  match semantics {
    crate::MinecraftTargetSemantics::HitFaceCenter => "hit_face_center".to_string(),
    crate::MinecraftTargetSemantics::BlockCenter => "block_center".to_string(),
  }
}

fn spatial_query_kind_label(kind: crate::TrainingResultSpatialQueryKind) -> String {
  match kind {
    crate::TrainingResultSpatialQueryKind::BlockProjection => "block_projection".to_string(),
  }
}

fn spatial_query_visibility_label(visibility: crate::ProjectionVisibility) -> String {
  match visibility {
    crate::ProjectionVisibility::Visible => "visible".to_string(),
    crate::ProjectionVisibility::BehindCamera => "behind_camera".to_string(),
    crate::ProjectionVisibility::OutOfFrustum => "out_of_frustum".to_string(),
    crate::ProjectionVisibility::OutsideWindow => "outside_window".to_string(),
  }
}

fn spatial_query_screen_point_label(point: auv_driver::geometry::Point) -> String {
  format!("{},{}", point.x, point.y)
}

fn spatial_query_manifest_fields(value: &TrainingResultSpatialQueryManifest) -> MinecraftTrainingResultSpatialQueryManifestSummary {
  MinecraftTrainingResultSpatialQueryManifestSummary {
    schema_version: value.schema_version,
    training_result_semantic_manifest_path: value.training_result_semantic_manifest_path.clone(),
    source_training_result_artifact_manifest_path: value.source_training_result_artifact_manifest_path.clone(),
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
    selected_backend: value.selected_backend.map(|backend| backend.as_str().to_string()),
    status: value.status.as_str().to_string(),
    reason: value.reason.map(|reason| reason.as_str().to_string()),
    visibility: value.visibility.map(spatial_query_visibility_label),
    screen_point: value.screen_point.map(spatial_query_screen_point_label),
    match_radius_px: value.match_radius_px,
    confidence: value.confidence,
    basis_frame_id: value.basis_frame_id.clone(),
    comparison_verdict: value.comparison_verdict.map(|verdict| verdict.as_str().to_string()),
    known_limits: value.known_limits.clone(),
  }
}

impl From<crate::HoldoutRenderQualityMetrics> for MinecraftHoldoutRenderQualityMetricsSummary {
  fn from(value: crate::HoldoutRenderQualityMetrics) -> Self {
    Self {
      l1_mean: value.l1_mean,
      mse: value.mse,
      psnr: value.psnr,
    }
  }
}

impl From<TrainingResultHoldoutRenderQualityManifest> for MinecraftHoldoutRenderQualityManifestSummary {
  fn from(value: TrainingResultHoldoutRenderQualityManifest) -> Self {
    Self {
      schema_version: value.schema_version,
      training_result_semantic_manifest_path: value.training_result_semantic_manifest_path,
      holdout_preview_manifest_path: value.holdout_preview_manifest_path,
      source_training_result_artifact_manifest_path: value.source_training_result_artifact_manifest_path,
      source_training_result_manifest_path: value.source_training_result_manifest_path,
      source_training_job_manifest_path: value.source_training_job_manifest_path,
      source_scene_packet_manifest_path: value.source_scene_packet_manifest_path,
      source_run_ids: value.source_run_ids,
      holdout_frame_index: value.holdout_frame_index,
      basis_checkpoint_path: value.basis_checkpoint_path,
      rendered_image_path: value.rendered_image_path,
      image_size_match: value.image_size_match,
      metrics: value.metrics.map(MinecraftHoldoutRenderQualityMetricsSummary::from),
      status: value.status.as_str().to_string(),
      reason: value.reason.map(|reason| reason.as_str().to_string()),
      verdict: value.verdict.as_str().to_string(),
      known_limits: value.known_limits,
    }
  }
}

impl From<TrainingResultHoldoutRenderQualityInspectReport> for MinecraftHoldoutRenderQualityInspectReportSummary {
  fn from(value: TrainingResultHoldoutRenderQualityInspectReport) -> Self {
    Self {
      schema_version: value.schema_version,
      training_result_holdout_render_quality_manifest_path: value.training_result_holdout_render_quality_manifest_path,
      training_result_semantic_manifest_path: value.training_result_semantic_manifest_path,
      holdout_preview_manifest_path: value.holdout_preview_manifest_path,
      source_training_result_artifact_manifest_path: value.source_training_result_artifact_manifest_path,
      source_training_result_manifest_path: value.source_training_result_manifest_path,
      source_training_job_manifest_path: value.source_training_job_manifest_path,
      source_scene_packet_manifest_path: value.source_scene_packet_manifest_path,
      source_run_ids: value.source_run_ids,
      holdout_frame_index: value.holdout_frame_index,
      basis_checkpoint_path: value.basis_checkpoint_path,
      rendered_image_path: value.rendered_image_path,
      image_size_match: value.image_size_match,
      metrics: value.metrics.map(MinecraftHoldoutRenderQualityMetricsSummary::from),
      status: value.status.as_str().to_string(),
      reason: value.reason.map(|reason| reason.as_str().to_string()),
      verdict: value.verdict.as_str().to_string(),
      warnings: value.warnings,
      known_limits: value.known_limits,
    }
  }
}

impl From<TrainingResultSpatialQueryManifest> for MinecraftTrainingResultSpatialQueryManifestSummary {
  fn from(value: TrainingResultSpatialQueryManifest) -> Self {
    spatial_query_manifest_fields(&value)
  }
}

impl From<TrainingResultSpatialQueryInspectReport> for MinecraftTrainingResultSpatialQueryInspectReportSummary {
  fn from(value: TrainingResultSpatialQueryInspectReport) -> Self {
    let manifest_fields = spatial_query_manifest_fields(&TrainingResultSpatialQueryManifest {
      schema_version: value.schema_version,
      generated_at_millis: value.generated_at_millis,
      training_result_semantic_manifest_path: value.training_result_semantic_manifest_path.clone(),
      source_training_result_artifact_manifest_path: value.source_training_result_artifact_manifest_path.clone(),
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
      training_result_spatial_query_manifest_path: value.training_result_spatial_query_manifest_path.clone(),
      training_result_semantic_manifest_path: manifest_fields.training_result_semantic_manifest_path.clone(),
      source_training_result_artifact_manifest_path: manifest_fields.source_training_result_artifact_manifest_path.clone(),
      source_training_result_manifest_path: manifest_fields.source_training_result_manifest_path.clone(),
      source_training_job_manifest_path: manifest_fields.source_training_job_manifest_path.clone(),
      source_training_launch_plan_path: manifest_fields.source_training_launch_plan_path.clone(),
      source_training_package_manifest_path: manifest_fields.source_training_package_manifest_path.clone(),
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
      provider_reason: value.provider_reason.map(|reason| reason.as_str().to_string()),
      provider_message: value.provider_message.clone(),
      reference_status: value.reference_status.as_str().to_string(),
      reference_reason: value.reference_reason.map(|reason| reason.as_str().to_string()),
      reference_basis_frame_id: value.reference_basis_frame_id.clone(),
      reference_source_frame_json_path: value.reference_source_frame_json_path.clone(),
      reference_screenshot_path: value.reference_screenshot_path.clone(),
      scene_packet_frame_count: value.scene_packet_frame_count,
      warnings: value.warnings.clone(),
      known_limits: manifest_fields.known_limits.clone(),
    }
  }
}

impl From<TrainingResultSemanticCheckpointRecord> for MinecraftTrainingResultSemanticCheckpointSummary {
  fn from(value: TrainingResultSemanticCheckpointRecord) -> Self {
    Self {
      relative_path: value.relative_path,
      byte_size: value.byte_size,
    }
  }
}

impl From<crate::HoldoutFrameWitness> for MinecraftTrainingResultHoldoutPreviewFrameWitnessSummary {
  fn from(value: crate::HoldoutFrameWitness) -> Self {
    Self {
      frame_index: value.frame_index,
      spatial_frame_id: value.spatial_frame_id,
      screenshot_path: value.screenshot_path,
      frame_json_path: value.frame_json_path,
    }
  }
}

impl From<TrainingResultHoldoutPreviewManifest> for MinecraftTrainingResultHoldoutPreviewManifestSummary {
  fn from(value: TrainingResultHoldoutPreviewManifest) -> Self {
    Self {
      schema_version: value.schema_version,
      training_result_semantic_manifest_path: value.training_result_semantic_manifest_path,
      source_training_result_artifact_manifest_path: value.source_training_result_artifact_manifest_path,
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
      holdout_frame: value.holdout_frame.map(MinecraftTrainingResultHoldoutPreviewFrameWitnessSummary::from),
      basis_checkpoint_path: value.basis_checkpoint_path,
      holdout_screenshot_path: value.holdout_screenshot_path,
      reference_overlay_path: value.reference_overlay_path,
      status: value.status.as_str().to_string(),
      reason: value.reason.map(|reason| reason.as_str().to_string()),
      known_limits: value.known_limits,
    }
  }
}

impl From<TrainingResultHoldoutPreviewInspectReport> for MinecraftTrainingResultHoldoutPreviewInspectReportSummary {
  fn from(value: TrainingResultHoldoutPreviewInspectReport) -> Self {
    Self {
      schema_version: value.schema_version,
      training_result_holdout_preview_manifest_path: value.training_result_holdout_preview_manifest_path,
      training_result_semantic_manifest_path: value.training_result_semantic_manifest_path,
      source_training_result_artifact_manifest_path: value.source_training_result_artifact_manifest_path,
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
      holdout_frame: value.holdout_frame.map(MinecraftTrainingResultHoldoutPreviewFrameWitnessSummary::from),
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
      source_training_result_artifact_manifest_path: value.source_training_result_artifact_manifest_path,
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
      semantic_reason: value.semantic_reason.map(|reason| reason.as_str().to_string()),
      config_path: value.config_path,
      models_dir_path: value.models_dir_path,
      status_snapshot_path: value.status_snapshot_path,
      config_trainer: value.config_trainer,
      checkpoint_files: value.checkpoint_files.into_iter().map(MinecraftTrainingResultSemanticCheckpointSummary::from).collect(),
      checkpoint_count: value.checkpoint_count,
      known_limits: value.known_limits,
    }
  }
}

impl From<TrainingResultSemanticInspectReport> for MinecraftTrainingResultSemanticInspectReportSummary {
  fn from(value: TrainingResultSemanticInspectReport) -> Self {
    Self {
      schema_version: value.schema_version,
      training_result_semantic_manifest_path: value.training_result_semantic_manifest_path,
      source_training_result_artifact_manifest_path: value.source_training_result_artifact_manifest_path,
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
      semantic_reason: value.semantic_reason.map(|reason| reason.as_str().to_string()),
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

impl From<crate::TrainingResultNormalizedArtifactRecord> for MinecraftTrainingResultNormalizedArtifactSummary {
  fn from(value: crate::TrainingResultNormalizedArtifactRecord) -> Self {
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
