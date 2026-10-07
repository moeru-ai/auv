pub mod app;
pub mod display;
pub mod input;
pub mod media_control;
mod ocr;
pub mod overlay;
pub mod scan;
pub mod screen;
pub mod window;

#[derive(serde::Serialize)]
pub struct CaptureResult<'a> {
  bounds: &'a auv_driver::Rect,
  pixel_dimensions: auv_driver::PixelSize,
  scale_factor: f64,
  backend: &'a str,
  fallback_reason: Option<&'a str>,
}

impl CaptureResult<'_> {
  /// `WIDTHxHEIGHT` for human reports.
  fn pixel_size_report(&self) -> String {
    format!("{}x{}", self.pixel_dimensions.width, self.pixel_dimensions.height)
  }
}

/// Facts of a Runner-held capture; the same JSON as an in-process capture.
pub fn runner_capture_result(capture: &auv::client::runner::RunnerCapture) -> CaptureResult<'_> {
  CaptureResult {
    bounds: &capture.bounds,
    pixel_dimensions: capture.pixel_size,
    scale_factor: capture.scale_factor,
    backend: &capture.backend,
    fallback_reason: capture.fallback_reason.as_deref(),
  }
}

pub fn capture_result(capture: &auv_driver::Capture) -> CaptureResult<'_> {
  CaptureResult {
    bounds: &capture.bounds,
    pixel_dimensions: auv_driver::PixelSize::new(capture.image.width(), capture.image.height()),
    scale_factor: capture.scale_factor,
    backend: &capture.backend,
    fallback_reason: capture.fallback_reason.as_deref(),
  }
}

#[derive(serde::Serialize)]
pub struct DisplayCaptureResult<'a> {
  display: &'a auv_driver::Display,
  capture: CaptureResult<'a>,
}

pub fn display_capture_result<'a>(display: &'a auv_driver::Display, capture: CaptureResult<'a>) -> DisplayCaptureResult<'a> {
  DisplayCaptureResult { display, capture }
}
