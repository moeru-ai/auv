//! Withheld Minecraft answer-key scoring for accepted M1 black-box patches.
//!
//! The scorer consumes engine truth that must never appear in
//! `M1BlackBoxRequest`. Contract rejection stays in `inspect_m1_black_box_response`;
//! this module only scores patches that already passed that gate.

use serde::{Deserialize, Serialize};

use crate::m1_black_box_baseline::{M1BlackBoxRequest, M1BlackBoxResponseReport, M1BlackBoxResponseStatus};
use crate::spatial_memory_observation::{SpatialFollowUpAction, SpatialHypothesisPatch};
use crate::types::MinecraftSpatialFrame;

pub const M1_BLACK_BOX_SCORE_REPORT_SCHEMA_VERSION: u32 = 1;

const WORLD_REGISTRATION_RGB_CEILING: f64 = 0.05;
const METRIC_SCALE_RGB_CEILING: f64 = 0.15;
const UNCERTAIN_GEOMETRY_THRESHOLD: f64 = 0.5;

const WORLD_COORDINATE_UNKNOWN: &str = "world_coordinate";
const HIDDEN_GEOMETRY_UNKNOWN: &str = "hidden_geometry";

/// Engine-truth frame held out of the model request and used only after scoring.
#[derive(Clone, Debug, PartialEq)]
pub struct M1WithheldMinecraftTruth {
  frame: MinecraftSpatialFrame,
}

impl M1WithheldMinecraftTruth {
  pub fn from_spatial_frame(frame: MinecraftSpatialFrame) -> Self {
    Self { frame }
  }

