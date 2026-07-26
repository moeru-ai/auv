#[cfg(feature = "tracing")]
mod run_read;

#[cfg(feature = "tracing")]
pub use run_read::{BALATRO_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT, BalatroArtifactPublishError};

mod blind_action;
pub mod cache;
#[cfg(feature = "card-corner-onnx")]
pub mod card_corner;
pub mod card_detection_eval_witness;
pub mod card_detection_producer;
pub mod card_detection_quality;
pub mod card_detection_semantic;
pub mod card_detection_spatial_query;
mod cards_action;
mod cards_clear;
mod cash_out;
pub mod cli;
pub mod config;
mod consumable_use;
pub mod detector;
mod game_restart;
mod hand_selection;
pub mod model;
mod object_sell;
pub mod observation;
pub mod output;
mod pack_choose;
mod pack_skip;
mod store_buy;
mod store_next_round;

pub use blind_action::{
  BlindSelectConfirmation, BlindSelectConfirmationFailure, BlindSelectRequest, BlindSelectResult, BlindSkipConfirmation,
  BlindSkipConfirmationFailure, BlindSkipRequest, BlindSkipResult,
};
pub use cards_action::{
  CardCommitAction, CardCommitChange, CardCommitChanges, CardCommitConfirmation, CardCommitConfirmationFailure, CardCommitKind,
  CardCommitRequest, CardCommitResult, CardCommitState, CardCommitStop, CardsSelectRequest, CardsSelectResult,
};
pub use cards_clear::{CardSelectionToggle, CardsClearIncompleteReason, CardsClearOutcome, CardsClearRequest, CardsClearResult};
pub use cash_out::{
  CashOutConfirmation, CashOutConfirmationBasis, CashOutConfirmationFailure, CashOutConfirmationRequest, CashOutRequest, CashOutResult,
};
pub use cli::{
  CliArgs, Command, OutputMode, blind_select, blind_skip, cards_clear, cards_discard, cards_play, cards_select, cash_out, consumable_use,
  game_restart, object_sell, pack_choose, pack_skip, store_buy, store_next_round,
};
pub use config::BalatroModelConfig;
pub use consumable_use::{
  ConsumableUseAction, ConsumableUseConfirmation, ConsumableUseConfirmationBasis, ConsumableUseConfirmationFailure, ConsumableUseControl,
  ConsumableUseRequest, ConsumableUseResult, ConsumableUseState, ConsumableUseStop,
};
pub use game_restart::{GameRestartClick, GameRestartOutcome, GameRestartRequest, GameRestartResult, GameRestartTarget};
pub use hand_selection::{HandSelectionResult, HandSelectionState, HandSelectionToggle, HandSelectionToggleKind};
pub use model::{
  BalatroPhase, BalatroState, ButtonTarget, CardSlot, ConsumableSlot, JokerSlot, ObjectZone, RoundState, ScoreState, SlotId, StoreItem,
  StoreState,
};
pub use object_sell::{
  ObjectSellClick, ObjectSellConfirmation, ObjectSellConfirmationBasis, ObjectSellConfirmationFailure, ObjectSellIncompleteReason,
  ObjectSellOutcome, ObjectSellRequest, ObjectSellResult, SellableObject,
};
pub use observation::{ObservationError, observe_image};
pub use pack_choose::{
  PackChoice, PackChoiceId, PackChooseAction, PackChooseConfirmation, PackChooseConfirmationBasis, PackChooseConfirmationFailure,
  PackChooseControl, PackChooseRequest, PackChooseResult, PackChooseState, PackChooseStop,
};
pub use pack_skip::{PackSkipConfirmation, PackSkipConfirmationFailure, PackSkipRequest, PackSkipResult};
pub use store_buy::{
  StoreBuyClick, StoreBuyConfirmation, StoreBuyConfirmationBasis, StoreBuyConfirmationFailure, StoreBuyIncompleteReason, StoreBuyOutcome,
  StoreBuyRequest, StoreBuyResult,
};
pub use store_next_round::{
  StoreNextRoundConfirmation, StoreNextRoundConfirmationFailure, StoreNextRoundConfirmationRequest, StoreNextRoundConfirmationStrength,
  StoreNextRoundRequest, StoreNextRoundResult, StoreNextRoundTarget,
};

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
