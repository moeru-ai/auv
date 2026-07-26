//! Minecraft tracing artifact producers.

use auv_tracing::{ArtifactMetadata, ArtifactPurpose, Attributes, ByteLength, Context, JsonArtifactError, StoreError, ValidationError};
use serde::Serialize;

pub const MINECRAFT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT: u64 = 4 * 1024 * 1024;

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
    source: StoreError,
  },
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

// TODO(auv-inspector): typed Minecraft artifact readers remain deferred until
// an owner-approved inspector contract defines discovery and validation inputs.
