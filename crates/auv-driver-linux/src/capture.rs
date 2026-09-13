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
#[cfg(any(target_os = "linux", test))]
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
      let captured = capture_fallback(PORTAL_SCREENCAST_BACKEND, &error, || match target.as_ref() {
        Some(target) => capture_area(target.display.frame, target.display.frame),
        None => capture_full(),
      })?;
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
    Err(error) => with_primary_capture_failure(
      capture_fallback(PORTAL_SCREENCAST_BACKEND, &error, || capture_area(region, target.display.frame))?,
      PORTAL_SCREENCAST_BACKEND,
      &error.to_string(),
    ),
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

#[cfg(any(target_os = "linux", test))]
fn capture_fallback<T>(
  primary_backend: &str,
  primary_error: &auv_driver_common::error::DriverError,
  fallback: impl FnOnce() -> DriverResult<T>,
) -> DriverResult<T> {
  fallback()
    .map_err(|fallback_error| backend(format!("{primary_backend} failed: {primary_error}; fallback capture also failed: {fallback_error}")))
}

#[cfg(target_os = "linux")]
fn capture_monitor_frame_for_session(
  state: &Arc<Mutex<LinuxDriverSessionState>>,
  target_bounds: Option<Rect>,
) -> DriverResult<ScreenCastFrame> {
  // TODO(linux-capture-session-lock): the outer session mutex remains held
  // during the bounded frame wait because ScreenCastSession is currently
  // stored directly in this shared state. Move capture ownership behind its
  // own synchronization only when a concurrent driver-session slice defines
  // how capture, input, and session invalidation coordinate.
  let mut state = state.lock().expect("linux driver session state poisoned");
  if state.screencast_session.is_none() {
    let restore_tokens = state.restore_tokens.clone();
    state.screencast_session = Some(ScreenCastSession::open_monitor(restore_tokens.as_ref(), state.portal_app_id.as_deref())?);
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
  use crate::native::portal::run;
  use ashpd::desktop::screenshot::Screenshot;

  // NOTICE: this legacy Screenshot fallback still requests interactive consent;
  // it does not consume the persistent ScreenCast grant. See
  // `docs/ai/references/driver/2026-09-12-linux-portal-authorization-and-runner-reuse.md`.
  // NOTICE(linux-portal-screenshot): GNOME Wayland does not expose a stable
  // non-portal screenshot API for ordinary clients. The compositor/user owns
  // screenshot consent; persistent capture uses ScreenCast/PipeWire above.
  let response = run("take portal screenshot", async { Screenshot::request().interactive(true).modal(true).send().await?.response() })?;
  let path = url::Url::parse(response.uri().as_str())
    .map_err(|error| backend(format!("invalid screenshot URI: {error}")))?
    .to_file_path()
    .map_err(|_| backend("portal screenshot URI is not a local file"))?;
  let image = image::open(&path).map_err(|error| backend(format!("failed to open portal screenshot {path:?}: {error}")))?.to_rgba8();
  let _ = std::fs::remove_file(path);
  Ok(image)
}

#[cfg(test)]
#[path = "capture_test.rs"]
mod tests;
