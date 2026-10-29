use super::*;

#[test]
fn window_keyboard_dry_run_does_not_query_or_activate_the_os() {
  use auv_driver_common::Driver;
  use auv_driver_common::geometry::{CoordinateSpace, Rect, Size};
  use auv_driver_common::window::{Window, WindowRef};

  // These are request data, not a live native window. Dry run must finish before OS admission.
  let target = InputTarget::Window(Window {
    reference: WindowRef { id: "1".into() },
    title: None, app_name: None, application_id: None, process_id: Some(42),
    frame: Rect { origin: Point::new(0.0, 0.0), size: Size::new(100.0, 100.0) },
    coordinate_space: CoordinateSpace::Screen, is_main: true, is_visible: true,
  });
  let session = crate::WindowsDriver::new().open_local().unwrap();
  let result = session.input().input_keyboard(&target, vec![KeyboardInput::TypeText {
    text: "planned only".into(),
    options: TypeTextOptions { policy: InputPolicy::ForegroundPreferred, ..Default::default() },
  }], true);
  assert!(matches!(result, Ok(None)), "window dry run failed: {result:?}");
}

// https://github.com/moeru-ai/auv/actions/runs/30574130256/job/90978006366
#[test]
fn ci_90978006366_click_parts_support_repeated_clicks() {
  // ROOT CAUSE:
  //
  // If Click::Repeated was passed to the Windows driver, the crate failed to
  // compile because click_at still matched only the older single/double shape.
  //
  // Before the fix, Windows CI stopped at a non-exhaustive match.
  // The fix keeps every shared Click variant mapped to native count/interval values.
  assert_eq!(
    click_parts(&Click::Repeated {
      count: 3,
      interval: Duration::from_millis(60),
    })
    .expect("repeated click"),
    (3, Duration::from_millis(60))
  );
  assert!(matches!(
    click_parts(&Click::Repeated {
      count: 0,
      interval: Duration::from_millis(60),
    }),
    Err(DriverError::InvalidInput { message })
      if message == "repeated click count must be greater than zero"
  ));
}

#[test]
fn normalize_absolute_maps_axis_endpoints() {
  // A 1920-wide desktop starting at origin 0 maps x=0 -> 0 and x=1919 -> 65535.
  assert_eq!(normalize_absolute(0.0, 0, 1920), 0);
  assert_eq!(normalize_absolute(1919.0, 0, 1920), 65535);
}

#[test]
fn normalize_absolute_offsets_by_virtual_origin() {
  // A secondary monitor starting at x=-1920: its left edge maps to 0.
  assert_eq!(normalize_absolute(-1920.0, -1920, 1920), 0);
}

#[test]
fn normalize_absolute_clamps_out_of_range_and_handles_degenerate_extent() {
  assert_eq!(normalize_absolute(5000.0, 0, 1920), 65535);
  assert_eq!(normalize_absolute(-50.0, 0, 1920), 0);
  assert_eq!(normalize_absolute(10.0, 0, 1), 0);
}

#[test]
fn parse_key_chord_reads_special_keys_case_insensitively() {
  assert_eq!(
    parse_key_chord("Return").unwrap(),
    KeyChord {
      modifiers: vec![],
      key: vk::RETURN,
    }
  );
  assert_eq!(
    parse_key_chord("esc").unwrap(),
    KeyChord {
      modifiers: vec![],
      key: vk::ESCAPE,
    }
  );
}

#[test]
fn parse_key_chord_reads_single_alphanumeric() {
  assert_eq!(parse_key_chord("a").unwrap().key, u16::from(b'A'));
  assert_eq!(parse_key_chord("7").unwrap().key, u16::from(b'7'));
}

#[test]
fn parse_key_chord_reads_shortcut_with_modifiers() {
  let chord = parse_key_chord("ctrl+shift+p").unwrap();

  assert_eq!(chord.modifiers, vec![vk::CONTROL, vk::SHIFT]);
  assert_eq!(chord.key, u16::from(b'P'));
}

