//! Resolve requested keysyms using the compositor's actual keyboard layout.
use std::collections::HashMap;
use std::fs::File;
use std::os::unix::fs::FileExt;

use auv_driver_common::error::DriverResult;
use evdev::KeyCode;
use wayland_client::{
  Connection, Dispatch, Proxy, QueueHandle, WEnum,
  globals::{GlobalListContents, registry_queue_init},
  protocol::{wl_keyboard, wl_registry, wl_seat},
};
use xkbcommon::xkb;

use crate::error::{backend, invalid_input};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Stroke {
  pub key: KeyCode,
  pub shift: bool,
}

#[derive(Debug)]
pub(super) struct Keymap(HashMap<i32, Stroke>);

impl Keymap {
  pub fn load() -> DriverResult<Self> {
    let connection = Connection::connect_to_env().map_err(|error| backend(format!("connect to Wayland keyboard: {error}")))?;
    let (globals, mut queue) = registry_queue_init::<KeyboardState>(&connection).map_err(|error| backend(error.to_string()))?;
    let qh = queue.handle();
    let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=8, ()).map_err(|error| backend(format!("bind Wayland seat: {error}")))?;
    let keyboard = seat.get_keyboard(&qh, ());
    let mut state = KeyboardState::default();
    queue.roundtrip(&mut state).map_err(|error| backend(format!("read Wayland keymap: {error}")))?;
    if keyboard.version() >= 3 {
      keyboard.release();
    }
    if seat.version() >= 5 {
      seat.release();
    }
    let text = state.keymap.ok_or_else(|| backend("compositor did not provide an XKB keymap"))??;
    let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
    let keymap = xkb::Keymap::new_from_string(&context, text, xkb::KEYMAP_FORMAT_TEXT_V1, xkb::COMPILE_NO_FLAGS)
      .ok_or_else(|| backend("compositor returned an invalid XKB keymap"))?;
    Self::from_xkb(&keymap)
  }

  fn from_xkb(map: &xkb::Keymap) -> DriverResult<Self> {
    // TODO: Without focused-seat group state, only mappings identical in every
    // layout are safe. Group-specific keys await a layout tracking contract.
    let mut shared = Self::for_layout(map, 0);
    for layout in 1..map.num_layouts() {
      let other = Self::for_layout(map, layout);
      shared.0.retain(|symbol, stroke| other.0.get(symbol) == Some(stroke));
    }
    Ok(shared)
  }

  fn for_layout(map: &xkb::Keymap, layout: u32) -> Self {
    let shift = map.mod_get_index(xkb::MOD_NAME_SHIFT);
    let shift_mask = 1_u32.checked_shl(shift).unwrap_or(0);
    let mut strokes = HashMap::new();
    for code in map.min_keycode().raw()..=map.max_keycode().raw() {
      // XKB's evdev rules use the kernel code plus eight.
      let Some(evdev_code) = code.checked_sub(8).and_then(|code| u16::try_from(code).ok()) else {
        continue;
      };
      let key = xkb::Keycode::new(code);
      for level in 0..map.num_levels_for_key(key, layout) {
        let mut masks = [0; 16];
        let count = map.key_get_mods_for_level(key, layout, level, &mut masks);
        let mask = masks[..count].iter().copied().filter(|mask| *mask == 0 || (shift_mask != 0 && *mask == shift_mask)).min();
        let Some(mask) = mask else {
          continue;
        };
        for symbol in map.key_get_syms_by_level(key, layout, level) {
          let stroke = Stroke {
            key: KeyCode(evdev_code),
            shift: mask != 0,
          };
          strokes
            .entry(symbol.raw() as i32)
            .and_modify(|existing: &mut Stroke| {
              if existing.shift && !stroke.shift {
                *existing = stroke;
              }
            })
            .or_insert(stroke);
        }
      }
    }
    Self(strokes)
  }

  pub fn stroke(&self, symbol: i32) -> DriverResult<Stroke> {
    self
      .0
      .get(&symbol)
      .copied()
      .ok_or_else(|| invalid_input(format!("keysym {symbol:#x} has no unshifted/Shift mapping in the current XKB layout")))
  }

  pub fn keys(&self) -> impl Iterator<Item = KeyCode> + '_ {
    self.0.values().map(|stroke| stroke.key)
  }
}