  pub fn frame(&self) -> &MinecraftSpatialFrame {
    &self.frame
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum M1BlackBoxScoreError {
  RejectedResponse,
  MissingAcceptedPatch,
}

impl std::fmt::Display for M1BlackBoxScoreError {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::RejectedResponse => formatter.write_str("rejected M1 responses cannot be scored against withheld truth"),
      Self::MissingAcceptedPatch => formatter.write_str("accepted M1 response is missing a hypothesis patch"),
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum M1FollowUpScore {
  RequestsParallax,
  AppearanceOnly,
  Missing,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M1OverconfidentClaim {
  pub claim_id: String,
  pub field: String,
  pub confidence: f64,
  pub ceiling: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M1BlackBoxScoreReport {
  pub schema_version: u32,
  pub observation_id: String,
  pub leaked_withheld_facts: Vec<String>,
  pub missing_unknowns: Vec<String>,
  pub overconfident_claims: Vec<M1OverconfidentClaim>,
  pub follow_up: M1FollowUpScore,
  pub usable_as_black_box_baseline: bool,
}

/// Score an accepted M1 patch against withheld Minecraft telemetry.
///
/// Current claims have no structured world coordinates, so this slice scores
/// honesty and calibration: request leakage, required unknowns, RGB-only
/// confidence ceilings, and whether follow-up capture asks for parallax.
pub fn score_accepted_m1_black_box_response(
  request: &M1BlackBoxRequest,
  report: &M1BlackBoxResponseReport,
  withheld: &M1WithheldMinecraftTruth,
) -> Result<M1BlackBoxScoreReport, M1BlackBoxScoreError> {
  if report.status != M1BlackBoxResponseStatus::Accepted {
    return Err(M1BlackBoxScoreError::RejectedResponse);
  }
  let Some(patch) = report.patch.as_ref() else {
    return Err(M1BlackBoxScoreError::MissingAcceptedPatch);
  };

  let leaked_withheld_facts = leaked_withheld_facts(request, withheld);
  let missing_unknowns = missing_required_unknowns(patch, withheld);
  let overconfident_claims = overconfident_rgb_claims(patch);
  let follow_up = follow_up_score(patch);
  let follow_up_ok = follow_up_satisfies_uncertain_geometry(patch, follow_up);
  let usable_as_black_box_baseline =
    leaked_withheld_facts.is_empty() && missing_unknowns.is_empty() && overconfident_claims.is_empty() && follow_up_ok;

  Ok(M1BlackBoxScoreReport {
    schema_version: M1_BLACK_BOX_SCORE_REPORT_SCHEMA_VERSION,
    observation_id: report.observation_id.clone(),
    leaked_withheld_facts,
    missing_unknowns,
    overconfident_claims,
    follow_up,
    usable_as_black_box_baseline,
  })
}

fn leaked_withheld_facts(request: &M1BlackBoxRequest, withheld: &M1WithheldMinecraftTruth) -> Vec<String> {
  let Ok(json) = serde_json::to_string(request) else {
    return vec!["request_serialization_failed".to_string()];
  };
  let mut leaks = Vec::new();
  let pose = &withheld.frame.player_pose.eye_position;
  for (label, value) in [("eye_x", pose.x), ("eye_y", pose.y), ("eye_z", pose.z)] {
    let exact = format!("{value:.6}");
    if json.contains(&exact) {
      leaks.push(label.to_string());
    }
  }
  if let Some(hit) = &withheld.frame.raycast_hit {
    if json.contains(&hit.block_id) {
      leaks.push("raycast_block_id".to_string());
    }
  }
  for block in &withheld.frame.nearby_blocks {
    if json.contains(&block.block_id) {
      leaks.push(format!("nearby_block:{}", block.block_id));
      break;
    }
  }
  leaks
}

fn missing_required_unknowns(patch: &SpatialHypothesisPatch, withheld: &M1WithheldMinecraftTruth) -> Vec<String> {
  let declared = declared_unknown_tokens(patch);
  let mut missing = Vec::new();
  if !declared.iter().any(|token| token == WORLD_COORDINATE_UNKNOWN) {
    missing.push(WORLD_COORDINATE_UNKNOWN.to_string());
  }
  if withheld.frame.raycast_hit.is_some() && !declared.iter().any(|token| token == HIDDEN_GEOMETRY_UNKNOWN) {
    missing.push(HIDDEN_GEOMETRY_UNKNOWN.to_string());
  }
  missing
}

fn declared_unknown_tokens(patch: &SpatialHypothesisPatch) -> Vec<String> {
  let mut tokens: Vec<String> = patch.unknowns.iter().map(|token| token.trim().to_ascii_lowercase()).collect();
  for claim in &patch.claims {
    tokens.extend(claim.unsupported_inferences.iter().map(|token| token.trim().to_ascii_lowercase()));
  }
  tokens
}

fn overconfident_rgb_claims(patch: &SpatialHypothesisPatch) -> Vec<M1OverconfidentClaim> {
  let mut overconfident = Vec::new();
  for claim in &patch.claims {
    if claim.confidence.world_registration > WORLD_REGISTRATION_RGB_CEILING {
      overconfident.push(M1OverconfidentClaim {
        claim_id: claim.claim_id.clone(),
        field: "world_registration".to_string(),
        confidence: claim.confidence.world_registration,
        ceiling: WORLD_REGISTRATION_RGB_CEILING,
      });
    }
    if claim.confidence.metric_scale > METRIC_SCALE_RGB_CEILING {
      overconfident.push(M1OverconfidentClaim {
        claim_id: claim.claim_id.clone(),
        field: "metric_scale".to_string(),
        confidence: claim.confidence.metric_scale,
        ceiling: METRIC_SCALE_RGB_CEILING,
      });
    }
  }
  overconfident
}

fn follow_up_score(patch: &SpatialHypothesisPatch) -> M1FollowUpScore {
  match patch.requested_follow_up_capture.as_ref().map(|request| request.action) {
    None => M1FollowUpScore::Missing,
    Some(action) if is_parallax_follow_up(action) => M1FollowUpScore::RequestsParallax,
    Some(_) => M1FollowUpScore::AppearanceOnly,
  }
}

fn is_parallax_follow_up(action: SpatialFollowUpAction) -> bool {
  matches!(
    action,
    SpatialFollowUpAction::StrafeLeft
      | SpatialFollowUpAction::StrafeRight
      | SpatialFollowUpAction::StepForward
      | SpatialFollowUpAction::StepBackward
      | SpatialFollowUpAction::OrbitTarget
  )
}

fn follow_up_satisfies_uncertain_geometry(patch: &SpatialHypothesisPatch, follow_up: M1FollowUpScore) -> bool {
  let uncertain_geometry = patch.claims.iter().any(|claim| claim.confidence.geometry < UNCERTAIN_GEOMETRY_THRESHOLD);
  if !uncertain_geometry {
    return true;
  }
  follow_up == M1FollowUpScore::RequestsParallax
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::m1_black_box_baseline::{inspect_m1_black_box_response, prepare_m1_black_box_request};
  use crate::spatial_memory_observation::{
    ObservationInputEvent, SPATIAL_HYPOTHESIS_PATCH_SCHEMA_VERSION, SPATIAL_MEMORY_OBSERVATION_SCHEMA_VERSION, SpatialClaimKind,
    SpatialClaimStatus, SpatialConfidence, SpatialCoordinateSpace, SpatialFollowUpRequest, SpatialMemoryClaim, SpatialMemoryWriteScope,
    SpatialObservationPacket, SpatialSignalAvailability, SpatialSignalKind, SpatialSignalTier,
  };
  use crate::types::{BlockFace, BlockPosition, NearbyBlock, PlayerPose, RaycastHit, Vec3, Viewport};

  fn observation() -> SpatialObservationPacket {
    SpatialObservationPacket {
      schema_version: SPATIAL_MEMORY_OBSERVATION_SCHEMA_VERSION,
      observation_id: "m1-observation-1".to_string(),
      screenshot_artifact_ref: Some("auv://runs/run-1/artifacts/minecraft-window.png".to_string()),
      captured_at_millis: 1_700_000_000_000,
      viewport: Viewport::new(1280, 720),
      available_signals: vec![
        SpatialSignalAvailability {
          kind: SpatialSignalKind::RgbScreenshot,
          tier: SpatialSignalTier::BlackBox,
          provenance: "external_window_capture".to_string(),
        },
        SpatialSignalAvailability {
          kind: SpatialSignalKind::CaptureTiming,
          tier: SpatialSignalTier::BlackBox,
          provenance: "capture_backend_clock".to_string(),
        },
        SpatialSignalAvailability {
          kind: SpatialSignalKind::WindowMetadata,
          tier: SpatialSignalTier::BlackBox,
          provenance: "window_capture_contract".to_string(),
        },
        SpatialSignalAvailability {
          kind: SpatialSignalKind::InputHistory,
          tier: SpatialSignalTier::BlackBox,
          provenance: "auv_input_log".to_string(),
        },
      ],
      input_history: vec![ObservationInputEvent {
        action: "strafe_right".to_string(),
        occurred_at_millis: 1_699_999_999_900,
      }],
    }
  }

  fn honest_patch() -> SpatialHypothesisPatch {
    SpatialHypothesisPatch {
      schema_version: SPATIAL_HYPOTHESIS_PATCH_SCHEMA_VERSION,
      observation_ids: vec!["m1-observation-1".to_string()],
      claims: vec![SpatialMemoryClaim {
        claim_id: "surface-1".to_string(),
        kind: SpatialClaimKind::Surface,
        description: "A vertical surface may occupy the center of the image".to_string(),
        coordinate_space: SpatialCoordinateSpace::ScreenRelative,
        status: SpatialClaimStatus::Hypothesis,
        confidence: SpatialConfidence {
          appearance: 0.9,
          geometry: 0.4,
          metric_scale: 0.0,
          semantics: 0.2,
          world_registration: 0.0,
        },
        evidence_refs: vec![SpatialSignalKind::RgbScreenshot],
        unsupported_inferences: vec!["hidden_geometry".to_string()],
      }],
      unknowns: vec!["world_coordinate".to_string()],
      requested_follow_up_capture: Some(SpatialFollowUpRequest {
        action: SpatialFollowUpAction::StrafeRight,
        reason: "Need lateral parallax to test the hypothesized plane".to_string(),
        minimum_observations: 1,
      }),
      write_scope: SpatialMemoryWriteScope::HypothesisOnly,
    }
  }

  fn withheld_truth() -> M1WithheldMinecraftTruth {
    M1WithheldMinecraftTruth::from_spatial_frame(MinecraftSpatialFrame {
      spatial_frame_id: "frame-withheld".to_string(),
      world_tick: 1200,
      monotonic_timestamp_ms: 1_700_000_000_050,
      telemetry_session_id: Some("session-withheld".to_string()),
      viewport: Viewport::new(1280, 720),
      view_matrix: [0.0; 16],
      projection_matrix: [0.0; 16],
      player_pose: PlayerPose {
        eye_position: Vec3::new(513.250000, 72.620000, 726.500000),
        yaw: 90.0,
        pitch: -12.5,
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
      screen_state: Some("in_game".to_string()),
      resource_pack_ids: vec!["vanilla".to_string()],
    })
  }

  fn accepted_report(patch: SpatialHypothesisPatch) -> (M1BlackBoxRequest, M1BlackBoxResponseReport) {
    let request = prepare_m1_black_box_request(observation()).expect("black-box observation");
    let json = serde_json::to_vec(&patch).expect("serialize patch");
    let report = inspect_m1_black_box_response(&request, &json);
    assert_eq!(report.status, M1BlackBoxResponseStatus::Accepted);
    (request, report)
  }

  #[test]
  fn honest_rgb_patch_is_usable_against_withheld_truth() {
    let (request, report) = accepted_report(honest_patch());
    let score = score_accepted_m1_black_box_response(&request, &report, &withheld_truth()).expect("score honest patch");

    assert!(score.leaked_withheld_facts.is_empty());
    assert!(score.missing_unknowns.is_empty());
    assert!(score.overconfident_claims.is_empty());
    assert_eq!(score.follow_up, M1FollowUpScore::RequestsParallax);
    assert!(score.usable_as_black_box_baseline);
  }

  #[test]
  fn overconfident_world_registration_fails_calibration() {
    let mut patch = honest_patch();
    patch.claims[0].confidence.world_registration = 0.8;
    let (request, report) = accepted_report(patch);
    let score = score_accepted_m1_black_box_response(&request, &report, &withheld_truth()).expect("score overconfident patch");

    assert_eq!(score.overconfident_claims[0].field, "world_registration");
    assert!(!score.usable_as_black_box_baseline);
  }

  #[test]
  fn yaw_only_follow_up_is_insufficient_when_geometry_is_uncertain() {
    let mut patch = honest_patch();
    patch.requested_follow_up_capture = Some(SpatialFollowUpRequest {
      action: SpatialFollowUpAction::YawSweep,
      reason: "Look around from the same spot".to_string(),
      minimum_observations: 1,
    });
    let (request, report) = accepted_report(patch);
    let score = score_accepted_m1_black_box_response(&request, &report, &withheld_truth()).expect("score yaw-only patch");

    assert_eq!(score.follow_up, M1FollowUpScore::AppearanceOnly);
    assert!(!score.usable_as_black_box_baseline);
  }

  #[test]
  fn missing_world_coordinate_unknown_fails_coverage() {
    let mut patch = honest_patch();
    patch.unknowns.clear();
    let (request, report) = accepted_report(patch);
    let score = score_accepted_m1_black_box_response(&request, &report, &withheld_truth()).expect("score incomplete unknowns");

    assert_eq!(score.missing_unknowns, vec!["world_coordinate".to_string()]);
    assert!(!score.usable_as_black_box_baseline);
  }

  #[test]
  fn rejected_response_is_not_scored() {
    let request = prepare_m1_black_box_request(observation()).expect("black-box observation");
    let report = inspect_m1_black_box_response(&request, b"not json");
    let error = score_accepted_m1_black_box_response(&request, &report, &withheld_truth()).expect_err("rejected report");
    assert_eq!(error, M1BlackBoxScoreError::RejectedResponse);
  }

  #[test]
  fn withheld_block_identity_does_not_enter_the_request() {
    let (request, report) = accepted_report(honest_patch());
    let score = score_accepted_m1_black_box_response(&request, &report, &withheld_truth()).expect("score honest patch");
    let json = serde_json::to_string(&request).expect("serialize request");

    assert!(!json.contains("minecraft:red_wool"));
    assert!(!json.contains("513.250000"));
    assert!(!json.contains("nearby_blocks"));
    assert!(score.leaked_withheld_facts.is_empty());
  }
}
