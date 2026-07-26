//! Balatro tracing artifact producers.

use auv_tracing::{
  ArtifactMetadata, ArtifactPurpose, Attributes, ByteLength, Context, EventPayload, JsonArtifactError, StoreError, ValidationError,
};
use serde::Serialize;

pub const BALATRO_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT: u64 = 4 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum BalatroArtifactPublishError {
  #[error("invalid Balatro artifact purpose {value:?}: {source}")]
  InvalidPurpose {
    value: &'static str,
    #[source]
    source: ValidationError,
  },
  #[error("failed to construct Balatro artifact {purpose}: {source}")]
  Json {
    purpose: ArtifactPurpose,
    #[source]
    source: JsonArtifactError,
  },
  #[error("failed to publish Balatro artifact {purpose}: {source}")]
  Publication {
    purpose: ArtifactPurpose,
    #[source]
    source: StoreError,
  },
}

#[derive(Serialize)]
struct BalatroArtifactPreparationFailed {
  purpose: &'static str,
  error: String,
}

impl EventPayload for BalatroArtifactPreparationFailed {
  const NAME: &'static str = "auv.balatro.artifact_preparation_failed";
  const VERSION: u32 = 1;
}

pub(crate) fn emit_json_artifact<T: Serialize>(purpose: &'static str, value: &T) {
  let context = Context::current();
  if !context.can_publish_artifacts() {
    return;
  }
  match prepare_json_emission(purpose, value) {
    Ok(emission) => drop(emission),
    Err(error) => context.in_scope(|| {
      auv_tracing::emit_event!(BalatroArtifactPreparationFailed {
        purpose,
        error: error.to_string(),
      });
    }),
  }
}

pub(crate) async fn publish_json_artifact<T: Serialize>(
  context: Option<&Context>,
  purpose: &'static str,
  value: &T,
) -> Result<Option<ArtifactMetadata>, BalatroArtifactPublishError> {
  let Some(context) = context.filter(|context| context.can_publish_artifacts()) else {
    return Ok(None);
  };
  let purpose = parse_artifact_purpose(purpose)?;
  let emission = context
    .in_scope(|| {
      auv_tracing::emit_json_artifact(
        purpose.clone(),
        Attributes::empty(),
        ByteLength::new(BALATRO_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT).expect("static Balatro JSON limit is valid"),
        value,
      )
    })
    .map_err(|source| BalatroArtifactPublishError::Json {
      purpose: purpose.clone(),
      source,
    })?;
  emission.await.map_err(|source| BalatroArtifactPublishError::Publication { purpose, source })
}

fn parse_artifact_purpose(purpose: &'static str) -> Result<ArtifactPurpose, BalatroArtifactPublishError> {
  ArtifactPurpose::parse(purpose).map_err(|source| BalatroArtifactPublishError::InvalidPurpose {
    value: purpose,
    source,
  })
}

fn prepare_json_emission<T: Serialize>(
  purpose: &'static str,
  value: &T,
) -> Result<auv_tracing::ArtifactEmission, BalatroArtifactPublishError> {
  let purpose = parse_artifact_purpose(purpose)?;
  auv_tracing::emit_json_artifact(
    purpose.clone(),
    Attributes::empty(),
    ByteLength::new(BALATRO_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT).expect("static Balatro JSON limit is valid"),
    value,
  )
  .map_err(|source| BalatroArtifactPublishError::Json { purpose, source })
}

// TODO(auv-inspector): typed Balatro artifact readers remain deferred until an
// owner-approved inspector contract defines discovery and validation inputs.
