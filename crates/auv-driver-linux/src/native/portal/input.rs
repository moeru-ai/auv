use std::collections::HashMap;
use std::time::Duration;

use auv_driver_common::display::Display;
use auv_driver_common::error::DriverResult;
use auv_driver_common::geometry::{Point, Rect};
use auv_driver_common::input::{Click, MouseButton, Scroll};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

use crate::capture::list_displays;
use crate::error::{backend, invalid_input};

use super::persistence::{RestoreTokenKind, RestoreTokenStore};
use super::request::{
  close_session, create_remote_desktop_session, interface_version, portal_proxy, restore_token, session_connection, session_request,
};
use super::{ScreenCastStream, decode_streams, select_monitor_sources};

const REMOTE_DESKTOP_INTERFACE: &str = "org.freedesktop.portal.RemoteDesktop";
const DEVICE_KEYBOARD: u32 = 1;
const DEVICE_POINTER: u32 = 2;
const PERSIST_UNTIL_REVOKED: u32 = 2;
const PERSISTENCE_INTERFACE_VERSION: u32 = 2;
const STATE_RELEASED: u32 = 0;
const STATE_PRESSED: u32 = 1;
const BUTTON_LEFT: i32 = 0x110;
const BUTTON_RIGHT: i32 = 0x111;
const BUTTON_MIDDLE: i32 = 0x112;

pub struct PortalInput;

impl PortalInput {
  pub fn open(restore_tokens: Option<&RestoreTokenStore>) -> DriverResult<InputSession> {
    InputSession::open(restore_tokens)
  }
}

pub struct InputSession {
  connection: Connection,
  session_handle: OwnedObjectPath,
  devices: u32,
  streams: Vec<ScreenCastStream>,
  output_mappings: Vec<OutputMapping>,
}

impl std::fmt::Debug for InputSession {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter
      .debug_struct("InputSession")
      .field("session_handle", &self.session_handle)
      .field("devices", &self.devices)
      .field("streams", &self.streams)
      .field("output_mappings", &self.output_mappings)
      .finish()
  }
}

impl InputSession {
  fn open(restore_tokens: Option<&RestoreTokenStore>) -> DriverResult<Self> {
    let connection = session_connection()?;
    let session_handle = create_remote_desktop_session(&connection)?;
    let result = (|| {
      let persistent =
        restore_tokens.is_some() && interface_version(&connection, REMOTE_DESKTOP_INTERFACE)? >= PERSISTENCE_INTERFACE_VERSION;
      let results = if let Some(restore_tokens) = restore_tokens.filter(|_| persistent) {
        restore_tokens.rotate(RestoreTokenKind::RemoteDesktopInput, |current| {
          select_input_devices(&connection, &session_handle, current, true)?;
          select_monitor_sources(&connection, &session_handle)?;
          let results = start_remote_desktop(&connection, &session_handle)?;
          let replacement = restore_token(&results, REMOTE_DESKTOP_INTERFACE)?;
          Ok((results, replacement))
        })?
      } else {
        select_input_devices(&connection, &session_handle, None, false)?;
        select_monitor_sources(&connection, &session_handle)?;
        start_remote_desktop(&connection, &session_handle)?
      };
      let streams = decode_streams(&results)?;
      if streams.is_empty() {
        return Err(backend("remote desktop portal started without screencast streams"));
      }
      let devices = results.get("devices").and_then(|value| u32::try_from(value).ok()).unwrap_or(0);
      if devices & DEVICE_KEYBOARD == 0 && devices & DEVICE_POINTER == 0 {
        return Err(backend("remote desktop portal started without keyboard or pointer access"));
      }
      let output_mappings = remote_desktop_output_mappings(&streams).unwrap_or_default();
      Ok((devices, streams, output_mappings))
    })();
    match result {
      Ok((devices, streams, output_mappings)) => Ok(Self {
        connection,
        session_handle,
        devices,
        streams,
        output_mappings,
      }),
      Err(error) => {
        let close_result = close_session(&connection, &session_handle);
        match close_result {
          Ok(()) => Err(error),
          Err(close_error) => Err(backend(format!("{error}; also failed to close remote desktop portal session: {close_error}"))),
        }
      }
    }
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

  pub fn click_at(&mut self, point: Point, click: Click) -> DriverResult<()> {
    let (count, interval) = click_parts(&click)?;
    self.require_pointer()?;
    self.move_pointer_to(point)?;
    for index in 0..count {
      self.notify_pointer_button(MouseButton::Left, STATE_PRESSED)?;
      self.notify_pointer_button(MouseButton::Left, STATE_RELEASED)?;
      if index + 1 < count && !interval.is_zero() {
        std::thread::sleep(interval);
      }
    }
    Ok(())
  }

  fn move_pointer_to(&self, point: Point) -> DriverResult<()> {
    let motion = self.resolve_stream_point(point)?;
    debug_input_mapping(|| format!("point {point:?} -> motion {motion:?}"));
    self.notify_pointer_motion_absolute(motion.stream_id, motion.absolute_point)?;
    if !point_is_origin(motion.relative_delta) {
      self.notify_pointer_motion(motion.relative_delta)?;
    }
    Ok(())
  }

  fn notify_pointer_motion(&self, delta: Point) -> DriverResult<()> {
    let options: HashMap<&str, Value<'_>> = HashMap::new();
    self
      .remote_desktop()?
      .call_method("NotifyPointerMotion", &(&self.session_handle, options, delta.x, delta.y))
      .map_err(|error| backend(format!("failed to notify relative pointer motion by ({}, {}): {error}", delta.x, delta.y)))?;
    Ok(())
  }

  pub fn scroll_at(&mut self, point: Point, scroll: Scroll) -> DriverResult<()> {
    self.require_pointer()?;
    self.move_pointer_to(point)?;
    self.scroll(scroll)
  }

  fn scroll(&self, scroll: Scroll) -> DriverResult<()> {
    let options: HashMap<&str, Value<'_>> = HashMap::new();
    self
      .remote_desktop()?
      .call_method("NotifyPointerAxis", &(&self.session_handle, options, scroll.delta_x, scroll.delta_y))
      .map_err(|error| backend(format!("failed to notify pointer axis: {error}")))?;
    let mut finish_options = HashMap::new();
    finish_options.insert("finish", Value::from(true));
    self
      .remote_desktop()?
      .call_method("NotifyPointerAxis", &(&self.session_handle, finish_options, 0.0_f64, 0.0_f64))
      .map_err(|error| backend(format!("failed to finish pointer axis: {error}")))?;
    Ok(())
  }

  fn notify_keyboard_keysym(&self, keysym: i32, state: u32) -> DriverResult<()> {
    let options: HashMap<&str, Value<'_>> = HashMap::new();
    self
      .remote_desktop()?
      .call_method("NotifyKeyboardKeysym", &(&self.session_handle, options, keysym, state))
      .map_err(|error| backend(format!("failed to notify keyboard keysym {keysym}: {error}")))?;
    Ok(())
  }

  fn notify_pointer_motion_absolute(&self, stream: u32, point: Point) -> DriverResult<()> {
    let options: HashMap<&str, Value<'_>> = HashMap::new();
    self
      .remote_desktop()?
      .call_method("NotifyPointerMotionAbsolute", &(&self.session_handle, options, stream, point.x, point.y))
      .map_err(|error| backend(format!("failed to notify absolute pointer motion to ({}, {}): {error}", point.x, point.y)))?;
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
      .call_method("NotifyPointerButton", &(&self.session_handle, options, button, state))
      .map_err(|error| backend(format!("failed to notify pointer button {button}: {error}")))?;
    Ok(())
  }

  fn remote_desktop(&self) -> DriverResult<Proxy<'_>> {
    portal_proxy(&self.connection, REMOTE_DESKTOP_INTERFACE)
  }

