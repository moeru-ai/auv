//! Canonical root-owned run artifact producers and readers.

use auv_driver::InputActionResult;
use auv_scan::{CoverageView, ScanCoverageArtifact};
use auv_tracing::{
  ArtifactMetadata, ArtifactPurpose, ArtifactUri, ArtifactWriteError, Attributes, ByteLength, Context, EventPayload, JsonArtifactError,
  JsonArtifactReadError, ReadArtifactError, RunSnapshot, RunStore, ValidationError,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::contract::{RecognitionResult, RecognitionSource};
use crate::scroll_scan::{
  SCROLL_SCAN_JSON_BYTE_LIMIT, SCROLL_SCAN_PAYLOAD_TOO_LARGE_CODE, SCROLL_SCAN_PURPOSE, ScrollScanArtifact, validate_scroll_scan_artifact,
};

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
    source: ArtifactWriteError,
  },
}

#[derive(Debug, thiserror::Error)]
pub enum RootArtifactReadError {
  #[error("invalid expected root artifact purpose {value:?}: {source}")]
  InvalidExpectedPurpose {
    value: &'static str,
    #[source]
    source: ValidationError,
  },
  #[error("failed to read root artifact: {source}")]
  Read {
    #[from]
    source: ReadArtifactError,
  },
  #[error("artifact {uri} is malformed JSON: {source}")]
  MalformedJson {
    uri: Box<ArtifactUri>,
    #[source]
    source: serde_json::Error,
  },
  #[error("artifact {uri} failed domain validation: {message}")]
  InvalidPayload {
    uri: Box<ArtifactUri>,
    message: String,
  },
  #[error("expected at most one {purpose} artifact, found {actual}")]
  AmbiguousPurpose {
    purpose: &'static str,
    actual: usize,
  },
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ScrollScanReadError(#[from] pub RootArtifactReadError);

impl ScrollScanReadError {
  pub fn code(&self) -> auv_tracing::ErrorCode {
    let suffix = match &self.0 {
      RootArtifactReadError::Read {
        source: ReadArtifactError::SnapshotAuthorityMismatch { .. },
      } => "snapshot_authority_mismatch",
      RootArtifactReadError::Read {
        source: ReadArtifactError::WrongRun { .. },
      } => "wrong_owner",
      RootArtifactReadError::Read {
        source: ReadArtifactError::NotCommitted { .. },
      } => "dangling_uri",
      RootArtifactReadError::Read {
        source: ReadArtifactError::WrongPurpose { .. },
      } => "wrong_purpose",
      RootArtifactReadError::Read {
        source: ReadArtifactError::WrongContentType { .. },
      } => "wrong_content_type",
      RootArtifactReadError::Read {
        source: ReadArtifactError::PayloadTooLarge { .. },
      } => return auv_tracing::ErrorCode::parse(SCROLL_SCAN_PAYLOAD_TOO_LARGE_CODE).unwrap(),
      RootArtifactReadError::Read {
        source: ReadArtifactError::LengthOutOfRange { .. },
      } => "length_out_of_range",
      RootArtifactReadError::Read {
        source: ReadArtifactError::Allocation { .. },
      } => "allocation_failed",
      RootArtifactReadError::Read {
        source: ReadArtifactError::Open { .. },
      } => "open_failed",
      RootArtifactReadError::Read {
        source: ReadArtifactError::Stream { .. },
      } => "stream_failed",
      RootArtifactReadError::Read {
        source: ReadArtifactError::LengthMismatch { .. },
      } => "length_mismatch",
      RootArtifactReadError::Read {
        source: ReadArtifactError::DigestMismatch { .. },
      } => "digest_mismatch",
      RootArtifactReadError::MalformedJson { .. } => "malformed_json",
      RootArtifactReadError::InvalidPayload { .. } => "invalid_payload",
      RootArtifactReadError::InvalidExpectedPurpose { .. } | RootArtifactReadError::AmbiguousPurpose { .. } => "invalid_contract",
    };
    auv_tracing::ErrorCode::parse(format!("auv.runtime.scroll_scan.{suffix}")).expect("static scroll-scan error code")
  }
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

pub async fn list_input_action_results(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
) -> Result<Vec<InputActionResult>, RootArtifactReadError> {
  let mut values = Vec::new();
  for uri in artifact_uris_for_purpose(store, snapshot, INPUT_ACTION_RESULT_PURPOSE)? {
    values.push(read_json_artifact(store, snapshot, &uri, INPUT_ACTION_RESULT_PURPOSE, validate_input_action_result).await?);
  }
  Ok(values)
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DetectorRecognitionLineage {
  pub artifact_uri: ArtifactUri,
  pub recognition_id: String,
  pub source: RecognitionSource,
  pub producer: String,
  pub model_id: Option<String>,
  pub execution_provider: Option<String>,
  pub all_count: usize,
  pub filtered_count: usize,
  pub best_item_id: Option<String>,
  pub evidence_artifacts: Vec<ArtifactUri>,
}

pub async fn list_detector_recognition_lineage(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
) -> Result<Vec<DetectorRecognitionLineage>, RootArtifactReadError> {
  let mut lineage = Vec::new();
  for uri in artifact_uris_for_purpose(store, snapshot, DETECTOR_RECOGNITION_PURPOSE)? {
    let recognition: RecognitionResult =
      read_json_artifact(store, snapshot, &uri, DETECTOR_RECOGNITION_PURPOSE, validate_recognition_result).await?;
    lineage.push(DetectorRecognitionLineage {
      artifact_uri: uri,
      recognition_id: recognition.recognition_id,
      source: recognition.source,
      producer: recognition.provenance.producer,
      model_id: recognition.provenance.model_id,
      execution_provider: recognition.provenance.execution_provider,
      all_count: recognition.all.len(),
      filtered_count: recognition.filtered.len(),
      best_item_id: recognition.best.as_ref().map(|item| item.item_id.clone()),
      evidence_artifacts: recognition.evidence_artifacts,
    });
  }
  Ok(lineage)
}

pub(crate) fn artifact_uris_for_purpose(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  purpose: &'static str,
) -> Result<Vec<ArtifactUri>, RootArtifactReadError> {
  validate_snapshot_authority(store, snapshot)?;
  let purpose = expected_purpose(purpose)?;
  Ok(
    snapshot
      .artifacts()
      .values()
      .filter(|artifact| artifact.metadata().purpose() == &purpose)
      .map(|artifact| artifact.metadata().uri().clone())
      .collect(),
  )
}

pub(crate) async fn read_one_json_artifact<T, V>(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  purpose: &'static str,
  validate: V,
) -> Result<Option<T>, RootArtifactReadError>
where
  T: DeserializeOwned,
  V: FnOnce(&T) -> Result<(), String>,
{
  let matches = artifact_uris_for_purpose(store, snapshot, purpose)?;
  match matches.as_slice() {
    [] => Ok(None),
    [uri] => read_json_artifact(store, snapshot, uri, purpose, validate).await.map(Some),
    _ => Err(RootArtifactReadError::AmbiguousPurpose {
      purpose,
      actual: matches.len(),
    }),
  }
}

pub(crate) async fn read_json_artifact<T, V>(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
  purpose: &'static str,
  validate: V,
) -> Result<T, RootArtifactReadError>
where
  T: DeserializeOwned,
  V: FnOnce(&T) -> Result<(), String>,
{
  read_json_artifact_bounded(store, snapshot, uri, purpose, ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT, validate).await
}

async fn read_json_artifact_bounded<T, V>(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
  purpose: &'static str,
  limit: u64,
  validate: V,
) -> Result<T, RootArtifactReadError>
where
  T: DeserializeOwned,
  V: FnOnce(&T) -> Result<(), String>,
{
  let expected_purpose = expected_purpose(purpose)?;
  let value = auv_tracing::read_json_artifact(
    store,
    snapshot,
    uri,
    &expected_purpose,
    ByteLength::new(limit).expect("artifact reader limit must be non-zero"),
  )
  .await
  .map_err(|error| match error {
    JsonArtifactReadError::Artifact(source) => RootArtifactReadError::Read { source },
    JsonArtifactReadError::Decode { source, .. } => RootArtifactReadError::MalformedJson {
      uri: Box::new(uri.clone()),
      source,
    },
  })?;
  validate(&value).map_err(|message| RootArtifactReadError::InvalidPayload {
    uri: Box::new(uri.clone()),
    message,
  })?;
  Ok(value)
}

pub async fn read_scroll_scan(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
) -> Result<ScrollScanArtifact, ScrollScanReadError> {
  read_json_artifact_bounded(store, snapshot, uri, SCROLL_SCAN_PURPOSE, SCROLL_SCAN_JSON_BYTE_LIMIT, validate_scroll_scan_artifact)
    .await
    .map_err(Into::into)
}

fn validate_snapshot_authority(store: &dyn RunStore, snapshot: &RunSnapshot) -> Result<(), RootArtifactReadError> {
  if snapshot.authority_id() != store.authority_id() {
    return Err(
      ReadArtifactError::SnapshotAuthorityMismatch {
        snapshot_authority: snapshot.authority_id(),
        store_authority: store.authority_id(),
      }
      .into(),
    );
  }
  Ok(())
}

fn expected_purpose(value: &'static str) -> Result<ArtifactPurpose, RootArtifactReadError> {
  ArtifactPurpose::parse(value).map_err(|source| RootArtifactReadError::InvalidExpectedPurpose { value, source })
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

#[cfg(test)]
mod tests {
  use std::sync::Arc;

  use super::*;
  use crate::contract::RecognitionProvenance;
  use auv_tracing::{AuthorityId, MemoryRunStore, RunId, configure, dispatcher};

  fn input_action() -> InputActionResult {
    InputActionResult {
      selected_path: auv_driver::InputDeliveryPath::Noop,
      attempts: Vec::new(),
      mouse_disturbance: auv_driver::DisturbanceLevel::None,
      focus_disturbance: auv_driver::DisturbanceLevel::None,
      clipboard_disturbance: auv_driver::DisturbanceLevel::None,
    }
  }

  fn recognition() -> RecognitionResult {
    RecognitionResult {
      recognition_id: "recognition-root-reader".to_string(),
      source: RecognitionSource::VisualRow,
      provenance: RecognitionProvenance {
        producer: "fixture".to_string(),
        model_id: Some("rows-v1".to_string()),
        execution_provider: None,
      },
      scope: crate::contract::RecognitionScope {
        surface: crate::contract::RecognitionSurface::Window,
        display_ref: None,
        native_display_id: None,
        app_bundle_id: Some("com.example.App".to_string()),
        window_title: Some("Example".to_string()),
        window_number: Some(7),
        region_hint: None,
        capture_artifact_uri: None,
        capture_contract_artifact_uri: None,
      },
      best: None,
      filtered: Vec::new(),
      all: Vec::new(),
      evidence_artifacts: Vec::new(),
    }
  }

  #[tokio::test]
  async fn publishers_are_noops_without_a_current_context() {
    assert!(publish_input_action_result(None, &input_action()).await.expect("disabled input publication").is_none());
    assert!(publish_detector_recognition(None, &recognition()).await.expect("disabled recognition publication").is_none());
    let coverage = CoverageView::complete(Vec::new());
    assert!(publish_scan_coverage(None, &coverage).await.expect("disabled coverage publication").is_none());
  }

  #[tokio::test]
  async fn detached_input_action_preparation_failure_does_not_change_the_primary_value() {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
    let invalid = InputActionResult {
      selected_path: auv_driver::InputDeliveryPath::WindowTargetedMouse,
      attempts: vec![auv_driver::InputAttempt::success(
        auv_driver::InputDeliveryPath::AxPress,
      )],
      mouse_disturbance: auv_driver::DisturbanceLevel::None,
      focus_disturbance: auv_driver::DisturbanceLevel::None,
      clipboard_disturbance: auv_driver::DisturbanceLevel::None,
    };

    let value = root.in_scope(|| {
      emit_input_action_result(&invalid);
      42
    });
    dispatch.flush().await.expect("preparation diagnostic flush");
    let snapshot = store.load_snapshot(run_id).await.expect("snapshot read").expect("diagnostic run");

    assert_eq!(value, 42);
    assert!(snapshot.artifacts().is_empty());
    assert!(
      snapshot.events().iter().any(|event| event.schema().name().as_str() == "auv.runtime.artifact_preparation_failed"),
      "invalid detached evidence must be visible as a typed diagnostic"
    );
  }

  #[tokio::test]
  async fn typed_root_artifacts_round_trip_through_one_snapshot_authority() {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
    let input = input_action();
    let recognition = recognition();

    let input_metadata =
      publish_input_action_result(Some(&root), &input).await.expect("publish input result").expect("input publication enabled");
    let recognition_metadata =
      publish_detector_recognition(Some(&root), &recognition).await.expect("publish recognition").expect("recognition publication enabled");
    dispatch.flush().await.expect("flush root artifacts");
    let snapshot = store.load_snapshot(run_id).await.expect("snapshot read").expect("root artifact snapshot");

    for metadata in [&input_metadata, &recognition_metadata] {
      assert_eq!(metadata.uri().run_id(), run_id);
      assert_eq!(metadata.content_type().to_string(), "application/json");
      assert!(snapshot.artifacts().contains_key(metadata.uri()));
    }
    assert_eq!(input_metadata.purpose().as_str(), INPUT_ACTION_RESULT_PURPOSE);
    assert_eq!(recognition_metadata.purpose().as_str(), DETECTOR_RECOGNITION_PURPOSE);
    assert_eq!(list_input_action_results(store.as_ref(), &snapshot).await.expect("read input results"), vec![input]);
    let lineage = list_detector_recognition_lineage(store.as_ref(), &snapshot).await.expect("read recognition lineage");
    assert_eq!(lineage.len(), 1);
    assert_eq!(lineage[0].artifact_uri, recognition_metadata.uri().clone());
    assert_eq!(lineage[0].recognition_id, recognition.recognition_id);
    assert_eq!(lineage[0].producer, "fixture");
  }

  #[tokio::test]
  async fn typed_reader_rejects_an_artifact_uri_owned_by_another_run_before_open() {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
    publish_input_action_result(Some(&root), &input_action()).await.expect("publish input result");
    dispatch.flush().await.expect("flush input result");
    let snapshot = store.load_snapshot(run_id).await.expect("snapshot read").expect("input snapshot");
    let foreign = ArtifactUri::from_ids(RunId::new(), auv_tracing::ArtifactId::new());

    let error = read_json_artifact::<InputActionResult, _>(
      store.as_ref(),
      &snapshot,
      &foreign,
      INPUT_ACTION_RESULT_PURPOSE,
      validate_input_action_result,
    )
    .await
    .expect_err("foreign URI must fail ownership validation");

    assert!(matches!(
      error,
      RootArtifactReadError::Read {
        source: ReadArtifactError::WrongRun { .. }
      }
    ));
  }
}
