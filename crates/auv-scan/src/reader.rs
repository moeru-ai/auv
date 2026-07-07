//! Crate-local read-side: load frame artifacts, verify PNG dimensions, summarize metadata.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::artifact::{ScanArtifactError, read_frame_artifact};
use crate::frame::ScanFrame;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanFrameBundle {
  pub frames: Vec<ScanFrame>,
  pub source_dir: PathBuf,
  pub loaded_json_paths: Vec<PathBuf>,
}

#[derive(Debug, Error)]
pub enum ScanInspectError {
  #[error(transparent)]
  Artifact(#[from] ScanArtifactError),
  #[error("no scan-frame artifacts found in directory")]
  NoFramesFound,
  #[error("image file missing: {path}")]
  ImageFileMissing { path: String },
  #[error("image dimension mismatch: expected {expected_w}x{expected_h}, found {actual_w}x{actual_h}")]
  ImageDimensionMismatch {
    expected_w: u32,
    expected_h: u32,
    actual_w: u32,
    actual_h: u32,
  },
  #[error("duplicate sequence_index {index} in {first_file} and {second_file}")]
  DuplicateSequenceIndex {
    index: u32,
    first_file: String,
    second_file: String,
  },
  #[error("non-monotonic sequence_index: previous {previous}, found {found}")]
  NonMonotonicSequenceIndex { previous: u32, found: u32 },
  #[error(transparent)]
  Io(#[from] std::io::Error),
}

fn is_scan_frame_artifact_name(file_name: &str) -> bool {
  let Some(stem) = file_name.strip_prefix("scan-frame-") else {
    return false;
  };
  let Some(digits) = stem.strip_suffix(".json") else {
    return false;
  };
  !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

/// Load all `scan-frame-*.json` artifacts from `dir` (top level only).
pub fn load_scan_frames_from_dir(dir: &Path) -> Result<ScanFrameBundle, ScanInspectError> {
  let mut entries: Vec<(PathBuf, ScanFrame)> = Vec::new();
  for entry in fs::read_dir(dir)? {
    let entry = entry?;
    let path = entry.path();
    if !path.is_file() {
      continue;
    }
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
      continue;
    };
    if !is_scan_frame_artifact_name(file_name) {
      continue;
    }
    let frame = read_frame_artifact(&path)?;
    entries.push((path, frame));
  }

  if entries.is_empty() {
    return Err(ScanInspectError::NoFramesFound);
  }

  let mut first_by_index: HashMap<u32, String> = HashMap::new();
  for (path, frame) in &entries {
    let file_name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    if let Some(first_file) = first_by_index.get(&frame.sequence_index) {
      return Err(ScanInspectError::DuplicateSequenceIndex {
        index: frame.sequence_index,
        first_file: first_file.clone(),
        second_file: file_name,
      });
    }
    first_by_index.insert(frame.sequence_index, file_name);
  }

  entries.sort_by(|(path_a, frame_a), (path_b, frame_b)| {
    frame_a.sequence_index.cmp(&frame_b.sequence_index).then_with(|| path_a.file_name().cmp(&path_b.file_name()))
  });

  for window in entries.windows(2) {
    let previous = window[0].1.sequence_index;
    let found = window[1].1.sequence_index;
    if found <= previous {
      return Err(ScanInspectError::NonMonotonicSequenceIndex { previous, found });
    }
  }

  let loaded_json_paths = entries.iter().map(|(path, _)| path.clone()).collect();
  let frames = entries.into_iter().map(|(_, frame)| frame).collect();

  Ok(ScanFrameBundle {
    frames,
    source_dir: dir.to_path_buf(),
    loaded_json_paths,
  })
}

fn png_dimensions(image_bytes: &[u8]) -> Result<(u32, u32), ScanInspectError> {
  let reader = image::ImageReader::new(std::io::Cursor::new(image_bytes))
    .with_guessed_format()
    .map_err(|err| ScanInspectError::Io(std::io::Error::other(err)))?;
  reader.into_dimensions().map_err(|err| ScanInspectError::Io(std::io::Error::other(err)))
}

/// Read PNG dimensions from disk and compare to wire `image.width` / `image.height`.
pub fn verify_frame_image_dimensions(source_dir: &Path, frame: &ScanFrame) -> Result<(), ScanInspectError> {
  let image_path = source_dir.join(&frame.image.file_name);
  if !image_path.is_file() {
    return Err(ScanInspectError::ImageFileMissing {
      path: image_path.display().to_string(),
    });
  }
  let image_bytes = fs::read(&image_path)?;
  let (actual_w, actual_h) = png_dimensions(&image_bytes)?;
  if actual_w != frame.image.width || actual_h != frame.image.height {
    return Err(ScanInspectError::ImageDimensionMismatch {
      expected_w: frame.image.width,
      expected_h: frame.image.height,
      actual_w,
      actual_h,
    });
  }
  Ok(())
}

/// Metadata-only summary from in-memory [`ScanFrame`] fields (no disk IO).
pub fn summarize_scan_frame_text(frame: &ScanFrame) -> String {
  format!(
    "frame_id={} sequence_index={} captured_at_millis={} image={}x{} file={} window={}x{}",
    frame.frame_id,
    frame.sequence_index,
    frame.captured_at_millis,
    frame.image.width,
    frame.image.height,
    frame.image.file_name,
    frame.window_bounds.width,
    frame.window_bounds.height,
  )
}

/// Replay frames from an artifact directory (read-only; no driver or capture).
pub fn replay_scan_frames_from_dir(dir: &Path) -> Result<ScanFrameBundle, ScanInspectError> {
  load_scan_frames_from_dir(dir)
}

#[cfg(test)]
mod test_support {
  use super::*;

  #[derive(Clone, Debug, PartialEq, Eq)]
  pub struct FrameFieldExpectation {
    pub expected: ScanFrame,
  }

  pub fn assert_frame_matches_expectation(frame: &ScanFrame, expectation: &FrameFieldExpectation) {
    assert_eq!(frame, &expectation.expected);
  }
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use super::test_support::{FrameFieldExpectation, assert_frame_matches_expectation};
  use super::*;
  use crate::artifact::{frame_artifact_file_name, read_frame_artifact, write_frame_artifact};
  use crate::fixture::build_frame_from_fixture;
  use crate::frame::{SCAN_FRAME_SCHEMA_VERSION, ScanBounds, ScanFrame, ScanImageRef};
  use crate::producer::{produce_frame_from_fixture_dir, produce_frames_from_fixture_dir};

  fn single_frame_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scan/temporal/single_frame_v0")
  }

  fn two_frame_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scan/temporal/two_frame_v0")
  }

