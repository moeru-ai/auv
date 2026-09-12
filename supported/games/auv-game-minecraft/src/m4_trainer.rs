//! M4 Brush/OpenSplat trainer packet and result recording.
//!
//! Builds a reconstruction-oriented multi-view input packet from a gated M2
//! session. Engine pose and camera matrices belong on this trainer path only;
//! they must not be copied onto M1/M3 black-box VLM request JSON.
//!
//! TODO(m4-live-trainer): real Brush/OpenSplat execution stays outside this crate
//! until a sibling records artifacts under `.tmp/m4-session/`.
//! TODO(m4-occlusion-holdout): occlusion-specific holdout scoring stays deferred;
//! holdout render metrics use photometric MC-17 evidence only.
//! TODO(m4-clock-domain): JVM telemetry and AUV capture clocks are not calibrated
//! across domains; `capture_skew_ms` is recorded, not wall-clock alignment proof.
//! NOTICE(m4-vlm-boundary): crate内无 VLM transport; M4 不声称 VLM 几何命中率。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use auv_file::{JsonWriteOptions, write_json_file};
use serde::{Deserialize, Serialize};

use crate::m2_multi_view::{M2RelativeMotion, M2Session, M2ViewRole, M2ViewSample};
use crate::m3_query_scoring::M3QueryScoreReport;
use crate::training_launch::TrainingBackend;
use crate::training_result_holdout_render_quality::HoldoutRenderQualityMetrics;
use crate::types::{PlayerPose, Viewport};

pub const M4_TRAINER_PACKET_SCHEMA_VERSION: u32 = 1;
pub const M4_TRAINER_RESULT_REPORT_SCHEMA_VERSION: u32 = 1;

