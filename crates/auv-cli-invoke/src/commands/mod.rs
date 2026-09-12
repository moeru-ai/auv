pub mod app;
pub mod display;
pub mod input;
pub mod media_control;
mod ocr;
pub mod overlay;
pub mod scan;
pub mod screen;
pub mod window;

use std::sync::OnceLock;
use std::time::Instant;

static MONOTONIC_EPOCH: OnceLock<Instant> = OnceLock::new();

/// Milliseconds from a process-local monotonic origin.
///
/// NOTICE: This clock is not comparable to other processes' monotonic clocks
/// (for example JVM `nanoTime`); use only for same-process capture binding.
pub fn monotonic_timestamp_ms() -> u64 {
  let epoch = MONOTONIC_EPOCH.get_or_init(Instant::now);
  u64::try_from(epoch.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[derive(serde::Serialize)]
pub struct CaptureResult<'a> {
  bounds: &'a auv_driver::Rect,
  pixel_dimensions: PixelDimensions,
  scale_factor: f64,
  backend: &'a str,
  fallback_reason: Option<&'a str>,
  capture_monotonic_timestamp_ms: u64,
}

#[derive(serde::Serialize)]
struct PixelDimensions {
  width: u32,
  height: u32,
}

pub fn capture_result(capture: &auv_driver::Capture, capture_monotonic_timestamp_ms: u64) -> CaptureResult<'_> {
  CaptureResult {
    bounds: &capture.bounds,
    pixel_dimensions: PixelDimensions {
      width: capture.image.width(),
      height: capture.image.height(),
    },
    scale_factor: capture.scale_factor,
    backend: &capture.backend,
    fallback_reason: capture.fallback_reason.as_deref(),
    capture_monotonic_timestamp_ms,
  }
}

#[derive(serde::Serialize)]
pub struct DisplayCaptureResult<'a> {
  display: &'a auv_driver::Display,
  capture: CaptureResult<'a>,
}

pub fn display_capture_result<'a>(
  display: &'a auv_driver::Display,
  capture: &'a auv_driver::Capture,
  capture_monotonic_timestamp_ms: u64,
) -> DisplayCaptureResult<'a> {
  DisplayCaptureResult {
    display,
    capture: capture_result(capture, capture_monotonic_timestamp_ms),
  }
}
