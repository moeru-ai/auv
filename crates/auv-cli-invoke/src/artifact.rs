use auv_tracing::{ArtifactPurpose, Attributes, ByteLength, ContentType, NewArtifact, Sha256Digest};
use futures_util::io::Cursor as AsyncCursor;
use image::{DynamicImage, ImageFormat, RgbaImage};
use sha2::{Digest, Sha256};

pub(crate) type OwnedArtifact = NewArtifact<AsyncCursor<Vec<u8>>>;

pub(crate) fn emission_enabled() -> bool {
  auv_tracing::Context::current().authority_id().is_some()
}

pub(crate) fn record_uri(
  output: &mut crate::InvokeCommandOutput,
  signal: &str,
  emission: Result<Option<auv_tracing::ArtifactMetadata>, auv_tracing::ArtifactWriteError>,
) {
  match emission {
    Ok(Some(metadata)) => {
      output.signals.insert(signal.to_string(), metadata.uri().to_string());
    }
    Ok(None) => {}
    Err(error) => output.notes.push(format!("artifact instrumentation failed: {error}")),
  }
}

pub(crate) fn json_artifact<T: serde::Serialize>(purpose: &str, value: &T, attributes: Attributes) -> Result<OwnedArtifact, String> {
  let body = serde_json::to_vec_pretty(value).map_err(|error| format!("failed to serialize {purpose} artifact: {error}"))?;
  bytes_artifact(purpose, "application/json", body, attributes)
}

pub(crate) fn png_artifact(purpose: &str, image: &RgbaImage, attributes: Attributes) -> Result<OwnedArtifact, String> {
  let mut body = std::io::Cursor::new(Vec::new());
  DynamicImage::ImageRgba8(image.clone())
    .write_to(&mut body, ImageFormat::Png)
    .map_err(|error| format!("failed to encode {purpose} artifact: {error}"))?;
  bytes_artifact(purpose, "image/png", body.into_inner(), attributes)
}

pub(crate) fn file_artifact(
  purpose: &str,
  content_type: &str,
  path: &std::path::Path,
  attributes: Attributes,
) -> Result<OwnedArtifact, String> {
  let body = std::fs::read(path).map_err(|error| format!("failed to read {purpose} artifact bytes: {error}"))?;
  bytes_artifact(purpose, content_type, body, attributes)
}

fn bytes_artifact(purpose: &str, content_type: &str, body: Vec<u8>, attributes: Attributes) -> Result<OwnedArtifact, String> {
  let byte_length = u64::try_from(body.len()).map_err(|_| format!("{purpose} artifact length does not fit u64"))?;
  Ok(NewArtifact::new(
    ArtifactPurpose::parse(purpose).map_err(|error| format!("invalid {purpose} artifact purpose: {error}"))?,
    ContentType::parse(content_type).map_err(|error| format!("invalid {purpose} artifact content type: {error}"))?,
    ByteLength::new(byte_length).map_err(|error| format!("invalid {purpose} artifact length: {error}"))?,
    Sha256Digest::new(Sha256::digest(&body).into()),
    attributes,
    AsyncCursor::new(body),
  ))
}