#[test]
fn parse_key_chord_deduplicates_modifiers() {
  let chord = parse_key_chord("ctrl+control+f").unwrap();

  assert_eq!(chord.modifiers, vec![vk::CONTROL]);
  assert_eq!(chord.key, u16::from(b'F'));
}

#[test]
fn parse_key_chord_rejects_empty_and_unknown() {
  assert!(parse_key_chord("   ").is_err());
  assert!(parse_key_chord("ctrl+").is_err());
  assert!(parse_key_chord("nope").is_err());
}

#[test]
fn text_submit_virtual_key_supports_return_only() {
  assert_eq!(text_submit_virtual_key(TextSubmit::No).unwrap(), None);
  assert_eq!(text_submit_virtual_key(TextSubmit::Return).unwrap(), Some(vk::RETURN));
  assert!(text_submit_virtual_key(TextSubmit::Search).is_err());
}

#[test]
fn click_modifiers_map_to_standard_platform_keys() {
  assert_eq!(
    click_modifier_keys(ClickModifiers {
      shift: true,
      control: true,
      alt: true,
      meta: true
    }),
    [0x10, 0x11, 0x12, 0x5b]
  );
  assert!(click_modifier_keys(Default::default()).is_empty());
}

#[cfg(target_os = "windows")]
#[test]
fn click_batch_and_partial_delivery_cleanup_preserve_release_order() {
  use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT_KEYBOARD, INPUT_MOUSE, KEYEVENTF_KEYUP, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
  };
  let keys = [0x10, 0x11];
  let batch = native::click_inputs(auv_driver_common::MouseButton::Left, &keys);
  assert_eq!(batch.len(), 6);
  // SAFETY: Read each union only after checking its INPUT tag.
  for (index, key) in [(0, 0x10), (1, 0x11), (4, 0x11), (5, 0x10)] {
    assert_eq!(batch[index].r#type, INPUT_KEYBOARD);
    assert_eq!(unsafe { batch[index].Anonymous.ki.wVk.0 }, key);
    assert_eq!(unsafe { batch[index].Anonymous.ki.dwFlags.contains(KEYEVENTF_KEYUP) }, index >= 4);
  }
  for (index, flags) in [(2, MOUSEEVENTF_LEFTDOWN), (3, MOUSEEVENTF_LEFTUP)] {
    assert_eq!(batch[index].r#type, INPUT_MOUSE);
    assert_eq!(unsafe { batch[index].Anonymous.mi.dwFlags }, flags);
  }
  for sent in 0..=batch.len() {
    let cleanup = native::click_cleanup(auv_driver_common::MouseButton::Left, &keys, sent);
    let expected_keys: &[u16] = match sent {
      0 | 6 => &[],
      1 | 5 => &[0x10],
      _ => &[0x11, 0x10],
    };
    let mut released = Vec::new();
    let mut mouse_releases = 0;
    for event in cleanup {
      if event.r#type == INPUT_KEYBOARD {
        assert!(unsafe { event.Anonymous.ki.dwFlags.contains(KEYEVENTF_KEYUP) });
        released.push(unsafe { event.Anonymous.ki.wVk.0 });
      } else {
        assert_eq!(event.r#type, INPUT_MOUSE);
        assert_eq!(unsafe { event.Anonymous.mi.dwFlags }, MOUSEEVENTF_LEFTUP);
        mouse_releases += 1;
      }
    }
    assert_eq!(released, expected_keys, "prefix {sent}");
    assert_eq!(mouse_releases, usize::from(sent == 3));
  }
}

// ROOT CAUSE:
//
// If an agent pressed an arrow, Home/End, Page Up/Down or a function key on
// Windows, or named punctuation (`ctrl+comma`, `ctrl+,`), the driver failed
// with `invalid key` because its table held only Return, Tab, Escape, Space,
// Delete, Backspace and media keys, and single characters had to be ASCII
// letters or digits.
//
// The fix adds the navigation and function keys and resolves punctuation
// through the shared key names and the active keyboard layout.
#[test]
fn navigation_and_function_keys_resolve_to_virtual_keys() {
  assert_eq!(parse_key_chord("Left").unwrap().key, vk::LEFT);
  assert_eq!(parse_key_chord("arrow_right").unwrap().key, vk::RIGHT);
  assert_eq!(parse_key_chord("Page-Down").unwrap().key, vk::NEXT);
  assert_eq!(parse_key_chord("home").unwrap().key, vk::HOME);
  assert_eq!(parse_key_chord("F5").unwrap().key, vk::F1 + 4);
  assert_eq!(parse_key_chord("f24").unwrap().key, vk::F1 + 23);
  assert_eq!(
    parse_key_chord("ctrl+shift+Left").unwrap(),
    KeyChord {
      modifiers: vec![vk::CONTROL, vk::SHIFT],
      key: vk::LEFT
    }
  );
  assert!(is_extended_key(vk::LEFT) && is_extended_key(vk::DELETE) && !is_extended_key(vk::RETURN));
  assert_eq!(parse_key_chord("menu").unwrap().key, vk::APPS);
  assert_eq!(parse_key_chord("PrintScreen").unwrap().key, vk::SNAPSHOT);
  assert_eq!(parse_key_chord("caps_lock").unwrap().key, vk::CAPITAL);
  assert!(parse_key_chord("forward-delete").is_err());
}

// Punctuation resolves through VkKeyScanW, so these run on Windows only; the
// expected codes are those of a US layout, which CI and the test host use.
#[cfg(target_os = "windows")]
#[test]
fn punctuation_resolves_through_the_active_layout() {
  const VK_OEM_COMMA: u16 = 0xBC;
  const VK_OEM_PLUS: u16 = 0xBB;
  assert_eq!(
    parse_key_chord("ctrl+,").unwrap(),
    KeyChord {
      modifiers: vec![vk::CONTROL],
      key: VK_OEM_COMMA
    }
  );
  assert_eq!(
    parse_key_chord("ctrl+comma").unwrap(),
    KeyChord {
      modifiers: vec![vk::CONTROL],
      key: VK_OEM_COMMA
    }
  );
  // `+` is Shift plus `=` on a US layout.
  assert_eq!(
    parse_key_chord("ctrl+plus").unwrap(),
    KeyChord {
      modifiers: vec![vk::SHIFT, vk::CONTROL],
      key: VK_OEM_PLUS
    }
  );
}

#[test]
fn shared_key_symbols_preserve_windows_named_key_behavior() {
  assert_eq!(special_virtual_key("enter"), Some(vk::RETURN));
  assert_eq!(special_virtual_key("delete"), Some(vk::DELETE));
  assert_eq!(special_virtual_key("backspace"), Some(vk::BACK));
  assert_eq!(special_virtual_key("play_pause"), Some(vk::MEDIA_PLAY_PAUSE));
  assert_eq!(special_virtual_key("forward_delete"), None);
  assert_eq!(modifier_virtual_key("win"), Some(vk::LWIN));
  assert_eq!(modifier_virtual_key("super"), None);
}

#[cfg(target_os = "windows")]
#[test]
fn right_and_middle_partial_clicks_release_the_selected_button() {
  use auv_driver_common::MouseButton;
  use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT_MOUSE, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
  };
  for (button, down, up) in [
    (MouseButton::Right, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
    (MouseButton::Middle, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
  ] {
    let batch = native::click_inputs(button, &[]);
    assert_eq!(batch.len(), 2);
    for (event, expected) in batch.iter().zip([down, up]) {
      assert_eq!(event.r#type, INPUT_MOUSE);
      // SAFETY: The checked INPUT_MOUSE tag identifies the initialized mi field.
      assert_eq!(unsafe { event.Anonymous.mi.dwFlags }, expected);
    }
    let cleanup = native::click_cleanup(button, &[], 1);
    assert_eq!(cleanup.len(), 1);
    assert_eq!(cleanup[0].r#type, INPUT_MOUSE);
    // SAFETY: The checked INPUT_MOUSE tag identifies the initialized mi field.
    assert_eq!(unsafe { cleanup[0].Anonymous.mi.dwFlags }, up);
    assert!(native::click_cleanup(button, &[], 2).is_empty());
  }
}

#[test]
fn wheel_units_map_logical_pixels_to_win32_notches_with_scroll_down_positive() {
  // ROOT CAUSE:
  //
  // If a caller sent `Scroll::new(0.0, 300.0)` meaning 300 logical pixels
  // down, Windows multiplied it by WHEEL_DELTA as 300 upward notches.
  //
  // Before the fix, Windows treated deltas as notches with Win32's up-positive
  // sign, unlike macOS pixels and Linux down-positive deltas.
  // The fix keeps `Scroll` in logical pixels, positive toward later content.
  assert_eq!(
    wheel_units(Scroll::new(0.0, 100.0)).unwrap(),
    WheelUnits {
      vertical: -120,
      horizontal: 0
    }
  );
  assert_eq!(
    wheel_units(Scroll::new(0.0, -50.0)).unwrap(),
    WheelUnits {
      vertical: 60,
      horizontal: 0
    }
  );
  assert_eq!(
    wheel_units(Scroll::new(100.0, 0.0)).unwrap(),
    WheelUnits {
      vertical: 0,
      horizontal: 120
    }
  );
}

#[test]
fn wheel_units_reject_values_outside_posted_wheel_word() {
  assert!(matches!(wheel_units(Scroll::new(0.0, f64::NAN)), Err(DriverError::InvalidInput { .. })));
  assert!(matches!(wheel_units(Scroll::new(f64::INFINITY, 0.0)), Err(DriverError::InvalidInput { .. })));
  assert!(wheel_units(Scroll::new(0.0, 27_305.0)).is_ok());
  assert!(matches!(wheel_units(Scroll::new(0.0, 27_400.0)), Err(DriverError::InvalidInput { .. })));
}

#[test]
fn a_paste_read_by_the_foreground_application_gives_the_clipboard_back_at_once() {
  let sent = Instant::now();
  let read = ClipboardRead {
    reader: ClipboardReader::Process(42),
    at: sent + Duration::from_millis(12),
  };
  assert_eq!(
    paste_read(Some(read), sent, Some(42)),
    PasteRead::Foreground {
      after: Duration::from_millis(12)
    }
  );
}

#[test]
fn a_paste_read_first_by_anyone_else_is_held_before_the_clipboard_comes_back() {
  // Once anyone has read lent text, Windows hands it out without asking again, so the
  // foreground application's own read can no longer be seen.
  let sent = Instant::now();
  let at = sent + Duration::from_millis(5);
  let other = ClipboardRead {
    reader: ClipboardReader::Process(7),
    at,
  };
  let unnamed = ClipboardRead {
    reader: ClipboardReader::Unknown,
    at,
  };
  let after = Duration::from_millis(5);
  assert_eq!(
    paste_read(Some(other), sent, Some(42)),
    PasteRead::Other {
      reader: ClipboardReader::Process(7),
      after
    }
  );
  assert_eq!(
    paste_read(Some(unnamed), sent, Some(42)),
    PasteRead::Other {
      reader: ClipboardReader::Unknown,
      after
    }
  );
  // With no foreground window, no reader can be the foreground application.
  assert_eq!(
    paste_read(Some(other), sent, None),
    PasteRead::Other {
      reader: ClipboardReader::Process(7),
      after
    }
  );
}

#[test]
fn a_paste_nobody_read_is_unread() {
  assert_eq!(paste_read(None, Instant::now(), Some(42)), PasteRead::Unread);
}
