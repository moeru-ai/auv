//! Capture-driven text recognition that backs the session `VisionApi`.
//!
//! Bridges the capture surface to the system OCR engine: a caller hands in a
//! shared [`Capture`] plus a [`RatioRect`] sub-region and receives recognized
//! text whose bounds are mapped back into the capture's coordinate space
//! (screen for display captures, window for window captures). This mirrors the
//! macOS driver's `VisionApi`, keeping pixel<->capture-space mapping out of
//! consumer code. The crop and coordinate math is host-independent so it stays
//! unit-testable without a live OCR engine.

use auv_driver::capture::Capture;
use auv_driver::error::DriverResult;
use auv_driver::geometry::{Point, RatioRect, Rect};
use auv_driver::vision::{RecognizedText, TextRecognition, TextRecognitionOptions};

use crate::error::backend;
use crate::ocr::recognize_text_in_rgba;

/// A recognized text region whose bounds are expressed in the capture's
/// coordinate space, paired with a flattened confidence for ranking.
#[derive(Clone, Debug, PartialEq)]
pub struct OcrMatch {
  pub text: String,
  pub confidence: f64,
  pub bounds: Rect,
}

impl OcrMatch {
  pub fn action_point(&self) -> Point {
    self.bounds.center()
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OcrMatches {
  pub matches: Vec<OcrMatch>,
}

impl OcrMatches {
  pub fn best_match(&self) -> Option<&OcrMatch> {
    self.matches.first()
  }
}

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
mod tests {
  use auv_driver::geometry::Rect;
  use image::RgbaImage;

  use super::*;

  fn capture(width: u32, height: u32, bounds: Rect) -> Capture {
    Capture {
      image: RgbaImage::from_pixel(width, height, image::Rgba([0, 0, 0, 255])),
      bounds,
      scale_factor: 1.0,
      backend: "test".to_string(),
      fallback_reason: None,
    }
  }

  fn recognized(text: &str, bounds: Rect) -> RecognizedText {
    RecognizedText {
      text: text.to_string(),
      bounds,
      confidence: Some(0.9),
    }
  }

  #[test]
  fn crop_pixels_clamps_region_to_image_bounds() {
    let capture = capture(200, 100, Rect::new(0.0, 0.0, 200.0, 100.0));

    // A region that starts mid-image and extends past the right/bottom edges
    // must be clamped so the crop stays inside the image.
    let crop = crop_pixels(&capture, RatioRect::new(0.5, 0.5, 1.0, 1.0));

    assert_eq!(
      crop,
      CropPixels {
        x: 100,
        y: 50,
        width: 100,
        height: 50,
      }
    );
  }

  #[test]
  fn crop_pixels_full_region_covers_whole_image() {
    let capture = capture(64, 48, Rect::new(0.0, 0.0, 64.0, 48.0));

    let crop = crop_pixels(&capture, RatioRect::new(0.0, 0.0, 1.0, 1.0));

    assert_eq!(
      crop,
      CropPixels {
        x: 0,
        y: 0,
        width: 64,
        height: 48,
      }
    );
  }

  #[test]
  fn map_recognition_offsets_by_crop_origin_and_capture_scale() {
    // 2x scale: image is 200x100 px over a 100x50 capture-space frame whose
    // origin is offset, so 1 capture unit == 2 image pixels.
    let capture = capture(200, 100, Rect::new(10.0, 20.0, 100.0, 50.0));
    let crop = CropPixels {
      x: 40,
      y: 20,
      width: 100,
      height: 60,
    };
    let recognition = TextRecognition {
      text: "hi".to_string(),
      regions: vec![recognized("hi", Rect::new(8.0, 4.0, 20.0, 10.0))],
    };

    let mapped = map_recognition_to_capture(&recognition, &capture, crop);
    let bounds = mapped.regions[0].bounds;

    // full-image px = (8+40, 4+20) = (48, 24); / 2 scale = (24, 12); + origin.
    assert_eq!(bounds.origin.x, 10.0 + 24.0);
    assert_eq!(bounds.origin.y, 20.0 + 12.0);
    assert_eq!(bounds.size.width, 10.0);
    assert_eq!(bounds.size.height, 5.0);
    assert_eq!(mapped.regions[0].confidence, Some(0.9));
  }

  #[test]
  fn ocr_matches_filters_to_query_and_flattens_confidence() {
    let recognition = TextRecognition {
      text: "Play\nPause".to_string(),
      regions: vec![
        RecognizedText {
          text: "Play".to_string(),
          bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
          // 0.5 is exactly representable in f32 and f64, so the flattened
          // confidence compares cleanly.
          confidence: Some(0.5),
        },
        recognized("Pause", Rect::new(0.0, 20.0, 10.0, 10.0)),
      ],
    };

    let matches = ocr_matches_from_recognition(&recognition, "play");

    assert_eq!(matches.matches.len(), 1);
    let best = matches.best_match().expect("one match");
    assert_eq!(best.text, "Play");
    assert_eq!(best.confidence, 0.5);
  }

  #[test]
  fn empty_region_recognizes_nothing_without_calling_ocr() {
    let capture = capture(100, 100, Rect::new(0.0, 0.0, 100.0, 100.0));

    // A zero-width region short-circuits before reaching the OCR engine, so
    // this stays valid (and asserts an empty result) on every target.
    let recognition = recognize_text_in_capture(&capture, RatioRect::new(0.0, 0.0, 0.0, 1.0), &Default::default())
      .expect("empty region yields empty recognition");

    assert!(recognition.regions.is_empty());
    assert!(recognition.text.is_empty());
  }
}
