use super::*;
use crate::frame::{SCAN_FRAME_SCHEMA_VERSION, ScanBounds, ScanImageDimensions};

fn frame(sequence_index: u32, x: i64, y: i64) -> ScanFrame {
  ScanFrame {
    schema_version: SCAN_FRAME_SCHEMA_VERSION.into(),
    frame_id: format!("frame-{sequence_index}"),
    sequence_index,
    captured_at_millis: 0,
    window_bounds: ScanBounds {
      x,
      y,
      width: 8,
      height: 8,
    },
    viewport_bounds: None,
    image_dimensions: ScanImageDimensions {
      width: 8,
      height: 8,
    },
  }
}

#[test]
fn estimates_signed_two_dimensional_delta() {
  let bundle = ScanFrameBundle {
    frames: vec![frame(0, 7, 20), frame(1, -3, 32)],
  };

  let result = estimate_viewport_motion(&bundle).expect("two frames");
  assert!(matches!(
    result,
    MotionResult::Estimated(MotionEstimate {
      delta_x: -10,
      delta_y: 12,
      confidence: 1.0,
    })
  ));
}

#[test]
fn rejects_a_bundle_without_an_adjacent_pair() {
  let bundle = ScanFrameBundle {
    frames: vec![frame(0, 0, 0)],
  };

  let error = estimate_viewport_motion(&bundle).expect_err("single frame");
  assert!(matches!(error, MotionError::InsufficientFrames { found: 1 }));
}

#[test]
fn non_monotonic_sequence_has_no_motion_estimate() {
  let result = estimate_viewport_motion_between(&frame(4, 0, 0), &frame(3, 4, 5));

  assert!(matches!(
    result,
    MotionResult::Unknown(MotionUnknown { code, .. }) if code == "motion_unknown"
  ));
}
