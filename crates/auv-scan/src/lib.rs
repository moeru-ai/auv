//! Temporal scan contracts — `scan-frame-v0` wire, artifact IO, producers, and read-side loader.

#[cfg(test)]
mod fixture;
#[cfg(test)]
mod scene_fixture_support;

pub mod artifact;
pub mod association;
pub mod coverage;
pub mod coverage_artifact;
pub mod frame;
pub mod lifecycle;
pub mod motion;
pub mod producer;
pub mod reader;
pub mod scene_state;
pub mod scene_state_inspect;
pub mod timeline;

pub use artifact::{
  ScanArtifactError, frame_artifact_file_name, read_frame_artifact, write_frame_artifact,
};
pub use association::{
  AssociationDiagnostic, AssociationResult, FrameObservation, associate_adjacent_frames,
};
pub use coverage::{
  CompletenessClaim, CoverageEntry, CoverageView, NegativeEvidence, build_coverage_view,
};
pub use coverage_artifact::{
  CoverageArtifactError, SCAN_COVERAGE_ARTIFACT_FILE_NAME, SCAN_COVERAGE_SCHEMA_VERSION,
  ScanCoverageWire, coverage_view_to_wire, read_coverage_artifact, write_coverage_artifact,
};
pub use frame::{SCAN_FRAME_SCHEMA_VERSION, ScanBounds, ScanFrame, ScanImageRef};
pub use lifecycle::{
  LifecycleError, LifecycleEvent, LifecycleVerdict, TransitionEvidence, evaluate_lifecycle,
};
pub use motion::{
  MotionError, MotionEstimate, MotionResult, MotionUnknown, estimate_viewport_motion,
};
#[cfg(feature = "live-capture")]
pub use producer::live::produce_frame_from_capture;
pub use producer::{
  FrameCaptureMeta, ProducedFrame, ProducedFrameBatch, ScanProducerError, bounds_to_scan_bounds,
  bounds_to_scan_bounds_f64, build_scan_frame, frame_from_capture, produce_frame_from_fixture_dir,
  produce_frames_from_fixture_dir, write_frame_with_image,
};
pub use reader::{
  ScanFrameBundle, ScanInspectError, load_scan_frames_from_dir, replay_scan_frames_from_dir,
  summarize_scan_frame_text, verify_frame_image_dimensions,
};
pub use scene_state::{
  ActionReadiness, IdentityAssessment, ObservationRequest, SceneDiagnostic, SceneDraftAnswers,
  SceneStateError, SceneStateInput, SceneStateProduct, TrackSceneSummary, VisibilityAssessment,
  build_scene_state_product, summarize_scene_state_text,
};
pub use scene_state_inspect::{
  SceneStateInspect, SceneStateListSummary, build_scene_state_inspect,
  format_scene_state_inspect_text, summarize_scene_state_inspect,
};
pub use timeline::{
  DIAG_INSUFFICIENT_FRAMES, DIAG_UNSUPPORTED_FRAME_COUNT, SCAN_TIMELINE_ARTIFACT_FILE_NAME,
  SCAN_TIMELINE_SCHEMA_VERSION, ScanTimelineWire, TimelineDiagnosticWire, TimelineError,
  TimelineMotionWire, TimelineSegmentWire, build_scan_timeline_from_bundle,
  format_scan_timeline_text, read_timeline_artifact, write_timeline_artifact,
};
