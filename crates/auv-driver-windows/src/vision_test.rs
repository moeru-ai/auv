use auv_driver_common::geometry::{Point, Rect};
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

#[test]
fn pixel_to_coordinate_projection_under_scale_ratios() {
  // NOTE: map_rect_to_capture derives per-axis scaling ratios strictly from
  // image dimensions and capture bounds; it does NOT read capture.scale_factor.
  // These tests verify mathematical projection formulas under given dimensions,
  // and do NOT claim to verify native Windows DPI awareness or DWM behavior.
  let logical_frame = Rect::new(100.0, 200.0, 800.0, 600.0);
  let expected_bounds = Rect::new(220.0, 250.0, 120.0, 40.0);
  let expected_action_point = Point::new(280.0, 270.0);

  // 1. Representative symmetric scaling configurations: 100% and 150%
  let symmetric_configs: [(f64, u32, u32); 2] = [
    (1.0, 800, 600),  // 100%
    (1.5, 1200, 900), // 150%
  ];

  for (scale, img_w, img_h) in symmetric_configs {
    let cap = Capture {
      origin: None,
      image: RgbaImage::from_pixel(img_w, img_h, image::Rgba([0, 0, 0, 255])),
      bounds: logical_frame,
      scale_factor: scale,
      backend: "test".to_string(),
      fallback_reason: None,
    };
    let crop = CropPixels {
      x: 0,
      y: 0,
      width: img_w,
      height: img_h,
    };
    let pixel_bounds = Rect::new(120.0 * scale, 50.0 * scale, 120.0 * scale, 40.0 * scale);
    let mapped = map_rect_to_capture(pixel_bounds, &cap, crop);

    assert!((mapped.origin.x - expected_bounds.origin.x).abs() < 1e-9);
    assert!((mapped.origin.y - expected_bounds.origin.y).abs() < 1e-9);
    assert!((mapped.size.width - expected_bounds.size.width).abs() < 1e-9);
    assert!((mapped.size.height - expected_bounds.size.height).abs() < 1e-9);
    assert!((mapped.center().x - expected_action_point.x).abs() < 1e-9);
    assert!((mapped.center().y - expected_action_point.y).abs() < 1e-9);
  }

  // 2. Asymmetric / non-isometric X/Y scale ratios (e.g. 1.25x horizontal, 1.5x vertical)
  let asym_cap = Capture {
    origin: None,
    image: RgbaImage::from_pixel(1000, 900, image::Rgba([0, 0, 0, 255])),
    bounds: logical_frame, // 800x600 -> x_scale = 1.25, y_scale = 1.5
    scale_factor: 1.0,     // intentionally decoupled to prove scale_factor is not read
    backend: "test".to_string(),
    fallback_reason: None,
  };
  let asym_crop = CropPixels {
    x: 0,
    y: 0,
    width: 1000,
    height: 900,
  };
  let asym_pixel_bounds = Rect::new(120.0 * 1.25, 50.0 * 1.5, 120.0 * 1.25, 40.0 * 1.5);
  let asym_mapped = map_rect_to_capture(asym_pixel_bounds, &asym_cap, asym_crop);
  assert!((asym_mapped.origin.x - expected_bounds.origin.x).abs() < 1e-9);
  assert!((asym_mapped.origin.y - expected_bounds.origin.y).abs() < 1e-9);
  assert!((asym_mapped.size.width - expected_bounds.size.width).abs() < 1e-9);
  assert!((asym_mapped.size.height - expected_bounds.size.height).abs() < 1e-9);
  assert!((asym_mapped.center().x - expected_action_point.x).abs() < 1e-9);
  assert!((asym_mapped.center().y - expected_action_point.y).abs() < 1e-9);
}

