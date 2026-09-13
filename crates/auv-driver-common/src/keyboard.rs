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
/// TODO: physical keycodes and persistent holds need a separate approved delivery contract.
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

#[cfg(test)]
mod tests {
  use super::*;

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
