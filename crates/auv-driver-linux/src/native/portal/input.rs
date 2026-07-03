use std::collections::HashMap;
use std::time::Duration;

use auv_driver::error::DriverResult;
use auv_driver::geometry::Point;
use auv_driver::input::{Click, MouseButton, Scroll};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

use crate::error::backend;

use super::request::{
  PORTAL_DESTINATION, create_remote_desktop_session, portal_proxy, session_connection,
  session_request,
};

const REMOTE_DESKTOP_INTERFACE: &str = "org.freedesktop.portal.RemoteDesktop";
const DEVICE_KEYBOARD: u32 = 1;
const DEVICE_POINTER: u32 = 2;
const STATE_RELEASED: u32 = 0;
const STATE_PRESSED: u32 = 1;
const BUTTON_LEFT: i32 = 0x110;
const BUTTON_RIGHT: i32 = 0x111;
const BUTTON_MIDDLE: i32 = 0x112;

pub struct PortalInput;

impl PortalInput {
  pub fn open() -> DriverResult<InputSession> {
    InputSession::open()
  }
}

pub struct InputSession {
  connection: Connection,
  session_handle: OwnedObjectPath,
  devices: u32,
}

impl std::fmt::Debug for InputSession {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter
      .debug_struct("InputSession")
      .field("session_handle", &self.session_handle)
      .field("devices", &self.devices)
      .finish()
  }
}

impl InputSession {
  fn open() -> DriverResult<Self> {
    let connection = session_connection()?;
    let session_handle = create_remote_desktop_session(&connection)?;
    let mut options = HashMap::new();
    options.insert("types", Value::from(DEVICE_KEYBOARD | DEVICE_POINTER));
    session_request(
      &connection,
      REMOTE_DESKTOP_INTERFACE,
      "SelectDevices",
      &session_handle,
      options,
    )?;
    let results = start_remote_desktop(&connection, &session_handle)?;
    let devices = results
      .get("devices")
      .and_then(|value| u32::try_from(value).ok())
      .unwrap_or(0);
    if devices & DEVICE_KEYBOARD == 0 && devices & DEVICE_POINTER == 0 {
      return Err(backend(
        "remote desktop portal started without keyboard or pointer access",
      ));
    }
    Ok(Self {
      connection,
      session_handle,
      devices,
    })
  }

  pub fn key_press(&mut self, keysym: i32) -> DriverResult<()> {
    self.require_keyboard()?;
    self.notify_keyboard_keysym(keysym, STATE_PRESSED)?;
    self.notify_keyboard_keysym(keysym, STATE_RELEASED)
  }

  pub fn key_chord(&mut self, modifiers: &[i32], key: i32) -> DriverResult<()> {
    self.require_keyboard()?;
    for modifier in modifiers {
      self.notify_keyboard_keysym(*modifier, STATE_PRESSED)?;
    }
    let key_result = self.key_press(key);
    for modifier in modifiers.iter().rev() {
      let _ = self.notify_keyboard_keysym(*modifier, STATE_RELEASED);
    }
    key_result
  }

  pub fn click_at(&mut self, point: Point, click: Click) -> DriverResult<Option<String>> {
    self.require_pointer()?;
    // TODO(linux-portal-input-stream): Absolute motion ideally needs a
    // ScreenCast stream id and logical-size mapping. GNOME rejects stream `0`
    // as an invalid position in local validation; replace this fallback with
    // explicit ScreenCast mapping when the capture stream slice lands.
    let fallback_reason = match self.notify_pointer_motion_absolute(0, point) {
      Ok(()) => None,
      Err(error) => Some(format!(
        "absolute pointer motion was unavailable ({error}); clicked at current pointer position"
      )),
    };
    let (count, interval) = match click {
      Click::Single => (1, Duration::ZERO),
      Click::Double { interval } => (2, interval),
    };
    for index in 0..count {
      self.notify_pointer_button(MouseButton::Left, STATE_PRESSED)?;
      self.notify_pointer_button(MouseButton::Left, STATE_RELEASED)?;
      if index + 1 < count && !interval.is_zero() {
        std::thread::sleep(interval);
      }
    }
    Ok(fallback_reason)
  }

  pub fn scroll(&mut self, scroll: Scroll) -> DriverResult<()> {
    self.require_pointer()?;
    let options: HashMap<&str, Value<'_>> = HashMap::new();
    self
      .remote_desktop()?
      .call_method(
        "NotifyPointerAxis",
        &(
          &self.session_handle,
          options,
          scroll.delta_x,
          scroll.delta_y,
        ),
      )
      .map_err(|error| backend(format!("failed to notify pointer axis: {error}")))?;
    let mut finish_options = HashMap::new();
    finish_options.insert("finish", Value::from(true));
    self
      .remote_desktop()?
      .call_method(
        "NotifyPointerAxis",
        &(&self.session_handle, finish_options, 0.0_f64, 0.0_f64),
      )
      .map_err(|error| backend(format!("failed to finish pointer axis: {error}")))?;
    Ok(())
  }

