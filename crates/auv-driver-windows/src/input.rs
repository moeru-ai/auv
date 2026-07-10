//! Foreground pointer and keyboard input via Win32 `SendInput`.
//!
//! Mirrors the macOS driver's `InputApi`, but Windows has no accessibility
//! input path, so every primitive is delivered as a foreground synthetic event
//! (`InputDeliveryPath::ForegroundSystemEvents`). Pointer moves use absolute
//! virtual-desktop coordinates; text is injected as Unicode key events and key
//! presses/shortcuts as virtual-key events. The coordinate math and key parsing
//! live in host-independent helpers so they stay unit-testable without sending
//! real input.
// TODO(windows-input-target-lease): like the macOS slice, this types/clicks
// into the current foreground target. Target-aware, window-prepared input is
// deferred until an owner-approved slice connects it to focus leasing.

use std::time::Duration;

use auv_driver_common::error::DriverResult;
use auv_driver_common::geometry::Point;
use auv_driver_common::input::{
  Click, DisturbanceLevel, InputActionResult, InputAttempt, InputDeliveryPath, InputPolicy, KeyPressOptions, Scroll, TextSubmit,
  TypeTextOptions,
};

use crate::error::invalid_input;

/// Virtual-key code for a modifier, kept as a bare `u16` so shortcut parsing
/// stays independent of the `windows` crate types.
mod vk {
  pub const BACK: u16 = 0x08;
  pub const TAB: u16 = 0x09;
  pub const RETURN: u16 = 0x0D;
  pub const SHIFT: u16 = 0x10;
  pub const CONTROL: u16 = 0x11;
  pub const MENU: u16 = 0x12; // Alt
  pub const ESCAPE: u16 = 0x1B;
  pub const SPACE: u16 = 0x20;
  pub const DELETE: u16 = 0x2E;
  pub const LWIN: u16 = 0x5B;
  // Media keys (global — no app focus required).
  pub const MEDIA_NEXT_TRACK: u16 = 0xB0;
  pub const MEDIA_PREV_TRACK: u16 = 0xB1;
  pub const MEDIA_STOP: u16 = 0xB2;
  pub const MEDIA_PLAY_PAUSE: u16 = 0xB3;
}

/// A parsed key request: zero or more modifier virtual-keys plus one target key.
#[derive(Clone, Debug, PartialEq, Eq)]
struct KeyChord {
  modifiers: Vec<u16>,
  key: u16,
}

pub fn click_at(point: Point, click: Click) -> DriverResult<InputActionResult> {
  let count = match click {
    Click::Single => 1u32,
    Click::Double { .. } => 2,
  };
  let interval = match click {
    Click::Single => Duration::ZERO,
    Click::Double { interval } => interval,
  };
  native::click(point, count, interval)?;
  Ok(foreground_result(DisturbanceLevel::Temporary, DisturbanceLevel::Unknown, DisturbanceLevel::None))
}

pub fn scroll_at(point: Point, scroll: Scroll, settle: Duration) -> DriverResult<InputActionResult> {
  native::scroll(point, scroll)?;
  sleep_if_nonzero(settle);
  Ok(foreground_result(DisturbanceLevel::Temporary, DisturbanceLevel::Unknown, DisturbanceLevel::None))
}

pub fn type_text(text: &str, options: TypeTextOptions) -> DriverResult<InputActionResult> {
  if matches!(options.policy, InputPolicy::BackgroundOnly) {
    return Err(invalid_input("windows type_text cannot use background_only input policy"));
  }
  let submit_key = text_submit_virtual_key(options.submit)?;
  native::type_text(text, &options, submit_key)?;
  sleep_if_nonzero(options.settle);
  Ok(foreground_result(DisturbanceLevel::None, DisturbanceLevel::Unknown, DisturbanceLevel::None))
}

pub fn press_key(options: KeyPressOptions) -> DriverResult<InputActionResult> {
  let chord = parse_key_chord(&options.key)?;
  native::press_chord(&chord)?;
  sleep_if_nonzero(options.settle);
  Ok(foreground_result(DisturbanceLevel::None, DisturbanceLevel::Unknown, DisturbanceLevel::None))
}

/// Issues the system copy shortcut (Ctrl+C) against the foreground target.
///
/// Mirrors the macOS driver's keystroke-based copy: it activates the active
/// control's own copy handler rather than reading selection text directly, so
/// the resulting clipboard contents depend on the focused application.
pub fn copy() -> DriverResult<()> {
  native::press_chord(&KeyChord {
    modifiers: vec![vk::CONTROL],
    key: u16::from(b'C'),
  })
}

