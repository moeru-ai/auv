//! M3 viewpoint-conditioned memory/query scoring against holdout answer keys.
//!
//! Flow: bound M2 session → anchor hypothesis memory → black-box query request
//! from a second view → externally supplied structured response → score against
//! withheld engine geometry at the query view only.
//!
//! M4 trainer packets live in `m4_trainer.rs`.
//! TODO(m3-vlm-transport): provider-neutral query transport stays outside this
//! crate until an owner-approved boundary exists.
//! TODO(m3-occlusion-holdout): holdout visibility is frustum/containment only;
//! per-block occlusion scoring stays deferred until telemetry exposes a depth
//! buffer, target-directed ray, or per-block visibility signal — see
//! `reacquisition.rs` and the lane handoff occlusion finding.

use std::path::Path;

use auv_driver::geometry::Point;
use auv_file::{JsonWriteOptions, write_json_file};
use serde::{Deserialize, Serialize};

use crate::m1_black_box_scoring::M1WithheldMinecraftTruth;
use crate::m2_multi_view::{M2Session, M2ViewRole};
use crate::reacquisition::{ReacquisitionQuery, ReacquisitionStatus, reacquire_from_geometry};
use crate::spatial_memory_observation::{
  SpatialHypothesisMemory, SpatialHypothesisPatch, SpatialObservationPacket, validate_spatial_hypothesis_patch,
};
use crate::types::{BlockFace, BlockPosition, MinecraftTargetSemantics};

pub const M3_QUERY_REQUEST_SCHEMA_VERSION: u32 = 1;
pub const M3_SPATIAL_QUERY_RESPONSE_SCHEMA_VERSION: u32 = 1;
pub const M3_QUERY_RESPONSE_REPORT_SCHEMA_VERSION: u32 = 1;
pub const M3_QUERY_SCORE_REPORT_SCHEMA_VERSION: u32 = 1;
pub const M3_QUERY_VERIFICATION_REPORT_SCHEMA_VERSION: u32 = 1;

/// Pixel tolerance when comparing claimed projection to holdout geometry.
pub const M3_PROJECTION_TOLERANCE_PX: f64 = 48.0;

/// Built-in role prompt for querying multi-view hypothesis memory from a new
/// viewpoint. It must not grant access to withheld engine truth.
pub const MULTI_VIEW_SPATIAL_QUERY_PROMPT: &str = r#"你是多视角空间记忆查询器，不是世界真值生成器。

你只能使用输入中明确存在的当前视角截图、视口信息、输入历史，以及来自先前视角的 hypothesis/candidate memory patch。禁止假设不存在的 depth、raycast、world pose、nearby blocks 或 view matrix。禁止把模型常识或语言补全当作观测。

请回答：从当前视角出发，anchor memory 中标记的目标在当前屏幕上的可见性如何？如果可见，给出 screen-relative 投影；如果不可见或证据不足，必须明确 unknown/refusal，而不是猜测。

