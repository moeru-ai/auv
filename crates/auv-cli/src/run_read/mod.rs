//! Product read-side helpers: public query-wired adapters plus crate-local
//! access to ordinary donor readers.

mod query_wired_live_action;
mod query_wired_projection;

pub use self::query_wired_live_action::*;

use auv_game_minecraft::run_read::{
  MinecraftTrainingResultSpatialQueryManifestLineage, derive_minecraft_training_result_spatial_query_action_readiness,
  extract_minecraft_training_result_spatial_query_manifests,
};
use auv_game_osu::run_read::{
  OsuVisualTruthSpatialQueryManifestLineage, derive_osu_visual_truth_spatial_query_action_readiness,
  extract_osu_visual_truth_spatial_query_manifests,
};
use auv_inspect_model::{ArtifactRefView, artifact_record_view, is_json_mime, read_artifact_json};

pub(crate) use crate::integrations::minecraft::query_live_action::QUERY_WIRED_LIVE_ACTION_OPERATION_ID;
pub(crate) use crate::integrations::osu::query_live_action::QUERY_WIRED_LIVE_ACTION_OPERATION_ID as OSU_QUERY_WIRED_LIVE_ACTION_OPERATION_ID;
pub(crate) use auv_runtime::contract::{
  FailureLayer, OperationOutput, OperationResult, OperationStatus, VerificationMethod, VerificationResult,
};
pub(crate) use auv_runtime::model::AuvResult;
pub(crate) use auv_tracing_driver::store::{CanonicalRun, LocalStore};
