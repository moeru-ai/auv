//! Temporal scan contracts, pure evaluators, and fixture decoders.
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

mod association;
mod coverage;
mod coverage_artifact;
mod frame;
mod lifecycle;
mod motion;
mod producer;
mod reader;
mod scene_state;
mod scene_state_inspect;
mod timeline;
mod tracks;

pub use association::{AssociationDiagnostic, AssociationResult, FrameObservation, associate_adjacent_frames};
pub use coverage::{CoverageEntry, CoverageStatus, CoverageView, NegativeEvidence, build_coverage_view};
pub use coverage_artifact::ScanCoverageArtifact;
pub use frame::{SCAN_FRAME_SCHEMA_VERSION, ScanBounds, ScanFrame, ScanFrameError, ScanImageDimensions};
pub use lifecycle::{LifecycleError, LifecycleEvent, LifecycleVerdict, TransitionEvidence, evaluate_lifecycle};
pub use motion::{MotionError, MotionEstimate, MotionResult, MotionUnknown, estimate_viewport_motion};
pub use producer::{
  CoverageProducerError, FrameCaptureMeta, LoadedFrameFixture, ScanProducerError, bounds_to_scan_bounds, build_coverage_fixture,
  build_scan_frame, frame_from_capture, load_frame_fixture,
};
pub use reader::{ScanFrameBundle, summarize_scan_frame_text};
pub use scene_state::{
  ActionReadiness, IdentityAssessment, ObservationRequest, SceneDiagnostic, SceneDraftAnswers, SceneFrame, SceneStateError, SceneStateInput,
  SceneStateProduct, SceneTrackState, VisibilityAssessment, build_scene_state_product, summarize_scene_state_text,
};
pub use scene_state_inspect::{
  SceneStateInspect, SceneStateListSummary, build_scene_state_inspect, format_scene_state_inspect_text, summarize_scene_state_inspect,
};
pub use timeline::{
  DIAG_INSUFFICIENT_FRAMES, DIAG_UNSUPPORTED_FRAME_COUNT, SCAN_TIMELINE_SCHEMA_VERSION, ScanTimelineWire, TimelineDiagnosticWire,
  TimelineMotionWire, TimelineSegmentWire, build_scan_timeline_from_bundle, format_scan_timeline_text,
};
pub use tracks::{
  DIAG_OBSERVATIONS_FRAME_MISMATCH, SCAN_TRACKS_SCHEMA_VERSION, ScanTracksWire, TrackSegmentWire, TracksDiagnosticWire,
  build_scan_tracks_from_bundle, format_scan_tracks_text,
};
