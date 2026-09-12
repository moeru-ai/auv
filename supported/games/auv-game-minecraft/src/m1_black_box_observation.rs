//! Split a bound Minecraft capture into a black-box M1 observation and withheld truth.
//!
//! The observation is the only payload `prepare_m1_black_box_request` should see.
//! Engine pose, raycast, nearby blocks, matrices, inventory, and resource packs
//! stay on `M1WithheldMinecraftTruth`.

use std::path::Path;

use crate::bind::{BoundSpatialFrame, bind_capture_to_frame};
use crate::dataset::SourceArtifactUri;
use crate::ingest::read_latest_spatial_frame_from_tail;
use crate::m1_black_box_baseline::{M1BlackBoxRequest, M1BlackBoxRequestError, is_m1_input_action, prepare_m1_black_box_request};
use crate::m1_black_box_scoring::M1WithheldMinecraftTruth;
use crate::spatial_memory_observation::{
  ObservationInputEvent, SPATIAL_MEMORY_OBSERVATION_SCHEMA_VERSION, SpatialObservationPacket, SpatialSignalAvailability, SpatialSignalKind,
  SpatialSignalTier,
};

/// Sidecar `screen_state` for an unpaused world view. Pause (`menu`) and chunk
/// overlays (`loading_or_overlay`) are not M1 RGB observations.
pub const M1_IN_GAME_SCREEN_STATE: &str = "in_game";

/// Bound-frame split used to prepare an M1 request without feeding the scorer's
/// answer key into the model prompt.
#[derive(Clone, Debug, PartialEq)]
pub struct M1BlackBoxObservationSplit {
  pub observation: SpatialObservationPacket,
  pub withheld: M1WithheldMinecraftTruth,
}

/// Live tail ingest: black-box request plus the withheld engine frame used only
/// after `inspect_m1_black_box_response`.
#[derive(Clone, Debug, PartialEq)]
pub struct M1BlackBoxLivePreparation {
  pub split: M1BlackBoxObservationSplit,
  pub request: M1BlackBoxRequest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum M1BlackBoxObservationError {
  MissingScreenshot,
  InvalidScreenshotArtifactRef,
  ZeroViewport,
  InvalidInputAction { action: String },
  TelemetryRead(String),
  NoTelemetryFrame,
  ScreenNotInGame { screen_state: Option<String> },
  Prepare(M1BlackBoxRequestError),
}

impl std::fmt::Display for M1BlackBoxObservationError {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::MissingScreenshot => formatter.write_str("M1 observation split requires a bound screenshot artifact"),
      Self::InvalidScreenshotArtifactRef => formatter.write_str("M1 observation split requires a canonical AUV screenshot artifact URI"),
      Self::ZeroViewport => formatter.write_str("M1 observation split requires a non-zero viewport"),
      Self::InvalidInputAction { action } => write!(formatter, "M1 observation split has unsupported input action {action:?}"),
      Self::TelemetryRead(error) => write!(formatter, "M1 live observation failed to read telemetry: {error}"),
      Self::NoTelemetryFrame => formatter.write_str("M1 live observation requires a well-formed telemetry frame"),
      Self::ScreenNotInGame { screen_state } => match screen_state {
        Some(state) => write!(formatter, "M1 live observation requires screen_state {M1_IN_GAME_SCREEN_STATE}, found {state}"),
        None => write!(formatter, "M1 live observation requires screen_state {M1_IN_GAME_SCREEN_STATE}"),
      },
      Self::Prepare(error) => write!(formatter, "M1 live observation failed to prepare the black-box request: {error}"),
    }
  }
}

