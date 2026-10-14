use super::*;
use enigo::{InputError, InputResult};

#[derive(Default)]
struct Recorder {
  events: Vec<String>,
  fail_at: Option<usize>,
}
impl Recorder {
  fn record(&mut self, event: String) -> InputResult<()> {
    self.events.push(event);
    if self.fail_at == Some(self.events.len()) {
      Err(InputError::Simulate("injected failure"))
    } else {
      Ok(())
    }
  }
}
impl Keyboard for Recorder {
  fn fast_text(&mut self, text: &str) -> InputResult<Option<()>> {
    self.record(format!("text:{text}")).map(|()| Some(()))
  }
  fn key(&mut self, key: Key, direction: Direction) -> InputResult<()> {
    self.record(format!("{key:?}:{direction:?}"))
  }
  fn raw(&mut self, _: u16, _: Direction) -> InputResult<()> {
    unreachable!()
  }
}
impl Mouse for Recorder {
  fn button(&mut self, button: Button, direction: Direction) -> InputResult<()> {
    self.record(format!("{button:?}:{direction:?}"))
  }
  fn move_mouse(&mut self, x: i32, y: i32, _: Coordinate) -> InputResult<()> {
    self.record(format!("move:{x},{y}"))
  }
  fn scroll(&mut self, length: i32, axis: Axis) -> InputResult<()> {
    self.record(format!("scroll:{length}:{axis:?}"))
  }
  fn main_display(&self) -> InputResult<(i32, i32)> {
    Ok((800, 600))
  }
  fn location(&self) -> InputResult<(i32, i32)> {
    Ok((0, 0))
  }
}

#[test]
fn failed_press_still_releases_all_attempted_keys_in_reverse_order() {
  // ROOT CAUSE: A failed transport call may already have delivered a key-down.
  // Cleanup must include that key and all previously held modifiers.
  let mut input = Recorder {
    fail_at: Some(2),
    ..Default::default()
  };
  assert!(
    with_keys(&mut input, &[Key::Control, Key::Shift, Key::Unicode('a')], |_| -> DriverResult<()> { panic!("action must not run") })
      .is_err()
  );
  assert_eq!(
    input.events,
    [
      "Control:Press",
      "Shift:Press",
      "Shift:Release",
      "Control:Release"
    ]
  );
}

#[test]
fn cleanup_continues_when_one_release_fails() {
  let mut input = Recorder {
    fail_at: Some(3),
    ..Default::default()
  };
  assert!(with_keys(&mut input, &[Key::Control, Key::Shift], |_| Ok(())).is_err());
  assert_eq!(
    input.events,
    [
      "Control:Press",
      "Shift:Press",
      "Shift:Release",
      "Control:Release"
    ]
  );
}

#[test]
fn failed_drag_releases_the_mouse_button() {
  let mut input = Recorder {
    fail_at: Some(2),
    ..Default::default()
  };
  assert!(with_button(&mut input, Button::Left, |input| input.move_mouse(20, 30, Coordinate::Abs).map_err(backend)).is_err());
  assert_eq!(input.events, ["Left:Press", "move:20,30", "Left:Release"]);
}

#[test]
fn invalid_keys_and_repetition_are_rejected_as_a_whole() {
  let options = PressKeysOptions {
    keys: vec!["ctrl".into(), "unknown".into()],
    ..Default::default()
  };
  assert!(parse_keys(&options).is_err());
  assert!(
    parse_keys(&PressKeysOptions {
      count: 2,
      keys: vec!["a".into()],
      ..Default::default()
    })
    .is_err()
  );
  let options = KeyPressOptions {
    key: "ctrl+control+shift+p".into(),
    ..Default::default()
  };
  assert_eq!(parse_keys(&options.into()).unwrap(), [Key::Control, Key::Shift, Key::Unicode('p')]);
  assert_eq!(
    parse_keys(
      &KeyPressOptions {
        key: "+".into(),
        ..Default::default()
      }
      .into()
    )
    .unwrap(),
    [Key::Unicode('+')]
  );
}

