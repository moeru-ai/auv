use auv_driver::capture::{Capture, DisplayCapture, RegionCapture};
use auv_driver::display::{Display, ObservedDisplays};
use auv_driver::error::DriverResult;
use auv_driver::geometry::{CoordinateSpace, Rect};

use crate::error::{backend, invalid_input, not_found};

#[cfg(target_os = "linux")]
const CAPTURE_BACKEND: &str = "xcap.linux";

#[derive(Clone, Debug)]
struct DisplayTarget {
  index: usize,
  display: Display,
}

#[cfg(target_os = "linux")]
pub fn list_displays() -> DriverResult<ObservedDisplays> {
  let monitors = xcap::Monitor::all()
    .map_err(|error| backend(format!("failed to enumerate displays: {error}")))?;
  Ok(ObservedDisplays {
    displays: display_targets_from_monitors(&monitors)?
      .into_iter()
      .map(|target| target.display)
      .collect(),
  })
}

#[cfg(not(target_os = "linux"))]
pub fn list_displays() -> DriverResult<ObservedDisplays> {
  Err(auv_driver::error::DriverError::unsupported("display.list"))
}

#[cfg(target_os = "linux")]
pub fn capture_display(selector: Option<&str>) -> DriverResult<DisplayCapture> {
  let monitors = xcap::Monitor::all()
    .map_err(|error| backend(format!("failed to enumerate displays: {error}")))?;
  let targets = display_targets_from_monitors(&monitors)?;
  let target = resolve_display_target(&targets, selector)?;
  let monitor = monitors
    .get(target.index)
    .ok_or_else(|| not_found(format!("display index {}", target.index)))?;
  let image = match monitor.capture_image() {
    Ok(image) => image::RgbaImage::from_raw(image.width(), image.height(), image.into_raw())
      .ok_or_else(|| backend("failed to decode captured display RGBA image"))?,
    Err(primary_error) => portal_screenshot().map_err(|fallback_error| {
      backend(format!(
        "failed to capture display via xcap ({primary_error}); portal screenshot fallback failed ({fallback_error})"
      ))
    })?,
  };
  let capture = Capture {
    image,
    bounds: target.display.frame,
    scale_factor: target.display.scale_factor,
    backend: CAPTURE_BACKEND.to_string(),
    fallback_reason: None,
  };
  Ok(DisplayCapture {
    display: target.display,
    capture,
  })
}

#[cfg(not(target_os = "linux"))]
pub fn capture_display(_selector: Option<&str>) -> DriverResult<DisplayCapture> {
  Err(auv_driver::error::DriverError::unsupported(
    "display.capture",
  ))
}

#[cfg(target_os = "linux")]
pub fn capture_region(selector: Option<&str>, region: Rect) -> DriverResult<RegionCapture> {
  let monitors = xcap::Monitor::all()
    .map_err(|error| backend(format!("failed to enumerate displays: {error}")))?;
  let targets = display_targets_from_monitors(&monitors)?;
  let target = resolve_display_for_region(&targets, selector, region)?;
  let monitor = monitors
    .get(target.index)
    .ok_or_else(|| not_found(format!("display index {}", target.index)))?;
  let local_x = integral_capture_dimension("x", region.origin.x - target.display.frame.origin.x)?;
  let local_y = integral_capture_dimension("y", region.origin.y - target.display.frame.origin.y)?;
  let width = integral_positive_capture_dimension("width", region.size.width)?;
  let height = integral_positive_capture_dimension("height", region.size.height)?;
  let image = monitor
    .capture_region(local_x, local_y, width, height)
    .map_err(|error| backend(format!("failed to capture display region: {error}")))?;
  let image = image::RgbaImage::from_raw(image.width(), image.height(), image.into_raw())
    .ok_or_else(|| backend("failed to decode captured region RGBA image"))?;
  let capture = Capture {
    image,
    bounds: region,
    scale_factor: target.display.scale_factor,
    backend: CAPTURE_BACKEND.to_string(),
    fallback_reason: None,
  };
  Ok(RegionCapture {
    display: target.display,
    capture,
  })
}

#[cfg(not(target_os = "linux"))]
pub fn capture_region(_selector: Option<&str>, _region: Rect) -> DriverResult<RegionCapture> {
  Err(auv_driver::error::DriverError::unsupported(
    "display.capture_region",
  ))
}

