use super::*;

#[test]
fn reserved_result_uses_shared_input_schema() {
  let result = reserved_input_result("not wired yet");

  assert_eq!(result.selected_path, InputDeliveryPath::Unsupported);
  assert_eq!(result.attempts.len(), 1);
}

#[test]
fn paste_text_returns_typed_input_action_result() {
  let _: fn(&Arc<Mutex<LinuxDriverSessionState>>, PasteTextOptions) -> DriverResult<InputActionResult> = paste_text;
}

#[test]
fn navigation_keys_use_xkb_keysyms() {
  assert_eq!(keysym::named_or_char("left").unwrap(), keysym::LEFT);
  assert_eq!(keysym::named_or_char("ArrowDown").unwrap(), keysym::DOWN);
  assert_eq!(keysym::named_or_char("page_up").unwrap(), keysym::PAGE_UP);
  assert_eq!(keysym::named_or_char("end").unwrap(), keysym::END);
}

#[test]
fn parses_function_key_shortcuts_for_desktop_commands() {
  // ROOT CAUSE:
  //
  // If a remote Linux workflow needed GNOME's Alt+F2 command launcher, the
  // input parser rejected F2 even though the portal accepts its standard
  // keysym. This made recovery impossible when SSH was unavailable.
  assert_eq!(
    parse_key_chord("alt+f2").expect("Alt+F2"),
    KeyChord {
      modifiers: vec![keysym::ALT_L],
      key: 0xffbf,
    }
  );
  assert!(parse_key_chord("f13").is_err());
}

#[test]
fn parses_modifier_only_keys_for_desktop_shell_commands() {
  // ROOT CAUSE:
  //
  // If a remote Linux workflow needed GNOME's Super-key overview, the input
  // parser accepted Super as a shortcut modifier but rejected it as a
  // standalone key. This left minimized windows unreachable when SSH and
  // app-specific activation were unavailable.
  assert_eq!(
    parse_key_chord("super").expect("Super"),
    KeyChord {
      modifiers: Vec::new(),
      key: keysym::SUPER_L,
    }
  );
}

#[test]
fn click_modifiers_map_to_standard_platform_keys() {
  assert_eq!(
    click_modifier_keysyms(auv_driver_common::ClickModifiers {
      shift: true,
      control: true,
      alt: true,
      meta: true
    }),
    [0xffe1, 0xffe3, 0xffe9, 0xffeb]
  );
  assert!(click_modifier_keysyms(Default::default()).is_empty());
}

// ROOT CAUSE: direct type_text used to resolve characters after emitting the
// replace shortcut and preceding characters. Reject the whole request before
// contacting a backend, using the same prepared plan as batch delivery.
#[test]
fn direct_type_text_rejects_invalid_tail_before_opening_backend() {
  use auv_driver_common::Driver;
  let session = crate::LinuxDriver::new().open_local().unwrap();
  let error = session
    .input()
    .type_text(
      "prefix漢",
      TypeTextOptions {
        replace_existing: true,
        ..Default::default()
      },
    )
    .unwrap_err();
  assert!(matches!(error, DriverError::InvalidInput { .. }));
  assert!(session.state.lock().unwrap().input_session.is_none());
}

#[test]
fn prepared_text_retains_replace_submit_and_per_character_timing() {
  let delay = Duration::from_millis(7);
  let plan = KeyboardPlan::type_text(
    "a!",
    TypeTextOptions {
      replace_existing: true,
      submit: TextSubmit::Return,
      inter_char_delay: delay,
      ..Default::default()
    },
  )
  .unwrap();
  let events = plan.steps.iter().map(|(chord, delay)| (chord.modifiers.as_slice(), chord.key, *delay)).collect::<Vec<_>>();
  assert_eq!(
    events,
    vec![
      (&[keysym::CONTROL_L][..], 'a' as i32, Duration::ZERO),
      (&[][..], keysym::BACKSPACE, Duration::ZERO),
      (&[][..], 'a' as i32, delay),
      (&[][..], '!' as i32, delay),
      (&[][..], keysym::RETURN, Duration::ZERO),
    ]
  );
  let paste = KeyboardPlan::paste_text(&PasteTextOptions {
    replace_existing: true,
    submit: TextSubmit::Return,
    ..Default::default()
  });
  assert_eq!(
    paste.steps.iter().map(|(chord, _)| (chord.modifiers.as_slice(), chord.key)).collect::<Vec<_>>(),
    vec![
      (&[keysym::CONTROL_L][..], 'a' as i32),
      (&[keysym::CONTROL_L][..], 'v' as i32),
      (&[][..], keysym::RETURN),
    ]
  );
}