#[test]
fn function_keys_and_navigation_aliases_keep_physical_identity() {
  assert_eq!(parse_key("f13").unwrap(), Key::F13);
  assert_eq!(parse_key("F24").unwrap(), Key::F24);
  assert_eq!(parse_key("pgup").unwrap(), Key::PageUp);
  assert_eq!(parse_key("pgdn").unwrap(), Key::PageDown);
  assert_eq!(
    parse_keys(&PressKeysOptions {
      keys: vec![
        "pageup".into(),
        "pgup".into(),
        "pagedown".into(),
        "pgdn".into()
      ],
      ..Default::default()
    })
    .unwrap(),
    [Key::PageUp, Key::PageDown]
  );
  assert!(parse_key("f25").is_err());
}

#[test]
fn special_keys_map_to_distinct_x11_keysyms() {
  for (name, expected) in [
    ("prtsc", Key::PrintScr),
    ("printscreen", Key::PrintScr),
    ("capslock", Key::CapsLock),
    ("numlock", Key::Numlock),
    ("scrolllock", Key::ScrollLock),
    ("insert", Key::Insert),
    ("ins", Key::Insert),
    ("pause", Key::Pause),
    ("break", Key::Break),
  ] {
    assert_eq!(parse_key(name).unwrap(), expected, "{name}");
  }
  assert!(parse_key("fn").is_err());
}

// ROOT CAUSE:
//
// If a model pressed `ctrl+comma` (GIMP preferences) or `arrowright`, the
// X11 driver failed with `unknown key "comma"` because its table had no
// spelled-out punctuation and no separator or camel-case aliases. OSWorld
// episodes lost a step on each.
//
// The fix falls back to the shared logical key names, sending printable
// symbols as characters so Enigo chooses the keycode and Shift level.
#[test]
fn spelled_out_punctuation_and_arrow_aliases_resolve_through_shared_names() {
  assert_eq!(parse_key("comma").unwrap(), Key::Unicode(','));
  assert_eq!(parse_key("plus").unwrap(), Key::Unicode('+'));
  assert_eq!(parse_key("bracket_left").unwrap(), Key::Unicode('['));
  assert_eq!(parse_key("arrowright").unwrap(), Key::Other(xkeysym::Keysym::Right.raw()));
  assert_eq!(parse_key("Page_Down").unwrap(), Key::Other(xkeysym::Keysym::Page_Down.raw()));
  let options = KeyPressOptions {
    key: "ctrl+comma".into(),
    ..Default::default()
  };
  assert_eq!(parse_keys(&options.into()).unwrap(), [Key::Control, Key::Unicode(',')]);
  assert!(parse_key("notakey").is_err());
}

#[test]
fn rejects_unrepresentable_coordinates_and_invalid_text_before_mutation() {
  for point in [
    Point::new(f64::NAN, 0.0),
    Point::new(f64::INFINITY, 1.0),
    Point::new(32768.0, 1.0),
  ] {
    assert!(root_pixel(point).is_err());
  }
  assert!(
    validate_text(
      "bad\0text",
      TypeTextOptions {
        replace_existing: true,
        ..Default::default()
      }
    )
    .is_err()
  );
  assert!(
    validate_text(
      "hello",
      TypeTextOptions {
        policy: InputPolicy::BackgroundOnly,
        ..Default::default()
      }
    )
    .is_err()
  );
}

#[test]
fn pointer_input_rounds_subpixel_points_to_x11_root_pixels() {
  // ROOT CAUSE:
  //
  // If a drag path crossed more than one pixel, shared path interpolation
  // produced fractional intermediate points even when both endpoints were
  // integral. Before the fix, X11 rejected the first fractional sample and no
  // multi-pixel sampled drag could complete. Motion now projects samples onto the
  // nearest X11 root pixel while direct click/scroll validation stays strict.
  assert_eq!(root_pixel(Point::new(100.49, 200.5)).unwrap(), (100, 201));
  assert_eq!(root_pixel(Point::new(-10.5, -20.49)).unwrap(), (-11, -20));
  assert!(root_pixel(Point::new(f64::NAN, 0.0)).is_err());
  assert!(root_pixel(Point::new(32768.0, 0.0)).is_err());
}

// ROOT CAUSE:
//
// If `screen.clickText` resolved an OCR box with an odd width or height, its
// projected click point was a half pixel (for example 152.5), and the X11
// driver rejected direct clicks that were not whole pixels: "X11 input
// requires finite integral coordinates". 14 clickText calls failed this way
// in the 2026-10-12 OSWorld chrome run.
//
// Before the fix, clicks and scroll anchors kept strict integral validation.
// The fix rounds every pointer path to the nearest root pixel, like motion.
#[test]
fn click_text_half_pixel_centers_reach_the_nearest_root_pixel() {
  assert_eq!(root_pixel(Point::new(152.5, 80.0)).unwrap(), (153, 80));
  assert_eq!(root_pixel(Point::new(1919.4, 1079.6)).unwrap(), (1919, 1080));
}

