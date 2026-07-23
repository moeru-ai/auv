//! Canonical Balatro run-artifact transport shared by typed domain readers.

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

/// Balatro card-detection manifests are structured metadata, not bulk media.
pub const BALATRO_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT: u64 = 4 * 1024 * 1024;
pub const BALATRO_STRUCTURED_ARTIFACT_PAYLOAD_TOO_LARGE_CODE: &str = "auv.balatro.structured_artifact.payload_too_large";

const JSON_CONTENT_TYPE: &str = "application/json";

#[derive(Debug, thiserror::Error)]
pub enum BalatroArtifactPublishError {
  #[error("invalid Balatro artifact purpose {value:?}: {source}")]
  InvalidPurpose {
    value: &'static str,
    #[source]
    source: ValidationError,
  },
  #[error("invalid Balatro artifact content type {value:?} for {purpose}: {source}")]
  InvalidContentType {
    purpose: ArtifactPurpose,
    value: &'static str,
    #[source]
    source: ValidationError,
  },
  #[error("Balatro artifact {purpose} JSON length {actual} cannot be represented as u64: {source}")]
  LengthOutOfRange {
    purpose: ArtifactPurpose,
    actual: u128,
    #[source]
    source: TryFromIntError,
  },
  #[error("{BALATRO_STRUCTURED_ARTIFACT_PAYLOAD_TOO_LARGE_CODE}: {purpose} JSON is {actual} bytes, exceeding the {limit}-byte limit")]
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
  #[error("invalid byte length for Balatro artifact {purpose}: {source}")]
  InvalidByteLength {
    purpose: ArtifactPurpose,
    #[source]
    source: ValidationError,
  },
  #[error("failed to publish Balatro artifact {purpose}: {source}")]
  Publication {
    purpose: ArtifactPurpose,
    #[source]
    source: ArtifactWriteError,
  },
}

#[derive(Debug, thiserror::Error)]
pub enum BalatroArtifactReadError {
  #[error("invalid expected Balatro artifact purpose {value:?}: {source}")]
  InvalidExpectedPurpose {
    value: &'static str,
    #[source]
    source: ValidationError,
  },
  #[error("invalid expected Balatro artifact content type {value:?}: {source}")]
  InvalidExpectedContentType {
    value: &'static str,
    #[source]
    source: ValidationError,
  },
  #[error("Balatro snapshot authority {snapshot_authority} does not match store authority {store_authority}")]
  SnapshotAuthorityMismatch {
    snapshot_authority: AuthorityId,
    store_authority: AuthorityId,
  },
  #[error("Balatro artifact URI belongs to run {artifact_run_id}, not snapshot run {snapshot_run_id}")]
  WrongOwner {
    snapshot_run_id: RunId,
    artifact_run_id: RunId,
  },
  #[error("Balatro artifact URI is not committed in the supplied snapshot: {uri}")]
  DanglingUri { uri: ArtifactUri },
  #[error("Balatro artifact {uri} has purpose {actual}, expected {expected}")]
  WrongPurpose {
    uri: Box<ArtifactUri>,
    expected: ArtifactPurpose,
    actual: ArtifactPurpose,
  },
  #[error("Balatro artifact {uri} has content type {actual}, expected {expected}")]
  WrongContentType {
    uri: Box<ArtifactUri>,
    expected: Box<ContentType>,
    actual: Box<ContentType>,
  },
  #[error("Balatro artifact {uri} is {actual} bytes, exceeding the {limit}-byte structured-artifact limit")]
  PayloadTooLarge {
    uri: ArtifactUri,
    limit: u64,
    actual: u64,
  },
  #[error("Balatro artifact {uri} byte length {actual} cannot be represented by this process")]
  LengthOutOfRange { uri: ArtifactUri, actual: u64 },
  #[error("failed to reserve {expected} bytes for Balatro artifact {uri}: {source}")]
  Allocation {
    uri: ArtifactUri,
    expected: u64,
    #[source]
    source: TryReserveError,
  },
  #[error("failed to open Balatro artifact {uri}: {source}")]
  Open {
    uri: ArtifactUri,
    #[source]
    source: ReadError,
  },
  #[error("failed to stream Balatro artifact {uri}: {source}")]
  Stream {
    uri: ArtifactUri,
    #[source]
    source: ArtifactReadError,
  },
  #[error("Balatro artifact {uri} length mismatch: expected {expected}, read {actual}")]
  LengthMismatch {
    uri: ArtifactUri,
    expected: u64,
    actual: u64,
  },
  #[error("Balatro artifact {uri} digest mismatch: expected {expected}, read {actual}")]
  DigestMismatch {
    uri: Box<ArtifactUri>,
    expected: Sha256Digest,
    actual: Sha256Digest,
  },
  #[error("Balatro artifact {uri} is not the expected JSON type: {source}")]
  MalformedJson {
    uri: ArtifactUri,
    #[source]
    source: serde_json::Error,
  },
}

