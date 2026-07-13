pub mod artifact;
pub mod bind;
pub mod closed_scene_toy_fixture;
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
pub mod training_result;
pub mod training_result_artifact;
pub mod training_result_holdout_preview;
pub mod training_result_holdout_render_quality;
pub mod training_result_semantic;
pub mod training_result_spatial_query;
pub mod training_result_spatial_query_action;
pub mod training_result_spatial_query_action_wiring;
pub mod training_result_spatial_query_provider;
pub mod types;
pub mod verify;

pub use artifact::{MinecraftProjectionArtifact, ProjectionViewportBounds};
pub use bind::{BoundSpatialFrame, bind_capture_to_frame};
pub use dataset::{
  SPATIAL_BUNDLE_SCHEMA_VERSION, SourceRunSummary, SpatialBundleArtifactRecord, SpatialBundleCounts, SpatialBundleDirectory,
  SpatialBundleInputs, SpatialBundleManifest, SpatialBundleOutput, SpatialBundleSourceArtifact, export_spatial_bundle,
};
#[allow(deprecated)]
pub use ingest::{
  LatestFrameScan, TailFrameWaitConfig, read_latest_spatial_frame, read_latest_spatial_frame_from_tail,
  read_latest_spatial_frame_newer_than, scan_latest_spatial_frame,
};
pub use input_target::projected_window_point;
pub use measurement::{
  TEXTURE_SWEEP_REPORT_SCHEMA_VERSION, TextureSweepInputs, TextureSweepReport, TextureSweepReportRow, TextureSweepSample,
  TextureSweepSampleSet, TextureSweepSampleSource, TextureSweepThresholds, build_texture_sweep_report, evaluate_texture_sweep,
};
pub use overlay::render_projection_overlay;
pub use prep::{
  MINECRAFT_1_21_1_RESOURCE_PACK_FORMAT, TEXTURE_SWEEP_PREP_SCHEMA_VERSION, TEXTURE_SWEEP_PROFILE_DURATION_SECONDS,
  TextureSweepPreparationInputs, TextureSweepPreparationManifest, TextureSweepPreparationOutput, TextureSweepPreparedProfile,
  TextureSweepRunStep, prepare_texture_sweep_resource_packs,
};
pub use projection::MinecraftProjector;
pub use sample_builder::{
  TEXTURE_SWEEP_SAMPLE_BUILDER_GENERATOR, TextureSweepSampleBuildInputs, TextureSweepSampleBuildOutput,
  build_texture_sweep_samples_from_bundles,
};
pub use scene_packet::{
  SCENE_PACKET_INSPECT_REPORT_SCHEMA_VERSION, SCENE_PACKET_SCHEMA_VERSION, ScenePacketAnomalies, ScenePacketCameraRecord, ScenePacketCounts,
  ScenePacketFramePayload, ScenePacketFrameRecord, ScenePacketInputs, ScenePacketInspectCounts, ScenePacketInspectReport,
  ScenePacketManifest, ScenePacketOutput, ScenePacketResourcePackCoverage, export_3dgs_scene_packet,
};
pub use session_observation::{
  MinecraftSessionNode, MinecraftSessionObservation, MinecraftSessionObservationProvider, frame_to_session_observation,
};
pub use training_job::{
  TRAINING_JOB_INSPECT_REPORT_SCHEMA_VERSION, TRAINING_JOB_MANIFEST_SCHEMA_VERSION, TrainingJobEnvironment, TrainingLaunchJobBlocker,
  TrainingLaunchJobCounts, TrainingLaunchJobInputs, TrainingLaunchJobInspectReport, TrainingLaunchJobManifest, TrainingLaunchJobOutput,
  TrainingLaunchJobRequest, TrainingLaunchJobStatus, TrainingLaunchJobSubmission, launch_3dgs_training_job,
  launch_3dgs_training_job_with_environment,
};
pub use training_launch::{
  TRAINING_LAUNCH_INSPECT_REPORT_SCHEMA_VERSION, TRAINING_LAUNCH_PLAN_SCHEMA_VERSION, TrainingLaunchInspectReport,
  TrainingLaunchPlanManifest, TrainingLaunchPreparationInputs, TrainingLaunchPreparationOutput, TrainingLaunchReadiness,
  TrainingLaunchReadinessBlocker, prepare_3dgs_training_launch,
};
pub use training_package::{
  TRAINING_PACKAGE_INSPECT_REPORT_SCHEMA_VERSION, TRAINING_PACKAGE_SCHEMA_VERSION, TrainingCompatibilityFrameDecision,
  TrainingCompatibilitySkipReason, TrainingCompatibilitySkipReasonCount, TrainingCompatibilityStatus, TrainingCompatibilityViewReport,
  TrainingPackageCounts, TrainingPackageFrameRecord, TrainingPackageInputs, TrainingPackageInspectReport, TrainingPackageManifest,
  TrainingPackageOutput, export_3dgs_training_package,
};
pub use training_result::{
  TRAINING_RESULT_INSPECT_REPORT_SCHEMA_VERSION, TRAINING_RESULT_MANIFEST_SCHEMA_VERSION, TrainingResultArtifactRecord,
  TrainingResultEnvironment, TrainingResultInputs, TrainingResultInspectReport, TrainingResultManifest, TrainingResultOutput,
  TrainingResultReason, TrainingResultRequest, TrainingResultStatus, collect_3dgs_training_job_result,
  collect_3dgs_training_job_result_with_environment,
};
pub use training_result_artifact::{
  TRAINING_RESULT_ARTIFACT_FETCH_INSPECT_REPORT_SCHEMA_VERSION, TRAINING_RESULT_ARTIFACT_FETCH_MANIFEST_SCHEMA_VERSION,
  TrainingResultArtifactFetchEnvironment, TrainingResultArtifactFetchInputs, TrainingResultArtifactFetchInspectReport,
  TrainingResultArtifactFetchManifest, TrainingResultArtifactFetchOutput, TrainingResultArtifactFetchReason,
  TrainingResultArtifactFetchStatus, TrainingResultNormalizedArtifactKind, TrainingResultNormalizedArtifactRecord,
  fetch_3dgs_training_result_artifacts, fetch_3dgs_training_result_artifacts_with_command,
  fetch_3dgs_training_result_artifacts_with_environment,
};
pub use training_result_holdout_preview::{
  HoldoutFrameSelection, HoldoutFrameWitness, HoldoutPreviewAnswer, HoldoutPreviewReason, HoldoutPreviewRequest,
  MC16_V1_HOLDOUT_PREVIEW_KNOWN_LIMIT, TRAINING_RESULT_HOLDOUT_PREVIEW_INSPECT_REPORT_SCHEMA_VERSION,
  TRAINING_RESULT_HOLDOUT_PREVIEW_MANIFEST_SCHEMA_VERSION, TrainingResultHoldoutPreviewInputs, TrainingResultHoldoutPreviewInspectReport,
  TrainingResultHoldoutPreviewManifest, TrainingResultHoldoutPreviewOutput, inspect_3dgs_training_result_holdout,
};
pub use training_result_holdout_render_quality::{
  HoldoutRenderQualityAnswer, HoldoutRenderQualityBackend, HoldoutRenderQualityImageSize, HoldoutRenderQualityMetrics,
  HoldoutRenderQualityReason, HoldoutRenderQualityRequest, HoldoutRenderQualityVerdict, MC17_V1_HOLDOUT_RENDER_QUALITY_KNOWN_LIMIT,
  TRAINING_RESULT_HOLDOUT_RENDER_QUALITY_INSPECT_REPORT_SCHEMA_VERSION, TRAINING_RESULT_HOLDOUT_RENDER_QUALITY_MANIFEST_SCHEMA_VERSION,
  TrainingResultHoldoutRenderQualityInputs, TrainingResultHoldoutRenderQualityInspectReport, TrainingResultHoldoutRenderQualityManifest,
  TrainingResultHoldoutRenderQualityOutput, measure_3dgs_holdout_render_quality,
};
pub use training_result_semantic::{
  TRAINING_RESULT_SEMANTIC_INSPECT_REPORT_SCHEMA_VERSION, TRAINING_RESULT_SEMANTIC_MANIFEST_SCHEMA_VERSION,
  TrainingResultSemanticCheckpointRecord, TrainingResultSemanticInspectReport, TrainingResultSemanticManifest, TrainingResultSemanticReason,
  TrainingResultSemanticValidationInputs, TrainingResultSemanticValidationOutput, validate_3dgs_training_result,
};
pub use training_result_spatial_query::{
  TRAINING_RESULT_SPATIAL_QUERY_INSPECT_REPORT_SCHEMA_VERSION, TRAINING_RESULT_SPATIAL_QUERY_MANIFEST_SCHEMA_VERSION,
  TrainingResultSpatialQueryAnswer, TrainingResultSpatialQueryBackend, TrainingResultSpatialQueryComparisonVerdict,
  TrainingResultSpatialQueryInputs, TrainingResultSpatialQueryInspectReport, TrainingResultSpatialQueryKind,
  TrainingResultSpatialQueryManifest, TrainingResultSpatialQueryOutput, TrainingResultSpatialQueryReason, TrainingResultSpatialQueryRequest,
  TrainingResultSpatialQueryStatus, query_3dgs_training_result,
};
pub use training_result_spatial_query_action::{
  TrainingResultSpatialQueryActionEligibility, TrainingResultSpatialQueryActionReadiness, derive_action_readiness,
};
pub use training_result_spatial_query_action_wiring::{
  MC19_V1_D4_QUERY_WIRED_LIVE_ACTION_KNOWN_LIMIT, QueryActionWiringLineage, QueryActionWiringOutcome, QueryLiveClickExecutor,
  query_action_wiring_lineage_from_manifest, wire_query_manifest_to_action,
};
pub use training_result_spatial_query_provider::{
  MC15_V1_CHECKPOINT_NATIVE_KNOWN_LIMIT, MC18_V1_CLOSED_SCENE_TOY_KNOWN_LIMIT, MC18_V1_CLOSED_SCENE_TOY_NO_REFERENCE_LIMIT,
};
pub use types::{
  BlockFace, BlockPosition, InventorySummaryEntry, MinecraftBlockTarget, MinecraftProjectedPoint, MinecraftSpatialFrame,
  MinecraftTargetSemantics, NearbyBlock, NearbyEntity, PlayerPose, ProjectionVisibility, RaycastHit, Vec3, Viewport,
  mc6_projection_target_for_frame,
};
pub use verify::{
  MC20_V1_QUERY_WIRED_WITNESS_ABSENT_KNOWN_LIMIT, MismatchRefusal, MismatchRefusalReason, QueryWiredPostActionWitness, WorldDiffFailure,
  WorldDiffRequest, WorldDiffVerdict, evaluate_mismatch_refusal, evaluate_world_diff, verify_query_wired_live_action_semantic,
};

// NOTICE(mc4-live-refusal): MC-4 refusal logic now closes crate-local mismatch cases that can be
// proven from projection visibility, telemetry ordering, pre/post witness quality, and screenshot
// binding metadata already present on `MinecraftSpatialFrame`; real client samples are still required
// before this can claim live acceptance coverage for occlusion, skew thresholds, or runtime wiring.
