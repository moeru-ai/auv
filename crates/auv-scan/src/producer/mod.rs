//! Frame producer — fixture-first hermetic path + optional live capture mapping.
//!
//! NOTICE(scan-s1-slice-2): fixture and live **must** share `write_frame_with_image`
//! → `write_frame_artifact`. Fail-closed: no degraded/partial artifacts on disk.

mod error;

#[cfg(feature = "live-capture")]
pub mod live;

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::artifact::write_frame_artifact;
use crate::frame::{SCAN_FRAME_SCHEMA_VERSION, ScanBounds, ScanFrame, ScanImageRef};

pub use error::ScanProducerError;

/// Metadata supplied by a capture site when building a [`ScanFrame`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameCaptureMeta {
  pub frame_id: String,
  pub sequence_index: u32,
  pub captured_at_millis: u64,
  pub window_bounds: ScanBounds,
  pub viewport_bounds: Option<ScanBounds>,
  pub image_file_name: String,
  pub media_type: String,
}

/// Result of a successful produce/write bundle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProducedFrame {
  pub json_path: PathBuf,
  pub image_path: PathBuf,
  pub frame: ScanFrame,
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
  image: ScanImageRef,
}

#[derive(Debug, Deserialize)]
struct MultiFrameFixtureEntry {
  frame_id: String,
  sequence_index: u32,
  captured_at_millis: u64,
  window_bounds: ScanBounds,
  viewport_bounds: Option<ScanBounds>,
  image: ScanImageRef,
}

#[derive(Debug, Deserialize)]
struct MultiFrameFixtureManifest {
  scenario: String,
  frames: Vec<MultiFrameFixtureEntry>,
}

/// Result of a successful multi-frame produce/write batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProducedFrameBatch {
  pub produced: Vec<ProducedFrame>,
}

/// Build a validated [`ScanFrame`] from capture metadata and image dimensions.
pub fn build_scan_frame(
  meta: FrameCaptureMeta,
  image_width: u32,
  image_height: u32,
) -> Result<ScanFrame, ScanProducerError> {
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
    image: ScanImageRef {
      file_name: meta.image_file_name,
      width: image_width,
      height: image_height,
      media_type: meta.media_type,
    },
  };
  frame.validate_wire()?;
  Ok(frame)
}

/// Round a driver [`auv_driver::geometry::Rect`] into pixel [`ScanBounds`].
pub fn bounds_to_scan_bounds(rect: &auv_driver::geometry::Rect) -> ScanBounds {
  bounds_to_scan_bounds_f64(
    rect.origin.x,
    rect.origin.y,
    rect.size.width,
    rect.size.height,
  )
}

/// Table-testable f64 → i64 bounds rounding (shared by live mapping).
pub fn bounds_to_scan_bounds_f64(x: f64, y: f64, width: f64, height: f64) -> ScanBounds {
  ScanBounds {
    x: x.round() as i64,
    y: y.round() as i64,
    width: width.round() as i64,
    height: height.round() as i64,
  }
}

/// Map a driver [`Capture`] into a [`ScanFrame`] (memory-only; no disk IO).
pub fn frame_from_capture(
  capture: &auv_driver::Capture,
  meta: FrameCaptureMeta,
) -> Result<ScanFrame, ScanProducerError> {
  build_scan_frame(meta, capture.image.width(), capture.image.height())
}

/// Write PNG bytes then JSON artifact. On JSON failure, removes the PNG (fail-closed).
pub fn write_frame_with_image(
  dir: &Path,
  frame: &ScanFrame,
  image_bytes: &[u8],
) -> Result<ProducedFrame, ScanProducerError> {
  frame.validate_wire()?;
  fs::create_dir_all(dir)?;
  let image_path = dir.join(&frame.image.file_name);
  fs::write(&image_path, image_bytes)?;
  match write_frame_artifact(dir, frame) {
    Ok(json_path) => Ok(ProducedFrame {
      json_path,
      image_path,
      frame: frame.clone(),
    }),
    Err(err) => {
      let _ = fs::remove_file(&image_path);
      Err(err.into())
    }
  }
}

fn fixture_meta_from_entry(entry: MultiFrameFixtureEntry) -> FrameCaptureMeta {
  FrameCaptureMeta {
    frame_id: entry.frame_id,
    sequence_index: entry.sequence_index,
    captured_at_millis: entry.captured_at_millis,
    window_bounds: entry.window_bounds,
    viewport_bounds: entry.viewport_bounds,
    image_file_name: entry.image.file_name,
    media_type: entry.image.media_type,
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
    image_file_name: manifest.image.file_name,
    media_type: manifest.image.media_type,
  }
}

