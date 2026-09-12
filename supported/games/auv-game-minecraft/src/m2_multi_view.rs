//! M2 multi-view capture binding: anchor / translate / revisit sessions.
//!
//! Withheld engine pose stays off the model path. Pairwise motion is computed
//! from `M1WithheldMinecraftTruth` and serialized as relative deltas only.
//!
//! M3 viewpoint-conditioned memory/query scoring lives in `m3_query_scoring.rs`.
//! M4 trainer packets live in `m4_trainer.rs`.
//! TODO(m2-vlm-transport): provider-neutral multi-view request emission stays
//! outside this crate until an owner-approved transport boundary exists.
//!
//! NOTICE(live-evidence-m2-2026-09-12): Windows three-view session at
//! `.tmp/m2-session/` (`session-report.json`, `views.json`); live run reported
//! `meets_m2_capture_gate=true` with ≥0.5 m withheld eye translation and non-zero
//! `GetTickCount64` capture clocks. See 3dgs design doc M2 section for distances.

use std::path::Path;

use auv_file::{JsonWriteOptions, write_json_file};
use serde::{Deserialize, Serialize};

use crate::bind::bind_capture_to_frame;
use crate::dataset::SourceArtifactUri;
use crate::ingest::read_latest_spatial_frame_from_tail;
use crate::m1_black_box_baseline::{M1BlackBoxRequest, prepare_m1_black_box_request};
use crate::m1_black_box_observation::{M1_IN_GAME_SCREEN_STATE, M1BlackBoxObservationError, split_bound_frame_for_m1};
use crate::m1_black_box_scoring::M1WithheldMinecraftTruth;
use crate::spatial_memory_observation::{ObservationInputEvent, SpatialObservationPacket};
use crate::types::{MinecraftSpatialFrame, PlayerPose, Vec3};

pub const M2_MULTI_VIEW_SESSION_REPORT_SCHEMA_VERSION: u32 = 1;

/// Minimum eye-position Euclidean translation (meters) for parallax evidence.
///
/// Minecraft blocks are 1 m; 0.5 m is half a block of lateral or forward motion.
pub const M2_SIGNIFICANT_TRANSLATION_METERS: f64 = 0.5;

