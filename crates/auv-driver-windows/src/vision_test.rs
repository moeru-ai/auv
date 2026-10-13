use auv_driver_common::geometry::Rect;
use image::RgbaImage;

use super::*;

fn capture(width: u32, height: u32, bounds: Rect) -> Capture {
  Capture {
    origin: None,
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
  let crop = crop_pixels(&capture, RelativeRect::new(0.5, 0.5, 1.0, 1.0));

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

  let crop = crop_pixels(&capture, RelativeRect::new(0.0, 0.0, 1.0, 1.0));

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
  let mut capture = capture(200, 100, Rect::new(10.0, 20.0, 100.0, 50.0));
  capture.origin = Some(auv_driver_common::Position::in_window(
    &auv_driver_common::WindowRef {
      id: "geometry-window".into(),
    },
    auv_driver_common::WindowPoint::new(0.0, 0.0),
  ));
  let crop = CropPixels {
    x: 40,
    y: 20,
    width: 100,
    height: 60,
  };
  let recognition = TextRecognition {
    origin: None,
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
  let local = mapped.relative_to(&capture).unwrap();
  let target = local.positioned_regions().unwrap().next().unwrap();
  assert_eq!(target.position.point, auv_driver_common::Point::new(29.0, 14.5));
  assert_eq!(target.position.coordinate_space, auv_driver_common::CoordinateSpace::Window("geometry-window".into()));
}

// ROOT CAUSE:
//
// If a query matched one word of a long Windows OCR line (a menu bar such as
// "File Edit View"), the match carried the whole line's bounds, because only
// line regions reached the matcher, so window.clickText clicked the middle of
// the line instead of the word.
//
// The fix matches over the OCR word boxes and bounds a match by its words.
#[test]
fn a_match_inside_a_long_line_is_bounded_by_its_words() {
  use auv_driver_common::vision::RecognizedWord;
  let word = |text: &str, x: f64| RecognizedWord {
    text: text.to_string(),
    bounds: Rect::new(x, 4.0, 30.0, 10.0),
    confidence: None,
  };
  let lines = [crate::ocr::OcrLine {
    text: "File Edit View".to_string(),
    words: vec![word("File", 0.0), word("Edit", 40.0), word("View", 80.0)],
  }];
  let capture = capture(400, 200, Rect::new(100.0, 50.0, 200.0, 100.0));
  let crop = CropPixels {
    x: 20,
    y: 10,
    width: 200,
    height: 100,
  };

  let matches = map_matches_to_capture(match_word_spans(lines.iter().map(|line| line.words.as_slice()), "edit"), &capture, crop);

  assert_eq!(matches.matches.len(), 1);
  assert_eq!(matches.matches[0].text, "Edit");
  // (40 + 20) / 2 + 100, (4 + 10) / 2 + 50, 30 / 2, 10 / 2
  assert_eq!(matches.matches[0].bounds, Rect::new(130.0, 57.0, 15.0, 5.0));
  assert_eq!(text_recognition_from_lines(&lines).regions[0].bounds, Rect::new(0.0, 4.0, 110.0, 10.0));
}

#[test]
fn empty_region_recognizes_nothing_without_calling_ocr() {
  let capture = capture(100, 100, Rect::new(0.0, 0.0, 100.0, 100.0));

  // A zero-width region short-circuits before reaching the OCR engine, so
  // this stays valid (and asserts an empty result) on every target.
  let recognition = recognize_text_in_capture(&capture, RelativeRect::new(0.0, 0.0, 0.0, 1.0), &Default::default())
    .expect("empty region yields empty recognition");

  assert!(recognition.regions.is_empty());
  assert!(recognition.text.is_empty());
}
