//! Image evidence artifacts: one encoding for screenshots, OCR sources and
//! overlays across AUV and app crates.

use futures_util::io::Cursor;

use image::codecs::webp::WebPEncoder;
use image::{EncodableLayout, ImageBuffer, ImageEncoder, PixelWithColorType};

use crate::{EmitBytesOptions, NewArtifact, ValidationError};

/// Why an image could not become an artifact.
#[derive(Debug, thiserror::Error)]
pub enum ImageArtifactError {
  #[error("failed to encode image artifact: {0}")]
  Encode(#[from] image::ImageError),
  #[error("invalid image artifact: {0}")]
  Invalid(#[from] ValidationError),
}

/// Encodes `image` as an artifact body with `options`' purpose and attributes.
///
/// NOTICE(image-artifact-webp): evidence is lossless WebP (`image/webp`,
/// `.webp`): pixels round-trip exactly, which OCR evidence needs. On macOS
/// display and window captures (2026-10-07) it was 30–74% smaller than PNG
/// at 1.2–3.1× its encode time (~15–30 ms for a Retina window, 10–20 ms more
/// than PNG; capture itself takes hundreds of ms). Lossy AVIF was
/// rejected: 0.8–4 s per capture, and it blurs small text. See
/// `docs/ai/references/driver/2026-10-06-capture-references-and-positions-design.md`.
///
/// This only encodes; callers check `Context::can_publish_artifacts` first so
/// unrecorded calls skip the work, then emit the result.
pub fn image_artifact<P>(
  options: EmitBytesOptions,
  image: &ImageBuffer<P, Vec<P::Subpixel>>,
) -> Result<NewArtifact<Cursor<Vec<u8>>>, ImageArtifactError>
where
  P: PixelWithColorType,
  [P::Subpixel]: EncodableLayout,
{
  let mut body = Vec::new();
  WebPEncoder::new_lossless(&mut body).write_image(image.as_raw().as_bytes(), image.width(), image.height(), P::COLOR_TYPE)?;
  Ok(NewArtifact::from_bytes(options.with_content_type("image/webp").with_file_extension("webp"), body)?)
}

#[cfg(test)]
#[path = "image_test.rs"]
mod tests;
