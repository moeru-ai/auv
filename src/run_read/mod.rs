//! Canonical root-owned tracing artifact producers.
//!
//! NOTICE(auv-inspector): this module retains its historical name while the
//! producer migration is in progress. Read/index APIs are intentionally absent;
//! they require an owner-approved inspector contract over persisted trace data.

use auv_driver::InputActionResult;
use auv_scan::{CoverageView, ScanCoverageArtifact};
use auv_tracing::{
  ArtifactMetadata, ArtifactPurpose, Attributes, ByteLength, Context, EventPayload, JsonArtifactError, StoreError, ValidationError,
};
use serde::Serialize;

use crate::contract::RecognitionResult;
use crate::scroll_scan::{SCROLL_SCAN_JSON_BYTE_LIMIT, SCROLL_SCAN_PURPOSE, ScrollScanArtifact, validate_scroll_scan_artifact};

pub use auv_driver::INPUT_ACTION_RESULT_PURPOSE;
pub const DETECTOR_RECOGNITION_PURPOSE: &str = "auv.runtime.detector_recognition";
pub const SCENE_STATE_INPUT_PURPOSE: &str = "auv.runtime.scene_state_input";
pub const SCAN_COVERAGE_PURPOSE: &str = "auv.runtime.scan_coverage";
pub const ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT: u64 = 4 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum RootArtifactPublishError {
  #[error("invalid root artifact purpose {value:?}: {source}")]
  InvalidPurpose {
    value: &'static str,
    #[source]
    source: ValidationError,
  },
  #[error("root artifact {purpose} failed domain validation: {message}")]
  InvalidPayload {
    purpose: ArtifactPurpose,
    message: String,
  },
  #[error("failed to construct root artifact {purpose}: {source}")]
  Json {
    purpose: ArtifactPurpose,
    #[source]
    source: JsonArtifactError,
  },
  #[error("failed to publish root artifact {purpose}: {source}")]
  Publication {
    purpose: ArtifactPurpose,
    #[source]
    source: StoreError,
  },
}

#[derive(Serialize)]
struct RootArtifactPreparationFailed {
  purpose: &'static str,
  error: String,
}

impl EventPayload for RootArtifactPreparationFailed {
  const NAME: &'static str = "auv.runtime.artifact_preparation_failed";
  const VERSION: u32 = 1;
}

/// Admits typed input evidence without making recording part of the operation
/// result. Preparation failures become tracing diagnostics; store failures are
/// reported by the active dispatch.
pub fn emit_input_action_result(value: &InputActionResult) {
  emit_json_artifact_bounded(INPUT_ACTION_RESULT_PURPOSE, value, ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT, validate_input_action_result);
}

pub fn emit_scan_coverage(value: &CoverageView) {
  let artifact = ScanCoverageArtifact::new(value.clone());
  emit_json_artifact_bounded(SCAN_COVERAGE_PURPOSE, &artifact, ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT, |_| Ok(()));
}

pub fn emit_scroll_scan(value: &ScrollScanArtifact) {
  emit_json_artifact_bounded(SCROLL_SCAN_PURPOSE, value, SCROLL_SCAN_JSON_BYTE_LIMIT, validate_scroll_scan_artifact);
}

