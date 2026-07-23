//! Canonical root-owned run artifact producers and readers.

use std::collections::TryReserveError;
use std::num::TryFromIntError;

use auv_driver::InputActionResult;
use auv_scan::{SCAN_COVERAGE_SCHEMA_VERSION, ScanCoverageWire};
use auv_tracing::{
  ArtifactMetadata, ArtifactPurpose, ArtifactReadError, ArtifactUri, ArtifactWriteError, Attributes, AuthorityId, ByteLength, ContentType,
  Context, NewArtifact, ReadError, RunId, RunSnapshot, RunStore, Sha256Digest, ValidationError,
};
use futures_util::StreamExt;
use futures_util::io::Cursor as AsyncCursor;
use serde::Serialize;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};

use crate::contract::{RecognitionResult, RecognitionSource};
use crate::scroll_scan::{SCROLL_SCAN_JSON_BYTE_LIMIT, SCROLL_SCAN_PAYLOAD_TOO_LARGE_CODE, SCROLL_SCAN_PURPOSE, ScrollScanArtifact};

pub const INPUT_ACTION_RESULT_PURPOSE: &str = "auv.driver.input_action_result";
pub const DETECTOR_RECOGNITION_PURPOSE: &str = "auv.runtime.detector_recognition";
pub const SCENE_STATE_INPUT_PURPOSE: &str = "auv.runtime.scene_state_input";
pub const SCAN_COVERAGE_PURPOSE: &str = "auv.runtime.scan_coverage";
pub const ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT: u64 = 4 * 1024 * 1024;

const JSON_CONTENT_TYPE: &str = "application/json";

