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
fn main_visible_prefers_application_window_over_desktop_shell_surface() {
  // ROOT CAUSE:
  //
  // If AT-SPI enumerated GNOME Shell before the active application, the shell
  // surface was marked main and won the default selector even though it was not
  // the application window the user could operate.
  //
  // Before the fix, `window.findText` captured the shell surface and projected
  // OCR coordinates through its unrelated bounds. The fix excludes desktop
  // shell surfaces while normal application windows are available.
  let shell = Window {
    reference: WindowRef {
      id: "shell".to_string(),
    },
    title: Some("Main stage".to_string()),
    app_name: Some("gnome-shell".to_string()),
    app_bundle_id: Some("org.gnome.Shell".to_string()),
    process_id: None,
    frame: Rect::new(0.0, 55.0, 100.0, 56.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
  };
  let application = Window {
    reference: WindowRef {
      id: "code".to_string(),
    },
    title: Some("AGENTS.md - Visual Studio Code".to_string()),
    app_name: Some("code".to_string()),
    app_bundle_id: None,
    process_id: None,
    frame: Rect::new(0.0, 32.0, 2560.0, 1408.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: false,
    is_visible: true,
  };
  let selector = WindowSelector {
    app: Some(AppSelector {
      frontmost: true,
      ..AppSelector::default()
    }),
    main_visible: true,
    ..WindowSelector::default()
  };

  let resolved = resolve_from_windows(&[shell, application.clone()], &selector).expect("application window resolves");

  assert_eq!(resolved, application);
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
