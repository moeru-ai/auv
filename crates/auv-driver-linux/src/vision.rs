//! Capture-driven text recognition that backs the Linux session `VisionApi`.
//!
//! Linux OCR mirrors the Windows driver shape: `ocr` owns the Tesseract call,
//! while this module owns crop math and projection from cropped-image pixels
//! back into capture coordinates.

use auv_driver_common::capture::Capture;
use auv_driver_common::error::DriverResult;
use auv_driver_common::geometry::{RatioRect, Rect};
pub use auv_driver_common::vision::{OcrMatch, OcrMatches};
use auv_driver_common::vision::{RecognizedText, TextRecognition, TextRecognitionOptions};

use crate::error::backend;
use crate::ocr::recognize_text_in_rgba;

pub fn recognize_text_in_capture(capture: &Capture, region: RatioRect, options: &TextRecognitionOptions) -> DriverResult<TextRecognition> {
  let crop = crop_pixels(capture, region);
  if crop.width == 0 || crop.height == 0 {
    return Ok(TextRecognition::default());
  }
  let cropped = image::imageops::crop_imm(&capture.image, crop.x, crop.y, crop.width, crop.height).to_image();
  let recognition =
    recognize_text_in_rgba(cropped.as_raw(), crop.width, crop.height, options).map_err(|error| backend(error.to_string()))?;
  Ok(map_recognition_to_capture(&recognition, capture, crop))
}

pub fn find_text_in_capture(
  capture: &Capture,
  query: &str,
  region: RatioRect,
  options: &TextRecognitionOptions,
) -> DriverResult<OcrMatches> {
  let recognition = recognize_text_in_capture(capture, region, options)?;
  Ok(ocr_matches_from_recognition(&recognition, query))
}

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
  let width = ratio_to_pixel(region.width, image_width).min(image_width - x);
  let height = ratio_to_pixel(region.height, image_height).min(image_height - y);
  CropPixels {
    x,
    y,
    width,
    height,
  }
}

fn ratio_to_pixel(ratio: f64, extent: u32) -> u32 {
  if !ratio.is_finite() || ratio <= 0.0 {
    return 0;
  }
  (f64::from(extent) * ratio).round().clamp(0.0, f64::from(extent)) as u32
}

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
  TextRecognition {
    text: regions.iter().map(|region| region.text.as_str()).collect::<Vec<_>>().join("\n"),
    regions,
  }
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
