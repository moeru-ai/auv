pub mod artifact_roles;
pub mod inspect;
pub mod run_read;

pub mod benchmark;
pub mod dataset;
pub mod detection_eval_quality;
pub mod detection_eval_witness;
pub mod projection;
pub mod visual_eval;
pub mod visual_truth;
pub mod visual_truth_semantic;
pub mod visual_truth_spatial_query;
pub mod visual_truth_spatial_query_action;
pub mod visual_truth_spatial_query_action_wiring;

pub use benchmark::{
  BenchmarkEvidenceSummary, BenchmarkInputs, BenchmarkOutput, CapturePhase, CaptureSample, CaptureTraceSample, DetectionEvalInputs,
  DetectionEvalManifest, DetectionEvalOutput, DispatchSample, LatencyReport, MapSummary, ObjectKind, RunMode, ScheduledAction,
  VerificationSummary, evaluate_detection_fixture, run_benchmark,
};
pub use dataset::{
  DatasetExportInputs, DatasetExportOutput, DatasetFrameRecord, DatasetLabelEntry, DatasetManifest, DatasetSkippedFrame, export_dataset,
};
pub use projection::{PlayfieldProjection, ProjectionArtifact, ProjectionBounds, ProjectionDerivationMethod};
pub use visual_eval::{
  EvalProjection, FrameDetections, FrameEvaluation, FrameKey, FrameLabelOutcome, FrameSpatialOutcome, LabelMap, VisualEvalReport,
  evaluate_visual_truth, iou,
};
pub use visual_truth::{CaptureFrame, ExpectedObjectTruth, VisualTruthFrame, VisualTruthManifest, build_visual_truth_manifest};

pub use detection_eval_quality::{
  DetectionEvalQualityInputs, DetectionEvalQualityInspectReport, DetectionEvalQualityManifest, DetectionEvalQualityMetrics,
  DetectionEvalQualityOutput, DetectionEvalQualityReason, DetectionEvalQualityStatus, DetectionEvalQualityVerdict,
  OSU_WQ1_V1_QUALITY_KNOWN_LIMIT, build_detection_eval_quality, build_detection_eval_quality_from_witness_dir,
  derive_detection_eval_quality_verdict,
};
pub use detection_eval_witness::{
  DetectionEvalFrameWitness, DetectionEvalWitnessInputs, DetectionEvalWitnessInspectReport, DetectionEvalWitnessManifest,
  DetectionEvalWitnessOutput, DetectionEvalWitnessReason, DetectionEvalWitnessStatus, OSU_WQ1_V1_WITNESS_KNOWN_LIMIT,
  build_detection_eval_witness,
};
pub use visual_truth_semantic::{
  VisualTruthSemanticInspectReport, VisualTruthSemanticManifest, VisualTruthSemanticReason, VisualTruthSemanticStatus,
  VisualTruthSemanticValidationInputs, VisualTruthSemanticValidationOutput, validate_visual_truth_semantic,
};
pub use visual_truth_spatial_query::{
  VisualTruthPixelVisibility, VisualTruthSpatialQueryBackend, VisualTruthSpatialQueryInputs, VisualTruthSpatialQueryInspectReport,
  VisualTruthSpatialQueryManifest, VisualTruthSpatialQueryOutput, VisualTruthSpatialQueryReason, VisualTruthSpatialQueryStatus,
  query_visual_truth_spatial,
};
pub use visual_truth_spatial_query_action::{
  VisualTruthSpatialQueryActionEligibility, VisualTruthSpatialQueryActionReadiness, derive_visual_truth_spatial_query_action_readiness,
};
pub use visual_truth_spatial_query_action_wiring::{
  OSU_QUERY_WIRED_LIVE_ACTION_KNOWN_LIMIT, VisualTruthQueryActionWiringLineage, VisualTruthQueryActionWiringOutcome,
  VisualTruthQueryLiveClickExecutor, visual_truth_query_action_wiring_lineage_from_manifest,
  wire_visual_truth_spatial_query_manifest_to_action,
};

pub use inspect::{inspect_sections_detection_eval, inspect_sections_primary};

pub use artifact_roles::*;
