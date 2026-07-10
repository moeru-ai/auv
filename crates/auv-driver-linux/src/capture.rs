#[cfg(target_os = "linux")]
use std::sync::{Arc, Mutex};

#[cfg(target_os = "linux")]
use auv_driver_common::capture::Capture;
use auv_driver_common::capture::{DisplayCapture, RegionCapture};
#[cfg(target_os = "linux")]
use auv_driver_common::display::Display;
use auv_driver_common::display::ObservedDisplays;
use auv_driver_common::error::DriverResult;
#[cfg(target_os = "linux")]
use auv_driver_common::geometry::CoordinateSpace;
use auv_driver_common::geometry::Rect;

#[cfg(target_os = "linux")]
use crate::driver::LinuxDriverSessionState;
#[cfg(target_os = "linux")]
use crate::error::backend;
#[cfg(any(target_os = "linux", test))]
use crate::error::invalid_input;
#[cfg(target_os = "linux")]
use crate::native::portal::{ScreenCastFrame, ScreenCastSession};
#[cfg(target_os = "linux")]
use display::{list_targets, resolve_for_region, selected_target_or_none};

mod display;

#[cfg(target_os = "linux")]
const PORTAL_CAPTURE_BACKEND: &str = "xdg-desktop-portal.screenshot";
#[cfg(target_os = "linux")]
const PORTAL_SCREENCAST_BACKEND: &str = "xdg-desktop-portal.screencast.pipewire";

#[cfg(target_os = "linux")]
pub fn list_displays() -> DriverResult<ObservedDisplays> {
  Ok(ObservedDisplays {
    displays: list_targets()?.into_iter().map(|target| target.display).collect(),
  })
}

#[cfg(not(target_os = "linux"))]
pub fn list_displays() -> DriverResult<ObservedDisplays> {
  Err(auv_driver_common::error::DriverError::unsupported("display.list"))
}

#[cfg(target_os = "linux")]
pub fn capture_display(state: &Arc<Mutex<LinuxDriverSessionState>>, selector: Option<&str>) -> DriverResult<DisplayCapture> {
  let target = selected_target_or_none(selector)?;
  let target_bounds = target.as_ref().map(|target| target.display.frame);
  match capture_monitor_frame_for_session(state, target_bounds) {
    Ok(frame) => {
      let display = target.map(|target| target.display).unwrap_or_else(|| display_from_screencast_frame(&frame));
      let scale_factor = capture_scale_factor(&frame.image, display.frame, display.scale_factor);
      let capture = Capture {
        image: frame.image,
        bounds: display.frame,
        scale_factor,
        backend: PORTAL_SCREENCAST_BACKEND.to_string(),
        fallback_reason: None,
      };
      Ok(DisplayCapture { display, capture })
    }
    Err(error) => {
      let captured = match target.as_ref() {
        Some(target) => capture_area(target.display.frame, target.display.frame)?,
        None => capture_full()?,
      };
      capture_display_from_captured(target, with_primary_capture_failure(captured, PORTAL_SCREENCAST_BACKEND, &error.to_string()))
    }
  }
}

#[cfg(target_os = "linux")]
fn capture_display_from_captured(target: Option<display::DisplayTarget>, captured: CapturedImage) -> DriverResult<DisplayCapture> {
  let display = target.map(|target| target.display).unwrap_or_else(|| synthetic_display_from_image(&captured.image));
  let scale_factor = capture_scale_factor(&captured.image, display.frame, display.scale_factor);
  let capture = Capture {
    image: captured.image,
    bounds: display.frame,
    scale_factor,
    backend: captured.backend,
    fallback_reason: captured.fallback_reason,
  };
  Ok(DisplayCapture { display, capture })
}

#[cfg(not(target_os = "linux"))]
pub fn capture_display(
  _state: &std::sync::Arc<std::sync::Mutex<crate::driver::LinuxDriverSessionState>>,
  _selector: Option<&str>,
) -> DriverResult<DisplayCapture> {
  Err(auv_driver_common::error::DriverError::unsupported("display.capture"))
}

