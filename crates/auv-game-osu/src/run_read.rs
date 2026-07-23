//! Osu ordinary run_read helpers for inspect composition.
//!
//! Depends on canonical `auv-tracing` run snapshots only (no `auv-cli`).
//! Product query-wired presentation consumes these same typed artifacts.

use std::collections::TryReserveError;
use std::io::Write;
use std::num::TryFromIntError;

use auv_tracing::{
  ArtifactMetadata, ArtifactPurpose, ArtifactReadError, ArtifactUri, ArtifactWriteError, Attributes, AuthorityId, ByteLength, ContentType,
  Context, ErrorCode, NewArtifact, ReadError, RunId, RunSnapshot, RunStore, Sha256Digest, ValidationError,
};
use futures_util::StreamExt;
use futures_util::io::Cursor as AsyncCursor;
use serde::Serialize;
use sha2::{Digest, Sha256};

pub const OSU_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT: u64 = 4 * 1024 * 1024;
pub const OSU_STRUCTURED_ARTIFACT_PAYLOAD_TOO_LARGE_CODE: &str = "auv.osu.structured_artifact.payload_too_large";

const JSON_CONTENT_TYPE: &str = "application/json";

#[derive(Debug, thiserror::Error)]
pub enum OsuArtifactPublishError {
  #[error("invalid osu! artifact purpose {value:?}: {source}")]
  InvalidPurpose {
    value: &'static str,
    #[source]
    source: ValidationError,
  },
  #[error("invalid osu! artifact content type {value:?} for {purpose}: {source}")]
  InvalidContentType {
    purpose: ArtifactPurpose,
    value: &'static str,
    #[source]
    source: ValidationError,
  },
  #[error("osu! artifact {purpose} failed domain validation: {message}")]
  InvalidPayload {
    purpose: ArtifactPurpose,
    message: String,
  },
  #[error("osu! artifact {purpose} JSON length {actual} cannot be represented as u64: {source}")]
  LengthOutOfRange {
    purpose: ArtifactPurpose,
    actual: u128,
    #[source]
    source: TryFromIntError,
  },
  #[error("{OSU_STRUCTURED_ARTIFACT_PAYLOAD_TOO_LARGE_CODE}: {purpose} JSON is {actual} bytes, exceeding the {limit}-byte limit")]
  PayloadTooLarge {
    purpose: ArtifactPurpose,
    limit: u64,
    actual: u64,
  },
  #[error("failed to allocate {purpose} JSON bytes: {source}")]
  Allocation {
    purpose: ArtifactPurpose,
    #[source]
    source: TryReserveError,
  },
  #[error("failed to serialize {purpose} as JSON: {source}")]
  Serialize {
    purpose: ArtifactPurpose,
    #[source]
    source: serde_json::Error,
  },
  #[error("invalid byte length for osu! artifact {purpose}: {source}")]
  InvalidByteLength {
    purpose: ArtifactPurpose,
    #[source]
    source: ValidationError,
  },
  #[error("failed to publish osu! artifact {purpose}: {source}")]
  Publication {
    purpose: ArtifactPurpose,
    #[source]
    source: ArtifactWriteError,
  },
}

