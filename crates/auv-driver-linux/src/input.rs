use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::driver::LinuxDriverSessionState;
use crate::error::invalid_input;
use crate::native::portal::{InputSession, PortalInput};
use auv_driver::error::DriverResult;
use auv_driver::geometry::Point;
use auv_driver::input::{
  Click, DisturbanceLevel, InputActionResult, InputAttempt, InputDeliveryPath, InputPolicy,
  KeyPressOptions, PasteTextOptions, Scroll, TextSubmit, TypeTextOptions,
};

use crate::clipboard::{
  restore as restore_clipboard, set_text as set_clipboard_text, snapshot as snapshot_clipboard,
};

pub(crate) fn click_at(
  state: &Arc<Mutex<LinuxDriverSessionState>>,
  point: Point,
  click: Click,
) -> DriverResult<InputActionResult> {
  let fallback_reason = with_input_session(state, |session| session.click_at(point, click))?;
  Ok(pointer_result(fallback_reason))
}

pub(crate) fn scroll_at(
  state: &Arc<Mutex<LinuxDriverSessionState>>,
  _point: Point,
  scroll: Scroll,
  settle: Duration,
) -> DriverResult<InputActionResult> {
  with_input_session(state, |session| session.scroll(scroll))?;
  sleep_if_nonzero(settle);
  Ok(pointer_result(None))
}

pub(crate) fn type_text(
  state: &Arc<Mutex<LinuxDriverSessionState>>,
  text: &str,
  options: TypeTextOptions,
) -> DriverResult<InputActionResult> {
  if matches!(options.policy, InputPolicy::BackgroundOnly) {
    return Err(invalid_input(
      "linux type_text cannot use background_only input policy",
    ));
  }
  with_input_session(state, |session| {
    if options.replace_existing {
      session.key_chord(&[keysym::CONTROL_L], keysym::for_char('a')?)?;
      session.key_press(keysym::BACKSPACE)?;
    }
    for ch in text.chars() {
      session.key_press(keysym::for_char(ch)?)?;
      sleep_if_nonzero(options.inter_char_delay);
    }
    match options.submit {
      TextSubmit::No => {}
      TextSubmit::Return | TextSubmit::Search | TextSubmit::Done | TextSubmit::Go => {
        session.key_press(keysym::RETURN)?;
      }
    }
    Ok(())
  })?;
  sleep_if_nonzero(options.settle);
  Ok(keyboard_result())
}

pub(crate) fn press_key(
  state: &Arc<Mutex<LinuxDriverSessionState>>,
  options: KeyPressOptions,
) -> DriverResult<InputActionResult> {
  let chord = parse_key_chord(&options.key)?;
  with_input_session(state, |session| {
    session.key_chord(&chord.modifiers, chord.key)
  })?;
  sleep_if_nonzero(options.settle);
  Ok(keyboard_result())
}

pub(crate) fn copy(state: &Arc<Mutex<LinuxDriverSessionState>>) -> DriverResult<()> {
  with_input_session(state, |session| {
    session.key_chord(&[keysym::CONTROL_L], keysym::for_char('c')?)
  })
}

pub(crate) fn paste(state: &Arc<Mutex<LinuxDriverSessionState>>) -> DriverResult<()> {
  with_input_session(state, |session| {
    session.key_chord(&[keysym::CONTROL_L], keysym::for_char('v')?)
  })
}

pub(crate) fn paste_text(
  state: &Arc<Mutex<LinuxDriverSessionState>>,
  options: PasteTextOptions,
) -> DriverResult<()> {
  let snapshot = snapshot_clipboard(state)?;
  let result = (|| {
    set_clipboard_text(state, &options.text)?;
    with_input_session(state, |session| {
      if options.replace_existing {
        session.key_chord(&[keysym::CONTROL_L], keysym::for_char('a')?)?;
      }
      session.key_chord(&[keysym::CONTROL_L], keysym::for_char('v')?)?;
      match options.submit {
        TextSubmit::No => {}
        TextSubmit::Return | TextSubmit::Search | TextSubmit::Done | TextSubmit::Go => {
          session.key_press(keysym::RETURN)?;
        }
      }
      Ok(())
    })?;
    sleep_if_nonzero(options.settle);
    Ok(())
  })();
  let restore_result = restore_clipboard(state, &snapshot);
  match (result, restore_result) {
    (Ok(()), Ok(())) => Ok(()),
    (Err(action_error), Ok(())) => Err(action_error),
    (Ok(()), Err(restore_error)) => Err(crate::error::backend(format!(
      "pasted text but failed to restore clipboard: {restore_error}"
    ))),
    (Err(action_error), Err(restore_error)) => Err(crate::error::backend(format!(
      "{action_error}; additionally failed to restore clipboard: {restore_error}"
    ))),
  }
}

pub fn reserved_input_result(reason: impl Into<String>) -> InputActionResult {
  let reason = reason.into();
  InputActionResult {
    selected_path: InputDeliveryPath::Unsupported,
    attempts: vec![InputAttempt::failure(
      InputDeliveryPath::Unsupported,
      reason.clone(),
    )],
    fallback_reason: Some(reason),
    mouse_disturbance: DisturbanceLevel::None,
    focus_disturbance: DisturbanceLevel::None,
    clipboard_disturbance: DisturbanceLevel::None,
  }
}

