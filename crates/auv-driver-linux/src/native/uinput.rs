//! Foreground input through named virtual devices; never reads physical inputs.
use std::cell::RefCell;
use std::thread;
use std::time::Duration;

use auv_driver_common::{
  DriverResult,
  geometry::{Point, Rect},
  input::{Click, Scroll},
};
use evdev::{
  AbsInfo, AbsoluteAxisCode, AttributeSet, EventType, InputEvent, KeyCode, PropType, RelativeAxisCode, UinputAbsSetup, uinput::VirtualDevice,
};

use super::keymap::Keymap;
use crate::{
  capture::list_displays,
  error::{backend, invalid_input},
  input::{combine_release, keysym, with_click_modifiers},
};

#[derive(Debug)]
pub(crate) struct InputSession {
  device: VirtualDevice,
  keymap: Keymap,
  wheel_remainder: (f64, f64),
}

impl InputSession {
  pub fn open() -> DriverResult<Self> {
    let keymap = Keymap::load()?;
    let keys: AttributeSet<KeyCode> = keymap.keys().chain([KeyCode::BTN_LEFT, KeyCode::BTN_RIGHT, KeyCode::BTN_MIDDLE]).collect();
    let axes: AttributeSet<RelativeAxisCode> = [RelativeAxisCode::REL_WHEEL, RelativeAxisCode::REL_HWHEEL].into_iter().collect();
    let properties: AttributeSet<PropType> = [PropType::POINTER].into_iter().collect();
    let device = VirtualDevice::builder()
      .and_then(|builder| {
        builder
          .name("AUV Input")
          .with_keys(&keys)?
          .with_relative_axes(&axes)?
          .with_properties(&properties)?
          .with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode::ABS_X, AbsInfo::new(0, 0, 65535, 0, 0, 0)))?
          .with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode::ABS_Y, AbsInfo::new(0, 0, 65535, 0, 0, 0)))?
          .build()
      })
      .map_err(|error| backend(format!("create AUV input through /dev/uinput: {error}; grant this user access to /dev/uinput")))?;
    // NOTICE: uinput creation precedes udev/libinput discovery. There is no
    // compositor-neutral device-ready acknowledgement. Allow discovery before
    // the first event, as described in docs.kernel.org/input/uinput.html.
    // Remove this delay when a compositor acknowledgement is available.
    thread::sleep(Duration::from_millis(250));
    Ok(Self {
      device,
      keymap,
      wheel_remainder: (0.0, 0.0),
    })
  }

  pub fn move_to(&mut self, point: Point) -> DriverResult<()> {
    let displays = list_displays()?.displays;
    // TODO: per-output absolute-device mapping differs across compositors.
    // Multi-output delivery awaits an explicit mapping contract and live tests.
    if displays.len() != 1 {
      return Err(invalid_input("uinput absolute pointer currently requires one Wayland output"));
    }
    let events = motion_events(point, displays[0].frame)?;
    self.device.emit(&events).map_err(|error| backend(format!("move uinput pointer: {error}")))
  }

  pub fn click_at(&mut self, point: Point, click: Click, modifiers: &[i32]) -> DriverResult<()> {
    if click.count() == 0 {
      return Err(invalid_input("repeated click count must be greater than zero"));
    }
    let modifiers = self.modifier_codes(modifiers)?;
    self.move_to(point)?;
    thread::sleep(Duration::from_millis(20));
    // Keep keyboard and pointer events on the same kernel device stream.
    let device = RefCell::new(&mut self.device);
    with_click_modifiers(
      &modifiers,
      |key, pressed| {
        let result = emit_key(&mut device.borrow_mut(), key, pressed);
        // NOTICE: Mutter can dispatch a pointer frame before pending keyboard
        // modifiers, even on one evdev device. Live GTK probes reproduced this.
        // Allow modifier propagation before the click or next pointer action;
        // remove when a compositor input-state acknowledgement is available.
        thread::sleep(Duration::from_millis(40));
        result
      },
      || {
        for index in 0..click.count() {
          let press = emit_key(&mut device.borrow_mut(), KeyCode::BTN_LEFT, true);
          if press.is_ok() {
            thread::sleep(Duration::from_millis(34));
          }
          let release = emit_key(&mut device.borrow_mut(), KeyCode::BTN_LEFT, false);
          combine_release(press, release)?;
          if index + 1 < click.count() {
            thread::sleep(click.interval().unwrap_or_default());
          }
        }
        Ok(())
      },
    )
  }

  pub fn validate_keys(&self, keys: &[i32]) -> DriverResult<()> {
    let map = Keymap::load()?;
    for key in keys {
      map.stroke(*key)?;
    }
    Ok(())
  }

  pub fn key_press(&mut self, key: i32) -> DriverResult<()> {
    self.key_chord(&[], key)
  }

  pub fn key_chord(&mut self, modifiers: &[i32], key: i32) -> DriverResult<()> {
    // Refresh the map between chords so a changed layout is not silently stale.
    // NOTICE: lock states (Caps/Num Lock), compose and IME text are not modeled;
    // this backend delivers key events, with semantic verification kept separate.
    self.keymap = Keymap::load()?;
    let stroke = self.keymap.stroke(key)?;
    let held = modifiers.iter().map(|key| self.keymap.stroke(*key)).collect::<DriverResult<Vec<_>>>()?;
    let needs_shift = stroke.shift || held.iter().any(|stroke| stroke.shift);
    let mut modifiers = held.iter().map(|stroke| stroke.key).collect::<Vec<_>>();
    if needs_shift {
      let shift = self.keymap.stroke(keysym::SHIFT_L)?.key;
      if !modifiers.contains(&shift) {
        modifiers.insert(0, shift);
      }
    }
    let (presses, releases) = chord_events(&modifiers, stroke.key);
    let press = self.device.emit(&presses).map_err(|error| backend(format!("press uinput chord: {error}")));
    let release = self.device.emit(&releases).map_err(|error| backend(format!("release uinput chord: {error}")));
    combine_release(press, release)
  }

  pub fn scroll_at(&mut self, point: Point, scroll: Scroll) -> DriverResult<()> {
    if !scroll.delta_x.is_finite() || !scroll.delta_y.is_finite() {
      return Err(invalid_input("scroll delta must be finite"));
    }
    self.move_to(point)?;
    // Portal-style continuous scroll uses 15 units per wheel notch in this
    // backend. Keep fractional remainders so repeated small deltas accumulate.
    let x = self.wheel_remainder.0 + scroll.delta_x / 15.0;
    let y = self.wheel_remainder.1 - scroll.delta_y / 15.0;
    if x.abs() > f64::from(i32::MAX) || y.abs() > f64::from(i32::MAX) {
      return Err(invalid_input("scroll delta exceeds uinput range"));
    }
    let events = [
      InputEvent::new(EventType::RELATIVE.0, RelativeAxisCode::REL_HWHEEL.0, x as i32),
      InputEvent::new(EventType::RELATIVE.0, RelativeAxisCode::REL_WHEEL.0, y as i32),
    ];
    self.device.emit(&events).map_err(|error| backend(format!("scroll uinput pointer: {error}")))?;
    self.wheel_remainder = (x.fract(), y.fract());
    Ok(())
  }

  fn modifier_codes(&self, symbols: &[i32]) -> DriverResult<Vec<KeyCode>> {
    symbols.iter().map(|symbol| self.keymap.stroke(*symbol).map(|stroke| stroke.key)).collect()
  }
}

