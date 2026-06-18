pub mod artifact;
pub mod bind;
pub mod dataset;
pub mod evidence;
pub mod ingest;
pub mod input_target;
pub mod measurement;
pub mod overlay;
pub mod prep;
pub mod projection;
pub mod sample_builder;
pub mod scene_packet;
pub mod session_observation;
pub mod types;
pub mod verify;

pub use artifact::{MinecraftProjectionArtifact, ProjectionViewportBounds};
pub use bind::{BoundSpatialFrame, bind_capture_to_frame};
pub use dataset::{
  SPATIAL_BUNDLE_SCHEMA_VERSION, SourceRunSummary, SpatialBundleArtifactRecord,
  SpatialBundleCounts, SpatialBundleDirectory, SpatialBundleInputs, SpatialBundleManifest,
  SpatialBundleOutput, SpatialBundleSourceArtifact, export_spatial_bundle,
};
pub use ingest::{
  LatestFrameScan, read_latest_spatial_frame, read_latest_spatial_frame_from_tail,
  scan_latest_spatial_frame,
};
pub use input_target::projected_window_point;
pub use measurement::{
  TEXTURE_SWEEP_REPORT_SCHEMA_VERSION, TextureSweepInputs, TextureSweepReport,
  TextureSweepReportRow, TextureSweepSample, TextureSweepSampleSet, TextureSweepSampleSource,
  TextureSweepThresholds, build_texture_sweep_report, evaluate_texture_sweep,
};
pub use overlay::render_projection_overlay;
pub use prep::{
  MINECRAFT_1_21_1_RESOURCE_PACK_FORMAT, TEXTURE_SWEEP_PREP_SCHEMA_VERSION,
  TEXTURE_SWEEP_PROFILE_DURATION_SECONDS, TextureSweepPreparationInputs,
  TextureSweepPreparationManifest, TextureSweepPreparationOutput, TextureSweepPreparedProfile,
  TextureSweepRunStep, prepare_texture_sweep_resource_packs,
};
pub use projection::MinecraftProjector;
pub use sample_builder::{
  TEXTURE_SWEEP_SAMPLE_BUILDER_GENERATOR, TextureSweepSampleBuildInputs,
  TextureSweepSampleBuildOutput, build_texture_sweep_samples_from_bundles,
};
pub use scene_packet::{
  SCENE_PACKET_SCHEMA_VERSION, ScenePacketCameraRecord, ScenePacketCounts, ScenePacketFramePayload,
  ScenePacketFrameRecord, ScenePacketInputs, ScenePacketManifest, ScenePacketOutput,
  export_3dgs_scene_packet,
};
pub use session_observation::{
  MinecraftSessionNode, MinecraftSessionObservation, MinecraftSessionObservationProvider,
  frame_to_session_observation,
};
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
