//! Adjacent-frame viewport motion read-model (crate-local; no durable wire in v0).

use thiserror::Error;

use crate::frame::ScanFrame;
use crate::reader::ScanFrameBundle;

#[derive(Clone, Debug, PartialEq)]
pub struct MotionEstimate {
  pub delta_x: i64,
  pub delta_y: i64,
  pub confidence: f64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MotionUnknown {
  pub code: String,
  pub message: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MotionResult {
  Estimated(MotionEstimate),
  Unknown(MotionUnknown),
}

#[derive(Debug, Error)]
pub enum MotionError {
  #[error("motion requires at least two frames, found {found}")]
  InsufficientFrames { found: usize },
}

/// Estimate viewport motion between two adjacent frames (2D `window_bounds` delta).
pub(crate) fn estimate_viewport_motion_between(first: &ScanFrame, second: &ScanFrame) -> MotionResult {
  if second.sequence_index <= first.sequence_index {
    return MotionResult::Unknown(MotionUnknown {
      code: "motion_unknown".into(),
      message: "non-monotonic sequence_index between adjacent frames".into(),
    });
  }
  MotionResult::Estimated(MotionEstimate {
    delta_x: second.window_bounds.x - first.window_bounds.x,
    delta_y: second.window_bounds.y - first.window_bounds.y,
    confidence: 1.0,
  })
}

/// Estimate viewport motion between the first two frames in `bundle` (two-frame helper).
pub fn estimate_viewport_motion(bundle: &ScanFrameBundle) -> Result<MotionResult, MotionError> {
  if bundle.frames.len() < 2 {
    return Err(MotionError::InsufficientFrames {
      found: bundle.frames.len(),
    });
  }
  Ok(estimate_viewport_motion_between(&bundle.frames[0], &bundle.frames[1]))
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use super::*;
  use crate::frame::ScanFrame;
  use crate::producer::produce_frames_from_fixture_dir;
  use crate::reader::load_scan_frames_from_dir;

  fn two_frame_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scan/temporal/two_frame_v0")
  }

  #[test]
  fn estimate_viewport_motion_matches_two_frame_fixture() {
    let fixture_dir = two_frame_fixture_dir();
    let out_dir = std::env::temp_dir().join(format!("auv-scan-motion-estimate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out_dir);
    produce_frames_from_fixture_dir(&fixture_dir, &out_dir).expect("produce");
    let bundle = load_scan_frames_from_dir(&out_dir).expect("load");
    let result = estimate_viewport_motion(&bundle).expect("estimate");
    match result {
      MotionResult::Estimated(estimate) => {
        assert_eq!(estimate.delta_x, 0);
        assert_eq!(estimate.delta_y, 12);
      }
      other => panic!("expected estimate, got {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&out_dir);
  }

  #[test]
  fn estimate_viewport_motion_rejects_single_frame() {
    let bundle = ScanFrameBundle {
      frames: vec![ScanFrame {
        schema_version: crate::frame::SCAN_FRAME_SCHEMA_VERSION.to_string(),
        frame_id: "only".into(),
        sequence_index: 0,
        captured_at_millis: 0,
        window_bounds: crate::frame::ScanBounds {
          x: 0,
          y: 0,
          width: 8,
          height: 8,
        },
        viewport_bounds: None,
        image: crate::frame::ScanImageRef {
          file_name: "only.png".into(),
          width: 8,
          height: 8,
          media_type: "image/png".into(),
        },
      }],
      source_dir: PathBuf::from("/tmp"),
      loaded_json_paths: Vec::new(),
    };
    let err = estimate_viewport_motion(&bundle).expect_err("single");
    assert!(matches!(err, MotionError::InsufficientFrames { found: 1 }));
  }
}
