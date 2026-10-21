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
  Ok(auv_tracing::emit_artifact(capture_evidence(purpose, capture, None)?))
}

/// Encodes a capture as a logical-resolution evidence image that records where
/// it sits on screen, so readers can place pointer actions on it.
///
/// The image is downscaled to logical points (see `ImageResolution::Logical`),
/// so a logical screen point `(x, y)` inside the capture is pixel
/// `(x - capture.bounds.x, y - capture.bounds.y)` of the stored image.
///
/// `capture_ref` is the Runner's `CaptureRef.capture_id`, which joins the image
/// to the RPC response that returned the reference.
///
/// This only encodes: callers check `Context::can_publish_artifacts` first and
/// emit the result, which lets a Runner encode off its request thread.
pub fn capture_evidence(
  purpose: &str,
  capture: &auv_driver::Capture,
  capture_ref: Option<&str>,
) -> Result<auv_tracing::NewArtifact<futures_util::io::Cursor<Vec<u8>>>, String> {
  let mut attributes = capture_attributes(capture)?;
  if let Some(capture_ref) = capture_ref {
    attributes = auv_tracing::Attributes::from_iter(
      attributes
        .iter()
        .map(|(key, value)| (key.to_string(), value.clone()))
        .chain([("capture.ref_id".to_string(), auv_tracing::AttributeValue::string(capture_ref))]),
    );
  }
  let options = EmitBytesOptions::new().with_purpose(purpose).with_attributes(attributes);
  auv_tracing::image_artifact(options, &capture.image, auv_tracing::ImageResolution::Logical(capture.scale_factor))
    .map_err(|error| format!("{purpose}: {error}"))
}

fn capture_attributes(capture: &auv_driver::Capture) -> Result<auv_tracing::Attributes, String> {
  use auv_tracing::AttributeValue;
  let float = |value: f64| AttributeValue::float(value).map_err(|error| format!("capture geometry is not finite: {error}"));
  let bounds = capture.bounds;
  let mut attributes = vec![
    ("capture.bounds.x".to_string(), float(bounds.origin.x)?),
    ("capture.bounds.y".to_string(), float(bounds.origin.y)?),
    ("capture.bounds.width".to_string(), float(bounds.size.width)?),
    ("capture.bounds.height".to_string(), float(bounds.size.height)?),
    ("capture.backend".to_string(), AttributeValue::string(&capture.backend)),
  ];
  match &capture.origin {
    Some(origin) => {
      let (space, id) = match &origin.coordinate_space {
        auv_driver::CoordinateSpace::Screen => ("screen", None),
        auv_driver::CoordinateSpace::Display(id) => ("display", Some(id)),
        auv_driver::CoordinateSpace::Window(id) => ("window", Some(id)),
      };
      attributes.push(("capture.origin.space".to_string(), AttributeValue::string(space)));
      if let Some(id) = id {
        attributes.push(("capture.origin.id".to_string(), AttributeValue::string(id)));
      }
      attributes.push(("capture.origin.x".to_string(), float(origin.point.x)?));
      attributes.push(("capture.origin.y".to_string(), float(origin.point.y)?));
    }
    None => attributes.push(("capture.origin.space".to_string(), AttributeValue::string("unbound"))),
  }
  Ok(auv_tracing::Attributes::from_iter(attributes))
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