impl BalatroArtifactReadError {
  pub fn code(&self) -> ErrorCode {
    let value = match self {
      Self::InvalidExpectedPurpose { .. } | Self::InvalidExpectedContentType { .. } => "auv.balatro.artifact.invalid_reader_contract",
      Self::SnapshotAuthorityMismatch { .. } => "auv.balatro.artifact.snapshot_authority_mismatch",
      Self::WrongOwner { .. } => "auv.balatro.artifact.wrong_owner",
      Self::DanglingUri { .. } => "auv.balatro.artifact.dangling_uri",
      Self::WrongPurpose { .. } => "auv.balatro.artifact.wrong_purpose",
      Self::WrongContentType { .. } => "auv.balatro.artifact.wrong_content_type",
      Self::PayloadTooLarge { .. } => BALATRO_STRUCTURED_ARTIFACT_PAYLOAD_TOO_LARGE_CODE,
      Self::LengthOutOfRange { .. } => "auv.balatro.artifact.length_out_of_range",
      Self::Allocation { .. } => "auv.balatro.artifact.allocation_failed",
      Self::Open { .. } => "auv.balatro.artifact.open_failed",
      Self::Stream { .. } => "auv.balatro.artifact.stream_failed",
      Self::LengthMismatch { .. } => "auv.balatro.artifact.length_mismatch",
      Self::DigestMismatch { .. } => "auv.balatro.artifact.digest_mismatch",
      Self::MalformedJson { .. } => "auv.balatro.artifact.malformed_json",
    };
    ErrorCode::parse(value).expect("static Balatro artifact error code is valid")
  }
}