#[derive(Debug, thiserror::Error)]
pub enum OsuArtifactReadError {
  #[error("invalid expected osu! artifact purpose {value:?}: {source}")]
  InvalidExpectedPurpose {
    value: &'static str,
    #[source]
    source: ValidationError,
  },
  #[error("invalid expected osu! artifact content type {value:?}: {source}")]
  InvalidExpectedContentType {
    value: &'static str,
    #[source]
    source: ValidationError,
  },
  #[error("osu! snapshot authority {snapshot_authority} does not match store authority {store_authority}")]
  SnapshotAuthorityMismatch {
    snapshot_authority: AuthorityId,
    store_authority: AuthorityId,
  },
  #[error("osu! artifact URI belongs to run {artifact_run_id}, not snapshot run {snapshot_run_id}")]
  WrongOwner {
    snapshot_run_id: RunId,
    artifact_run_id: RunId,
  },
  #[error("osu! artifact URI is not committed in the supplied snapshot: {uri}")]
  DanglingUri { uri: ArtifactUri },
  #[error("osu! artifact {uri} has purpose {actual}, expected {expected}")]
  WrongPurpose {
    uri: Box<ArtifactUri>,
    expected: ArtifactPurpose,
    actual: ArtifactPurpose,
  },
  #[error("osu! artifact {uri} has content type {actual}, expected {expected}")]
  WrongContentType {
    uri: Box<ArtifactUri>,
    expected: Box<ContentType>,
    actual: Box<ContentType>,
  },
  #[error("osu! artifact {uri} is {actual} bytes, exceeding the {limit}-byte structured-artifact limit")]
  PayloadTooLarge {
    uri: ArtifactUri,
    limit: u64,
    actual: u64,
  },
  #[error("osu! artifact {uri} byte length {actual} cannot be represented by this process")]
  LengthOutOfRange { uri: ArtifactUri, actual: u64 },
  #[error("failed to reserve {expected} bytes for osu! artifact {uri}: {source}")]
  Allocation {
    uri: ArtifactUri,
    expected: u64,
    #[source]
    source: TryReserveError,
  },
  #[error("failed to open osu! artifact {uri}: {source}")]
  Open {
    uri: ArtifactUri,
    #[source]
    source: ReadError,
  },
  #[error("failed to stream osu! artifact {uri}: {source}")]
  Stream {
    uri: ArtifactUri,
    #[source]
    source: ArtifactReadError,
  },
  #[error("osu! artifact {uri} length mismatch: expected {expected}, read {actual}")]
  LengthMismatch {
    uri: ArtifactUri,
    expected: u64,
    actual: u64,
  },
  #[error("osu! artifact {uri} digest mismatch: expected {expected}, read {actual}")]
  DigestMismatch {
    uri: Box<ArtifactUri>,
    expected: Sha256Digest,
    actual: Sha256Digest,
  },
  #[error("osu! artifact {uri} is not the expected JSON type: {source}")]
  MalformedJson {
    uri: ArtifactUri,
    #[source]
    source: serde_json::Error,
  },
  #[error("osu! artifact {uri} failed domain validation: {message}")]
  InvalidPayload { uri: ArtifactUri, message: String },
}

impl OsuArtifactReadError {
  pub fn code(&self) -> ErrorCode {
    let value = match self {
      Self::InvalidExpectedPurpose { .. } | Self::InvalidExpectedContentType { .. } => "auv.osu.artifact.invalid_reader_contract",
      Self::SnapshotAuthorityMismatch { .. } => "auv.osu.artifact.snapshot_authority_mismatch",
      Self::WrongOwner { .. } => "auv.osu.artifact.wrong_owner",
      Self::DanglingUri { .. } => "auv.osu.artifact.dangling_uri",
      Self::WrongPurpose { .. } => "auv.osu.artifact.wrong_purpose",
      Self::WrongContentType { .. } => "auv.osu.artifact.wrong_content_type",
      Self::PayloadTooLarge { .. } => OSU_STRUCTURED_ARTIFACT_PAYLOAD_TOO_LARGE_CODE,
      Self::LengthOutOfRange { .. } => "auv.osu.artifact.length_out_of_range",
      Self::Allocation { .. } => "auv.osu.artifact.allocation_failed",
      Self::Open { .. } => "auv.osu.artifact.open_failed",
      Self::Stream { .. } => "auv.osu.artifact.stream_failed",
      Self::LengthMismatch { .. } => "auv.osu.artifact.length_mismatch",
      Self::DigestMismatch { .. } => "auv.osu.artifact.digest_mismatch",
      Self::MalformedJson { .. } => "auv.osu.artifact.malformed_json",
      Self::InvalidPayload { .. } => "auv.osu.artifact.invalid_payload",
    };
    ErrorCode::parse(value).expect("static osu! artifact error code is valid")
  }
}

