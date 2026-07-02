//! Hermetic fixture support — **not** part of the stable public API.

use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::artifact::ScanArtifactError;
use crate::frame::{SCAN_FRAME_SCHEMA_VERSION, ScanBounds, ScanFrame, ScanImageRef};

const MANIFEST_FILE: &str = "manifest.json";

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct FixtureManifest {
  scenario: String,
  frame_id: String,
  sequence_index: u32,
  captured_at_millis: u64,
  window_bounds: ScanBounds,
  viewport_bounds: Option<ScanBounds>,
  image: ScanImageRef,
}

pub(crate) fn build_frame_from_fixture(fixture_dir: &Path) -> Result<ScanFrame, ScanArtifactError> {
  let manifest_path = fixture_dir.join(MANIFEST_FILE);
  let manifest_bytes = fs::read(&manifest_path)?;
  let manifest: FixtureManifest = serde_json::from_slice(&manifest_bytes)?;
  let image_path = fixture_dir.join(&manifest.image.file_name);
  if !image_path.is_file() {
    return Err(ScanArtifactError::Io(std::io::Error::new(
      std::io::ErrorKind::NotFound,
      format!("fixture image not found: {}", image_path.display()),
    )));
  }
  let _scenario = manifest.scenario;
  let frame = ScanFrame {
    schema_version: SCAN_FRAME_SCHEMA_VERSION.to_string(),
    frame_id: manifest.frame_id,
    sequence_index: manifest.sequence_index,
    captured_at_millis: manifest.captured_at_millis,
    window_bounds: manifest.window_bounds,
    viewport_bounds: manifest.viewport_bounds,
    image: manifest.image,
  };
  frame.validate_wire()?;
  Ok(frame)
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use super::*;
  use crate::artifact::{frame_artifact_file_name, read_frame_artifact, write_frame_artifact};
  use crate::frame::{SCAN_FRAME_SCHEMA_VERSION, ScanBounds, ScanImageRef};

  fn single_frame_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scan/temporal/single_frame_v0")
  }

  #[test]
  fn build_frame_from_fixture_single_frame_v0() {
    let frame = build_frame_from_fixture(&single_frame_fixture_dir()).expect("build");
    assert_eq!(frame.schema_version, SCAN_FRAME_SCHEMA_VERSION);
    assert_eq!(frame.frame_id, "frame-0001");
    assert_eq!(frame.sequence_index, 0);
    assert_eq!(frame.captured_at_millis, 1_700_000_000_000);
    assert_eq!(
      frame.window_bounds,
      ScanBounds {
        x: 0,
        y: 0,
        width: 800,
        height: 600,
      }
    );
    assert_eq!(frame.viewport_bounds, None);
    assert_eq!(
      frame.image,
      ScanImageRef {
        file_name: "frame-0001.png".to_string(),
        width: 8,
        height: 8,
        media_type: "image/png".to_string(),
      }
    );
  }

  #[test]
  fn read_frame_artifact_matches_golden_wire() {
    let fixture_dir = single_frame_fixture_dir();
    let frame = build_frame_from_fixture(&fixture_dir).expect("build");
    let golden_path = fixture_dir.join("golden").join(frame_artifact_file_name(0));
    let golden = read_frame_artifact(&golden_path).expect("golden read");
    assert_eq!(frame, golden);
    let out_dir = std::env::temp_dir().join(format!("auv-scan-golden-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out_dir);
    let written = write_frame_artifact(&out_dir, &frame).expect("write");
    let read_back = read_frame_artifact(&written).expect("read");
    assert_eq!(read_back, golden);
    let _ = fs::remove_dir_all(&out_dir);
  }
}
