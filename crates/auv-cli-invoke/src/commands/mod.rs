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
  /// Runner reference for subsequent OCR, image fetches and guarded clicks.
  /// Local captures have no Runner resource and omit this field.
  #[serde(skip_serializing_if = "Option::is_none")]
  capture_id: Option<&'a str>,
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

/// Facts of a Runner-held capture, including its reference for later operations.
pub fn runner_capture_result(capture: &auv::client::runner::RunnerCapture) -> CaptureResult<'_> {
  CaptureResult {
    capture_id: Some(capture.reference.id()),
    bounds: &capture.bounds,
    pixel_dimensions: capture.pixel_size,
    scale_factor: capture.scale_factor,
    backend: &capture.backend,
    fallback_reason: capture.fallback_reason.as_deref(),
  }
}

pub fn capture_result(capture: &auv_driver::Capture) -> CaptureResult<'_> {
  CaptureResult {
    capture_id: None,
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