pub(crate) async fn publish_json_artifact<T, V>(
  context: Option<&Context>,
  purpose: &'static str,
  value: &T,
  validate: V,
) -> Result<Option<ArtifactMetadata>, OsuArtifactPublishError>
where
  T: Serialize,
  V: FnOnce(&T) -> Result<(), String>,
{
  let Some(context) = context.filter(|context| context.can_publish_artifacts()) else {
    return Ok(None);
  };

  let purpose = ArtifactPurpose::parse(purpose).map_err(|source| OsuArtifactPublishError::InvalidPurpose {
    value: purpose,
    source,
  })?;
  validate(value).map_err(|message| OsuArtifactPublishError::InvalidPayload {
    purpose: purpose.clone(),
    message,
  })?;
  let body = serialize_json_bounded(&purpose, value)?;
  let byte_length = u64::try_from(body.len()).map_err(|source| OsuArtifactPublishError::LengthOutOfRange {
    purpose: purpose.clone(),
    actual: body.len() as u128,
    source,
  })?;
  let artifact = NewArtifact::new(
    purpose.clone(),
    ContentType::parse(JSON_CONTENT_TYPE).map_err(|source| OsuArtifactPublishError::InvalidContentType {
      purpose: purpose.clone(),
      value: JSON_CONTENT_TYPE,
      source,
    })?,
    ByteLength::new(byte_length).map_err(|source| OsuArtifactPublishError::InvalidByteLength {
      purpose: purpose.clone(),
      source,
    })?,
    Sha256Digest::new(Sha256::digest(&body).into()),
    Attributes::empty(),
    AsyncCursor::new(body),
  );
  context.in_scope(|| auv_tracing::emit_artifact!(artifact)).await.map_err(|source| OsuArtifactPublishError::Publication { purpose, source })
}

pub(crate) async fn read_json_artifact<T, V>(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
  expected_purpose: &'static str,
  validate: V,
) -> Result<T, OsuArtifactReadError>
where
  T: serde::de::DeserializeOwned,
  V: FnOnce(&T) -> Result<(), String>,
{
  let bytes = read_json_artifact_bytes(store, snapshot, uri, expected_purpose).await?;
  let value = serde_json::from_slice(&bytes).map_err(|source| OsuArtifactReadError::MalformedJson {
    uri: uri.clone(),
    source,
  })?;
  validate(&value).map_err(|message| OsuArtifactReadError::InvalidPayload {
    uri: uri.clone(),
    message,
  })?;
  Ok(value)
}

