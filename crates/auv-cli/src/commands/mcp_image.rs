//! Explicit image fetch from the MCP server's own receipts. No caller paths or store scans.
use std::collections::VecDeque;
use std::io::{Cursor, Read};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use auv_tracing::{ArtifactId, ArtifactMetadata, FileTracingStore, RunId, Sha256Digest};
use base64::Engine as _;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits, imageops::FilterType};
use rmcp::{
  ErrorData as McpError,
  model::{CallToolResult, Content},
};
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
  fn lock_entries(&self) -> MutexGuard<'_, VecDeque<(ArtifactMetadata, PathBuf)>> {
    match self.entries.lock() {
      Ok(entries) => entries,
      Err(poisoned) => {
        tracing::warn!("recovering poisoned MCP image receipt catalog");
        // Queue mutations insert or remove complete receipts. Unwinding cannot
        // change their identity or path, but can interrupt FIFO eviction.
        let mut entries = poisoned.into_inner();
        while entries.len() > MAX_SESSION_IMAGES {
          entries.pop_front();
        }
        self.entries.clear_poison();
        entries
      }
    }
  }

  pub(super) fn register(&self, artifacts: &[ArtifactMetadata], store: &FileTracingStore) {
    let mut entries = self.lock_entries();
    for metadata in artifacts {
      if image_format(metadata).is_none() {
        continue;
      }
      // Both path and digest originate in the successful AUV artifact write receipt.
      // No metadata is loaded from a caller-selected path or a pre-existing store.
      // Receipt order also covers images returned from an older, reused Run.
      // Prepare the complete replacement before removing an existing receipt.
      let receipt = (metadata.clone(), store.artifact_path(metadata));
      entries.retain(|(existing, _)| existing.uri() != metadata.uri());
      entries.push_back(receipt);
      while entries.len() > MAX_SESSION_IMAGES {
        entries.pop_front();
      }
    }
  }

  pub(super) fn read(&self, run_id: &str, artifact_id: &str, max_width: u32, max_height: u32) -> Result<CallToolResult, McpError> {
    let run_id: RunId = run_id.parse().map_err(|_| McpError::invalid_params("run_id must be a canonical non-nil UUID", None))?;
    let artifact_id: ArtifactId =
      artifact_id.parse().map_err(|_| McpError::invalid_params("artifact_id must be a canonical non-nil UUID", None))?;
    if !(1..=4096).contains(&max_width) || !(1..=4096).contains(&max_height) {
      return Err(McpError::invalid_params("image dimensions must be within 1..=4096", None));
    }
    let (metadata, path) = self
      .lock_entries()
      .iter()
      .find(|(metadata, _)| metadata.uri().run_id() == run_id && metadata.uri().artifact_id() == artifact_id)
      .cloned()
      .ok_or_else(|| McpError::invalid_params("image artifact not available in this MCP server instance", None))?;
    if metadata.byte_length().get() > MAX_SOURCE_BYTES {
      return Err(McpError::invalid_params("source image exceeds 16 MiB", None));
    }
    let file = std::fs::File::open(path).map_err(|_| McpError::internal_error("image artifact cannot be opened", None))?;
    let info = file.metadata().map_err(|_| McpError::internal_error("image artifact metadata unavailable", None))?;
    if !info.is_file() || info.len() != metadata.byte_length().get() {
      return Err(McpError::internal_error("image artifact length changed", None));
    }
    let mut bytes = Vec::with_capacity(metadata.byte_length().get() as usize);
    file.take(MAX_SOURCE_BYTES + 1).read_to_end(&mut bytes).map_err(|_| McpError::internal_error("image artifact cannot be read", None))?;
    if bytes.len() as u64 != metadata.byte_length().get() || Sha256Digest::new(Sha256::digest(&bytes).into()) != metadata.sha256() {
      return Err(McpError::internal_error("image artifact integrity check failed", None));
    }
    let format = image_format(&metadata).ok_or_else(|| McpError::invalid_params("unsupported image content type", None))?;
    if image::guess_format(&bytes).ok() != Some(format) {
      return Err(McpError::invalid_params("image content type does not match its bytes", None));
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    // PNG metadata can expand while the decoder reads its header. Apply the
    // budget before construction and retain that decoder for pixel decoding.
    // ImageReader forwards constructor limits to PNG; JPEG and WebP receive
    // limits after their header parse. Source bytes are already bounded above.
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits.clone());
    let decoder =
      reader.into_decoder().map_err(|_| McpError::invalid_params("invalid image header or exceeded dimension/allocation limit", None))?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_SOURCE_PIXELS {
      return Err(McpError::invalid_params("source image exceeds dimension or 16 megapixel limit", None));
    }
    // NOTICE(mcp-image-allocation-budgets): this bounds the external pixel buffer
    // separately from the decoder's internal budget. Do not replace its limits
    // after header parsing. A combined cap needs codec allocation accounting;
    // see docs/ai/references/invoke-cli/2026-10-27-mcp-image-artifacts.md.
    limits
      .reserve(decoder.total_bytes())
      .map_err(|_| McpError::invalid_params("source image exceeds 128 MiB pixel allocation limit", None))?;
    let mut image = DynamicImage::from_decoder(decoder)
      .map_err(|_| McpError::invalid_params("image decoding failed or exceeded allocation limit", None))?;
    if width > max_width || height > max_height {
      // NOTICE: image 0.25.10 thumbnail sums u16 channels in u32 and can overflow.
      // Triangle uses floating-point samples. Reconsider thumbnail only after
      // image/src/imageops/sample.rs supports large 16-bit sampling regions.
      // Assignment releases the original pixels after resizing, before encoding.
      image = image.resize(max_width, max_height, FilterType::Triangle);
    }
    let dimensions = (image.width(), image.height());
    let mut output = Cursor::new(Vec::new());
    image.write_to(&mut output, ImageFormat::Png).map_err(|_| McpError::internal_error("image encoding failed", None))?;
    // Encoding is complete; release decoded pixels before Base64 allocation.
    drop(image);
    if output.get_ref().len() > MAX_OUTPUT_BYTES {
      return Err(McpError::invalid_params("encoded image exceeds 8 MiB; request a smaller image", None));
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
