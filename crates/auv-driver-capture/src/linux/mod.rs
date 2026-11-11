//! Existing Portal capture algorithms. The caller retains native session ownership
//! and controls synchronization; source discovery and fallback retain their order.

use auv_driver_capture_common::{Capture, CaptureOptions, DisplayCapture, RegionCapture};
use auv_driver_common::error::{backend, invalid_input};
use auv_driver_common::{CoordinateSpace, Display, DriverResult, Rect};
use auv_driver_desktop::linux::display::{self, list_targets, resolve_for_region, selected_target_or_none};

#[cfg(feature = "linux-screencast")]
mod native;
#[cfg(feature = "linux-screencast")]
pub use native::{ScreenCastFrame, ScreenCastSession, ScreenCastStream};

/// Explicit composition policy. A direct ScreenCast driver never selects a
/// fallback just because another backend feature is enabled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Fallback {
  #[default]
  None,
  #[cfg(feature = "linux-screenshot")]
  Screenshot,
}

#[cfg(any(feature = "linux-screenshot", test))]
const PORTAL_CAPTURE_BACKEND: &str = "xdg-desktop-portal.screenshot";
#[cfg(any(feature = "linux-screencast", test))]
const PORTAL_SCREENCAST_BACKEND: &str = "xdg-desktop-portal.screencast.pipewire";

#[cfg(feature = "linux-screencast")]
pub fn capture_display(
  selector: Option<&str>,
  primary: impl FnOnce(Option<Rect>) -> DriverResult<ScreenCastFrame>,
  fallback: Fallback,
) -> DriverResult<DisplayCapture> {
  #[cfg(not(feature = "linux-screenshot"))]
  let _ = fallback;
  let target = selected_target_or_none(selector)?;
  let target_bounds = target.as_ref().map(|target| target.display.frame);
  match primary(target_bounds) {
    Ok(frame) => {
      let display = target.map(|target| target.display).unwrap_or_else(|| display_from_screencast_frame(&frame));
      let scale_factor = capture_scale_factor(&frame.image, display.frame, display.scale_factor);
      let capture = Capture {
        origin: Some(auv_driver_common::Position::in_screen(auv_driver_common::ScreenPoint::from(display.frame.origin))),
        image: frame.image,
        bounds: display.frame,
        scale_factor,
        backend: PORTAL_SCREENCAST_BACKEND.to_string(),
        fallback_reason: None,
      };
      Ok(DisplayCapture { display, capture })
    }
    #[cfg(feature = "linux-screenshot")]
    Err(error) if fallback == Fallback::Screenshot => {
      let captured = capture_fallback(PORTAL_SCREENCAST_BACKEND, &error, || match target.as_ref() {
        Some(target) => capture_area(target.display.frame, target),
        None => Err(backend("Screenshot fallback requires known display geometry")),
      })?;
      capture_display_from_captured(target, with_primary_capture_failure(captured, PORTAL_SCREENCAST_BACKEND, &error.to_string()))
    }
    Err(error) => Err(error),
  }
}

#[cfg(any(feature = "linux-screenshot", test))]
fn capture_display_from_captured(target: Option<display::DisplayTarget>, captured: CapturedImage) -> DriverResult<DisplayCapture> {
  // NOTICE: Unbound images and non-native Screenshot pixel layouts cannot prove
  // this display mapping. Accept them only after an owner-approved mapping contract.
  let display = target.ok_or_else(|| backend("Screenshot fallback requires known display geometry"))?.display;
  // Screenshot responses contain only a URI, without the selected area's
  // origin or size. Reject partial images before assigning screen coordinates.
  // https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Screenshot.html
  if !display.scale_factor.is_finite() || display.scale_factor <= 0.0 {
    return Err(backend("Screenshot fallback requires a positive finite display scale"));
  }
  let width = scaled_positive_capture_dimension("source width", display.frame.size.width, display.scale_factor)?;
  let height = scaled_positive_capture_dimension("source height", display.frame.size.height, display.scale_factor)?;
  if captured.image.dimensions() != (width, height) {
    return Err(backend(format!(
      "Screenshot fallback returned {}x{} pixels, but display {:?} requires {width}x{height}. The screenshot has no proven screen coordinates",
      captured.image.width(),
      captured.image.height(),
      display.id
    )));
  }
  let scale_factor = capture_scale_factor(&captured.image, display.frame, display.scale_factor);
  let capture = Capture {
    origin: Some(auv_driver_common::Position::in_screen(auv_driver_common::ScreenPoint::from(display.frame.origin))),
    image: captured.image,
    bounds: display.frame,
    scale_factor,
    backend: captured.backend,
    fallback_reason: captured.fallback_reason,
  };
  Ok(DisplayCapture { display, capture })
}

