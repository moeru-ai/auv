use std::time::Duration;

use ashpd::desktop::remote_desktop::{DeviceType, KeyState, NotifyPointerAxisOptions, RemoteDesktop, SelectDevicesOptions};
use ashpd::desktop::screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType};
use ashpd::desktop::{PersistMode, Session};
use ashpd::enumflags2::BitFlags;
use auv_driver_common::display::Display;
use auv_driver_common::error::DriverResult;
use auv_driver_common::geometry::{Point, Rect};
use auv_driver_common::input::{Click, MouseButton, Scroll};

use crate::capture::list_displays;
use crate::error::{backend, invalid_input};
use crate::input::{combine_release, with_click_modifiers};

use super::ScreenCastStream;
use super::persistence::{RestoreTokenKind, RestoreTokenStore};
use super::request::{run, session_connection};

const BUTTON_LEFT: i32 = 0x110;
const BUTTON_RIGHT: i32 = 0x111;
const BUTTON_MIDDLE: i32 = 0x112;
// NOTICE(portal-pointer-settle): games may update hover hit-testing only once
// per rendered frame. Let an absolute portal motion survive one 60 Hz frame
// before pressing so the click is dispatched to the newly hovered control.
const POINTER_SETTLE_DURATION: Duration = Duration::from_millis(20);
// NOTICE(portal-game-click-pulse): some games sample mouse state once per
// rendered frame. Hold the portal button across two 60 Hz frames so a delivered
// click is observable even when press begins just after a frame boundary.
const CLICK_PRESS_DURATION: Duration = Duration::from_millis(34);

pub struct PortalInput;

impl PortalInput {
  pub fn open(restore_tokens: Option<&RestoreTokenStore>, app_id: Option<&str>) -> DriverResult<InputSession> {
    InputSession::open(restore_tokens, app_id)
  }
}

pub struct InputSession {
  remote_desktop: RemoteDesktop,
  session: Session<RemoteDesktop>,
  devices: BitFlags<DeviceType>,
  streams: Vec<ScreenCastStream>,
  output_mappings: Vec<OutputMapping>,
}

impl std::fmt::Debug for InputSession {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter
      .debug_struct("InputSession")
      .field("session", &self.session)
      .field("devices", &self.devices)
      .field("streams", &self.streams)
      .field("output_mappings", &self.output_mappings)
      .finish()
  }
}