  fn notify_keyboard_keysym(&self, keysym: i32, state: u32) -> DriverResult<()> {
    let options: HashMap<&str, Value<'_>> = HashMap::new();
    self
      .remote_desktop()?
      .call_method(
        "NotifyKeyboardKeysym",
        &(&self.session_handle, options, keysym, state),
      )
      .map_err(|error| {
        backend(format!(
          "failed to notify keyboard keysym {keysym}: {error}"
        ))
      })?;
    Ok(())
  }

  fn notify_pointer_motion_absolute(&self, stream: u32, point: Point) -> DriverResult<()> {
    let options: HashMap<&str, Value<'_>> = HashMap::new();
    self
      .remote_desktop()?
      .call_method(
        "NotifyPointerMotionAbsolute",
        &(&self.session_handle, options, stream, point.x, point.y),
      )
      .map_err(|error| {
        backend(format!(
          "failed to notify absolute pointer motion to ({}, {}): {error}",
          point.x, point.y
        ))
      })?;
    Ok(())
  }

  fn notify_pointer_button(&self, button: MouseButton, state: u32) -> DriverResult<()> {
    let button = match button {
      MouseButton::Left => BUTTON_LEFT,
      MouseButton::Right => BUTTON_RIGHT,
      MouseButton::Middle => BUTTON_MIDDLE,
    };
    let options: HashMap<&str, Value<'_>> = HashMap::new();
    self
      .remote_desktop()?
      .call_method(
        "NotifyPointerButton",
        &(&self.session_handle, options, button, state),
      )
      .map_err(|error| backend(format!("failed to notify pointer button {button}: {error}")))?;
    Ok(())
  }

  fn remote_desktop(&self) -> DriverResult<Proxy<'_>> {
    portal_proxy(&self.connection, REMOTE_DESKTOP_INTERFACE)
  }

  fn require_keyboard(&self) -> DriverResult<()> {
    if self.devices & DEVICE_KEYBOARD == 0 {
      Err(backend(
        "remote desktop portal session has no keyboard access",
      ))
    } else {
      Ok(())
    }
  }

  fn require_pointer(&self) -> DriverResult<()> {
    if self.devices & DEVICE_POINTER == 0 {
      Err(backend(
        "remote desktop portal session has no pointer access",
      ))
    } else {
      Ok(())
    }
  }
}

impl Drop for InputSession {
  fn drop(&mut self) {
    let _ = close_session(&self.connection, &self.session_handle);
  }
}

fn start_remote_desktop(
  connection: &Connection,
  session_handle: &OwnedObjectPath,
) -> DriverResult<HashMap<String, OwnedValue>> {
  let handle_token = super::request::portal_token("start");
  let request = super::request::portal_request_proxy(connection, &handle_token)?;
  let mut responses = super::request::response_signal(&request, REMOTE_DESKTOP_INTERFACE, "Start")?;
  let proxy = portal_proxy(connection, REMOTE_DESKTOP_INTERFACE)?;
  let mut options = HashMap::new();
  options.insert("handle_token", Value::from(handle_token.as_str()));
  proxy
    .call_method("Start", &(session_handle, "", options))
    .map_err(|error| {
      backend(format!(
        "failed to start remote desktop portal session: {error}"
      ))
    })?;
  super::request::wait_response(&mut responses, REMOTE_DESKTOP_INTERFACE, "Start")
}

fn close_session(connection: &Connection, session_handle: &OwnedObjectPath) -> DriverResult<()> {
  let session = Proxy::new(
    connection,
    PORTAL_DESTINATION,
    session_handle.clone(),
    "org.freedesktop.portal.Session",
  )
  .map_err(|error| backend(format!("failed to create portal session proxy: {error}")))?;
  session
    .call_method("Close", &())
    .map_err(|error| backend(format!("failed to close portal session: {error}")))?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn device_mask_requests_keyboard_and_pointer() {
    assert_eq!(DEVICE_KEYBOARD | DEVICE_POINTER, 3);
  }

  #[test]
  fn evdev_button_codes_match_primary_buttons() {
    assert_eq!(BUTTON_LEFT, 0x110);
    assert_eq!(BUTTON_RIGHT, 0x111);
    assert_eq!(BUTTON_MIDDLE, 0x112);
  }
}
