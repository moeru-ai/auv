use std::time::Duration;

use ashpd::desktop::remote_desktop::{DeviceType, RemoteDesktop, SelectDevicesOptions};
use ashpd::desktop::{PersistMode, Session};
use ashpd::enumflags2::BitFlags;
use auv_driver_common::error::DriverResult;
use auv_driver_common::geometry::Point;
use auv_driver_common::input::{Click, MouseButton, Scroll};
use evdev::KeyCode;

use crate::error::{backend, invalid_input};
use crate::input::{combine_release, with_held_keys};
use crate::native::eis::EisInput;
use crate::native::keymap::Keymap;

use super::persistence::{RestoreTokenKind, RestoreTokenStore};
use super::request::{run, run_with_consent, session_connection};

// NOTICE(portal-pointer-settle): games may update hover hit-testing only once
// per rendered frame. Let an absolute EIS motion survive one 60 Hz frame
// before pressing so the click is dispatched to the newly hovered control.
const POINTER_SETTLE_DURATION: Duration = Duration::from_millis(20);
// NOTICE(portal-game-click-pulse): some games sample mouse state once per
// rendered frame. Hold the EIS button across two 60 Hz frames so a delivered
// click is observable even when press begins just after a frame boundary.
const CLICK_PRESS_DURATION: Duration = Duration::from_millis(34);

pub struct InputSession {
  session: Session<RemoteDesktop>,
  devices: BitFlags<DeviceType>,
  eis: EisInput,
  wheel_remainder: (f64, f64),
}

impl std::fmt::Debug for InputSession {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter.debug_struct("InputSession").field("session", &self.session).field("devices", &self.devices).finish()
  }
}

impl InputSession {
  pub fn open(restore_tokens: Option<&RestoreTokenStore>, app_id: Option<&ashpd::AppID>) -> DriverResult<Self> {
    let connection = session_connection(app_id)?;
    let remote_desktop = run("open RemoteDesktop", RemoteDesktop::with_connection(connection))?;
    let session = run("create input session", remote_desktop.create_session(Default::default()))?;
    let persistent = restore_tokens.is_some() && remote_desktop.version() >= 2;
    let start = |restore: Option<&str>| {
      run_with_consent("start input session", async {
        let mut options = SelectDevicesOptions::default().set_devices(DeviceType::Keyboard | DeviceType::Pointer);
        if persistent {
          options = options.set_persist_mode(PersistMode::ExplicitlyRevoked).set_restore_token(restore);
        }
        remote_desktop.select_devices(&session, options).await?.response()?;
        remote_desktop.start(&session, None, Default::default()).await?.response()
      })
    };
    let selected = if let Some(store) = restore_tokens.filter(|_| persistent) {
      store.rotate(RestoreTokenKind::RemoteDesktopInput, |current| {
        let selected = start(current)?;
        let replacement = selected.restore_token().map(str::to_owned);
        Ok((selected, replacement))
      })?
    } else {
      start(None)?
    };
    let devices = selected.devices();
    if !devices.intersects(DeviceType::Keyboard | DeviceType::Pointer) {
      return Err(backend("remote desktop portal started without keyboard or pointer access"));
    }
    let fd = run("connect to EIS", remote_desktop.connect_to_eis(&session, Default::default()))?;
    let eis = EisInput::open(fd, devices.contains(DeviceType::Keyboard), devices.contains(DeviceType::Pointer))?;
    Ok(Self {
      session,
      devices,
      eis,
      wheel_remainder: (0.0, 0.0),
    })
  }

  pub fn key_transition(&self, key: KeyCode, down: bool) -> DriverResult<()> {
    self.require_keyboard()?;
    self.eis.key_transition(key, down)
  }

  pub fn key_chord(&mut self, layout: &Keymap, modifiers: &[i32], key: i32) -> DriverResult<()> {
    self.require_keyboard()?;
    let presses = layout.chord_keys(modifiers, key)?;
    let mut attempted = 0;
    let mut result = Ok(());
    for key in &presses {
      attempted += 1;
      result = self.eis.key_transition(*key, true);
      if result.is_err() {
        break;
      }
    }
    for key in presses[..attempted].iter().rev() {
      result = combine_release(result, self.eis.key_transition(*key, false));
    }
    result
  }

  pub fn click_at(&mut self, point: Point, button: MouseButton, click: Click, modifiers: &[i32]) -> DriverResult<()> {
    let (count, interval) = click_parts(&click)?;
    self.require_pointer()?;
    if !modifiers.is_empty() {
      self.require_keyboard()?;
    }
    let modifier_keys = if modifiers.is_empty() {
      Vec::new()
    } else {
      self.eis.keymap()?.held_keys(modifiers)?
    };
    self.eis.move_to(point)?;
    std::thread::sleep(POINTER_SETTLE_DURATION);
    with_held_keys(
      &modifier_keys,
      |key, pressed| self.eis.key_transition(key, pressed),
      || {
        for index in 0..count {
          // A failed reply may follow delivery: attempt release even when the
          // press reports an error, before unwinding modifier state.
          let press = self.eis.button(point, button, true);
          if press.is_ok() {
            std::thread::sleep(CLICK_PRESS_DURATION);
          }
          let release = self.eis.button(point, button, false);
          combine_release(press, release)?;
          if index + 1 < count && !interval.is_zero() {
            std::thread::sleep(interval);
          }
        }
        Ok(())
      },
    )
  }

  pub fn button(&mut self, point: Point, button: MouseButton, down: bool) -> DriverResult<()> {
    self.require_pointer()?;
    self.eis.button(point, button, down)
  }

  pub fn move_to(&mut self, point: Point) -> DriverResult<()> {
    self.require_pointer()?;
    self.eis.move_to(point)
  }

  pub fn scroll_at(&mut self, point: Point, scroll: Scroll) -> DriverResult<()> {
    self.require_pointer()?;
    let (notches, remainder) = crate::native::wheel_notches(self.wheel_remainder, scroll)?;
    self.eis.move_to(point)?;
    if notches != (0, 0) {
      self.eis.scroll(point, notches)?;
    }
    self.wheel_remainder = remainder;
    Ok(())
  }

  pub fn keymap(&self) -> DriverResult<Keymap> {
    self.require_keyboard()?;
    self.eis.keymap()
  }

  pub fn validate_keys(&self, layout: &Keymap, keys: &[i32]) -> DriverResult<()> {
    for key in keys {
      layout.stroke(*key)?;
    }
    Ok(())
  }

  fn require_keyboard(&self) -> DriverResult<()> {
    if !self.devices.contains(DeviceType::Keyboard) {
      Err(backend("remote desktop portal session has no keyboard access"))
    } else {
      Ok(())
    }
  }

  fn require_pointer(&self) -> DriverResult<()> {
    if !self.devices.contains(DeviceType::Pointer) {
      Err(backend("remote desktop portal session has no pointer access"))
    } else {
      Ok(())
    }
  }
}

impl Drop for InputSession {
  fn drop(&mut self) {
    self.eis.shutdown();
    let _ = run("close input session", self.session.close());
  }
}

fn click_parts(click: &Click) -> DriverResult<(u8, Duration)> {
  let count = click.count();
  if count == 0 {
    return Err(invalid_input("repeated click count must be greater than zero"));
  }
  Ok((count, click.interval().unwrap_or(Duration::ZERO)))
}

#[cfg(test)]
#[path = "input_test.rs"]
mod tests;