#[cfg(target_os = "linux")]
fn display_targets_from_monitors(monitors: &[xcap::Monitor]) -> DriverResult<Vec<DisplayTarget>> {
  if monitors.is_empty() {
    return Err(not_found("display"));
  }
  monitors
    .iter()
    .enumerate()
    .map(|(index, monitor)| {
      let x = monitor
        .x()
        .map_err(|error| backend(format!("failed to read display x: {error}")))?
        as f64;
      let y = monitor
        .y()
        .map_err(|error| backend(format!("failed to read display y: {error}")))?
        as f64;
      let width = monitor
        .width()
        .map_err(|error| backend(format!("failed to read display width: {error}")))?
        as f64;
      let height = monitor
        .height()
        .map_err(|error| backend(format!("failed to read display height: {error}")))?
        as f64;
      let scale_factor = monitor
        .scale_factor()
        .map_err(|error| backend(format!("failed to read display scale: {error}")))?
        as f64;
      let native_id = monitor
        .id()
        .map_err(|error| backend(format!("failed to read display id: {error}")))?
        .to_string();
      Ok(DisplayTarget {
        index,
        display: Display {
          id: native_id,
          name: Some(format!("display_{index}")),
          frame: Rect::new(x, y, width, height),
          coordinate_space: CoordinateSpace::Screen,
          scale_factor,
          is_primary: monitor
            .is_primary()
            .map_err(|error| backend(format!("failed to read display primary flag: {error}")))?,
          is_builtin: None,
        },
      })
    })
    .collect()
}

fn resolve_display_target(
  targets: &[DisplayTarget],
  selector: Option<&str>,
) -> DriverResult<DisplayTarget> {
  if let Some(selector) = selector {
    let selector = selector.trim();
    return targets
      .iter()
      .find(|target| {
        target.display.id == selector
          || target
            .display
            .name
            .as_deref()
            .is_some_and(|display_ref| display_ref == selector)
      })
      .cloned()
      .ok_or_else(|| not_found(format!("display {selector:?}")));
  }

  targets
    .iter()
    .find(|target| target.display.is_primary)
    .or_else(|| targets.first())
    .cloned()
    .ok_or_else(|| not_found("primary display"))
}

fn resolve_display_for_region(
  targets: &[DisplayTarget],
  selector: Option<&str>,
  region: Rect,
) -> DriverResult<DisplayTarget> {
  let selected = if selector.is_some() {
    vec![resolve_display_target(targets, selector)?]
  } else {
    targets.to_vec()
  };
  selected
    .into_iter()
    .find(|target| rect_contains_rect(target.display.frame, region))
    .ok_or_else(|| not_found("display containing region"))
}

fn rect_contains_rect(container: Rect, candidate: Rect) -> bool {
  candidate.origin.x >= container.origin.x
    && candidate.origin.y >= container.origin.y
    && candidate.origin.x + candidate.size.width <= container.origin.x + container.size.width
    && candidate.origin.y + candidate.size.height <= container.origin.y + container.size.height
}

fn integral_capture_dimension(name: &str, value: f64) -> DriverResult<u32> {
  if value.fract() != 0.0 {
    return Err(invalid_input(format!(
      "region {name} must be an integer in backend capture units"
    )));
  }
  if !(0.0..=f64::from(u32::MAX)).contains(&value) {
    return Err(invalid_input(format!(
      "region {name} must be within u32 capture bounds"
    )));
  }
  Ok(value as u32)
}

