//! Canonical Minecraft run-artifact transport shared by typed domain readers.

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

/// Minecraft structured artifacts carry metadata and manifests, not bulk media.
pub const MINECRAFT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT: u64 = 4 * 1024 * 1024;
pub const MINECRAFT_STRUCTURED_ARTIFACT_PAYLOAD_TOO_LARGE_CODE: &str = "auv.minecraft.structured_artifact.payload_too_large";

const JSON_CONTENT_TYPE: &str = "application/json";

#[derive(Debug, thiserror::Error)]
pub enum MinecraftArtifactPublishError {
  #[error("invalid Minecraft artifact purpose {value:?}: {source}")]
  InvalidPurpose {
    value: &'static str,
    #[source]
    source: ValidationError,
  },
  #[error("invalid Minecraft artifact content type {value:?} for {purpose}: {source}")]
  InvalidContentType {
    purpose: ArtifactPurpose,
    value: &'static str,
    #[source]
    source: ValidationError,
  },
  #[error("Minecraft artifact {purpose} failed domain validation: {message}")]
  InvalidPayload {
    purpose: ArtifactPurpose,
    message: String,
  },
  #[error("Minecraft artifact {purpose} JSON length {actual} cannot be represented as u64: {source}")]
  LengthOutOfRange {
    purpose: ArtifactPurpose,
    actual: u128,
    #[source]
    source: TryFromIntError,
  },
  #[error("{MINECRAFT_STRUCTURED_ARTIFACT_PAYLOAD_TOO_LARGE_CODE}: {purpose} JSON is {actual} bytes, exceeding the {limit}-byte limit")]
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
  #[error("invalid byte length for Minecraft artifact {purpose}: {source}")]
  InvalidByteLength {
    purpose: ArtifactPurpose,
    #[source]
    source: ValidationError,
  },
  #[error("failed to publish Minecraft artifact {purpose}: {source}")]
  Publication {
    purpose: ArtifactPurpose,
    #[source]
    source: ArtifactWriteError,
  },
}

#[derive(Debug, thiserror::Error)]
pub enum MinecraftArtifactReadError {
  #[error("invalid expected Minecraft artifact purpose {value:?}: {source}")]
  InvalidExpectedPurpose {
    value: &'static str,
    #[source]
    source: ValidationError,
  },
  #[error("invalid expected Minecraft artifact content type {value:?}: {source}")]
  InvalidExpectedContentType {
    value: &'static str,
    #[source]
    source: ValidationError,
  },
  #[error("Minecraft snapshot authority {snapshot_authority} does not match store authority {store_authority}")]
  SnapshotAuthorityMismatch {
    snapshot_authority: AuthorityId,
    store_authority: AuthorityId,
  },
  #[error("Minecraft artifact URI belongs to run {artifact_run_id}, not snapshot run {snapshot_run_id}")]
  WrongOwner {
    snapshot_run_id: RunId,
    artifact_run_id: RunId,
  },
  #[error("Minecraft artifact URI is not committed in the supplied snapshot: {uri}")]
  DanglingUri { uri: ArtifactUri },
  #[error("Minecraft artifact {uri} has purpose {actual}, expected {expected}")]
  WrongPurpose {
    uri: Box<ArtifactUri>,
    expected: ArtifactPurpose,
    actual: ArtifactPurpose,
  },
  #[error("Minecraft artifact {uri} has content type {actual}, expected {expected}")]
  WrongContentType {
    uri: Box<ArtifactUri>,
    expected: Box<ContentType>,
    actual: Box<ContentType>,
  },
  #[error("Minecraft artifact {uri} is {actual} bytes, exceeding the {limit}-byte structured-artifact limit")]
  PayloadTooLarge {
    uri: ArtifactUri,
    limit: u64,
    actual: u64,
  },
  #[error("Minecraft artifact {uri} byte length {actual} cannot be represented by this process")]
  LengthOutOfRange { uri: ArtifactUri, actual: u64 },
  #[error("failed to reserve {expected} bytes for Minecraft artifact {uri}: {source}")]
  Allocation {
    uri: ArtifactUri,
    expected: u64,
    #[source]
    source: TryReserveError,
  },
  #[error("failed to open Minecraft artifact {uri}: {source}")]
  Open {
    uri: ArtifactUri,
    #[source]
    source: ReadError,
  },
  #[error("failed to stream Minecraft artifact {uri}: {source}")]
  Stream {
    uri: ArtifactUri,
    #[source]
    source: ArtifactReadError,
  },
  #[error("Minecraft artifact {uri} length mismatch: expected {expected}, read {actual}")]
  LengthMismatch {
    uri: ArtifactUri,
    expected: u64,
    actual: u64,
  },
  #[error("Minecraft artifact {uri} digest mismatch: expected {expected}, read {actual}")]
  DigestMismatch {
    uri: Box<ArtifactUri>,
    expected: Sha256Digest,
    actual: Sha256Digest,
  },
  #[error("Minecraft artifact {uri} is not the expected JSON type: {source}")]
  MalformedJson {
    uri: ArtifactUri,
    #[source]
    source: serde_json::Error,
  },
  #[error("Minecraft artifact {uri} failed domain validation: {message}")]
  InvalidPayload { uri: ArtifactUri, message: String },
}