pub(crate) async fn publish_json_artifact<T: Serialize>(
  context: Option<&Context>,
  purpose: &'static str,
  value: &T,
) -> Result<Option<ArtifactMetadata>, BalatroArtifactPublishError> {
  // Contexts without artifact authority, including telemetry-only contexts,
  // must not validate the contract or serialize the domain value.
  let Some(context) = context.filter(|context| context.can_publish_artifacts()) else {
    return Ok(None);
  };

  let purpose = ArtifactPurpose::parse(purpose).map_err(|source| BalatroArtifactPublishError::InvalidPurpose {
    value: purpose,
    source,
  })?;
  let body = serialize_json_bounded(&purpose, value)?;
  let byte_length = u64::try_from(body.len()).map_err(|source| BalatroArtifactPublishError::LengthOutOfRange {
    purpose: purpose.clone(),
    actual: body.len() as u128,
    source,
  })?;
  let artifact = NewArtifact::new(
    purpose.clone(),
    ContentType::parse(JSON_CONTENT_TYPE).map_err(|source| BalatroArtifactPublishError::InvalidContentType {
      purpose: purpose.clone(),
      value: JSON_CONTENT_TYPE,
      source,
    })?,
    ByteLength::new(byte_length).map_err(|source| BalatroArtifactPublishError::InvalidByteLength {
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
    .map_err(|source| BalatroArtifactPublishError::Publication { purpose, source })
}

pub(crate) fn artifact_uris_for_purpose(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  purpose: &'static str,
) -> Result<Vec<ArtifactUri>, BalatroArtifactReadError> {
  validate_snapshot_authority(store, snapshot)?;
  let purpose = expected_artifact_purpose(purpose)?;
  Ok(
    snapshot
      .artifacts()
      .values()
      .filter(|artifact| artifact.metadata().purpose() == &purpose)
      .map(|artifact| artifact.metadata().uri().clone())
      .collect(),
  )
}

pub(crate) fn validate_snapshot_authority(store: &dyn RunStore, snapshot: &RunSnapshot) -> Result<(), BalatroArtifactReadError> {
  let store_authority = store.authority_id();
  if snapshot.authority_id() != store_authority {
    return Err(BalatroArtifactReadError::SnapshotAuthorityMismatch {
      snapshot_authority: snapshot.authority_id(),
      store_authority,
    });
  }
  Ok(())
}

pub(crate) async fn read_json_artifact_bytes(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
  expected_purpose: &'static str,
) -> Result<Vec<u8>, BalatroArtifactReadError> {
  validate_snapshot_authority(store, snapshot)?;
  let expected_purpose = expected_artifact_purpose(expected_purpose)?;
  let expected_content_type = expected_json_content_type()?;
  if uri.run_id() != snapshot.run_id() {
    return Err(BalatroArtifactReadError::WrongOwner {
      snapshot_run_id: snapshot.run_id(),
      artifact_run_id: uri.run_id(),
    });
  }
  let metadata = snapshot.artifacts().get(uri).ok_or_else(|| BalatroArtifactReadError::DanglingUri { uri: uri.clone() })?.metadata();
  if metadata.purpose() != &expected_purpose {
    return Err(BalatroArtifactReadError::WrongPurpose {
      uri: Box::new(uri.clone()),
      expected: expected_purpose,
      actual: metadata.purpose().clone(),
    });
  }
  if metadata.content_type() != &expected_content_type {
    return Err(BalatroArtifactReadError::WrongContentType {
      uri: Box::new(uri.clone()),
      expected: Box::new(expected_content_type),
      actual: Box::new(metadata.content_type().clone()),
    });
  }

  let expected_length = metadata.byte_length().get();
  if expected_length > BALATRO_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT {
    return Err(BalatroArtifactReadError::PayloadTooLarge {
      uri: uri.clone(),
      limit: BALATRO_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT,
      actual: expected_length,
    });
  }
  let expected_capacity = usize::try_from(expected_length).map_err(|_| BalatroArtifactReadError::LengthOutOfRange {
    uri: uri.clone(),
    actual: expected_length,
  })?;
  let mut bytes = Vec::new();
  bytes.try_reserve_exact(expected_capacity).map_err(|source| BalatroArtifactReadError::Allocation {
    uri: uri.clone(),
    expected: expected_length,
    source,
  })?;
  let mut reader = store.open_artifact(uri.clone()).await.map_err(|source| BalatroArtifactReadError::Open {
    uri: uri.clone(),
    source,
  })?;
  let mut actual_length = 0_u64;
  while let Some(chunk) = reader.next().await {
    let chunk = chunk.map_err(|source| BalatroArtifactReadError::Stream {
      uri: uri.clone(),
      source,
    })?;
    actual_length = actual_length.checked_add(chunk.len() as u64).ok_or_else(|| BalatroArtifactReadError::PayloadTooLarge {
      uri: uri.clone(),
      limit: BALATRO_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT,
      actual: u64::MAX,
    })?;
    if actual_length > BALATRO_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT {
      return Err(BalatroArtifactReadError::PayloadTooLarge {
        uri: uri.clone(),
        limit: BALATRO_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT,
        actual: actual_length,
      });
    }
    if actual_length > expected_length {
      return Err(BalatroArtifactReadError::LengthMismatch {
        uri: uri.clone(),
        expected: expected_length,
        actual: actual_length,
      });
    }
    bytes.extend_from_slice(&chunk);
  }
  if actual_length != expected_length {
    return Err(BalatroArtifactReadError::LengthMismatch {
      uri: uri.clone(),
      expected: expected_length,
      actual: actual_length,
    });
  }
  let actual_digest = Sha256Digest::new(Sha256::digest(&bytes).into());
  if actual_digest != metadata.sha256() {
    return Err(BalatroArtifactReadError::DigestMismatch {
      uri: Box::new(uri.clone()),
      expected: metadata.sha256(),
      actual: actual_digest,
    });
  }
  Ok(bytes)
}

fn expected_artifact_purpose(value: &'static str) -> Result<ArtifactPurpose, BalatroArtifactReadError> {
  ArtifactPurpose::parse(value).map_err(|source| BalatroArtifactReadError::InvalidExpectedPurpose { value, source })
}

fn expected_json_content_type() -> Result<ContentType, BalatroArtifactReadError> {
  ContentType::parse(JSON_CONTENT_TYPE).map_err(|source| BalatroArtifactReadError::InvalidExpectedContentType {
    value: JSON_CONTENT_TYPE,
    source,
  })
}

fn serialize_json_bounded<T: Serialize>(purpose: &ArtifactPurpose, value: &T) -> Result<Vec<u8>, BalatroArtifactPublishError> {
  let mut output = BoundedJsonBuffer::new(purpose, BALATRO_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT);
  let result = serde_json::to_writer(&mut output, value);
  if let Some(failure) = output.failure.take() {
    return Err(match failure {
      JsonBufferFailure::LengthOutOfRange { actual, source } => BalatroArtifactPublishError::LengthOutOfRange {
        purpose: purpose.clone(),
        actual,
        source,
      },
      JsonBufferFailure::PayloadTooLarge { actual } => BalatroArtifactPublishError::PayloadTooLarge {
        purpose: purpose.clone(),
        limit: output.limit,
        actual,
      },
      JsonBufferFailure::Allocation(source) => BalatroArtifactPublishError::Allocation {
        purpose: purpose.clone(),
        source,
      },
    });
  }
  result.map_err(|source| BalatroArtifactPublishError::Serialize {
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

#[cfg(test)]
mod tests {
  use std::error::Error as _;
  use std::sync::Arc;

  use auv_tracing::{
    BoxFuture, Context, MemoryRunStore, RunId, TelemetryError, TelemetryItem, TelemetryProjector, TelemetryRoutePolicy, configure,
    dispatcher,
  };
  use serde::Serializer;

  use super::*;

  struct PanicOnSerialize;

  impl Serialize for PanicOnSerialize {
    fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
    where
      S: Serializer,
    {
      panic!("serializer must not run")
    }
  }

  struct NoopProjector;

  impl TelemetryProjector for NoopProjector {
    fn project(&self, _item: TelemetryItem) -> BoxFuture<'_, Result<(), TelemetryError>> {
      Box::pin(async { Ok(()) })
    }

    fn flush(&self) -> BoxFuture<'_, Result<(), TelemetryError>> {
      Box::pin(async { Ok(()) })
    }
  }

  #[test]
  fn disabled_publication_does_not_parse_or_serialize() {
    futures_executor::block_on(async {
      let published =
        publish_json_artifact(None, "not a valid purpose", &PanicOnSerialize).await.expect("disabled publication must short-circuit");

      assert!(published.is_none());
    });
  }

  #[test]
  fn telemetry_only_publication_does_not_parse_or_serialize() {
    futures_executor::block_on(async {
      let dispatch = configure()
        .project_telemetry(Arc::new(NoopProjector), TelemetryRoutePolicy::fixed_fields_only())
        .build()
        .expect("telemetry-only dispatch");
      let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));

      let published = publish_json_artifact(Some(&root), "not a valid purpose", &PanicOnSerialize)
        .await
        .expect("telemetry-only publication must short-circuit");

      assert!(published.is_none());
    });
  }

  #[test]
  fn enabled_publication_validates_purpose_before_serializing() {
    futures_executor::block_on(async {
      let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
      let dispatch = configure().run_store(store).build().expect("memory dispatch");
      let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));

      let error = publish_json_artifact(Some(&root), "not a valid purpose", &PanicOnSerialize)
        .await
        .expect_err("invalid purpose must fail before serialization");

      assert!(error.source().and_then(|source| source.downcast_ref::<ValidationError>()).is_some());
      match error {
        BalatroArtifactPublishError::InvalidPurpose { value, source } => {
          assert_eq!(value, "not a valid purpose");
          assert_eq!(source, ArtifactPurpose::parse(value).expect_err("fixture purpose is invalid"));
        }
        other => panic!("expected invalid-purpose error, got {other:?}"),
      }
    });
  }
}
