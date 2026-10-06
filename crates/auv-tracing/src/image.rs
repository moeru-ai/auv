//! Image evidence artifacts: one encoding for screenshots, OCR sources and
//! overlays across AUV and app crates.

use futures_util::io::Cursor;

use image::codecs::webp::WebPEncoder;
use image::{ImageBuffer, ImageEncoder, PixelWithColorType};

use crate::{AttributeValue, Attributes, EmitBytesOptions, NewArtifact, ValidationError};

/// The pixel resolution an image artifact is stored at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ImageResolution {
  /// Every captured pixel. For evidence that must match an algorithm's exact
  /// input, such as OCR region-of-interest probes.
  Native,
  /// Downscaled by the capture's backing scale factor to logical points (1×).
  /// Bounds, OCR boxes and click points are logical, so the image lines up
  /// with them, at a quarter of a Retina capture's pixels.
  Logical(f64),
}

/// Why an image could not become an artifact.
#[derive(Debug, thiserror::Error)]
pub enum ImageArtifactError {
  #[error("failed to encode image artifact: {0}")]
  Encode(#[from] image::ImageError),
  #[error("invalid image artifact: {0}")]
  Invalid(#[from] ValidationError),
}

/// Encodes `image` at `resolution` as an artifact body with `options`'
/// purpose and attributes. A downscaled artifact records its source size in
/// the `image.source_width`, `image.source_height` and `image.scale_factor`
/// attributes.
///
/// NOTICE(image-artifact-webp): evidence is lossless WebP (`image/webp`,
/// `.webp`): pixels round-trip exactly, which OCR evidence needs. On macOS
/// display and window captures (2026-10-07) it was 30–74% smaller than PNG
/// at 1.2–3.1× its encode time (~15–30 ms for a Retina window, 10–20 ms more
/// than PNG; capture itself takes hundreds of ms). Lossy AVIF was
/// rejected: 0.8–4 s per capture, and it blurs small text. See
/// `docs/ai/references/driver/2026-10-06-capture-references-and-positions-design.md`.
///
/// NOTICE(image-artifact-logical): logical downscaling averages pixel areas
/// (`imageops::thumbnail`): it is what a 1× display shows, and took ~9 ms for
/// a Retina window, versus 21–37 ms for Triangle, CatmullRom or Lanczos3.
///
/// This only encodes; callers check `Context::can_publish_artifacts` first so
/// unrecorded calls skip the work, then emit the result.
pub fn image_artifact<P>(
  options: EmitBytesOptions,
  image: &ImageBuffer<P, Vec<u8>>,
  resolution: ImageResolution,
) -> Result<NewArtifact<Cursor<Vec<u8>>>, ImageArtifactError>
where
  P: PixelWithColorType<Subpixel = u8> + 'static,
{
  let (source_width, source_height) = image.dimensions();
  let logical = match resolution {
    ImageResolution::Logical(scale) if scale.is_finite() && scale > 1.0 => {
      let width = ((f64::from(source_width) / scale).round() as u32).max(1);
      let height = ((f64::from(source_height) / scale).round() as u32).max(1);
      Some((image::imageops::thumbnail(image, width, height), scale))
    }
    _ => None,
  };
  let (encoded, options) = match &logical {
    Some((downscaled, scale)) => {
      let attributes = options.attributes().iter().map(|(key, value)| (key.to_string(), value.clone())).chain([
        ("image.source_width".to_string(), AttributeValue::integer(i64::from(source_width))),
        ("image.source_height".to_string(), AttributeValue::integer(i64::from(source_height))),
        ("image.scale_factor".to_string(), AttributeValue::float(*scale)?),
      ]);
      let attributes = Attributes::from_iter(attributes);
      (downscaled, options.with_attributes(attributes))
    }
    None => (image, options),
  };
  let mut body = Vec::new();
  WebPEncoder::new_lossless(&mut body).write_image(encoded.as_raw(), encoded.width(), encoded.height(), P::COLOR_TYPE)?;
  Ok(NewArtifact::from_bytes(options.with_content_type("image/webp").with_file_extension("webp"), body)?)
}

#[cfg(test)]
#[path = "image_test.rs"]
mod tests;