/// Recommended root for live M4 trainer runs (not on C:).
pub const M4_LIVE_SESSION_ROOT: &str = r"F:\auv\.tmp\m4-session";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M4TrainerCommandRecord {
  pub trainer_backend: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub trainer_version: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub backend_contract_revision: Option<String>,
  pub launch_command: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum M4SeedPointCloudSource {
  None,
  RaycastHits,
  External,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M4SeedPointCloudProvenance {
  pub used: bool,
  pub source: M4SeedPointCloudSource,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub relative_path: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub transforms_relative_path: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub point_count: Option<usize>,
}

/// One trainer-bound view with reconstruction cameras. Unlike M1/M3 black-box
/// observations, this record may include engine pose and projection matrices.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M4TrainerViewRecord {
  pub observation_id: String,
  pub role: M2ViewRole,
  pub screenshot_artifact_ref: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub local_screenshot_path: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub capture_monotonic_timestamp_ms: Option<u64>,
  pub capture_skew_ms: i64,
  pub viewport: Viewport,
  pub view_matrix: [f64; 16],
  pub projection_matrix: [f64; 16],
  pub player_pose: PlayerPose,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M4TrainerInputPacket {
  pub schema_version: u32,
  pub session_id: String,
  pub meets_m2_capture_gate: bool,
  pub views: Vec<M4TrainerViewRecord>,
  pub relative_motions: Vec<M2RelativeMotion>,
  pub trainer_command: M4TrainerCommandRecord,
  pub seed_point_cloud: M4SeedPointCloudProvenance,
  pub holdout_observation_ids: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M4OutputArtifactLineage {
  pub normalized_result_dir: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub checkpoint_path: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub splat_path: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub source_training_launch_plan_path: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub source_training_package_manifest_path: Option<String>,
  pub artifacts_present: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M4HoldoutRenderMetricRecord {
  pub holdout_observation_id: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub holdout_frame_index: Option<usize>,
  pub image_size_match: bool,
  pub metrics: HoldoutRenderQualityMetrics,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M4HoldoutSpatialQueryMetricRecord {
  pub holdout_observation_id: String,
  pub session_id: String,
  pub anchor_observation_id: String,
  pub query_observation_id: String,
  pub meets_m3_query_gate: bool,
  pub visibility_class_correct: bool,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub projection_pixel_error_px: Option<f64>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub projection_within_tolerance: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M4TrainerResultReport {
  pub schema_version: u32,
  pub input_packet: M4TrainerInputPacket,
  pub output_lineage: M4OutputArtifactLineage,
  pub holdout_render_metrics: Vec<M4HoldoutRenderMetricRecord>,
  pub holdout_spatial_query_metrics: Vec<M4HoldoutSpatialQueryMetricRecord>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub trainer_exit_status: Option<i32>,
  pub black_box_leaks: Vec<String>,
  pub gate_failures: Vec<String>,
  pub meets_m4_trainer_gate: bool,
}

/// Optional local screenshot path per M2 observation id for live artifact export.
#[derive(Clone, Debug, PartialEq)]
pub struct M4TrainerPacketBuildInput {
  pub session: M2Session,
  pub trainer_backend: TrainingBackend,
  pub launch_command: String,
  pub trainer_version: Option<String>,
  pub backend_contract_revision: Option<String>,
  pub seed_point_cloud: M4SeedPointCloudProvenance,
  pub holdout_view_roles: Vec<M2ViewRole>,
  pub local_screenshot_paths: BTreeMap<String, PathBuf>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct M4TrainerResultRecordInput {
  pub packet: M4TrainerInputPacket,
  /// When set, leak detection verifies trainer-only pose fields did not appear on
  /// the session's M1 black-box request/observation JSON.
  pub source_session: Option<M2Session>,
  pub output_lineage: M4OutputArtifactLineage,
  pub holdout_render_metrics: Vec<M4HoldoutRenderMetricRecord>,
  pub holdout_spatial_query_metrics: Vec<M4HoldoutSpatialQueryMetricRecord>,
  pub trainer_exit_status: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum M4TrainerError {
  UngatedM2Session { gate_failures: Vec<String> },
  MissingTrainerCommand,
  EmptyHoldoutSelection,
}

impl std::fmt::Display for M4TrainerError {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::UngatedM2Session { gate_failures } => {
        write!(formatter, "M4 trainer packet requires a gated M2 session; failures: {}", gate_failures.join("; "))
      }
      Self::MissingTrainerCommand => formatter.write_str("M4 trainer packet requires a non-empty launch command and backend"),
      Self::EmptyHoldoutSelection => formatter.write_str("M4 trainer packet requires at least one holdout view role"),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum M4TrainerPersistenceError {
  Persistence(String),
}

impl std::fmt::Display for M4TrainerPersistenceError {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Persistence(message) => write!(formatter, "M4 trainer report persistence failed: {message}"),
    }
  }
}

/// Build a trainer input packet from a gated M2 session without executing a trainer.
pub fn build_m4_trainer_packet_from_session(input: &M4TrainerPacketBuildInput) -> Result<M4TrainerInputPacket, M4TrainerError> {
  if !input.session.meets_m2_capture_gate {
    return Err(M4TrainerError::UngatedM2Session {
      gate_failures: input.session.gate_failures.clone(),
    });
  }
  if input.launch_command.trim().is_empty() {
    return Err(M4TrainerError::MissingTrainerCommand);
  }
  if input.holdout_view_roles.is_empty() {
    return Err(M4TrainerError::EmptyHoldoutSelection);
  }

  let withheld = crate::m2_multi_view::m2_session_withheld_truth(&input.session);
  let views = input
    .session
    .views
    .iter()
    .zip(withheld.iter())
    .map(|(sample, truth)| trainer_view_from_sample(sample, truth, &input.local_screenshot_paths))
    .collect::<Vec<_>>();

  let holdout_observation_ids = input
    .holdout_view_roles
    .iter()
    .map(|role| input.session.views.iter().find(|view| view.role == *role).map(|view| view.observation_id.clone()).ok_or(*role))
    .collect::<Result<Vec<_>, M2ViewRole>>()
    .map_err(|_| M4TrainerError::EmptyHoldoutSelection)?;

  let known_limits = default_packet_known_limits(&input.session);

  Ok(M4TrainerInputPacket {
    schema_version: M4_TRAINER_PACKET_SCHEMA_VERSION,
    session_id: input.session.session_id.clone(),
    meets_m2_capture_gate: true,
    views,
    relative_motions: input.session.relative_motions.clone(),
    trainer_command: M4TrainerCommandRecord {
      trainer_backend: input.trainer_backend.manifest_name().to_string(),
      trainer_version: input.trainer_version.clone(),
      backend_contract_revision: input.backend_contract_revision.clone(),
      launch_command: input.launch_command.clone(),
    },
    seed_point_cloud: input.seed_point_cloud.clone(),
    holdout_observation_ids,
    known_limits,
  })
}

/// Record a trainer result from an input packet and externally produced artifacts.
///
/// This slice does not execute Brush/OpenSplat; callers supply lineage and metrics.
pub fn record_m4_trainer_result(input: M4TrainerResultRecordInput) -> M4TrainerResultReport {
  let black_box_leaks = input.source_session.as_ref().map(detect_m4_session_black_box_boundary_violations).unwrap_or_default();
  let gate_failures = evaluate_m4_trainer_gate_failures(&input, &black_box_leaks);
  let meets_m4_trainer_gate = gate_failures.is_empty();

  M4TrainerResultReport {
    schema_version: M4_TRAINER_RESULT_REPORT_SCHEMA_VERSION,
    input_packet: input.packet,
    output_lineage: input.output_lineage,
    holdout_render_metrics: input.holdout_render_metrics,
    holdout_spatial_query_metrics: input.holdout_spatial_query_metrics,
    trainer_exit_status: input.trainer_exit_status,
    black_box_leaks,
    gate_failures,
    meets_m4_trainer_gate,
  }
}

/// True when the result report satisfies the M4 trainer sample gate for this slice.
///
/// This measures capture packet honesty, trainer command/lineage recording, and
/// holdout photometric + spatial-query evidence — not VLM geometric hit-rate.
pub fn meets_m4_trainer_gate(report: &M4TrainerResultReport) -> bool {
  report.meets_m4_trainer_gate
}

pub fn write_m4_trainer_result_report(path: &Path, report: &M4TrainerResultReport) -> Result<(), M4TrainerPersistenceError> {
  write_json_file(
    path,
    report,
    JsonWriteOptions {
      create_parent_dirs: true,
      trailing_newline: true,
    },
  )
  .map_err(|error| M4TrainerPersistenceError::Persistence(format!("{error:?}")))
}

/// Convert an M3 holdout query score into the M4 metric record shape.
pub fn m4_holdout_spatial_query_metric_from_m3(score: &M3QueryScoreReport) -> M4HoldoutSpatialQueryMetricRecord {
  M4HoldoutSpatialQueryMetricRecord {
    holdout_observation_id: score.query_observation_id.clone(),
    session_id: score.session_id.clone(),
    anchor_observation_id: score.anchor_observation_id.clone(),
    query_observation_id: score.query_observation_id.clone(),
    meets_m3_query_gate: score.meets_m3_query_gate,
    visibility_class_correct: score.visibility_class_correct,
    projection_pixel_error_px: score.projection_pixel_error_px,
    projection_within_tolerance: score.projection_within_tolerance,
  }
}

fn trainer_view_from_sample(
  sample: &M2ViewSample,
  truth: &crate::m1_black_box_scoring::M1WithheldMinecraftTruth,
  local_paths: &BTreeMap<String, PathBuf>,
) -> M4TrainerViewRecord {
  let frame = truth.frame();
  M4TrainerViewRecord {
    observation_id: sample.observation_id.clone(),
    role: sample.role,
    screenshot_artifact_ref: sample.screenshot_artifact_ref.clone(),
    local_screenshot_path: local_paths.get(&sample.observation_id).map(|path| path.display().to_string()),
    capture_monotonic_timestamp_ms: sample.capture_monotonic_timestamp_ms,
    capture_skew_ms: sample.capture_skew_ms,
    viewport: frame.viewport,
    view_matrix: frame.view_matrix,
    projection_matrix: frame.projection_matrix,
    player_pose: frame.player_pose,
  }
}

fn default_packet_known_limits(session: &M2Session) -> Vec<String> {
  let mut limits = vec![
    "M4 trainer packet includes engine pose and camera matrices for reconstruction only; do not copy onto M1/M3 black-box VLM requests".to_string(),
    "M4 does not execute Brush/OpenSplat inside auv-game-minecraft; live trainer runs are recorded by an external sibling".to_string(),
    "Holdout render metrics are photometric evidence (l1_mean/mse/psnr/ssim); they are not pixel/block hit-rate or VLM geometric accuracy claims".to_string(),
  ];
  if session.capture_skew_used_sidecar_fallback {
    limits.push(
      "capture_skew_used_sidecar_fallback=true: JVM telemetry and AUV capture clocks differ; skew 0 does not prove temporal alignment"
        .to_string(),
    );
  }
  limits
}

fn evaluate_m4_trainer_gate_failures(input: &M4TrainerResultRecordInput, black_box_leaks: &[String]) -> Vec<String> {
  let mut failures = Vec::new();

  if !input.packet.meets_m2_capture_gate {
    failures.push("m2_capture_gate=false".to_string());
  }
  if input.packet.trainer_command.trainer_backend.trim().is_empty() || input.packet.trainer_command.launch_command.trim().is_empty() {
    failures.push("missing_trainer_command_or_backend".to_string());
  }
  if !input.output_lineage.artifacts_present {
    failures.push("output_artifact_lineage_missing".to_string());
  }
  if input.output_lineage.normalized_result_dir.trim().is_empty() {
    failures.push("normalized_result_dir_missing".to_string());
  }
  if input.holdout_render_metrics.is_empty() {
    failures.push("holdout_render_metrics_missing".to_string());
  } else if !input.holdout_render_metrics.iter().any(|metric| metric.metrics.l1_mean.is_some() || metric.metrics.mse.is_some()) {
    failures.push("holdout_render_photometric_metrics_missing".to_string());
  }
  if input.holdout_spatial_query_metrics.is_empty() {
    failures.push("holdout_spatial_query_metrics_missing".to_string());
  }
  for holdout_id in &input.packet.holdout_observation_ids {
    if !input.holdout_render_metrics.iter().any(|metric| metric.holdout_observation_id == *holdout_id) {
      failures.push(format!("holdout_render_metric_missing_for:{holdout_id}"));
    }
    if !input.holdout_spatial_query_metrics.iter().any(|metric| metric.holdout_observation_id == *holdout_id) {
      failures.push(format!("holdout_spatial_query_metric_missing_for:{holdout_id}"));
    }
  }
  if !black_box_leaks.is_empty() {
    failures.push(format!("black_box_leaks:{}", black_box_leaks.join(",")));
  }

  failures
}

/// Verify M1/M3 black-box artifacts from the source session were not mutated
/// to include trainer-only engine pose fields.
pub fn detect_m4_session_black_box_boundary_violations(session: &M2Session) -> Vec<String> {
  let mut violations = Vec::new();
  for request in &session.requests {
    let Ok(json) = serde_json::to_string(request) else {
      violations.push("request_serialization_failed".to_string());
      continue;
    };
    for marker in [
      "view_matrix",
      "projection_matrix",
      "player_pose",
      "eye_position",
      "nearby_blocks",
    ] {
      if json.contains(marker) {
        violations.push(format!("m1_request_contains:{marker}"));
      }
    }
  }
  for observation in &session.observations {
    let Ok(json) = serde_json::to_string(observation) else {
      violations.push("observation_serialization_failed".to_string());
      continue;
    };
    for marker in [
      "view_matrix",
      "projection_matrix",
      "player_pose",
      "eye_position",
      "nearby_blocks",
    ] {
      if json.contains(marker) {
        violations.push(format!("m1_observation_contains:{marker}"));
      }
    }
  }
  violations.sort();
  violations.dedup();
  violations
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::m2_multi_view::{M2ViewCaptureInput, build_m2_session_from_captures};
  use crate::m3_query_scoring::{M3HoldoutVisibility, M3QueryScoreReport, M3QueryVisibilityAnswer};
  use crate::types::{BlockFace, BlockPosition, NearbyBlock, PlayerPose, RaycastHit, Vec3, Viewport};

  const SCREENSHOT_ANCHOR: &str = "auv://runs/run-m4/artifacts/view-anchor.png";
  const SCREENSHOT_TRANSLATE: &str = "auv://runs/run-m4/artifacts/view-translate.png";
  const SCREENSHOT_REVISIT: &str = "auv://runs/run-m4/artifacts/view-revisit.png";

  fn base_frame(spatial_frame_id: &str, eye: Vec3, yaw: f64, pitch: f64, timestamp_ms: u64) -> crate::types::MinecraftSpatialFrame {
    crate::types::MinecraftSpatialFrame {
      spatial_frame_id: spatial_frame_id.to_string(),
      world_tick: 1200,
      monotonic_timestamp_ms: timestamp_ms,
      telemetry_session_id: Some("session-m4".to_string()),
      viewport: Viewport::new(1280, 720),
      view_matrix: [3.0; 16],
      projection_matrix: [4.0; 16],
      player_pose: PlayerPose {
        eye_position: eye,
        yaw,
        pitch,
      },
      raycast_hit: Some(RaycastHit {
        block_pos: BlockPosition::new(520, 72, 730),
        face: BlockFace::North,
        block_id: "minecraft:red_wool".to_string(),
      }),
      nearby_blocks: vec![NearbyBlock {
        block_pos: BlockPosition::new(519, 72, 730),
        block_id: "minecraft:smooth_stone".to_string(),
      }],
      nearby_entities: Vec::new(),
      inventory_summary: Vec::new(),
      screenshot_artifact_ref: None,
      mc_capture_skew_ms: Some(15),
      screen_state: Some(crate::m1_black_box_observation::M1_IN_GAME_SCREEN_STATE.to_string()),
      resource_pack_ids: vec!["vanilla".to_string()],
    }
  }

  fn capture_input(
    frame: crate::types::MinecraftSpatialFrame,
    screenshot: &str,
    role: M2ViewRole,
    capture_ms: Option<u64>,
  ) -> M2ViewCaptureInput {
    M2ViewCaptureInput {
      telemetry_path: None,
      frame: Some(frame),
      screenshot_artifact_ref: screenshot.to_string(),
      capture_monotonic_timestamp_ms: capture_ms,
      role,
      input_history: Vec::new(),
    }
  }

  fn strafe_session() -> M2Session {
    let inputs = vec![
      capture_input(
        base_frame("frame-anchor", Vec3::new(10.0, 72.0, 20.0), 90.0, -10.0, 1_000),
        SCREENSHOT_ANCHOR,
        M2ViewRole::Anchor,
        Some(950),
      ),
      capture_input(
        base_frame("frame-translate", Vec3::new(11.0, 72.0, 20.0), 90.0, -10.0, 2_000),
        SCREENSHOT_TRANSLATE,
        M2ViewRole::Translate,
        Some(1_950),
      ),
      capture_input(
        base_frame("frame-revisit", Vec3::new(10.5, 72.0, 20.5), 95.0, -9.0, 3_000),
        SCREENSHOT_REVISIT,
        M2ViewRole::Revisit,
        Some(2_950),
      ),
    ];
    build_m2_session_from_captures(Some("m4-strafe".to_string()), &inputs).expect("strafe session")
  }

  fn yaw_only_session() -> M2Session {
    let eye = Vec3::new(10.0, 72.0, 20.0);
    let inputs = vec![
      capture_input(base_frame("frame-yaw-a", eye, 90.0, -10.0, 1_000), SCREENSHOT_ANCHOR, M2ViewRole::Anchor, Some(950)),
      capture_input(base_frame("frame-yaw-b", eye, 120.0, -10.0, 2_000), SCREENSHOT_TRANSLATE, M2ViewRole::Translate, Some(1_950)),
      capture_input(base_frame("frame-yaw-c", eye, 150.0, -5.0, 3_000), SCREENSHOT_REVISIT, M2ViewRole::Revisit, Some(2_950)),
    ];
    build_m2_session_from_captures(None, &inputs).expect("yaw-only session")
  }

  fn packet_build_input(session: M2Session) -> M4TrainerPacketBuildInput {
    let mut local_paths = BTreeMap::new();
    for view in &session.views {
      local_paths.insert(view.observation_id.clone(), PathBuf::from(format!(r"F:\auv\.tmp\m4-session\{}.png", view.observation_id)));
    }
    M4TrainerPacketBuildInput {
      session,
      trainer_backend: TrainingBackend::Brush,
      launch_command: "brush F:\\auv\\.tmp\\m4-session\\dataset --export-path F:\\auv\\.tmp\\m4-session\\trainer-output".to_string(),
      trainer_version: Some("0.3.0".to_string()),
      backend_contract_revision: Some("v0.3.0".to_string()),
      seed_point_cloud: M4SeedPointCloudProvenance {
        used: true,
        source: M4SeedPointCloudSource::RaycastHits,
        relative_path: Some("points3d.ply".to_string()),
        transforms_relative_path: Some("transforms.json".to_string()),
        point_count: Some(3),
      },
      holdout_view_roles: vec![M2ViewRole::Translate],
      local_screenshot_paths: local_paths,
    }
  }

  fn fake_result_input(packet: M4TrainerInputPacket, session: M2Session) -> M4TrainerResultRecordInput {
    let holdout_id = packet.holdout_observation_ids[0].clone();
    let query_observation_id = holdout_id.clone();
    M4TrainerResultRecordInput {
      packet,
      source_session: Some(session),
      output_lineage: M4OutputArtifactLineage {
        normalized_result_dir: r"F:\auv\.tmp\m4-session\trainer-output".to_string(),
        checkpoint_path: Some(r"F:\auv\.tmp\m4-session\trainer-output\splat_00030000.ply".to_string()),
        splat_path: Some(r"F:\auv\.tmp\m4-session\trainer-output\splat_00030000.ply".to_string()),
        source_training_launch_plan_path: Some(r"F:\auv\.tmp\m4-session\training-launch-plan.json".to_string()),
        source_training_package_manifest_path: Some(r"F:\auv\.tmp\m4-session\training-package-manifest.json".to_string()),
        artifacts_present: true,
      },
      holdout_render_metrics: vec![M4HoldoutRenderMetricRecord {
        holdout_observation_id: holdout_id.clone(),
        holdout_frame_index: Some(1),
        image_size_match: true,
        metrics: HoldoutRenderQualityMetrics {
          l1_mean: Some(0.04),
          mse: Some(0.002),
          psnr: Some(28.5),
          ssim: None,
        },
      }],
      holdout_spatial_query_metrics: vec![M4HoldoutSpatialQueryMetricRecord {
        holdout_observation_id: holdout_id,
        session_id: "m4-strafe".to_string(),
        anchor_observation_id: "obs-anchor".to_string(),
        query_observation_id,
        meets_m3_query_gate: true,
        visibility_class_correct: true,
        projection_pixel_error_px: Some(12.0),
        projection_within_tolerance: Some(true),
      }],
      trainer_exit_status: Some(0),
    }
  }

  #[test]
  fn m4_rejects_ungated_yaw_only_m2_session() {
    let error = build_m4_trainer_packet_from_session(&packet_build_input(yaw_only_session())).expect_err("yaw-only session");
    assert!(matches!(error, M4TrainerError::UngatedM2Session { .. }));
  }

  #[test]
  fn m4_trainer_packet_includes_camera_pose_not_in_m1_requests() {
    let session = strafe_session();
    let packet = build_m4_trainer_packet_from_session(&packet_build_input(session.clone())).expect("gated packet");

    assert!(packet.meets_m2_capture_gate);
    assert_eq!(packet.views.len(), 3);
    assert!(packet.views[0].view_matrix.iter().any(|value| *value == 3.0));
    assert!(packet.views[0].player_pose.eye_position.x == 10.0);

    let packet_json = serde_json::to_string(&packet).expect("serialize packet");
    assert!(packet_json.contains("view_matrix"));
    assert!(packet_json.contains("player_pose"));

    let boundary_violations = detect_m4_session_black_box_boundary_violations(&session);
    assert!(boundary_violations.is_empty(), "M1 path must stay black-box: {boundary_violations:?}");
  }

  #[test]
  fn m4_fake_trainer_result_passes_gate_with_fixture_artifacts() {
    let session = strafe_session();
    let packet = build_m4_trainer_packet_from_session(&packet_build_input(session.clone())).expect("packet");
    let report = record_m4_trainer_result(fake_result_input(packet, session));

    assert!(report.meets_m4_trainer_gate);
    assert!(meets_m4_trainer_gate(&report));
    assert!(report.gate_failures.is_empty());
    assert_eq!(report.trainer_exit_status, Some(0));
    assert!(report.output_lineage.artifacts_present);
  }

  #[test]
  fn m4_missing_output_lineage_fails_gate() {
    let session = strafe_session();
    let packet = build_m4_trainer_packet_from_session(&packet_build_input(session.clone())).expect("packet");
    let mut input = fake_result_input(packet, session);
    input.output_lineage.artifacts_present = false;
    input.output_lineage.normalized_result_dir.clear();

    let report = record_m4_trainer_result(input);
    assert!(!meets_m4_trainer_gate(&report));
    assert!(report.gate_failures.iter().any(|failure| failure.contains("output_artifact_lineage_missing")));
    assert!(report.gate_failures.iter().any(|failure| failure.contains("normalized_result_dir_missing")));
  }

  #[test]
  fn m4_missing_trainer_command_fails_gate() {
    let session = strafe_session();
    let packet = build_m4_trainer_packet_from_session(&packet_build_input(session.clone())).expect("packet");
    let mut input = fake_result_input(packet, session);
    input.packet.trainer_command.launch_command.clear();
    input.packet.trainer_command.trainer_backend.clear();

    let report = record_m4_trainer_result(input);
    assert!(!meets_m4_trainer_gate(&report));
    assert!(report.gate_failures.iter().any(|failure| failure == "missing_trainer_command_or_backend"));
  }

  #[test]
  fn m4_holdout_metric_from_m3_score_round_trips_fields() {
    let score = M3QueryScoreReport {
      schema_version: crate::m3_query_scoring::M3_QUERY_SCORE_REPORT_SCHEMA_VERSION,
      session_id: "m4-strafe".to_string(),
      anchor_observation_id: "obs-anchor".to_string(),
      query_observation_id: "obs-translate".to_string(),
      session_meets_m2_gate: true,
      request_leaks: Vec::new(),
      target_anchor_recall: true,
      visibility_class_correct: true,
      holdout_visibility: M3HoldoutVisibility::Visible,
      response_visibility: M3QueryVisibilityAnswer::Visible,
      projection_pixel_error_px: Some(91.3),
      projection_within_tolerance: Some(true),
      relative_depth_order_correct: Some(false),
      unknown_refusal_correct: true,
      overconfident_when_wrong: false,
      meets_m3_query_gate: true,
    };
    let metric = m4_holdout_spatial_query_metric_from_m3(&score);
    assert_eq!(metric.projection_pixel_error_px, Some(91.3));
    assert!(metric.meets_m3_query_gate);
  }

  #[test]
  fn m4_trainer_result_report_round_trips_through_json_persistence() {
    let session = strafe_session();
    let packet = build_m4_trainer_packet_from_session(&packet_build_input(session.clone())).expect("packet");
    let report = record_m4_trainer_result(fake_result_input(packet, session));
    let path = tempfile::NamedTempFile::new().expect("temp report").into_temp_path();
    write_m4_trainer_result_report(&path, &report).expect("write report");
    let restored: M4TrainerResultReport = auv_file::read_json_file(&path).expect("read report");
    assert_eq!(restored, report);
  }

  #[test]
  fn m4_live_session_root_points_off_c_drive() {
    assert!(M4_LIVE_SESSION_ROOT.starts_with(r"F:\"));
    assert!(!M4_LIVE_SESSION_ROOT.starts_with(r"C:\"));
  }
}