fn with_input_session<T>(
  state: &Arc<Mutex<LinuxDriverSessionState>>,
  operation: impl FnOnce(&mut InputSession) -> DriverResult<T>,
) -> DriverResult<T> {
  let mut state = state.lock().expect("linux driver session state poisoned");
  if state.input_session.is_none() {
    state.input_session = Some(PortalInput::open()?);
  }
  operation(
    state
      .input_session
      .as_mut()
      .expect("input session was just initialized"),
  )
}

fn keyboard_result() -> InputActionResult {
  InputActionResult {
    selected_path: InputDeliveryPath::ForegroundSystemEvents,
    attempts: vec![InputAttempt::success(
      InputDeliveryPath::ForegroundSystemEvents,
    )],
    fallback_reason: None,
    mouse_disturbance: DisturbanceLevel::None,
    focus_disturbance: DisturbanceLevel::Unknown,
    clipboard_disturbance: DisturbanceLevel::None,
  }
}

fn pointer_result(fallback_reason: Option<String>) -> InputActionResult {
  InputActionResult {
    selected_path: InputDeliveryPath::ForegroundSystemEvents,
    attempts: vec![InputAttempt::success(
      InputDeliveryPath::ForegroundSystemEvents,
    )],
    fallback_reason,
    mouse_disturbance: DisturbanceLevel::Temporary,
    focus_disturbance: DisturbanceLevel::Unknown,
    clipboard_disturbance: DisturbanceLevel::None,
  }
}

fn sleep_if_nonzero(duration: Duration) {
  if !duration.is_zero() {
    std::thread::sleep(duration);
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct KeyChord {
  modifiers: Vec<i32>,
  key: i32,
}

fn parse_key_chord(input: &str) -> DriverResult<KeyChord> {
  let trimmed = input.trim();
  if trimmed.is_empty() {
    return Err(invalid_input("key must not be empty"));
  }
  if trimmed.contains('+') {
    let parts = trimmed
      .split('+')
      .map(str::trim)
      .filter(|part| !part.is_empty())
      .collect::<Vec<_>>();
    if parts.len() < 2 {
      return Err(invalid_input(format!(
        "invalid shortcut {trimmed}; expected a form like ctrl+f"
      )));
    }
    let (key_part, modifier_parts) = parts.split_last().expect("len checked");
    let mut modifiers = Vec::new();
    for raw in modifier_parts {
      let modifier = keysym::modifier(raw).ok_or_else(|| {
        invalid_input(format!(
          "invalid shortcut {trimmed}; unsupported modifier {raw}"
        ))
      })?;
      if !modifiers.contains(&modifier) {
        modifiers.push(modifier);
      }
    }
    Ok(KeyChord {
      modifiers,
      key: keysym::named_or_char(key_part)?,
    })
  } else {
    Ok(KeyChord {
      modifiers: Vec::new(),
      key: keysym::named_or_char(trimmed)?,
    })
  }
}

mod keysym {
  use auv_driver::error::DriverResult;

  use crate::error::invalid_input;

  pub const BACKSPACE: i32 = 0xff08;
  pub const TAB: i32 = 0xff09;
  pub const RETURN: i32 = 0xff0d;
  pub const ESCAPE: i32 = 0xff1b;
  pub const DELETE: i32 = 0xffff;
  pub const SHIFT_L: i32 = 0xffe1;
  pub const CONTROL_L: i32 = 0xffe3;
  pub const ALT_L: i32 = 0xffe9;
  pub const SUPER_L: i32 = 0xffeb;

  pub fn modifier(raw: &str) -> Option<i32> {
    match raw.to_ascii_lowercase().as_str() {
      "ctrl" | "control" => Some(CONTROL_L),
      "shift" => Some(SHIFT_L),
      "alt" | "option" => Some(ALT_L),
      "super" | "win" | "cmd" | "command" | "meta" => Some(SUPER_L),
      _ => None,
    }
  }

  pub fn named_or_char(raw: &str) -> DriverResult<i32> {
    if let Some(keysym) = named(raw) {
      return Ok(keysym);
    }
    let mut chars = raw.chars();
    let Some(ch) = chars.next() else {
      return Err(invalid_input("key must not be empty"));
    };
    if chars.next().is_some() {
      return Err(invalid_input(format!(
        "invalid key {raw}; use a special key, shortcut, or type_text for multi-character text"
      )));
    }
    for_char(ch)
  }

  pub fn for_char(ch: char) -> DriverResult<i32> {
    if ch.is_ascii() && !ch.is_control() {
      return Ok(ch as i32);
    }
    match ch {
      '\n' | '\r' => Ok(RETURN),
      '\t' => Ok(TAB),
      _ => Err(invalid_input(format!(
        "linux portal keyboard input only supports ASCII text in this slice; unsupported character {ch:?}"
      ))),
    }
  }

  fn named(raw: &str) -> Option<i32> {
    match raw.to_ascii_lowercase().as_str() {
      "return" | "enter" => Some(RETURN),
      "tab" => Some(TAB),
      "escape" | "esc" => Some(ESCAPE),
      "space" => Some(' ' as i32),
      "delete" => Some(DELETE),
      "backspace" | "back" => Some(BACKSPACE),
      _ => None,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn reserved_result_uses_shared_input_schema() {
    let result = reserved_input_result("not wired yet");

    assert_eq!(result.selected_path, InputDeliveryPath::Unsupported);
    assert_eq!(result.attempts.len(), 1);
  }
}
