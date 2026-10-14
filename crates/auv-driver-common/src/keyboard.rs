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
    let symbol = match normalized_key_name(raw).as_str() {
      "return" => Keysym::Return,
      "enter" => Keysym::KP_Enter,
      "tab" => Keysym::Tab,
      "escape" | "esc" => Keysym::Escape,
      "space" => Keysym::space,
      "backspace" | "back" => Keysym::BackSpace,
      "delete" | "forwarddelete" => Keysym::Delete,
      "home" => Keysym::Home,
      "end" => Keysym::End,
      "left" | "arrowleft" | "leftarrow" => Keysym::Left,
      "right" | "arrowright" | "rightarrow" => Keysym::Right,
      "up" | "arrowup" | "uparrow" => Keysym::Up,
      "down" | "arrowdown" | "downarrow" => Keysym::Down,
      "pageup" | "pgup" => Keysym::Page_Up,
      "pagedown" | "pgdn" => Keysym::Page_Down,
      "insert" => Keysym::Insert,
      "menu" | "apps" | "contextmenu" => Keysym::Menu,
      "print" | "printscreen" | "prtsc" => Keysym::Print,
      "pause" | "break" => Keysym::Pause,
      "capslock" => Keysym::Caps_Lock,
      "numlock" => Keysym::Num_Lock,
      "scrolllock" => Keysym::Scroll_Lock,
      "mediaplaypause" | "playpause" => Keysym::XF86_AudioPlay,
      "medianext" | "nexttrack" => Keysym::XF86_AudioNext,
      "mediaprev" | "prevtrack" => Keysym::XF86_AudioPrev,
      "mediastop" | "stop" => Keysym::XF86_AudioStop,
      // Spelled-out punctuation, as in X keysym and pyautogui names. Models
      // and shell callers use these where the literal character is awkward,
      // for example `ctrl+comma` or `ctrl+plus`.
      "comma" => Keysym::comma,
      "period" | "dot" => Keysym::period,
      "slash" => Keysym::slash,
      "backslash" => Keysym::backslash,
      "minus" | "hyphen" => Keysym::minus,
      "plus" => Keysym::plus,
      "equal" | "equals" => Keysym::equal,
      "semicolon" => Keysym::semicolon,
      "colon" => Keysym::colon,
      "apostrophe" | "quote" => Keysym::apostrophe,
      "quotedbl" | "doublequote" => Keysym::quotedbl,
      "grave" | "backtick" | "backquote" => Keysym::grave,
      "bracketleft" | "leftbracket" => Keysym::bracketleft,
      "bracketright" | "rightbracket" => Keysym::bracketright,
      "braceleft" | "leftbrace" => Keysym::braceleft,
      "braceright" | "rightbrace" => Keysym::braceright,
      "parenleft" | "leftparen" => Keysym::parenleft,
      "parenright" | "rightparen" => Keysym::parenright,
      "less" | "lessthan" => Keysym::less,
      "greater" | "greaterthan" => Keysym::greater,
      "question" | "questionmark" => Keysym::question,
      "exclam" | "exclamation" => Keysym::exclam,
      "at" => Keysym::at,
      "numbersign" | "hash" => Keysym::numbersign,
      "dollar" => Keysym::dollar,
      "percent" => Keysym::percent,
      "asciicircum" | "caret" => Keysym::asciicircum,
      "ampersand" => Keysym::ampersand,
      "asterisk" | "star" => Keysym::asterisk,
      "underscore" => Keysym::underscore,
      "bar" | "pipe" => Keysym::bar,
      "asciitilde" | "tilde" => Keysym::asciitilde,
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

/// Lowercases a multi-character key name and drops `_`, `-`, and spaces, so
/// `Page_Down`, `page-down`, and `ArrowRight` share one spelling. A single
/// character is returned as is: `-` and `_` are keys themselves.
fn normalized_key_name(raw: &str) -> String {
  if raw.chars().nth(1).is_none() {
    return raw.to_string();
  }
  raw.chars().filter(|char| !matches!(char, '_' | '-' | ' ')).flat_map(char::to_lowercase).collect()
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

/// Whether an application id or name belongs to a terminal emulator that
/// pastes on Ctrl+Shift+V. In those terminals Ctrl+V is the shell's
/// quoted-insert, so a Ctrl+V paste inserts nothing.
///
/// VTE terminals (GNOME Terminal, Console, Ptyxis, Tilix, Terminator,
/// xfce4-terminal), Konsole, kitty, Alacritty and WezTerm paste on
/// Ctrl+Shift+V. xterm and urxvt bind neither shortcut by default.
// NOTICE(terminal-paste): matched by application id/name substrings, as
// cua-driver's `terminal.rs` does; extend the list with evidence from a
// terminal that pastes on Ctrl+Shift+V but is not recognized.
pub fn is_terminal_emulator(application: &str) -> bool {
  let application = application.to_ascii_lowercase();
  [
    "terminal",
    "org.gnome.console",
    "kgx",
    "ptyxis",
    "tilix",
    "terminator",
    "konsole",
    "kitty",
    "alacritty",
    "wezterm",
  ]
  .iter()
  .any(|name| application.contains(name))
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

  // ROOT CAUSE:
  //
  // If a model asked for `ctrl+comma` or `arrow_right`, the key failed with
  // "unknown key" because only literal characters and a few camel-case names
  // were accepted. OSWorld runs lost steps on `comma` and `arrowright`.
  //
  // The fix accepts spelled-out punctuation and ignores case, `_`, `-`, and
  // spaces in multi-character names.
  #[test]
  fn spelled_out_punctuation_and_separator_variants_resolve_to_one_symbol() {
    assert_eq!("comma".parse::<Key>().unwrap(), Key::Symbol(Keysym::comma));
    assert_eq!("Period".parse::<Key>().unwrap(), Key::Symbol(Keysym::period));
    assert_eq!("plus".parse::<Key>().unwrap(), Key::Symbol(Keysym::plus));
    assert_eq!("bracket_left".parse::<Key>().unwrap(), Key::Symbol(Keysym::bracketleft));
    for spelling in [
      "right",
      "arrowright",
      "ArrowRight",
      "arrow_right",
      "arrow-right",
      "right_arrow",
    ] {
      assert_eq!(spelling.parse::<Key>().unwrap(), Key::Symbol(Keysym::Right), "{spelling}");
    }
    for spelling in ["pagedown", "page_down", "Page-Down", "pgdn"] {
      assert_eq!(spelling.parse::<Key>().unwrap(), Key::Symbol(Keysym::Page_Down), "{spelling}");
    }
    assert_eq!("media_play_pause".parse::<Key>().unwrap(), Key::Symbol(Keysym::XF86_AudioPlay));
    // Single characters stay literal keys, including the separators themselves.
    assert_eq!("-".parse::<Key>().unwrap(), Key::Symbol(Keysym::minus));
    assert_eq!("_".parse::<Key>().unwrap(), Key::Symbol(Keysym::underscore));
  }
}