#[test]
fn logical_scroll_pixels_accumulate_to_x11_wheel_detents() {
  // ROOT CAUSE:
  //
  // Before alignment with the shared Scroll contract, X11 treated deltas as
  // whole detents while the other Linux routes treated them as logical pixels.
  // This conversion preserves the common sign and carries a sub-notch amount
  // across calls in the same session.
  assert_eq!(wheel_notches((0.0, 0.0), Scroll::new(120.0, -240.0)).unwrap(), ((1, -2), (0.0, 0.0)));
  let (notches, remainder) = wheel_notches((0.0, 0.0), Scroll::new(0.0, 60.0)).unwrap();
  assert_eq!(notches, (0, 0));
  assert_eq!(wheel_notches(remainder, Scroll::new(0.0, 60.0)).unwrap(), ((0, 1), (0.0, 0.0)));
  assert!(wheel_notches((0.0, 0.0), Scroll::new(f64::NAN, 1.0)).is_err());
  assert!(wheel_notches((0.0, 0.0), Scroll::new(0.0, 0.0)).is_err());
}

#[test]
fn delivery_does_not_claim_semantic_success_or_clipboard_changes() {
  let result = delivered(true);
  assert!(!result.verified);
  assert_eq!(result.mouse_disturbance, DisturbanceLevel::Foreground);
  assert_eq!(result.clipboard_disturbance, DisturbanceLevel::None);
  assert!(result.validate().is_ok());
}

// ROOT CAUSE:
//
// If a keyboard batch contained a paste, X11 rejected the whole batch because
// the driver had no CLIPBOARD owner. Before the fix, `input.keyboard` batches
// with `pasteText` failed on X11 while Wayland and macOS delivered them.
// The fix plans the paste so it runs in order with the other actions.
#[test]
fn keyboard_batch_plans_paste_in_order() {
  let inputs = vec![
    KeyboardInput::TypeText {
      text: "safe".into(),
      options: TypeTextOptions {
        policy: InputPolicy::ForegroundPreferred,
        ..Default::default()
      },
    },
    KeyboardInput::PasteText {
      options: PasteTextOptions {
        text: "héllo 世界".into(),
        ..Default::default()
      },
      policy: InputPolicy::ForegroundPreferred,
    },
  ];

  let plans = validate_keyboard_batch(&InputTarget::Foreground, &inputs).unwrap();
  assert!(matches!(&plans[0], KeyboardPlan::Type { text, .. } if text == "safe"));
  assert!(matches!(&plans[1], KeyboardPlan::Paste { options } if options.text == "héllo 世界"));
}

#[test]
fn keyboard_batch_rejects_background_paste_before_delivery() {
  let inputs = vec![KeyboardInput::PasteText {
    options: PasteTextOptions::default(),
    policy: InputPolicy::BackgroundOnly,
  }];
  let error = validate_keyboard_batch(&InputTarget::Foreground, &inputs).expect_err("background paste must be rejected");
  assert_eq!(error.progress.action_index, 0);
  assert!(matches!(error.cause, DriverError::InvalidInput { .. }));
}

#[test]
fn exact_restore_reports_temporary_clipboard_disturbance() {
  let result = paste_result(Ok(()), Ok(Restore::Exact)).unwrap();
  assert_eq!(result.selected_path, InputDeliveryPath::ClipboardPaste);
  assert_eq!(result.clipboard_disturbance, DisturbanceLevel::Temporary);
  assert_eq!(result.focus_disturbance, DisturbanceLevel::Unknown);
  assert_eq!(result.mouse_disturbance, DisturbanceLevel::None);
  assert!(!result.verified);
  assert_eq!(result.attempts.len(), 1);
  assert!(result.attempts[0].succeeded);
  assert_eq!(result.attempts[0].message, None);
  assert!(result.validate().is_ok());
}

#[test]
fn partial_or_skipped_restore_reports_unknown_clipboard_disturbance() {
  for restore in [Restore::TextOnly, Restore::Skipped] {
    let result = paste_result(Ok(()), Ok(restore)).unwrap();
    assert_eq!(result.clipboard_disturbance, DisturbanceLevel::Unknown, "{restore:?}");
    assert!(result.attempts[0].succeeded);
    assert!(result.attempts[0].message.is_some(), "{restore:?} must explain why the clipboard differs");
  }
}

