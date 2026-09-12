//! M1 black-box verification loop: prepare → inspect → score.
//!
//! Model transport stays outside this crate. Callers supply externally produced
//! response JSON after capture and telemetry binding.

use std::path::Path;

use auv_file::{JsonWriteOptions, write_json_file};
use serde::{Deserialize, Serialize};

use crate::m1_black_box_baseline::{M1BlackBoxRequest, M1BlackBoxResponseReport, M1BlackBoxResponseStatus, inspect_m1_black_box_response};
use crate::m1_black_box_observation::{M1BlackBoxObservationError, prepare_m1_black_box_from_telemetry_tail};
use crate::m1_black_box_scoring::{M1BlackBoxScoreReport, score_accepted_m1_black_box_response};
use crate::spatial_memory_observation::ObservationInputEvent;

pub const M1_BLACK_BOX_VERIFICATION_REPORT_SCHEMA_VERSION: u32 = 1;

/// Typed report for one M1 black-box verification pass.
///
/// Withheld Minecraft answer-key material is scored internally and is not
/// serialized into this artifact.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M1BlackBoxVerificationReport {
  pub schema_version: u32,
  pub observation_id: String,
  pub request: M1BlackBoxRequest,
  pub response: M1BlackBoxResponseReport,
  pub score: Option<M1BlackBoxScoreReport>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum M1BlackBoxVerificationError {
  Preparation(M1BlackBoxObservationError),
  Persistence(String),
}

impl std::fmt::Display for M1BlackBoxVerificationError {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Preparation(error) => write!(formatter, "M1 black-box verification preparation failed: {error}"),
      Self::Persistence(message) => write!(formatter, "M1 black-box verification report persistence failed: {message}"),
    }
  }
}

/// Run the M1 consumption/scoring loop for one telemetry tail frame and model response.
///
/// When `capture_monotonic_timestamp_ms` is `None`, the newest in-game sidecar frame
/// timestamp is used as the capture clock. Invoke JSON still has no monotonic clock;
/// callers with an externally aligned capture clock should pass `Some(...)`.
///
/// TODO(m1-vlm-transport): provider-neutral request artifact emission and raw response
/// capture stay outside this crate until an owner-approved transport boundary exists.
pub fn verify_m1_black_box_from_telemetry_tail(
  telemetry_path: &Path,
  screenshot_artifact_ref: String,
  capture_monotonic_timestamp_ms: Option<u64>,
  input_history: Vec<ObservationInputEvent>,
  response_json: &[u8],
) -> Result<M1BlackBoxVerificationReport, M1BlackBoxVerificationError> {
  let prepared =
    prepare_m1_black_box_from_telemetry_tail(telemetry_path, screenshot_artifact_ref, capture_monotonic_timestamp_ms, input_history)
      .map_err(M1BlackBoxVerificationError::Preparation)?;
  let response = inspect_m1_black_box_response(&prepared.request, response_json);
  let score = if response.status == M1BlackBoxResponseStatus::Accepted {
    Some(
      score_accepted_m1_black_box_response(&prepared.request, &response, &prepared.split.withheld)
        .expect("accepted response must score against withheld truth"),
    )
  } else {
    None
  };

  Ok(M1BlackBoxVerificationReport {
    schema_version: M1_BLACK_BOX_VERIFICATION_REPORT_SCHEMA_VERSION,
    observation_id: prepared.request.observation().observation_id.clone(),
    request: prepared.request,
    response,
    score,
  })
}

