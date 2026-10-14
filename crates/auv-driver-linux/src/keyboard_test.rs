use crate::LinuxDriver;
use auv_driver_common::{Driver, InputPolicy, InputTarget, KeyboardInput, PressKeysOptions};

#[test]
fn invalid_tail_is_rejected_before_delivering_the_prefix() {
  // ROOT CAUSE: the invoke batch contract was not connected to the Linux driver.
  // Validate a malformed tail before any backend or desktop access.
  let session = LinuxDriver::new().open_local().unwrap();
  let action = |keys: &[&str]| KeyboardInput::PressKeys {
    options: PressKeysOptions {
      keys: keys.iter().map(|key| (*key).into()).collect(),
      ..Default::default()
    },
    policy: InputPolicy::ForegroundPreferred,
  };
  let error = session.input().input_keyboard(&InputTarget::Foreground, vec![action(&["a"]), action(&["invalid-key"])], false).unwrap_err();
  assert_eq!(error.progress.action_index, 1);
  assert!(error.progress.completed.is_empty());
  assert_eq!(error.progress.completed_presses, 0);
  assert!(matches!(error.cause, auv_driver_common::DriverError::InvalidInput { .. }));
}

#[test]
fn dry_run_accepts_literal_plus_and_repeated_chords_without_a_desktop() {
  let session = LinuxDriver::new().open_local().unwrap();
  let result = session
    .input()
    .input_keyboard(
      &InputTarget::Foreground,
      vec![KeyboardInput::PressKeys {
        options: PressKeysOptions {
          keys: vec!["ctrl".into(), "+".into()],
          count: 2,
          interval: std::time::Duration::from_millis(10),
          ..Default::default()
        },
        policy: InputPolicy::ForegroundPreferred,
      }],
      true,
    )
    .unwrap();
  assert!(result.is_none());
}

// ROOT CAUSE:
//
// If a caller named the window or application it was typing into, Linux
// refused the keys as "targeted keyboard input is not supported", even when
// that target had keyboard focus. Wayland cannot address keys to a window, so
// the fix serves a target only while it is the AT-SPI active window.
#[test]
fn a_keyboard_target_is_served_only_while_it_has_focus() {
  use auv_driver_common::{CoordinateSpace, Rect, Window, WindowRef};
  let window = |id: &str, app_id: Option<&str>, app_name: &str| Window {
    reference: WindowRef { id: id.to_string() },
    title: Some(app_name.to_string()),
    app_name: Some(app_name.to_string()),
    app_bundle_id: app_id.map(str::to_string),
    process_id: None,
    frame: Rect::new(0.0, 0.0, 10.0, 10.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
  };
  let editor = window("atspi::editor", Some("org.gnome.TextEditor"), "gnome-text-editor");
  let terminal = window("atspi::terminal", None, "org.gnome.Terminal");
  let app = |id: &str| InputTarget::Application {
    bundle_id: id.to_string(),
  };

  assert!(crate::keyboard::target_is_focused(&app("org.gnome.TextEditor"), Some(&editor)));
  assert!(crate::keyboard::target_is_focused(&app("org.gnome.Terminal"), Some(&terminal)), "falls back to the app name");
  assert!(!crate::keyboard::target_is_focused(&app("org.gnome.TextEditor"), Some(&terminal)));
  assert!(crate::keyboard::target_is_focused(&InputTarget::Window(editor.clone()), Some(&editor)));
  assert!(!crate::keyboard::target_is_focused(&InputTarget::Window(editor.clone()), Some(&terminal)));
  assert!(!crate::keyboard::target_is_focused(&app("org.gnome.TextEditor"), None));
}
