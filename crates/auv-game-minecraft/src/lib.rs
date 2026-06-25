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
pub mod training_job;
pub mod training_launch;
pub mod training_package;
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
  SCENE_PACKET_INSPECT_REPORT_SCHEMA_VERSION, SCENE_PACKET_SCHEMA_VERSION, ScenePacketAnomalies,
  ScenePacketCameraRecord, ScenePacketCounts, ScenePacketFramePayload, ScenePacketFrameRecord,
  ScenePacketInputs, ScenePacketInspectCounts, ScenePacketInspectReport, ScenePacketManifest,
  ScenePacketOutput, ScenePacketResourcePackCoverage, export_3dgs_scene_packet,
};
pub use session_observation::{
  MinecraftSessionNode, MinecraftSessionObservation, MinecraftSessionObservationProvider,
  frame_to_session_observation,
};
pub use training_job::{
  TRAINING_JOB_INSPECT_REPORT_SCHEMA_VERSION, TRAINING_JOB_MANIFEST_SCHEMA_VERSION,
  TrainingLaunchJobBlocker, TrainingLaunchJobCounts, TrainingLaunchJobInputs,
  TrainingLaunchJobInspectReport, TrainingLaunchJobManifest, TrainingLaunchJobOutput,
  TrainingLaunchJobRequest, TrainingLaunchJobStatus, TrainingLaunchJobSubmission,
  launch_3dgs_training_job,
};
pub use training_launch::{
  TRAINING_LAUNCH_INSPECT_REPORT_SCHEMA_VERSION, TRAINING_LAUNCH_PLAN_SCHEMA_VERSION,
  TrainingLaunchInspectReport, TrainingLaunchPlanManifest, TrainingLaunchPreparationInputs,
  TrainingLaunchPreparationOutput, TrainingLaunchReadiness, TrainingLaunchReadinessBlocker,
  prepare_3dgs_training_launch,
};
pub use training_package::{
  TRAINING_PACKAGE_INSPECT_REPORT_SCHEMA_VERSION, TRAINING_PACKAGE_SCHEMA_VERSION,
  TrainingCompatibilityFrameDecision, TrainingCompatibilitySkipReason,
  TrainingCompatibilitySkipReasonCount, TrainingCompatibilityStatus,
  TrainingCompatibilityViewReport, TrainingPackageCounts, TrainingPackageFrameRecord,
  TrainingPackageInputs, TrainingPackageInspectReport, TrainingPackageManifest,
  TrainingPackageOutput, export_3dgs_training_package,
};
pub use types::{
  BlockFace, BlockPosition, InventorySummaryEntry, MinecraftBlockTarget, MinecraftProjectedPoint,
  MinecraftSpatialFrame, MinecraftTargetSemantics, NearbyBlock, NearbyEntity, PlayerPose,
  ProjectionVisibility, RaycastHit, Vec3, Viewport, mc6_projection_target_for_frame,
};
pub use verify::{
  MismatchRefusal, MismatchRefusalReason, WorldDiffFailure, WorldDiffRequest, WorldDiffVerdict,
  evaluate_mismatch_refusal, evaluate_world_diff,
};

// NOTICE(mc4-live-refusal): MC-4 refusal logic now closes crate-local mismatch cases that can be
// proven from projection visibility, telemetry ordering, pre/post witness quality, and screenshot
// binding metadata already present on `MinecraftSpatialFrame`; real client samples are still required
// before this can claim live acceptance coverage for occlusion, skew thresholds, or runtime wiring.