#[test]
fn subregion_crop_pixels_and_projection_with_independent_expectations() {
  // Uses irregular, non-round dimensions (1001 x 751) and non-integer relative subregion
  let logical_frame = Rect::new(100.0, 200.0, 800.0, 600.0);
  let cap = Capture {
    origin: None,
    image: RgbaImage::from_pixel(1001, 751, image::Rgba([0, 0, 0, 255])),
    bounds: logical_frame,
    scale_factor: 1.25,
    backend: "test".to_string(),
    fallback_reason: None,
  };

  // Sub-region with fractional boundaries: x=12.3%, y=23.4%, w=50.0%, h=40.0%
  let subregion = RelativeRect::new(0.123, 0.234, 0.500, 0.400);

  // 1. Independently assert expected crop pixel bounds
  // x: round(0.123 * 1001) = round(123.123) = 123
  // y: round(0.234 * 751) = round(175.734) = 176
  // w: round(0.500 * 1001) = round(500.5) = 501
  // h: round(0.400 * 751) = round(300.4) = 300
  let expected_crop = CropPixels {
    x: 123,
    y: 176,
    width: 501,
    height: 300,
  };
  let crop = crop_pixels(&cap, subregion);
  assert_eq!(crop, expected_crop, "crop_pixels output must match independent computation");

  // 2. Fixed OCR match in cropped-image pixels (completely independent of crop.x / crop.y)
  let ocr_box = Rect::new(45.0, 35.0, 120.0, 40.0);

  // Independent mathematical projection to capture space:
  // scale_x = 1001.0 / 800.0 = 1.25125
  // scale_y = 751.0 / 600.0 = 1.2516666666666667
  // full_x = 45.0 + 123 = 168.0 pixels -> mapped_x = 100.0 + 168.0 * (800 / 1001) ≈ 234.26573426573427
  // full_y = 35.0 + 176 = 211.0 pixels -> mapped_y = 200.0 + 211.0 * (600 / 751) ≈ 368.5752330226365
  // mapped_w = 120.0 * (800 / 1001) ≈ 95.9040959040959
  // mapped_h = 40.0 * (600 / 751) ≈ 31.95739014647137
  let expected_x = 100.0 + (168.0 * 800.0 / 1001.0);
  let expected_y = 200.0 + (211.0 * 600.0 / 751.0);
  let expected_w = 120.0 * 800.0 / 1001.0;
  let expected_h = 40.0 * 600.0 / 751.0;
  let expected_action = Point::new(expected_x + expected_w / 2.0, expected_y + expected_h / 2.0);

  let mapped = map_rect_to_capture(ocr_box, &cap, crop);
  assert!((mapped.origin.x - expected_x).abs() < 1e-9);
  assert!((mapped.origin.y - expected_y).abs() < 1e-9);
  assert!((mapped.size.width - expected_w).abs() < 1e-9);
  assert!((mapped.size.height - expected_h).abs() < 1e-9);
  assert!((mapped.center().x - expected_action.x).abs() < 1e-9);
  assert!((mapped.center().y - expected_action.y).abs() < 1e-9);

  // 3. Confirm that an off-by-one crop error is reliably detected
  let off_by_one_crop = CropPixels {
    x: crop.x + 1,
    ..crop
  };
  let off_mapped = map_rect_to_capture(ocr_box, &cap, off_by_one_crop);
  let delta_x = (off_mapped.origin.x - expected_x).abs();
  assert!(delta_x > 0.7, "A 1-pixel crop offset must alter mapped coordinates by ~0.8 units (got {delta_x})");
}

#[test]
fn ocr_projection_with_negative_screen_origin_multimonitor() {
  let capture = Capture {
    origin: None,
    image: RgbaImage::from_pixel(1920, 1080, image::Rgba([0, 0, 0, 255])),
    bounds: Rect::new(-1920.0, -200.0, 1920.0, 1080.0),
    scale_factor: 1.0,
    backend: "test".to_string(),
    fallback_reason: None,
  };
  let crop = CropPixels {
    x: 100,
    y: 50,
    width: 800,
    height: 600,
  };
  let word_bounds = Rect::new(50.0, 30.0, 120.0, 40.0);
  let mapped = map_rect_to_capture(word_bounds, &capture, crop);

  assert_eq!(mapped, Rect::new(-1770.0, -120.0, 120.0, 40.0));
  assert_eq!(mapped.center(), Point::new(-1710.0, -100.0));
}
