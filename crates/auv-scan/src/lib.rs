//! Temporal scan contracts, producers, frame and coverage IO, and read-side projections.
//!
//! The crate root is the only public import path. Implementation modules stay
//! private so one concept cannot be imported through both `auv_scan::Type` and
//! `auv_scan::module::Type`.
//!
//! ```
//! use auv_scan::ScanFrame;
//! ```
//!
//! ```compile_fail
//! use auv_scan::frame::ScanFrame;
//! ```

#[cfg(test)]
mod fixture;
#[cfg(test)]
mod scene_fixture_support;

mod association;
mod coverage;
mod coverage_wire;
mod frame;
mod frame_io;
mod lifecycle;
mod motion;
mod producer;
mod reader;
mod scene_state;
mod scene_state_inspect;
mod timeline;
mod tracks;

pub use association::{AssociationDiagnostic, AssociationResult, FrameObservation, associate_adjacent_frames};
pub use coverage::{CompletenessClaim, CoverageEntry, CoverageView, NegativeEvidence, build_coverage_view};
pub use coverage_wire::{
  CompletenessWire, CoverageArtifactError, CoverageEntryWire, NegativeEvidenceWire, SCAN_COVERAGE_ARTIFACT_FILE_NAME,
  SCAN_COVERAGE_ARTIFACT_ROLE, SCAN_COVERAGE_SCHEMA_VERSION, ScanCoverageWire, coverage_view_to_wire, read_coverage_artifact,
  write_coverage_artifact,
};
pub use frame::{SCAN_FRAME_SCHEMA_VERSION, ScanBounds, ScanFrame, ScanImageRef};
pub use frame_io::{ScanArtifactError, frame_artifact_file_name, read_frame_artifact, write_frame_artifact};
pub use lifecycle::{LifecycleError, LifecycleEvent, LifecycleVerdict, TransitionEvidence, evaluate_lifecycle};
pub use motion::{MotionError, MotionEstimate, MotionResult, MotionUnknown, estimate_viewport_motion};
#[cfg(feature = "live-capture")]
pub use producer::live::produce_frame_from_capture;
pub use producer::{
  CoverageProducerError, FrameCaptureMeta, ProducedCoverage, ProducedFrame, ProducedFrameBatch, ScanProducerError, bounds_to_scan_bounds,
  bounds_to_scan_bounds_f64, build_scan_frame, frame_from_capture, produce_coverage_from_fixture_dir, produce_frame_from_fixture_dir,
  produce_frames_from_fixture_dir, write_frame_with_image,
};
pub use reader::{
  ScanFrameBundle, ScanInspectError, load_scan_frames_from_dir, replay_scan_frames_from_dir, summarize_scan_frame_text,
  verify_frame_image_dimensions,
};
pub use scene_state::{
  ActionReadiness, IdentityAssessment, ObservationRequest, SceneDiagnostic, SceneDraftAnswers, SceneStateError, SceneStateInput,
  SceneStateProduct, TrackSceneSummary, VisibilityAssessment, build_scene_state_product, summarize_scene_state_text,
};
pub use scene_state_inspect::{
  CoverageInspectSource, SceneStateInspect, SceneStateListSummary, build_scene_state_inspect, format_scene_state_inspect_text,
  summarize_scene_state_inspect,
};
pub use timeline::{
  DIAG_INSUFFICIENT_FRAMES, DIAG_UNSUPPORTED_FRAME_COUNT, SCAN_TIMELINE_ARTIFACT_FILE_NAME, SCAN_TIMELINE_SCHEMA_VERSION, ScanTimelineWire,
  TimelineDiagnosticWire, TimelineError, TimelineMotionWire, TimelineSegmentWire, build_scan_timeline_from_bundle,
  format_scan_timeline_text, read_timeline_artifact, write_timeline_artifact,
};
pub use tracks::{
  AssociationDiagnosticWire, AssociationResultWire, DIAG_OBSERVATIONS_FRAME_MISMATCH, SCAN_TRACKS_ARTIFACT_FILE_NAME,
  SCAN_TRACKS_SCHEMA_VERSION, ScanTracksWire, TrackSegmentWire, TracksDiagnosticWire, TracksError, build_scan_tracks_from_bundle,
  format_scan_tracks_text, read_tracks_artifact, write_tracks_artifact,
};