  fn require_keyboard(&self) -> DriverResult<()> {
    if self.devices & DEVICE_KEYBOARD == 0 {
      Err(backend("remote desktop portal session has no keyboard access"))
    } else {
      Ok(())
    }
  }

  fn require_pointer(&self) -> DriverResult<()> {
    if self.devices & DEVICE_POINTER == 0 {
      Err(backend("remote desktop portal session has no pointer access"))
    } else {
      Ok(())
    }
  }

  fn resolve_stream_point(&self, point: Point) -> DriverResult<MotionTarget> {
    if let Some(motion) = self.resolve_mapped_stream_point(point) {
      return Ok(motion);
    }
    let Some(stream) = self.streams.iter().find(|stream| stream.contains(point)) else {
      return Err(backend(format!("no screencast stream contains point {:?}; streams={:?}", point, self.streams)));
    };
    Ok(MotionTarget::absolute(stream.id, stream.local_point(point)?))
  }

  fn resolve_mapped_stream_point(&self, point: Point) -> Option<MotionTarget> {
    self
      .output_mappings
      .iter()
      .find(|mapping| rect_contains_point(mapping.logical_rect, point))
      .map(|mapping| mapping.to_motion_target(point))
  }
}

fn select_input_devices(
  connection: &Connection,
  session_handle: &OwnedObjectPath,
  restore: Option<&str>,
  persistent: bool,
) -> DriverResult<()> {
  let mut options = HashMap::new();
  options.insert("types", Value::from(DEVICE_KEYBOARD | DEVICE_POINTER));
  if persistent {
    options.insert("persist_mode", Value::from(PERSIST_UNTIL_REVOKED));
    if let Some(restore) = restore {
      options.insert("restore_token", Value::from(restore));
    }
  }
  session_request(connection, REMOTE_DESKTOP_INTERFACE, "SelectDevices", session_handle, options)?;
  Ok(())
}

impl Drop for InputSession {
  fn drop(&mut self) {
    let _ = close_session(&self.connection, &self.session_handle);
  }
}

fn click_parts(click: &Click) -> DriverResult<(u8, Duration)> {
  let count = click.count();
  if count == 0 {
    return Err(invalid_input("repeated click count must be greater than zero"));
  }
  Ok((count, click.interval().unwrap_or(Duration::ZERO)))
}

