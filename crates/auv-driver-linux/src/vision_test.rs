use auv_driver_common::geometry::Rect;
use image::RgbaImage;

use super::*;

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
fn map_recognition_offsets_by_crop_origin_and_capture_scale() {
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

  assert_eq!(bounds.origin.x, 34.0);
  assert_eq!(bounds.origin.y, 32.0);
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

  let recognition = recognize_text_in_capture(&capture, RatioRect::new(0.0, 0.0, 0.0, 1.0), &Default::default())
    .expect("empty region yields empty recognition");

  assert!(recognition.regions.is_empty());
  assert!(recognition.text.is_empty());
}
