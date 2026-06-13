pub mod benchmark;
pub mod dataset;
pub mod projection;
pub mod visual_eval;
pub mod visual_truth;

pub use benchmark::{
  BenchmarkEvidenceSummary, BenchmarkInputs, BenchmarkOutput, CapturePhase, CaptureSample,
  CaptureTraceSample, DetectionEvalInputs, DetectionEvalManifest, DetectionEvalOutput,
  DispatchSample, LatencyReport, MapSummary, ObjectKind, RunMode, ScheduledAction,
  VerificationSummary, evaluate_detection_fixture, run_benchmark,
};
pub use dataset::{
  DatasetExportInputs, DatasetExportOutput, DatasetFrameRecord, DatasetLabelEntry, DatasetManifest,
  DatasetSkippedFrame, export_dataset,
};
pub use projection::{
  PlayfieldProjection, ProjectionArtifact, ProjectionBounds, ProjectionDerivationMethod,
};
pub use visual_eval::{
  EvalProjection, FrameDetections, FrameEvaluation, FrameKey, FrameLabelOutcome,
  FrameSpatialOutcome, LabelMap, VisualEvalReport, evaluate_visual_truth, iou,
};
pub use visual_truth::{
  CaptureFrame, ExpectedObjectTruth, VisualTruthFrame, VisualTruthManifest,
  build_visual_truth_manifest,
};
