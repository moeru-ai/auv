//! Capture-driven text recognition that backs the session `VisionApi`.
//!
//! Bridges the capture surface to the system OCR engine: a caller hands in a
//! shared [`Capture`] plus a [`RelativeRect`] sub-region and receives recognized
//! text whose bounds are mapped back into the capture's coordinate space
//! (screen for display captures, window for window captures). This mirrors the
//! macOS driver's `VisionApi`, keeping pixel<->capture-space mapping out of
//! consumer code. The crop and coordinate math is host-independent so it stays
//! unit-testable without a live OCR engine.

use auv_driver_common::capture::Capture;
use auv_driver_common::error::DriverResult;
use auv_driver_common::geometry::{Rect, RelativeRect};
pub use auv_driver_common::vision::{OcrMatch, OcrMatches};
use auv_driver_common::vision::{RecognizedText, TextRecognition, TextRecognitionOptions, match_word_spans};

use crate::error::backend;
use crate::ocr::{OcrLine, recognize_lines_in_rgba, text_recognition_from_lines};

/// Recognizes text inside `region` of `capture`, returning bounds in the
/// capture's coordinate space.
pub fn recognize_text_in_capture(
  capture: &Capture,
  region: RelativeRect,
  options: &TextRecognitionOptions,
) -> DriverResult<TextRecognition> {
  let Some((lines, crop)) = recognize_lines_in_capture(capture, region, options)? else {
    // An empty sub-region has no pixels to recognize; return an empty result
    // rather than handing a zero-sized bitmap to the OCR engine.
    return Ok(TextRecognition {
      origin: capture.recognition_origin(),
      ..Default::default()
    });
  };
  Ok(map_recognition_to_capture(&text_recognition_from_lines(&lines), capture, crop))
}

/// Finds `query` inside `region`. Each match is bounded by the words it
/// matched, not by its whole OCR line, so its action point lands on the text.
pub fn find_text_in_capture(
  capture: &Capture,
  query: &str,
  region: RelativeRect,
  options: &TextRecognitionOptions,
) -> DriverResult<OcrMatches> {
  let Some((lines, crop)) = recognize_lines_in_capture(capture, region, options)? else {
    return Ok(OcrMatches::default());
  };
  Ok(map_matches_to_capture(match_word_spans(lines.iter().map(|line| line.words.as_slice()), query), capture, crop))
}

/// Runs OCR on the cropped `region`, or returns `None` for an empty crop.
fn recognize_lines_in_capture(
  capture: &Capture,
  region: RelativeRect,
  options: &TextRecognitionOptions,
) -> DriverResult<Option<(Vec<OcrLine>, CropPixels)>> {
  let crop = crop_pixels(capture, region);
  if crop.width == 0 || crop.height == 0 {
    return Ok(None);
  }
  let cropped = image::imageops::crop_imm(&capture.image, crop.x, crop.y, crop.width, crop.height).to_image();
  let start_time = std::time::Instant::now();
  let lines = recognize_lines_in_rgba(cropped.as_raw(), crop.width, crop.height, options).map_err(backend)?;
  let elapsed_ms = start_time.elapsed().as_secs_f64() * 1000.0;
  crate::latency::record_latency_event(
    "recognize_text_in_capture",
    elapsed_ms,
    Some((crop.width, crop.height)),
    Some("Windows.Media.Ocr"),
    None,
  );
  Ok(Some((lines, crop)))
}

/// Integer pixel sub-rectangle of a capture image, clamped to the image bounds.
#[derive(Clone, Copy, Debug, PartialEq)]
struct CropPixels {
  x: u32,
  y: u32,
  width: u32,
  height: u32,
}

fn crop_pixels(capture: &Capture, region: RelativeRect) -> CropPixels {
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
  let regions = recognition
    .regions
    .iter()
    .map(|region| RecognizedText {
      text: region.text.clone(),
      confidence: region.confidence,
      bounds: map_rect_to_capture(region.bounds, capture, crop),
    })
    .collect::<Vec<_>>();
  let text = regions.iter().map(|region| region.text.as_str()).collect::<Vec<_>>().join("\n");
  TextRecognition {
    origin: capture.recognition_origin(),
    text,
    regions,
  }
}

fn map_matches_to_capture(matches: OcrMatches, capture: &Capture, crop: CropPixels) -> OcrMatches {
  OcrMatches {
    matches: matches
      .matches
      .into_iter()
      .map(|matched| OcrMatch {
        bounds: map_rect_to_capture(matched.bounds, capture, crop),
        ..matched
      })
      .collect(),
  }
}

/// Maps a rect in cropped-image pixels into the capture's coordinate space.
fn map_rect_to_capture(bounds: Rect, capture: &Capture, crop: CropPixels) -> Rect {
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
  // OCR bounds are in cropped-image pixels: shift by the crop origin to get
  // full-image pixels, then divide by the capture's pixels-per-unit scale
  // and offset by the capture origin to land in capture space.
  let full_x = bounds.origin.x + f64::from(crop.x);
  let full_y = bounds.origin.y + f64::from(crop.y);
  Rect::new(
    capture.bounds.origin.x + full_x / x_scale,
    capture.bounds.origin.y + full_y / y_scale,
    bounds.size.width / x_scale,
    bounds.size.height / y_scale,
  )
}

#[cfg(test)]
#[path = "vision_test.rs"]
mod tests;
