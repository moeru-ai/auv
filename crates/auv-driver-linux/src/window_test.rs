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
    origin: None,
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

fn capture_target() -> Window {
  Window {
    reference: WindowRef {
      id: "atspi::1.42/org/a11y/atspi/accessible/1".into(),
    },
    title: Some("Editor".into()),
    app_name: None,
    app_bundle_id: None,
    process_id: None,
    frame: Rect::new(3.0, 4.0, 2.0, 2.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
  }
}

#[test]
fn capture_target_rejects_another_reference_with_matching_title_and_frame() {
  // ROOT CAUSE:
  //
  // If the requested reference was absent, capture used the caller's frame
  // without resolving that reference. Another window's pixels could receive
  // the missing reference. The fix requires an exact live reference match.
  let target = capture_target();
  let mut other = target.clone();
  other.reference.id = "atspi::1.43/org/a11y/atspi/accessible/1".into();

  let error = resolve_capture_target(&[other], &target.reference, None).unwrap_err();

  assert!(matches!(error, auv_driver_common::DriverError::NotFound { .. }));
  assert!(error.to_string().contains(&target.reference.id));
}

#[test]
fn capture_target_rejects_invisible_window() {
  let mut target = capture_target();
  target.is_visible = false;

  let error = resolve_capture_target(&[target.clone()], &target.reference, None).unwrap_err();

  assert!(matches!(error, auv_driver_common::DriverError::NotFound { .. }));
}

#[test]
fn capture_target_uses_current_frame_for_crop_and_bounds() {
  // A caller's Window is a snapshot. Capture must refresh its frame by reference.
  let stale = capture_target();
  let mut current = stale.clone();
  current.frame = Rect::new(6.0, 7.0, 3.0, 2.0);
  let windows = [current.clone()];
  let resolved = resolve_capture_target(&windows, &stale.reference, None).unwrap();
  let mut image = image::RgbaImage::new(10, 10);
  image.put_pixel(6, 7, image::Rgba([1, 2, 3, 255]));
  let display = Capture {
    origin: None,
    image,
    bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
    scale_factor: 1.0,
    backend: "test".into(),
    fallback_reason: None,
  };

  let crop = crop_capture_to_window(&display, resolved.frame).unwrap();

  assert_eq!(resolved, &current);
  assert_eq!(crop.dimensions(), (3, 2));
  assert_eq!(*crop.get_pixel(0, 0), image::Rgba([1, 2, 3, 255]));
}

#[test]
fn capture_target_rejects_current_origin_shared_with_another_window() {
  let stale = capture_target();
  let mut current = stale.clone();
  current.frame.origin.x = 6.0;
  let mut other = current.clone();
  other.reference.id = "atspi::1.43/org/a11y/atspi/accessible/1".into();

  let error = resolve_capture_target(&[current, other], &stale.reference, None).unwrap_err();

  assert!(matches!(error, auv_driver_common::DriverError::InvalidInput { .. }));
  assert!(error.to_string().contains("shares AT-SPI origin"));
}

#[test]
fn capture_target_rejects_window_closed_during_capture() {
  let target = capture_target();

  let error = resolve_capture_target(&[], &target.reference, Some(target.frame)).unwrap_err();

  assert!(matches!(error, auv_driver_common::DriverError::NotFound { .. }));
}

#[test]
fn capture_target_rejects_window_moved_during_capture() {
  let before = capture_target();
  let mut after = before.clone();
  after.frame.origin.x += 1.0;

  let error = resolve_capture_target(&[after], &before.reference, Some(before.frame)).unwrap_err();

  assert!(matches!(error, auv_driver_common::DriverError::InvalidInput { .. }));
  assert!(error.to_string().contains("changed frame during display capture"));
}

#[test]
fn capture_target_rejects_window_resized_during_capture() {
  let before = capture_target();
  let mut after = before.clone();
  after.frame.size.width += 1.0;

  let error = resolve_capture_target(&[after], &before.reference, Some(before.frame)).unwrap_err();

  assert!(matches!(error, auv_driver_common::DriverError::InvalidInput { .. }));
  assert!(error.to_string().contains("changed frame during display capture"));
}

#[test]
fn capture_target_accepts_unchanged_window_after_capture() {
  let target = capture_target();
  let windows = [target.clone()];

  let resolved = resolve_capture_target(&windows, &target.reference, Some(target.frame)).unwrap();

  assert_eq!(resolved, &target);
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires a live Wayland/AT-SPI desktop; regression must fail before portal capture"]
fn window_capture_rejects_nonexistent_reference_on_live_desktop() {
  use auv_driver_common::{Driver, DriverError};

  // ROOT CAUSE:
  //
  // If a WindowRef did not exist, capture only decoded its string and checked
  // other windows for a shared origin. It never looked up the requested window.
  // Before the fix, this reference received a successful 13x13 desktop crop.
  // The fix returns NotFound before opening a capture portal.
  let window = Window {
    reference: WindowRef {
      id: "atspi::1.999999/org/a11y/atspi/accessible/999999".into(),
    },
    title: None,
    app_name: None,
    app_bundle_id: None,
    process_id: None,
    frame: Rect::new(10.0, 10.0, 10.0, 10.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: false,
    is_visible: true,
  };
  let session = crate::LinuxDriver::new().open_local().unwrap();
  let result = session.window().capture(&window);
  let error = result.map(|capture| capture.image.dimensions()).expect_err("a nonexistent WindowRef must not receive a window-bound capture");
  assert!(matches!(error, DriverError::NotFound { .. }), "{error}");
  assert!(error.to_string().contains(&window.reference.id), "{error}");
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires a stable live Wayland/AT-SPI application window and portal consent"]
fn window_capture_refreshes_stale_frame_on_live_desktop() {
  use auv_driver_common::Driver;

  let session = crate::LinuxDriver::new().open_local().unwrap();
  let current = session.window().resolve(SelectWindow::main_visible()).unwrap();
  let mut stale = current.clone();
  stale.frame = Rect::new(10.0, 10.0, 10.0, 10.0);
  assert_ne!(stale.frame, current.frame);

  let capture = session.window().capture(&stale).unwrap();

  assert_eq!(capture.bounds, current.frame);
  assert_eq!(
    capture.origin,
    Some(auv_driver_common::Position::in_window(&current.reference, auv_driver_common::WindowPoint::new(0.0, 0.0)))
  );
  assert_eq!(
    capture.image.dimensions(),
    ((current.frame.size.width * capture.scale_factor).round() as u32, (current.frame.size.height * capture.scale_factor).round() as u32,)
  );
  if let Some(path) = std::env::var_os("AUV_WINDOW_TEST_PNG") {
    capture.image.save(path).unwrap();
  }
  eprintln!(
    "reference={} bounds={:?} pixels={:?} backend={}",
    current.reference.id,
    capture.bounds,
    capture.image.dimensions(),
    capture.backend
  );
}
