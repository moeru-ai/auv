use auv_driver_common::geometry::Rect;
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
