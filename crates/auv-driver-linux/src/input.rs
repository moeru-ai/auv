use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::driver::InputBackend;
use crate::driver::LinuxDriverSessionState;
use crate::error::{backend, invalid_input};
use crate::native::portal::{InputSession as PortalSession, PortalInput};
use auv_driver_common::error::{DriverError, DriverResult};
use auv_driver_common::geometry::Point;
use auv_driver_common::input::{
  Click, ClickModifiers, DisturbanceLevel, InputActionResult, InputAttempt, InputDeliveryPath, InputPolicy, KeyPressOptions,
  PasteTextOptions, Scroll, TextSubmit, TypeTextOptions,
};

use crate::clipboard::{restore as restore_clipboard, set_text as set_clipboard_text, snapshot as snapshot_clipboard};

#[derive(Debug)]
pub(crate) enum InputSession {
  Portal(PortalSession),
  #[cfg(target_os = "linux")]
  Uinput(crate::native::uinput::InputSession),
}

impl InputSession {
  pub(crate) fn validate_keys(&self, keys: &[i32]) -> DriverResult<()> {
    match self {
      Self::Portal(_) => {
        let _ = keys;
        Ok(())
      }
      #[cfg(target_os = "linux")]
      Self::Uinput(session) => session.validate_keys(keys),
    }
  }

  fn move_to(&mut self, point: Point) -> DriverResult<()> {
    match self {
      Self::Portal(session) => session.move_to(point),
      #[cfg(target_os = "linux")]
      Self::Uinput(session) => session.move_to(point),
    }
  }
  fn click_at(&mut self, point: Point, click: Click, modifiers: &[i32]) -> DriverResult<()> {
    match self {
      Self::Portal(session) => session.click_at(point, click, modifiers),
      #[cfg(target_os = "linux")]
      Self::Uinput(session) => session.click_at(point, click, modifiers),
    }
  }
  fn scroll_at(&mut self, point: Point, scroll: Scroll) -> DriverResult<()> {
    match self {
      Self::Portal(session) => session.scroll_at(point, scroll),
      #[cfg(target_os = "linux")]
      Self::Uinput(session) => session.scroll_at(point, scroll),
    }
  }
  fn key_press(&mut self, key: i32) -> DriverResult<()> {
    match self {
      Self::Portal(session) => session.key_press(key),
      #[cfg(target_os = "linux")]
      Self::Uinput(session) => session.key_press(key),
    }
  }
  pub(crate) fn key_chord(&mut self, modifiers: &[i32], key: i32) -> DriverResult<()> {
    match self {
      Self::Portal(session) => session.key_chord(modifiers, key),
      #[cfg(target_os = "linux")]
      Self::Uinput(session) => session.key_chord(modifiers, key),
    }
  }
}

pub(crate) fn click_at(
  state: &Arc<Mutex<LinuxDriverSessionState>>,
  point: Point,
  click: Click,
  modifiers: ClickModifiers,
) -> DriverResult<InputActionResult> {
  let keys = click_modifier_keysyms(modifiers);
  with_input_session(state, |session| session.click_at(point, click, &keys))?;
  Ok(pointer_result())
}

fn click_modifier_keysyms(modifiers: ClickModifiers) -> Vec<i32> {
  [
    (modifiers.shift, keysym::SHIFT_L),
    (modifiers.control, keysym::CONTROL_L),
    (modifiers.alt, keysym::ALT_L),
    (modifiers.meta, keysym::SUPER_L),
  ]
  .into_iter()
  .filter_map(|(enabled, key)| enabled.then_some(key))
  .collect()
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

pub(crate) fn with_input_session<T>(
  state: &Arc<Mutex<LinuxDriverSessionState>>,
  operation: impl FnOnce(&mut InputSession) -> DriverResult<T>,
) -> DriverResult<T> {
  let mut state = state.lock().expect("linux driver session state poisoned");
  if state.input_session.is_none() {
    let restore_tokens = state.restore_tokens.clone();
    state.input_session = Some(match state.input_backend {
      InputBackend::Portal => InputSession::Portal(PortalInput::open(restore_tokens.as_ref(), state.portal_app_id.as_deref())?),
      #[cfg(target_os = "linux")]
      InputBackend::Uinput => InputSession::Uinput(crate::native::uinput::InputSession::open()?),
      #[cfg(not(target_os = "linux"))]
      InputBackend::Uinput => return Err(DriverError::unsupported("Linux uinput")),
    });
  }
  let result = operation(state.input_session.as_mut().expect("input session was just initialized"));
  if result.is_err() {
    // A successful input call does not prove semantic delivery. An error drops
    // either backend and never retries the same operation. For Portal, a restored
    // stream still delivers events. Drop a failed session so the next action
    // reopens it through the durable restore-token rotation instead of reusing
    // a stale stream indefinitely.
    state.input_session = None;
  }
  result
}

pub(crate) fn keyboard_result() -> InputActionResult {
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
  let options = auv_driver_common::PressKeysOptions::from(KeyPressOptions {
    key: input.into(),
    ..Default::default()
  });
  let keys = crate::keyboard::combination(&options)?;
  let (key, held) = keys.split_last().expect("validated nonempty keys");
  Ok(KeyChord {
    modifiers: held.to_vec(),
    key: *key,
  })
}

pub(crate) mod keysym {
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
      _ => Err(invalid_input(format!("linux keyboard input only supports ASCII text in this slice; unsupported character {ch:?}"))),
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

/// Scope keyboard transitions to
/// this click and attempt every release, including a press with an uncertain
/// D-Bus reply. Session failure also closes the portal in the owning input API.
pub(crate) fn with_click_modifiers<K: Copy>(
  modifiers: &[K],
  mut key_event: impl FnMut(K, bool) -> DriverResult<()>,
  click: impl FnOnce() -> DriverResult<()>,
) -> DriverResult<()> {
  let mut attempted = 0;
  let mut result = Ok(());
  for key in modifiers {
    attempted += 1;
    result = key_event(*key, true);
    if result.is_err() {
      break;
    }
  }
  if result.is_ok() {
    result = click();
  }
  for key in modifiers[..attempted].iter().rev() {
    result = combine_release(result, key_event(*key, false));
  }
  result
}

pub(crate) fn combine_release(action: DriverResult<()>, release: DriverResult<()>) -> DriverResult<()> {
  match (action, release) {
    (Ok(()), result) | (result, Ok(())) => result,
    (Err(action), Err(release)) => Err(backend(format!("{action}; additionally failed to release input: {release}"))),
  }
}
