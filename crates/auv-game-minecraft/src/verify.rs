use serde::{Deserialize, Serialize};

use crate::types::{BlockPosition, MinecraftBlockTarget, MinecraftProjectedPoint, MinecraftSpatialFrame, ProjectionVisibility};

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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
  evaluate_mismatch_refusal_with_capture(
    pre,
    projected,
    expected_target,
    pre.screenshot_artifact_ref.is_some(),
    screenshot_is_minecraft_window,
    max_capture_skew_ms,
  )
}

pub(crate) fn evaluate_mismatch_refusal_with_capture(
  pre: &MinecraftSpatialFrame,
  projected: &MinecraftProjectedPoint,
  expected_target: &MinecraftBlockTarget,
  screenshot_available: bool,
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

  if !screenshot_available {
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

pub fn evaluate_world_diff(pre: &MinecraftSpatialFrame, post: &MinecraftSpatialFrame, request: &WorldDiffRequest) -> WorldDiffVerdict {
  let observed_item_delta = request.expected_item_id.as_deref().map(|item_id| inventory_delta(pre, post, item_id));

  if post.monotonic_timestamp_ms <= pre.monotonic_timestamp_ms {
    return WorldDiffVerdict::unreliable(target_block_id(post, request.target.block_pos), observed_item_delta);
  }

  let Some(pre_witness) = pre_target_witness(pre, request.target.block_pos) else {
    return WorldDiffVerdict::unreliable(target_block_id(post, request.target.block_pos), observed_item_delta);
  };

  let post_block_id = target_block_id(post, request.target.block_pos);
  let removed = is_removed(&pre_witness, post_block_id.as_deref());
  let same_block_state_change =
    request.allow_same_block_state_change && post.world_tick > pre.world_tick && post_block_id.as_deref() == Some(pre_witness.as_str());
  let state_changed = removed || same_block_state_change;
  let semantic_matched = request.expected_item_id.as_ref().map(|_| removed && observed_item_delta.unwrap_or_default() > 0);

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

  frame.nearby_blocks.iter().find(|block| block.block_pos == block_pos).map(|block| block.block_id.clone())
}

fn is_menu_scene(scene: &str) -> bool {
  matches!(scene, "menu" | "loading" | "pause_menu" | "loading_or_overlay")
}

fn inventory_delta(pre: &MinecraftSpatialFrame, post: &MinecraftSpatialFrame, item_id: &str) -> i64 {
  inventory_count(post, item_id) - inventory_count(pre, item_id)
}

fn inventory_count(frame: &MinecraftSpatialFrame, item_id: &str) -> i64 {
  frame.inventory_summary.iter().find(|entry| entry.item_id == item_id).map(|entry| i64::from(entry.count)).unwrap_or_default()
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