#[cfg(target_os = "linux")]
pub fn capture_region(state: &Arc<Mutex<LinuxDriverSessionState>>, selector: Option<&str>, region: Rect) -> DriverResult<RegionCapture> {
  let targets = list_targets()?;
  let target = resolve_for_region(&targets, selector, region)?;
  let captured = match capture_monitor_frame_for_session(state, Some(target.display.frame)) {
    Ok(frame) => CapturedImage {
      image: crop_portal_screenshot_to_region(frame.image, target.display.frame, region)?,
      backend: format!("{PORTAL_SCREENCAST_BACKEND}.crop"),
      fallback_reason: Some("region pixels were cropped from PipeWire screencast using Wayland xdg-output logical bounds".to_string()),
    },
    Err(error) => with_primary_capture_failure(capture_area(region, target.display.frame)?, PORTAL_SCREENCAST_BACKEND, &error.to_string()),
  };
  let scale_factor = capture_scale_factor(&captured.image, region, target.display.scale_factor);
  let capture = Capture {
    image: captured.image,
    bounds: region,
    scale_factor,
    backend: captured.backend,
    fallback_reason: captured.fallback_reason,
  };
  Ok(RegionCapture {
    display: target.display,
    capture,
  })
}

#[cfg(not(target_os = "linux"))]
pub fn capture_region(
  _state: &std::sync::Arc<std::sync::Mutex<crate::driver::LinuxDriverSessionState>>,
  _selector: Option<&str>,
  _region: Rect,
) -> DriverResult<RegionCapture> {
  Err(auv_driver_common::error::DriverError::unsupported("display.capture_region"))
}

struct CapturedImage {
  image: image::RgbaImage,
  backend: String,
  fallback_reason: Option<String>,
}

#[cfg(target_os = "linux")]
fn capture_monitor_frame_for_session(
  state: &Arc<Mutex<LinuxDriverSessionState>>,
  target_bounds: Option<Rect>,
) -> DriverResult<ScreenCastFrame> {
  let mut state = state.lock().expect("linux driver session state poisoned");
  if state.screencast_session.is_none() {
    state.screencast_session = Some(ScreenCastSession::open_monitor()?);
  }
  state.screencast_session.as_mut().expect("screencast session was just initialized").capture_monitor_frame(target_bounds)
}

#[cfg(target_os = "linux")]
fn capture_full() -> DriverResult<CapturedImage> {
  Ok(CapturedImage {
    image: portal_screenshot()?,
    backend: PORTAL_CAPTURE_BACKEND.to_string(),
    fallback_reason: None,
  })
}

#[cfg(target_os = "linux")]
fn capture_area(region: Rect, source_bounds: Rect) -> DriverResult<CapturedImage> {
  Ok(CapturedImage {
    image: crop_portal_screenshot_to_region(portal_screenshot()?, source_bounds, region)?,
    backend: format!("{PORTAL_CAPTURE_BACKEND}.crop"),
    fallback_reason: Some("region pixels were cropped from portal screenshot using Wayland xdg-output logical bounds".to_string()),
  })
}

#[cfg(target_os = "linux")]
fn synthetic_display_from_image(image: &image::RgbaImage) -> Display {
  Display {
    id: "portal-screenshot".to_string(),
    name: Some("XDG desktop portal screenshot".to_string()),
    frame: Rect::new(0.0, 0.0, f64::from(image.width()), f64::from(image.height())),
    coordinate_space: CoordinateSpace::Screen,
    scale_factor: 1.0,
    is_primary: true,
    is_builtin: None,
  }
}

#[cfg(target_os = "linux")]
fn display_from_screencast_frame(frame: &ScreenCastFrame) -> Display {
  let bounds =
    frame.stream.logical_rect().unwrap_or_else(|| Rect::new(0.0, 0.0, f64::from(frame.image.width()), f64::from(frame.image.height())));
  Display {
    id: frame.stream.mapping_id.clone().unwrap_or_else(|| format!("pipewire-stream-{}", frame.stream.id)),
    name: frame.stream.mapping_id.clone(),
    frame: bounds,
    coordinate_space: CoordinateSpace::Screen,
    scale_factor: capture_scale_factor(&frame.image, bounds, 1.0),
    is_primary: true,
    is_builtin: None,
  }
}

