use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use auv_file::{
  JsonFileReadError, JsonFileWriteError, JsonWriteOptions, read_json_file as read_json_file_helper,
  write_json_file as write_json_file_helper,
};
use auv_stage_status::StageStatus;
use image::RgbImage;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::scene_packet::ScenePacketFramePayload;
use crate::training_result_holdout_preview::{HoldoutFrameWitness, TrainingResultHoldoutPreviewManifest};
use crate::training_result_semantic::TrainingResultSemanticManifest;
use crate::types::{MinecraftSpatialFrame, PlayerPose, Viewport};

pub type TrainingResultHoldoutRenderQualityResult<T> = Result<T, String>;

pub const TRAINING_RESULT_HOLDOUT_RENDER_QUALITY_MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const TRAINING_RESULT_HOLDOUT_RENDER_QUALITY_INSPECT_REPORT_SCHEMA_VERSION: u32 = 1;

pub const MC17_V1_HOLDOUT_RENDER_QUALITY_KNOWN_LIMIT: &str =
  "MC-17 v1 records photometric metrics as evidence only; pass/fail thresholds and trained splat usefulness verdicts are deferred";

const HOLDOUT_RENDER_QUALITY_MANIFEST_FILE: &str = "minecraft-3dgs-holdout-render-quality.json";
const HOLDOUT_RENDER_QUALITY_INSPECT_FILE: &str = "minecraft-3dgs-holdout-render-quality-inspect.json";

