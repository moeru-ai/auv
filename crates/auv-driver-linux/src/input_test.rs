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