#[derive(Debug, thiserror::Error)]
pub enum RootArtifactPublishError {
  #[error("invalid root artifact purpose {value:?}: {source}")]
  InvalidPurpose {
    value: &'static str,
    #[source]
    source: ValidationError,
  },
  #[error("invalid root artifact content type: {0}")]
  InvalidContentType(ValidationError),
  #[error("root artifact {purpose} failed domain validation: {message}")]
  InvalidPayload {
    purpose: ArtifactPurpose,
    message: String,
  },
  #[error("failed to serialize root artifact {purpose}: {source}")]
  Serialize {
    purpose: ArtifactPurpose,
    #[source]
    source: serde_json::Error,
  },
  #[error("root artifact {purpose} JSON length {actual} cannot be represented as u64: {source}")]
  LengthOutOfRange {
    purpose: ArtifactPurpose,
    actual: u128,
    #[source]
    source: TryFromIntError,
  },
  #[error("root artifact {purpose} is {actual} bytes, exceeding the {limit}-byte limit")]
  PayloadTooLarge {
    purpose: ArtifactPurpose,
    limit: u64,
    actual: u64,
  },
  #[error("invalid root artifact byte length for {purpose}: {source}")]
  InvalidByteLength {
    purpose: ArtifactPurpose,
    #[source]
    source: ValidationError,
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
  #[error("invalid expected root artifact content type: {0}")]
  InvalidExpectedContentType(ValidationError),
  #[error("snapshot authority {snapshot_authority} does not match store authority {store_authority}")]
  SnapshotAuthorityMismatch {
    snapshot_authority: AuthorityId,
    store_authority: AuthorityId,
  },
  #[error("artifact URI belongs to run {artifact_run_id}, not snapshot run {snapshot_run_id}")]
  WrongOwner {
    snapshot_run_id: RunId,
    artifact_run_id: RunId,
  },
  #[error("artifact URI is not committed in the supplied snapshot: {uri}")]
  DanglingUri { uri: ArtifactUri },
  #[error("artifact {uri} has purpose {actual}, expected {expected}")]
  WrongPurpose {
    uri: ArtifactUri,
    expected: ArtifactPurpose,
    actual: ArtifactPurpose,
  },
  #[error("artifact {uri} has content type {actual}, expected {expected}")]
  WrongContentType {
    uri: ArtifactUri,
    expected: ContentType,
    actual: ContentType,
  },
  #[error("artifact {uri} is {actual} bytes, exceeding the {limit}-byte limit")]
  PayloadTooLarge {
    uri: ArtifactUri,
    limit: u64,
    actual: u64,
  },
  #[error("artifact {uri} byte length {actual} cannot be represented by this process")]
  LengthOutOfRange { uri: ArtifactUri, actual: u64 },
  #[error("failed to reserve {expected} bytes for artifact {uri}: {source}")]
  Allocation {
    uri: ArtifactUri,
    expected: u64,
    #[source]
    source: TryReserveError,
  },
  #[error("failed to open artifact {uri}: {source}")]
  Open {
    uri: ArtifactUri,
    #[source]
    source: ReadError,
  },
  #[error("failed while streaming artifact {uri}: {source}")]
  Stream {
    uri: ArtifactUri,
    #[source]
    source: ArtifactReadError,
  },
  #[error("artifact {uri} length mismatch: expected {expected}, read {actual}")]
  LengthMismatch {
    uri: ArtifactUri,
    expected: u64,
    actual: u64,
  },
  #[error("artifact {uri} digest mismatch: expected {expected}, read {actual}")]
  DigestMismatch {
    uri: ArtifactUri,
    expected: Sha256Digest,
    actual: Sha256Digest,
  },
  #[error("artifact {uri} is malformed JSON: {source}")]
  MalformedJson {
    uri: ArtifactUri,
    #[source]
    source: serde_json::Error,
  },
  #[error("artifact {uri} failed domain validation: {message}")]
  InvalidPayload { uri: ArtifactUri, message: String },
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
      RootArtifactReadError::SnapshotAuthorityMismatch { .. } => "snapshot_authority_mismatch",
      RootArtifactReadError::WrongOwner { .. } => "wrong_owner",
      RootArtifactReadError::DanglingUri { .. } => "dangling_uri",
      RootArtifactReadError::WrongPurpose { .. } => "wrong_purpose",
      RootArtifactReadError::WrongContentType { .. } => "wrong_content_type",
      RootArtifactReadError::PayloadTooLarge { .. } => return auv_tracing::ErrorCode::parse(SCROLL_SCAN_PAYLOAD_TOO_LARGE_CODE).unwrap(),
      RootArtifactReadError::LengthOutOfRange { .. } => "length_out_of_range",
      RootArtifactReadError::Allocation { .. } => "allocation_failed",
      RootArtifactReadError::Open { .. } => "open_failed",
      RootArtifactReadError::Stream { .. } => "stream_failed",
      RootArtifactReadError::LengthMismatch { .. } => "length_mismatch",
      RootArtifactReadError::DigestMismatch { .. } => "digest_mismatch",
      RootArtifactReadError::MalformedJson { .. } => "malformed_json",
      RootArtifactReadError::InvalidPayload { .. } => "invalid_payload",
      RootArtifactReadError::InvalidExpectedPurpose { .. }
      | RootArtifactReadError::InvalidExpectedContentType(_)
      | RootArtifactReadError::AmbiguousPurpose { .. } => "invalid_contract",
    };
    auv_tracing::ErrorCode::parse(&format!("auv.runtime.scroll_scan.{suffix}")).expect("static scroll-scan error code")
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
  value: &ScanCoverageWire,
) -> Result<Option<ArtifactMetadata>, RootArtifactPublishError> {
  publish_json_artifact(context, SCAN_COVERAGE_PURPOSE, value, validate_scan_coverage).await
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
  let Some(context) = context.filter(|context| context.can_publish_artifacts()) else {
    return Ok(None);
  };
  let purpose = ArtifactPurpose::parse(purpose).map_err(|source| RootArtifactPublishError::InvalidPurpose {
    value: purpose,
    source,
  })?;
  validate(value).map_err(|message| RootArtifactPublishError::InvalidPayload {
    purpose: purpose.clone(),
    message,
  })?;
  let body = serde_json::to_vec(value).map_err(|source| RootArtifactPublishError::Serialize {
    purpose: purpose.clone(),
    source,
  })?;
  let byte_length = u64::try_from(body.len()).map_err(|source| RootArtifactPublishError::LengthOutOfRange {
    purpose: purpose.clone(),
    actual: body.len() as u128,
    source,
  })?;
  if byte_length > ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT {
    return Err(RootArtifactPublishError::PayloadTooLarge {
      purpose,
      limit: ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT,
      actual: byte_length,
    });
  }
  let artifact = NewArtifact::new(
    purpose.clone(),
    ContentType::parse(JSON_CONTENT_TYPE).map_err(RootArtifactPublishError::InvalidContentType)?,
    ByteLength::new(byte_length).map_err(|source| RootArtifactPublishError::InvalidByteLength {
      purpose: purpose.clone(),
      source,
    })?,
    Sha256Digest::new(Sha256::digest(&body).into()),
    Attributes::empty(),
    AsyncCursor::new(body),
  );
  context
    .in_scope(|| auv_tracing::emit_artifact!(artifact))
    .await
    .map_err(|source| RootArtifactPublishError::Publication { purpose, source })
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
  pub backend: Option<String>,
  pub model_id: Option<String>,
  pub execution_provider: Option<String>,
  pub all_count: usize,
  pub filtered_count: usize,
  pub best_item_id: Option<String>,
  pub evidence_artifacts: Vec<ArtifactUri>,
  pub known_limits: Vec<String>,
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
      backend: detail_string(&recognition.detail, "backend"),
      model_id: detail_string(&recognition.detail, "model_id"),
      execution_provider: detail_string(&recognition.detail, "execution_provider"),
      all_count: recognition.all.len(),
      filtered_count: recognition.filtered.len(),
      best_item_id: recognition.best.as_ref().map(|item| item.item_id.clone()),
      evidence_artifacts: recognition.evidence,
      known_limits: recognition.known_limits,
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
  let bytes = read_json_bytes(store, snapshot, uri, purpose, ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT).await?;
  let value = serde_json::from_slice(&bytes).map_err(|source| RootArtifactReadError::MalformedJson {
    uri: uri.clone(),
    source,
  })?;
  validate(&value).map_err(|message| RootArtifactReadError::InvalidPayload {
    uri: uri.clone(),
    message,
  })?;
  Ok(value)
}