#[cfg(target_os = "linux")]
fn with_primary_capture_failure(mut captured: CapturedImage, primary_backend: &str, primary_error: &str) -> CapturedImage {
  let fallback = captured.fallback_reason.take().unwrap_or_else(|| format!("used {PORTAL_CAPTURE_BACKEND} fallback"));
  captured.fallback_reason = Some(format!("{primary_backend} failed ({primary_error}); {fallback}"));
  captured
}

#[cfg(any(target_os = "linux", test))]
fn crop_portal_screenshot_to_region(image: image::RgbaImage, source_bounds: Rect, region: Rect) -> DriverResult<image::RgbaImage> {
  if source_bounds.size.width <= 0.0 || source_bounds.size.height <= 0.0 {
    return Err(invalid_input("source bounds must be positive"));
  }
  let scale_x = f64::from(image.width()) / source_bounds.size.width;
  let scale_y = f64::from(image.height()) / source_bounds.size.height;
  let x = scaled_capture_dimension("x", region.origin.x - source_bounds.origin.x, scale_x)?;
  let y = scaled_capture_dimension("y", region.origin.y - source_bounds.origin.y, scale_y)?;
  let width = scaled_positive_capture_dimension("width", region.size.width, scale_x)?;
  let height = scaled_positive_capture_dimension("height", region.size.height, scale_y)?;
  if x + width > image.width() || y + height > image.height() {
    return Err(invalid_input(format!("region {:?} exceeds portal screenshot bounds {}x{}", region, image.width(), image.height())));
  }
  Ok(image::imageops::crop_imm(&image, x, y, width, height).to_image())
}

#[cfg(target_os = "linux")]
fn capture_scale_factor(image: &image::RgbaImage, bounds: Rect, default: f64) -> f64 {
  if bounds.size.width <= 0.0 {
    return default;
  }
  f64::from(image.width()) / bounds.size.width
}

#[cfg(any(target_os = "linux", test))]
fn scaled_capture_dimension(name: &str, value: f64, scale: f64) -> DriverResult<u32> {
  let value = (value * scale).round();
  if !(0.0..=f64::from(u32::MAX)).contains(&value) {
    return Err(invalid_input(format!("region {name} must be within u32 capture bounds")));
  }
  Ok(value as u32)
}

#[cfg(any(target_os = "linux", test))]
fn scaled_positive_capture_dimension(name: &str, value: f64, scale: f64) -> DriverResult<u32> {
  let value = scaled_capture_dimension(name, value, scale)?;
  if value == 0 {
    return Err(invalid_input(format!("region {name} must be positive")));
  }
  Ok(value)
}

#[cfg(target_os = "linux")]
fn portal_screenshot() -> DriverResult<image::RgbaImage> {
  use serde::Deserialize;
  use std::collections::HashMap;
  use zbus::blocking::Proxy;
  use zbus::zvariant::{OwnedValue, Type, Value};

  #[derive(Deserialize, Type)]
  #[zvariant(signature = "dict")]
  struct ScreenshotResponse {
    uri: OwnedValue,
  }

  let connection = zbus::blocking::Connection::session().map_err(|error| backend(format!("failed to connect to session bus: {error}")))?;
  let handle_token = format!(
    "auv_{}_{}",
    std::process::id(),
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|duration| duration.as_millis()).unwrap_or_default()
  );
  let request = portal_request_proxy(&connection, &handle_token)?;
  let proxy =
    Proxy::new(&connection, "org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop", "org.freedesktop.portal.Screenshot")
      .map_err(|error| backend(format!("failed to create screenshot portal proxy: {error}")))?;
  let mut options = HashMap::new();
  options.insert("handle_token", Value::from(handle_token.as_str()));
  // NOTICE(linux-portal-screenshot): GNOME Wayland does not expose a stable
  // non-portal screenshot API for ordinary clients. Keep this interactive so
  // the compositor/user owns screenshot consent; replace with ScreenCast or
  // PipeWire when the owner approves the capture stream slice.
  options.insert("interactive", Value::from(true));
  options.insert("modal", Value::from(true));
  proxy.call_method("Screenshot", &("", options)).map_err(|error| backend(format!("failed to request portal screenshot: {error}")))?;

  let mut responses =
    request.receive_signal("Response").map_err(|error| backend(format!("failed to subscribe to portal response: {error}")))?;
  let response = responses.next().ok_or_else(|| backend("portal screenshot did not return a response"))?;
  let (code, body): (u32, ScreenshotResponse) =
    response.body().deserialize().map_err(|error| backend(format!("failed to decode portal screenshot response: {error}")))?;
  if code != 0 {
    return Err(backend(format!("portal screenshot response code {code}")));
  }
  let uri = match Value::from(body.uri) {
    Value::Str(uri) => uri.to_string(),
    other => {
      return Err(backend(format!("portal screenshot returned non-string uri: {other:?}")));
    }
  };
  let path = file_uri_to_path(&uri)?;
  let image = image::open(&path).map_err(|error| backend(format!("failed to open portal screenshot {path:?}: {error}")))?.to_rgba8();
  let _ = std::fs::remove_file(path);
  Ok(image)
}

