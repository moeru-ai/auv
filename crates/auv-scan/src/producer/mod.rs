//! Frame construction and fixture decoding.

mod coverage;
mod error;

use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::frame::{SCAN_FRAME_SCHEMA_VERSION, ScanBounds, ScanFrame, ScanImageDimensions};

pub use coverage::{CoverageProducerError, build_coverage_fixture};
pub use error::ScanProducerError;

/// Metadata supplied by a capture site when building a [`ScanFrame`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameCaptureMeta {
  pub frame_id: String,
  pub sequence_index: u32,
  pub captured_at_millis: u64,
  pub window_bounds: ScanBounds,
  pub viewport_bounds: Option<ScanBounds>,
}

/// One decoded fixture frame. Fixture paths remain input-only and never become
/// artifact locators.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedFrameFixture {
  frame: ScanFrame,
  image_bytes: Vec<u8>,
}

impl LoadedFrameFixture {
  pub fn frame(&self) -> &ScanFrame {
    &self.frame
  }

  pub fn into_parts(self) -> (ScanFrame, Vec<u8>) {
    (self.frame, self.image_bytes)
  }
}

const MANIFEST_FILE: &str = "manifest.json";

#[derive(Debug, Deserialize)]
struct FixtureManifest {
  scenario: String,
  frame_id: String,
  sequence_index: u32,
  captured_at_millis: u64,
  window_bounds: ScanBounds,
  viewport_bounds: Option<ScanBounds>,
  image: FixtureImage,
}

#[derive(Debug, Deserialize)]
struct MultiFrameFixtureEntry {
  frame_id: String,
  sequence_index: u32,
  captured_at_millis: u64,
  window_bounds: ScanBounds,
  viewport_bounds: Option<ScanBounds>,
  image: FixtureImage,
}

#[derive(Debug, Deserialize)]
struct FixtureImage {
  file_name: String,
}

#[derive(Debug, Deserialize)]
struct MultiFrameFixtureManifest {
  scenario: String,
  frames: Vec<MultiFrameFixtureEntry>,
}

/// Build a validated [`ScanFrame`] from capture metadata and image dimensions.
pub fn build_scan_frame(meta: FrameCaptureMeta, image_width: u32, image_height: u32) -> Result<ScanFrame, ScanProducerError> {
  if image_width == 0 || image_height == 0 {
    return Err(ScanProducerError::ZeroImageDimension);
  }
  let frame = ScanFrame {
    schema_version: SCAN_FRAME_SCHEMA_VERSION.to_string(),
    frame_id: meta.frame_id,
    sequence_index: meta.sequence_index,
    captured_at_millis: meta.captured_at_millis,
    window_bounds: meta.window_bounds,
    viewport_bounds: meta.viewport_bounds,
    image_dimensions: ScanImageDimensions {
      width: image_width,
      height: image_height,
    },
  };
  frame.validate_wire()?;
  Ok(frame)
}

/// Round a driver [`auv_driver::geometry::Rect`] into pixel [`ScanBounds`].
pub fn bounds_to_scan_bounds(rect: &auv_driver::geometry::Rect) -> ScanBounds {
  bounds_to_scan_bounds_f64(rect.origin.x, rect.origin.y, rect.size.width, rect.size.height)
}

/// Round raw floating-point bounds shared by live capture mapping.
fn bounds_to_scan_bounds_f64(x: f64, y: f64, width: f64, height: f64) -> ScanBounds {
  ScanBounds {
    x: x.round() as i64,
    y: y.round() as i64,
    width: width.round() as i64,
    height: height.round() as i64,
  }
}

/// Map a driver [`Capture`] into a [`ScanFrame`] (memory-only; no disk IO).
pub fn frame_from_capture(capture: &auv_driver::Capture, meta: FrameCaptureMeta) -> Result<ScanFrame, ScanProducerError> {
  build_scan_frame(meta, capture.image.width(), capture.image.height())
}

fn fixture_meta_from_entry(entry: MultiFrameFixtureEntry) -> FrameCaptureMeta {
  FrameCaptureMeta {
    frame_id: entry.frame_id,
    sequence_index: entry.sequence_index,
    captured_at_millis: entry.captured_at_millis,
    window_bounds: entry.window_bounds,
    viewport_bounds: entry.viewport_bounds,
  }
}

fn fixture_meta_from_manifest(manifest: FixtureManifest) -> FrameCaptureMeta {
  let _scenario = manifest.scenario;
  FrameCaptureMeta {
    frame_id: manifest.frame_id,
    sequence_index: manifest.sequence_index,
    captured_at_millis: manifest.captured_at_millis,
    window_bounds: manifest.window_bounds,
    viewport_bounds: manifest.viewport_bounds,
  }
}