pub const M2_MIN_VIEW_COUNT: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum M2ViewRole {
  Anchor,
  Translate,
  Revisit,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M2ViewSample {
  pub observation_id: String,
  pub screenshot_artifact_ref: String,
  pub capture_monotonic_timestamp_ms: Option<u64>,
  pub telemetry_frame_id: String,
  pub capture_skew_ms: i64,
  pub role: M2ViewRole,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct M2TranslationDeltaMeters {
  pub x: f64,
  pub y: f64,
  pub z: f64,
}

impl M2TranslationDeltaMeters {
  pub const fn new(x: f64, y: f64, z: f64) -> Self {
    Self { x, y, z }
  }

  pub fn from_vec3(delta: Vec3) -> Self {
    Self {
      x: delta.x,
      y: delta.y,
      z: delta.z,
    }
  }

  pub fn euclidean_distance(self) -> f64 {
    (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M2RelativeMotion {
  pub from_observation_id: String,
  pub to_observation_id: String,
  pub translation_m: M2TranslationDeltaMeters,
  pub translation_distance_m: f64,
  pub yaw_delta_deg: f64,
  pub pitch_delta_deg: f64,
  pub translation_is_significant: bool,
  pub rotation_only: bool,
}

/// One bound multi-view session with withheld truth kept in memory only.
#[derive(Clone, Debug, PartialEq)]
pub struct M2Session {
  pub session_id: String,
  pub views: Vec<M2ViewSample>,
  pub relative_motions: Vec<M2RelativeMotion>,
  pub observations: Vec<SpatialObservationPacket>,
  pub requests: Vec<M1BlackBoxRequest>,
  pub meets_m2_capture_gate: bool,
  pub gate_failures: Vec<String>,
  /// True when every view used the sidecar frame timestamp as the capture clock
  /// (recorded skew 0). NOTICE: JVM telemetry and AUV capture clocks differ;
  /// skew 0 does not prove temporal alignment without external calibration.
  pub capture_skew_used_sidecar_fallback: bool,
  withheld: Vec<M1WithheldMinecraftTruth>,
}

/// Serializable M2 report for offline inspection. No nearby_blocks, view_matrix,
/// or absolute eye coordinates — only relative motion deltas and M1 observations.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M2MultiViewSessionReport {
  pub schema_version: u32,
  pub session_id: String,
  pub views: Vec<M2ViewSample>,
  pub relative_motions: Vec<M2RelativeMotion>,
  pub observations: Vec<SpatialObservationPacket>,
  pub requests: Vec<M1BlackBoxRequest>,
  pub meets_m2_capture_gate: bool,
  pub gate_failures: Vec<String>,
  pub capture_skew_used_sidecar_fallback: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum M2MultiViewError {
  MissingFrameSource,
  TelemetryRead(String),
  NoTelemetryFrame,
  ScreenNotInGame { screen_state: Option<String> },
  Observation(M1BlackBoxObservationError),
  Prepare(String),
  EmptySession,
}

impl std::fmt::Display for M2MultiViewError {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::MissingFrameSource => formatter.write_str("M2 view capture requires a spatial frame or telemetry path"),
      Self::TelemetryRead(error) => write!(formatter, "M2 view capture failed to read telemetry: {error}"),
      Self::NoTelemetryFrame => formatter.write_str("M2 view capture requires a well-formed telemetry frame"),
      Self::ScreenNotInGame { screen_state } => match screen_state {
        Some(state) => write!(formatter, "M2 view capture requires screen_state {M1_IN_GAME_SCREEN_STATE}, found {state}"),
        None => write!(formatter, "M2 view capture requires screen_state {M1_IN_GAME_SCREEN_STATE}"),
      },
      Self::Observation(error) => write!(formatter, "M2 view capture failed to split bound frame: {error}"),
      Self::Prepare(error) => write!(formatter, "M2 view capture failed to prepare black-box request: {error}"),
      Self::EmptySession => formatter.write_str("M2 session requires at least one view capture"),
    }
  }
}

/// Caller-supplied capture for one M2 view role.
#[derive(Clone, Debug, PartialEq)]
pub struct M2ViewCaptureInput {
  /// When set, read the newest frame from this telemetry JSONL tail.
  pub telemetry_path: Option<std::path::PathBuf>,
  /// When set, bind this frame directly (hermetic tests and replay).
  pub frame: Option<MinecraftSpatialFrame>,
  pub screenshot_artifact_ref: String,
  pub capture_monotonic_timestamp_ms: Option<u64>,
  pub role: M2ViewRole,
  pub input_history: Vec<ObservationInputEvent>,
}

/// One bound M2 view before session assembly. Withheld truth is kept internal.
#[derive(Clone, Debug, PartialEq)]
pub struct M2IngestedView {
  pub sample: M2ViewSample,
  pub observation: SpatialObservationPacket,
  pub request: M1BlackBoxRequest,
  pub used_sidecar_capture_clock: bool,
  withheld: M1WithheldMinecraftTruth,
}

/// Bind one screenshot to telemetry and split into M1 observation + withheld truth.
pub fn ingest_m2_view_capture(input: &M2ViewCaptureInput) -> Result<M2IngestedView, M2MultiViewError> {
  let frame = resolve_spatial_frame(input)?;
  if frame.screen_state.as_deref() != Some(M1_IN_GAME_SCREEN_STATE) {
    return Err(M2MultiViewError::ScreenNotInGame {
      screen_state: frame.screen_state,
    });
  }

  let used_sidecar_capture_clock = input.capture_monotonic_timestamp_ms.is_none();
  let capture_clock_ms = input.capture_monotonic_timestamp_ms.unwrap_or(frame.monotonic_timestamp_ms);
  let bound = bind_capture_to_frame(frame, Some(input.screenshot_artifact_ref.clone()), capture_clock_ms);
  let split = split_bound_frame_for_m1(bound, input.input_history.clone()).map_err(M2MultiViewError::Observation)?;
  let request = prepare_m1_black_box_request(split.observation.clone()).map_err(|error| M2MultiViewError::Prepare(error.to_string()))?;

  let sample = M2ViewSample {
    observation_id: split.observation.observation_id.clone(),
    screenshot_artifact_ref: split.observation.screenshot_artifact_ref.clone().expect("split guarantees screenshot"),
    capture_monotonic_timestamp_ms: input.capture_monotonic_timestamp_ms,
    telemetry_frame_id: split.withheld.frame().spatial_frame_id.clone(),
    capture_skew_ms: split.withheld.frame().mc_capture_skew_ms.unwrap_or(0),
    role: input.role,
  };

  Ok(M2IngestedView {
    sample,
    observation: split.observation,
    request,
    used_sidecar_capture_clock,
    withheld: split.withheld,
  })
}

/// Build an M2 session from ordered ingested views.
pub fn build_m2_session_from_ingested_views(session_id: Option<String>, views: Vec<M2IngestedView>) -> M2Session {
  assemble_m2_session(session_id, views)
}

/// Build an M2 session from ordered view captures.
pub fn build_m2_session_from_captures(session_id: Option<String>, captures: &[M2ViewCaptureInput]) -> Result<M2Session, M2MultiViewError> {
  if captures.is_empty() {
    return Err(M2MultiViewError::EmptySession);
  }

  let mut ingested_views = Vec::with_capacity(captures.len());
  for capture in captures {
    ingested_views.push(ingest_m2_view_capture(capture)?);
  }

  Ok(assemble_m2_session(session_id, ingested_views))
}

fn resolve_spatial_frame(input: &M2ViewCaptureInput) -> Result<MinecraftSpatialFrame, M2MultiViewError> {
  if let Some(frame) = &input.frame {
    return Ok(frame.clone());
  }
  let Some(path) = input.telemetry_path.as_deref() else {
    return Err(M2MultiViewError::MissingFrameSource);
  };
  match read_latest_spatial_frame_from_tail(path) {
    Ok(Some(frame)) => Ok(frame),
    Ok(None) => Err(M2MultiViewError::NoTelemetryFrame),
    Err(error) => Err(M2MultiViewError::TelemetryRead(error)),
  }
}

fn assemble_m2_session(session_id: Option<String>, ingested_views: Vec<M2IngestedView>) -> M2Session {
  let views: Vec<M2ViewSample> = ingested_views.iter().map(|view| view.sample.clone()).collect();
  let observations: Vec<SpatialObservationPacket> = ingested_views.iter().map(|view| view.observation.clone()).collect();
  let requests: Vec<M1BlackBoxRequest> = ingested_views.iter().map(|view| view.request.clone()).collect();
  let withheld: Vec<M1WithheldMinecraftTruth> = ingested_views.iter().map(|view| view.withheld.clone()).collect();
  let capture_skew_used_sidecar_fallback = ingested_views.iter().all(|view| view.used_sidecar_capture_clock);

  let relative_motions = compute_pairwise_motions(&views, &withheld);
  let (meets_m2_capture_gate, gate_failures) = evaluate_m2_capture_gate(&views, &relative_motions);

  let session_id = session_id.unwrap_or_else(|| default_session_id(&views));

  M2Session {
    session_id,
    views,
    relative_motions,
    observations,
    requests,
    meets_m2_capture_gate,
    gate_failures,
    capture_skew_used_sidecar_fallback,
    withheld,
  }
}

fn default_session_id(views: &[M2ViewSample]) -> String {
  views.first().map(|view| format!("m2-{}", view.telemetry_frame_id)).unwrap_or_else(|| "m2-empty".to_string())
}

fn compute_pairwise_motions(views: &[M2ViewSample], withheld: &[M1WithheldMinecraftTruth]) -> Vec<M2RelativeMotion> {
  let mut motions = Vec::new();
  for left in 0..views.len() {
    for right in (left + 1)..views.len() {
      motions.push(relative_motion_between(
        &views[left].observation_id,
        &views[right].observation_id,
        withheld[left].frame().player_pose,
        withheld[right].frame().player_pose,
      ));
    }
  }
  motions
}

fn relative_motion_between(from_id: &str, to_id: &str, from_pose: PlayerPose, to_pose: PlayerPose) -> M2RelativeMotion {
  let translation = M2TranslationDeltaMeters::from_vec3(Vec3::new(
    to_pose.eye_position.x - from_pose.eye_position.x,
    to_pose.eye_position.y - from_pose.eye_position.y,
    to_pose.eye_position.z - from_pose.eye_position.z,
  ));
  let translation_distance_m = translation.euclidean_distance();
  let yaw_delta_deg = normalize_angle_deg(to_pose.yaw - from_pose.yaw);
  let pitch_delta_deg = normalize_angle_deg(to_pose.pitch - from_pose.pitch);
  let translation_is_significant = translation_distance_m >= M2_SIGNIFICANT_TRANSLATION_METERS;
  let rotation_only = !translation_is_significant && (yaw_delta_deg.abs() > 0.01 || pitch_delta_deg.abs() > 0.01);

  M2RelativeMotion {
    from_observation_id: from_id.to_string(),
    to_observation_id: to_id.to_string(),
    translation_m: translation,
    translation_distance_m,
    yaw_delta_deg,
    pitch_delta_deg,
    translation_is_significant,
    rotation_only,
  }
}

fn normalize_angle_deg(delta: f64) -> f64 {
  let mut normalized = delta % 360.0;
  if normalized > 180.0 {
    normalized -= 360.0;
  } else if normalized <= -180.0 {
    normalized += 360.0;
  }
  normalized
}

fn evaluate_m2_capture_gate(views: &[M2ViewSample], relative_motions: &[M2RelativeMotion]) -> (bool, Vec<String>) {
  let mut failures = Vec::new();

  if views.len() < M2_MIN_VIEW_COUNT {
    failures.push(format!("view_count={} < required {}", views.len(), M2_MIN_VIEW_COUNT));
  }

  for view in views {
    if SourceArtifactUri::new(&view.screenshot_artifact_ref).is_err() {
      failures.push(format!("view {} has non-canonical screenshot URI", view.observation_id));
    }
    if view.capture_skew_ms == 0 {
      // Skew may legitimately be 0 when falling back to sidecar clock; still recorded.
    }
  }

  if !relative_motions.iter().any(|motion| motion.translation_is_significant) {
    failures.push(format!("no pair reached significant translation (>={M2_SIGNIFICANT_TRANSLATION_METERS} m eye-position Euclidean)"));
  }

  (failures.is_empty(), failures)
}

/// Project an in-memory session into the serializable report shape.
pub fn m2_session_report(session: &M2Session) -> M2MultiViewSessionReport {
  M2MultiViewSessionReport {
    schema_version: M2_MULTI_VIEW_SESSION_REPORT_SCHEMA_VERSION,
    session_id: session.session_id.clone(),
    views: session.views.clone(),
    relative_motions: session.relative_motions.clone(),
    observations: session.observations.clone(),
    requests: session.requests.clone(),
    meets_m2_capture_gate: session.meets_m2_capture_gate,
    gate_failures: session.gate_failures.clone(),
    capture_skew_used_sidecar_fallback: session.capture_skew_used_sidecar_fallback,
  }
}

/// Withheld engine frames for operator inspection only. Must not be serialized
/// into model-facing artifacts.
pub fn m2_session_withheld_truth(session: &M2Session) -> &[M1WithheldMinecraftTruth] {
  &session.withheld
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum M2MultiViewPersistenceError {
  Persistence(String),
}

impl std::fmt::Display for M2MultiViewPersistenceError {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Persistence(message) => write!(formatter, "M2 multi-view session report persistence failed: {message}"),
    }
  }
}

/// Persist an M2 session report as JSON for offline inspection or replay.
pub fn write_m2_session_report(path: &Path, session: &M2Session) -> Result<(), M2MultiViewPersistenceError> {
  let report = m2_session_report(session);
  write_json_file(
    path,
    &report,
    JsonWriteOptions {
      create_parent_dirs: true,
      trailing_newline: true,
    },
  )
  .map_err(|error| M2MultiViewPersistenceError::Persistence(format!("{error:?}")))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::types::{BlockFace, BlockPosition, NearbyBlock, PlayerPose, RaycastHit, Viewport};

  const SCREENSHOT_ANCHOR: &str = "auv://runs/run-m2/artifacts/view-anchor.png";
  const SCREENSHOT_TRANSLATE: &str = "auv://runs/run-m2/artifacts/view-translate.png";
  const SCREENSHOT_REVISIT: &str = "auv://runs/run-m2/artifacts/view-revisit.png";

  fn base_frame(spatial_frame_id: &str, eye: Vec3, yaw: f64, pitch: f64, timestamp_ms: u64) -> MinecraftSpatialFrame {
    MinecraftSpatialFrame {
      spatial_frame_id: spatial_frame_id.to_string(),
      world_tick: 1200,
      monotonic_timestamp_ms: timestamp_ms,
      telemetry_session_id: Some("session-m2".to_string()),
      viewport: Viewport::new(1280, 720),
      view_matrix: [1.0; 16],
      projection_matrix: [2.0; 16],
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
      mc_capture_skew_ms: None,
      screen_state: Some(M1_IN_GAME_SCREEN_STATE.to_string()),
      resource_pack_ids: vec!["vanilla".to_string()],
    }
  }

  fn capture_input(frame: MinecraftSpatialFrame, screenshot: &str, role: M2ViewRole, capture_ms: Option<u64>) -> M2ViewCaptureInput {
    M2ViewCaptureInput {
      telemetry_path: None,
      frame: Some(frame),
      screenshot_artifact_ref: screenshot.to_string(),
      capture_monotonic_timestamp_ms: capture_ms,
      role,
      input_history: Vec::new(),
    }
  }

  fn strafe_session_inputs() -> Vec<M2ViewCaptureInput> {
    vec![
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
    ]
  }

  fn yaw_only_session_inputs() -> Vec<M2ViewCaptureInput> {
    let eye = Vec3::new(10.0, 72.0, 20.0);
    vec![
      capture_input(base_frame("frame-yaw-a", eye, 90.0, -10.0, 1_000), SCREENSHOT_ANCHOR, M2ViewRole::Anchor, Some(950)),
      capture_input(base_frame("frame-yaw-b", eye, 120.0, -10.0, 2_000), SCREENSHOT_TRANSLATE, M2ViewRole::Translate, Some(1_950)),
      capture_input(base_frame("frame-yaw-c", eye, 150.0, -5.0, 3_000), SCREENSHOT_REVISIT, M2ViewRole::Revisit, Some(2_950)),
    ]
  }

  #[test]
  fn m2_strafe_session_passes_capture_gate() {
    let session = build_m2_session_from_captures(Some("m2-strafe".to_string()), &strafe_session_inputs()).expect("strafe session");

    assert!(session.meets_m2_capture_gate);
    assert!(session.gate_failures.is_empty());
    assert_eq!(session.views.len(), 3);
    assert!(session.relative_motions.iter().any(|motion| motion.translation_is_significant));
    assert_eq!(session.views[0].role, M2ViewRole::Anchor);
    assert_eq!(session.views[1].role, M2ViewRole::Translate);
    assert_eq!(session.views[2].role, M2ViewRole::Revisit);
  }

  #[test]
  fn m2_yaw_only_session_fails_capture_gate() {
    let session = build_m2_session_from_captures(None, &yaw_only_session_inputs()).expect("yaw-only session");

    assert!(!session.meets_m2_capture_gate);
    assert!(session.gate_failures.iter().any(|failure| failure.contains("significant translation")));
    assert!(session.relative_motions.iter().all(|motion| motion.rotation_only));
  }

  #[test]
  fn m2_under_three_views_fails_capture_gate() {
    let inputs = strafe_session_inputs()[..2].to_vec();
    let session = build_m2_session_from_captures(None, &inputs).expect("two-view session");

    assert!(!session.meets_m2_capture_gate);
    assert!(session.gate_failures.iter().any(|failure| failure.contains("view_count=2")));
  }

  #[test]
  fn m2_observation_and_request_json_have_no_engine_truth() {
    let session = build_m2_session_from_captures(None, &strafe_session_inputs()).expect("strafe session");
    let report = m2_session_report(&session);
    let report_json = serde_json::to_string(&report).expect("serialize report");

    for request in &session.requests {
      let request_json = serde_json::to_string(request).expect("serialize request");
      assert!(!request_json.contains("nearby_blocks"));
      assert!(!request_json.contains("player_pose"));
      assert!(!request_json.contains("view_matrix"));
      assert!(!request_json.contains("513.250000"));
      assert!(!request_json.contains("minecraft:red_wool"));
    }

    for observation in &session.observations {
      let observation_json = serde_json::to_string(observation).expect("serialize observation");
      assert!(!observation_json.contains("nearby_blocks"));
      assert!(!observation_json.contains("player_pose"));
      assert!(!observation_json.contains("view_matrix"));
      assert!(!observation_json.contains("eye_position"));
    }

    assert!(!report_json.contains("nearby_blocks"));
    assert!(!report_json.contains("view_matrix"));
    assert!(!report_json.contains("eye_position"));
    assert!(report_json.contains("translation_m"));
    assert!(report_json.contains("translation_distance_m"));
  }

  #[test]
  fn m2_session_report_round_trips_through_json_persistence() {
    let session = build_m2_session_from_captures(None, &strafe_session_inputs()).expect("strafe session");
    let path = tempfile::NamedTempFile::new().expect("temp report").into_temp_path();
    write_m2_session_report(&path, &session).expect("write report");
    let restored: M2MultiViewSessionReport = auv_file::read_json_file(&path).expect("read report");

    assert_eq!(restored, m2_session_report(&session));
    assert!(restored.meets_m2_capture_gate);
  }

  #[test]
  fn m2_sidecar_capture_clock_records_zero_skew_with_notice_flag() {
    let mut inputs = strafe_session_inputs();
    for input in &mut inputs {
      input.capture_monotonic_timestamp_ms = None;
    }
    let session = build_m2_session_from_captures(None, &inputs).expect("sidecar clock session");

    assert!(session.capture_skew_used_sidecar_fallback);
    assert!(session.views.iter().all(|view| view.capture_skew_ms == 0));
    assert!(session.meets_m2_capture_gate);
  }

  #[test]
  fn m2_withheld_truth_stays_off_report_path() {
    let session = build_m2_session_from_captures(None, &strafe_session_inputs()).expect("strafe session");
    let withheld = m2_session_withheld_truth(&session);
    let report_json = serde_json::to_string(&m2_session_report(&session)).expect("serialize report");

    assert_eq!(withheld.len(), 3);
    assert_eq!(withheld[0].frame().nearby_blocks[0].block_id, "minecraft:smooth_stone");
    assert!(!report_json.contains("minecraft:smooth_stone"));
    assert!(!report_json.contains("minecraft:red_wool"));
  }
}
