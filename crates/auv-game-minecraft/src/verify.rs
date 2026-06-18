use serde::{Deserialize, Serialize};

use crate::types::{
  BlockPosition, MinecraftBlockTarget, MinecraftProjectedPoint, MinecraftSpatialFrame,
  ProjectionVisibility,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldDiffFailure {
  VerificationUnreliable,
  StateChangedNoMatch,
  SemanticMismatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MismatchRefusalReason {
  NotMinecraftWindow,
  ScreenshotUnavailable,
  ScreenshotUnbound,
  CaptureSkewUnreliable,
  ProjectedOutsideWindow,
  TargetBehindCamera,
  TargetOutOfFrustum,
  TargetOccluded,
  TelemetryUnreliable,
  MenuLoadingScreen,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MismatchRefusal {
  pub refused: bool,
  pub reason: Option<MismatchRefusalReason>,
  pub basis_frame_id: Option<String>,
  pub observed_block_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldDiffRequest {
  pub target: MinecraftBlockTarget,
  pub expected_item_id: Option<String>,
  pub allow_same_block_state_change: bool,
}

impl WorldDiffRequest {
  pub fn new(target: MinecraftBlockTarget) -> Self {
    Self {
      target,
      expected_item_id: None,
      allow_same_block_state_change: false,
    }
  }

  pub fn with_expected_item_id(mut self, expected_item_id: impl Into<String>) -> Self {
    self.expected_item_id = Some(expected_item_id.into());
    self
  }

  pub fn allow_same_block_state_change(mut self) -> Self {
    self.allow_same_block_state_change = true;
    self
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldDiffVerdict {
  pub executed: bool,
  pub state_changed: bool,
  pub semantic_matched: Option<bool>,
  pub failure: Option<WorldDiffFailure>,
  pub observed_block_id: Option<String>,
  pub observed_item_delta: Option<i64>,
}

impl WorldDiffVerdict {
  fn unreliable(observed_block_id: Option<String>, observed_item_delta: Option<i64>) -> Self {
    Self {
      executed: true,
      state_changed: false,
      semantic_matched: None,
      failure: Some(WorldDiffFailure::VerificationUnreliable),
      observed_block_id,
      observed_item_delta,
    }
  }
}

pub fn evaluate_mismatch_refusal(
  pre: &MinecraftSpatialFrame,
  projected: &MinecraftProjectedPoint,
  expected_target: &MinecraftBlockTarget,
  screenshot_is_minecraft_window: bool,
  max_capture_skew_ms: Option<i64>,
) -> MismatchRefusal {
  if !screenshot_is_minecraft_window {
    return MismatchRefusal {
      refused: true,
      reason: Some(MismatchRefusalReason::NotMinecraftWindow),
      basis_frame_id: Some(pre.spatial_frame_id.clone()),
      observed_block_id: target_block_id(pre, expected_target.block_pos),
    };
  }

  if pre.screenshot_artifact_ref.is_none() {
    return MismatchRefusal {
      refused: true,
      reason: Some(MismatchRefusalReason::ScreenshotUnavailable),
      basis_frame_id: Some(pre.spatial_frame_id.clone()),
      observed_block_id: target_block_id(pre, expected_target.block_pos),
    };
  }

  let Some(capture_skew_ms) = pre.mc_capture_skew_ms else {
    return MismatchRefusal {
      refused: true,
      reason: Some(MismatchRefusalReason::ScreenshotUnbound),
      basis_frame_id: Some(pre.spatial_frame_id.clone()),
      observed_block_id: target_block_id(pre, expected_target.block_pos),
    };
  };

  if let Some(limit_ms) = max_capture_skew_ms
    && capture_skew_ms.abs() > limit_ms
  {
    return MismatchRefusal {
      refused: true,
      reason: Some(MismatchRefusalReason::CaptureSkewUnreliable),
      basis_frame_id: Some(pre.spatial_frame_id.clone()),
      observed_block_id: target_block_id(pre, expected_target.block_pos),
    };
  }

  if let Some(scene) = pre.screen_state.as_deref() {
    if is_menu_scene(scene) {
      return MismatchRefusal {
        refused: true,
        reason: Some(MismatchRefusalReason::MenuLoadingScreen),
        basis_frame_id: Some(pre.spatial_frame_id.clone()),
        observed_block_id: target_block_id(pre, expected_target.block_pos),
      };
    }
  }

  let reason = match projected.visibility {
    ProjectionVisibility::Visible => {
      if projected.screen_point.is_none() {
        Some(MismatchRefusalReason::ProjectedOutsideWindow)
      } else if let Some(hit) = &pre.raycast_hit {
        if hit.block_pos != expected_target.block_pos {
          Some(MismatchRefusalReason::TargetOccluded)
        } else {
          None
        }
      } else {
        Some(MismatchRefusalReason::TelemetryUnreliable)
      }
    }
    ProjectionVisibility::BehindCamera => Some(MismatchRefusalReason::TargetBehindCamera),
    ProjectionVisibility::OutOfFrustum => Some(MismatchRefusalReason::TargetOutOfFrustum),
    ProjectionVisibility::OutsideWindow => Some(MismatchRefusalReason::ProjectedOutsideWindow),
  };

  MismatchRefusal {
    refused: reason.is_some(),
    reason,
    basis_frame_id: Some(pre.spatial_frame_id.clone()),
    observed_block_id: target_block_id(pre, expected_target.block_pos),
  }
}

pub fn evaluate_world_diff(
  pre: &MinecraftSpatialFrame,
  post: &MinecraftSpatialFrame,
  request: &WorldDiffRequest,
) -> WorldDiffVerdict {
  let observed_item_delta = request
    .expected_item_id
    .as_deref()
    .map(|item_id| inventory_delta(pre, post, item_id));

  if post.monotonic_timestamp_ms <= pre.monotonic_timestamp_ms {
    return WorldDiffVerdict::unreliable(
      target_block_id(post, request.target.block_pos),
      observed_item_delta,
    );
  }

  let Some(pre_witness) = pre_target_witness(pre, request.target.block_pos) else {
    return WorldDiffVerdict::unreliable(
      target_block_id(post, request.target.block_pos),
      observed_item_delta,
    );
  };

  let post_block_id = target_block_id(post, request.target.block_pos);
  let removed = is_removed(&pre_witness, post_block_id.as_deref());
  let same_block_state_change = request.allow_same_block_state_change
    && post.world_tick > pre.world_tick
    && post_block_id.as_deref() == Some(pre_witness.as_str());
  let state_changed = removed || same_block_state_change;
  let semantic_matched = request
    .expected_item_id
    .as_ref()
    .map(|_| removed && observed_item_delta.unwrap_or_default() > 0);

  let failure = if removed {
    match semantic_matched {
      Some(true) | None => None,
      Some(false) => Some(WorldDiffFailure::StateChangedNoMatch),
    }
  } else if same_block_state_change {
    None
  } else if observed_item_delta.unwrap_or_default() > 0 {
    Some(WorldDiffFailure::SemanticMismatch)
  } else {
    None
  };

  WorldDiffVerdict {
    executed: true,
    state_changed,
    semantic_matched,
    failure,
    observed_block_id: post_block_id,
    observed_item_delta,
  }
}

fn pre_target_witness(pre: &MinecraftSpatialFrame, block_pos: BlockPosition) -> Option<String> {
  if let Some(hit) = &pre.raycast_hit
    && hit.block_pos == block_pos
    && !is_air_block_id(&hit.block_id)
  {
    return Some(hit.block_id.clone());
  }

  target_block_id(pre, block_pos).filter(|block_id| !is_air_block_id(block_id))
}

fn target_block_id(frame: &MinecraftSpatialFrame, block_pos: BlockPosition) -> Option<String> {
  if let Some(hit) = &frame.raycast_hit
    && hit.block_pos == block_pos
  {
    return Some(hit.block_id.clone());
  }

  frame
    .nearby_blocks
    .iter()
    .find(|block| block.block_pos == block_pos)
    .map(|block| block.block_id.clone())
}

fn is_menu_scene(scene: &str) -> bool {
  matches!(
    scene,
    "menu" | "loading" | "pause_menu" | "loading_or_overlay"
  )
}

fn inventory_delta(
  pre: &MinecraftSpatialFrame,
  post: &MinecraftSpatialFrame,
  item_id: &str,
) -> i64 {
  inventory_count(post, item_id) - inventory_count(pre, item_id)
}

fn inventory_count(frame: &MinecraftSpatialFrame, item_id: &str) -> i64 {
  frame
    .inventory_summary
    .iter()
    .find(|entry| entry.item_id == item_id)
    .map(|entry| i64::from(entry.count))
    .unwrap_or_default()
}

fn is_removed(pre_block_id: &str, post_block_id: Option<&str>) -> bool {
  if is_air_block_id(pre_block_id) {
    return false;
  }

  match post_block_id {
    // NOTICE(mc3-nearby-block-radius): POST absence counts as removal only because PRE already witnessed a non-air block at the same target.
    None => true,
    Some(block_id) => is_air_block_id(block_id),
  }
}

fn is_air_block_id(block_id: &str) -> bool {
  block_id == "minecraft:air"
}

#[cfg(test)]
mod tests {
  use auv_driver::geometry::Point;

  use super::*;
  use crate::types::{
    BlockFace, BlockPosition, InventorySummaryEntry, NearbyBlock, NearbyEntity, PlayerPose,
    RaycastHit, Vec3, Viewport,
  };

  fn frame_at(
    world_tick: u64,
    timestamp_ms: u64,
    raycast_hit: Option<RaycastHit>,
    nearby_blocks: Vec<NearbyBlock>,
    inventory_summary: Vec<InventorySummaryEntry>,
  ) -> MinecraftSpatialFrame {
    MinecraftSpatialFrame {
      spatial_frame_id: format!("frame-{world_tick}"),
      world_tick,
      monotonic_timestamp_ms: timestamp_ms,
      viewport: Viewport::new(800, 600),
      view_matrix: [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
      ],
      projection_matrix: [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
      ],
      player_pose: PlayerPose {
        eye_position: Vec3::new(0.0, 0.0, 0.0),
        yaw: 0.0,
        pitch: 0.0,
      },
      raycast_hit,
      nearby_blocks,
      nearby_entities: vec![NearbyEntity {
        entity_id: "pig-1".to_string(),
        entity_kind: "minecraft:pig".to_string(),
      }],
      inventory_summary,
      screenshot_artifact_ref: Some("artifact://frame.png".to_string()),
      mc_capture_skew_ms: Some(0),
      screen_state: None,
      resource_pack_ids: Vec::new(),
    }
  }

  fn target() -> MinecraftBlockTarget {
    MinecraftBlockTarget {
      block_pos: BlockPosition::new(1, 2, 3),
      face: Some(BlockFace::North),
    }
  }

  fn witnessed_stone() -> RaycastHit {
    RaycastHit {
      block_pos: target().block_pos,
      face: BlockFace::North,
      block_id: "minecraft:stone".to_string(),
    }
  }

  fn visible_projection() -> MinecraftProjectedPoint {
    MinecraftProjectedPoint {
      screen_point: Some(Point::new(320.0, 240.0)),
      visibility: ProjectionVisibility::Visible,
      match_radius_px: 12.0,
      basis_frame_id: "frame-10".to_string(),
      confidence: 1.0,
    }
  }

  #[test]
  fn refuses_when_not_minecraft_window() {
    let refusal = evaluate_mismatch_refusal(
      &frame_at(10, 1_000, Some(witnessed_stone()), vec![], vec![]),
      &visible_projection(),
      &target(),
      false,
      Some(50),
    );

    assert_eq!(
      refusal.reason,
      Some(MismatchRefusalReason::NotMinecraftWindow)
    );
    assert!(refusal.refused);
  }

  #[test]
  fn refuses_when_screenshot_binding_is_missing() {
    let mut frame = frame_at(10, 1_000, Some(witnessed_stone()), vec![], vec![]);
    frame.screenshot_artifact_ref = None;

    let refusal =
      evaluate_mismatch_refusal(&frame, &visible_projection(), &target(), true, Some(50));

    assert_eq!(
      refusal.reason,
      Some(MismatchRefusalReason::ScreenshotUnavailable)
    );
    assert!(refusal.refused);
  }

  #[test]
  fn refuses_when_capture_binding_timestamp_is_missing() {
    let mut frame = frame_at(10, 1_000, Some(witnessed_stone()), vec![], vec![]);
    frame.mc_capture_skew_ms = None;

    let refusal =
      evaluate_mismatch_refusal(&frame, &visible_projection(), &target(), true, Some(50));

    assert_eq!(
      refusal.reason,
      Some(MismatchRefusalReason::ScreenshotUnbound)
    );
    assert!(refusal.refused);
  }
  #[test]
  fn refuses_when_capture_skew_exceeds_limit() {
    let mut frame = frame_at(10, 1_000, Some(witnessed_stone()), vec![], vec![]);
    frame.mc_capture_skew_ms = Some(120);

    let refusal =
      evaluate_mismatch_refusal(&frame, &visible_projection(), &target(), true, Some(50));

    assert_eq!(
      refusal.reason,
      Some(MismatchRefusalReason::CaptureSkewUnreliable)
    );
    assert!(refusal.refused);
  }

  #[test]
  fn refuses_when_screen_state_is_menu_before_geometry() {
    // A pause-menu frame whose target still projects in-frustum and on-target
    // must refuse with the menu reason, not fall through to a geometry verdict.
    let mut frame = frame_at(10, 1_000, Some(witnessed_stone()), vec![], vec![]);
    frame.screen_state = Some("menu".to_string());

    let refusal =
      evaluate_mismatch_refusal(&frame, &visible_projection(), &target(), true, Some(50));

    assert_eq!(
      refusal.reason,
      Some(MismatchRefusalReason::MenuLoadingScreen)
    );
    assert!(refusal.refused);
  }

  #[test]
  fn refuses_when_screen_state_is_loading_overlay() {
    let mut frame = frame_at(10, 1_000, Some(witnessed_stone()), vec![], vec![]);
    frame.screen_state = Some("loading_or_overlay".to_string());

    let refusal =
      evaluate_mismatch_refusal(&frame, &visible_projection(), &target(), true, Some(50));

    assert_eq!(
      refusal.reason,
      Some(MismatchRefusalReason::MenuLoadingScreen)
    );
    assert!(refusal.refused);
  }

  #[test]
  fn allows_in_game_screen_state_through_to_geometry() {
    // `in_game` must not trip the menu refusal; a clean visible+matching frame
    // still binds successfully.
    let mut frame = frame_at(10, 1_000, Some(witnessed_stone()), vec![], vec![]);
    frame.screen_state = Some("in_game".to_string());

    let refusal =
      evaluate_mismatch_refusal(&frame, &visible_projection(), &target(), true, Some(50));

    assert_eq!(refusal.reason, None);
    assert!(!refusal.refused);
  }

  #[test]
  fn refuses_when_projection_is_visible_but_outside_window_bounds() {
    let projected = MinecraftProjectedPoint {
      screen_point: None,
      visibility: ProjectionVisibility::Visible,
      match_radius_px: 12.0,
      basis_frame_id: "frame-10".to_string(),
      confidence: 1.0,
    };

    let refusal = evaluate_mismatch_refusal(
      &frame_at(10, 1_000, Some(witnessed_stone()), vec![], vec![]),
      &projected,
      &target(),
      true,
      Some(50),
    );

    assert_eq!(
      refusal.reason,
      Some(MismatchRefusalReason::ProjectedOutsideWindow)
    );
    assert!(refusal.refused);
  }

  #[test]
  fn refuses_when_target_is_occluded_by_other_raycast_hit() {
    let frame = frame_at(
      10,
      1_000,
      Some(RaycastHit {
        block_pos: BlockPosition::new(9, 9, 9),
        face: BlockFace::South,
        block_id: "minecraft:dirt".to_string(),
      }),
      vec![],
      vec![],
    );

    let refusal =
      evaluate_mismatch_refusal(&frame, &visible_projection(), &target(), true, Some(50));

    assert_eq!(refusal.reason, Some(MismatchRefusalReason::TargetOccluded));
    assert!(refusal.refused);
  }

  #[test]
  fn refuses_when_target_is_out_of_frustum() {
    let projected = MinecraftProjectedPoint {
      screen_point: None,
      visibility: ProjectionVisibility::OutOfFrustum,
      match_radius_px: 12.0,
      basis_frame_id: "frame-10".to_string(),
      confidence: 1.0,
    };

    let refusal = evaluate_mismatch_refusal(
      &frame_at(10, 1_000, Some(witnessed_stone()), vec![], vec![]),
      &projected,
      &target(),
      true,
      Some(50),
    );

    assert_eq!(
      refusal.reason,
      Some(MismatchRefusalReason::TargetOutOfFrustum)
    );
    assert!(refusal.refused);
  }

  #[test]
  fn world_diff_accepts_newer_sample_even_if_world_tick_is_same_when_block_is_removed() {
    let pre = frame_at(10, 1_000, Some(witnessed_stone()), vec![], vec![]);
    let post = frame_at(
      10,
      1_050,
      Some(RaycastHit {
        block_pos: target().block_pos,
        face: BlockFace::North,
        block_id: "minecraft:air".to_string(),
      }),
      vec![],
      vec![],
    );

    let verdict = evaluate_world_diff(&pre, &post, &WorldDiffRequest::new(target()));

    assert_eq!(verdict.executed, true);
    assert_eq!(verdict.state_changed, true);
    assert_eq!(verdict.failure, None);
    assert_eq!(verdict.observed_block_id.as_deref(), Some("minecraft:air"));
  }

  #[test]
  fn refuses_when_visible_projection_lacks_raycast_witness() {
    let refusal = evaluate_mismatch_refusal(
      &frame_at(10, 1_000, None, vec![], vec![]),
      &visible_projection(),
      &target(),
      true,
      Some(50),
    );

    assert_eq!(
      refusal.reason,
      Some(MismatchRefusalReason::TelemetryUnreliable)
    );
    assert!(refusal.refused);
  }

  #[test]
  fn allows_visible_bound_target_with_matching_raycast() {
    let refusal = evaluate_mismatch_refusal(
      &frame_at(10, 1_000, Some(witnessed_stone()), vec![], vec![]),
      &visible_projection(),
      &target(),
      true,
      Some(50),
    );

    assert_eq!(refusal.reason, None);
    assert!(!refusal.refused);
  }

  #[test]
  fn matches_when_block_disappears_and_inventory_rises() {
    let pre = frame_at(
      10,
      1_000,
      Some(witnessed_stone()),
      vec![NearbyBlock {
        block_pos: target().block_pos,
        block_id: "minecraft:stone".to_string(),
      }],
      vec![InventorySummaryEntry {
        item_id: "minecraft:stone".to_string(),
        count: 1,
      }],
    );
    let post = frame_at(
      11,
      1_050,
      None,
      vec![],
      vec![InventorySummaryEntry {
        item_id: "minecraft:stone".to_string(),
        count: 2,
      }],
    );
    let request = WorldDiffRequest::new(target()).with_expected_item_id("minecraft:stone");

    let verdict = evaluate_world_diff(&pre, &post, &request);

    assert_eq!(
      verdict,
      WorldDiffVerdict {
        executed: true,
        state_changed: true,
        semantic_matched: Some(true),
        failure: None,
        observed_block_id: None,
        observed_item_delta: Some(1),
      }
    );
  }

  #[test]
  fn reports_state_changed_no_match_when_inventory_stays_flat() {
    let pre = frame_at(
      10,
      1_000,
      Some(witnessed_stone()),
      vec![NearbyBlock {
        block_pos: target().block_pos,
        block_id: "minecraft:stone".to_string(),
      }],
      vec![InventorySummaryEntry {
        item_id: "minecraft:stone".to_string(),
        count: 1,
      }],
    );
    let post = frame_at(11, 1_050, None, vec![], vec![]);
    let request = WorldDiffRequest::new(target()).with_expected_item_id("minecraft:stone");

    let verdict = evaluate_world_diff(&pre, &post, &request);

    assert!(verdict.state_changed);
    assert_eq!(verdict.semantic_matched, Some(false));
    assert_eq!(verdict.failure, Some(WorldDiffFailure::StateChangedNoMatch));
    assert_eq!(verdict.observed_item_delta, Some(-1));
  }

  #[test]
  fn reports_semantic_mismatch_when_inventory_rises_but_block_remains() {
    let pre = frame_at(
      10,
      1_000,
      Some(witnessed_stone()),
      vec![NearbyBlock {
        block_pos: target().block_pos,
        block_id: "minecraft:stone".to_string(),
      }],
      vec![InventorySummaryEntry {
        item_id: "minecraft:stone".to_string(),
        count: 1,
      }],
    );
    let post = frame_at(
      11,
      1_050,
      Some(witnessed_stone()),
      vec![NearbyBlock {
        block_pos: target().block_pos,
        block_id: "minecraft:stone".to_string(),
      }],
      vec![InventorySummaryEntry {
        item_id: "minecraft:stone".to_string(),
        count: 2,
      }],
    );
    let request = WorldDiffRequest::new(target()).with_expected_item_id("minecraft:stone");

    let verdict = evaluate_world_diff(&pre, &post, &request);

    assert!(!verdict.state_changed);
    assert_eq!(verdict.semantic_matched, Some(false));
    assert_eq!(verdict.failure, Some(WorldDiffFailure::SemanticMismatch));
    assert_eq!(
      verdict.observed_block_id.as_deref(),
      Some("minecraft:stone")
    );
  }

  #[test]
  fn reports_unreliable_when_pre_witness_is_missing() {
    let pre = frame_at(10, 1_000, None, vec![], vec![]);
    let post = frame_at(11, 1_050, None, vec![], vec![]);
    let request = WorldDiffRequest::new(target()).with_expected_item_id("minecraft:stone");

    let verdict = evaluate_world_diff(&pre, &post, &request);

    assert_eq!(
      verdict,
      WorldDiffVerdict {
        executed: true,
        state_changed: false,
        semantic_matched: None,
        failure: Some(WorldDiffFailure::VerificationUnreliable),
        observed_block_id: None,
        observed_item_delta: Some(0),
      }
    );
  }

  #[test]
  fn treats_same_block_with_newer_tick_as_state_change_when_allowed() {
    let pre = frame_at(10, 1_000, Some(witnessed_stone()), vec![], vec![]);
    let post = frame_at(11, 1_050, Some(witnessed_stone()), vec![], vec![]);
    let request = WorldDiffRequest::new(target()).allow_same_block_state_change();

    let verdict = evaluate_world_diff(&pre, &post, &request);

    assert_eq!(verdict.executed, true);
    assert_eq!(verdict.state_changed, true);
    assert_eq!(verdict.failure, None);
    assert_eq!(
      verdict.observed_block_id.as_deref(),
      Some("minecraft:stone")
    );
  }

  #[test]
  fn reports_unreliable_when_post_tick_is_not_newer() {
    let pre = frame_at(
      10,
      1_000,
      Some(witnessed_stone()),
      vec![NearbyBlock {
        block_pos: target().block_pos,
        block_id: "minecraft:stone".to_string(),
      }],
      vec![],
    );
    let post = frame_at(10, 1_000, None, vec![], vec![]);
    let request = WorldDiffRequest::new(target());

    let verdict = evaluate_world_diff(&pre, &post, &request);

    assert_eq!(
      verdict.failure,
      Some(WorldDiffFailure::VerificationUnreliable)
    );
    assert!(!verdict.state_changed);
    assert_eq!(verdict.semantic_matched, None);
  }

  #[test]
  fn treats_minecraft_air_as_removed() {
    let pre = frame_at(
      10,
      1_000,
      Some(witnessed_stone()),
      vec![NearbyBlock {
        block_pos: target().block_pos,
        block_id: "minecraft:stone".to_string(),
      }],
      vec![],
    );
    let post = frame_at(
      11,
      1_050,
      Some(RaycastHit {
        block_pos: target().block_pos,
        face: BlockFace::North,
        block_id: "minecraft:air".to_string(),
      }),
      vec![NearbyBlock {
        block_pos: target().block_pos,
        block_id: "minecraft:air".to_string(),
      }],
      vec![],
    );
    let request = WorldDiffRequest::new(target());

    let verdict = evaluate_world_diff(&pre, &post, &request);

    assert!(verdict.state_changed);
    assert_eq!(verdict.failure, None);
  }
}
