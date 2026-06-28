pub mod cache;
#[cfg(feature = "card-corner-onnx")]
pub mod card_corner;
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

pub use cli::{CliArgs, Command, OutputMode};
pub use config::BalatroModelConfig;
pub use model::{
  BalatroPhase, BalatroState, ButtonTarget, CardSlot, ConsumableSlot, JokerSlot, ObjectZone,
  RoundState, ScoreState, SlotId, StoreItem, StoreState,
};
pub use observation::{ObservationError, observe_image};
pub use operation::{OperationRequest, OperationResult, VerificationMode, VerificationProfile};

pub use card_detection_producer::{
  CardDetectionBundleManifest, DETECTION_BUNDLE_FILE, EXPECTED_SLOTS_FILE, ExpectedSlotEntry,
  ExpectedSlotsManifest, LoadedDetectionBundle, load_detection_bundle, load_expected_slots,
};
pub use card_detection_quality::{
  BALATRO_X2_QUALITY_KNOWN_LIMIT, CardDetectionEvalReport, CardDetectionQualityBackend,
  CardDetectionQualityInputs, CardDetectionQualityInspectReport, CardDetectionQualityManifest,
  CardDetectionQualityMetrics, CardDetectionQualityOutput, CardDetectionQualityReason,
  CardDetectionQualityStatus, CardDetectionQualityVerdict, build_card_detection_quality,
  derive_card_detection_quality_verdict,
};
pub use card_detection_semantic::{
  CardDetectionSemanticInspectReport, CardDetectionSemanticManifest, CardDetectionSemanticReason,
  CardDetectionSemanticStatus, CardDetectionSemanticValidationInputs,
  CardDetectionSemanticValidationOutput, validate_card_detection_semantic,
};
pub use card_detection_spatial_query::{
  CardDetectionSpatialQueryBackend, CardDetectionSpatialQueryInputs,
  CardDetectionSpatialQueryInspectReport, CardDetectionSpatialQueryManifest,
  CardDetectionSpatialQueryOutput, CardDetectionSpatialQueryReason,
  CardDetectionSpatialQueryStatus, query_card_detection_spatial,
};