#[derive(Default)]
struct KeyboardState {
  keymap: Option<DriverResult<String>>,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for KeyboardState {
  fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {
  }
}
wayland_client::delegate_noop!(KeyboardState: ignore wl_seat::WlSeat);
impl Dispatch<wl_keyboard::WlKeyboard, ()> for KeyboardState {
  fn event(state: &mut Self, _: &wl_keyboard::WlKeyboard, event: wl_keyboard::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
    if let wl_keyboard::Event::Keymap { format, fd, size } = event {
      state.keymap = Some((|| {
        if format != WEnum::Value(wl_keyboard::KeymapFormat::XkbV1) || size > 4 * 1024 * 1024 {
          return Err(backend("unsupported Wayland keymap format or size"));
        }
        read_keymap(&File::from(fd), size)
      })());
    }
  }
}

/// The compositor may send duplicated descriptors sharing a file offset.
/// Positional reads preserve that offset and allow subsequent connections.
fn read_keymap(file: &File, size: u32) -> DriverResult<String> {
  let mut bytes = vec![0; size as usize];
  file.read_exact_at(&mut bytes, 0).map_err(|error| backend(format!("read XKB keymap fd: {error}")))?;
  let text = String::from_utf8(bytes).map_err(|error| backend(format!("decode XKB keymap: {error}")))?;
  Ok(text.trim_end_matches('\0').to_owned())
}

#[cfg(test)]
mod tests {
  use super::*;
  // ROOT CAUSE: GNOME duplicates a keymap FD with a shared offset. Sequential
  // reads exhausted it, so later connections received an empty keymap.
  #[test]
  fn repeated_reads_ignore_shared_descriptor_offset() {
    use std::io::{Seek, SeekFrom, Write};
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(b"keymap\0").unwrap();
    file.seek(SeekFrom::End(0)).unwrap();
    let duplicate = file.try_clone().unwrap();
    assert_eq!(read_keymap(&file, 7).unwrap(), "keymap");
    assert_eq!(read_keymap(&duplicate, 7).unwrap(), "keymap");
    assert_eq!(file.stream_position().unwrap(), 7);
  }

  #[test]
  fn multiple_layouts_only_allow_agreed_strokes() {
    let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
    let map = xkb::Keymap::new_from_names(&context, "", "", "us,de", "", None, xkb::COMPILE_NO_FLAGS).unwrap();
    let map = Keymap::from_xkb(&map).unwrap();
    assert_eq!(map.stroke('a' as i32).unwrap().key, KeyCode::KEY_A);
    assert!(map.stroke('y' as i32).is_err());
  }

  #[test]
  fn character_mapping_respects_compositor_layout_and_shift() {
    let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
    let us = xkb::Keymap::new_from_names(&context, "", "", "us", "", None, xkb::COMPILE_NO_FLAGS).unwrap();
    let de = xkb::Keymap::new_from_names(&context, "", "", "de", "", None, xkb::COMPILE_NO_FLAGS).unwrap();
    let us = Keymap::from_xkb(&us).unwrap();
    let de = Keymap::from_xkb(&de).unwrap();
    assert_eq!(us.stroke('y' as i32).unwrap().key, KeyCode::KEY_Y);
    assert_eq!(de.stroke('y' as i32).unwrap().key, KeyCode::KEY_Z);
    assert_eq!(
      us.stroke('A' as i32).unwrap(),
      Stroke {
        key: KeyCode::KEY_A,
        shift: true
      }
    );
    assert_eq!(
      us.stroke('!' as i32).unwrap(),
      Stroke {
        key: KeyCode::KEY_1,
        shift: true
      }
    );
    assert!(us.stroke(0x1008ffff).is_err());
  }
}
