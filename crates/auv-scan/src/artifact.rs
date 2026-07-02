//! Frame artifact read/write for `scan-frame-v0`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::frame::{SCAN_FRAME_SCHEMA_VERSION, ScanFrame};

#[derive(Debug, Error)]
pub enum ScanArtifactError {
  #[error("schema_version mismatch: expected {SCAN_FRAME_SCHEMA_VERSION}, found {found}")]
  SchemaMismatch { found: String },
  #[error("invalid bounds for {field}")]
  InvalidBounds { field: &'static str },
  #[error("missing required field: {0}")]
  MissingField(&'static str),
  #[error(transparent)]
  Io(#[from] std::io::Error),
  #[error("json parse error: {0}")]
  Json(#[from] serde_json::Error),
}

/// Returns full artifact file name including extension.
/// `sequence_index` 0 → `"scan-frame-0001.json"`.
pub fn frame_artifact_file_name(sequence_index: u32) -> String {
  format!("scan-frame-{:04}.json", sequence_index.saturating_add(1))
}

pub fn write_frame_artifact(dir: &Path, frame: &ScanFrame) -> Result<PathBuf, ScanArtifactError> {
  frame.validate_wire()?;
  fs::create_dir_all(dir)?;
  let file_name = frame_artifact_file_name(frame.sequence_index);
  let path = dir.join(&file_name);
  let json = serde_json::to_string_pretty(frame)?;
  let mut file = fs::File::create(&path)?;
  file.write_all(json.as_bytes())?;
  file.write_all(b"\n")?;
  Ok(path)
}

pub fn read_frame_artifact(path: &Path) -> Result<ScanFrame, ScanArtifactError> {
  let bytes = fs::read(path)?;
  let value: serde_json::Value = serde_json::from_slice(&bytes)?;
  let Some(schema_version) = value.get("schema_version") else {
    return Err(ScanArtifactError::MissingField("schema_version"));
  };
  let Some(schema_version) = schema_version.as_str() else {
    return Err(ScanArtifactError::SchemaMismatch {
      found: schema_version.to_string(),
    });
  };
  if schema_version.is_empty() {
    return Err(ScanArtifactError::MissingField("schema_version"));
  }
  let frame: ScanFrame = serde_json::from_value(value)?;
  frame.validate_wire()?;
  Ok(frame)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::frame::{ScanBounds, ScanImageRef};

  fn sample_frame() -> ScanFrame {
    ScanFrame {
      schema_version: SCAN_FRAME_SCHEMA_VERSION.to_string(),
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
      image: ScanImageRef {
        file_name: "frame-0001.png".to_string(),
        width: 8,
        height: 8,
        media_type: "image/png".to_string(),
      },
    }
  }

  #[test]
  fn write_then_read_frame_artifact_roundtrip() {
    let dir = std::env::temp_dir().join(format!("auv-scan-roundtrip-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let written = write_frame_artifact(&dir, &sample_frame()).expect("write");
    let read_back = read_frame_artifact(&written).expect("read");
    assert_eq!(read_back, sample_frame());
    let _ = fs::remove_dir_all(&dir);
  }

  #[test]
  fn read_frame_artifact_rejects_unknown_schema_version() {
    let dir = std::env::temp_dir().join(format!("auv-scan-schema-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let mut frame = sample_frame();
    frame.schema_version = "scan-frame-v99".to_string();
    let path = dir.join("bad.json");
    fs::write(&path, serde_json::to_string_pretty(&frame).unwrap()).unwrap();
    let err = read_frame_artifact(&path).expect_err("schema");
    assert!(matches!(err, ScanArtifactError::SchemaMismatch { .. }));
    let _ = fs::remove_dir_all(&dir);
  }

  #[test]
  fn read_frame_artifact_rejects_missing_schema_version() {
    let dir = std::env::temp_dir().join(format!("auv-scan-missing-schema-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let mut value = serde_json::to_value(sample_frame()).unwrap();
    value.as_object_mut().unwrap().remove("schema_version");
    let path = dir.join("missing-schema.json");
    fs::write(&path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
    let err = read_frame_artifact(&path).expect_err("missing schema_version");
    assert!(matches!(
      err,
      ScanArtifactError::MissingField("schema_version")
    ));
    let _ = fs::remove_dir_all(&dir);
  }

  #[test]
  fn read_frame_artifact_rejects_non_positive_bounds() {
    let dir = std::env::temp_dir().join(format!("auv-scan-bounds-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let mut frame = sample_frame();
    frame.window_bounds.width = 0;
    let path = dir.join("bad-bounds.json");
    fs::write(&path, serde_json::to_string_pretty(&frame).unwrap()).unwrap();
    let err = read_frame_artifact(&path).expect_err("bounds");
    assert!(matches!(
      err,
      ScanArtifactError::InvalidBounds {
        field: "window_bounds"
      }
    ));
    let _ = fs::remove_dir_all(&dir);
  }

  #[test]
  fn frame_artifact_file_name_includes_json_extension() {
    assert_eq!(frame_artifact_file_name(0), "scan-frame-0001.json");
    assert_eq!(frame_artifact_file_name(9), "scan-frame-0010.json");
  }
}
