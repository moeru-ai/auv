//! Osu ordinary run_read helpers for inspect composition.
//!
//! Depends on canonical `auv-tracing` run snapshots only (no `auv-cli`).
//! Product query-wired presentation consumes these same typed artifacts.

use auv_tracing::{
  ArtifactMetadata, ArtifactPurpose, ArtifactUri, ArtifactWriteError, Attributes, ByteLength, Context, ErrorCode, JsonArtifactError,
  JsonArtifactReadError, ReadArtifactError, RunSnapshot, RunStore, ValidationError,
};
use serde::Serialize;

pub const OSU_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT: u64 = 4 * 1024 * 1024;
pub const OSU_STRUCTURED_ARTIFACT_PAYLOAD_TOO_LARGE_CODE: &str = "auv.osu.structured_artifact.payload_too_large";

#[derive(Debug, thiserror::Error)]
pub enum OsuArtifactPublishError {
  #[error("invalid osu! artifact purpose {value:?}: {source}")]
  InvalidPurpose {
    value: &'static str,
    #[source]
    source: ValidationError,
  },
  #[error("osu! artifact {purpose} failed domain validation: {message}")]
  InvalidPayload {
    purpose: ArtifactPurpose,
    message: String,
  },
  #[error("failed to construct osu! artifact {purpose}: {source}")]
  Json {
    purpose: ArtifactPurpose,
    #[source]
    source: JsonArtifactError,
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
  #[error("failed to read osu! artifact: {source}")]
  Read {
    #[from]
    source: ReadArtifactError,
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
      Self::InvalidExpectedPurpose { .. } => "auv.osu.artifact.invalid_reader_contract",
      Self::Read {
        source: ReadArtifactError::PayloadTooLarge { .. },
      } => OSU_STRUCTURED_ARTIFACT_PAYLOAD_TOO_LARGE_CODE,
      Self::Read {
        source: ReadArtifactError::SnapshotAuthorityMismatch { .. },
      } => "auv.osu.artifact.snapshot_authority_mismatch",
      Self::Read {
        source: ReadArtifactError::WrongRun { .. },
      } => "auv.osu.artifact.wrong_owner",
      Self::Read {
        source: ReadArtifactError::NotCommitted { .. },
      } => "auv.osu.artifact.dangling_uri",
      Self::Read {
        source: ReadArtifactError::WrongPurpose { .. },
      } => "auv.osu.artifact.wrong_purpose",
      Self::Read {
        source: ReadArtifactError::WrongContentType { .. },
      } => "auv.osu.artifact.wrong_content_type",
      Self::Read {
        source: ReadArtifactError::LengthOutOfRange { .. },
      } => "auv.osu.artifact.length_out_of_range",
      Self::Read {
        source: ReadArtifactError::Allocation { .. },
      } => "auv.osu.artifact.allocation_failed",
      Self::Read {
        source: ReadArtifactError::Open { .. },
      } => "auv.osu.artifact.open_failed",
      Self::Read {
        source: ReadArtifactError::Stream { .. },
      } => "auv.osu.artifact.stream_failed",
      Self::Read {
        source: ReadArtifactError::LengthMismatch { .. },
      } => "auv.osu.artifact.length_mismatch",
      Self::Read {
        source: ReadArtifactError::DigestMismatch { .. },
      } => "auv.osu.artifact.digest_mismatch",
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
  let emission = context
    .in_scope(|| {
      auv_tracing::emit_json_artifact(
        purpose.clone(),
        Attributes::empty(),
        ByteLength::new(OSU_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT).expect("static osu! JSON limit is valid"),
        value,
      )
    })
    .map_err(|source| OsuArtifactPublishError::Json {
      purpose: purpose.clone(),
      source,
    })?;
  emission.await.map_err(|source| OsuArtifactPublishError::Publication { purpose, source })
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
  let expected_purpose = ArtifactPurpose::parse(expected_purpose).map_err(|source| OsuArtifactReadError::InvalidExpectedPurpose {
    value: expected_purpose,
    source,
  })?;
  let value = auv_tracing::read_json_artifact(
    store,
    snapshot,
    uri,
    &expected_purpose,
    ByteLength::new(OSU_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT).expect("static osu! JSON limit is valid"),
  )
  .await
  .map_err(|error| match error {
    JsonArtifactReadError::Artifact(source) => OsuArtifactReadError::Read { source },
    JsonArtifactReadError::Decode { source, .. } => OsuArtifactReadError::MalformedJson {
      uri: uri.clone(),
      source,
    },
  })?;
  validate(&value).map_err(|message| OsuArtifactReadError::InvalidPayload {
    uri: uri.clone(),
    message,
  })?;
  Ok(value)
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