fn start_remote_desktop(connection: &Connection, session_handle: &OwnedObjectPath) -> DriverResult<HashMap<String, OwnedValue>> {
  let handle_token = super::request::portal_token("start");
  let request = super::request::portal_request_proxy(connection, &handle_token)?;
  let mut responses = super::request::response_signal(&request, REMOTE_DESKTOP_INTERFACE, "Start")?;
  let proxy = portal_proxy(connection, REMOTE_DESKTOP_INTERFACE)?;
  let mut options = HashMap::new();
  options.insert("handle_token", Value::from(handle_token.as_str()));
  proxy
    .call_method("Start", &(session_handle, "", options))
    .map_err(|error| backend(format!("failed to start remote desktop portal session: {error}")))?;
  super::request::wait_response(&mut responses, REMOTE_DESKTOP_INTERFACE, "Start")
}

#[derive(Clone, Debug, PartialEq)]
struct MotionTarget {
  stream_id: u32,
  absolute_point: Point,
  relative_delta: Point,
}

impl MotionTarget {
  fn absolute(stream_id: u32, absolute_point: Point) -> Self {
    Self {
      stream_id,
      absolute_point,
      relative_delta: Point::new(0.0, 0.0),
    }
  }
}

#[derive(Clone, Debug, PartialEq)]
struct OutputMapping {
  stream_id: u32,
  logical_rect: Rect,
  stream_rect: Rect,
  scale_factor: f64,
}

impl OutputMapping {
  fn to_motion_target(&self, point: Point) -> MotionTarget {
    let local = Point::new(point.x - self.logical_rect.origin.x, point.y - self.logical_rect.origin.y);
    let scaled = Point::new(local.x * self.scale_factor, local.y * self.scale_factor);
    let absolute_point =
      Point::new(clamp(scaled.x, 0.0, self.stream_rect.size.width - 1.0), clamp(scaled.y, 0.0, self.stream_rect.size.height - 1.0));
    let delivered_by_absolute = Point::new(absolute_point.x / self.scale_factor, absolute_point.y / self.scale_factor);
    MotionTarget {
      stream_id: self.stream_id,
      absolute_point,
      relative_delta: Point::new(local.x - delivered_by_absolute.x, local.y - delivered_by_absolute.y),
    }
  }
}

fn remote_desktop_output_mappings(streams: &[ScreenCastStream]) -> DriverResult<Vec<OutputMapping>> {
  let displays = list_displays()?.displays;
  Ok(output_mappings(&displays, streams))
}

fn output_mappings(displays: &[Display], streams: &[ScreenCastStream]) -> Vec<OutputMapping> {
  displays.iter().filter_map(|display| output_mapping(display, streams)).collect()
}

fn output_mapping(display: &Display, streams: &[ScreenCastStream]) -> Option<OutputMapping> {
  // NOTICE(linux-remote-desktop-scaled-motion): GNOME's RemoteDesktop portal can
  // advertise monitor stream geometry in Wayland logical coordinates while
  // `NotifyPointerMotionAbsolute` lands using scaled output coordinates under
  // PaperWM. Absolute motion rejects coordinates outside the advertised logical
  // stream bounds, so large scaled points are delivered as absolute-to-edge plus
  // relative motion. Remove this when portal metadata exposes an explicit
  // motion-coordinate space.
  let matches = streams
    .iter()
    .filter_map(|stream| stream.logical_rect().map(|rect| (stream, rect)))
    .filter(|(_, rect)| same_rect(*rect, display.frame))
    .collect::<Vec<_>>();
  let [(stream, stream_rect)] = matches.as_slice() else {
    return None;
  };
  Some(OutputMapping {
    stream_id: stream.id,
    logical_rect: display.frame,
    stream_rect: *stream_rect,
    scale_factor: display.scale_factor,
  })
}

fn rect_contains_point(rect: Rect, point: Point) -> bool {
  point.x >= rect.origin.x
    && point.y >= rect.origin.y
    && point.x <= rect.origin.x + rect.size.width
    && point.y <= rect.origin.y + rect.size.height
}

fn same_rect(left: Rect, right: Rect) -> bool {
  same_scalar(left.origin.x, right.origin.x)
    && same_scalar(left.origin.y, right.origin.y)
    && same_scalar(left.size.width, right.size.width)
    && same_scalar(left.size.height, right.size.height)
}

fn same_scalar(left: f64, right: f64) -> bool {
  (left - right).abs() <= 0.5
}

fn point_is_origin(point: Point) -> bool {
  same_scalar(point.x, 0.0) && same_scalar(point.y, 0.0)
}

fn clamp(value: f64, min: f64, max: f64) -> f64 {
  value.max(min).min(max)
}

fn debug_input_mapping(message: impl FnOnce() -> String) {
  if std::env::var_os("AUV_LINUX_INPUT_DEBUG").is_some() {
    eprintln!("auv-driver-linux input: {}", message());
  }
}

#[cfg(test)]
#[path = "input_test.rs"]
mod tests;