async fn read_json_bytes(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
  purpose: &'static str,
  limit: u64,
) -> Result<Vec<u8>, RootArtifactReadError> {
  validate_snapshot_authority(store, snapshot)?;
  if uri.run_id() != snapshot.run_id() {
    return Err(RootArtifactReadError::WrongOwner {
      snapshot_run_id: snapshot.run_id(),
      artifact_run_id: uri.run_id(),
    });
  }
  let metadata = snapshot.artifacts().get(uri).ok_or_else(|| RootArtifactReadError::DanglingUri { uri: uri.clone() })?.metadata();
  let expected_purpose = expected_purpose(purpose)?;
  if metadata.purpose() != &expected_purpose {
    return Err(RootArtifactReadError::WrongPurpose {
      uri: uri.clone(),
      expected: expected_purpose,
      actual: metadata.purpose().clone(),
    });
  }
  let expected_content_type = ContentType::parse(JSON_CONTENT_TYPE).map_err(RootArtifactReadError::InvalidExpectedContentType)?;
  if metadata.content_type() != &expected_content_type {
    return Err(RootArtifactReadError::WrongContentType {
      uri: uri.clone(),
      expected: expected_content_type,
      actual: metadata.content_type().clone(),
    });
  }
  let expected_length = metadata.byte_length().get();
  if expected_length > limit {
    return Err(RootArtifactReadError::PayloadTooLarge {
      uri: uri.clone(),
      limit,
      actual: expected_length,
    });
  }
  let capacity = usize::try_from(expected_length).map_err(|_| RootArtifactReadError::LengthOutOfRange {
    uri: uri.clone(),
    actual: expected_length,
  })?;
  let mut bytes = Vec::new();
  bytes.try_reserve_exact(capacity).map_err(|source| RootArtifactReadError::Allocation {
    uri: uri.clone(),
    expected: expected_length,
    source,
  })?;
  let mut reader = store.open_artifact(uri.clone()).await.map_err(|source| RootArtifactReadError::Open {
    uri: uri.clone(),
    source,
  })?;
  let mut actual_length = 0_u64;
  while let Some(chunk) = reader.next().await {
    let chunk = chunk.map_err(|source| RootArtifactReadError::Stream {
      uri: uri.clone(),
      source,
    })?;
    actual_length = actual_length.checked_add(chunk.len() as u64).unwrap_or(u64::MAX);
    if actual_length > limit {
      return Err(RootArtifactReadError::PayloadTooLarge {
        uri: uri.clone(),
        limit,
        actual: actual_length,
      });
    }
    if actual_length > expected_length {
      return Err(RootArtifactReadError::LengthMismatch {
        uri: uri.clone(),
        expected: expected_length,
        actual: actual_length,
      });
    }
    bytes.extend_from_slice(&chunk);
  }
  if actual_length != expected_length {
    return Err(RootArtifactReadError::LengthMismatch {
      uri: uri.clone(),
      expected: expected_length,
      actual: actual_length,
    });
  }
  let actual_digest = Sha256Digest::new(Sha256::digest(&bytes).into());
  if actual_digest != metadata.sha256() {
    return Err(RootArtifactReadError::DigestMismatch {
      uri: uri.clone(),
      expected: metadata.sha256(),
      actual: actual_digest,
    });
  }
  Ok(bytes)
}