async fn read_json_artifact_bytes(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
  expected_purpose: &'static str,
) -> Result<Vec<u8>, OsuArtifactReadError> {
  let store_authority = store.authority_id();
  if snapshot.authority_id() != store_authority {
    return Err(OsuArtifactReadError::SnapshotAuthorityMismatch {
      snapshot_authority: snapshot.authority_id(),
      store_authority,
    });
  }
  let expected_purpose = ArtifactPurpose::parse(expected_purpose).map_err(|source| OsuArtifactReadError::InvalidExpectedPurpose {
    value: expected_purpose,
    source,
  })?;
  let expected_content_type = ContentType::parse(JSON_CONTENT_TYPE).map_err(|source| OsuArtifactReadError::InvalidExpectedContentType {
    value: JSON_CONTENT_TYPE,
    source,
  })?;
  if uri.run_id() != snapshot.run_id() {
    return Err(OsuArtifactReadError::WrongOwner {
      snapshot_run_id: snapshot.run_id(),
      artifact_run_id: uri.run_id(),
    });
  }
  let metadata = snapshot.artifacts().get(uri).ok_or_else(|| OsuArtifactReadError::DanglingUri { uri: uri.clone() })?.metadata();
  if metadata.purpose() != &expected_purpose {
    return Err(OsuArtifactReadError::WrongPurpose {
      uri: Box::new(uri.clone()),
      expected: expected_purpose,
      actual: metadata.purpose().clone(),
    });
  }
  if metadata.content_type() != &expected_content_type {
    return Err(OsuArtifactReadError::WrongContentType {
      uri: Box::new(uri.clone()),
      expected: Box::new(expected_content_type),
      actual: Box::new(metadata.content_type().clone()),
    });
  }

  let expected_length = metadata.byte_length().get();
  if expected_length > OSU_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT {
    return Err(OsuArtifactReadError::PayloadTooLarge {
      uri: uri.clone(),
      limit: OSU_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT,
      actual: expected_length,
    });
  }
  let expected_capacity = usize::try_from(expected_length).map_err(|_| OsuArtifactReadError::LengthOutOfRange {
    uri: uri.clone(),
    actual: expected_length,
  })?;
  let mut bytes = Vec::new();
  bytes.try_reserve_exact(expected_capacity).map_err(|source| OsuArtifactReadError::Allocation {
    uri: uri.clone(),
    expected: expected_length,
    source,
  })?;
  let mut reader = store.open_artifact(uri.clone()).await.map_err(|source| OsuArtifactReadError::Open {
    uri: uri.clone(),
    source,
  })?;
  let mut actual_length = 0_u64;
  while let Some(chunk) = reader.next().await {
    let chunk = chunk.map_err(|source| OsuArtifactReadError::Stream {
      uri: uri.clone(),
      source,
    })?;
    actual_length = actual_length.checked_add(chunk.len() as u64).ok_or_else(|| OsuArtifactReadError::PayloadTooLarge {
      uri: uri.clone(),
      limit: OSU_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT,
      actual: u64::MAX,
    })?;
    if actual_length > OSU_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT {
      return Err(OsuArtifactReadError::PayloadTooLarge {
        uri: uri.clone(),
        limit: OSU_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT,
        actual: actual_length,
      });
    }
    if actual_length > expected_length {
      return Err(OsuArtifactReadError::LengthMismatch {
        uri: uri.clone(),
        expected: expected_length,
        actual: actual_length,
      });
    }
    bytes.extend_from_slice(&chunk);
  }
  if actual_length != expected_length {
    return Err(OsuArtifactReadError::LengthMismatch {
      uri: uri.clone(),
      expected: expected_length,
      actual: actual_length,
    });
  }
  let actual_digest = Sha256Digest::new(Sha256::digest(&bytes).into());
  if actual_digest != metadata.sha256() {
    return Err(OsuArtifactReadError::DigestMismatch {
      uri: Box::new(uri.clone()),
      expected: metadata.sha256(),
      actual: actual_digest,
    });
  }
  Ok(bytes)
}

fn serialize_json_bounded<T: Serialize>(purpose: &ArtifactPurpose, value: &T) -> Result<Vec<u8>, OsuArtifactPublishError> {
  let mut output = BoundedJsonBuffer::new(purpose, OSU_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT);
  let result = serde_json::to_writer(&mut output, value);
  if let Some(failure) = output.failure.take() {
    return Err(match failure {
      JsonBufferFailure::LengthOutOfRange { actual, source } => OsuArtifactPublishError::LengthOutOfRange {
        purpose: purpose.clone(),
        actual,
        source,
      },
      JsonBufferFailure::PayloadTooLarge { actual } => OsuArtifactPublishError::PayloadTooLarge {
        purpose: purpose.clone(),
        limit: output.limit,
        actual,
      },
      JsonBufferFailure::Allocation(source) => OsuArtifactPublishError::Allocation {
        purpose: purpose.clone(),
        source,
      },
    });
  }
  result.map_err(|source| OsuArtifactPublishError::Serialize {
    purpose: purpose.clone(),
    source,
  })?;
  Ok(output.bytes)
}

struct BoundedJsonBuffer<'a> {
  purpose: &'a ArtifactPurpose,
  limit: u64,
  bytes: Vec<u8>,
  failure: Option<JsonBufferFailure>,
}

enum JsonBufferFailure {
  LengthOutOfRange {
    actual: u128,
    source: TryFromIntError,
  },
  PayloadTooLarge {
    actual: u64,
  },
  Allocation(TryReserveError),
}

impl<'a> BoundedJsonBuffer<'a> {
  fn new(purpose: &'a ArtifactPurpose, limit: u64) -> Self {
    Self {
      purpose,
      limit,
      bytes: Vec::new(),
      failure: None,
    }
  }

  fn fail(&mut self, failure: JsonBufferFailure) -> std::io::Error {
    self.failure = Some(failure);
    std::io::Error::other(format!("{} JSON exceeded its bounded buffer", self.purpose))
  }
}