#[cfg(feature = "linux-screencast")]
pub fn capture_region(
  selector: Option<&str>,
  region: Rect,
  primary: impl FnOnce(Option<Rect>) -> DriverResult<ScreenCastFrame>,
  fallback: Fallback,
) -> DriverResult<RegionCapture> {
  #[cfg(not(feature = "linux-screenshot"))]
  let _ = fallback;
  let targets = list_targets()?;
  let target = resolve_for_region(&targets, selector, region)?;
  let captured = match primary(Some(target.display.frame)) {
    Ok(frame) => CapturedImage {
      image: crop_portal_screenshot_to_region(frame.image, target.display.frame, region)?,
      backend: format!("{PORTAL_SCREENCAST_BACKEND}.crop"),
      fallback_reason: Some("region pixels were cropped from PipeWire screencast using Wayland xdg-output logical bounds".to_string()),
    },
    #[cfg(feature = "linux-screenshot")]
    Err(error) if fallback == Fallback::Screenshot => with_primary_capture_failure(
      capture_fallback(PORTAL_SCREENCAST_BACKEND, &error, || capture_area(region, &target))?,
      PORTAL_SCREENCAST_BACKEND,
      &error.to_string(),
    ),
    Err(error) => return Err(error),
  };
  let scale_factor = capture_scale_factor(&captured.image, region, target.display.scale_factor);
  let capture = Capture {
    origin: Some(auv_driver_common::Position::in_screen(auv_driver_common::ScreenPoint::from(region.origin))),
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

struct CapturedImage {
  image: image::RgbaImage,
  backend: String,
  fallback_reason: Option<String>,
}

#[cfg(any(all(feature = "linux-screencast", feature = "linux-screenshot"), test))]
fn capture_fallback<T>(
  primary_backend: &str,
  primary_error: &auv_driver_common::error::DriverError,
  fallback: impl FnOnce() -> DriverResult<T>,
) -> DriverResult<T> {
  fallback()
    .map_err(|fallback_error| backend(format!("{primary_backend} failed: {primary_error}; fallback capture also failed: {fallback_error}")))
}

#[cfg(feature = "linux-screenshot")]
fn capture_area(region: Rect, target: &display::DisplayTarget) -> DriverResult<CapturedImage> {
  let targets = list_targets()?;
  let source = screenshot_source(&targets, target)?;
  let image = portal_screenshot()?;
  // Consent can outlast an output change. Do not bind the image to stale bounds.
  screenshot_source(&list_targets()?, source)?;
  let full = capture_display_from_captured(
    Some(source.clone()),
    CapturedImage {
      image,
      backend: PORTAL_CAPTURE_BACKEND.to_string(),
      fallback_reason: None,
    },
  )?;
  Ok(CapturedImage {
    image: crop_portal_screenshot_to_region(full.capture.image, source.display.frame, region)?,
    backend: format!("{PORTAL_CAPTURE_BACKEND}.crop"),
    fallback_reason: Some("region pixels were cropped from a dimension-validated single-display portal screenshot".to_string()),
  })
}

#[cfg(any(feature = "linux-screenshot", test))]
fn screenshot_source<'a>(
  targets: &'a [display::DisplayTarget],
  target: &display::DisplayTarget,
) -> DriverResult<&'a display::DisplayTarget> {
  // TODO(linux-screenshot-desktop-layout): multi-output Screenshot cropping is
  // deferred because the URI has no composite layout or per-output pixel map.
  // Re-open this fallback only with an owner-approved desktop mapping contract.
  let [source] = targets else {
    return Err(backend(format!("Screenshot fallback requires exactly one known display, found {}", targets.len())));
  };
  if source.display.id != target.display.id
    || source.display.frame != target.display.frame
    || source.display.scale_factor != target.display.scale_factor
  {
    return Err(backend("display geometry changed during Screenshot fallback"));
  }
  Ok(source)
}

#[cfg(feature = "linux-screencast")]
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

#[cfg(any(all(feature = "linux-screencast", feature = "linux-screenshot"), test))]
fn with_primary_capture_failure(mut captured: CapturedImage, primary_backend: &str, primary_error: &str) -> CapturedImage {
  let fallback = captured.fallback_reason.take().unwrap_or_else(|| format!("used {PORTAL_CAPTURE_BACKEND} fallback"));
  captured.fallback_reason = Some(format!("{primary_backend} failed ({primary_error}); {fallback}"));
  captured
}

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

fn capture_scale_factor(image: &image::RgbaImage, bounds: Rect, default: f64) -> f64 {
  if bounds.size.width <= 0.0 {
    return default;
  }
  f64::from(image.width()) / bounds.size.width
}

fn scaled_capture_dimension(name: &str, value: f64, scale: f64) -> DriverResult<u32> {
  let value = (value * scale).round();
  if !(0.0..=f64::from(u32::MAX)).contains(&value) {
    return Err(invalid_input(format!("region {name} must be within u32 capture bounds")));
  }
  Ok(value as u32)
}