impl MinecraftArtifactReadError {
  pub fn code(&self) -> ErrorCode {
    let value = match self {
      Self::InvalidExpectedPurpose { .. } | Self::InvalidExpectedContentType { .. } => "auv.minecraft.artifact.invalid_reader_contract",
      Self::SnapshotAuthorityMismatch { .. } => "auv.minecraft.artifact.snapshot_authority_mismatch",
      Self::WrongOwner { .. } => "auv.minecraft.artifact.wrong_owner",
      Self::DanglingUri { .. } => "auv.minecraft.artifact.dangling_uri",
      Self::WrongPurpose { .. } => "auv.minecraft.artifact.wrong_purpose",
      Self::WrongContentType { .. } => "auv.minecraft.artifact.wrong_content_type",
      Self::PayloadTooLarge { .. } => MINECRAFT_STRUCTURED_ARTIFACT_PAYLOAD_TOO_LARGE_CODE,
      Self::LengthOutOfRange { .. } => "auv.minecraft.artifact.length_out_of_range",
      Self::Allocation { .. } => "auv.minecraft.artifact.allocation_failed",
      Self::Open { .. } => "auv.minecraft.artifact.open_failed",
      Self::Stream { .. } => "auv.minecraft.artifact.stream_failed",
      Self::LengthMismatch { .. } => "auv.minecraft.artifact.length_mismatch",
      Self::DigestMismatch { .. } => "auv.minecraft.artifact.digest_mismatch",
      Self::MalformedJson { .. } => "auv.minecraft.artifact.malformed_json",
      Self::InvalidPayload { .. } => "auv.minecraft.artifact.invalid_payload",
    };
    ErrorCode::parse(value).expect("static Minecraft artifact error code is valid")
  }
}