/// Project a bound telemetry frame onto the M1 black-box observation contract.
///
/// Input history is caller-supplied because the Fabric sidecar does not record
/// AUV input events. Pass an empty list when no trusted input log exists.
///
/// TODO(m1-auv-input-history): do not invent input events from player pose
/// deltas. Unlock when an AUV input log is bound to the same capture clock as
/// `BoundSpatialFrame`.
pub fn split_bound_frame_for_m1(
  bound: BoundSpatialFrame,
  input_history: Vec<ObservationInputEvent>,
) -> Result<M1BlackBoxObservationSplit, M1BlackBoxObservationError> {
  if bound.frame.viewport.width == 0 || bound.frame.viewport.height == 0 {
    return Err(M1BlackBoxObservationError::ZeroViewport);
  }
  let Some(screenshot_artifact_ref) =
    bound.frame.screenshot_artifact_ref.as_deref().map(str::trim).filter(|reference| !reference.is_empty())
  else {
    return Err(M1BlackBoxObservationError::MissingScreenshot);
  };
  if SourceArtifactUri::new(screenshot_artifact_ref).is_err() {
    return Err(M1BlackBoxObservationError::InvalidScreenshotArtifactRef);
  }
  if let Some(action) = input_history.iter().map(|event| event.action.as_str()).find(|action| !is_m1_input_action(action)) {
    return Err(M1BlackBoxObservationError::InvalidInputAction {
      action: action.to_string(),
    });
  }

  let mut available_signals = vec![
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
  ];
  if !input_history.is_empty() {
    available_signals.push(SpatialSignalAvailability {
      kind: SpatialSignalKind::InputHistory,
      tier: SpatialSignalTier::BlackBox,
      provenance: "auv_input_log".to_string(),
    });
  }

  let observation = SpatialObservationPacket {
    schema_version: SPATIAL_MEMORY_OBSERVATION_SCHEMA_VERSION,
    observation_id: format!("m1-{}", bound.frame.spatial_frame_id),
    screenshot_artifact_ref: Some(screenshot_artifact_ref.to_string()),
    captured_at_millis: capture_clock_millis(&bound),
    viewport: bound.frame.viewport,
    available_signals,
    input_history,
  };

  Ok(M1BlackBoxObservationSplit {
    observation,
    withheld: M1WithheldMinecraftTruth::from_spatial_frame(bound.frame),
  })
}

/// Read the newest sidecar frame, bind a caller-supplied screenshot, and build
/// the M1 request without copying engine truth into the observation.
///
/// Screenshot bytes stay on the capture producer. This function only accepts a
/// canonical `auv://runs/<id>/artifacts/<name>` URI and the capture clock from
/// that producer. `menu` and `loading_or_overlay` frames are refused so a pause
/// overlay cannot be scored as an in-world RGB observation.
///
/// NOTICE(m1-windows-invoke-capture): 2026-09-12 live `auv invoke window.capture
/// --title Minecraft` produced `backend=printwindow.windows`, no fallback, a
/// non-black in-game PNG, and a canonical `auv://runs/<id>/artifacts/<id>` URI.
/// Invoke JSON still has no capture monotonic clock; bind with the sidecar
/// frame timestamp (recorded skew 0) or an externally aligned clock. Do not add
/// WGC. Provenance enum replacement stays behind `TODO(m1-live-capture-vocabulary)`.
pub fn prepare_m1_black_box_from_telemetry_tail(
  telemetry_path: &Path,
  screenshot_artifact_ref: String,
  capture_monotonic_timestamp_ms: Option<u64>,
  input_history: Vec<ObservationInputEvent>,
) -> Result<M1BlackBoxLivePreparation, M1BlackBoxObservationError> {
  let frame = match read_latest_spatial_frame_from_tail(telemetry_path) {
    Ok(Some(frame)) => frame,
    Ok(None) => return Err(M1BlackBoxObservationError::NoTelemetryFrame),
    Err(error) => return Err(M1BlackBoxObservationError::TelemetryRead(error)),
  };
  if frame.screen_state.as_deref() != Some(M1_IN_GAME_SCREEN_STATE) {
    return Err(M1BlackBoxObservationError::ScreenNotInGame {
      screen_state: frame.screen_state,
    });
  }

  let capture_clock_ms = capture_monotonic_timestamp_ms.unwrap_or(frame.monotonic_timestamp_ms);
  let bound = bind_capture_to_frame(frame, Some(screenshot_artifact_ref), capture_clock_ms);
  let split = split_bound_frame_for_m1(bound, input_history)?;
  let request = prepare_m1_black_box_request(split.observation.clone()).map_err(M1BlackBoxObservationError::Prepare)?;
  Ok(M1BlackBoxLivePreparation { split, request })
}