fn scaled_positive_capture_dimension(name: &str, value: f64, scale: f64) -> DriverResult<u32> {
  let value = scaled_capture_dimension(name, value, scale)?;
  if value == 0 {
    return Err(invalid_input(format!("region {name} must be positive")));
  }
  Ok(value)
}

#[cfg(feature = "linux-screenshot")]
fn portal_screenshot() -> DriverResult<image::RgbaImage> {
  use ashpd::desktop::screenshot::{AvailableTargets, ScreenshotOptions, ScreenshotProxy};
  use auv_driver_portal_linux::run_with_consent;

  // NOTICE: this legacy Screenshot fallback can still request user consent;
  // it does not consume the persistent ScreenCast grant. See
  // `docs/ai/references/driver/2026-09-12-linux-portal-authorization-and-runner-reuse.md`.
  // NOTICE(linux-portal-screenshot): GNOME Wayland does not expose a stable
  // non-portal screenshot API for ordinary clients. The compositor/user owns
  // screenshot consent; persistent capture uses ScreenCast/PipeWire above.
  // Disable area/window customization: its result has no coordinate metadata.
  // Version 3 also exposes an explicit Screen target. Version 2 remains bounded
  // by the single-output and exact pixel-dimension checks in `capture_area`.
  let response = run_with_consent("take portal screenshot", async {
    let proxy = ScreenshotProxy::new().await?;
    let mut options = ScreenshotOptions::default().set_interactive(false).set_modal(true);
    if proxy.version() >= 3 {
      options = options.set_target(AvailableTargets::Screen);
    }
    proxy.screenshot(None, options).await?.response()
  })?;
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

pub mod window;

// These are explicit backend implementations. Compiling another backend never
// changes the fallback policy of this ScreenCast source.
#[cfg(feature = "linux-screencast")]
impl auv_driver_capture_common::DisplayCaptureDriver for ScreenCastSession {
  fn capture_display(&mut self, options: CaptureOptions) -> DriverResult<DisplayCapture> {
    options.validate_display()?;
    let captured = capture_display(options.display.as_deref(), |bounds| self.capture_monitor_frame(bounds), Fallback::None)?;
    Ok(DisplayCapture {
      capture: captured.capture.at_resolution(options.resolution),
      ..captured
    })
  }
}
#[cfg(feature = "linux-screencast")]
impl auv_driver_capture_common::RegionCaptureDriver for ScreenCastSession {
  fn capture_region(&mut self, options: CaptureOptions) -> DriverResult<RegionCapture> {
    let region = options.validate_region()?;
    let captured = capture_region(options.display.as_deref(), region, |bounds| self.capture_monitor_frame(bounds), Fallback::None)?;
    Ok(RegionCapture {
      capture: captured.capture.at_resolution(options.resolution),
      ..captured
    })
  }
}

/// The Screenshot portal backend. Consent and source geometry rules match the
/// existing fallback; selecting this driver does not open a ScreenCast session.
#[cfg(feature = "linux-screenshot")]
#[derive(Clone, Copy, Debug, Default)]
pub struct Screenshot;

#[cfg(feature = "linux-screenshot")]
impl auv_driver_capture_common::DisplayCaptureDriver for Screenshot {
  fn capture_display(&mut self, options: CaptureOptions) -> DriverResult<DisplayCapture> {
    options.validate_display()?;
    let target =
      selected_target_or_none(options.display.as_deref())?.ok_or_else(|| backend("Screenshot fallback requires known display geometry"))?;
    let image = capture_area(target.display.frame, &target)?;
    let captured = capture_display_from_captured(Some(target), image)?;
    Ok(DisplayCapture {
      capture: captured.capture.at_resolution(options.resolution),
      ..captured
    })
  }
}
#[cfg(feature = "linux-screenshot")]
impl auv_driver_capture_common::RegionCaptureDriver for Screenshot {
  fn capture_region(&mut self, options: CaptureOptions) -> DriverResult<RegionCapture> {
    let region = options.validate_region()?;
    let target = resolve_for_region(&list_targets()?, options.display.as_deref(), region)?;
    let image = capture_area(region, &target)?;
    let scale_factor = capture_scale_factor(&image.image, region, target.display.scale_factor);
    let capture = Capture {
      origin: Some(auv_driver_common::Position::in_screen(auv_driver_common::ScreenPoint::from(region.origin))),
      image: image.image,
      bounds: region,
      scale_factor,
      backend: image.backend,
      fallback_reason: image.fallback_reason,
    }
    .at_resolution(options.resolution);
    Ok(RegionCapture {
      display: target.display,
      capture,
    })
  }
}
