use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::driver::LinuxDriverSessionState;
use crate::error::invalid_input;
use crate::native::portal::{InputSession, PortalInput};
use auv_driver_common::error::{DriverError, DriverResult};
use auv_driver_common::geometry::Point;
use auv_driver_common::input::{
  Click, DisturbanceLevel, InputActionResult, InputAttempt, InputDeliveryPath, InputPolicy, KeyPressOptions, PasteTextOptions, Scroll,
  TextSubmit, TypeTextOptions,
};

use crate::clipboard::{restore as restore_clipboard, set_text as set_clipboard_text, snapshot as snapshot_clipboard};

pub(crate) fn click_at(state: &Arc<Mutex<LinuxDriverSessionState>>, point: Point, click: Click) -> DriverResult<InputActionResult> {
  with_input_session(state, |session| session.click_at(point, click))?;
  Ok(pointer_result())
}

pub(crate) fn move_to(state: &Arc<Mutex<LinuxDriverSessionState>>, point: Point) -> DriverResult<InputActionResult> {
  with_input_session(state, |session| session.move_to(point))?;
  Ok(pointer_result())
}

pub(crate) fn current_position() -> DriverResult<Point> {
  // TODO(linux-wayland-pointer-position): The RemoteDesktop portal can inject
  // motion but cannot report the current logical pointer position. Add this
  // capability when a compositor-neutral Wayland or portal API can supply it.
  Err(DriverError::unsupported("linux.input.current_position on Wayland"))
}

pub(crate) fn scroll_at(
  state: &Arc<Mutex<LinuxDriverSessionState>>,
  point: Point,
  scroll: Scroll,
  settle: Duration,
) -> DriverResult<InputActionResult> {
  with_input_session(state, |session| session.scroll_at(point, scroll))?;
  sleep_if_nonzero(settle);
  Ok(pointer_result())
}

