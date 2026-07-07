//! Optional live capture → artifact bundle (non-merge-gate).
//!
//! NOTICE(scan-s1-slice-2): uses the same `write_frame_with_image` path as fixture
//! producer; OS window orchestration is deferred to a later slice.

use std::path::Path;

use super::{FrameCaptureMeta, ProducedFrame, ScanProducerError, frame_from_capture, write_frame_with_image};
use auv_driver::Capture;

/// Build a frame from an in-memory [`Capture`] and write PNG + JSON to `out_dir`.
pub fn produce_frame_from_capture(out_dir: &Path, capture: &Capture, meta: FrameCaptureMeta) -> Result<ProducedFrame, ScanProducerError> {
  let frame = frame_from_capture(capture, meta)?;
  let png = image::ImageBuffer::from_raw(frame.image.width, frame.image.height, capture.image.clone().into_raw())
    .ok_or(ScanProducerError::ZeroImageDimension)?;
  let mut image_bytes = Vec::new();
  let mut cursor = std::io::Cursor::new(&mut image_bytes);
  image::DynamicImage::ImageRgba8(png)
    .write_to(&mut cursor, image::ImageFormat::Png)
    .map_err(|err| ScanProducerError::Io(std::io::Error::other(err)))?;
  write_frame_with_image(out_dir, &frame, &image_bytes)
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use auv_driver::Capture;
  use image::RgbaImage;

  use super::*;
  use crate::artifact::read_frame_artifact;
  use crate::frame::ScanBounds;

  #[test]
  #[ignore = "live"]
  fn produce_frame_from_capture_writes_artifact() {
    let image = RgbaImage::new(4, 4);
    let capture = Capture {
      image,
      bounds: auv_driver::geometry::Rect::new(0.0, 0.0, 4.0, 4.0),
      scale_factor: 1.0,
      backend: "test".to_string(),
      fallback_reason: None,
    };
    let meta = FrameCaptureMeta {
      frame_id: "live-frame-0001".to_string(),
      sequence_index: 0,
      captured_at_millis: 1,
      window_bounds: ScanBounds {
        x: 0,
        y: 0,
        width: 4,
        height: 4,
      },
      viewport_bounds: None,
      image_file_name: "frame-0001.png".to_string(),
      media_type: "image/png".to_string(),
    };
    let out_dir = PathBuf::from(std::env::temp_dir()).join(format!("auv-scan-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out_dir);
    let produced = produce_frame_from_capture(&out_dir, &capture, meta).expect("produce");
    read_frame_artifact(&produced.json_path).expect("read");
    let _ = std::fs::remove_dir_all(&out_dir);
  }
}