/// Issues the system paste shortcut (Ctrl+V) against the foreground target.
pub fn paste() -> DriverResult<()> {
  native::press_chord(&KeyChord {
    modifiers: vec![vk::CONTROL],
    key: u16::from(b'V'),
  })
}

fn foreground_result(
  mouse_disturbance: DisturbanceLevel,
  focus_disturbance: DisturbanceLevel,
  clipboard_disturbance: DisturbanceLevel,
) -> InputActionResult {
  InputActionResult {
    selected_path: InputDeliveryPath::ForegroundSystemEvents,
    attempts: vec![InputAttempt::success(
      InputDeliveryPath::ForegroundSystemEvents,
    )],
    fallback_reason: None,
    mouse_disturbance,
    focus_disturbance,
    clipboard_disturbance,
  }
}

fn sleep_if_nonzero(duration: Duration) {
  if !duration.is_zero() {
    std::thread::sleep(duration);
  }
}

/// Maps a screen coordinate onto the `0..=65535` absolute axis that
/// `MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK` expects, where `origin` and
/// `extent` are the virtual desktop's start and size on that axis.
fn normalize_absolute(coord: f64, origin: i32, extent: i32) -> i32 {
  if extent <= 1 {
    return 0;
  }
  let relative = coord - f64::from(origin);
  let scaled = relative * 65535.0 / f64::from(extent - 1);
  scaled.round().clamp(0.0, 65535.0) as i32
}

fn text_submit_virtual_key(submit: TextSubmit) -> DriverResult<Option<u16>> {
  match submit {
    TextSubmit::No => Ok(None),
    TextSubmit::Return => Ok(Some(vk::RETURN)),
    TextSubmit::Search | TextSubmit::Done | TextSubmit::Go => {
      Err(invalid_input(format!("text submit {submit:?} is not supported by the windows desktop driver yet")))
    }
  }
}

fn parse_key_chord(input: &str) -> DriverResult<KeyChord> {
  let trimmed = input.trim();
  if trimmed.is_empty() {
    return Err(invalid_input("key must not be empty"));
  }
  if trimmed.contains('+') {
    return parse_shortcut(trimmed);
  }
  if let Some(key) = special_virtual_key(trimmed) {
    return Ok(KeyChord {
      modifiers: Vec::new(),
      key,
    });
  }
  if let Some(key) = single_char_virtual_key(trimmed) {
    return Ok(KeyChord {
      modifiers: Vec::new(),
      key,
    });
  }
  Err(invalid_input(format!(
    "invalid key {trimmed}; use a special key like Return, a shortcut like ctrl+f, or type_text for multi-character text"
  )))
}

fn parse_shortcut(shortcut: &str) -> DriverResult<KeyChord> {
  let parts = shortcut.split('+').map(str::trim).filter(|part| !part.is_empty()).collect::<Vec<_>>();
  if parts.len() < 2 {
    return Err(invalid_input(format!("invalid shortcut {shortcut}; expected a form like ctrl+f or ctrl+shift+p")));
  }
  let (key_part, modifier_parts) = parts.split_last().expect("len checked >= 2");
  let key = single_char_virtual_key(key_part)
    .or_else(|| special_virtual_key(key_part))
    .ok_or_else(|| invalid_input(format!("invalid shortcut {shortcut}; unsupported key {key_part}")))?;
  let mut modifiers = Vec::new();
  for raw in modifier_parts {
    let modifier =
      modifier_virtual_key(raw).ok_or_else(|| invalid_input(format!("invalid shortcut {shortcut}; unsupported modifier {raw}")))?;
    if !modifiers.contains(&modifier) {
      modifiers.push(modifier);
    }
  }
  Ok(KeyChord { modifiers, key })
}

fn modifier_virtual_key(raw: &str) -> Option<u16> {
  match raw.to_ascii_lowercase().as_str() {
    "ctrl" | "control" => Some(vk::CONTROL),
    "shift" => Some(vk::SHIFT),
    "alt" | "option" => Some(vk::MENU),
    "win" | "cmd" | "command" | "meta" => Some(vk::LWIN),
    _ => None,
  }
}