fn png_dimensions(image_bytes: &[u8]) -> Result<(u32, u32), ScanProducerError> {
  let reader = image::ImageReader::new(std::io::Cursor::new(image_bytes))
    .with_guessed_format()
    .map_err(|err| ScanProducerError::Io(std::io::Error::other(err)))?;
  reader.into_dimensions().map_err(|err| ScanProducerError::Io(std::io::Error::other(err)))
}

/// Decode one hermetic fixture into a typed frame and its original image bytes.
pub fn load_frame_fixture(fixture_dir: &Path) -> Result<LoadedFrameFixture, ScanProducerError> {
  let manifest_path = fixture_dir.join(MANIFEST_FILE);
  let manifest_bytes = fs::read(&manifest_path)?;
  let manifest: FixtureManifest = serde_json::from_slice(&manifest_bytes)?;
  let image_path = fixture_dir.join(&manifest.image.file_name);
  if !image_path.is_file() {
    return Err(ScanProducerError::MissingImage {
      path: image_path.display().to_string(),
    });
  }
  let image_bytes = fs::read(&image_path)?;
  let (image_width, image_height) = png_dimensions(&image_bytes)?;
  let frame = build_scan_frame(fixture_meta_from_manifest(manifest), image_width, image_height)?;
  Ok(LoadedFrameFixture { frame, image_bytes })
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use super::*;

  fn sample_meta() -> FrameCaptureMeta {
    FrameCaptureMeta {
      frame_id: "frame-0001".into(),
      sequence_index: 0,
      captured_at_millis: 1_700_000_000_000,
      window_bounds: ScanBounds {
        x: 0,
        y: 0,
        width: 800,
        height: 600,
      },
      viewport_bounds: None,
    }
  }

  fn single_frame_input() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("testdata").join("scan").join("temporal").join("single_frame_v0")
  }

  #[test]
  fn rounds_capture_bounds_at_half_away_from_zero() {
    assert_eq!(
      bounds_to_scan_bounds_f64(0.4, 0.5, 10.4, 20.5),
      ScanBounds {
        x: 0,
        y: 1,
        width: 10,
        height: 21,
      }
    );
    assert_eq!(
      bounds_to_scan_bounds_f64(-1.6, -1.5, 3.0, 4.0),
      ScanBounds {
        x: -2,
        y: -2,
        width: 3,
        height: 4,
      }
    );
  }

  #[test]
  fn rejects_zero_image_dimensions() {
    assert!(matches!(build_scan_frame(sample_meta(), 0, 8), Err(ScanProducerError::ZeroImageDimension)));
    assert!(matches!(build_scan_frame(sample_meta(), 8, 0), Err(ScanProducerError::ZeroImageDimension)));
  }

  #[test]
  fn decodes_png_dimensions_from_fixture_input() {
    let loaded = load_frame_fixture(&single_frame_input()).expect("valid frame input");

    assert_eq!(
      loaded.frame().image_dimensions,
      ScanImageDimensions {
        width: 8,
        height: 8
      }
    );
    assert!(!loaded.clone().into_parts().1.is_empty());
  }
}

fn load_multi_frame_fixture(fixture_dir: &Path) -> Result<Vec<(ScanFrame, Vec<u8>)>, ScanProducerError> {
  let manifest_path = fixture_dir.join(MANIFEST_FILE);
  let manifest_bytes = fs::read(&manifest_path)?;
  let manifest: MultiFrameFixtureManifest = serde_json::from_slice(&manifest_bytes)?;
  let _scenario = manifest.scenario;
  if manifest.frames.is_empty() {
    return Err(ScanProducerError::NoFramesInFixture);
  }
  let mut frames = Vec::with_capacity(manifest.frames.len());
  let mut seen_frame_ids = std::collections::HashSet::new();
  let mut seen_sequence_indices = std::collections::HashSet::new();
  for entry in manifest.frames {
    if !seen_frame_ids.insert(entry.frame_id.clone()) {
      return Err(ScanProducerError::DuplicateFrameId {
        frame_id: entry.frame_id,
      });
    }
    if !seen_sequence_indices.insert(entry.sequence_index) {
      return Err(ScanProducerError::DuplicateSequenceIndex {
        sequence_index: entry.sequence_index,
      });
    }
    let image_path = fixture_dir.join(&entry.image.file_name);
    if !image_path.is_file() {
      return Err(ScanProducerError::MissingImage {
        path: image_path.display().to_string(),
      });
    }
    let image_bytes = fs::read(&image_path)?;
    let (image_width, image_height) = png_dimensions(&image_bytes)?;
    let frame = build_scan_frame(fixture_meta_from_entry(entry), image_width, image_height)?;
    frames.push((frame, image_bytes));
  }
  frames.sort_by_key(|(frame, _)| frame.sequence_index);
  Ok(frames)
}