fn emit_key(device: &mut VirtualDevice, key: KeyCode, pressed: bool) -> DriverResult<()> {
  device
    .emit(&[InputEvent::new(EventType::KEY.0, key.0, i32::from(pressed))])
    .map_err(|error| backend(format!("emit uinput key {}: {error}", key.0)))
}

fn chord_events(modifiers: &[KeyCode], key: KeyCode) -> (Vec<InputEvent>, Vec<InputEvent>) {
  let mut codes = modifiers.to_vec();
  if !codes.contains(&key) {
    codes.push(key);
  }
  (
    codes.iter().map(|key| InputEvent::new(EventType::KEY.0, key.0, 1)).collect(),
    codes.iter().rev().map(|key| InputEvent::new(EventType::KEY.0, key.0, 0)).collect(),
  )
}

fn motion_events(point: Point, bounds: Rect) -> DriverResult<[InputEvent; 2]> {
  let x = (point.x - bounds.origin.x) / bounds.size.width;
  let y = (point.y - bounds.origin.y) / bounds.size.height;
  if bounds.size.width <= 0.0
    || bounds.size.height <= 0.0
    || !x.is_finite()
    || !y.is_finite()
    || !(0.0..=1.0).contains(&x)
    || !(0.0..=1.0).contains(&y)
  {
    return Err(invalid_input("uinput pointer target is outside the Wayland output"));
  }
  Ok([
    InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, (x * 65535.0).round() as i32),
    InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Y.0, (y * 65535.0).round() as i32),
  ])
}

#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn absolute_events_map_logical_output_origin_and_reject_nonfinite_points() {
    let bounds = Rect::new(-800.0, 100.0, 800.0, 600.0);
    let events = motion_events(Point::new(-400.0, 250.0), bounds).unwrap();
    assert_eq!(events[0].value(), 32768);
    assert_eq!(events[1].value(), 16384);
    assert!(motion_events(Point::new(f64::NAN, 250.0), bounds).is_err());
    assert!(motion_events(Point::new(1.0, 250.0), bounds).is_err());
  }
  #[test]
  fn chord_batches_release_modifiers_in_reverse_without_duplicate_keys() {
    let (press, release) = chord_events(&[KeyCode::KEY_LEFTCTRL, KeyCode::KEY_LEFTSHIFT], KeyCode::KEY_A);
    assert_eq!(press.iter().map(|event| (event.code(), event.value())).collect::<Vec<_>>(), [(29, 1), (42, 1), (30, 1)]);
    assert_eq!(release.iter().map(|event| (event.code(), event.value())).collect::<Vec<_>>(), [(30, 0), (42, 0), (29, 0)]);
    assert_eq!(chord_events(&[KeyCode::KEY_LEFTSHIFT], KeyCode::KEY_LEFTSHIFT).0.len(), 1);
  }
}