impl InputSession {
  fn open(restore_tokens: Option<&RestoreTokenStore>, app_id: Option<&str>) -> DriverResult<Self> {
    let connection = session_connection(app_id)?;
    let remote_desktop = run("open RemoteDesktop", RemoteDesktop::with_connection(connection.clone()))?;
    let screencast = run("open ScreenCast", Screencast::with_connection(connection))?;
    let session = run("create input session", remote_desktop.create_session(Default::default()))?;
    let mut input = Self {
      remote_desktop,
      session,
      devices: BitFlags::empty(),
      streams: Vec::new(),
      output_mappings: Vec::new(),
    };
    let persistent = restore_tokens.is_some() && input.remote_desktop.version() >= 2;
    let start = |restore: Option<&str>| {
      run("start input session", async {
        let mut options = SelectDevicesOptions::default().set_devices(DeviceType::Keyboard | DeviceType::Pointer);
        if persistent {
          options = options.set_persist_mode(PersistMode::ExplicitlyRevoked).set_restore_token(restore);
        }
        input.remote_desktop.select_devices(&input.session, options).await?.response()?;
        screencast
          .select_sources(
            &input.session,
            SelectSourcesOptions::default()
              .set_sources(BitFlags::from(SourceType::Monitor))
              .set_multiple(true)
              .set_cursor_mode(CursorMode::Hidden),
          )
          .await?
          .response()?;
        input.remote_desktop.start(&input.session, None, Default::default()).await?.response()
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
    input.streams = selected.streams().iter().map(ScreenCastStream::from).collect();
    if input.streams.is_empty() {
      return Err(backend("remote desktop portal started without screencast streams"));
    }
    input.devices = selected.devices();
    if !input.devices.intersects(DeviceType::Keyboard | DeviceType::Pointer) {
      return Err(backend("remote desktop portal started without keyboard or pointer access"));
    }
    input.output_mappings = remote_desktop_output_mappings(&input.streams).unwrap_or_default();
    Ok(input)
  }

  pub fn key_press(&mut self, keysym: i32) -> DriverResult<()> {
    self.require_keyboard()?;
    self.notify_keyboard_keysym(keysym, KeyState::Pressed)?;
    self.notify_keyboard_keysym(keysym, KeyState::Released)
  }

  pub fn key_chord(&mut self, modifiers: &[i32], key: i32) -> DriverResult<()> {
    self.require_keyboard()?;
    for modifier in modifiers {
      self.notify_keyboard_keysym(*modifier, KeyState::Pressed)?;
    }
    let key_result = self.key_press(key);
    for modifier in modifiers.iter().rev() {
      let _ = self.notify_keyboard_keysym(*modifier, KeyState::Released);
    }
    key_result
  }

  pub fn click_at(&mut self, point: Point, click: Click, modifiers: &[i32]) -> DriverResult<()> {
    let (count, interval) = click_parts(&click)?;
    self.require_pointer()?;
    if !modifiers.is_empty() {
      self.require_keyboard()?;
    }
    self.move_pointer_to(point)?;
    std::thread::sleep(POINTER_SETTLE_DURATION);
    with_click_modifiers(
      modifiers,
      |key, pressed| {
        self.notify_keyboard_keysym(
          key,
          if pressed {
            KeyState::Pressed
          } else {
            KeyState::Released
          },
        )
      },
      || {
        for index in 0..count {
          // A failed D-Bus reply may follow delivery: attempt release even when
          // the press reports an error, before unwinding modifier state.
          let press = self.notify_pointer_button(MouseButton::Left, KeyState::Pressed);
          if press.is_ok() {
            std::thread::sleep(CLICK_PRESS_DURATION);
          }
          let release = self.notify_pointer_button(MouseButton::Left, KeyState::Released);
          combine_release(press, release)?;
          if index + 1 < count && !interval.is_zero() {
            std::thread::sleep(interval);
          }
        }
        Ok(())
      },
    )
  }

  pub fn move_to(&mut self, point: Point) -> DriverResult<()> {
    self.require_pointer()?;
    self.move_pointer_to(point)
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
    run("notify relative pointer motion", self.remote_desktop.notify_pointer_motion(&self.session, delta.x, delta.y, Default::default()))
  }

  pub fn scroll_at(&mut self, point: Point, scroll: Scroll) -> DriverResult<()> {
    self.require_pointer()?;
    self.move_pointer_to(point)?;
    self.scroll(scroll)
  }

  fn scroll(&self, scroll: Scroll) -> DriverResult<()> {
    run("notify pointer axis", async {
      self.remote_desktop.notify_pointer_axis(&self.session, scroll.delta_x, scroll.delta_y, Default::default()).await?;
      self.remote_desktop.notify_pointer_axis(&self.session, 0.0, 0.0, NotifyPointerAxisOptions::default().set_finish(true)).await
    })
  }

  fn notify_keyboard_keysym(&self, keysym: i32, state: KeyState) -> DriverResult<()> {
    run("notify keyboard keysym", self.remote_desktop.notify_keyboard_keysym(&self.session, keysym, state, Default::default()))
  }

  fn notify_pointer_motion_absolute(&self, stream: u32, point: Point) -> DriverResult<()> {
    run(
      "notify absolute pointer motion",
      self.remote_desktop.notify_pointer_motion_absolute(&self.session, stream, point.x, point.y, Default::default()),
    )
  }

  fn notify_pointer_button(&self, button: MouseButton, state: KeyState) -> DriverResult<()> {
    let button = match button {
      MouseButton::Left => BUTTON_LEFT,
      MouseButton::Right => BUTTON_RIGHT,
      MouseButton::Middle => BUTTON_MIDDLE,
    };
    run("notify pointer button", self.remote_desktop.notify_pointer_button(&self.session, button, state, Default::default()))
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

impl Drop for InputSession {
  fn drop(&mut self) {
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