pub(crate) async fn publish_json_artifact<T, V>(
  context: Option<&Context>,
  purpose: &'static str,
  value: &T,
  validate: V,
) -> Result<Option<ArtifactMetadata>, MinecraftArtifactPublishError>
where
  T: Serialize,
  V: FnOnce(&T) -> Result<(), String>,
{
  // Disabled and telemetry-only contexts must not inspect or serialize the
  // direct domain return value.
  let Some(context) = context.filter(|context| context.can_publish_artifacts()) else {
    return Ok(None);
  };

  let purpose = ArtifactPurpose::parse(purpose).map_err(|source| MinecraftArtifactPublishError::InvalidPurpose {
    value: purpose,
    source,
  })?;
  validate(value).map_err(|message| MinecraftArtifactPublishError::InvalidPayload {
    purpose: purpose.clone(),
    message,
  })?;
  let body = serialize_json_bounded(&purpose, value)?;
  let byte_length = u64::try_from(body.len()).map_err(|source| MinecraftArtifactPublishError::LengthOutOfRange {
    purpose: purpose.clone(),
    actual: body.len() as u128,
    source,
  })?;
  let artifact = NewArtifact::new(
    purpose.clone(),
    ContentType::parse(JSON_CONTENT_TYPE).map_err(|source| MinecraftArtifactPublishError::InvalidContentType {
      purpose: purpose.clone(),
      value: JSON_CONTENT_TYPE,
      source,
    })?,
    ByteLength::new(byte_length).map_err(|source| MinecraftArtifactPublishError::InvalidByteLength {
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
    .map_err(|source| MinecraftArtifactPublishError::Publication { purpose, source })
}

pub(crate) fn artifact_uris_for_purpose(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  purpose: &'static str,
) -> Result<Vec<ArtifactUri>, MinecraftArtifactReadError> {
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

pub(crate) fn validate_snapshot_authority(store: &dyn RunStore, snapshot: &RunSnapshot) -> Result<(), MinecraftArtifactReadError> {
  let store_authority = store.authority_id();
  if snapshot.authority_id() != store_authority {
    return Err(MinecraftArtifactReadError::SnapshotAuthorityMismatch {
      snapshot_authority: snapshot.authority_id(),
      store_authority,
    });
  }
  Ok(())
}

pub(crate) async fn read_json_artifact<T, V>(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
  expected_purpose: &'static str,
  validate: V,
) -> Result<T, MinecraftArtifactReadError>
where
  T: serde::de::DeserializeOwned,
  V: FnOnce(&T) -> Result<(), String>,
{
  let bytes = read_json_artifact_bytes(store, snapshot, uri, expected_purpose).await?;
  let value = serde_json::from_slice(&bytes).map_err(|source| MinecraftArtifactReadError::MalformedJson {
    uri: uri.clone(),
    source,
  })?;
  validate(&value).map_err(|message| MinecraftArtifactReadError::InvalidPayload {
    uri: uri.clone(),
    message,
  })?;
  Ok(value)
}

pub(crate) async fn read_json_artifact_bytes(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
  expected_purpose: &'static str,
) -> Result<Vec<u8>, MinecraftArtifactReadError> {
  validate_snapshot_authority(store, snapshot)?;
  let expected_purpose = expected_artifact_purpose(expected_purpose)?;
  let expected_content_type = expected_json_content_type()?;
  if uri.run_id() != snapshot.run_id() {
    return Err(MinecraftArtifactReadError::WrongOwner {
      snapshot_run_id: snapshot.run_id(),
      artifact_run_id: uri.run_id(),
    });
  }
  let metadata = snapshot.artifacts().get(uri).ok_or_else(|| MinecraftArtifactReadError::DanglingUri { uri: uri.clone() })?.metadata();
  if metadata.purpose() != &expected_purpose {
    return Err(MinecraftArtifactReadError::WrongPurpose {
      uri: Box::new(uri.clone()),
      expected: expected_purpose,
      actual: metadata.purpose().clone(),
    });
  }
  if metadata.content_type() != &expected_content_type {
    return Err(MinecraftArtifactReadError::WrongContentType {
      uri: Box::new(uri.clone()),
      expected: Box::new(expected_content_type),
      actual: Box::new(metadata.content_type().clone()),
    });
  }

  let expected_length = metadata.byte_length().get();
  if expected_length > MINECRAFT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT {
    return Err(MinecraftArtifactReadError::PayloadTooLarge {
      uri: uri.clone(),
      limit: MINECRAFT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT,
      actual: expected_length,
    });
  }
  let expected_capacity = usize::try_from(expected_length).map_err(|_| MinecraftArtifactReadError::LengthOutOfRange {
    uri: uri.clone(),
    actual: expected_length,
  })?;
  let mut bytes = Vec::new();
  bytes.try_reserve_exact(expected_capacity).map_err(|source| MinecraftArtifactReadError::Allocation {
    uri: uri.clone(),
    expected: expected_length,
    source,
  })?;
  let mut reader = store.open_artifact(uri.clone()).await.map_err(|source| MinecraftArtifactReadError::Open {
    uri: uri.clone(),
    source,
  })?;
  let mut actual_length = 0_u64;
  while let Some(chunk) = reader.next().await {
    let chunk = chunk.map_err(|source| MinecraftArtifactReadError::Stream {
      uri: uri.clone(),
      source,
    })?;
    actual_length = actual_length.checked_add(chunk.len() as u64).ok_or_else(|| MinecraftArtifactReadError::PayloadTooLarge {
      uri: uri.clone(),
      limit: MINECRAFT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT,
      actual: u64::MAX,
    })?;
    if actual_length > MINECRAFT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT {
      return Err(MinecraftArtifactReadError::PayloadTooLarge {
        uri: uri.clone(),
        limit: MINECRAFT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT,
        actual: actual_length,
      });
    }
    if actual_length > expected_length {
      return Err(MinecraftArtifactReadError::LengthMismatch {
        uri: uri.clone(),
        expected: expected_length,
        actual: actual_length,
      });
    }
    bytes.extend_from_slice(&chunk);
  }
  if actual_length != expected_length {
    return Err(MinecraftArtifactReadError::LengthMismatch {
      uri: uri.clone(),
      expected: expected_length,
      actual: actual_length,
    });
  }
  let actual_digest = Sha256Digest::new(Sha256::digest(&bytes).into());
  if actual_digest != metadata.sha256() {
    return Err(MinecraftArtifactReadError::DigestMismatch {
      uri: Box::new(uri.clone()),
      expected: metadata.sha256(),
      actual: actual_digest,
    });
  }
  Ok(bytes)
}

fn expected_artifact_purpose(value: &'static str) -> Result<ArtifactPurpose, MinecraftArtifactReadError> {
  ArtifactPurpose::parse(value).map_err(|source| MinecraftArtifactReadError::InvalidExpectedPurpose { value, source })
}

fn expected_json_content_type() -> Result<ContentType, MinecraftArtifactReadError> {
  ContentType::parse(JSON_CONTENT_TYPE).map_err(|source| MinecraftArtifactReadError::InvalidExpectedContentType {
    value: JSON_CONTENT_TYPE,
    source,
  })
}

fn serialize_json_bounded<T: Serialize>(purpose: &ArtifactPurpose, value: &T) -> Result<Vec<u8>, MinecraftArtifactPublishError> {
  let mut output = BoundedJsonBuffer::new(purpose, MINECRAFT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT);
  let result = serde_json::to_writer(&mut output, value);
  if let Some(failure) = output.failure.take() {
    return Err(match failure {
      JsonBufferFailure::LengthOutOfRange { actual, source } => MinecraftArtifactPublishError::LengthOutOfRange {
        purpose: purpose.clone(),
        actual,
        source,
      },
      JsonBufferFailure::PayloadTooLarge { actual } => MinecraftArtifactPublishError::PayloadTooLarge {
        purpose: purpose.clone(),
        limit: output.limit,
        actual,
      },
      JsonBufferFailure::Allocation(source) => MinecraftArtifactPublishError::Allocation {
        purpose: purpose.clone(),
        source,
      },
    });
  }
  result.map_err(|source| MinecraftArtifactPublishError::Serialize {
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
