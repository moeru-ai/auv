use auv_tracing::{ArtifactMetadata, ArtifactPurpose, Attributes, ContentType, EventPayload, NewArtifact};
use image::{ExtendedColorType, ImageEncoder, RgbaImage, codecs::png::PngEncoder};

#[derive(serde::Serialize)]
struct ArtifactPreparationFailed {
  purpose: String,
  error: String,
}

impl EventPayload for ArtifactPreparationFailed {
  const NAME: &'static str = "auv.invoke.artifact_preparation_failed";
  const VERSION: u32 = 1;
}

pub(crate) fn emit_png(purpose: &str, image: &RgbaImage) {
  if !auv_tracing::Context::current().can_publish_artifacts() {
    return;
  }
  let mut body = Vec::new();
  let emission = PngEncoder::new(&mut body)
    .write_image(image.as_raw(), image.width(), image.height(), ExtendedColorType::Rgba8)
    .map_err(|error| format!("failed to encode {purpose} PNG artifact: {error}"))
    .and_then(|()| {
      auv_tracing::emit_bytes_artifact(
        ArtifactPurpose::parse(purpose).map_err(|error| format!("invalid {purpose} artifact purpose: {error}"))?,
        ContentType::parse("image/png").expect("static PNG content type is valid"),
        Attributes::empty(),
        body,
      )
      .map_err(|error| format!("invalid {purpose} artifact bytes: {error}"))
    });
  match emission {
    Ok(emission) => drop(emission),
    Err(error) => auv_tracing::emit_event!(ArtifactPreparationFailed {
      purpose: purpose.to_string(),
      error,
    }),
  }
}

pub(crate) async fn emit_bytes_with_receipt(purpose: &str, content_type: &str, body: Vec<u8>) -> Option<ArtifactMetadata> {
  if !auv_tracing::Context::current().can_publish_artifacts() {
    return None;
  }
  let emission = match ArtifactPurpose::parse(purpose).map_err(|error| format!("invalid {purpose} artifact purpose: {error}")).and_then(
    |parsed_purpose| {
      let content_type = ContentType::parse(content_type).map_err(|error| format!("invalid artifact content type: {error}"))?;
      auv_tracing::emit_bytes_artifact(parsed_purpose, content_type, Attributes::empty(), body)
        .map_err(|error| format!("invalid {purpose} artifact bytes: {error}"))
    },
  ) {
    Ok(emission) => emission,
    Err(error) => {
      auv_tracing::emit_event!(ArtifactPreparationFailed {
        purpose: purpose.to_string(),
        error,
      });
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

#[cfg(test)]
#[path = "artifact_test.rs"]
mod tests;