fn special_virtual_key(raw: &str) -> Option<u16> {
  match raw.to_ascii_lowercase().as_str() {
    "return" | "enter" => Some(vk::RETURN),
    "tab" => Some(vk::TAB),
    "escape" | "esc" => Some(vk::ESCAPE),
    "space" => Some(vk::SPACE),
    "delete" => Some(vk::DELETE),
    "backspace" | "back" => Some(vk::BACK),
    "media_play_pause" | "play_pause" => Some(vk::MEDIA_PLAY_PAUSE),
    "media_next" | "next_track" => Some(vk::MEDIA_NEXT_TRACK),
    "media_prev" | "prev_track" => Some(vk::MEDIA_PREV_TRACK),
    "media_stop" | "stop" => Some(vk::MEDIA_STOP),
    _ => None,
  }
}

/// Maps a single ASCII alphanumeric character to its virtual-key code. Letters
/// map to their uppercase code point and digits to their ASCII code, which is
/// the Win32 VK convention for `0-9`/`A-Z`.
fn single_char_virtual_key(raw: &str) -> Option<u16> {
  let mut chars = raw.chars();
  let first = chars.next()?;
  if chars.next().is_some() {
    return None;
  }
  if first.is_ascii_alphanumeric() {
    Some(u16::from(first.to_ascii_uppercase() as u8))
  } else {
    None
  }
}

#[cfg(target_os = "windows")]
mod native {
  use std::mem::size_of;
  use std::thread;
  use std::time::Duration;

  use auv_driver_common::error::DriverResult;
  use auv_driver_common::geometry::Point;
  use auv_driver_common::input::{Scroll, TypeTextOptions};
  use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOUSE_EVENT_FLAGS,
    MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK,
    MOUSEEVENTF_WHEEL, MOUSEINPUT, SendInput, VIRTUAL_KEY,
  };
  use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, WHEEL_DELTA,
  };

  use super::{KeyChord, normalize_absolute};
  use crate::error::backend;

  /// Sends one batch of synthetic input events, failing if the OS rejected any
  /// (a blocked input session or UIPI denial returns a short count).
  fn send_inputs(inputs: &[INPUT]) -> DriverResult<()> {
    if inputs.is_empty() {
      return Ok(());
    }
    let sent = unsafe { SendInput(inputs, size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
      return Err(backend(format!("SendInput injected {sent} of {} events (input may be blocked)", inputs.len())));
    }
    Ok(())
  }

  fn mouse_input(dx: i32, dy: i32, mouse_data: i32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
      r#type: INPUT_MOUSE,
      Anonymous: INPUT_0 {
        mi: MOUSEINPUT {
          dx,
          dy,
          mouseData: mouse_data as u32,
          dwFlags: flags,
          time: 0,
          dwExtraInfo: 0,
        },
      },
    }
  }

  fn unicode_input(unit: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
      r#type: INPUT_KEYBOARD,
      Anonymous: INPUT_0 {
        ki: KEYBDINPUT {
          wVk: VIRTUAL_KEY(0),
          wScan: unit,
          dwFlags: KEYEVENTF_UNICODE | flags,
          time: 0,
          dwExtraInfo: 0,
        },
      },
    }
  }

  fn virtual_key_input(vk: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
      r#type: INPUT_KEYBOARD,
      Anonymous: INPUT_0 {
        ki: KEYBDINPUT {
          wVk: VIRTUAL_KEY(vk),
          wScan: 0,
          dwFlags: flags,
          time: 0,
          dwExtraInfo: 0,
        },
      },
    }
  }

  fn move_cursor(point: Point) -> DriverResult<()> {
    let origin_x = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let origin_y = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
    let width = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) };
    let height = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) };
    let dx = normalize_absolute(point.x, origin_x, width);
    let dy = normalize_absolute(point.y, origin_y, height);
    send_inputs(&[mouse_input(
      dx,
      dy,
      0,
      MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
    )])
  }

  pub(super) fn click(point: Point, count: u32, interval: Duration) -> DriverResult<()> {
    move_cursor(point)?;
    for index in 0..count {
      send_inputs(&[
        mouse_input(0, 0, 0, MOUSEEVENTF_LEFTDOWN),
        mouse_input(0, 0, 0, MOUSEEVENTF_LEFTUP),
      ])?;
      if index + 1 < count && !interval.is_zero() {
        thread::sleep(interval);
      }
    }
    Ok(())
  }

  pub(super) fn scroll(point: Point, scroll: Scroll) -> DriverResult<()> {
    move_cursor(point)?;
    let mut inputs = Vec::new();
    // Windows wheel deltas are signed multiples of WHEEL_DELTA (120). Positive
    // vertical scrolls up and positive horizontal scrolls right, per the Win32
    // wheel convention.
    let vertical = wheel_amount(scroll.delta_y);
    if vertical != 0 {
      inputs.push(mouse_input(0, 0, vertical, MOUSEEVENTF_WHEEL));
    }
    let horizontal = wheel_amount(scroll.delta_x);
    if horizontal != 0 {
      inputs.push(mouse_input(0, 0, horizontal, MOUSEEVENTF_HWHEEL));
    }
    send_inputs(&inputs)
  }

  fn wheel_amount(delta: f64) -> i32 {
    if !delta.is_finite() {
      return 0;
    }
    (delta * f64::from(WHEEL_DELTA)).round() as i32
  }

  pub(super) fn type_text(text: &str, options: &TypeTextOptions, submit_key: Option<u16>) -> DriverResult<()> {
    if options.replace_existing {
      // Ctrl+A then Backspace clears the focused field before typing.
      press_chord(&KeyChord {
        modifiers: vec![super::vk::CONTROL],
        key: u16::from(b'A'),
      })?;
      send_inputs(&[
        virtual_key_input(super::vk::BACK, KEYBD_EVENT_FLAGS(0)),
        virtual_key_input(super::vk::BACK, KEYEVENTF_KEYUP),
      ])?;
    }
    let inter_char_delay = options.inter_char_delay;
    for character in text.chars() {
      let mut buffer = [0u16; 2];
      for unit in character.encode_utf16(&mut buffer) {
        send_inputs(&[
          unicode_input(*unit, KEYBD_EVENT_FLAGS(0)),
          unicode_input(*unit, KEYEVENTF_KEYUP),
        ])?;
      }
      if !inter_char_delay.is_zero() {
        thread::sleep(inter_char_delay);
      }
    }
    if let Some(vk) = submit_key {
      send_inputs(&[
        virtual_key_input(vk, KEYBD_EVENT_FLAGS(0)),
        virtual_key_input(vk, KEYEVENTF_KEYUP),
      ])?;
    }
    Ok(())
  }

  pub(super) fn press_chord(chord: &KeyChord) -> DriverResult<()> {
    let mut inputs = Vec::new();
    for modifier in &chord.modifiers {
      inputs.push(virtual_key_input(*modifier, KEYBD_EVENT_FLAGS(0)));
    }
    inputs.push(virtual_key_input(chord.key, KEYBD_EVENT_FLAGS(0)));
    inputs.push(virtual_key_input(chord.key, KEYEVENTF_KEYUP));
    // Release modifiers in reverse press order.
    for modifier in chord.modifiers.iter().rev() {
      inputs.push(virtual_key_input(*modifier, KEYEVENTF_KEYUP));
    }
    send_inputs(&inputs)
  }
}