pub async fn read_scroll_scan(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
) -> Result<ScrollScanArtifact, ScrollScanReadError> {
  let bytes = read_json_bytes(store, snapshot, uri, SCROLL_SCAN_PURPOSE, SCROLL_SCAN_JSON_BYTE_LIMIT).await?;
  serde_json::from_slice(&bytes).map_err(|source| {
    RootArtifactReadError::MalformedJson {
      uri: uri.clone(),
      source,
    }
    .into()
  })
}

fn validate_snapshot_authority(store: &dyn RunStore, snapshot: &RunSnapshot) -> Result<(), RootArtifactReadError> {
  if snapshot.authority_id() != store.authority_id() {
    return Err(RootArtifactReadError::SnapshotAuthorityMismatch {
      snapshot_authority: snapshot.authority_id(),
      store_authority: store.authority_id(),
    });
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

fn validate_scan_coverage(value: &ScanCoverageWire) -> Result<(), String> {
  if value.schema_version != SCAN_COVERAGE_SCHEMA_VERSION {
    return Err(format!("schema version mismatch: found {}", value.schema_version));
  }
  Ok(())
}

fn detail_string(detail: &serde_json::Value, key: &str) -> Option<String> {
  detail.get(key).and_then(serde_json::Value::as_str).map(str::to_string)
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;

  use auv_tracing::{MemoryRunStore, configure, dispatcher};
  use serde_json::json;

  use super::*;

  fn input_action() -> InputActionResult {
    InputActionResult {
      selected_path: auv_driver::InputDeliveryPath::Noop,
      attempts: Vec::new(),
      fallback_reason: None,
      mouse_disturbance: auv_driver::DisturbanceLevel::None,
      focus_disturbance: auv_driver::DisturbanceLevel::None,
      clipboard_disturbance: auv_driver::DisturbanceLevel::None,
    }
  }

  fn recognition() -> RecognitionResult {
    RecognitionResult {
      recognition_id: "recognition-root-reader".to_string(),
      source: RecognitionSource::VisualRow,
      scope: crate::contract::RecognitionScope {
        surface: crate::contract::RecognitionSurface::Window,
        display_ref: None,
        native_display_id: None,
        app_bundle_id: Some("com.example.App".to_string()),
        window_title: Some("Example".to_string()),
        window_number: Some(7),
        region_hint: None,
        capture_artifact: None,
        capture_contract_artifact: None,
      },
      best: None,
      filtered: Vec::new(),
      all: Vec::new(),
      detail: json!({ "backend": "fixture", "model_id": "rows-v1" }),
      evidence: Vec::new(),
      known_limits: vec!["fixture".to_string()],
    }
  }

  #[tokio::test]
  async fn publishers_are_noops_without_a_current_context() {
    assert!(publish_input_action_result(None, &input_action()).await.expect("disabled input publication").is_none());
    assert!(publish_detector_recognition(None, &recognition()).await.expect("disabled recognition publication").is_none());
    let coverage = ScanCoverageWire {
      schema_version: SCAN_COVERAGE_SCHEMA_VERSION.to_string(),
      entries: Vec::new(),
      open_uncertainty_codes: Vec::new(),
      negative_evidence: Vec::new(),
      completeness: auv_scan::CompletenessWire::Complete,
    };
    assert!(publish_scan_coverage(None, &coverage).await.expect("disabled coverage publication").is_none());
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
      assert_eq!(metadata.content_type().to_string(), JSON_CONTENT_TYPE);
      assert!(snapshot.artifacts().contains_key(metadata.uri()));
    }
    assert_eq!(input_metadata.purpose().as_str(), INPUT_ACTION_RESULT_PURPOSE);
    assert_eq!(recognition_metadata.purpose().as_str(), DETECTOR_RECOGNITION_PURPOSE);
    assert_eq!(list_input_action_results(store.as_ref(), &snapshot).await.expect("read input results"), vec![input]);
    let lineage = list_detector_recognition_lineage(store.as_ref(), &snapshot).await.expect("read recognition lineage");
    assert_eq!(lineage.len(), 1);
    assert_eq!(lineage[0].artifact_uri, recognition_metadata.uri().clone());
    assert_eq!(lineage[0].recognition_id, recognition.recognition_id);
    assert_eq!(lineage[0].backend.as_deref(), Some("fixture"));
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

    assert!(matches!(error, RootArtifactReadError::WrongOwner { .. }));
  }
}
