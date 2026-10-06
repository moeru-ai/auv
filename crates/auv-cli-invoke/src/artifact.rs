use auv_tracing::{ArtifactMetadata, EmitBytesOptions, EventPayload, NewArtifact};

#[derive(serde::Serialize)]
struct ArtifactPreparationFailed {
  purpose: String,
  error: String,
}

impl EventPayload for ArtifactPreparationFailed {
  const NAME: &'static str = "auv.invoke.artifact_preparation_failed";
  const VERSION: u32 = 1;
}

/// Records a capture as evidence at logical resolution (see `ImageResolution::Logical`).
pub(crate) fn emit_capture(purpose: &str, capture: &auv_driver::Capture) {
  if !auv_tracing::Context::current().can_publish_artifacts() {
    return;
  }
  match prepare_capture(purpose, capture) {
    Ok(emission) => drop(emission),
    Err(error) => emit_preparation_failure(purpose, error),
  }
}

pub(crate) async fn emit_capture_with_receipt(purpose: &str, capture: &auv_driver::Capture) -> Option<ArtifactMetadata> {
  if !auv_tracing::Context::current().can_publish_artifacts() {
    return None;
  }
  let emission = match prepare_capture(purpose, capture) {
    Ok(emission) => emission,
    Err(error) => {
      emit_preparation_failure(purpose, error);
      return None;
    }
  };
  match emission.await {
    Ok(metadata) => metadata,
    Err(error) => {
      emit_preparation_failure(purpose, error.to_string());
      None
    }
  }
}

fn prepare_capture(purpose: &str, capture: &auv_driver::Capture) -> Result<auv_tracing::ArtifactEmission, String> {
  let options = EmitBytesOptions::new().with_purpose(purpose);
  let artifact = auv_tracing::image_artifact(options, &capture.image, auv_tracing::ImageResolution::Logical(capture.scale_factor))
    .map_err(|error| format!("{purpose}: {error}"))?;
  Ok(auv_tracing::emit_artifact(artifact))
}

fn emit_preparation_failure(purpose: &str, error: String) {
  auv_tracing::emit_event!(ArtifactPreparationFailed {
    purpose: purpose.to_string(),
    error,
  });
}

pub(crate) async fn emit_bytes_with_receipt(options: EmitBytesOptions, body: Vec<u8>) -> Option<ArtifactMetadata> {
  if !auv_tracing::Context::current().can_publish_artifacts() {
    return None;
  }
  let purpose = options.purpose().to_string();
  let emission = match auv_tracing::emit_bytes_artifact(options, body).map_err(|error| format!("invalid {purpose} artifact bytes: {error}"))
  {
    Ok(emission) => emission,
    Err(error) => {
      auv_tracing::emit_event!(ArtifactPreparationFailed { purpose, error });
      return None;
    }
  };
  emission.await.ok().flatten()
}

pub(crate) fn emit_prepared<R>(purpose: &str, artifact: Result<NewArtifact<R>, String>)
where
  R: futures_util::io::AsyncRead + Unpin + Send + 'static,
{
  if !auv_tracing::Context::current().can_publish_artifacts() {
    return;
  }
  match artifact {
    Ok(artifact) => drop(auv_tracing::emit_artifact!(artifact)),
    Err(error) => auv_tracing::emit_event!(ArtifactPreparationFailed {
      purpose: purpose.to_string(),
      error,
    }),
  }
}

pub(crate) async fn emit_prepared_with_receipt<R>(purpose: &str, artifact: Result<NewArtifact<R>, String>) -> Option<ArtifactMetadata>
where
  R: futures_util::io::AsyncRead + Unpin + Send + 'static,
{
  if !auv_tracing::Context::current().can_publish_artifacts() {
    return None;
  }
  let emission = match artifact {
    Ok(artifact) => auv_tracing::emit_artifact!(artifact),
    Err(error) => {
      emit_preparation_failure(purpose, error);
      return None;
    }
  };
  match emission.await {
    Ok(metadata) => metadata,
    Err(error) => {
      emit_preparation_failure(purpose, error.to_string());
      None
    }
  }
}

#[cfg(test)]
#[path = "artifact_test.rs"]
mod tests;
