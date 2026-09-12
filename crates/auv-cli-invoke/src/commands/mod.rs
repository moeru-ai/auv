pub mod app;
pub mod display;
pub mod input;
pub mod media_control;
mod ocr;
pub mod overlay;
pub mod scan;
pub mod screen;
pub mod window;

#[cfg(not(windows))]
use std::sync::OnceLock;
#[cfg(not(windows))]
use std::time::Instant;

#[cfg(not(windows))]
static MONOTONIC_EPOCH: OnceLock<Instant> = OnceLock::new();

/// Milliseconds from a monotonic origin used as the capture-instant witness.
///
/// NOTICE: This clock is not comparable to other processes' monotonic clocks
/// (for example JVM `nanoTime`); `bind_capture_to_frame` records skew without
/// pretending the bases match.
///
/// NOTICE: `auv invoke` is a short-lived process. A process-local `Instant`
/// epoch initialized at the first capture call is almost always 0 ms, which
/// made live `window.capture` JSON report `capture_monotonic_timestamp_ms: 0`
/// on 2026-09-12. Windows therefore uses `GetTickCount64` (ms since boot) so
/// successive CLI invocations get distinct non-zero stamps.
pub fn monotonic_timestamp_ms() -> u64 {
  #[cfg(windows)]
  {
    windows_tick_count64_ms()
  }
  #[cfg(not(windows))]
  {
    let epoch = MONOTONIC_EPOCH.get_or_init(Instant::now);
    u64::try_from(epoch.elapsed().as_millis()).unwrap_or(u64::MAX)
  }
}

#[cfg(windows)]
fn windows_tick_count64_ms() -> u64 {
  #[link(name = "kernel32")]
  unsafe extern "system" {
    fn GetTickCount64() -> u64;
  }
  // SAFETY: GetTickCount64 is a parameterless kernel32 query that returns
  // milliseconds since system start. It is valid to call at any time.
  unsafe { GetTickCount64() }
}

#[cfg(test)]
mod monotonic_clock_tests {
  use super::monotonic_timestamp_ms;
  use std::time::Duration;

  #[test]
  fn capture_clock_is_non_decreasing() {
    let first = monotonic_timestamp_ms();
    std::thread::sleep(Duration::from_millis(2));
    let second = monotonic_timestamp_ms();
    assert!(second >= first);
    #[cfg(windows)]
    assert!(first > 0, "Windows GetTickCount64 must not stamp short-lived invoke as 0");
  }
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
