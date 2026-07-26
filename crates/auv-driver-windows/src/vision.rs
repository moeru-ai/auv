//! Capture-driven text recognition that backs the session `VisionApi`.
//!
//! Bridges the capture surface to the system OCR engine: a caller hands in a
//! shared [`Capture`] plus a [`RatioRect`] sub-region and receives recognized
//! text whose bounds are mapped back into the capture's coordinate space
//! (screen for display captures, window for window captures). This mirrors the
//! macOS driver's `VisionApi`, keeping pixel<->capture-space mapping out of
//! consumer code. The crop and coordinate math is host-independent so it stays
//! unit-testable without a live OCR engine.

use auv_driver_common::capture::Capture;
use auv_driver_common::error::DriverResult;
use auv_driver_common::geometry::{RatioRect, Rect};
pub use auv_driver_common::vision::{OcrMatch, OcrMatches};
use auv_driver_common::vision::{RecognizedText, TextRecognition, TextRecognitionOptions};

use crate::error::backend;
use crate::ocr::recognize_text_in_rgba;

/// Recognizes text inside `region` of `capture`, returning bounds in the
/// capture's coordinate space.
pub fn recognize_text_in_capture(capture: &Capture, region: RatioRect, options: &TextRecognitionOptions) -> DriverResult<TextRecognition> {
  let crop = crop_pixels(capture, region);
  if crop.width == 0 || crop.height == 0 {
    // An empty sub-region has no pixels to recognize; return an empty result
    // rather than handing a zero-sized bitmap to the OCR engine.
    return Ok(TextRecognition::default());
  }
  let cropped = image::imageops::crop_imm(&capture.image, crop.x, crop.y, crop.width, crop.height).to_image();
  let recognition = recognize_text_in_rgba(cropped.as_raw(), crop.width, crop.height, options).map_err(backend)?;
  Ok(map_recognition_to_capture(&recognition, capture, crop))
}

/// Recognizes text inside `region` and filters to regions containing `query`.
pub fn find_text_in_capture(
  capture: &Capture,
  query: &str,
  region: RatioRect,
  options: &TextRecognitionOptions,
) -> DriverResult<OcrMatches> {
  let recognition = recognize_text_in_capture(capture, region, options)?;
  Ok(ocr_matches_from_recognition(&recognition, query))
}

/// Integer pixel sub-rectangle of a capture image, clamped to the image bounds.
#[derive(Clone, Copy, Debug, PartialEq)]
struct CropPixels {
  x: u32,
  y: u32,
  width: u32,
  height: u32,
}

fn crop_pixels(capture: &Capture, region: RatioRect) -> CropPixels {
  let image_width = capture.image.width();
  let image_height = capture.image.height();
  let x = ratio_to_pixel(region.x, image_width);
  let y = ratio_to_pixel(region.y, image_height);
  // Clamp the extent so the crop never runs past the image edge; an origin at
  // or beyond the edge yields a zero-sized (and therefore skipped) crop.
  let width = ratio_to_pixel(region.width, image_width).min(image_width - x);
  let height = ratio_to_pixel(region.height, image_height).min(image_height - y);
  CropPixels {
    x,
    y,
    width,
    height,
  }
}

/// Converts a ratio along one axis into a pixel offset clamped to `[0, extent]`.
fn ratio_to_pixel(ratio: f64, extent: u32) -> u32 {
  if !ratio.is_finite() || ratio <= 0.0 {
    return 0;
  }
  (f64::from(extent) * ratio).round().clamp(0.0, f64::from(extent)) as u32
}

/// Maps OCR regions (in cropped-image pixels) into the capture's coordinate
/// space, mirroring the macOS driver's capture-space projection.
fn map_recognition_to_capture(recognition: &TextRecognition, capture: &Capture, crop: CropPixels) -> TextRecognition {
  let x_scale = if capture.bounds.size.width > 0.0 {
    f64::from(capture.image.width()) / capture.bounds.size.width
  } else {
    1.0
  };
  let y_scale = if capture.bounds.size.height > 0.0 {
    f64::from(capture.image.height()) / capture.bounds.size.height
  } else {
    1.0
  };
  let regions = recognition
    .regions
    .iter()
    .map(|region| {
      // OCR bounds are in cropped-image pixels: shift by the crop origin to get
      // full-image pixels, then divide by the capture's pixels-per-unit scale
      // and offset by the capture origin to land in capture space.
      let full_x = region.bounds.origin.x + f64::from(crop.x);
      let full_y = region.bounds.origin.y + f64::from(crop.y);
      RecognizedText {
        text: region.text.clone(),
        confidence: region.confidence,
        bounds: Rect::new(
          capture.bounds.origin.x + full_x / x_scale,
          capture.bounds.origin.y + full_y / y_scale,
          region.bounds.size.width / x_scale,
          region.bounds.size.height / y_scale,
        ),
      }
    })
    .collect::<Vec<_>>();
  let text = regions.iter().map(|region| region.text.as_str()).collect::<Vec<_>>().join("\n");
  TextRecognition { text, regions }
}

fn ocr_matches_from_recognition(recognition: &TextRecognition, query: &str) -> OcrMatches {
  let matches = recognition
    .find_contains(query)
    .into_iter()
    .map(|recognized| OcrMatch {
      text: recognized.text.clone(),
      confidence: recognized.confidence.unwrap_or_default() as f64,
      bounds: recognized.bounds,
    })
    .collect();
  OcrMatches { matches }
}

#[cfg(test)]
#[path = "vision_test.rs"]
mod tests;