pub(crate) fn type_text(
  state: &Arc<Mutex<LinuxDriverSessionState>>,
  text: &str,
  options: TypeTextOptions,
) -> DriverResult<InputActionResult> {
  if matches!(options.policy, InputPolicy::BackgroundOnly) {
    return Err(invalid_input("linux type_text cannot use background_only input policy"));
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

pub(crate) fn press_key(state: &Arc<Mutex<LinuxDriverSessionState>>, options: KeyPressOptions) -> DriverResult<InputActionResult> {
  let chord = parse_key_chord(&options.key)?;
  with_input_session(state, |session| session.key_chord(&chord.modifiers, chord.key))?;
  sleep_if_nonzero(options.settle);
  Ok(keyboard_result())
}

pub(crate) fn copy(state: &Arc<Mutex<LinuxDriverSessionState>>) -> DriverResult<()> {
  with_input_session(state, |session| session.key_chord(&[keysym::CONTROL_L], keysym::for_char('c')?))
}

pub(crate) fn paste(state: &Arc<Mutex<LinuxDriverSessionState>>) -> DriverResult<()> {
  with_input_session(state, |session| session.key_chord(&[keysym::CONTROL_L], keysym::for_char('v')?))
}

pub(crate) fn paste_text(state: &Arc<Mutex<LinuxDriverSessionState>>, options: PasteTextOptions) -> DriverResult<InputActionResult> {
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
    (Ok(()), Ok(())) => Ok(InputActionResult {
      selected_path: InputDeliveryPath::ClipboardPaste,
      attempts: vec![InputAttempt::success(InputDeliveryPath::ClipboardPaste)],
      verified: false,
      mouse_disturbance: DisturbanceLevel::None,
      focus_disturbance: DisturbanceLevel::Unknown,
      clipboard_disturbance: DisturbanceLevel::Temporary,
    }),
    (Err(action_error), Ok(())) => Err(action_error),
    (Ok(()), Err(restore_error)) => Err(crate::error::backend(format!("pasted text but failed to restore clipboard: {restore_error}"))),
    (Err(action_error), Err(restore_error)) => {
      Err(crate::error::backend(format!("{action_error}; additionally failed to restore clipboard: {restore_error}")))
    }
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
    verified: false,
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
    let restore_tokens = state.restore_tokens.clone();
    state.input_session = Some(PortalInput::open(restore_tokens.as_ref())?);
  }
  let result = operation(state.input_session.as_mut().expect("input session was just initialized"));
  if result.is_err() {
    // A successful RemoteDesktop D-Bus call does not prove that a restored
    // stream still delivers events. Drop a failed session so the next action
    // reopens it through the durable restore-token rotation instead of reusing
    // a stale stream indefinitely.
    state.input_session = None;
  }
  result
}

fn keyboard_result() -> InputActionResult {
  InputActionResult {
    selected_path: InputDeliveryPath::ForegroundSystemEvents,
    attempts: vec![InputAttempt::success(
      InputDeliveryPath::ForegroundSystemEvents,
    )],
    verified: false,
    mouse_disturbance: DisturbanceLevel::None,
    focus_disturbance: DisturbanceLevel::Unknown,
    clipboard_disturbance: DisturbanceLevel::None,
  }
}

fn pointer_result() -> InputActionResult {
  InputActionResult {
    selected_path: InputDeliveryPath::ForegroundSystemEvents,
    attempts: vec![InputAttempt::success(
      InputDeliveryPath::ForegroundSystemEvents,
    )],
    verified: false,
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
    let parts = trimmed.split('+').map(str::trim).filter(|part| !part.is_empty()).collect::<Vec<_>>();
    if parts.len() < 2 {
      return Err(invalid_input(format!("invalid shortcut {trimmed}; expected a form like ctrl+f")));
    }
    let (key_part, modifier_parts) = parts.split_last().expect("len checked");
    let mut modifiers = Vec::new();
    for raw in modifier_parts {
      let modifier =
        keysym::modifier(raw).ok_or_else(|| invalid_input(format!("invalid shortcut {trimmed}; unsupported modifier {raw}")))?;
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
  use auv_driver_common::error::DriverResult;

  use crate::error::invalid_input;

  pub const BACKSPACE: i32 = 0xff08;
  pub const TAB: i32 = 0xff09;
  pub const RETURN: i32 = 0xff0d;
  pub const ESCAPE: i32 = 0xff1b;
  pub const HOME: i32 = 0xff50;
  pub const LEFT: i32 = 0xff51;
  pub const UP: i32 = 0xff52;
  pub const RIGHT: i32 = 0xff53;
  pub const DOWN: i32 = 0xff54;
  pub const PAGE_UP: i32 = 0xff55;
  pub const PAGE_DOWN: i32 = 0xff56;
  pub const END: i32 = 0xff57;
  pub const INSERT: i32 = 0xff63;
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
      return Err(invalid_input(format!("invalid key {raw}; use a special key, shortcut, or type_text for multi-character text")));
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
      _ => Err(invalid_input(format!("linux portal keyboard input only supports ASCII text in this slice; unsupported character {ch:?}"))),
    }
  }

  fn named(raw: &str) -> Option<i32> {
    let normalized = raw.to_ascii_lowercase();
    if let Some(number) =
      normalized.strip_prefix('f').and_then(|number| number.parse::<i32>().ok()).filter(|number| (1..=12).contains(number))
    {
      // Portal keysym values assign F1 through F12 consecutively from 0xffbe.
      return Some(0xffbd + number);
    }
    match normalized.as_str() {
      "return" | "enter" => Some(RETURN),
      "tab" => Some(TAB),
      "escape" | "esc" => Some(ESCAPE),
      "home" => Some(HOME),
      "left" | "arrowleft" => Some(LEFT),
      "up" | "arrowup" => Some(UP),
      "right" | "arrowright" => Some(RIGHT),
      "down" | "arrowdown" => Some(DOWN),
      "pageup" | "page_up" => Some(PAGE_UP),
      "pagedown" | "page_down" => Some(PAGE_DOWN),
      "end" => Some(END),
      "insert" => Some(INSERT),
      "space" => Some(' ' as i32),
      "delete" => Some(DELETE),
      "backspace" | "back" => Some(BACKSPACE),
      "ctrl" | "control" => Some(CONTROL_L),
      "shift" => Some(SHIFT_L),
      "alt" | "option" => Some(ALT_L),
      "super" | "win" | "cmd" | "command" | "meta" => Some(SUPER_L),
      _ => None,
    }
  }
}

#[cfg(test)]
#[path = "input_test.rs"]
mod tests;
