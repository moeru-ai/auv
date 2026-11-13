use auv_driver_common::geometry::{CoordinateSpace, Rect};
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
    application_id: None,
    process_id: Some(42),
    frame: Rect::new(0.0, 0.0, 500.0, 400.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
    state: Default::default(),
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
    application_id: None,
    process_id: Some(42),
    frame: Rect::new(0.0, 0.0, 500.0, 400.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
    state: Default::default(),
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
    application_id: None,
    process_id: Some(42),
    frame: Rect::new(0.0, 0.0, 500.0, 400.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
    state: Default::default(),
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
    application_id: Some("org.gnome.Shell".to_string()),
    process_id: None,
    frame: Rect::new(0.0, 55.0, 100.0, 56.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
    state: Default::default(),
  };
  let application = Window {
    reference: WindowRef {
      id: "code".to_string(),
    },
    title: Some("AGENTS.md - Visual Studio Code".to_string()),
    app_name: Some("code".to_string()),
    application_id: None,
    process_id: None,
    frame: Rect::new(0.0, 32.0, 2560.0, 1408.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: false,
    is_visible: true,
    state: Default::default(),
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
    application_id: None,
    process_id: None,
    frame: Rect::new(10.0, 10.0, 10.0, 10.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: false,
    is_visible: true,
    state: Default::default(),
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

// ROOT CAUSE:
//
// If an agent guessed an application id that no visible window carried
// (`com.google.Chrome` on Linux), resolution failed with a bare "main visible
// window was not found", and the agent spent a step on window.list.
//
// The fix keeps strict id matching and names the visible applications.
#[test]
fn an_unknown_application_id_lists_the_visible_applications() {
  let editor = Window {
    reference: WindowRef {
      id: "1".to_string(),
    },
    title: Some("notes.txt - Text Editor".to_string()),
    app_name: Some("gnome-text-editor".to_string()),
    application_id: Some("org.gnome.TextEditor".to_string()),
    process_id: Some(42),
    frame: Rect::new(0.0, 0.0, 500.0, 400.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
    state: Default::default(),
  };
  let selector = SelectWindow::main_visible().owned_by(auv_driver_common::App::id("com.google.Chrome"));
  let error = resolve_from_windows(std::slice::from_ref(&editor), &selector).unwrap_err();
  assert_eq!(error.to_string(), "application \"com.google.Chrome\" (visible applications: org.gnome.TextEditor) was not found");
  // The executable name is not an application id on Linux.
  let by_executable = SelectWindow::main_visible().owned_by(auv_driver_common::App::id("gnome-text-editor"));
  assert!(resolve_from_windows(&[editor], &by_executable).is_err());
}

// ROOT CAUSE:
//
// If a modal dialog was focused, Linux marked the first listed window as main
// instead of the dialog, so main-window resolution picked the larger frame
// behind it (LibreOffice Writer behind its Hyperlink dialog).
//
// With the active window marked main, the dialog wins the ranking.
#[test]
fn the_main_window_outranks_a_larger_frame() {
  let window = |id: &str, title: &str, is_main: bool, frame: Rect| Window {
    reference: WindowRef { id: id.to_string() },
    title: Some(title.to_string()),
    app_name: Some("soffice".to_string()),
    application_id: None,
    process_id: None,
    frame,
    coordinate_space: CoordinateSpace::Screen,
    is_main,
    is_visible: true,
    state: Default::default(),
  };
  let windows = [
    window("1", "Untitled 1 — LibreOffice Writer", false, Rect::new(0.0, 32.0, 2560.0, 1408.0)),
    window("2", "Hyperlink", true, Rect::new(0.0, 0.0, 600.0, 400.0)),
  ];
  assert_eq!(resolve_from_windows(&windows, &SelectWindow::main_visible()).unwrap().reference.id, "2");
}
