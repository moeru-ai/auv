//! Canonical Minecraft run-artifact transport shared by typed domain readers.

use auv_tracing::{
  ArtifactMetadata, ArtifactPurpose, ArtifactUri, ArtifactWriteError, Attributes, ByteLength, Context, ErrorCode, JsonArtifactError,
  JsonArtifactReadError, ReadArtifactError, RunSnapshot, RunStore, ValidationError,
};
use serde::Serialize;

/// Minecraft structured artifacts carry metadata and manifests, not bulk media.
pub const MINECRAFT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT: u64 = 4 * 1024 * 1024;
pub const MINECRAFT_STRUCTURED_ARTIFACT_PAYLOAD_TOO_LARGE_CODE: &str = "auv.minecraft.structured_artifact.payload_too_large";

#[derive(Debug, thiserror::Error)]
pub enum MinecraftArtifactPublishError {
  #[error("invalid Minecraft artifact purpose {value:?}: {source}")]
  InvalidPurpose {
    value: &'static str,
    #[source]
    source: ValidationError,
  },
  #[error("Minecraft artifact {purpose} failed domain validation: {message}")]
  InvalidPayload {
    purpose: ArtifactPurpose,
    message: String,
  },
  #[error("failed to construct Minecraft artifact {purpose}: {source}")]
  Json {
    purpose: ArtifactPurpose,
    #[source]
    source: JsonArtifactError,
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
  #[error("failed to read Minecraft artifact: {source}")]
  Read {
    #[from]
    source: ReadArtifactError,
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
      Self::InvalidExpectedPurpose { .. } => "auv.minecraft.artifact.invalid_reader_contract",
      Self::Read {
        source: ReadArtifactError::PayloadTooLarge { .. },
      } => MINECRAFT_STRUCTURED_ARTIFACT_PAYLOAD_TOO_LARGE_CODE,
      Self::Read {
        source: ReadArtifactError::SnapshotAuthorityMismatch { .. },
      } => "auv.minecraft.artifact.snapshot_authority_mismatch",
      Self::Read {
        source: ReadArtifactError::WrongRun { .. },
      } => "auv.minecraft.artifact.wrong_owner",
      Self::Read {
        source: ReadArtifactError::NotCommitted { .. },
      } => "auv.minecraft.artifact.dangling_uri",
      Self::Read {
        source: ReadArtifactError::WrongPurpose { .. },
      } => "auv.minecraft.artifact.wrong_purpose",
      Self::Read {
        source: ReadArtifactError::WrongContentType { .. },
      } => "auv.minecraft.artifact.wrong_content_type",
      Self::Read {
        source: ReadArtifactError::LengthOutOfRange { .. },
      } => "auv.minecraft.artifact.length_out_of_range",
      Self::Read {
        source: ReadArtifactError::Allocation { .. },
      } => "auv.minecraft.artifact.allocation_failed",
      Self::Read {
        source: ReadArtifactError::Open { .. },
      } => "auv.minecraft.artifact.open_failed",
      Self::Read {
        source: ReadArtifactError::Stream { .. },
      } => "auv.minecraft.artifact.stream_failed",
      Self::Read {
        source: ReadArtifactError::LengthMismatch { .. },
      } => "auv.minecraft.artifact.length_mismatch",
      Self::Read {
        source: ReadArtifactError::DigestMismatch { .. },
      } => "auv.minecraft.artifact.digest_mismatch",
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
  let emission = context
    .in_scope(|| {
      auv_tracing::emit_json_artifact(
        purpose.clone(),
        Attributes::empty(),
        ByteLength::new(MINECRAFT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT).expect("static Minecraft JSON limit is valid"),
        value,
      )
    })
    .map_err(|source| MinecraftArtifactPublishError::Json {
      purpose: purpose.clone(),
      source,
    })?;
  emission.await.map_err(|source| MinecraftArtifactPublishError::Publication { purpose, source })
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
    return Err(
      ReadArtifactError::SnapshotAuthorityMismatch {
        snapshot_authority: snapshot.authority_id(),
        store_authority,
      }
      .into(),
    );
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
  let expected_purpose = expected_artifact_purpose(expected_purpose)?;
  let value = auv_tracing::read_json_artifact(
    store,
    snapshot,
    uri,
    &expected_purpose,
    ByteLength::new(MINECRAFT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT).expect("static Minecraft JSON limit is valid"),
  )
  .await
  .map_err(|error| match error {
    JsonArtifactReadError::Artifact(source) => MinecraftArtifactReadError::Read { source },
    JsonArtifactReadError::Decode { source, .. } => MinecraftArtifactReadError::MalformedJson {
      uri: uri.clone(),
      source,
    },
  })?;
  validate(&value).map_err(|message| MinecraftArtifactReadError::InvalidPayload {
    uri: uri.clone(),
    message,
  })?;
  Ok(value)
}

fn expected_artifact_purpose(value: &'static str) -> Result<ArtifactPurpose, MinecraftArtifactReadError> {
  ArtifactPurpose::parse(value).map_err(|source| MinecraftArtifactReadError::InvalidExpectedPurpose { value, source })
}
