use auv_driver_common::geometry::CoordinateSpace;
use auv_driver_common::selector::Window as SelectWindow;
use auv_driver_common::window::WindowRef;

use super::*;

#[test]
fn resolve_from_windows_matches_title_contains() {
  let window = Window {
    reference: WindowRef {
      id: "1".to_string(),
    },
    title: Some("GNOME Text Editor".to_string()),
    app_name: Some("Text Editor".to_string()),
    app_bundle_id: None,
    process_id: Some(42),
    frame: Rect::new(0.0, 0.0, 500.0, 400.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
  };

  let resolved = resolve_from_windows(&[window.clone()], &SelectWindow::title_contains("Text Editor")).expect("window resolves");

  assert_eq!(resolved, window);
}

#[test]
fn resolve_from_windows_matches_title_contains_case_insensitive() {
  let window = Window {
    reference: WindowRef {
      id: "1".to_string(),
    },
    title: Some("Settings".to_string()),
    app_name: Some("GNOME Settings".to_string()),
    app_bundle_id: None,
    process_id: Some(42),
    frame: Rect::new(0.0, 0.0, 500.0, 400.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
  };

  let resolved = resolve_from_windows(&[window.clone()], &SelectWindow::title_contains("settings")).expect("window resolves");

  assert_eq!(resolved, window);
}

#[test]
fn resolve_from_windows_matches_app_name_contains_case_insensitive() {
  let window = Window {
    reference: WindowRef {
      id: "1".to_string(),
    },
    title: Some("Settings".to_string()),
    app_name: Some("GNOME Settings".to_string()),
    app_bundle_id: None,
    process_id: Some(42),
    frame: Rect::new(0.0, 0.0, 500.0, 400.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
  };

  let resolved = resolve_from_windows(
    &[window.clone()],
    &WindowSelector::default().owned_by(AppSelector {
      name: Some(TextMatcher::Contains("settings".to_string())),
      ..AppSelector::default()
    }),
  )
  .expect("window resolves");

  assert_eq!(resolved, window);
}

#[test]
fn crop_capture_to_window_uses_window_extents_inside_display_capture() {
  let mut image = image::RgbaImage::new(10, 10);
  image.put_pixel(3, 4, image::Rgba([1, 2, 3, 4]));
  let capture = Capture {
    image,
    bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
    scale_factor: 1.0,
    backend: "test".to_string(),
    fallback_reason: None,
  };

  let cropped = crop_capture_to_window(&capture, Rect::new(3.0, 4.0, 2.0, 2.0)).unwrap();

  assert_eq!(cropped.width(), 2);
  assert_eq!(cropped.height(), 2);
  assert_eq!(*cropped.get_pixel(0, 0), image::Rgba([1, 2, 3, 4]));
}

#[test]
fn window_source_scale_rejects_unmatched_stream_bounds() {
  let image = image::RgbaImage::new(5504, 2304);

  let error = window_source_scale_factor(&image, Rect::new(0.0, 0.0, 1505.0, 1077.0)).expect_err("non-uniform scale should be rejected");

  assert!(error.to_string().contains("not consistent with AT-SPI window bounds"));
}

#[test]
fn window_source_normalization_trims_black_portal_padding() {
  let mut image = image::RgbaImage::new(11, 6);
  for y in 1..4 {
    for x in 1..5 {
      image.put_pixel(x, y, image::Rgba([20, 20, 20, 255]));
    }
  }

  let (normalized, scale) =
    normalize_window_source_image(image, Rect::new(0.0, 0.0, 2.0, 1.5)).expect("black padding trims to target aspect");

  assert_eq!(normalized.width(), 4);
  assert_eq!(normalized.height(), 3);
  assert_eq!(scale, 2.0);
}
