use crate::types::MinecraftSpatialFrame;

/// A spatial frame bound to a real captured screenshot at (approximately) the
/// same instant, carrying the screenshot artifact reference and the measured
/// capture skew.
#[derive(Clone, Debug, PartialEq)]
pub struct BoundSpatialFrame {
  /// The frame with `screenshot_artifact_ref` and `mc_capture_skew_ms` populated.
  pub frame: MinecraftSpatialFrame,
  /// The signed skew that was written onto the frame, in milliseconds.
  pub capture_skew_ms: i64,
}

/// Bind a freshly ingested spatial frame to a real screenshot capture.
///
/// The sidecar stamps each frame with `monotonic_timestamp_ms` from the running
/// client. The capture layer stamps the screenshot with its own
/// `capture_monotonic_timestamp_ms` taken at the capture instant. The skew is
/// their signed difference: `frame_ts - capture_ts`.
///
/// IMPORTANT: the two timestamps come from DIFFERENT monotonic clocks (the MC
/// client vs the capturing process), so the skew is only meaningful when the
/// caller has aligned the clock bases (e.g. both read from the same wall-clock
/// reference, or a calibration offset already applied). This function does not
/// pretend the clocks share a base; it records the difference the caller hands
/// it. Threshold enforcement and the over-skew refusal live in
/// [`crate::verify::evaluate_mismatch_refusal`], which reads the
/// `mc_capture_skew_ms` this function writes.
pub fn bind_capture_to_frame(
  mut frame: MinecraftSpatialFrame,
  screenshot_artifact_ref: Option<String>,
  capture_monotonic_timestamp_ms: u64,
) -> BoundSpatialFrame {
  let frame_ts = i64::try_from(frame.monotonic_timestamp_ms).unwrap_or(i64::MAX);
  let capture_ts = i64::try_from(capture_monotonic_timestamp_ms).unwrap_or(i64::MAX);
  let capture_skew_ms = frame_ts.saturating_sub(capture_ts);
  frame.screenshot_artifact_ref = screenshot_artifact_ref;
  frame.mc_capture_skew_ms = Some(capture_skew_ms);
  BoundSpatialFrame {
    frame,
    capture_skew_ms,
  }
}

#[cfg(test)]
#[path = "bind_test.rs"]
mod tests;