fn integral_positive_capture_dimension(name: &str, value: f64) -> DriverResult<u32> {
  let value = integral_capture_dimension(name, value)?;
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

  let connection = zbus::blocking::Connection::session()
    .map_err(|error| backend(format!("failed to connect to session bus: {error}")))?;
  let handle_token = format!(
    "auv_{}_{}",
    std::process::id(),
    std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .map(|duration| duration.as_millis())
      .unwrap_or_default()
  );
  let request = portal_request_proxy(&connection, &handle_token)?;
  let proxy = Proxy::new(
    &connection,
    "org.freedesktop.portal.Desktop",
    "/org/freedesktop/portal/desktop",
    "org.freedesktop.portal.Screenshot",
  )
  .map_err(|error| backend(format!("failed to create screenshot portal proxy: {error}")))?;
  let mut options = HashMap::new();
  options.insert("handle_token", Value::from(handle_token.as_str()));
  // NOTICE(gnome-screenshot-portal): GNOME Wayland rejects the non-portal
  // capture protocol used by xcap in this environment. Keep this fallback
  // interactive so the compositor/user owns screenshot consent; replace with
  // ScreenCast/PipeWire when the owner approves the capture stream slice.
  options.insert("interactive", Value::from(true));
  options.insert("modal", Value::from(true));
  proxy
    .call_method("Screenshot", &("", options))
    .map_err(|error| backend(format!("failed to request portal screenshot: {error}")))?;

  let mut responses = request
    .receive_signal("Response")
    .map_err(|error| backend(format!("failed to subscribe to portal response: {error}")))?;
  let response = responses
    .next()
    .ok_or_else(|| backend("portal screenshot did not return a response"))?;
  let (code, body): (u32, ScreenshotResponse) = response.body().deserialize().map_err(|error| {
    backend(format!(
      "failed to decode portal screenshot response: {error}"
    ))
  })?;
  if code != 0 {
    return Err(backend(format!("portal screenshot response code {code}")));
  }
  let uri = match Value::from(body.uri) {
    Value::Str(uri) => uri.to_string(),
    other => {
      return Err(backend(format!(
        "portal screenshot returned non-string uri: {other:?}"
      )));
    }
  };
  let path = file_uri_to_path(&uri)?;
  let image = image::open(&path)
    .map_err(|error| {
      backend(format!(
        "failed to open portal screenshot {path:?}: {error}"
      ))
    })?
    .to_rgba8();
  let _ = std::fs::remove_file(path);
  Ok(image)
}

#[cfg(target_os = "linux")]
fn portal_request_proxy<'a>(
  connection: &'a zbus::blocking::Connection,
  handle_token: &str,
) -> DriverResult<zbus::blocking::Proxy<'a>> {
  let unique_name = connection
    .unique_name()
    .ok_or_else(|| backend("session bus connection has no unique name"))?
    .trim_start_matches(':')
    .replace('.', "_");
  let path = format!("/org/freedesktop/portal/desktop/request/{unique_name}/{handle_token}");
  zbus::blocking::Proxy::new(
    connection,
    "org.freedesktop.portal.Desktop",
    path,
    "org.freedesktop.portal.Request",
  )
  .map_err(|error| backend(format!("failed to create portal request proxy: {error}")))
}

#[cfg(target_os = "linux")]
fn file_uri_to_path(uri: &str) -> DriverResult<std::path::PathBuf> {
  let raw_path = uri
    .strip_prefix("file://")
    .ok_or_else(|| backend(format!("portal screenshot uri is not a file uri: {uri}")))?;
  Ok(std::path::PathBuf::from(percent_decode(raw_path)?))
}

#[cfg(target_os = "linux")]
fn percent_decode(raw: &str) -> DriverResult<String> {
  let mut bytes = Vec::with_capacity(raw.len());
  let mut chars = raw.as_bytes().iter().copied();
  while let Some(byte) = chars.next() {
    if byte == b'%' {
      let high = chars
        .next()
        .ok_or_else(|| backend(format!("invalid percent escape in {raw:?}")))?;
      let low = chars
        .next()
        .ok_or_else(|| backend(format!("invalid percent escape in {raw:?}")))?;
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
    _ => Err(backend(format!(
      "invalid percent escape hex digit {:?}",
      byte as char
    ))),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn display_resolution_prefers_primary() {
    let targets = vec![
      DisplayTarget {
        index: 0,
        display: display("left", false),
      },
      DisplayTarget {
        index: 1,
        display: display("primary", true),
      },
    ];

    let selected = resolve_display_target(&targets, None).expect("display resolves");

    assert_eq!(selected.display.id, "primary");
  }

  fn display(id: &str, is_primary: bool) -> Display {
    Display {
      id: id.to_string(),
      name: None,
      frame: Rect::new(0.0, 0.0, 100.0, 100.0),
      coordinate_space: CoordinateSpace::Screen,
      scale_factor: 1.0,
      is_primary,
      is_builtin: None,
    }
  }
}
