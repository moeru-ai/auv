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
  pixel_dimensions: PixelDimensions,
  scale_factor: f64,
  backend: &'a str,
  fallback_reason: Option<&'a str>,
}

#[derive(serde::Serialize)]
struct PixelDimensions {
  width: u32,
  height: u32,
}

pub fn capture_result(capture: &auv_driver::Capture) -> CaptureResult<'_> {
  CaptureResult {
    bounds: &capture.bounds,
    pixel_dimensions: PixelDimensions {
      width: capture.image.width(),
      height: capture.image.height(),
    },
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

pub fn display_capture_result<'a>(display: &'a auv_driver::Display, capture: &'a auv_driver::Capture) -> DisplayCaptureResult<'a> {
  DisplayCaptureResult {
    display,
    capture: capture_result(capture),
  }
}