/// Persist a verification report as JSON for offline inspection or replay.
pub fn write_m1_black_box_verification_report(
  path: &Path,
  report: &M1BlackBoxVerificationReport,
) -> Result<(), M1BlackBoxVerificationError> {
  write_json_file(
    path,
    report,
    JsonWriteOptions {
      create_parent_dirs: true,
      trailing_newline: true,
    },
  )
  .map_err(|error| M1BlackBoxVerificationError::Persistence(format!("{error:?}")))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::ingest::read_latest_spatial_frame_from_tail;
  use crate::m1_black_box_scoring::M1FollowUpScore;
  use crate::types::{BlockFace, BlockPosition, NearbyBlock, PlayerPose, RaycastHit, Vec3, Viewport};
  use std::path::PathBuf;

  const FIXTURE_DIR: &str = "tests/fixtures/m1";
  const SCREENSHOT: &str = "auv://runs/run-1/artifacts/minecraft-window.png";

  fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_DIR).join(name)
  }

  fn read_fixture_bytes(name: &str) -> Vec<u8> {
    std::fs::read(fixture_path(name)).expect("read fixture")
  }

  fn verify_with_fixture(patch_name: &str) -> M1BlackBoxVerificationReport {
    verify_m1_black_box_from_telemetry_tail(
      &fixture_path("telemetry_in_game.jsonl"),
      SCREENSHOT.to_string(),
      Some(1_700),
      Vec::new(),
      &read_fixture_bytes(patch_name),
    )
    .expect("fixture verification")
  }

  #[test]
  fn honest_fixture_is_usable_without_request_leakage() {
    let report = verify_with_fixture("honest_patch.json");
    let request_json = serde_json::to_string(&report.request).expect("serialize request");
    let score = report.score.expect("honest patch must score");

    assert_eq!(report.response.status, M1BlackBoxResponseStatus::Accepted);
    assert!(score.usable_as_black_box_baseline);
    assert!(score.leaked_withheld_facts.is_empty());
    assert_eq!(score.follow_up, M1FollowUpScore::RequestsParallax);
    assert!(!request_json.contains("nearby_blocks"));
    assert!(!request_json.contains("minecraft:red_wool"));
    assert!(!request_json.contains("513.250000"));
    assert!(!request_json.contains("player_pose"));
  }

  #[test]
  fn overconfident_fixture_is_not_usable() {
    let report = verify_with_fixture("overconfident_patch.json");
    let score = report.score.expect("overconfident patch must score");

    assert_eq!(report.response.status, M1BlackBoxResponseStatus::Accepted);
    assert!(!score.usable_as_black_box_baseline);
    assert!(!score.overconfident_claims.is_empty());
  }

  #[test]
  fn yaw_only_fixture_is_not_usable() {
    let report = verify_with_fixture("yaw_only_patch.json");
    let score = report.score.expect("yaw-only patch must score");

    assert_eq!(report.response.status, M1BlackBoxResponseStatus::Accepted);
    assert!(!score.usable_as_black_box_baseline);
    assert_eq!(score.follow_up, M1FollowUpScore::AppearanceOnly);
  }

  #[test]
  fn missing_unknowns_fixture_is_not_usable() {
    let report = verify_with_fixture("missing_unknowns_patch.json");
    let score = report.score.expect("missing-unknown patch must score");

    assert_eq!(report.response.status, M1BlackBoxResponseStatus::Accepted);
    assert!(!score.usable_as_black_box_baseline);
    assert!(score.missing_unknowns.contains(&"world_coordinate".to_string()));
  }

  #[test]
  fn leak_hidden_geometry_fixture_is_not_usable() {
    let report = verify_with_fixture("leak_hidden_geometry_patch.json");
    let score = report.score.expect("leak patch must score");

    assert_eq!(report.response.status, M1BlackBoxResponseStatus::Accepted);
    assert!(!score.usable_as_black_box_baseline);
    assert_eq!(score.missing_unknowns, vec!["hidden_geometry".to_string()]);
  }

  #[test]
  fn malformed_response_is_rejected_without_score() {
    let report = verify_m1_black_box_from_telemetry_tail(
      &fixture_path("telemetry_in_game.jsonl"),
      SCREENSHOT.to_string(),
      Some(1_700),
      Vec::new(),
      b"not json",
    )
    .expect("verification still returns a typed report");

    assert_eq!(report.response.status, M1BlackBoxResponseStatus::Rejected);
    assert!(report.score.is_none());
  }

  #[test]
  fn default_capture_clock_uses_newest_in_game_frame_timestamp() {
    let report = verify_m1_black_box_from_telemetry_tail(
      &fixture_path("telemetry_in_game.jsonl"),
      SCREENSHOT.to_string(),
      None,
      Vec::new(),
      &read_fixture_bytes("honest_patch.json"),
    )
    .expect("default capture clock");

    assert_eq!(report.request.observation().captured_at_millis, 2_000);
  }

  #[test]
  fn verification_report_round_trips_through_json_persistence() {
    let report = verify_with_fixture("honest_patch.json");
    let path = tempfile::NamedTempFile::new().expect("temp report").into_temp_path();
    write_m1_black_box_verification_report(&path, &report).expect("write report");
    let restored: M1BlackBoxVerificationReport = auv_file::read_json_file(&path).expect("read report");

    assert_eq!(restored, report);
    let restored_json = serde_json::to_string(&restored).expect("serialize restored report");
    assert!(!restored_json.contains("nearby_blocks"));
    assert!(!restored_json.contains("player_pose"));
    assert!(!restored_json.contains("view_matrix"));
  }

  #[test]
  #[ignore = "live Windows invoke: set AUV_M1_LIVE_TELEMETRY, AUV_M1_LIVE_SCREENSHOT, and optional AUV_M1_LIVE_RESPONSE"]
  fn live_verify_invoke_capture_without_leaking_engine_truth() {
    let telemetry = std::path::PathBuf::from(std::env::var("AUV_M1_LIVE_TELEMETRY").expect("AUV_M1_LIVE_TELEMETRY"));
    let screenshot = std::env::var("AUV_M1_LIVE_SCREENSHOT").expect("AUV_M1_LIVE_SCREENSHOT");
    let response_path = std::env::var("AUV_M1_LIVE_RESPONSE").expect("AUV_M1_LIVE_RESPONSE");
    let capture_ms = match std::env::var("AUV_M1_LIVE_CAPTURE_MS") {
      Ok(value) if !value.is_empty() => Some(value.parse().expect("AUV_M1_LIVE_CAPTURE_MS")),
      _ => None,
    };
    let response_json = std::fs::read(&response_path).expect("read live response");

    let report = verify_m1_black_box_from_telemetry_tail(&telemetry, screenshot.clone(), capture_ms, Vec::new(), &response_json)
      .unwrap_or_else(|error| panic!("live verification failed: {error}"));
    let request_json = serde_json::to_string(&report.request).expect("serialize request");
    let withheld = read_latest_spatial_frame_from_tail(&telemetry).expect("read live telemetry").expect("live telemetry frame");
    let leaked_block = withheld.nearby_blocks.iter().map(|block| block.block_id.as_str()).find(|block_id| request_json.contains(block_id));

    assert_eq!(report.request.observation().screenshot_artifact_ref.as_deref(), Some(screenshot.as_str()));
    assert!(!withheld.nearby_blocks.is_empty(), "withheld engine frame must keep nearby_blocks");
    assert!(leaked_block.is_none(), "request leaked block id {leaked_block:?}");
    assert!(!request_json.contains("nearby_blocks"));
    assert!(!request_json.contains("player_pose"));
    assert!(!request_json.contains("view_matrix"));
    assert!(!request_json.contains("eye_position"));
    if report.response.status == M1BlackBoxResponseStatus::Accepted {
      let score = report.score.expect("accepted live response must score");
      assert!(score.leaked_withheld_facts.is_empty());
    }
  }

  #[test]
  fn fixture_telemetry_matches_scoring_withheld_shape() {
    let frame = read_latest_spatial_frame_from_tail(&fixture_path("telemetry_in_game.jsonl"))
      .expect("read fixture telemetry")
      .expect("fixture telemetry frame");
    assert_eq!(frame.spatial_frame_id, "frame-1200-99");
    assert_eq!(frame.player_pose.eye_position.x, 513.250000);
    assert_eq!(frame.raycast_hit.as_ref().map(|hit| hit.block_id.as_str()), Some("minecraft:red_wool"));
    assert_eq!(frame.nearby_blocks[0].block_id, "minecraft:smooth_stone");
    assert_eq!(frame.viewport, Viewport::new(1280, 720));
    assert_eq!(
      frame.player_pose,
      PlayerPose {
        eye_position: Vec3::new(513.250000, 72.620000, 726.500000),
        yaw: 90.0,
        pitch: -12.5,
      }
    );
    assert_eq!(
      frame.raycast_hit,
      Some(RaycastHit {
        block_pos: BlockPosition::new(520, 72, 730),
        face: BlockFace::North,
        block_id: "minecraft:red_wool".to_string(),
      })
    );
    assert_eq!(
      frame.nearby_blocks,
      vec![NearbyBlock {
        block_pos: BlockPosition::new(519, 72, 730),
        block_id: "minecraft:smooth_stone".to_string(),
      }]
    );
  }
}