  fn temp_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("auv-scan-reader-{label}-{}", std::process::id()))
  }

  fn cleanup(dir: &Path) {
    let _ = fs::remove_dir_all(dir);
  }

  fn copy_golden_artifact_dir(out_dir: &Path) {
    let fixture_dir = single_frame_fixture_dir();
    fs::create_dir_all(out_dir).unwrap();
    fs::copy(fixture_dir.join("golden").join(frame_artifact_file_name(0)), out_dir.join(frame_artifact_file_name(0))).unwrap();
    fs::copy(fixture_dir.join("frame-0001.png"), out_dir.join("frame-0001.png")).unwrap();
  }

  #[test]
  fn load_scan_frames_from_dir_reads_golden_directory() {
    let dir = temp_dir("golden-load");
    cleanup(&dir);
    copy_golden_artifact_dir(&dir);
    let golden = read_frame_artifact(&dir.join(frame_artifact_file_name(0))).expect("golden");
    let bundle = load_scan_frames_from_dir(&dir).expect("load");
    assert_eq!(bundle.frames.len(), 1);
    assert_eq!(bundle.loaded_json_paths.len(), 1);
    assert_frame_matches_expectation(&bundle.frames[0], &FrameFieldExpectation { expected: golden });
    cleanup(&dir);
  }

  #[test]
  fn load_scan_frames_from_dir_sorts_by_sequence_index() {
    let dir = temp_dir("sort");
    cleanup(&dir);
    fs::create_dir_all(&dir).unwrap();
    let fixture_dir = single_frame_fixture_dir();
    let png_bytes = fs::read(fixture_dir.join("frame-0001.png")).unwrap();

    let frame0 = ScanFrame {
      schema_version: SCAN_FRAME_SCHEMA_VERSION.to_string(),
      frame_id: "frame-a".to_string(),
      sequence_index: 0,
      captured_at_millis: 1,
      window_bounds: ScanBounds {
        x: 0,
        y: 0,
        width: 8,
        height: 8,
      },
      viewport_bounds: None,
      image: ScanImageRef {
        file_name: "frame-a.png".to_string(),
        width: 8,
        height: 8,
        media_type: "image/png".to_string(),
      },
    };
    let frame1 = ScanFrame {
      sequence_index: 1,
      frame_id: "frame-b".to_string(),
      captured_at_millis: 2,
      image: ScanImageRef {
        file_name: "frame-b.png".to_string(),
        ..frame0.image.clone()
      },
      ..frame0.clone()
    };

    write_frame_artifact(&dir, &frame1).unwrap();
    write_frame_artifact(&dir, &frame0).unwrap();
    fs::write(dir.join("frame-a.png"), &png_bytes).unwrap();
    fs::write(dir.join("frame-b.png"), &png_bytes).unwrap();

    let bundle = load_scan_frames_from_dir(&dir).expect("load");
    assert_eq!(bundle.frames.len(), 2);
    assert_eq!(bundle.frames[0].sequence_index, 0);
    assert_eq!(bundle.frames[1].sequence_index, 1);
    cleanup(&dir);
  }

  #[test]
  fn load_scan_frames_from_dir_rejects_duplicate_sequence_index() {
    let dir = temp_dir("dup-index");
    cleanup(&dir);
    fs::create_dir_all(&dir).unwrap();
    let fixture_dir = single_frame_fixture_dir();
    let base = build_frame_from_fixture(&fixture_dir).expect("fixture");
    write_frame_artifact(&dir, &base).unwrap();
    let mut duplicate = base.clone();
    duplicate.frame_id = "frame-dup".to_string();
    let dup_path = dir.join("scan-frame-0002.json");
    fs::write(dup_path, serde_json::to_string_pretty(&duplicate).unwrap()).unwrap();

    let err = load_scan_frames_from_dir(&dir).expect_err("duplicate");
    assert!(matches!(err, ScanInspectError::DuplicateSequenceIndex { index: 0, .. }));
    cleanup(&dir);
  }

  #[test]
  fn verify_frame_image_dimensions_matches_png() {
    let dir = temp_dir("verify-ok");
    cleanup(&dir);
    copy_golden_artifact_dir(&dir);
    let frame = read_frame_artifact(&dir.join(frame_artifact_file_name(0))).expect("read");
    verify_frame_image_dimensions(&dir, &frame).expect("verify");
    cleanup(&dir);
  }

  #[test]
  fn verify_frame_image_dimensions_rejects_mismatch() {
    let dir = temp_dir("verify-mismatch");
    cleanup(&dir);
    copy_golden_artifact_dir(&dir);
    let mut frame = read_frame_artifact(&dir.join(frame_artifact_file_name(0))).expect("read");
    frame.image.height = 99;
    let err = verify_frame_image_dimensions(&dir, &frame).expect_err("mismatch");
    assert!(matches!(
      err,
      ScanInspectError::ImageDimensionMismatch {
        expected_w: 8,
        expected_h: 99,
        actual_w: 8,
        actual_h: 8,
      }
    ));
    cleanup(&dir);
  }

  #[test]
  fn load_scan_frames_from_dir_empty_dir_errors() {
    let dir = temp_dir("empty");
    cleanup(&dir);
    fs::create_dir_all(&dir).unwrap();
    let err = load_scan_frames_from_dir(&dir).expect_err("empty");
    assert!(matches!(err, ScanInspectError::NoFramesFound));
    cleanup(&dir);
  }

  #[test]
  fn load_scan_frames_from_dir_rejects_bad_schema() {
    let dir = temp_dir("bad-schema");
    cleanup(&dir);
    fs::create_dir_all(&dir).unwrap();
    let fixture_dir = single_frame_fixture_dir();
    let mut frame = build_frame_from_fixture(&fixture_dir).expect("fixture");
    frame.schema_version = "scan-frame-v99".to_string();
    let path = dir.join(frame_artifact_file_name(0));
    fs::write(path, serde_json::to_string_pretty(&frame).unwrap()).unwrap();
    let err = load_scan_frames_from_dir(&dir).expect_err("schema");
    assert!(matches!(err, ScanInspectError::Artifact(ScanArtifactError::SchemaMismatch { .. })));
    cleanup(&dir);
  }

  #[test]
  fn summarize_scan_frame_text_includes_key_fields() {
    let fixture_dir = single_frame_fixture_dir();
    let frame = build_frame_from_fixture(&fixture_dir).expect("fixture");
    let summary = summarize_scan_frame_text(&frame);
    assert!(summary.contains("frame_id=frame-0001"));
    assert!(summary.contains("sequence_index=0"));
    assert!(summary.contains("frame-0001.png"));
    assert!(summary.contains("image=8x8"));
  }

  #[test]
  fn producer_then_reader_roundtrip() {
    let fixture_dir = single_frame_fixture_dir();
    let out_dir = temp_dir("producer-reader");
    cleanup(&out_dir);
    let produced = produce_frame_from_fixture_dir(&fixture_dir, &out_dir).expect("produce");
    let bundle = load_scan_frames_from_dir(&out_dir).expect("load");
    assert_eq!(bundle.frames.len(), 1);
    assert_eq!(bundle.frames[0], produced.frame);
    verify_frame_image_dimensions(&out_dir, &bundle.frames[0]).expect("verify");
    cleanup(&out_dir);
  }

  #[test]
  fn load_scan_frames_from_dir_returns_two_sorted() {
    let fixture_dir = two_frame_fixture_dir();
    let out_dir = temp_dir("two-frame-load");
    cleanup(&out_dir);
    produce_frames_from_fixture_dir(&fixture_dir, &out_dir).expect("produce");
    let bundle = load_scan_frames_from_dir(&out_dir).expect("load");
    assert_eq!(bundle.frames.len(), 2);
    assert_eq!(bundle.frames[0].sequence_index, 0);
    assert_eq!(bundle.frames[1].sequence_index, 1);
    cleanup(&out_dir);
  }

  #[test]
  fn replay_scan_frames_does_not_invoke_capture() {
    let fixture_dir = two_frame_fixture_dir();
    let out_dir = temp_dir("two-frame-replay");
    cleanup(&out_dir);
    produce_frames_from_fixture_dir(&fixture_dir, &out_dir).expect("produce");
    let bundle = replay_scan_frames_from_dir(&out_dir).expect("replay");
    assert_eq!(bundle.frames.len(), 2);
    assert_eq!(bundle.frames[0].frame_id, "frame-0001");
    assert_eq!(bundle.frames[1].frame_id, "frame-0002");
    cleanup(&out_dir);
  }
}
