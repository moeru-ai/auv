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
fn dpi_scaling_100_125_150_deterministic_ocr_bounds_and_action_point_mapping() {
  // Logical window frame on screen: origin (100.0, 200.0), size 800x600 logical units.
  let logical_frame = Rect::new(100.0, 200.0, 800.0, 600.0);
  // Expected logical target button: origin (220.0, 250.0), size 120.0 x 40.0.
  // Within the window, logical offset is (120.0, 50.0).
  let expected_bounds = Rect::new(220.0, 250.0, 120.0, 40.0);
  let expected_action_point = Point::new(280.0, 270.0);

  // Test across standard Windows scaling configurations: 100%, 125%, 150%, and 200%.
  let configs: [(f64, u32, u32); 4] = [
    (1.0, 800, 600),   // 100% DPI (96 DPI)
    (1.25, 1000, 750), // 125% DPI (120 DPI)
    (1.5, 1200, 900),  // 150% DPI (144 DPI)
    (2.0, 1600, 1200), // 200% DPI (192 DPI)
  ];

  for (scale, img_w, img_h) in configs {
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

    // OCR engine produces bounding box in physical image pixels
    let pixel_bounds = Rect::new(120.0 * scale, 50.0 * scale, 120.0 * scale, 40.0 * scale);

    let mapped = map_rect_to_capture(pixel_bounds, &cap, crop);

    assert!((mapped.origin.x - expected_bounds.origin.x).abs() < 1e-9, "X mismatch at scale {scale}");
    assert!((mapped.origin.y - expected_bounds.origin.y).abs() < 1e-9, "Y mismatch at scale {scale}");
    assert!((mapped.size.width - expected_bounds.size.width).abs() < 1e-9, "Width mismatch at scale {scale}");
    assert!((mapped.size.height - expected_bounds.size.height).abs() < 1e-9, "Height mismatch at scale {scale}");
    assert!((mapped.center().x - expected_action_point.x).abs() < 1e-9, "Action point X mismatch at scale {scale}");
    assert!((mapped.center().y - expected_action_point.y).abs() < 1e-9, "Action point Y mismatch at scale {scale}");
  }
}

#[test]
fn dpi_scaling_with_subregion_crop_projects_exact_action_point() {
  let logical_frame = Rect::new(100.0, 200.0, 800.0, 600.0);
  // Logical button inside sub-region: origin (260.0, 320.0), size 120.0 x 40.0
  // Window-relative offset: (160.0, 120.0)
  let expected_bounds = Rect::new(260.0, 320.0, 120.0, 40.0);
  let expected_action_point = Point::new(320.0, 340.0);

  // Sub-region: x: 10%, y: 10%, width: 80%, height: 80%
  let subregion = RelativeRect::new(0.1, 0.1, 0.8, 0.8);

  let configs: [(f64, u32, u32); 3] = [
    (1.0, 800, 600),   // 100%
    (1.25, 1000, 750), // 125%
    (1.5, 1200, 900),  // 150%
  ];

  for (scale, img_w, img_h) in configs {
    let cap = Capture {
      origin: None,
      image: RgbaImage::from_pixel(img_w, img_h, image::Rgba([0, 0, 0, 255])),
      bounds: logical_frame,
      scale_factor: scale,
      backend: "test".to_string(),
      fallback_reason: None,
    };
    let crop = crop_pixels(&cap, subregion);

    // Full pixel coordinates of the button
    let full_px_x = 160.0 * scale;
    let full_px_y = 120.0 * scale;

    // Within the cropped image, OCR coordinates are relative to the crop origin
    let in_crop_bounds = Rect::new(full_px_x - f64::from(crop.x), full_px_y - f64::from(crop.y), 120.0 * scale, 40.0 * scale);

    let mapped = map_rect_to_capture(in_crop_bounds, &cap, crop);

    assert!((mapped.origin.x - expected_bounds.origin.x).abs() < 1e-9, "Crop X mismatch at scale {scale}");
    assert!((mapped.origin.y - expected_bounds.origin.y).abs() < 1e-9, "Crop Y mismatch at scale {scale}");
    assert!((mapped.size.width - expected_bounds.size.width).abs() < 1e-9, "Crop width mismatch at scale {scale}");
    assert!((mapped.size.height - expected_bounds.size.height).abs() < 1e-9, "Crop height mismatch at scale {scale}");
    assert!((mapped.center().x - expected_action_point.x).abs() < 1e-9, "Crop action point X mismatch at scale {scale}");
    assert!((mapped.center().y - expected_action_point.y).abs() < 1e-9, "Crop action point Y mismatch at scale {scale}");
  }
}

#[test]
fn multi_word_continuous_span_bounds_and_action_point_calculation() {
  use auv_driver_common::vision::RecognizedWord;
  let word = |text: &str, x: f64, width: f64| RecognizedWord {
    text: text.to_string(),
    bounds: Rect::new(x, 50.0, width, 20.0),
    confidence: Some(0.95),
  };
  let lines = [crate::ocr::OcrLine {
    text: "Open Project Settings Dialog".to_string(),
    words: vec![
      word("Open", 10.0, 40.0),
      word("Project", 60.0, 60.0),
      word("Settings", 130.0, 70.0),
      word("Dialog", 210.0, 50.0),
    ],
  }];
  let capture = capture(600, 300, Rect::new(100.0, 200.0, 600.0, 300.0));
  let crop = CropPixels {
    x: 0,
    y: 0,
    width: 600,
    height: 300,
  };

  // 1. Single word query "project": matches only "Project"
  let single = map_matches_to_capture(match_word_spans(lines.iter().map(|l| l.words.as_slice()), "project"), &capture, crop);
  assert_eq!(single.matches.len(), 1);
  assert_eq!(single.matches[0].text, "Project");
  assert_eq!(single.matches[0].bounds, Rect::new(160.0, 250.0, 60.0, 20.0));
  assert_eq!(single.matches[0].action_point(), Point::new(190.0, 260.0));

  // 2. Multi-word continuous span "project settings": bounds is union of "Project" and "Settings"
  let span2 = map_matches_to_capture(match_word_spans(lines.iter().map(|l| l.words.as_slice()), "project settings"), &capture, crop);
  assert_eq!(span2.matches.len(), 1);
  assert_eq!(span2.matches[0].text, "Project Settings");
  assert_eq!(span2.matches[0].bounds, Rect::new(160.0, 250.0, 140.0, 20.0));
  assert_eq!(span2.matches[0].action_point(), Point::new(230.0, 260.0));

  // 3. Three-word span "open project settings": bounds is union of 3 words
  let span3 = map_matches_to_capture(match_word_spans(lines.iter().map(|l| l.words.as_slice()), "open project settings"), &capture, crop);
  assert_eq!(span3.matches.len(), 1);
  assert_eq!(span3.matches[0].text, "Open Project Settings");
  assert_eq!(span3.matches[0].bounds, Rect::new(110.0, 250.0, 190.0, 20.0));
  assert_eq!(span3.matches[0].action_point(), Point::new(205.0, 260.0));

  // The span action point strictly matches the sub-phrase center, not the whole line (10..260 -> center 135.0)
  assert_ne!(span2.matches[0].action_point().x, 100.0 + 135.0);
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
