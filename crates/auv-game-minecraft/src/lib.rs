pub mod artifact;
pub mod input_target;
pub mod overlay;
pub mod projection;
pub mod types;
pub mod verify;

pub use artifact::{MinecraftProjectionArtifact, ProjectionViewportBounds};
pub use input_target::projected_window_point;
pub use overlay::render_projection_overlay;
pub use projection::MinecraftProjector;
pub use types::{
  BlockFace, BlockPosition, InventorySummaryEntry, MinecraftBlockTarget, MinecraftProjectedPoint,
  MinecraftSpatialFrame, NearbyBlock, NearbyEntity, PlayerPose, ProjectionVisibility, RaycastHit,
  Vec3, Viewport,
};
pub use verify::{
  MismatchRefusal, MismatchRefusalReason, WorldDiffFailure, WorldDiffRequest, WorldDiffVerdict,
  evaluate_mismatch_refusal, evaluate_world_diff,
};

// NOTICE(mc4-live-refusal): MC-4 refusal logic now closes crate-local mismatch cases that can be
// proven from projection visibility, telemetry ordering, pre/post witness quality, and screenshot
// binding metadata already present on `MinecraftSpatialFrame`; real client samples are still required
// before this can claim live acceptance coverage for occlusion, skew thresholds, or runtime wiring.
