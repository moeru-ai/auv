//! Temporal scan contracts — `scan-frame-v0` wire, artifact IO, producers, and read-side loader.

#[cfg(test)]
mod fixture;

pub mod artifact;
pub mod association;
pub mod coverage;
pub mod frame;
pub mod lifecycle;
pub mod motion;
pub mod producer;
pub mod reader;

pub use artifact::{
  ScanArtifactError, frame_artifact_file_name, read_frame_artifact, write_frame_artifact,
};
pub use association::{
  AssociationDiagnostic, AssociationResult, FrameObservation, associate_adjacent_frames,
};
pub use coverage::{
  CompletenessClaim, CoverageEntry, CoverageView, NegativeEvidence, build_coverage_view,
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