#[cfg(not(target_os = "windows"))]
mod native {
  use std::time::Duration;

  use auv_driver_common::error::{DriverError, DriverResult};
  use auv_driver_common::geometry::Point;
  use auv_driver_common::input::{Scroll, TypeTextOptions};

  use super::KeyChord;

  pub(super) fn click(_point: Point, _count: u32, _interval: Duration) -> DriverResult<()> {
    Err(DriverError::unsupported("input.click"))
  }

  pub(super) fn scroll(_point: Point, _scroll: Scroll) -> DriverResult<()> {
    Err(DriverError::unsupported("input.scroll"))
  }

  pub(super) fn type_text(_text: &str, _options: &TypeTextOptions, _submit_key: Option<u16>) -> DriverResult<()> {
    Err(DriverError::unsupported("input.type_text"))
  }

  pub(super) fn press_chord(_chord: &KeyChord) -> DriverResult<()> {
    Err(DriverError::unsupported("input.press_key"))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

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
    assert!(parse_key_chord("ctrl+@").is_err());
    assert!(parse_key_chord("nope").is_err());
  }

  #[test]
  fn text_submit_virtual_key_supports_return_only() {
    assert_eq!(text_submit_virtual_key(TextSubmit::No).unwrap(), None);
    assert_eq!(text_submit_virtual_key(TextSubmit::Return).unwrap(), Some(vk::RETURN));
    assert!(text_submit_virtual_key(TextSubmit::Search).is_err());
  }
}