#[cfg(target_os = "linux")]
fn portal_request_proxy<'a>(connection: &'a zbus::blocking::Connection, handle_token: &str) -> DriverResult<zbus::blocking::Proxy<'a>> {
  let unique_name =
    connection.unique_name().ok_or_else(|| backend("session bus connection has no unique name"))?.trim_start_matches(':').replace('.', "_");
  let path = format!("/org/freedesktop/portal/desktop/request/{unique_name}/{handle_token}");
  zbus::blocking::Proxy::new(connection, "org.freedesktop.portal.Desktop", path, "org.freedesktop.portal.Request")
    .map_err(|error| backend(format!("failed to create portal request proxy: {error}")))
}

#[cfg(target_os = "linux")]
fn file_uri_to_path(uri: &str) -> DriverResult<std::path::PathBuf> {
  let raw_path = uri.strip_prefix("file://").ok_or_else(|| backend(format!("portal screenshot uri is not a file uri: {uri}")))?;
  Ok(std::path::PathBuf::from(percent_decode(raw_path)?))
}

#[cfg(target_os = "linux")]
fn percent_decode(raw: &str) -> DriverResult<String> {
  let mut bytes = Vec::with_capacity(raw.len());
  let mut chars = raw.as_bytes().iter().copied();
  while let Some(byte) = chars.next() {
    if byte == b'%' {
      let high = chars.next().ok_or_else(|| backend(format!("invalid percent escape in {raw:?}")))?;
      let low = chars.next().ok_or_else(|| backend(format!("invalid percent escape in {raw:?}")))?;
      let decoded = hex_value(high)? * 16 + hex_value(low)?;
      bytes.push(decoded);
    } else {
      bytes.push(byte);
    }
  }
  String::from_utf8(bytes).map_err(|error| backend(format!("invalid UTF-8 file uri: {error}")))
}

#[cfg(target_os = "linux")]
fn hex_value(byte: u8) -> DriverResult<u8> {
  match byte {
    b'0'..=b'9' => Ok(byte - b'0'),
    b'a'..=b'f' => Ok(byte - b'a' + 10),
    b'A'..=b'F' => Ok(byte - b'A' + 10),
    _ => Err(backend(format!("invalid percent escape hex digit {:?}", byte as char))),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn portal_crop_maps_logical_bounds_to_screenshot_pixels() {
    let mut image = image::RgbaImage::new(100, 50);
    image.put_pixel(20, 10, image::Rgba([1, 2, 3, 4]));

    // ROOT CAUSE:
    //
    // If the portal returned an image in a different pixel size than GNOME's
    // logical display bounds, direct coordinate cropping rejected valid regions.
    //
    // Before the fix, a logical 200x100 display could not crop from a 100x50
    // portal image. The fix maps source bounds to image pixels before cropping.
    let cropped = crop_portal_screenshot_to_region(image, Rect::new(0.0, 0.0, 200.0, 100.0), Rect::new(40.0, 20.0, 20.0, 20.0))
      .expect("portal crop maps through source bounds");

    assert_eq!(cropped.width(), 10);
    assert_eq!(cropped.height(), 10);
    assert_eq!(*cropped.get_pixel(0, 0), image::Rgba([1, 2, 3, 4]));
  }
}