#[test]
fn restore_failure_is_an_error_even_after_delivery() {
  let error = paste_result(Ok(()), Err(backend("owner gone"))).unwrap_err();
  assert!(error.to_string().contains("pasted text but failed to restore clipboard"), "{error}");
  let error = paste_result(Err(backend("no paste consumer requested CLIPBOARD")), Err(backend("owner gone"))).unwrap_err();
  assert!(error.to_string().contains("no paste consumer"), "{error}");
  assert!(error.to_string().contains("additionally failed to restore clipboard"), "{error}");
  let error = paste_result(Err(backend("no paste consumer requested CLIPBOARD")), Ok(Restore::Exact)).unwrap_err();
  assert!(error.to_string().contains("no paste consumer"), "{error}");
}

#[test]
fn targeted_keyboard_batch_accepts_only_foreground_preferred_policy() {
  let press = |policy| KeyboardInput::PressKeys {
    options: PressKeysOptions {
      keys: vec!["f2".into()],
      ..Default::default()
    },
    policy,
  };
  let application = InputTarget::Application {
    id: "fixture".into(),
  };
  // The X11 exception activates the recipient and delivers foreground XTEST,
  // so a targeted foreground-preferred batch validates.
  assert_eq!(validate_keyboard_batch(&application, &[press(InputPolicy::ForegroundPreferred)]).unwrap().len(), 1);

  for policy in [
    InputPolicy::BackgroundPreferred,
    InputPolicy::BackgroundOnly,
  ] {
    let error = validate_keyboard_batch(&application, &[press(InputPolicy::ForegroundPreferred), press(policy)])
      .expect_err("background policy has no X11 route");
    assert_eq!(error.progress.action_index, 1);
    assert!(matches!(
      error.cause,
      DriverError::Unsupported {
        operation: "X11 background keyboard input"
      }
    ));
  }

  let error = validate_keyboard_batch(&InputTarget::Foreground, &[press(InputPolicy::BackgroundPreferred)])
    .expect_err("untargeted background policy must be rejected");
  assert!(matches!(error.cause, DriverError::InvalidInput { .. }));
}

#[test]
fn targeted_keyboard_result_reports_the_foreground_fallback() {
  let mut action = delivered(false);
  foreground_fallback(
    &mut action,
    &crate::window::Recipient {
      xid: 0x1a00007,
      process_id: Some(42),
    },
  );
  assert_eq!(action.selected_path, InputDeliveryPath::ForegroundSystemEvents);
  assert_eq!(action.attempts[0].path, InputDeliveryPath::WindowTargetedKeyboard);
  assert!(!action.attempts[0].succeeded);
  assert!(action.fallback_reason().is_some_and(|reason| reason.contains("x11:0x1a00007 (pid 42)")));
  assert_eq!(action.focus_disturbance, DisturbanceLevel::Foreground);
  assert!(!action.verified);
  assert!(action.validate().is_ok());
}

// ROOT CAUSE:
//
// If the focused window was GNOME Terminal, `input.pasteText` sent Ctrl+V,
// which bash reads as quoted-insert: the next Return arrived as a literal
// `^M` and pasted commands never ran. OSWorld terminal detours all failed.
//
// The fix sends Ctrl+Shift+V when the active window is a terminal emulator.
#[test]
fn terminal_emulators_are_recognized_by_application_id() {
  let ids = |ids: &[&str]| ids.iter().map(|id| (*id).to_string()).collect::<Vec<_>>();
  assert_eq!(terminal_application(&ids(&["gnome-terminal-server", "Gnome-terminal"])).as_deref(), Some("gnome-terminal-server"));
  assert_eq!(terminal_application(&ids(&["org.gnome.Terminal"])).as_deref(), Some("org.gnome.Terminal"));
  assert_eq!(terminal_application(&ids(&["kitty"])).as_deref(), Some("kitty"));
  assert_eq!(terminal_application(&ids(&["libreoffice-calc", "libreoffice"])), None);
  // xterm binds neither shortcut; it keeps Ctrl+V and fails honestly.
  assert_eq!(terminal_application(&ids(&["xterm", "XTerm"])), None);
  assert_eq!(terminal_application(&[]), None);
}