fn png_dimensions(image_bytes: &[u8]) -> Result<(u32, u32), ScanProducerError> {
  let reader = image::ImageReader::new(std::io::Cursor::new(image_bytes))
    .with_guessed_format()
    .map_err(|err| ScanProducerError::Io(std::io::Error::other(err)))?;
  reader
    .into_dimensions()
    .map_err(|err| ScanProducerError::Io(std::io::Error::other(err)))
}

fn load_fixture_frame(fixture_dir: &Path) -> Result<(ScanFrame, Vec<u8>), ScanProducerError> {
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
  let frame = build_scan_frame(
    fixture_meta_from_manifest(manifest),
    image_width,
    image_height,
  )?;
  Ok((frame, image_bytes))
}

/// Hermetic producer: read fixture manifest + PNG, write artifact bundle to `out_dir`.
pub fn produce_frame_from_fixture_dir(
  fixture_dir: &Path,
  out_dir: &Path,
) -> Result<ProducedFrame, ScanProducerError> {
  let (frame, image_bytes) = load_fixture_frame(fixture_dir)?;
  write_frame_with_image(out_dir, &frame, &image_bytes)
}

fn load_multi_frame_fixture(
  fixture_dir: &Path,
) -> Result<Vec<(ScanFrame, Vec<u8>)>, ScanProducerError> {
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

fn rollback_produced_frames(produced: &[ProducedFrame]) {
  for frame in produced.iter().rev() {
    let _ = fs::remove_file(&frame.json_path);
    let _ = fs::remove_file(&frame.image_path);
  }
}

/// Hermetic multi-frame producer: each frame uses `write_frame_with_image` (single write path).
pub fn produce_frames_from_fixture_dir(
  fixture_dir: &Path,
  out_dir: &Path,
) -> Result<ProducedFrameBatch, ScanProducerError> {
  let frames = load_multi_frame_fixture(fixture_dir)?;
  let mut produced = Vec::with_capacity(frames.len());
  for (frame, image_bytes) in frames {
    match write_frame_with_image(out_dir, &frame, &image_bytes) {
      Ok(produced_frame) => produced.push(produced_frame),
      Err(err) => {
        rollback_produced_frames(&produced);
        return Err(err);
      }
    }
  }
  Ok(ProducedFrameBatch { produced })
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use auv_driver::Capture;
  use image::RgbaImage;

  use super::*;
  use crate::artifact::{frame_artifact_file_name, read_frame_artifact};

  fn single_frame_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scan/temporal/single_frame_v0")
  }

  fn two_frame_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scan/temporal/two_frame_v0")
  }

  fn sample_meta() -> FrameCaptureMeta {
    FrameCaptureMeta {
      frame_id: "frame-0001".to_string(),
      sequence_index: 0,
      captured_at_millis: 1_700_000_000_000,
      window_bounds: ScanBounds {
        x: 0,
        y: 0,
        width: 800,
        height: 600,
      },
      viewport_bounds: None,
      image_file_name: "frame-0001.png".to_string(),
      media_type: "image/png".to_string(),
    }
  }

  #[test]
  fn produce_frame_from_fixture_dir_matches_golden() {
    let fixture_dir = single_frame_fixture_dir();
    let out_dir =
      std::env::temp_dir().join(format!("auv-scan-produce-golden-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out_dir);
    let produced = produce_frame_from_fixture_dir(&fixture_dir, &out_dir).expect("produce");
    let golden_path = fixture_dir.join("golden").join(frame_artifact_file_name(0));
    let golden = read_frame_artifact(&golden_path).expect("golden");
    let read_back = read_frame_artifact(&produced.json_path).expect("read");
    assert_eq!(read_back, golden);
    assert_eq!(produced.frame, golden);
    let _ = fs::remove_dir_all(&out_dir);
  }

  #[test]
  fn produce_frame_from_fixture_dir_writes_png_sibling() {
    let fixture_dir = single_frame_fixture_dir();
    let out_dir = std::env::temp_dir().join(format!("auv-scan-produce-png-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out_dir);
    let produced = produce_frame_from_fixture_dir(&fixture_dir, &out_dir).expect("produce");
    assert!(produced.image_path.is_file());
    assert_eq!(produced.image_path.file_name().unwrap(), "frame-0001.png");
    assert_eq!(produced.frame.image.file_name, "frame-0001.png");
    let _ = fs::remove_dir_all(&out_dir);
  }

  #[test]
  fn write_frame_with_image_roundtrip() {
    let fixture_dir = single_frame_fixture_dir();
    let image_bytes = fs::read(fixture_dir.join("frame-0001.png")).unwrap();
    let frame = build_scan_frame(sample_meta(), 8, 8).expect("build");
    let out_dir =
      std::env::temp_dir().join(format!("auv-scan-write-roundtrip-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out_dir);
    let produced = write_frame_with_image(&out_dir, &frame, &image_bytes).expect("write");
    let read_back = read_frame_artifact(&produced.json_path).expect("read");
    assert_eq!(read_back, frame);
    assert!(produced.image_path.is_file());
    let _ = fs::remove_dir_all(&out_dir);
  }

  #[test]
  fn bounds_to_scan_bounds_rounding_table() {
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
      bounds_to_scan_bounds_f64(-1.6, 2.0, 3.0, 4.0),
      ScanBounds {
        x: -2,
        y: 2,
        width: 3,
        height: 4,
      }
    );
  }

  #[test]
  fn produce_frame_from_fixture_dir_rejects_missing_png() {
    let fixture_dir =
      std::env::temp_dir().join(format!("auv-scan-fixture-no-png-{}", std::process::id()));
    let out_dir =
      std::env::temp_dir().join(format!("auv-scan-produce-missing-{}", std::process::id()));
    let _ = fs::remove_dir_all(&fixture_dir);
    let _ = fs::remove_dir_all(&out_dir);
    fs::create_dir_all(&fixture_dir).unwrap();
    fs::create_dir_all(&out_dir).unwrap();
    fs::write(
      fixture_dir.join(MANIFEST_FILE),
      serde_json::to_string_pretty(&serde_json::json!({
        "scenario": "missing_png",
        "frame_id": "frame-0001",
        "sequence_index": 0,
        "captured_at_millis": 1_700_000_000_000u64,
        "window_bounds": { "x": 0, "y": 0, "width": 8, "height": 8 },
        "viewport_bounds": null,
        "image": {
          "file_name": "frame-0001.png",
          "width": 8,
          "height": 8,
          "media_type": "image/png"
        }
      }))
      .unwrap(),
    )
    .unwrap();
    let err = produce_frame_from_fixture_dir(&fixture_dir, &out_dir).expect_err("missing png");
    assert!(matches!(
      err,
      ScanProducerError::MissingImage { path } if path.ends_with("frame-0001.png")
    ));
    assert!(!out_dir.join("frame-0001.png").exists());
    assert!(!out_dir.join(frame_artifact_file_name(0)).exists());
    let _ = fs::remove_dir_all(&fixture_dir);
    let _ = fs::remove_dir_all(&out_dir);
  }

  #[test]
  fn produce_failure_leaves_no_partial_artifact() {
    let frame = build_scan_frame(sample_meta(), 8, 8).expect("build");
    let image_bytes = vec![0u8; 8];
    let out_dir =
      std::env::temp_dir().join(format!("auv-scan-produce-partial-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out_dir);
    fs::create_dir_all(&out_dir).unwrap();
    let json_path = out_dir.join(frame_artifact_file_name(0));
    fs::create_dir(&json_path).unwrap();
    let err = write_frame_with_image(&out_dir, &frame, &image_bytes).expect_err("json blocked");
    assert!(matches!(err, ScanProducerError::Artifact(_)));
    assert!(!out_dir.join("frame-0001.png").exists());
    let _ = fs::remove_dir_all(&out_dir);
  }

  #[test]
  fn build_scan_frame_rejects_zero_dimension() {
    let err = build_scan_frame(sample_meta(), 0, 8).expect_err("zero width");
    assert!(matches!(err, ScanProducerError::ZeroImageDimension));
    let err = build_scan_frame(sample_meta(), 8, 0).expect_err("zero height");
    assert!(matches!(err, ScanProducerError::ZeroImageDimension));
  }

  #[test]
  fn frame_from_capture_builds_scan_frame_from_rgba() {
    let image = RgbaImage::new(8, 8);
    let capture = Capture {
      image,
      bounds: auv_driver::geometry::Rect::new(0.0, 0.0, 8.0, 8.0),
      scale_factor: 1.0,
      backend: "test".to_string(),
      fallback_reason: None,
    };
    let frame = frame_from_capture(&capture, sample_meta()).expect("frame");
    assert_eq!(frame.schema_version, SCAN_FRAME_SCHEMA_VERSION);
    assert_eq!(frame.image.width, 8);
    assert_eq!(frame.image.height, 8);
  }

  #[test]
  fn frame_from_capture_rejects_zero_dimension() {
    let image = RgbaImage::new(0, 8);
    let capture = Capture {
      image,
      bounds: auv_driver::geometry::Rect::new(0.0, 0.0, 0.0, 8.0),
      scale_factor: 1.0,
      backend: "test".to_string(),
      fallback_reason: None,
    };
    let err = frame_from_capture(&capture, sample_meta()).expect_err("zero");
    assert!(matches!(err, ScanProducerError::ZeroImageDimension));
  }

  #[test]
  fn produce_two_frame_fixture_writes_monotonic_indices() {
    let fixture_dir = two_frame_fixture_dir();
    let out_dir =
      std::env::temp_dir().join(format!("auv-scan-produce-two-frame-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out_dir);
    let batch = produce_frames_from_fixture_dir(&fixture_dir, &out_dir).expect("produce");
    assert_eq!(batch.produced.len(), 2);
    assert!(out_dir.join(frame_artifact_file_name(0)).is_file());
    assert!(out_dir.join(frame_artifact_file_name(1)).is_file());
    assert_eq!(batch.produced[0].frame.sequence_index, 0);
    assert_eq!(batch.produced[1].frame.sequence_index, 1);
    let _ = fs::remove_dir_all(&out_dir);
  }

  #[test]
  fn produce_two_frame_fixture_matches_golden() {
    let fixture_dir = two_frame_fixture_dir();
    let out_dir = std::env::temp_dir().join(format!(
      "auv-scan-produce-two-golden-{}",
      std::process::id()
    ));
    let _ = fs::remove_dir_all(&out_dir);
    let batch = produce_frames_from_fixture_dir(&fixture_dir, &out_dir).expect("produce");
    for (index, produced) in batch.produced.iter().enumerate() {
      let golden_path = fixture_dir
        .join("golden")
        .join(frame_artifact_file_name(index as u32));
      let golden = read_frame_artifact(&golden_path).expect("golden");
      let read_back = read_frame_artifact(&produced.json_path).expect("read");
      assert_eq!(read_back, golden);
    }
    let _ = fs::remove_dir_all(&out_dir);
  }

  #[test]
  fn two_frame_ids_are_unique() {
    let fixture_dir = two_frame_fixture_dir();
    let out_dir =
      std::env::temp_dir().join(format!("auv-scan-two-frame-ids-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out_dir);
    let batch = produce_frames_from_fixture_dir(&fixture_dir, &out_dir).expect("produce");
    let ids: Vec<_> = batch
      .produced
      .iter()
      .map(|p| p.frame.frame_id.as_str())
      .collect();
    assert_eq!(ids, vec!["frame-0001", "frame-0002"]);
    let _ = fs::remove_dir_all(&out_dir);
  }

  #[test]
  fn produce_two_frame_fixture_rejects_duplicate_sequence_index() {
    let fixture_dir = two_frame_fixture_dir();
    let temp_fixture = std::env::temp_dir().join(format!(
      "auv-scan-two-frame-dup-seq-fixture-{}",
      std::process::id()
    ));
    let out_dir = std::env::temp_dir().join(format!(
      "auv-scan-two-frame-dup-seq-out-{}",
      std::process::id()
    ));
    let _ = fs::remove_dir_all(&temp_fixture);
    let _ = fs::remove_dir_all(&out_dir);
    fs::create_dir_all(&temp_fixture).expect("temp fixture");
    fs::copy(
      fixture_dir.join("frame-0001.png"),
      temp_fixture.join("frame-0001.png"),
    )
    .expect("copy frame 1");
    fs::copy(
      fixture_dir.join("frame-0002.png"),
      temp_fixture.join("frame-0002.png"),
    )
    .expect("copy frame 2");
    let mut manifest: serde_json::Value =
      serde_json::from_slice(&fs::read(fixture_dir.join(MANIFEST_FILE)).expect("read manifest"))
        .expect("parse manifest");
    manifest["frames"][1]["sequence_index"] = serde_json::Value::from(0);
    fs::write(
      temp_fixture.join(MANIFEST_FILE),
      serde_json::to_vec_pretty(&manifest).expect("serialize manifest"),
    )
    .expect("write manifest");

    let err = produce_frames_from_fixture_dir(&temp_fixture, &out_dir).expect_err("duplicate seq");
    assert!(matches!(
      err,
      ScanProducerError::DuplicateSequenceIndex { sequence_index: 0 }
    ));
    let _ = fs::remove_dir_all(&temp_fixture);
    let _ = fs::remove_dir_all(&out_dir);
  }

  #[test]
  fn produce_two_frame_fixture_rolls_back_on_late_write_failure() {
    let fixture_dir = two_frame_fixture_dir();
    let out_dir = std::env::temp_dir().join(format!(
      "auv-scan-two-frame-rollback-{}",
      std::process::id()
    ));
    let _ = fs::remove_dir_all(&out_dir);
    fs::create_dir_all(out_dir.join(frame_artifact_file_name(1))).expect("poison path");

    let err = produce_frames_from_fixture_dir(&fixture_dir, &out_dir).expect_err("late failure");
    assert!(matches!(
      err,
      ScanProducerError::Artifact(crate::artifact::ScanArtifactError::Io(_))
    ));
    assert!(!out_dir.join(frame_artifact_file_name(0)).exists());
    assert!(!out_dir.join("frame-0001.png").exists());
    assert!(!out_dir.join("frame-0002.png").exists());
    let _ = fs::remove_dir_all(&out_dir);
  }
}
