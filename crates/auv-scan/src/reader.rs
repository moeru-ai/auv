//! In-memory scan frame bundles and metadata formatting.

use crate::frame::ScanFrame;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanFrameBundle {
  pub frames: Vec<ScanFrame>,
}

/// Metadata-only summary from in-memory [`ScanFrame`] fields (no disk IO).
pub fn summarize_scan_frame_text(frame: &ScanFrame) -> String {
  format!(
    "frame_id={} sequence_index={} captured_at_millis={} image={}x{} window={}x{}",
    frame.frame_id,
    frame.sequence_index,
    frame.captured_at_millis,
    frame.image_dimensions.width,
    frame.image_dimensions.height,
    frame.window_bounds.width,
    frame.window_bounds.height,
  )
}
