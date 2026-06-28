pub mod benchmark;
pub mod dataset;
pub mod projection;
pub mod visual_eval;
pub mod visual_truth;
pub mod visual_truth_semantic;
pub mod visual_truth_spatial_query;
pub mod visual_truth_spatial_query_action;

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

pub use visual_truth_semantic::{
  VisualTruthSemanticInspectReport, VisualTruthSemanticManifest, VisualTruthSemanticReason,
  VisualTruthSemanticStatus, VisualTruthSemanticValidationInputs,
  VisualTruthSemanticValidationOutput, validate_visual_truth_semantic,
};
pub use visual_truth_spatial_query::{
  VisualTruthPixelVisibility, VisualTruthSpatialQueryBackend, VisualTruthSpatialQueryInputs,
  VisualTruthSpatialQueryInspectReport, VisualTruthSpatialQueryManifest,
  VisualTruthSpatialQueryOutput, VisualTruthSpatialQueryReason, VisualTruthSpatialQueryStatus,
  query_visual_truth_spatial,
};
pub use visual_truth_spatial_query_action::{
  VisualTruthSpatialQueryActionEligibility, VisualTruthSpatialQueryActionReadiness,
  derive_visual_truth_spatial_query_action_readiness,
};