fn capture_clock_millis(bound: &BoundSpatialFrame) -> u64 {
  let frame_ts = i64::try_from(bound.frame.monotonic_timestamp_ms).unwrap_or(i64::MAX);
  let capture_ts = frame_ts.saturating_sub(bound.capture_skew_ms);
  u64::try_from(capture_ts).unwrap_or(0)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::bind::bind_capture_to_frame;
  use crate::m1_black_box_baseline::{inspect_m1_black_box_response, prepare_m1_black_box_request};
  use crate::m1_black_box_scoring::score_accepted_m1_black_box_response;
  use crate::spatial_memory_observation::{
    SPATIAL_HYPOTHESIS_PATCH_SCHEMA_VERSION, SpatialClaimKind, SpatialClaimStatus, SpatialConfidence, SpatialCoordinateSpace,
    SpatialFollowUpAction, SpatialFollowUpRequest, SpatialHypothesisPatch, SpatialMemoryClaim, SpatialMemoryWriteScope,
  };
  use crate::types::{BlockFace, BlockPosition, MinecraftSpatialFrame, NearbyBlock, PlayerPose, RaycastHit, Vec3, Viewport};

  const SCREENSHOT: &str = "auv://runs/run-1/artifacts/minecraft-window.png";

  fn engine_frame() -> MinecraftSpatialFrame {
    MinecraftSpatialFrame {
      spatial_frame_id: "frame-1200-99".to_string(),
      world_tick: 1200,
      monotonic_timestamp_ms: 2_000,
      telemetry_session_id: Some("session-withheld".to_string()),
      viewport: Viewport::new(1280, 720),
      view_matrix: [1.0; 16],
      projection_matrix: [2.0; 16],
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
    }
  }

  fn bound_frame() -> BoundSpatialFrame {
    bind_capture_to_frame(engine_frame(), Some(SCREENSHOT.to_string()), 1_700)
  }

  fn honest_patch(observation_id: &str) -> SpatialHypothesisPatch {
    SpatialHypothesisPatch {
      schema_version: SPATIAL_HYPOTHESIS_PATCH_SCHEMA_VERSION,
      observation_ids: vec![observation_id.to_string()],
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

  #[test]
  fn split_keeps_engine_truth_out_of_the_observation() {
    let split = split_bound_frame_for_m1(bound_frame(), Vec::new()).expect("split bound frame");
    let json = serde_json::to_string(&split.observation).expect("serialize observation");

    assert_eq!(split.observation.observation_id, "m1-frame-1200-99");
    assert_eq!(split.observation.captured_at_millis, 1_700);
    assert_eq!(split.observation.screenshot_artifact_ref.as_deref(), Some(SCREENSHOT));
    assert!(!split.observation.has_signal(SpatialSignalKind::InputHistory));
    assert!(!json.contains("513.250000"));
    assert!(!json.contains("minecraft:red_wool"));
    assert!(!json.contains("nearby_blocks"));
    assert!(!json.contains("view_matrix"));
    assert!(!json.contains("player_pose"));
    assert_eq!(split.withheld.frame().player_pose.eye_position.x, 513.250000);
    assert_eq!(split.withheld.frame().raycast_hit.as_ref().map(|hit| hit.block_id.as_str()), Some("minecraft:red_wool"));
  }

  #[test]
  fn split_feeds_prepare_and_score_without_request_leakage() {
    let split = split_bound_frame_for_m1(
      bound_frame(),
      vec![ObservationInputEvent {
        action: "strafe_right".to_string(),
        occurred_at_millis: 1_650,
      }],
    )
    .expect("split bound frame");
    let request = prepare_m1_black_box_request(split.observation.clone()).expect("black-box request");
    let json = serde_json::to_vec(&honest_patch(&split.observation.observation_id)).expect("serialize patch");
    let report = inspect_m1_black_box_response(&request, &json);
    let score = score_accepted_m1_black_box_response(&request, &report, &split.withheld).expect("score split observation");
    let request_json = serde_json::to_string(&request).expect("serialize request");

    assert!(split.observation.has_signal(SpatialSignalKind::InputHistory));
    assert!(score.usable_as_black_box_baseline);
    assert!(!request_json.contains("minecraft:red_wool"));
    assert!(!request_json.contains("513.250000"));
    assert!(!request_json.contains("nearby_blocks"));
  }

  #[test]
  fn missing_screenshot_is_rejected() {
    let bound = bind_capture_to_frame(engine_frame(), None, 1_700);
    let error = split_bound_frame_for_m1(bound, Vec::new()).expect_err("missing screenshot");
    assert_eq!(error, M1BlackBoxObservationError::MissingScreenshot);
  }

  #[test]
  fn non_canonical_screenshot_uri_is_rejected() {
    let bound = bind_capture_to_frame(engine_frame(), Some("artifact://shot.png".to_string()), 1_700);
    let error = split_bound_frame_for_m1(bound, Vec::new()).expect_err("invalid uri");
    assert_eq!(error, M1BlackBoxObservationError::InvalidScreenshotArtifactRef);
  }

  #[test]
  fn invented_input_action_is_rejected() {
    let error = split_bound_frame_for_m1(
      bound_frame(),
      vec![ObservationInputEvent {
        action: "open_inventory".to_string(),
        occurred_at_millis: 1_650,
      }],
    )
    .expect_err("unsupported action");
    assert_eq!(
      error,
      M1BlackBoxObservationError::InvalidInputAction {
        action: "open_inventory".to_string(),
      }
    );
  }

  fn write_telemetry(frames: &[MinecraftSpatialFrame]) -> tempfile::NamedTempFile {
    let file = tempfile::NamedTempFile::new().expect("temp telemetry");
    let mut body = String::new();
    for frame in frames {
      body.push_str(&serde_json::to_string(frame).expect("serialize frame"));
      body.push('\n');
    }
    std::fs::write(file.path(), body).expect("write telemetry");
    file
  }

  #[test]
  fn live_tail_in_game_frame_prepares_without_leaking_engine_truth() {
    let telemetry = write_telemetry(&[engine_frame()]);
    let prepared = prepare_m1_black_box_from_telemetry_tail(telemetry.path(), SCREENSHOT.to_string(), Some(1_700), Vec::new())
      .expect("in-game tail prepares");
    let request_json = serde_json::to_string(&prepared.request).expect("serialize request");

    assert_eq!(prepared.split.observation.observation_id, "m1-frame-1200-99");
    assert_eq!(prepared.split.observation.captured_at_millis, 1_700);
    assert_eq!(prepared.request.observation().screenshot_artifact_ref.as_deref(), Some(SCREENSHOT));
    assert_eq!(prepared.split.withheld.frame().nearby_blocks[0].block_id, "minecraft:smooth_stone");
    assert!(!request_json.contains("minecraft:red_wool"));
    assert!(!request_json.contains("minecraft:smooth_stone"));
    assert!(!request_json.contains("513.250000"));
    assert!(!request_json.contains("nearby_blocks"));
    assert!(!request_json.contains("view_matrix"));
    assert!(!request_json.contains("player_pose"));
  }

  #[test]
  fn live_tail_uses_the_newest_in_game_frame() {
    let mut older = engine_frame();
    older.spatial_frame_id = "frame-older".to_string();
    older.monotonic_timestamp_ms = 1_000;
    let mut newest = engine_frame();
    newest.spatial_frame_id = "frame-newest".to_string();
    newest.monotonic_timestamp_ms = 3_000;
    let telemetry = write_telemetry(&[older, newest]);
    let prepared = prepare_m1_black_box_from_telemetry_tail(telemetry.path(), SCREENSHOT.to_string(), Some(2_900), Vec::new())
      .expect("newest in-game frame");
    assert_eq!(prepared.split.observation.observation_id, "m1-frame-newest");
    assert_eq!(prepared.split.observation.captured_at_millis, 2_900);
  }

  #[test]
  fn live_tail_refuses_pause_menu_frames() {
    let mut paused = engine_frame();
    paused.screen_state = Some("menu".to_string());
    let telemetry = write_telemetry(&[paused]);
    let error = prepare_m1_black_box_from_telemetry_tail(telemetry.path(), SCREENSHOT.to_string(), Some(1_700), Vec::new())
      .expect_err("pause menu is not an M1 RGB observation");
    assert_eq!(
      error,
      M1BlackBoxObservationError::ScreenNotInGame {
        screen_state: Some("menu".to_string()),
      }
    );
  }

  #[test]
  fn live_tail_refuses_loading_overlay_frames() {
    let mut loading = engine_frame();
    loading.screen_state = Some("loading_or_overlay".to_string());
    let telemetry = write_telemetry(&[loading]);
    let error = prepare_m1_black_box_from_telemetry_tail(telemetry.path(), SCREENSHOT.to_string(), Some(1_700), Vec::new())
      .expect_err("loading overlay is not an M1 RGB observation");
    assert_eq!(
      error,
      M1BlackBoxObservationError::ScreenNotInGame {
        screen_state: Some("loading_or_overlay".to_string()),
      }
    );
  }

  #[test]
  fn live_tail_refuses_empty_telemetry() {
    let telemetry = write_telemetry(&[]);
    let error = prepare_m1_black_box_from_telemetry_tail(telemetry.path(), SCREENSHOT.to_string(), Some(1_700), Vec::new())
      .expect_err("empty telemetry");
    assert_eq!(error, M1BlackBoxObservationError::NoTelemetryFrame);
  }

  #[test]
  fn live_tail_refuses_malformed_only_telemetry() {
    let file = tempfile::NamedTempFile::new().expect("temp telemetry");
    std::fs::write(file.path(), "not-json\n   \n").expect("write malformed telemetry");
    let error = prepare_m1_black_box_from_telemetry_tail(file.path(), SCREENSHOT.to_string(), Some(1_700), Vec::new())
      .expect_err("malformed telemetry has no usable frame");
    assert_eq!(error, M1BlackBoxObservationError::NoTelemetryFrame);
  }

  #[test]
  fn live_sidecar_json_shape_prepares_without_leaking_block_ids() {
    let line = concat!(
      r#"{"spatial_frame_id":"frame-3060-29712432673300","world_tick":3060,"monotonic_timestamp_ms":29712432,"#,
      r#""telemetry_session_id":"d663d314-1213-4a19-a486-7c8b39379a03","viewport":{"width":854,"height":480},"#,
      r#""view_matrix":[-1,0,0,0,0,1,0,0,0,0,-1,0,0,0,0,1],"projection_matrix":[0.8,0,0,0,0,1.43,0,0,0,0,-1, -1,0,0,-0.1,0],"#,
      r#""player_pose":{"eye_position":{"x":-22.662026,"y":82.620000,"z":39.552317},"yaw":12.5,"pitch":-8.25},"#,
      r#""raycast_hit":null,"nearby_blocks":[{"block_pos":{"x":-23,"y":80,"z":40},"block_id":"minecraft:grass_block"}],"#,
      r#""inventory_summary":[],"resource_pack_ids":["vanilla","fabric"],"screen_state":"in_game"}"#,
    );
    let file = tempfile::NamedTempFile::new().expect("temp telemetry");
    std::fs::write(file.path(), format!("{line}\n")).expect("write sidecar line");
    let prepared = prepare_m1_black_box_from_telemetry_tail(file.path(), SCREENSHOT.to_string(), Some(29_712_000), Vec::new())
      .expect("sidecar-shaped in-game line prepares");
    let request_json = serde_json::to_string(&prepared.request).expect("serialize request");

    assert_eq!(prepared.split.observation.viewport.width, 854);
    assert_eq!(prepared.split.withheld.frame().nearby_blocks[0].block_id, "minecraft:grass_block");
    assert!(!request_json.contains("minecraft:grass_block"));
    assert!(!request_json.contains("-22.662026"));
  }

  #[test]
  #[ignore = "live Windows invoke: set AUV_M1_LIVE_TELEMETRY and AUV_M1_LIVE_SCREENSHOT"]
  fn live_invoke_artifact_uri_prepares_without_leaking_engine_truth() {
    let telemetry = std::path::PathBuf::from(std::env::var("AUV_M1_LIVE_TELEMETRY").expect("AUV_M1_LIVE_TELEMETRY"));
    let screenshot = std::env::var("AUV_M1_LIVE_SCREENSHOT").expect("AUV_M1_LIVE_SCREENSHOT");
    let capture_ms = match std::env::var("AUV_M1_LIVE_CAPTURE_MS") {
      Ok(value) if !value.is_empty() => Some(value.parse().expect("AUV_M1_LIVE_CAPTURE_MS")),
      _ => None,
    };

    let prepared = prepare_m1_black_box_from_telemetry_tail(&telemetry, screenshot.clone(), capture_ms, Vec::new())
      .unwrap_or_else(|error| panic!("live prepare failed: {error}"));
    let request_json = serde_json::to_string(&prepared.request).expect("serialize request");
    let withheld = prepared.split.withheld.frame();
    let leaked_block = withheld.nearby_blocks.iter().map(|block| block.block_id.as_str()).find(|block_id| request_json.contains(block_id));

    assert_eq!(prepared.request.observation().screenshot_artifact_ref.as_deref(), Some(screenshot.as_str()));
    assert!(!withheld.nearby_blocks.is_empty(), "withheld engine frame must keep nearby_blocks");
    assert!(leaked_block.is_none(), "request leaked block id {leaked_block:?}");
    assert!(!request_json.contains("nearby_blocks"));
    assert!(!request_json.contains("player_pose"));
    assert!(!request_json.contains("view_matrix"));
    assert!(!request_json.contains("eye_position"));
  }
}
