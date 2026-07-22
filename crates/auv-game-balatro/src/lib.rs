pub mod inspect;
mod run_read;

pub use run_read::{BALATRO_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT, BalatroArtifactPublishError, BalatroArtifactReadError};

pub mod artifact_roles;
pub mod cache;
#[cfg(feature = "card-corner-onnx")]
pub mod card_corner;
pub mod card_detection_eval_witness;
pub mod card_detection_producer;
pub mod card_detection_quality;
pub mod card_detection_semantic;
pub mod card_detection_spatial_query;
pub mod cli;
pub mod config;
pub mod detector;
pub mod model;
pub mod observation;
pub mod operation;
pub mod output;

pub use artifact_roles::{
  BALATRO_CARD_DETECTION_EVAL_WITNESS_INSPECT_ROLE, BALATRO_CARD_DETECTION_EVAL_WITNESS_ROLE, BALATRO_CARD_DETECTION_QUALITY_INSPECT_ROLE,
  BALATRO_CARD_DETECTION_QUALITY_ROLE, BALATRO_CARD_DETECTION_SEMANTIC_INSPECT_ROLE, BALATRO_CARD_DETECTION_SEMANTIC_ROLE,
  BALATRO_CARD_DETECTION_SPATIAL_QUERY_INSPECT_ROLE, BALATRO_CARD_DETECTION_SPATIAL_QUERY_ROLE,
};
pub use cli::{CliArgs, Command, OutputMode};
pub use config::BalatroModelConfig;
pub use model::{
  BalatroPhase, BalatroState, ButtonTarget, CardSlot, ConsumableSlot, JokerSlot, ObjectZone, RoundState, ScoreState, SlotId, StoreItem,
  StoreState,
};
pub use observation::{ObservationError, observe_image};
pub use operation::{OperationRequest, OperationResult, VerificationMode, VerificationProfile};

pub use card_detection_eval_witness::{
  BALATRO_X4_WITNESS_KNOWN_LIMIT, CardDetectionEvalReport, CardDetectionEvalWitnessInputs, CardDetectionEvalWitnessInspectReport,
  CardDetectionEvalWitnessManifest, CardDetectionEvalWitnessOutput, CardDetectionEvalWitnessReason, CardDetectionQualityBackend,
  CardDetectionSlotScore, build_card_detection_eval_witness,
};
pub use card_detection_producer::{
  CardDetectionBundleManifest, DETECTION_BUNDLE_FILE, EXPECTED_SLOTS_FILE, ExpectedSlotEntry, ExpectedSlotsManifest, LoadedDetectionBundle,
  load_detection_bundle, load_expected_slots,
};
pub use card_detection_quality::{
  BALATRO_X2_QUALITY_KNOWN_LIMIT, BALATRO_X4_WITNESS_BOUND_QUALITY_KNOWN_LIMIT, CardDetectionQualityInputs,
  CardDetectionQualityInspectReport, CardDetectionQualityManifest, CardDetectionQualityMetrics, CardDetectionQualityOutput,
  CardDetectionQualityReason, CardDetectionQualityVerdict, build_card_detection_quality, build_card_detection_quality_from_witness_dir,
  derive_card_detection_quality_verdict,
};
pub use card_detection_semantic::{
  CardDetectionSemanticInspectReport, CardDetectionSemanticManifest, CardDetectionSemanticReason, CardDetectionSemanticValidationInputs,
  CardDetectionSemanticValidationOutput, validate_card_detection_semantic,
};
pub use card_detection_spatial_query::{
  CardDetectionSpatialQueryBackend, CardDetectionSpatialQueryInputs, CardDetectionSpatialQueryInspectReport,
  CardDetectionSpatialQueryManifest, CardDetectionSpatialQueryOutput, CardDetectionSpatialQueryReason, CardDetectionSpatialQueryStatus,
  query_card_detection_spatial,
};

pub use inspect::inspect_sections;
