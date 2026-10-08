//! Logical key identities. Native keycodes and delivery support belong to drivers.
use std::str::FromStr;

use crate::error::{DriverError, DriverResult};
pub use xkeysym::Keysym;

/// Portable modifier meaning. Meta maps to Command on macOS and Windows/Super elsewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modifier {
  Shift,
  Control,
  Alt,
  Meta,
}

impl FromStr for Modifier {
  type Err = DriverError;

  fn from_str(raw: &str) -> DriverResult<Self> {
    match raw.trim().to_ascii_lowercase().as_str() {
      "shift" => Ok(Self::Shift),
      "control" | "ctrl" => Ok(Self::Control),
      "alt" | "option" => Ok(Self::Alt),
      "meta" | "cmd" | "command" | "super" | "win" => Ok(Self::Meta),
      _ => Err(DriverError::InvalidInput {
        message: format!("unknown modifier {raw:?}"),
      }),
    }
  }
}

/// A portable modifier or a logical XKB symbol, never a native physical keycode.
/// Parsing a symbol does not establish platform support or a keyboard-layout mapping.
/// TODO(keyboard-physical-code): native physical keycodes remain deferred until
/// an owner approves a portable identity contract; held keys use resolved names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
  Modifier(Modifier),
  Symbol(Keysym),
}

impl FromStr for Key {
  type Err = DriverError;

  fn from_str(raw: &str) -> DriverResult<Self> {
    if let Ok(modifier) = raw.parse() {
      return Ok(Self::Modifier(modifier));
    }
    let symbol = match raw.to_ascii_lowercase().as_str() {
      "return" => Keysym::Return,
      "enter" => Keysym::KP_Enter,
      "tab" => Keysym::Tab,
      "escape" | "esc" => Keysym::Escape,
      "space" => Keysym::space,
      "backspace" | "back" => Keysym::BackSpace,
      "delete" | "forwarddelete" | "forward_delete" => Keysym::Delete,
      "home" => Keysym::Home,
      "end" => Keysym::End,
      "left" | "arrowleft" => Keysym::Left,
      "right" | "arrowright" => Keysym::Right,
      "up" | "arrowup" => Keysym::Up,
      "down" | "arrowdown" => Keysym::Down,
      "pageup" | "page_up" => Keysym::Page_Up,
      "pagedown" | "page_down" => Keysym::Page_Down,
      "insert" => Keysym::Insert,
      "media_play_pause" | "play_pause" => Keysym::XF86_AudioPlay,
      "media_next" | "next_track" => Keysym::XF86_AudioNext,
      "media_prev" | "prev_track" => Keysym::XF86_AudioPrev,
      "media_stop" | "stop" => Keysym::XF86_AudioStop,
      name => {
        if let Some(number) = name.strip_prefix('f').and_then(|number| number.parse::<u32>().ok()).filter(|number| (1..=35).contains(number))
        {
          return Ok(Self::Symbol(Keysym::new(Keysym::F1.raw() + number - 1)));
        }
        let mut chars = raw.chars();
        match (chars.next(), chars.next()) {
          (Some(character), None) => Keysym::from_char(character),
          _ => {
            return Err(DriverError::InvalidInput {
              message: format!("unknown key {raw:?}"),
            });
          }
        }
      }
    };
    Ok(Self::Symbol(symbol))
  }
}

/// Splits a key combination such as `cmd+a` into its keys. A `+` separates
/// keys only when another character follows, so `cmd++` is Command and the
/// plus key, and `+` alone is the plus key. Keys are trimmed; empty parts are
/// dropped. This only splits; keys are validated by the keyboard driver.
pub fn split_key_combination(combination: &str) -> Vec<String> {
  let mut keys = Vec::new();
  let mut current = String::new();
  let mut chars = combination.chars().peekable();
  while let Some(char) = chars.next() {
    if char == '+' && chars.peek().is_some() {
      if !current.trim().is_empty() {
        keys.push(current.trim().to_string());
      }
      current.clear();
    } else {
      current.push(char);
    }
  }
  if !current.trim().is_empty() {
    keys.push(current.trim().to_string());
  }
  keys
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn key_combinations_split_on_plus_unless_it_is_the_last_key() {
    assert_eq!(split_key_combination("cmd+a"), ["cmd", "a"]);
    assert_eq!(split_key_combination(" cmd + shift + a "), ["cmd", "shift", "a"]);
    // A trailing `+` is the plus key, as in the playground and the JS SDK.
    assert_eq!(split_key_combination("cmd++"), ["cmd", "+"]);
    assert_eq!(split_key_combination("+"), ["+"]);
    assert_eq!(split_key_combination("++"), ["+"]);
    assert_eq!(split_key_combination("return"), ["return"]);
  }

  #[test]
  fn aliases_share_modifier_identity() {
    assert_eq!("cmd".parse::<Key>().unwrap(), Key::Modifier(Modifier::Meta));
    assert_eq!("control".parse::<Modifier>().unwrap(), "ctrl".parse::<Modifier>().unwrap());
  }

  #[test]
  fn symbols_preserve_case_and_key_identity() {
    assert_eq!("A".parse::<Key>().unwrap(), Key::Symbol(Keysym::A));
    assert_eq!("a".parse::<Key>().unwrap(), Key::Symbol(Keysym::a));
    assert_eq!("enter".parse::<Key>().unwrap(), Key::Symbol(Keysym::KP_Enter));
    assert_eq!("delete".parse::<Key>().unwrap(), Key::Symbol(Keysym::Delete));
    assert_eq!("F20".parse::<Key>().unwrap(), Key::Symbol(Keysym::F20));
    assert!("61".parse::<Key>().is_err());
  }
}
