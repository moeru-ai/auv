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
#[path = "motion_test.rs"]
mod tests;
