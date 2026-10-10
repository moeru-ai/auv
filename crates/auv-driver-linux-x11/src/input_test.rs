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

#[test]
fn rejects_lossy_coordinates_and_invalid_text_before_mutation() {
  for point in [
    Point::new(f64::NAN, 0.0),
    Point::new(0.5, 1.0),
    Point::new(32768.0, 1.0),
  ] {
    assert!(coordinates(point).is_err());
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
fn sampled_motion_rounds_subpixel_path_points_to_x11_root_pixels() {
  // ROOT CAUSE:
  //
  // If a drag path crossed more than one pixel, shared path interpolation
  // produced fractional intermediate points even when both endpoints were
  // integral. Before the fix, X11 rejected the first fractional sample and no
  // multi-pixel sampled drag could complete. Motion now projects samples onto the
  // nearest X11 root pixel while direct click/scroll validation stays strict.
  assert_eq!(motion_coordinates(Point::new(100.49, 200.5)).unwrap(), (100, 201));
  assert_eq!(motion_coordinates(Point::new(-10.5, -20.49)).unwrap(), (-11, -20));
  assert!(motion_coordinates(Point::new(f64::NAN, 0.0)).is_err());
  assert!(motion_coordinates(Point::new(32768.0, 0.0)).is_err());
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

#[test]
fn keyboard_batch_validation_rejects_unsupported_actions_before_delivery() {
  let inputs = vec![
    KeyboardInput::TypeText {
      text: "safe".into(),
      options: TypeTextOptions {
        policy: InputPolicy::ForegroundPreferred,
        ..Default::default()
      },
    },
    KeyboardInput::PasteText {
      options: auv_driver_common::PasteTextOptions {
        text: "not delivered".into(),
        ..Default::default()
      },
      policy: InputPolicy::ForegroundPreferred,
    },
  ];

  let error = validate_keyboard_batch(&InputTarget::Foreground, &inputs).expect_err("clipboard action must reject the whole batch");
  assert_eq!(error.progress.action_index, 1);
  assert!(error.progress.completed.is_empty());
  assert!(matches!(
    error.cause,
    DriverError::Unsupported {
      operation: "X11 clipboard paste"
    }
  ));
}

#[test]
fn keyboard_batch_requires_foreground_target_and_policy() {
  let input = KeyboardInput::PressKeys {
    options: PressKeysOptions {
      keys: vec!["f2".into()],
      ..Default::default()
    },
    policy: InputPolicy::ForegroundPreferred,
  };
  let error = validate_keyboard_batch(
    &InputTarget::Application {
      bundle_id: "fixture".into(),
    },
    std::slice::from_ref(&input),
  )
  .expect_err("targeted input must be rejected");
  assert!(matches!(error.cause, DriverError::Unsupported { .. }));

  let background = KeyboardInput::PressKeys {
    options: PressKeysOptions {
      keys: vec!["f2".into()],
      ..Default::default()
    },
    policy: InputPolicy::BackgroundPreferred,
  };
  let error = validate_keyboard_batch(&InputTarget::Foreground, &[background]).expect_err("background policy must be rejected");
  assert!(matches!(error.cause, DriverError::InvalidInput { .. }));
}