输出 M3SpatialQueryResponse JSON。不得写入 engine truth、方块坐标、绝对 eye 位置或 view matrix。"#;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M3QueryRequest {
  pub schema_version: u32,
  pub system_prompt: String,
  pub query_observation: SpatialObservationPacket,
  pub memory: SpatialHypothesisMemory,
  pub query_text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum M3QueryResponseStatus {
  Accepted,
  Rejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum M3QueryVisibilityAnswer {
  Visible,
  NotVisible,
  Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum M3RelativeDepthOrderAnswer {
  CloserThanAnchorView,
  FartherThanAnchorView,
  SameDistance,
  Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M3ScreenProjectionClaim {
  pub x: f64,
  pub y: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M3SpatialQueryResponse {
  pub schema_version: u32,
  pub query_observation_id: String,
  pub anchor_claim_id: String,
  pub visibility: M3QueryVisibilityAnswer,
  #[serde(default)]
  pub screen_projection: Option<M3ScreenProjectionClaim>,
  #[serde(default)]
  pub relative_depth_order: Option<M3RelativeDepthOrderAnswer>,
  #[serde(default)]
  pub refused_or_unknown: bool,
  #[serde(default)]
  pub confidence: Option<crate::spatial_memory_observation::SpatialConfidence>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M3QueryResponseReport {
  pub schema_version: u32,
  pub query_observation_id: String,
  pub status: M3QueryResponseStatus,
  pub response: Option<M3SpatialQueryResponse>,
  pub errors: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum M3HoldoutVisibility {
  Visible,
  NotVisible,
  Indeterminate,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M3QueryScoreReport {
  pub schema_version: u32,
  pub session_id: String,
  pub anchor_observation_id: String,
  pub query_observation_id: String,
  pub session_meets_m2_gate: bool,
  pub request_leaks: Vec<String>,
  pub target_anchor_recall: bool,
  pub visibility_class_correct: bool,
  pub holdout_visibility: M3HoldoutVisibility,
  pub response_visibility: M3QueryVisibilityAnswer,
  pub projection_pixel_error_px: Option<f64>,
  pub projection_within_tolerance: Option<bool>,
  pub relative_depth_order_correct: Option<bool>,
  pub unknown_refusal_correct: bool,
  pub overconfident_when_wrong: bool,
  pub meets_m3_query_gate: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M3QueryVerificationReport {
  pub schema_version: u32,
  pub session_id: String,
  pub request: M3QueryRequest,
  pub response: M3QueryResponseReport,
  pub score: Option<M3QueryScoreReport>,
}

/// Withheld scoring target. Block coordinates are answer-key material only and
/// must never appear in `M3QueryRequest`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct M3ScoringTarget {
  pub anchor_claim_id: String,
  pub block_pos: BlockPosition,
  pub block_face: Option<BlockFace>,
}

/// Session inputs for one M3 query/score pass.
#[derive(Clone, Debug, PartialEq)]
pub struct M3QuerySessionInput {
  pub session: M2Session,
  pub anchor_patch: SpatialHypothesisPatch,
  pub query_view_role: M2ViewRole,
  pub target: M3ScoringTarget,
}

#[derive(Clone, Debug, PartialEq)]
pub struct M3QueryPrepared {
  pub request: M3QueryRequest,
  pub anchor_observation_id: String,
  pub query_observation_id: String,
  pub session_id: String,
  pub session_meets_m2_gate: bool,
  withheld: M3QueryWithheldContext,
}

#[derive(Clone, Debug, PartialEq)]
struct M3QueryWithheldContext {
  query_frame: M1WithheldMinecraftTruth,
  anchor_frame: M1WithheldMinecraftTruth,
  target: M3ScoringTarget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum M3QueryError {
  UngatedM2Session { gate_failures: Vec<String> },
  MissingViewRole { role: M2ViewRole },
  AnchorMemory(String),
  EmptyQueryText,
}

impl std::fmt::Display for M3QueryError {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::UngatedM2Session { gate_failures } => {
        write!(formatter, "M3 query requires a gated M2 session; failures: {}", gate_failures.join("; "))
      }
      Self::MissingViewRole { role } => write!(formatter, "M3 query session is missing view role {role:?}"),
      Self::AnchorMemory(message) => write!(formatter, "M3 anchor memory preparation failed: {message}"),
      Self::EmptyQueryText => formatter.write_str("M3 query text must not be empty"),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum M3QueryScoreError {
  RejectedResponse,
  MissingAcceptedResponse,
}

impl std::fmt::Display for M3QueryScoreError {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::RejectedResponse => formatter.write_str("rejected M3 query responses cannot be scored against withheld truth"),
      Self::MissingAcceptedResponse => formatter.write_str("accepted M3 query response is missing structured output"),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum M3QueryPersistenceError {
  Persistence(String),
}

impl std::fmt::Display for M3QueryPersistenceError {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Persistence(message) => write!(formatter, "M3 query report persistence failed: {message}"),
    }
  }
}

/// Prepare a leak-free query request from a gated M2 session and anchor memory.
pub fn prepare_m3_query_from_session(input: &M3QuerySessionInput) -> Result<M3QueryPrepared, M3QueryError> {
  if !input.session.meets_m2_capture_gate {
    return Err(M3QueryError::UngatedM2Session {
      gate_failures: input.session.gate_failures.clone(),
    });
  }

  let anchor_index = find_view_index(&input.session, M2ViewRole::Anchor).ok_or(M3QueryError::MissingViewRole {
    role: M2ViewRole::Anchor,
  })?;
  let query_index = find_view_index(&input.session, input.query_view_role).ok_or(M3QueryError::MissingViewRole {
    role: input.query_view_role,
  })?;

  let anchor_observation = input.session.observations[anchor_index].clone();
  let query_observation = input.session.observations[query_index].clone();
  validate_spatial_hypothesis_patch(&anchor_observation, &input.anchor_patch)
    .map_err(|errors| M3QueryError::AnchorMemory(errors.into_iter().map(|error| error.to_string()).collect::<Vec<_>>().join("; ")))?;

  let mut memory = SpatialHypothesisMemory::default();
  memory
    .append(&anchor_observation, input.anchor_patch.clone())
    .map_err(|errors| M3QueryError::AnchorMemory(errors.into_iter().map(|error| error.to_string()).collect::<Vec<_>>().join("; ")))?;

  let query_text = default_query_text(&input.target.anchor_claim_id);
  if query_text.trim().is_empty() {
    return Err(M3QueryError::EmptyQueryText);
  }

  let request = M3QueryRequest {
    schema_version: M3_QUERY_REQUEST_SCHEMA_VERSION,
    system_prompt: MULTI_VIEW_SPATIAL_QUERY_PROMPT.to_string(),
    query_observation,
    memory,
    query_text,
  };

  let withheld_frames = crate::m2_multi_view::m2_session_withheld_truth(&input.session);
  Ok(M3QueryPrepared {
    request,
    anchor_observation_id: input.session.views[anchor_index].observation_id.clone(),
    query_observation_id: input.session.views[query_index].observation_id.clone(),
    session_id: input.session.session_id.clone(),
    session_meets_m2_gate: true,
    withheld: M3QueryWithheldContext {
      query_frame: withheld_frames[query_index].clone(),
      anchor_frame: withheld_frames[anchor_index].clone(),
      target: input.target.clone(),
    },
  })
}

/// Withheld answer-key context for operator inspect only. Must not be serialized
/// into model-facing artifacts.
pub fn m3_query_withheld_context(prepared: &M3QueryPrepared) -> (&M1WithheldMinecraftTruth, &M1WithheldMinecraftTruth, &M3ScoringTarget) {
  (&prepared.withheld.anchor_frame, &prepared.withheld.query_frame, &prepared.withheld.target)
}

/// Parse and validate externally produced query JSON.
pub fn inspect_m3_query_response(request: &M3QueryRequest, response_json: &[u8]) -> M3QueryResponseReport {
  let mut request_errors = Vec::new();
  if request.schema_version != M3_QUERY_REQUEST_SCHEMA_VERSION {
    request_errors.push(format!("unsupported M3 request schema {}; expected {}", request.schema_version, M3_QUERY_REQUEST_SCHEMA_VERSION));
  }
  if request.system_prompt != MULTI_VIEW_SPATIAL_QUERY_PROMPT {
    request_errors.push("M3 request system prompt does not match the built-in contract".to_string());
  }
  if !request_errors.is_empty() {
    return rejected_response_report(&request.query_observation.observation_id, request_errors);
  }

  let response = match serde_json::from_slice::<M3SpatialQueryResponse>(response_json) {
    Ok(response) => response,
    Err(error) => {
      return rejected_response_report(
        &request.query_observation.observation_id,
        vec![format!("invalid M3SpatialQueryResponse JSON: {error}")],
      );
    }
  };

  let mut errors = Vec::new();
  if response.schema_version != M3_SPATIAL_QUERY_RESPONSE_SCHEMA_VERSION {
    errors
      .push(format!("unsupported M3 response schema {}; expected {}", response.schema_version, M3_SPATIAL_QUERY_RESPONSE_SCHEMA_VERSION));
  }
  if response.query_observation_id != request.query_observation.observation_id {
    errors.push(format!(
      "response query_observation_id {:?} does not match request {:?}",
      response.query_observation_id, request.query_observation.observation_id
    ));
  }
  if response.anchor_claim_id.trim().is_empty() {
    errors.push("anchor_claim_id must not be empty".to_string());
  }
  if response.visibility == M3QueryVisibilityAnswer::Visible && response.screen_projection.is_none() {
    errors.push("visible answers must include screen_projection".to_string());
  }
  if response.visibility != M3QueryVisibilityAnswer::Unknown && response.refused_or_unknown {
    errors.push("refused_or_unknown may only be true when visibility is unknown".to_string());
  }

  if errors.is_empty() {
    M3QueryResponseReport {
      schema_version: M3_QUERY_RESPONSE_REPORT_SCHEMA_VERSION,
      query_observation_id: request.query_observation.observation_id.clone(),
      status: M3QueryResponseStatus::Accepted,
      response: Some(response),
      errors: Vec::new(),
    }
  } else {
    rejected_response_report(&request.query_observation.observation_id, errors)
  }
}

/// Score an accepted query response against withheld holdout geometry.
pub fn score_accepted_m3_query_response(
  prepared: &M3QueryPrepared,
  response: &M3QueryResponseReport,
) -> Result<M3QueryScoreReport, M3QueryScoreError> {
  if response.status != M3QueryResponseStatus::Accepted {
    return Err(M3QueryScoreError::RejectedResponse);
  }
  let Some(answer) = response.response.as_ref() else {
    return Err(M3QueryScoreError::MissingAcceptedResponse);
  };

  let request_leaks = leaked_withheld_facts(&prepared.request, &prepared.withheld);
  let holdout = compute_holdout_answer(&prepared.withheld);
  let target_anchor_recall = answer.anchor_claim_id == prepared.withheld.target.anchor_claim_id;
  let visibility_class_correct = visibility_matches_holdout(answer.visibility, holdout.visibility);
  let (projection_pixel_error_px, projection_within_tolerance) = projection_score(answer, &holdout);
  let relative_depth_order_correct = relative_depth_score(answer, &prepared.withheld);
  let unknown_refusal_correct = unknown_refusal_score(answer, holdout.visibility);
  let overconfident_when_wrong = overconfidence_when_wrong(answer, visibility_class_correct, projection_within_tolerance);
  let meets_m3_query_gate = prepared.session_meets_m2_gate
    && request_leaks.is_empty()
    && target_anchor_recall
    && visibility_class_correct
    && unknown_refusal_correct
    && !overconfident_when_wrong
    && projection_within_tolerance.unwrap_or(true);

  Ok(M3QueryScoreReport {
    schema_version: M3_QUERY_SCORE_REPORT_SCHEMA_VERSION,
    session_id: prepared.session_id.clone(),
    anchor_observation_id: prepared.anchor_observation_id.clone(),
    query_observation_id: prepared.query_observation_id.clone(),
    session_meets_m2_gate: prepared.session_meets_m2_gate,
    request_leaks,
    target_anchor_recall,
    visibility_class_correct,
    holdout_visibility: holdout.visibility,
    response_visibility: answer.visibility,
    projection_pixel_error_px,
    projection_within_tolerance,
    relative_depth_order_correct,
    unknown_refusal_correct,
    overconfident_when_wrong,
    meets_m3_query_gate,
  })
}

/// End-to-end prepare → inspect → score for one M3 query pass.
pub fn verify_m3_query_from_session(input: &M3QuerySessionInput, response_json: &[u8]) -> Result<M3QueryVerificationReport, M3QueryError> {
  let prepared = prepare_m3_query_from_session(input)?;
  let response = inspect_m3_query_response(&prepared.request, response_json);
  let score = if response.status == M3QueryResponseStatus::Accepted {
    Some(score_accepted_m3_query_response(&prepared, &response).expect("accepted response must score against withheld truth"))
  } else {
    None
  };

  Ok(M3QueryVerificationReport {
    schema_version: M3_QUERY_VERIFICATION_REPORT_SCHEMA_VERSION,
    session_id: prepared.session_id.clone(),
    request: prepared.request,
    response,
    score,
  })
}

/// True when the score report satisfies the M3 query sample gate for this slice.
///
/// This measures memory/query contract honesty and holdout agreement — not
/// geometric reconstruction accuracy, trainer output, or pixel/block hit-rate.
pub fn meets_m3_query_gate(report: &M3QueryScoreReport) -> bool {
  report.meets_m3_query_gate
}

pub fn write_m3_query_verification_report(path: &Path, report: &M3QueryVerificationReport) -> Result<(), M3QueryPersistenceError> {
  write_json_file(
    path,
    report,
    JsonWriteOptions {
      create_parent_dirs: true,
      trailing_newline: true,
    },
  )
  .map_err(|error| M3QueryPersistenceError::Persistence(format!("{error:?}")))
}

#[derive(Clone, Debug, PartialEq)]
struct M3HoldoutAnswer {
  visibility: M3HoldoutVisibility,
  screen_point: Option<Point>,
  match_radius_px: Option<f64>,
  relative_depth: M3RelativeDepthOrderAnswer,
}

fn compute_holdout_answer(withheld: &M3QueryWithheldContext) -> M3HoldoutAnswer {
  let query = ReacquisitionQuery {
    observer_frame: withheld.query_frame.frame().clone(),
    target_block: withheld.target.block_pos,
    target_face: withheld.target.block_face,
    target_semantics: MinecraftTargetSemantics::BlockCenter,
  };
  let answer = reacquire_from_geometry(&query).expect("reacquisition should return Ok for geometric outcomes");
  let visibility = match answer.status {
    ReacquisitionStatus::Reacquired => M3HoldoutVisibility::Visible,
    ReacquisitionStatus::NotVisible => M3HoldoutVisibility::NotVisible,
    ReacquisitionStatus::Failed => M3HoldoutVisibility::Indeterminate,
  };
  let relative_depth = relative_depth_holdout(withheld);

  M3HoldoutAnswer {
    visibility,
    screen_point: answer.screen_point,
    match_radius_px: answer.match_radius_px,
    relative_depth,
  }
}

fn relative_depth_holdout(withheld: &M3QueryWithheldContext) -> M3RelativeDepthOrderAnswer {
  let anchor_eye = withheld.anchor_frame.frame().player_pose.eye_position;
  let query_eye = withheld.query_frame.frame().player_pose.eye_position;
  let target = withheld.target.block_pos.center();
  let anchor_distance = distance(anchor_eye, target);
  let query_distance = distance(query_eye, target);
  let delta = query_distance - anchor_distance;
  if delta.abs() < 0.05 {
    M3RelativeDepthOrderAnswer::SameDistance
  } else if delta < 0.0 {
    M3RelativeDepthOrderAnswer::CloserThanAnchorView
  } else {
    M3RelativeDepthOrderAnswer::FartherThanAnchorView
  }
}

fn distance(from: crate::types::Vec3, to: crate::types::Vec3) -> f64 {
  let dx = to.x - from.x;
  let dy = to.y - from.y;
  let dz = to.z - from.z;
  (dx * dx + dy * dy + dz * dz).sqrt()
}

fn visibility_matches_holdout(response: M3QueryVisibilityAnswer, holdout: M3HoldoutVisibility) -> bool {
  match (response, holdout) {
    (M3QueryVisibilityAnswer::Visible, M3HoldoutVisibility::Visible) => true,
    (M3QueryVisibilityAnswer::NotVisible, M3HoldoutVisibility::NotVisible) => true,
    (M3QueryVisibilityAnswer::Unknown, M3HoldoutVisibility::NotVisible) => true,
    (M3QueryVisibilityAnswer::Unknown, M3HoldoutVisibility::Indeterminate) => true,
    (M3QueryVisibilityAnswer::NotVisible, M3HoldoutVisibility::Indeterminate) => true,
    _ => false,
  }
}

fn projection_score(answer: &M3SpatialQueryResponse, holdout: &M3HoldoutAnswer) -> (Option<f64>, Option<bool>) {
  if holdout.visibility != M3HoldoutVisibility::Visible {
    return (None, None);
  }
  let (Some(claimed), Some(truth)) = (answer.screen_projection.as_ref(), holdout.screen_point.as_ref()) else {
    return (None, Some(answer.visibility != M3QueryVisibilityAnswer::Visible));
  };
  if answer.visibility != M3QueryVisibilityAnswer::Visible {
    return (None, Some(false));
  }
  let error = ((claimed.x - truth.x).powi(2) + (claimed.y - truth.y).powi(2)).sqrt();
  let tolerance = holdout.match_radius_px.unwrap_or(M3_PROJECTION_TOLERANCE_PX);
  (Some(error), Some(error <= tolerance))
}

fn relative_depth_score(answer: &M3SpatialQueryResponse, withheld: &M3QueryWithheldContext) -> Option<bool> {
  let claimed = answer.relative_depth_order?;
  Some(claimed == relative_depth_holdout(withheld))
}

fn unknown_refusal_score(answer: &M3SpatialQueryResponse, holdout: M3HoldoutVisibility) -> bool {
  match holdout {
    M3HoldoutVisibility::Visible => !answer.refused_or_unknown,
    M3HoldoutVisibility::NotVisible | M3HoldoutVisibility::Indeterminate => {
      matches!(answer.visibility, M3QueryVisibilityAnswer::Unknown | M3QueryVisibilityAnswer::NotVisible) || answer.refused_or_unknown
    }
  }
}

fn overconfidence_when_wrong(
  answer: &M3SpatialQueryResponse,
  visibility_class_correct: bool,
  projection_within_tolerance: Option<bool>,
) -> bool {
  let Some(confidence) = answer.confidence.as_ref() else {
    return false;
  };
  let wrong = !visibility_class_correct || projection_within_tolerance == Some(false);
  wrong && confidence.geometry > 0.8
}

fn leaked_withheld_facts(request: &M3QueryRequest, withheld: &M3QueryWithheldContext) -> Vec<String> {
  let Ok(json) = serde_json::to_string(request) else {
    return vec!["request_serialization_failed".to_string()];
  };
  let mut leaks = Vec::new();
  for frame in [&withheld.anchor_frame, &withheld.query_frame] {
    let pose = &frame.frame().player_pose.eye_position;
    for (label, value) in [("eye_x", pose.x), ("eye_y", pose.y), ("eye_z", pose.z)] {
      let exact = format!("{value:.6}");
      if json.contains(&exact) {
        leaks.push(label.to_string());
      }
    }
    if let Some(hit) = &frame.frame().raycast_hit {
      if json.contains(&hit.block_id) {
        leaks.push("raycast_block_id".to_string());
      }
    }
    for block in &frame.frame().nearby_blocks {
      if json.contains(&block.block_id) {
        leaks.push(format!("nearby_block:{}", block.block_id));
        break;
      }
    }
    if json.contains("view_matrix") {
      leaks.push("view_matrix".to_string());
    }
    if json.contains("nearby_blocks") {
      leaks.push("nearby_blocks".to_string());
    }
  }
  let target = withheld.target.block_pos;
  for coord in [target.x, target.y, target.z] {
    if json.contains(&format!("\"x\":{coord}")) || json.contains(&format!("\"x\": {coord}")) {
      leaks.push("target_block_coordinate".to_string());
      break;
    }
  }
  leaks.sort();
  leaks.dedup();
  leaks
}

fn find_view_index(session: &M2Session, role: M2ViewRole) -> Option<usize> {
  session.views.iter().position(|view| view.role == role)
}

fn default_query_text(anchor_claim_id: &str) -> String {
  format!("From the current viewpoint, where does anchor memory claim {anchor_claim_id} project on screen, and is it visible?")
}

fn rejected_response_report(query_observation_id: &str, errors: Vec<String>) -> M3QueryResponseReport {
  M3QueryResponseReport {
    schema_version: M3_QUERY_RESPONSE_REPORT_SCHEMA_VERSION,
    query_observation_id: query_observation_id.to_string(),
    status: M3QueryResponseStatus::Rejected,
    response: None,
    errors,
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::m2_multi_view::{M2ViewCaptureInput, build_m2_session_from_captures};
  use crate::spatial_memory_observation::{
    SpatialClaimKind, SpatialClaimStatus, SpatialConfidence, SpatialCoordinateSpace, SpatialMemoryClaim, SpatialMemoryWriteScope,
    SpatialSignalKind,
  };
  use crate::types::{NearbyBlock, PlayerPose, RaycastHit, Vec3, Viewport};

  const SCREENSHOT_ANCHOR: &str = "auv://runs/run-m3/artifacts/view-anchor.png";
  const SCREENSHOT_TRANSLATE: &str = "auv://runs/run-m3/artifacts/view-translate.png";
  const SCREENSHOT_REVISIT: &str = "auv://runs/run-m3/artifacts/view-revisit.png";

  fn identity_matrix() -> [f64; 16] {
    [
      1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
  }

  fn base_frame(spatial_frame_id: &str, eye: Vec3, yaw: f64, pitch: f64, timestamp_ms: u64) -> crate::types::MinecraftSpatialFrame {
    crate::types::MinecraftSpatialFrame {
      spatial_frame_id: spatial_frame_id.to_string(),
      world_tick: 1200,
      monotonic_timestamp_ms: timestamp_ms,
      telemetry_session_id: Some("session-m3".to_string()),
      viewport: Viewport::new(800, 600),
      view_matrix: identity_matrix(),
      projection_matrix: identity_matrix(),
      player_pose: PlayerPose {
        eye_position: eye,
        yaw,
        pitch,
      },
      raycast_hit: Some(RaycastHit {
        block_pos: BlockPosition::new(0, 0, 0),
        face: BlockFace::North,
        block_id: "minecraft:red_wool".to_string(),
      }),
      nearby_blocks: vec![NearbyBlock {
        block_pos: BlockPosition::new(0, 0, 0),
        block_id: "minecraft:red_wool".to_string(),
      }],
      nearby_entities: Vec::new(),
      inventory_summary: Vec::new(),
      screenshot_artifact_ref: None,
      mc_capture_skew_ms: Some(12),
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
        base_frame("frame-anchor", Vec3::new(0.0, 0.0, 5.0), 180.0, 0.0, 1_000),
        SCREENSHOT_ANCHOR,
        M2ViewRole::Anchor,
        Some(950),
      ),
      capture_input(
        base_frame("frame-translate", Vec3::new(0.0, 0.0, 0.0), 0.0, 0.0, 2_000),
        SCREENSHOT_TRANSLATE,
        M2ViewRole::Translate,
        Some(1_950),
      ),
      capture_input(
        base_frame("frame-revisit", Vec3::new(0.5, 0.0, 4.0), 175.0, -2.0, 3_000),
        SCREENSHOT_REVISIT,
        M2ViewRole::Revisit,
        Some(2_950),
      ),
    ];
    build_m2_session_from_captures(Some("m3-strafe".to_string()), &inputs).expect("strafe session")
  }

  fn yaw_only_session() -> M2Session {
    build_m2_session_from_captures(None, &yaw_only_session_inputs()).expect("yaw-only session")
  }

  fn yaw_only_session_inputs() -> Vec<M2ViewCaptureInput> {
    let eye = Vec3::new(0.0, 0.0, 5.0);
    vec![
      capture_input(base_frame("frame-yaw-a", eye, 90.0, -10.0, 1_000), SCREENSHOT_ANCHOR, M2ViewRole::Anchor, Some(950)),
      capture_input(base_frame("frame-yaw-b", eye, 120.0, -10.0, 2_000), SCREENSHOT_TRANSLATE, M2ViewRole::Translate, Some(1_950)),
      capture_input(base_frame("frame-yaw-c", eye, 150.0, -5.0, 3_000), SCREENSHOT_REVISIT, M2ViewRole::Revisit, Some(2_950)),
    ]
  }

  fn anchor_patch(observation_id: &str) -> SpatialHypothesisPatch {
    SpatialHypothesisPatch {
      schema_version: crate::SPATIAL_HYPOTHESIS_PATCH_SCHEMA_VERSION,
      observation_ids: vec![observation_id.to_string()],
      claims: vec![SpatialMemoryClaim {
        claim_id: "anchor-target".to_string(),
        kind: SpatialClaimKind::Object,
        description: "A red surface was noted near the center from the anchor view".to_string(),
        coordinate_space: SpatialCoordinateSpace::ScreenRelative,
        status: SpatialClaimStatus::Hypothesis,
        confidence: SpatialConfidence {
          appearance: 0.85,
          geometry: 0.4,
          metric_scale: 0.05,
          semantics: 0.3,
          world_registration: 0.0,
        },
        evidence_refs: vec![SpatialSignalKind::RgbScreenshot],
        unsupported_inferences: vec!["hidden_geometry".to_string()],
      }],
      unknowns: vec![
        "world_coordinate".to_string(),
        "hidden_geometry".to_string(),
      ],
      requested_follow_up_capture: None,
      write_scope: SpatialMemoryWriteScope::HypothesisOnly,
    }
  }

  fn session_input(session: M2Session) -> M3QuerySessionInput {
    let anchor_id =
      session.views.iter().find(|view| view.role == M2ViewRole::Anchor).map(|view| view.observation_id.clone()).expect("anchor view");
    M3QuerySessionInput {
      session,
      anchor_patch: anchor_patch(&anchor_id),
      query_view_role: M2ViewRole::Translate,
      target: M3ScoringTarget {
        anchor_claim_id: "anchor-target".to_string(),
        block_pos: BlockPosition::new(0, 0, 0),
        block_face: None,
      },
    }
  }

  fn holdout_response(prepared: &M3QueryPrepared) -> M3SpatialQueryResponse {
    let holdout = compute_holdout_answer(&prepared.withheld);
    M3SpatialQueryResponse {
      schema_version: M3_SPATIAL_QUERY_RESPONSE_SCHEMA_VERSION,
      query_observation_id: prepared.query_observation_id.clone(),
      anchor_claim_id: prepared.withheld.target.anchor_claim_id.clone(),
      visibility: match holdout.visibility {
        M3HoldoutVisibility::Visible => M3QueryVisibilityAnswer::Visible,
        M3HoldoutVisibility::NotVisible | M3HoldoutVisibility::Indeterminate => M3QueryVisibilityAnswer::Unknown,
      },
      screen_projection: holdout.screen_point.map(|point| M3ScreenProjectionClaim {
        x: point.x,
        y: point.y,
      }),
      relative_depth_order: Some(relative_depth_holdout(&prepared.withheld)),
      refused_or_unknown: !matches!(holdout.visibility, M3HoldoutVisibility::Visible),
      confidence: Some(SpatialConfidence {
        appearance: 0.7,
        geometry: 0.5,
        metric_scale: 0.05,
        semantics: 0.3,
        world_registration: 0.0,
      }),
    }
  }

  #[test]
  fn m3_rejects_ungated_yaw_only_m2_session() {
    let input = session_input(yaw_only_session());
    let error = prepare_m3_query_from_session(&input).expect_err("yaw-only session must fail M3 prep");
    assert!(matches!(error, M3QueryError::UngatedM2Session { .. }));
  }

  #[test]
  fn m3_query_request_json_has_no_engine_truth() {
    let input = session_input(strafe_session());
    let prepared = prepare_m3_query_from_session(&input).expect("gated session");
    let json = serde_json::to_string(&prepared.request).expect("serialize request");

    assert!(!json.contains("nearby_blocks"));
    assert!(!json.contains("view_matrix"));
    assert!(!json.contains("eye_position"));
    assert!(!json.contains("minecraft:red_wool"));
    assert!(json.contains("anchor-target"));
    assert!(json.contains("memory"));
  }

  #[test]
  fn m3_withheld_truth_is_required_but_not_serialized_on_model_path() {
    let input = session_input(strafe_session());
    let prepared = prepare_m3_query_from_session(&input).expect("gated session");
    let (anchor, query, target) = m3_query_withheld_context(&prepared);
    let verification =
      verify_m3_query_from_session(&input, &serde_json::to_vec(&holdout_response(&prepared)).expect("serialize response")).expect("verify");

    assert_eq!(target.block_pos, BlockPosition::new(0, 0, 0));
    assert_eq!(anchor.frame().spatial_frame_id, "frame-anchor");
    assert_eq!(query.frame().spatial_frame_id, "frame-translate");

    let verification_json = serde_json::to_string(&verification).expect("serialize verification");
    assert!(!verification_json.contains("minecraft:red_wool"));
    assert!(!verification_json.contains("view_matrix"));
    assert!(!verification_json.contains("nearby_blocks"));
    assert!(verification.score.is_some());
  }

  #[test]
  fn m3_correct_holdout_response_passes_gate() {
    let input = session_input(strafe_session());
    let prepared = prepare_m3_query_from_session(&input).expect("gated session");
    let response_json = serde_json::to_vec(&holdout_response(&prepared)).expect("serialize response");
    let verification = verify_m3_query_from_session(&input, &response_json).expect("verify");
    let score = verification.score.expect("score");

    assert!(score.visibility_class_correct);
    assert!(score.target_anchor_recall);
    assert!(score.request_leaks.is_empty());
    assert!(meets_m3_query_gate(&score));
  }

  #[test]
  fn m3_incorrect_visibility_fails_gate() {
    let input = session_input(strafe_session());
    let prepared = prepare_m3_query_from_session(&input).expect("gated session");
    let holdout = compute_holdout_answer(&prepared.withheld);
    let mut wrong = holdout_response(&prepared);
    match holdout.visibility {
      M3HoldoutVisibility::Visible => {
        wrong.visibility = M3QueryVisibilityAnswer::NotVisible;
        wrong.screen_projection = None;
        wrong.refused_or_unknown = false;
      }
      _ => {
        wrong.visibility = M3QueryVisibilityAnswer::Visible;
        wrong.screen_projection = Some(M3ScreenProjectionClaim { x: 400.0, y: 300.0 });
        wrong.refused_or_unknown = false;
      }
    }

    let verification = verify_m3_query_from_session(&input, &serde_json::to_vec(&wrong).expect("serialize")).expect("verify");
    let score = verification.score.expect("score");

    assert!(!score.visibility_class_correct);
    assert!(!meets_m3_query_gate(&score));
  }

  #[test]
  fn m3_incorrect_projection_fails_tolerance() {
    let input = session_input(strafe_session());
    let prepared = prepare_m3_query_from_session(&input).expect("gated session");
    let holdout = compute_holdout_answer(&prepared.withheld);
    assert_eq!(holdout.visibility, M3HoldoutVisibility::Visible, "fixture must produce a visible holdout for projection scoring");
    let truth = holdout.screen_point.expect("visible holdout needs a screen point");
    let mut wrong = holdout_response(&prepared);
    wrong.screen_projection = Some(M3ScreenProjectionClaim {
      x: truth.x + 500.0,
      y: truth.y + 500.0,
    });

    let verification = verify_m3_query_from_session(&input, &serde_json::to_vec(&wrong).expect("serialize")).expect("verify");
    let score = verification.score.expect("score");

    assert_eq!(score.projection_within_tolerance, Some(false));
    assert!(!meets_m3_query_gate(&score));
  }

  #[test]
  fn m3_verification_report_round_trips_through_json_persistence() {
    let input = session_input(strafe_session());
    let prepared = prepare_m3_query_from_session(&input).expect("gated session");
    let verification =
      verify_m3_query_from_session(&input, &serde_json::to_vec(&holdout_response(&prepared)).expect("serialize")).expect("verify");
    let path = tempfile::NamedTempFile::new().expect("temp report").into_temp_path();
    write_m3_query_verification_report(&path, &verification).expect("write report");
    let restored: M3QueryVerificationReport = auv_file::read_json_file(&path).expect("read report");
    assert_eq!(restored, verification);
  }

  #[test]
  #[ignore = "live M3 scoring requires local M2 session, telemetry tail, and AUV_M3_LIVE=1"]
  fn m3_live_translate_view_scores_fixture_response() {
    if std::env::var("AUV_M3_LIVE").ok().as_deref() != Some("1") {
      return;
    }

    let session_root = Path::new(r"F:\auv\.tmp\m2-session");
    let roles = [
      ("v01", M2ViewRole::Anchor),
      ("v02", M2ViewRole::Translate),
      ("v03", M2ViewRole::Revisit),
    ];
    let mut captures = Vec::with_capacity(roles.len());
    for (label, role) in roles {
      let view_dir = session_root.join(label);
      let meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(view_dir.join("capture-meta.json")).expect("read capture meta")).expect("parse meta");
      captures.push(M2ViewCaptureInput {
        telemetry_path: Some(view_dir.join("telemetry.jsonl")),
        frame: None,
        screenshot_artifact_ref: meta["screenshot_uri"].as_str().expect("uri").to_string(),
        capture_monotonic_timestamp_ms: meta["capture_monotonic_timestamp_ms"].as_u64(),
        role,
        input_history: Vec::new(),
      });
    }

    let session = build_m2_session_from_captures(Some("m3-live-session".to_string()), &captures).expect("live session");
    assert!(session.meets_m2_capture_gate);

    let anchor_id =
      session.views.iter().find(|view| view.role == M2ViewRole::Anchor).map(|view| view.observation_id.clone()).expect("anchor");
    let anchor_index = session.views.iter().position(|view| view.role == M2ViewRole::Anchor).expect("anchor index");
    let anchor_truth = crate::m2_multi_view::m2_session_withheld_truth(&session)[anchor_index].clone();
    let target_block = anchor_truth.frame().raycast_hit.as_ref().map(|hit| hit.block_pos).unwrap_or(BlockPosition::new(0, 0, 0));

    let input = M3QuerySessionInput {
      session,
      anchor_patch: anchor_patch(&anchor_id),
      query_view_role: M2ViewRole::Translate,
      target: M3ScoringTarget {
        anchor_claim_id: "anchor-target".to_string(),
        block_pos: target_block,
        block_face: None,
      },
    };
    let prepared = prepare_m3_query_from_session(&input).expect("prepare live query");
    let verification = verify_m3_query_from_session(&input, &serde_json::to_vec(&holdout_response(&prepared)).expect("serialize"))
      .expect("verify live query");
    let out_dir = Path::new(r"F:\auv\.tmp\m3-session");
    std::fs::create_dir_all(out_dir).expect("create out dir");
    write_m3_query_verification_report(&out_dir.join("m3-query-report.json"), &verification).expect("write live report");
    assert!(verification.score.as_ref().is_some_and(meets_m3_query_gate));
  }
}