fn emit_json_artifact_bounded<T, V>(purpose: &'static str, value: &T, byte_limit: u64, validate: V)
where
  T: Serialize,
  V: FnOnce(&T) -> Result<(), String>,
{
  let context = Context::current();
  if !context.can_publish_artifacts() {
    return;
  }
  match validate_json_artifact_bounded(purpose, value, byte_limit, validate).and_then(|(purpose, byte_limit)| {
    auv_tracing::emit_json_artifact(purpose.clone(), Attributes::empty(), byte_limit, value)
      .map(|emission| (purpose.clone(), emission))
      .map_err(|source| RootArtifactPublishError::Json { purpose, source })
  }) {
    Ok((_, emission)) => drop(emission),
    Err(error) => context.in_scope(|| {
      auv_tracing::emit_event!(RootArtifactPreparationFailed {
        purpose,
        error: error.to_string(),
      });
    }),
  }
}

pub async fn publish_input_action_result(
  context: Option<&Context>,
  value: &InputActionResult,
) -> Result<Option<ArtifactMetadata>, RootArtifactPublishError> {
  publish_json_artifact(context, INPUT_ACTION_RESULT_PURPOSE, value, validate_input_action_result).await
}

pub async fn publish_detector_recognition(
  context: Option<&Context>,
  value: &RecognitionResult,
) -> Result<Option<ArtifactMetadata>, RootArtifactPublishError> {
  publish_json_artifact(context, DETECTOR_RECOGNITION_PURPOSE, value, validate_recognition_result).await
}

pub async fn publish_scan_coverage(
  context: Option<&Context>,
  value: &CoverageView,
) -> Result<Option<ArtifactMetadata>, RootArtifactPublishError> {
  let artifact = ScanCoverageArtifact::new(value.clone());
  publish_json_artifact(context, SCAN_COVERAGE_PURPOSE, &artifact, |_| Ok(())).await
}

pub async fn publish_scroll_scan(
  context: Option<&Context>,
  value: &ScrollScanArtifact,
) -> Result<Option<ArtifactMetadata>, RootArtifactPublishError> {
  publish_json_artifact_bounded(context, SCROLL_SCAN_PURPOSE, value, SCROLL_SCAN_JSON_BYTE_LIMIT, validate_scroll_scan_artifact).await
}

pub(crate) async fn publish_json_artifact<T, V>(
  context: Option<&Context>,
  purpose: &'static str,
  value: &T,
  validate: V,
) -> Result<Option<ArtifactMetadata>, RootArtifactPublishError>
where
  T: Serialize,
  V: FnOnce(&T) -> Result<(), String>,
{
  publish_json_artifact_bounded(context, purpose, value, ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT, validate).await
}

async fn publish_json_artifact_bounded<T, V>(
  context: Option<&Context>,
  purpose: &'static str,
  value: &T,
  byte_limit: u64,
  validate: V,
) -> Result<Option<ArtifactMetadata>, RootArtifactPublishError>
where
  T: Serialize,
  V: FnOnce(&T) -> Result<(), String>,
{
  let Some(context) = context.filter(|context| context.can_publish_artifacts()) else {
    return Ok(None);
  };
  let (purpose, byte_limit) = validate_json_artifact_bounded(purpose, value, byte_limit, validate)?;
  let emission =
    context.in_scope(|| auv_tracing::emit_json_artifact(purpose.clone(), Attributes::empty(), byte_limit, value)).map_err(|source| {
      RootArtifactPublishError::Json {
        purpose: purpose.clone(),
        source,
      }
    })?;
  emission.await.map_err(|source| RootArtifactPublishError::Publication { purpose, source })
}

fn validate_json_artifact_bounded<T, V>(
  purpose: &'static str,
  value: &T,
  byte_limit: u64,
  validate: V,
) -> Result<(ArtifactPurpose, ByteLength), RootArtifactPublishError>
where
  T: Serialize,
  V: FnOnce(&T) -> Result<(), String>,
{
  let purpose = ArtifactPurpose::parse(purpose).map_err(|source| RootArtifactPublishError::InvalidPurpose {
    value: purpose,
    source,
  })?;
  validate(value).map_err(|message| RootArtifactPublishError::InvalidPayload {
    purpose: purpose.clone(),
    message,
  })?;
  let byte_limit = ByteLength::new(byte_limit).expect("root JSON limits remain within the canonical whole-artifact limit");
  Ok((purpose, byte_limit))
}

fn validate_input_action_result(value: &InputActionResult) -> Result<(), String> {
  if value.attempts.iter().any(|attempt| attempt.succeeded && attempt.path != value.selected_path) {
    return Err("successful input attempt must match selected_path".to_string());
  }
  Ok(())
}

fn validate_recognition_result(value: &RecognitionResult) -> Result<(), String> {
  if value.recognition_id.trim().is_empty() {
    return Err("recognition_id must not be empty".to_string());
  }
  Ok(())
}
