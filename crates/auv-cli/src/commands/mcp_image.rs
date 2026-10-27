//! Explicit image fetch from the MCP server's own receipts. No caller paths or store scans.
use std::collections::VecDeque;
use std::io::{Cursor, Read};
use std::path::PathBuf;
use std::sync::Mutex;

use auv_tracing::{ArtifactId, ArtifactMetadata, FileTracingStore, RunId, Sha256Digest};
use base64::Engine as _;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits, imageops::FilterType};
use rmcp::model::{CallToolResult, Content};
use sha2::{Digest, Sha256};

const MAX_SOURCE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_SOURCE_PIXELS: u64 = 16 * 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
const MAX_SESSION_IMAGES: usize = 128;

#[derive(Default)]
pub(super) struct ImageArtifacts {
  entries: Mutex<VecDeque<(ArtifactMetadata, PathBuf)>>,
}

impl ImageArtifacts {
  pub(super) fn register(&self, artifacts: &[ArtifactMetadata], store: &FileTracingStore) -> Result<(), String> {
    let mut entries = self.entries.lock().map_err(|_| "image catalog unavailable")?;
    for metadata in artifacts {
      if image_format(metadata).is_none() {
        continue;
      }
      // Both path and digest originate in the successful AUV artifact write receipt.
      // No metadata is loaded from a caller-selected path or a pre-existing store.
      // Receipt order also covers images returned from an older, reused Run.
      entries.retain(|(existing, _)| existing.uri() != metadata.uri());
      entries.push_back((metadata.clone(), store.artifact_path(metadata)));
      while entries.len() > MAX_SESSION_IMAGES {
        entries.pop_front();
      }
    }
    Ok(())
  }

  pub(super) fn read(&self, run_id: &str, artifact_id: &str, max_width: u32, max_height: u32) -> Result<CallToolResult, String> {
    let run_id: RunId = run_id.parse().map_err(|_| "run_id must be a canonical non-nil UUID")?;
    let artifact_id: ArtifactId = artifact_id.parse().map_err(|_| "artifact_id must be a canonical non-nil UUID")?;
    if !(1..=4096).contains(&max_width) || !(1..=4096).contains(&max_height) {
      return Err("image dimensions must be within 1..=4096".into());
    }
    let (metadata, path) = self
      .entries
      .lock()
      .map_err(|_| "image catalog unavailable")?
      .iter()
      .find(|(metadata, _)| metadata.uri().run_id() == run_id && metadata.uri().artifact_id() == artifact_id)
      .cloned()
      .ok_or("image artifact not available in this MCP server instance")?;
    if metadata.byte_length().get() > MAX_SOURCE_BYTES {
      return Err("source image exceeds 16 MiB".into());
    }
    let file = std::fs::File::open(path).map_err(|_| "image artifact cannot be opened")?;
    let info = file.metadata().map_err(|_| "image artifact metadata unavailable")?;
    if !info.is_file() || info.len() != metadata.byte_length().get() {
      return Err("image artifact length changed".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_SOURCE_BYTES + 1).read_to_end(&mut bytes).map_err(|_| "image artifact cannot be read")?;
    if bytes.len() as u64 != metadata.byte_length().get() || Sha256Digest::new(Sha256::digest(&bytes).into()) != metadata.sha256() {
      return Err("image artifact integrity check failed".into());
    }
    let format = image_format(&metadata).ok_or("unsupported image content type")?;
    if image::guess_format(&bytes).ok() != Some(format) {
      return Err("image content type does not match its bytes".into());
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    // PNG metadata can expand while the decoder reads its header. Apply the
    // budget before construction and retain that decoder for pixel decoding.
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits.clone());
    let mut decoder = reader.into_decoder().map_err(|_| "invalid image header or exceeded dimension/allocation limit")?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_SOURCE_PIXELS {
      return Err("source image exceeds dimension or 16 megapixel limit".into());
    }
    // Preserve ImageReader::decode's output-buffer reservation when reusing the
    // decoder (`image` 0.25.10, src/io/image_reader_type.rs).
    limits.reserve(decoder.total_bytes()).map_err(|_| "source image exceeds 128 MiB pixel allocation limit")?;
    decoder.set_limits(limits).map_err(|_| "image decoder rejected remaining allocation budget")?;
    let image = DynamicImage::from_decoder(decoder).map_err(|_| "image decoding failed or exceeded allocation limit")?;
    let image = if width > max_width || height > max_height {
      // NOTICE: image 0.25.10 thumbnail sums u16 channels in u32 and can overflow.
      // Triangle uses floating-point samples. Reconsider thumbnail only after
      // image/src/imageops/sample.rs supports large 16-bit sampling regions.
      image.resize(max_width, max_height, FilterType::Triangle)
    } else {
      image
    };
    let dimensions = (image.width(), image.height());
    let mut output = Cursor::new(Vec::new());
    image.write_to(&mut output, ImageFormat::Png).map_err(|_| "image encoding failed")?;
    if output.get_ref().len() > MAX_OUTPUT_BYTES {
      return Err("encoded image exceeds 8 MiB; request a smaller image".into());
    }
    Ok(CallToolResult::success(vec![
      Content::text(
        serde_json::json!({ "run_id": run_id, "artifact_id": artifact_id,
        "source_width": width, "source_height": height, "width": dimensions.0, "height": dimensions.1 })
        .to_string(),
      ),
      Content::image(base64::engine::general_purpose::STANDARD.encode(output.into_inner()), "image/png"),
    ]))
  }
}

fn image_format(metadata: &ArtifactMetadata) -> Option<ImageFormat> {
  match metadata.content_type().as_str() {
    "image/png" => Some(ImageFormat::Png),
    "image/jpeg" => Some(ImageFormat::Jpeg),
    "image/webp" => Some(ImageFormat::WebP),
    _ => None,
  }
}

// NOTICE(mcp-image-instance-scope): durable lookup across MCP restarts is deferred.
// A separate authorized store-reader contract is needed before persisted run IDs
// can be admitted; otherwise a task agent could fetch another run's private evidence.

#[cfg(test)]
#[path = "mcp_image_test.rs"]
mod tests;
