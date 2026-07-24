//! Canonical Balatro run-artifact transport shared by typed domain readers.

use auv_tracing::{
  ArtifactMetadata, ArtifactPurpose, ArtifactUri, ArtifactWriteError, Attributes, ByteLength, Context, ErrorCode, EventPayload,
  JsonArtifactError, JsonArtifactReadError, ReadArtifactError, RunSnapshot, RunStore, ValidationError,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Balatro card-detection manifests are structured metadata, not bulk media.
pub const BALATRO_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT: u64 = 4 * 1024 * 1024;
pub const BALATRO_STRUCTURED_ARTIFACT_PAYLOAD_TOO_LARGE_CODE: &str = "auv.balatro.structured_artifact.payload_too_large";

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
  #[error("failed to read Balatro artifact: {source}")]
  Read {
    #[from]
    source: ReadArtifactError,
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
      Self::InvalidExpectedPurpose { .. } => "auv.balatro.artifact.invalid_reader_contract",
      Self::Read {
        source: ReadArtifactError::PayloadTooLarge { .. },
      } => BALATRO_STRUCTURED_ARTIFACT_PAYLOAD_TOO_LARGE_CODE,
      Self::Read {
        source: ReadArtifactError::SnapshotAuthorityMismatch { .. },
      } => "auv.balatro.artifact.snapshot_authority_mismatch",
      Self::Read {
        source: ReadArtifactError::WrongRun { .. },
      } => "auv.balatro.artifact.wrong_owner",
      Self::Read {
        source: ReadArtifactError::NotCommitted { .. },
      } => "auv.balatro.artifact.dangling_uri",
      Self::Read {
        source: ReadArtifactError::WrongPurpose { .. },
      } => "auv.balatro.artifact.wrong_purpose",
      Self::Read {
        source: ReadArtifactError::WrongContentType { .. },
      } => "auv.balatro.artifact.wrong_content_type",
      Self::Read {
        source: ReadArtifactError::LengthOutOfRange { .. },
      } => "auv.balatro.artifact.length_out_of_range",
      Self::Read {
        source: ReadArtifactError::Allocation { .. },
      } => "auv.balatro.artifact.allocation_failed",
      Self::Read {
        source: ReadArtifactError::Open { .. },
      } => "auv.balatro.artifact.open_failed",
      Self::Read {
        source: ReadArtifactError::Stream { .. },
      } => "auv.balatro.artifact.stream_failed",
      Self::Read {
        source: ReadArtifactError::LengthMismatch { .. },
      } => "auv.balatro.artifact.length_mismatch",
      Self::Read {
        source: ReadArtifactError::DigestMismatch { .. },
      } => "auv.balatro.artifact.digest_mismatch",
      Self::MalformedJson { .. } => "auv.balatro.artifact.malformed_json",
    };
    ErrorCode::parse(value).expect("static Balatro artifact error code is valid")
  }
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
  // Contexts without artifact authority, including telemetry-only contexts,
  // must not validate the contract or serialize the domain value.
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

pub(crate) async fn read_json_artifact<T>(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
  expected_purpose: &'static str,
) -> Result<T, BalatroArtifactReadError>
where
  T: DeserializeOwned,
{
  let expected_purpose = expected_artifact_purpose(expected_purpose)?;
  auv_tracing::read_json_artifact(
    store,
    snapshot,
    uri,
    &expected_purpose,
    ByteLength::new(BALATRO_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT).expect("static Balatro JSON limit is valid"),
  )
  .await
  .map_err(|error| match error {
    JsonArtifactReadError::Artifact(source) => BalatroArtifactReadError::Read { source },
    JsonArtifactReadError::Decode { source, .. } => BalatroArtifactReadError::MalformedJson {
      uri: uri.clone(),
      source,
    },
  })
}

fn expected_artifact_purpose(value: &'static str) -> Result<ArtifactPurpose, BalatroArtifactReadError> {
  ArtifactPurpose::parse(value).map_err(|source| BalatroArtifactReadError::InvalidExpectedPurpose { value, source })
}

#[cfg(test)]
mod tests {
  use std::error::Error as _;
  use std::sync::Arc;

  use auv_driver::{INPUT_ACTION_RESULT_PURPOSE, InputActionResult, InputDeliveryPath};
  use auv_tracing::{
    AuthorityId, BoxFuture, Context, MemoryRunStore, RunId, TelemetryError, TelemetryItem, TelemetryProjector, TelemetryRoutePolicy,
    configure, dispatcher,
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

  #[test]
  fn detached_delivery_recording_keeps_the_direct_value_and_uses_the_active_run_store() {
    futures_executor::block_on(async {
      let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
      let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
      let run_id = RunId::new();
      let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
      let delivery = InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse);

      let direct = root.in_scope(|| {
        emit_json_artifact(INPUT_ACTION_RESULT_PURPOSE, &delivery);
        42
      });
      dispatch.flush().await.expect("flush delivery artifact");
      let snapshot = store.load_snapshot(run_id).await.expect("load snapshot").expect("delivery run");

      assert_eq!(direct, 42);
      assert_eq!(snapshot.artifacts().len(), 1);
      assert_eq!(
        snapshot.artifacts().values().next().expect("delivery artifact").metadata().purpose().as_str(),
        INPUT_ACTION_RESULT_PURPOSE
      );
    });
  }
}