const METRIC_PARTIAL_KNOWN_LIMIT: &str = "MC-17 does not resize, crop, or auto-align mismatched holdout images in D1";
const SSIM_DEFERRED_KNOWN_LIMIT: &str = "MC-17 D1 defers SSIM computation";
const PERFECT_MATCH_PSNR_KNOWN_LIMIT: &str = "MC-17 omits PSNR when MSE is zero (identical RGB8 pixels)";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrainingResultHoldoutRenderQualityInputs {
  pub training_result_semantic_manifest_path: PathBuf,
  pub holdout_preview_manifest_path: PathBuf,
  pub render_command: String,
  pub output_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrainingResultHoldoutRenderQualityOutput {
  pub output_dir: PathBuf,
  pub manifest_path: PathBuf,
  pub inspect_report_path: PathBuf,
  pub manifest: TrainingResultHoldoutRenderQualityManifest,
  pub inspect_report: TrainingResultHoldoutRenderQualityInspectReport,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HoldoutRenderQualityRequest {
  pub normalized_result_dir: String,
  pub config_path: String,
  pub basis_checkpoint_path: String,
  pub holdout_frame_index: usize,
  pub holdout_frame_json_path: String,
  pub holdout_screenshot_path: String,
  pub viewport: Viewport,
  pub view_matrix: [f64; 16],
  pub projection_matrix: [f64; 16],
  pub player_pose: PlayerPose,
  pub requested_rendered_image_path: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HoldoutRenderQualityAnswer {
  pub status: StageStatus,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub rendered_image_path: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub message: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub known_limits: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HoldoutRenderQualityImageSize {
  pub width: u32,
  pub height: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HoldoutRenderQualityMetrics {
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub l1_mean: Option<f64>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub mse: Option<f64>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub psnr: Option<f64>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub ssim: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoldoutRenderQualityBackend {
  ExternalCommand,
}

impl HoldoutRenderQualityBackend {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::ExternalCommand => "external_command",
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrainingResultHoldoutRenderQualityManifest {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub training_result_semantic_manifest_path: String,
  pub holdout_preview_manifest_path: String,
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
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub holdout_frame: Option<HoldoutFrameWitness>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub basis_checkpoint_path: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub holdout_screenshot_path: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub rendered_image_path: Option<String>,
  pub render_backend: HoldoutRenderQualityBackend,
  pub image_size_match: bool,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub source_image_size: Option<HoldoutRenderQualityImageSize>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub rendered_image_size: Option<HoldoutRenderQualityImageSize>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub metrics: Option<HoldoutRenderQualityMetrics>,
  pub status: StageStatus,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reason: Option<HoldoutRenderQualityReason>,
  pub verdict: HoldoutRenderQualityVerdict,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrainingResultHoldoutRenderQualityInspectReport {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub training_result_holdout_render_quality_manifest_path: String,
  pub training_result_semantic_manifest_path: String,
  pub holdout_preview_manifest_path: String,
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
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub holdout_frame: Option<HoldoutFrameWitness>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub basis_checkpoint_path: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub holdout_screenshot_path: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub rendered_image_path: Option<String>,
  pub render_backend: HoldoutRenderQualityBackend,
  pub image_size_match: bool,
  pub l1_mean_available: bool,
  pub mse_available: bool,
  pub psnr_available: bool,
  pub ssim_available: bool,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub metrics: Option<HoldoutRenderQualityMetrics>,
  pub status: StageStatus,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reason: Option<HoldoutRenderQualityReason>,
  pub verdict: HoldoutRenderQualityVerdict,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoldoutRenderQualityReason {
  HoldoutPreviewUnreadable,
  HoldoutPreviewNotReady,
  SemanticSourceUnreadable,
  LineageMismatch,
  ConfigMissing,
  CheckpointMissing,
  HoldoutScreenshotMissing,
  HoldoutFrameJsonUnreadable,
  HoldoutRenderCommandFailed,
  RenderedImageMissing,
}

impl HoldoutRenderQualityReason {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::HoldoutPreviewUnreadable => "holdout_preview_unreadable",
      Self::HoldoutPreviewNotReady => "holdout_preview_not_ready",
      Self::SemanticSourceUnreadable => "semantic_source_unreadable",
      Self::LineageMismatch => "lineage_mismatch",
      Self::ConfigMissing => "config_missing",
      Self::CheckpointMissing => "checkpoint_missing",
      Self::HoldoutScreenshotMissing => "holdout_screenshot_missing",
      Self::HoldoutFrameJsonUnreadable => "holdout_frame_json_unreadable",
      Self::HoldoutRenderCommandFailed => "holdout_render_command_failed",
      Self::RenderedImageMissing => "rendered_image_missing",
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoldoutRenderQualityVerdict {
  MeasuredOnly,
  MetricPartial,
  Blocked,
  Failed,
}

impl HoldoutRenderQualityVerdict {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::MeasuredOnly => "measured_only",
      Self::MetricPartial => "metric_partial",
      Self::Blocked => "blocked",
      Self::Failed => "failed",
    }
  }
}

#[derive(Clone, Debug, PartialEq)]
struct HoldoutRenderQualityOutcome {
  status: StageStatus,
  reason: Option<HoldoutRenderQualityReason>,
  verdict: HoldoutRenderQualityVerdict,
  holdout_frame_index: usize,
  holdout_frame: Option<HoldoutFrameWitness>,
  basis_checkpoint_path: Option<String>,
  holdout_screenshot_path: Option<String>,
  rendered_image_path: Option<String>,
  image_size_match: bool,
  source_image_size: Option<HoldoutRenderQualityImageSize>,
  rendered_image_size: Option<HoldoutRenderQualityImageSize>,
  metrics: Option<HoldoutRenderQualityMetrics>,
  lineage: Option<LineageSnapshot>,
  warnings: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LineageSnapshot {
  source_training_result_artifact_manifest_path: String,
  source_training_result_manifest_path: String,
  source_training_job_manifest_path: String,
  source_training_launch_plan_path: String,
  source_training_package_manifest_path: String,
  source_scene_packet_manifest_path: String,
  source_bundle_manifest_paths: Vec<String>,
  source_run_ids: Vec<String>,
  trainer_backend: String,
  job_backend: String,
  normalized_result_dir: String,
}

pub fn measure_3dgs_holdout_render_quality(
  inputs: TrainingResultHoldoutRenderQualityInputs,
) -> TrainingResultHoldoutRenderQualityResult<TrainingResultHoldoutRenderQualityOutput> {
  fs::create_dir_all(&inputs.output_dir).map_err(|error| format!("failed to create output dir {}: {error}", inputs.output_dir.display()))?;

  let generated_at_millis = auv_tracing_driver::now_millis();
  let mut known_limits = BTreeSet::new();
  known_limits.insert(MC17_V1_HOLDOUT_RENDER_QUALITY_KNOWN_LIMIT.to_string());
  known_limits.insert(METRIC_PARTIAL_KNOWN_LIMIT.to_string());
  known_limits.insert(SSIM_DEFERRED_KNOWN_LIMIT.to_string());

  let holdout_preview_manifest =
    read_json_file::<TrainingResultHoldoutPreviewManifest>(&inputs.holdout_preview_manifest_path, "MC-16 holdout preview manifest").ok();

  let outcome = if holdout_preview_manifest.is_none() {
    blocked_outcome(
      HoldoutRenderQualityReason::HoldoutPreviewUnreadable,
      0,
      None,
      BTreeSet::from(["MC-16 holdout preview manifest is unreadable; MC-17 records blocked quality evidence only".to_string()]),
    )
  } else {
    let holdout_preview = holdout_preview_manifest.as_ref().expect("holdout preview present");
    known_limits.extend(holdout_preview.known_limits.iter().cloned());

    if holdout_preview.status != StageStatus::Ready
      || holdout_preview.holdout_frame.is_none()
      || holdout_preview.basis_checkpoint_path.is_none()
    {
      blocked_outcome(
        HoldoutRenderQualityReason::HoldoutPreviewNotReady,
        holdout_preview.holdout_frame_index,
        holdout_preview.holdout_frame.clone(),
        BTreeSet::from(["MC-16 holdout preview is not ready; MC-17 does not re-select holdout frames".to_string()]),
      )
    } else {
      match read_json_file::<TrainingResultSemanticManifest>(
        &inputs.training_result_semantic_manifest_path,
        "MC-10 training result semantic manifest",
      ) {
        Err(_) => blocked_outcome(
          HoldoutRenderQualityReason::SemanticSourceUnreadable,
          holdout_preview.holdout_frame_index,
          holdout_preview.holdout_frame.clone(),
          BTreeSet::new(),
        ),
        Ok(semantic_manifest) => evaluate_semantic_ready_gates(&inputs, holdout_preview, &semantic_manifest)?,
      }
    }
  };

  finish_output(inputs, generated_at_millis, outcome, holdout_preview_manifest.as_ref(), known_limits)
}

fn evaluate_semantic_ready_gates(
  inputs: &TrainingResultHoldoutRenderQualityInputs,
  holdout_preview: &TrainingResultHoldoutPreviewManifest,
  semantic_manifest: &TrainingResultSemanticManifest,
) -> TrainingResultHoldoutRenderQualityResult<HoldoutRenderQualityOutcome> {
  if !lineage_matches_semantic(&inputs.training_result_semantic_manifest_path, semantic_manifest, holdout_preview) {
    return Ok(blocked_outcome(
      HoldoutRenderQualityReason::LineageMismatch,
      holdout_preview.holdout_frame_index,
      holdout_preview.holdout_frame.clone(),
      BTreeSet::from(["MC-10 semantic manifest business keys do not match MC-16 holdout preview lineage".to_string()]),
    ));
  }

  if !Path::new(&semantic_manifest.config_path).is_file() {
    return Ok(blocked_outcome(
      HoldoutRenderQualityReason::ConfigMissing,
      holdout_preview.holdout_frame_index,
      holdout_preview.holdout_frame.clone(),
      BTreeSet::new(),
    ));
  }

  let basis_checkpoint_path = holdout_preview.basis_checkpoint_path.clone().expect("basis checkpoint present");
  if !Path::new(&basis_checkpoint_path).is_file() {
    return Ok(blocked_outcome(
      HoldoutRenderQualityReason::CheckpointMissing,
      holdout_preview.holdout_frame_index,
      holdout_preview.holdout_frame.clone(),
      BTreeSet::new(),
    ));
  }

  let holdout_screenshot_path = holdout_preview
    .holdout_screenshot_path
    .clone()
    .or_else(|| holdout_preview.holdout_frame.as_ref().map(|witness| witness.screenshot_path.clone()))
    .filter(|path| !path.is_empty());

  let Some(holdout_screenshot_path) = holdout_screenshot_path else {
    return Ok(blocked_outcome(
      HoldoutRenderQualityReason::HoldoutScreenshotMissing,
      holdout_preview.holdout_frame_index,
      holdout_preview.holdout_frame.clone(),
      BTreeSet::new(),
    ));
  };

  if !Path::new(&holdout_screenshot_path).is_file() {
    return Ok(blocked_outcome(
      HoldoutRenderQualityReason::HoldoutScreenshotMissing,
      holdout_preview.holdout_frame_index,
      holdout_preview.holdout_frame.clone(),
      BTreeSet::new(),
    ));
  }

  let holdout_frame = holdout_preview.holdout_frame.clone().expect("holdout frame present");
  let spatial_frame = match load_holdout_spatial_frame(&holdout_frame.frame_json_path) {
    Ok(frame) => frame,
    Err(error) => {
      return Ok(blocked_outcome(
        HoldoutRenderQualityReason::HoldoutFrameJsonUnreadable,
        holdout_preview.holdout_frame_index,
        Some(holdout_frame),
        BTreeSet::from([error]),
      ));
    }
  };

  Ok(run_holdout_render_and_metrics(
    inputs,
    holdout_preview,
    semantic_manifest,
    &holdout_frame,
    &holdout_screenshot_path,
    &basis_checkpoint_path,
    &spatial_frame,
  )?)
}

fn finish_output(
  inputs: TrainingResultHoldoutRenderQualityInputs,
  generated_at_millis: u64,
  outcome: HoldoutRenderQualityOutcome,
  holdout_preview_manifest: Option<&TrainingResultHoldoutPreviewManifest>,
  mut known_limits: BTreeSet<String>,
) -> TrainingResultHoldoutRenderQualityResult<TrainingResultHoldoutRenderQualityOutput> {
  known_limits.extend(outcome.warnings.iter().cloned());
  if outcome.verdict == HoldoutRenderQualityVerdict::MetricPartial {
    known_limits.insert(METRIC_PARTIAL_KNOWN_LIMIT.to_string());
  }
  if outcome.metrics.as_ref().is_some_and(|metrics| metrics.psnr.is_none() && metrics.mse == Some(0.0)) {
    known_limits.insert(PERFECT_MATCH_PSNR_KNOWN_LIMIT.to_string());
  }

  let lineage = outcome.lineage.clone().or_else(|| holdout_preview_manifest.map(lineage_from_holdout_preview));

  let (
    source_training_result_artifact_manifest_path,
    source_training_result_manifest_path,
    source_training_job_manifest_path,
    source_training_launch_plan_path,
    source_training_package_manifest_path,
    source_scene_packet_manifest_path,
    source_bundle_manifest_paths,
    source_run_ids,
    trainer_backend,
    job_backend,
    normalized_result_dir,
  ) = lineage.as_ref().map_or_else(
    || {
      (
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        Vec::new(),
        Vec::new(),
        String::new(),
        String::new(),
        String::new(),
      )
    },
    |lineage| {
      (
        lineage.source_training_result_artifact_manifest_path.clone(),
        lineage.source_training_result_manifest_path.clone(),
        lineage.source_training_job_manifest_path.clone(),
        lineage.source_training_launch_plan_path.clone(),
        lineage.source_training_package_manifest_path.clone(),
        lineage.source_scene_packet_manifest_path.clone(),
        lineage.source_bundle_manifest_paths.clone(),
        lineage.source_run_ids.clone(),
        lineage.trainer_backend.clone(),
        lineage.job_backend.clone(),
        lineage.normalized_result_dir.clone(),
      )
    },
  );

  let manifest_path = inputs.output_dir.join(HOLDOUT_RENDER_QUALITY_MANIFEST_FILE);
  let inspect_report_path = inputs.output_dir.join(HOLDOUT_RENDER_QUALITY_INSPECT_FILE);

  let metrics = outcome.metrics.clone();
  let warnings = outcome.warnings.iter().cloned().collect::<Vec<_>>();
  let known_limits = known_limits.into_iter().collect::<Vec<_>>();

  let manifest = TrainingResultHoldoutRenderQualityManifest {
    schema_version: TRAINING_RESULT_HOLDOUT_RENDER_QUALITY_MANIFEST_SCHEMA_VERSION,
    generated_at_millis,
    training_result_semantic_manifest_path: inputs.training_result_semantic_manifest_path.to_string_lossy().into_owned(),
    holdout_preview_manifest_path: inputs.holdout_preview_manifest_path.to_string_lossy().into_owned(),
    source_training_result_artifact_manifest_path,
    source_training_result_manifest_path,
    source_training_job_manifest_path,
    source_training_launch_plan_path,
    source_training_package_manifest_path,
    source_scene_packet_manifest_path,
    source_bundle_manifest_paths,
    source_run_ids,
    trainer_backend,
    job_backend,
    normalized_result_dir,
    holdout_frame_index: outcome.holdout_frame_index,
    holdout_frame: outcome.holdout_frame.clone(),
    basis_checkpoint_path: outcome.basis_checkpoint_path.clone(),
    holdout_screenshot_path: outcome.holdout_screenshot_path.clone(),
    rendered_image_path: outcome.rendered_image_path.clone(),
    render_backend: HoldoutRenderQualityBackend::ExternalCommand,
    image_size_match: outcome.image_size_match,
    source_image_size: outcome.source_image_size.clone(),
    rendered_image_size: outcome.rendered_image_size.clone(),
    metrics: metrics.clone(),
    status: outcome.status,
    reason: outcome.reason,
    verdict: outcome.verdict,
    known_limits: known_limits.clone(),
  };

  let inspect_report = TrainingResultHoldoutRenderQualityInspectReport {
    schema_version: TRAINING_RESULT_HOLDOUT_RENDER_QUALITY_INSPECT_REPORT_SCHEMA_VERSION,
    generated_at_millis,
    training_result_holdout_render_quality_manifest_path: manifest_path.to_string_lossy().into_owned(),
    training_result_semantic_manifest_path: manifest.training_result_semantic_manifest_path.clone(),
    holdout_preview_manifest_path: manifest.holdout_preview_manifest_path.clone(),
    source_training_result_artifact_manifest_path: manifest.source_training_result_artifact_manifest_path.clone(),
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
    holdout_frame_index: outcome.holdout_frame_index,
    holdout_frame: outcome.holdout_frame,
    basis_checkpoint_path: outcome.basis_checkpoint_path,
    holdout_screenshot_path: outcome.holdout_screenshot_path,
    rendered_image_path: outcome.rendered_image_path,
    render_backend: manifest.render_backend.clone(),
    image_size_match: outcome.image_size_match,
    l1_mean_available: metrics.as_ref().and_then(|value| value.l1_mean).is_some(),
    mse_available: metrics.as_ref().and_then(|value| value.mse).is_some(),
    psnr_available: metrics.as_ref().and_then(|value| value.psnr).is_some(),
    ssim_available: false,
    metrics,
    status: outcome.status,
    reason: outcome.reason,
    verdict: outcome.verdict,
    warnings,
    known_limits,
  };

  write_json_file(&manifest_path, &manifest, "MC-17 holdout render quality manifest")?;
  write_json_file(&inspect_report_path, &inspect_report, "MC-17 holdout render quality inspect report")?;

  Ok(TrainingResultHoldoutRenderQualityOutput {
    output_dir: inputs.output_dir,
    manifest_path,
    inspect_report_path,
    manifest,
    inspect_report,
  })
}

fn blocked_outcome(
  reason: HoldoutRenderQualityReason,
  holdout_frame_index: usize,
  holdout_frame: Option<HoldoutFrameWitness>,
  warnings: BTreeSet<String>,
) -> HoldoutRenderQualityOutcome {
  HoldoutRenderQualityOutcome {
    status: StageStatus::Blocked,
    reason: Some(reason),
    verdict: HoldoutRenderQualityVerdict::Blocked,
    holdout_frame_index,
    holdout_frame,
    basis_checkpoint_path: None,
    holdout_screenshot_path: None,
    rendered_image_path: None,
    image_size_match: false,
    source_image_size: None,
    rendered_image_size: None,
    metrics: None,
    lineage: None,
    warnings,
  }
}

fn failed_outcome(
  reason: HoldoutRenderQualityReason,
  holdout_preview: &TrainingResultHoldoutPreviewManifest,
  holdout_screenshot_path: Option<String>,
  rendered_image_path: Option<String>,
  warnings: BTreeSet<String>,
) -> HoldoutRenderQualityOutcome {
  HoldoutRenderQualityOutcome {
    status: StageStatus::Failed,
    reason: Some(reason),
    verdict: HoldoutRenderQualityVerdict::Failed,
    holdout_frame_index: holdout_preview.holdout_frame_index,
    holdout_frame: holdout_preview.holdout_frame.clone(),
    basis_checkpoint_path: holdout_preview.basis_checkpoint_path.clone(),
    holdout_screenshot_path,
    rendered_image_path,
    image_size_match: false,
    source_image_size: None,
    rendered_image_size: None,
    metrics: None,
    lineage: Some(lineage_from_holdout_preview(holdout_preview)),
    warnings,
  }
}

fn lineage_from_holdout_preview(holdout_preview: &TrainingResultHoldoutPreviewManifest) -> LineageSnapshot {
  LineageSnapshot {
    source_training_result_artifact_manifest_path: holdout_preview.source_training_result_artifact_manifest_path.clone(),
    source_training_result_manifest_path: holdout_preview.source_training_result_manifest_path.clone(),
    source_training_job_manifest_path: holdout_preview.source_training_job_manifest_path.clone(),
    source_training_launch_plan_path: holdout_preview.source_training_launch_plan_path.clone(),
    source_training_package_manifest_path: holdout_preview.source_training_package_manifest_path.clone(),
    source_scene_packet_manifest_path: holdout_preview.source_scene_packet_manifest_path.clone(),
    source_bundle_manifest_paths: holdout_preview.source_bundle_manifest_paths.clone(),
    source_run_ids: holdout_preview.source_run_ids.clone(),
    trainer_backend: holdout_preview.trainer_backend.clone(),
    job_backend: holdout_preview.job_backend.clone(),
    normalized_result_dir: holdout_preview.normalized_result_dir.clone(),
  }
}

fn lineage_matches_semantic(
  semantic_manifest_path: &Path,
  semantic_manifest: &TrainingResultSemanticManifest,
  holdout_preview: &TrainingResultHoldoutPreviewManifest,
) -> bool {
  semantic_manifest_path.to_string_lossy().into_owned() == holdout_preview.training_result_semantic_manifest_path
    && semantic_manifest.source_training_result_artifact_manifest_path == holdout_preview.source_training_result_artifact_manifest_path
    && semantic_manifest.source_training_result_manifest_path == holdout_preview.source_training_result_manifest_path
    && semantic_manifest.source_training_job_manifest_path == holdout_preview.source_training_job_manifest_path
    && semantic_manifest.source_training_launch_plan_path == holdout_preview.source_training_launch_plan_path
    && semantic_manifest.source_training_package_manifest_path == holdout_preview.source_training_package_manifest_path
    && semantic_manifest.source_scene_packet_manifest_path == holdout_preview.source_scene_packet_manifest_path
    && semantic_manifest.source_bundle_manifest_paths == holdout_preview.source_bundle_manifest_paths
    && semantic_manifest.source_run_ids == holdout_preview.source_run_ids
    && semantic_manifest.normalized_result_dir == holdout_preview.normalized_result_dir
    && semantic_manifest.trainer_backend == holdout_preview.trainer_backend
    && semantic_manifest.job_backend == holdout_preview.job_backend
}

fn run_holdout_render_and_metrics(
  inputs: &TrainingResultHoldoutRenderQualityInputs,
  holdout_preview: &TrainingResultHoldoutPreviewManifest,
  semantic_manifest: &TrainingResultSemanticManifest,
  holdout_frame: &HoldoutFrameWitness,
  holdout_screenshot_path: &str,
  basis_checkpoint_path: &str,
  spatial_frame: &MinecraftSpatialFrame,
) -> TrainingResultHoldoutRenderQualityResult<HoldoutRenderQualityOutcome> {
  let requested_rendered_image_path = inputs.output_dir.join(format!("rendered_holdout_frame_{:06}.png", holdout_frame.frame_index));
  let requested_rendered_image_path_string = requested_rendered_image_path.to_string_lossy().into_owned();

  let request = HoldoutRenderQualityRequest {
    normalized_result_dir: semantic_manifest.normalized_result_dir.clone(),
    config_path: semantic_manifest.config_path.clone(),
    basis_checkpoint_path: basis_checkpoint_path.to_string(),
    holdout_frame_index: holdout_frame.frame_index,
    holdout_frame_json_path: holdout_frame.frame_json_path.clone(),
    holdout_screenshot_path: holdout_screenshot_path.to_string(),
    viewport: spatial_frame.viewport,
    view_matrix: spatial_frame.view_matrix,
    projection_matrix: spatial_frame.projection_matrix,
    player_pose: spatial_frame.player_pose.clone(),
    requested_rendered_image_path: requested_rendered_image_path_string.clone(),
  };

  let answer = match run_external_holdout_render_quality(&inputs.render_command, &request) {
    Ok(answer) => answer,
    Err(error) => {
      return Ok(failed_outcome(
        HoldoutRenderQualityReason::HoldoutRenderCommandFailed,
        holdout_preview,
        Some(holdout_screenshot_path.to_string()),
        None,
        BTreeSet::from([error]),
      ));
    }
  };

  if answer.status != StageStatus::Ready {
    return Ok(failed_outcome(
      HoldoutRenderQualityReason::HoldoutRenderCommandFailed,
      holdout_preview,
      Some(holdout_screenshot_path.to_string()),
      answer.rendered_image_path,
      answer.message.into_iter().collect::<BTreeSet<_>>(),
    ));
  }

  let rendered_image_path = answer.rendered_image_path.unwrap_or(requested_rendered_image_path_string);

  if !Path::new(&rendered_image_path).is_file() {
    return Ok(failed_outcome(
      HoldoutRenderQualityReason::RenderedImageMissing,
      holdout_preview,
      Some(holdout_screenshot_path.to_string()),
      Some(rendered_image_path),
      BTreeSet::new(),
    ));
  }

  let source_image = image::open(holdout_screenshot_path)
    .map_err(|error| format!("failed to decode holdout screenshot {}: {error}", holdout_screenshot_path))?;
  let rendered_image = image::open(&rendered_image_path)
    .map_err(|error| format!("failed to decode rendered holdout image {}: {error}", rendered_image_path))?;

  let source_rgb = source_image.into_rgb8();
  let rendered_rgb = rendered_image.into_rgb8();
  let source_image_size = image_size_from_rgb(&source_rgb);
  let rendered_image_size = image_size_from_rgb(&rendered_rgb);

  let mut warnings = BTreeSet::new();
  if let Some(message) = answer.message {
    warnings.insert(message);
  }

  if source_rgb.width() == rendered_rgb.width() && source_rgb.height() == rendered_rgb.height() {
    let metrics = compute_rgb8_metrics(&source_rgb, &rendered_rgb);
    Ok(HoldoutRenderQualityOutcome {
      status: StageStatus::Ready,
      reason: None,
      verdict: HoldoutRenderQualityVerdict::MeasuredOnly,
      holdout_frame_index: holdout_preview.holdout_frame_index,
      holdout_frame: Some(holdout_frame.clone()),
      basis_checkpoint_path: Some(basis_checkpoint_path.to_string()),
      holdout_screenshot_path: Some(holdout_screenshot_path.to_string()),
      rendered_image_path: Some(rendered_image_path),
      image_size_match: true,
      source_image_size: Some(source_image_size),
      rendered_image_size: Some(rendered_image_size),
      metrics: Some(metrics),
      lineage: Some(lineage_from_holdout_preview(holdout_preview)),
      warnings,
    })
  } else {
    warnings.insert("holdout screenshot and rendered image dimensions differ; MC-17 records metric_partial without resize".to_string());
    Ok(HoldoutRenderQualityOutcome {
      status: StageStatus::Ready,
      reason: None,
      verdict: HoldoutRenderQualityVerdict::MetricPartial,
      holdout_frame_index: holdout_preview.holdout_frame_index,
      holdout_frame: Some(holdout_frame.clone()),
      basis_checkpoint_path: Some(basis_checkpoint_path.to_string()),
      holdout_screenshot_path: Some(holdout_screenshot_path.to_string()),
      rendered_image_path: Some(rendered_image_path),
      image_size_match: false,
      source_image_size: Some(source_image_size),
      rendered_image_size: Some(rendered_image_size),
      metrics: None,
      lineage: Some(lineage_from_holdout_preview(holdout_preview)),
      warnings,
    })
  }
}

fn run_external_holdout_render_quality(
  command_text: &str,
  request: &HoldoutRenderQualityRequest,
) -> TrainingResultHoldoutRenderQualityResult<HoldoutRenderQualityAnswer> {
  let mut command = Command::new("sh");
  command.arg("-lc").arg(command_text).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());

  let mut child = command.spawn().map_err(|error| format!("failed to run MC-17 holdout render command: {error}"))?;
  {
    let stdin = child.stdin.as_mut().ok_or_else(|| "failed to open stdin for MC-17 holdout render quality command".to_string())?;
    serde_json::to_writer(&mut *stdin, request)
      .map_err(|error| format!("failed to write MC-17 holdout render quality request JSON: {error}"))?;
    stdin.write_all(b"\n").map_err(|error| format!("failed to finish MC-17 holdout render quality request JSON: {error}"))?;
  }

  let output = child.wait_with_output().map_err(|error| format!("failed to wait for MC-17 holdout render command: {error}"))?;

  if !output.status.success() {
    return Err(format!(
      "MC-17 holdout render command exited with status {}: {}",
      output.status,
      String::from_utf8_lossy(&output.stderr).trim()
    ));
  }

  let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
  serde_json::from_str::<HoldoutRenderQualityAnswer>(&stdout)
    .map_err(|error| format!("failed to parse MC-17 holdout render command output: {error}"))
}

fn load_holdout_spatial_frame(frame_json_path: &str) -> TrainingResultHoldoutRenderQualityResult<MinecraftSpatialFrame> {
  let frame_path = PathBuf::from(frame_json_path);
  if let Ok(payload) = read_json_file::<ScenePacketFramePayload>(&frame_path, "MC-7 scene packet frame") {
    return Ok(payload.spatial_frame);
  }
  read_json_file::<MinecraftSpatialFrame>(&frame_path, "MC-7 scene packet frame")
}

fn image_size_from_rgb(image: &RgbImage) -> HoldoutRenderQualityImageSize {
  HoldoutRenderQualityImageSize {
    width: image.width(),
    height: image.height(),
  }
}

fn compute_rgb8_metrics(source: &RgbImage, rendered: &RgbImage) -> HoldoutRenderQualityMetrics {
  let pixel_count = source.width() as u64 * source.height() as u64;
  let channel_count = pixel_count * 3;
  let mut sum_abs = 0.0_f64;
  let mut sum_sq = 0.0_f64;

  for (source_pixel, rendered_pixel) in source.pixels().zip(rendered.pixels()) {
    for (source_channel, rendered_channel) in source_pixel.0.iter().zip(rendered_pixel.0.iter()) {
      let diff = f64::from(*source_channel) - f64::from(*rendered_channel);
      sum_abs += diff.abs();
      sum_sq += diff * diff;
    }
  }

  let l1_mean = sum_abs / channel_count as f64;
  let mse = sum_sq / channel_count as f64;
  let psnr = if mse > 0.0 {
    Some(10.0 * ((255.0 * 255.0) / mse).log10())
  } else {
    None
  };

  HoldoutRenderQualityMetrics {
    l1_mean: Some(l1_mean),
    mse: Some(mse),
    psnr,
    ssim: None,
  }
}

fn read_json_file<T: DeserializeOwned>(path: &Path, label: &str) -> TrainingResultHoldoutRenderQualityResult<T> {
  read_json_file_helper(path).map_err(|error| match error {
    JsonFileReadError::Open(error) => {
      format!("failed to open {label} {}: {error}", path.display())
    }
    JsonFileReadError::Parse(error) => {
      format!("failed to parse {label} {}: {error}", path.display())
    }
  })
}

fn write_json_file<T: Serialize>(path: &Path, value: &T, label: &str) -> TrainingResultHoldoutRenderQualityResult<()> {
  write_json_file_helper(path, value, JsonWriteOptions::default()).map_err(|error| match error {
    JsonFileWriteError::CreateParent(error) | JsonFileWriteError::Write(error) => {
      format!("failed to write {label} {}: {error}", path.display())
    }
    JsonFileWriteError::Serialize(error) => format!("failed to serialize {label}: {error}"),
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::scene_packet::{ScenePacketCounts, ScenePacketFrameRecord, ScenePacketManifest};
  use crate::training_result::TrainingResultStatus;
  use crate::training_result_holdout_preview::{
    TRAINING_RESULT_HOLDOUT_PREVIEW_MANIFEST_SCHEMA_VERSION, TrainingResultHoldoutPreviewManifest,
  };
  use crate::training_result_semantic::TrainingResultSemanticManifest;
  use crate::types::{BlockFace, PlayerPose, RaycastHit, Vec3};
  use image::{ImageBuffer, Rgb};
  use tempfile::TempDir;

  fn identity_matrix() -> [f64; 16] {
    [
      1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
  }

  fn test_frame(view_matrix: [f64; 16], projection_matrix: [f64; 16]) -> MinecraftSpatialFrame {
    MinecraftSpatialFrame {
      spatial_frame_id: "frame-1".to_string(),
      world_tick: 1,
      monotonic_timestamp_ms: 100,
      telemetry_session_id: None,
      viewport: Viewport::new(4, 4),
      view_matrix,
      projection_matrix,
      player_pose: PlayerPose {
        eye_position: Vec3::new(0.0, 0.0, 0.0),
        yaw: 0.0,
        pitch: 0.0,
      },
      raycast_hit: Some(RaycastHit {
        block_pos: crate::types::BlockPosition::new(0, 0, 0),
        face: BlockFace::North,
        block_id: "minecraft:stone".to_string(),
      }),
      nearby_blocks: Vec::new(),
      nearby_entities: Vec::new(),
      inventory_summary: Vec::new(),
      screenshot_artifact_ref: None,
      mc_capture_skew_ms: None,
      screen_state: Some("in_game".to_string()),
      resource_pack_ids: Vec::new(),
    }
  }

  fn write_png(path: &Path, width: u32, height: u32, fill: u8) {
    let image: RgbImage = ImageBuffer::from_fn(width, height, |_x, _y| Rgb([fill, fill.saturating_add(1), 128]));
    image.save(path).expect("save png");
  }

  fn write_semantic_manifest(temp: &TempDir, scene_packet_manifest_path: &Path, normalized_dir: &Path, config_exists: bool) -> PathBuf {
    let models_dir = normalized_dir.join("nerfstudio_models");
    fs::create_dir_all(&models_dir).expect("models dir");
    let config_path = normalized_dir.join("config.yml");
    if config_exists {
      fs::write(&config_path, "trainer: nerfstudio.splatfacto\n").expect("config");
    }

    let manifest = TrainingResultSemanticManifest {
      schema_version: 1,
      generated_at_millis: 1,
      source_training_result_artifact_manifest_path: "/tmp/d11.json".to_string(),
      source_training_result_manifest_path: "/tmp/result.json".to_string(),
      source_training_job_manifest_path: "/tmp/job.json".to_string(),
      source_training_launch_plan_path: "/tmp/launch.json".to_string(),
      source_training_package_manifest_path: "/tmp/package.json".to_string(),
      source_scene_packet_manifest_path: scene_packet_manifest_path.to_string_lossy().into_owned(),
      source_bundle_manifest_paths: vec!["/tmp/bundle.json".to_string()],
      source_run_ids: vec!["run-1".to_string()],
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      source_result_status: TrainingResultStatus::Succeeded,
      normalized_result_dir: normalized_dir.to_string_lossy().into_owned(),
      semantic_status: StageStatus::Ready,
      semantic_reason: None,
      config_path: config_path.to_string_lossy().into_owned(),
      models_dir_path: models_dir.to_string_lossy().into_owned(),
      status_snapshot_path: None,
      config_trainer: Some("nerfstudio.splatfacto".to_string()),
      checkpoint_files: Vec::new(),
      checkpoint_count: 0,
      known_limits: vec!["fixture".to_string()],
    };
    let path = temp.path().join("semantic.json");
    write_json_file(&path, &manifest, "semantic fixture").expect("semantic fixture");
    path
  }

  struct HoldoutFixture {
    semantic_manifest_path: PathBuf,
    holdout_preview_manifest_path: PathBuf,
    output_dir: PathBuf,
  }

  fn write_holdout_fixture(
    temp: &TempDir,
    holdout_status: StageStatus,
    include_checkpoint: bool,
    include_config: bool,
    include_screenshot: bool,
    semantic_lineage_suffix: Option<&str>,
  ) -> HoldoutFixture {
    let normalized_dir = temp.path().join("normalized");
    let scene_packet_dir = temp.path().join("scene-packet");
    fs::create_dir_all(scene_packet_dir.join("frames")).expect("frames dir");

    let frame = test_frame(identity_matrix(), identity_matrix());
    let frame_json_name = "frames/frame_000001.json";
    let frame_json_path = scene_packet_dir.join(frame_json_name);
    write_json_file(&frame_json_path, &frame, "frame fixture").expect("frame fixture");

    let screenshot_rel = "frames/frame_000001.png";
    let holdout_screenshot_path = scene_packet_dir.join(screenshot_rel);
    if include_screenshot {
      write_png(&holdout_screenshot_path, 4, 4, 10);
    }

    let scene_packet_manifest = ScenePacketManifest {
      schema_version: 1,
      generated_at_millis: 1,
      source_bundle_manifest_paths: vec!["/tmp/bundle.json".to_string()],
      source_run_ids: vec!["run-1".to_string()],
      counts: ScenePacketCounts {
        frames: 1,
        screenshots: 1,
        missing_screenshots: 0,
      },
      frames: vec![ScenePacketFrameRecord {
        frame_index: 1,
        spatial_frame_id: frame.spatial_frame_id.clone(),
        source_run_id: "run-1".to_string(),
        source_bundle_manifest_path: "/tmp/bundle.json".to_string(),
        source_frame_artifact_id: "artifact_0001".to_string(),
        source_frame_bundle_path: "spatial_frames/frame.json".to_string(),
        frame_json_path: frame_json_name.to_string(),
        screenshot_artifact_id: Some("shot_0001".to_string()),
        screenshot_path: Some(screenshot_rel.to_string()),
        monotonic_timestamp_ms: 100,
        viewport: frame.viewport,
        screen_state: frame.screen_state.clone(),
        resource_pack_ids: Vec::new(),
      }],
      known_limits: Vec::new(),
    };
    let scene_packet_manifest_path = scene_packet_dir.join("scene-packet.json");
    write_json_file(&scene_packet_manifest_path, &scene_packet_manifest, "scene packet fixture").expect("scene packet");

    let semantic_manifest_path = write_semantic_manifest(temp, &scene_packet_manifest_path, &normalized_dir, include_config);

    let checkpoint_path = normalized_dir.join("nerfstudio_models/step-000001.ckpt");
    if include_checkpoint {
      fs::create_dir_all(checkpoint_path.parent().expect("parent")).expect("models dir");
      fs::write(&checkpoint_path, b"fixture-checkpoint").expect("checkpoint");
    }

    let holdout_frame = HoldoutFrameWitness {
      frame_index: 1,
      spatial_frame_id: frame.spatial_frame_id.clone(),
      screenshot_path: holdout_screenshot_path.to_string_lossy().into_owned(),
      frame_json_path: frame_json_path.to_string_lossy().into_owned(),
    };

    let mut trainer_backend = "nerfstudio.splatfacto".to_string();
    if let Some(suffix) = semantic_lineage_suffix {
      trainer_backend.push_str(suffix);
    }

    let holdout_preview_manifest = TrainingResultHoldoutPreviewManifest {
      schema_version: TRAINING_RESULT_HOLDOUT_PREVIEW_MANIFEST_SCHEMA_VERSION,
      generated_at_millis: 1,
      training_result_semantic_manifest_path: semantic_manifest_path.to_string_lossy().into_owned(),
      source_training_result_artifact_manifest_path: "/tmp/d11.json".to_string(),
      source_training_result_manifest_path: "/tmp/result.json".to_string(),
      source_training_job_manifest_path: "/tmp/job.json".to_string(),
      source_training_launch_plan_path: "/tmp/launch.json".to_string(),
      source_training_package_manifest_path: "/tmp/package.json".to_string(),
      source_scene_packet_manifest_path: scene_packet_manifest_path.to_string_lossy().into_owned(),
      source_bundle_manifest_paths: vec!["/tmp/bundle.json".to_string()],
      source_run_ids: vec!["run-1".to_string()],
      trainer_backend,
      job_backend: "remote".to_string(),
      normalized_result_dir: normalized_dir.to_string_lossy().into_owned(),
      holdout_frame_index: 1,
      holdout_frame: if holdout_status == StageStatus::Ready {
        Some(holdout_frame.clone())
      } else {
        None
      },
      basis_checkpoint_path: if holdout_status == StageStatus::Ready {
        Some(checkpoint_path.to_string_lossy().into_owned())
      } else {
        None
      },
      holdout_screenshot_path: if holdout_status == StageStatus::Ready && include_screenshot {
        Some(holdout_screenshot_path.to_string_lossy().into_owned())
      } else {
        None
      },
      reference_overlay_path: None,
      status: holdout_status,
      reason: None,
      known_limits: vec!["fixture".to_string()],
    };

    let holdout_preview_manifest_path = temp.path().join("holdout-preview.json");
    write_json_file(&holdout_preview_manifest_path, &holdout_preview_manifest, "holdout preview fixture").expect("holdout preview");

    HoldoutFixture {
      semantic_manifest_path,
      holdout_preview_manifest_path,
      output_dir: temp.path().join("quality-output"),
    }
  }

  const COPY_RENDER_COMMAND: &str = r#"python3 -c '
import json, shutil, sys
req = json.load(sys.stdin)
dest = req["requested_rendered_image_path"]
shutil.copy(req["holdout_screenshot_path"], dest)
print(json.dumps({"status": "ready", "rendered_image_path": dest}))
'"#;

  fn mismatch_render_command(mismatch_png: &Path) -> String {
    format!(
      r#"python3 -c '
import json, shutil, sys
req = json.load(sys.stdin)
dest = req["requested_rendered_image_path"]
shutil.copy("{path}", dest)
print(json.dumps({{"status": "ready", "rendered_image_path": dest}}))
'"#,
      path = mismatch_png.display()
    )
  }

  #[test]
  fn blocked_invalid_mc16_manifest_records_quality_evidence() {
    let temp = TempDir::new().expect("tempdir");
    let fixture = write_holdout_fixture(&temp, StageStatus::Ready, true, true, true, None);

    let output = measure_3dgs_holdout_render_quality(TrainingResultHoldoutRenderQualityInputs {
      training_result_semantic_manifest_path: fixture.semantic_manifest_path,
      holdout_preview_manifest_path: temp.path().join("missing-holdout.json"),
      render_command: COPY_RENDER_COMMAND.to_string(),
      output_dir: fixture.output_dir.clone(),
    })
    .expect("blocked invalid mc16");

    assert_eq!(output.manifest.status, StageStatus::Blocked);
    assert_eq!(output.manifest.reason, Some(HoldoutRenderQualityReason::HoldoutPreviewUnreadable));
    assert_eq!(output.manifest.verdict, HoldoutRenderQualityVerdict::Blocked);
    assert!(output.manifest_path.is_file());
    assert!(output.inspect_report_path.is_file());
  }

  #[test]
  fn blocked_lineage_mismatch_records_quality_evidence() {
    let temp = TempDir::new().expect("tempdir");
    let fixture = write_holdout_fixture(&temp, StageStatus::Ready, true, true, true, Some("-mismatch"));

    let output = measure_3dgs_holdout_render_quality(TrainingResultHoldoutRenderQualityInputs {
      training_result_semantic_manifest_path: fixture.semantic_manifest_path,
      holdout_preview_manifest_path: fixture.holdout_preview_manifest_path,
      render_command: COPY_RENDER_COMMAND.to_string(),
      output_dir: fixture.output_dir,
    })
    .expect("blocked lineage mismatch");

    assert_eq!(output.manifest.status, StageStatus::Blocked);
    assert_eq!(output.manifest.reason, Some(HoldoutRenderQualityReason::LineageMismatch));
  }

  #[test]
  fn blocked_missing_checkpoint_records_quality_evidence() {
    let temp = TempDir::new().expect("tempdir");
    let fixture = write_holdout_fixture(&temp, StageStatus::Ready, false, true, true, None);

    let output = measure_3dgs_holdout_render_quality(TrainingResultHoldoutRenderQualityInputs {
      training_result_semantic_manifest_path: fixture.semantic_manifest_path,
      holdout_preview_manifest_path: fixture.holdout_preview_manifest_path,
      render_command: COPY_RENDER_COMMAND.to_string(),
      output_dir: fixture.output_dir,
    })
    .expect("blocked missing checkpoint");

    assert_eq!(output.manifest.status, StageStatus::Blocked);
    assert_eq!(output.manifest.reason, Some(HoldoutRenderQualityReason::CheckpointMissing));
  }

  #[test]
  fn failed_render_command_nonzero_records_quality_evidence() {
    let temp = TempDir::new().expect("tempdir");
    let fixture = write_holdout_fixture(&temp, StageStatus::Ready, true, true, true, None);

    let output = measure_3dgs_holdout_render_quality(TrainingResultHoldoutRenderQualityInputs {
      training_result_semantic_manifest_path: fixture.semantic_manifest_path,
      holdout_preview_manifest_path: fixture.holdout_preview_manifest_path,
      render_command: "exit 17".to_string(),
      output_dir: fixture.output_dir,
    })
    .expect("failed render command");

    assert_eq!(output.manifest.status, StageStatus::Failed);
    assert_eq!(output.manifest.reason, Some(HoldoutRenderQualityReason::HoldoutRenderCommandFailed));
    assert_eq!(output.manifest.verdict, HoldoutRenderQualityVerdict::Failed);
  }

  #[test]
  fn failed_render_bad_stdout_records_quality_evidence() {
    let temp = TempDir::new().expect("tempdir");
    let fixture = write_holdout_fixture(&temp, StageStatus::Ready, true, true, true, None);

    let output = measure_3dgs_holdout_render_quality(TrainingResultHoldoutRenderQualityInputs {
      training_result_semantic_manifest_path: fixture.semantic_manifest_path,
      holdout_preview_manifest_path: fixture.holdout_preview_manifest_path,
      render_command: "printf 'not-json'".to_string(),
      output_dir: fixture.output_dir,
    })
    .expect("failed bad stdout");

    assert_eq!(output.manifest.status, StageStatus::Failed);
    assert_eq!(output.manifest.reason, Some(HoldoutRenderQualityReason::HoldoutRenderCommandFailed));
  }

  #[test]
  fn metric_partial_on_dimension_mismatch_records_quality_evidence() {
    let temp = TempDir::new().expect("tempdir");
    let fixture = write_holdout_fixture(&temp, StageStatus::Ready, true, true, true, None);
    let mismatch_png = temp.path().join("mismatch_8x8.png");
    write_png(&mismatch_png, 8, 8, 1);

    let output = measure_3dgs_holdout_render_quality(TrainingResultHoldoutRenderQualityInputs {
      training_result_semantic_manifest_path: fixture.semantic_manifest_path,
      holdout_preview_manifest_path: fixture.holdout_preview_manifest_path,
      render_command: mismatch_render_command(&mismatch_png),
      output_dir: fixture.output_dir,
    })
    .expect("metric partial");

    assert_eq!(output.manifest.status, StageStatus::Ready);
    assert_eq!(output.manifest.verdict, HoldoutRenderQualityVerdict::MetricPartial);
    assert!(!output.manifest.image_size_match);
    assert!(output.manifest.metrics.is_none());
    assert!(output.manifest.rendered_image_path.as_deref().is_some_and(|path| Path::new(path).is_file()));
  }

  #[test]
  fn measured_only_on_dimension_match_populates_metrics() {
    let temp = TempDir::new().expect("tempdir");
    let fixture = write_holdout_fixture(&temp, StageStatus::Ready, true, true, true, None);

    let output = measure_3dgs_holdout_render_quality(TrainingResultHoldoutRenderQualityInputs {
      training_result_semantic_manifest_path: fixture.semantic_manifest_path,
      holdout_preview_manifest_path: fixture.holdout_preview_manifest_path,
      render_command: COPY_RENDER_COMMAND.to_string(),
      output_dir: fixture.output_dir,
    })
    .expect("measured only");

    assert_eq!(output.manifest.status, StageStatus::Ready);
    assert_eq!(output.manifest.verdict, HoldoutRenderQualityVerdict::MeasuredOnly);
    assert!(output.manifest.image_size_match);
    let metrics = output.manifest.metrics.expect("metrics");
    assert_eq!(metrics.l1_mean, Some(0.0));
    assert_eq!(metrics.mse, Some(0.0));
    assert!(metrics.psnr.is_none());
    assert!(metrics.ssim.is_none());
    assert!(output.inspect_report.l1_mean_available);
    assert!(!output.inspect_report.ssim_available);
  }

  #[test]
  fn blocked_mc16_not_ready_skips_render_command() {
    let temp = TempDir::new().expect("tempdir");
    let fixture = write_holdout_fixture(&temp, StageStatus::Blocked, true, true, true, None);

    let output = measure_3dgs_holdout_render_quality(TrainingResultHoldoutRenderQualityInputs {
      training_result_semantic_manifest_path: fixture.semantic_manifest_path,
      holdout_preview_manifest_path: fixture.holdout_preview_manifest_path,
      render_command: "echo 'render should not run' && exit 9".to_string(),
      output_dir: fixture.output_dir,
    })
    .expect("blocked mc16");

    assert_eq!(output.manifest.status, StageStatus::Blocked);
    assert_eq!(output.manifest.reason, Some(HoldoutRenderQualityReason::HoldoutPreviewNotReady));
    assert!(output.manifest.rendered_image_path.is_none());
  }

  #[test]
  fn lineage_matches_semantic_compares_manifest_path_and_business_keys() {
    let temp = TempDir::new().expect("tempdir");
    let fixture = write_holdout_fixture(&temp, StageStatus::Ready, true, true, true, None);
    let semantic = read_json_file::<TrainingResultSemanticManifest>(&fixture.semantic_manifest_path, "semantic").expect("semantic");
    let holdout =
      read_json_file::<TrainingResultHoldoutPreviewManifest>(&fixture.holdout_preview_manifest_path, "holdout").expect("holdout");

    assert!(lineage_matches_semantic(&fixture.semantic_manifest_path, &semantic, &holdout));
  }

  #[test]
  fn persisted_artifacts_do_not_contain_render_command_text() {
    let temp = TempDir::new().expect("tempdir");
    let fixture = write_holdout_fixture(&temp, StageStatus::Ready, true, true, true, None);
    let secret = "super-secret-render-command-token-abc123";
    let render_command = format!("printf '{{\"status\":\"blocked\"}}' # {secret}");

    let output = measure_3dgs_holdout_render_quality(TrainingResultHoldoutRenderQualityInputs {
      training_result_semantic_manifest_path: fixture.semantic_manifest_path,
      holdout_preview_manifest_path: fixture.holdout_preview_manifest_path,
      render_command,
      output_dir: fixture.output_dir,
    })
    .expect("quality evidence");

    let manifest_text = fs::read_to_string(&output.manifest_path).expect("manifest bytes");
    let inspect_text = fs::read_to_string(&output.inspect_report_path).expect("inspect bytes");
    assert!(!manifest_text.contains(secret));
    assert!(!inspect_text.contains(secret));
    assert_eq!(output.manifest.render_backend, HoldoutRenderQualityBackend::ExternalCommand);
    assert!(manifest_text.contains("\"render_backend\": \"external_command\""));
  }
}