impl Write for BoundedJsonBuffer<'_> {
  fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
    let Some(next_length) = self.bytes.len().checked_add(buffer.len()) else {
      return Err(self.fail(JsonBufferFailure::PayloadTooLarge { actual: u64::MAX }));
    };
    let next_length = match u64::try_from(next_length) {
      Ok(length) => length,
      Err(source) => {
        return Err(self.fail(JsonBufferFailure::LengthOutOfRange {
          actual: next_length as u128,
          source,
        }));
      }
    };
    if next_length > self.limit {
      return Err(self.fail(JsonBufferFailure::PayloadTooLarge {
        actual: next_length,
      }));
    }
    if let Err(source) = self.bytes.try_reserve(buffer.len()) {
      return Err(self.fail(JsonBufferFailure::Allocation(source)));
    }
    self.bytes.extend_from_slice(buffer);
    Ok(buffer.len())
  }

  fn flush(&mut self) -> std::io::Result<()> {
    Ok(())
  }
}

pub(crate) fn now_millis() -> u64 {
  u64::try_from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()).unwrap_or(u64::MAX)
}

use crate::detection_eval_quality::{OSU_DETECTION_EVAL_QUALITY_PURPOSE, read_osu_detection_eval_quality};
use crate::detection_eval_witness::{OSU_DETECTION_EVAL_WITNESS_PURPOSE, read_osu_detection_eval_witness};
use crate::visual_truth_semantic::{OSU_VISUAL_TRUTH_SEMANTIC_PURPOSE, read_osu_visual_truth_semantic};
use crate::visual_truth_spatial_query::{OSU_VISUAL_TRUTH_SPATIAL_QUERY_PURPOSE, read_osu_visual_truth_spatial_query};
use crate::{DetectionEvalQualityManifest, DetectionEvalWitnessManifest, VisualTruthSemanticManifest, VisualTruthSpatialQueryManifest};

#[derive(Clone, Debug, PartialEq)]
pub struct OsuInspectedArtifact<T> {
  uri: ArtifactUri,
  payload: T,
}

impl<T> OsuInspectedArtifact<T> {
  fn new(uri: ArtifactUri, payload: T) -> Self {
    Self { uri, payload }
  }

  pub fn uri(&self) -> &ArtifactUri {
    &self.uri
  }

  pub fn payload(&self) -> &T {
    &self.payload
  }
}

pub(crate) fn validate_snapshot_authority(store: &dyn RunStore, snapshot: &RunSnapshot) -> Result<(), OsuArtifactReadError> {
  let store_authority = store.authority_id();
  if snapshot.authority_id() != store_authority {
    return Err(OsuArtifactReadError::SnapshotAuthorityMismatch {
      snapshot_authority: snapshot.authority_id(),
      store_authority,
    });
  }
  Ok(())
}

pub(crate) fn artifacts_for_purpose(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  purpose: &'static str,
) -> Result<Vec<ArtifactUri>, OsuArtifactReadError> {
  validate_snapshot_authority(store, snapshot)?;
  let purpose = ArtifactPurpose::parse(purpose).map_err(|source| OsuArtifactReadError::InvalidExpectedPurpose {
    value: purpose,
    source,
  })?;
  Ok(
    snapshot
      .artifacts()
      .values()
      .filter(|artifact| artifact.metadata().purpose() == &purpose)
      .map(|artifact| artifact.metadata().uri().clone())
      .collect(),
  )
}

pub(crate) async fn extract_osu_visual_truth_semantic_manifests(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
) -> Result<Vec<OsuInspectedArtifact<VisualTruthSemanticManifest>>, OsuArtifactReadError> {
  let mut manifests = Vec::new();
  for uri in artifacts_for_purpose(store, snapshot, OSU_VISUAL_TRUTH_SEMANTIC_PURPOSE)? {
    let manifest = read_osu_visual_truth_semantic(store, snapshot, &uri).await?;
    manifests.push(OsuInspectedArtifact::new(uri, manifest));
  }
  Ok(manifests)
}

