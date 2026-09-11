//! Balatro tracing artifact producers.

use auv_tracing::{
  ArtifactMetadata, ArtifactPurpose, AttributeValue, Attributes, ByteLength, Context, EmitBytesOptions, EventPayload, JsonArtifactError,
  StoreError,
};
use serde::Serialize;

pub const BALATRO_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT: u64 = 4 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum BalatroArtifactPublishError {
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

pub(crate) fn emit_png_artifact(purpose: &'static str, source: &str, image: &image::RgbaImage) {
  let context = Context::current();
  if !context.can_publish_artifacts() {
    return;
  }
  let mut encoded = std::io::Cursor::new(Vec::new());
  if let Err(error) = image::DynamicImage::ImageRgba8(image.clone()).write_to(&mut encoded, image::ImageFormat::Png) {
    context.in_scope(|| {
      auv_tracing::emit_event!(BalatroArtifactPreparationFailed {
        purpose,
        error: error.to_string(),
      });
    });
    return;
  }
  let options = EmitBytesOptions::new()
    .with_purpose(purpose)
    .with_content_type("image/png")
    .with_file_extension("png")
    .with_attributes(Attributes::from_iter([("source", AttributeValue::string(source))]));
  match auv_tracing::emit_bytes_artifact(options, encoded.into_inner()) {
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
  let purpose = ArtifactPurpose::new(purpose);
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

fn prepare_json_emission<T: Serialize>(
  purpose: &'static str,
  value: &T,
) -> Result<auv_tracing::ArtifactEmission, BalatroArtifactPublishError> {
  let purpose = ArtifactPurpose::new(purpose);
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

#[cfg(test)]
mod tests {
  use std::sync::Arc;

  use auv_tracing::{Context, MemoryTracingStore, RunId, TraceRecord, configure, dispatcher};

  use super::emit_png_artifact;

  #[test]
  fn observed_frame_png_is_visible_in_the_run_store() {
    futures_executor::block_on(async {
      let store = Arc::new(MemoryTracingStore::new());
      let dispatch = configure().tracing_store(store.clone()).build().expect("memory tracing dispatch");
      let context = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));

      context.in_scope(|| emit_png_artifact("auv.balatro.observation.capture", "fixture://before", &image::RgbaImage::new(2, 2)));
      dispatch.flush().await.expect("flush tracing");

      assert!(store.records().iter().any(|record| {
        matches!(record, TraceRecord::Artifact { metadata, .. } if metadata.purpose().as_str() == "auv.balatro.observation.capture")
      }));
    });
  }
}