pub async fn extract_osu_visual_truth_spatial_query_manifests(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
) -> Result<Vec<OsuInspectedArtifact<VisualTruthSpatialQueryManifest>>, OsuArtifactReadError> {
  let mut manifests = Vec::new();
  for uri in artifacts_for_purpose(store, snapshot, OSU_VISUAL_TRUTH_SPATIAL_QUERY_PURPOSE)? {
    let manifest = read_osu_visual_truth_spatial_query(store, snapshot, &uri).await?;
    manifests.push(OsuInspectedArtifact::new(uri, manifest));
  }
  Ok(manifests)
}

pub(crate) async fn extract_osu_detection_eval_witness_manifests(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
) -> Result<Vec<OsuInspectedArtifact<DetectionEvalWitnessManifest>>, OsuArtifactReadError> {
  let mut manifests = Vec::new();
  for uri in artifacts_for_purpose(store, snapshot, OSU_DETECTION_EVAL_WITNESS_PURPOSE)? {
    let manifest = read_osu_detection_eval_witness(store, snapshot, &uri).await?;
    manifests.push(OsuInspectedArtifact::new(uri, manifest));
  }
  Ok(manifests)
}

pub(crate) async fn extract_osu_detection_eval_quality_manifests(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
) -> Result<Vec<OsuInspectedArtifact<DetectionEvalQualityManifest>>, OsuArtifactReadError> {
  let mut manifests = Vec::new();
  for uri in artifacts_for_purpose(store, snapshot, OSU_DETECTION_EVAL_QUALITY_PURPOSE)? {
    let manifest = read_osu_detection_eval_quality(store, snapshot, &uri).await?;
    manifests.push(OsuInspectedArtifact::new(uri, manifest));
  }
  Ok(manifests)
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;
  use std::sync::atomic::{AtomicUsize, Ordering};

  use auv_tracing::{BoxFuture, RunId, TelemetryError, TelemetryItem, TelemetryProjector, TelemetryRoutePolicy, configure, dispatcher};
  use serde::Serializer;

  use super::*;

  struct PanicOnSerialize;

  impl Serialize for PanicOnSerialize {
    fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
    where
      S: Serializer,
    {
      panic!("disabled publication must not serialize or construct an artifact body")
    }
  }

  #[derive(Default)]
  struct CountingProjector {
    item_count: AtomicUsize,
  }

  impl TelemetryProjector for CountingProjector {
    fn project(&self, _item: TelemetryItem) -> BoxFuture<'_, Result<(), TelemetryError>> {
      self.item_count.fetch_add(1, Ordering::Relaxed);
      Box::pin(async { Ok(()) })
    }

    fn flush(&self) -> BoxFuture<'_, Result<(), TelemetryError>> {
      Box::pin(async { Ok(()) })
    }
  }

  #[test]
  fn disabled_publication_returns_before_purpose_validation_payload_validation_and_body_construction() {
    futures_executor::block_on(async {
      let validation_count = AtomicUsize::new(0);

      let published = publish_json_artifact(None, "not a valid purpose", &PanicOnSerialize, |_| {
        validation_count.fetch_add(1, Ordering::Relaxed);
        panic!("disabled publication must not run domain validation")
      })
      .await
      .expect("disabled publication must short-circuit");

      assert!(published.is_none());
      assert_eq!(validation_count.load(Ordering::Relaxed), 0);
    });
  }

  #[test]
  fn telemetry_only_publication_returns_before_purpose_validation_payload_validation_body_construction_and_polling() {
    futures_executor::block_on(async {
      let projector = Arc::new(CountingProjector::default());
      let dispatch = configure()
        .project_telemetry(projector.clone(), TelemetryRoutePolicy::fixed_fields_only())
        .build()
        .expect("telemetry-only dispatch");
      let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
      let validation_count = AtomicUsize::new(0);

      let published = publish_json_artifact(Some(&root), "not a valid purpose", &PanicOnSerialize, |_| {
        validation_count.fetch_add(1, Ordering::Relaxed);
        panic!("telemetry-only publication must not run domain validation")
      })
      .await
      .expect("telemetry-only publication must short-circuit");
      dispatch.flush().await.expect("flush telemetry-only dispatch");

      assert!(published.is_none());
      assert_eq!(validation_count.load(Ordering::Relaxed), 0);
      assert_eq!(projector.item_count.load(Ordering::Relaxed), 0);
    });
  }
}
