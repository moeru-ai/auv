//! Existing macOS xcap and ScreenCaptureKit capture policies.
#![cfg_attr(not(target_os = "macos"), allow(dead_code, unused_imports))]
pub use auv_driver_capture_common::macos::WindowAxSize;
use auv_driver_capture_common::{Capture, CaptureOptions, CaptureResolution, DisplayCapture, RegionCapture};
use auv_driver_common::display::ObservedDisplays;
use auv_driver_common::error::{backend, invalid_input, not_found};
use auv_driver_common::{CoordinateSpace, Display, DriverError, DriverResult, Rect, Size, Window};
use image::RgbaImage;
use std::{thread, time::Duration};

#[cfg(feature = "macos-screencapturekit")]
pub mod native;

pub fn list_displays() -> DriverResult<ObservedDisplays> {
  Ok(ObservedDisplays {
    displays: list_display_targets()?.into_iter().map(|target| target.display).collect(),
  })
}

#[derive(Clone, Debug)]
struct MacosDisplayTarget {
  index: usize,
  display: Display,
}

fn resolve_display_target(targets: &[MacosDisplayTarget], selector: Option<&str>) -> DriverResult<MacosDisplayTarget> {
  if let Some(selector) = selector {
    let selector = selector.trim();
    return targets
      .iter()
      .find(|target| target.display.id == selector || target.display.name.as_deref().is_some_and(|display_ref| display_ref == selector))
      .cloned()
      .ok_or_else(|| not_found(format!("display {selector:?}")));
  }

  targets.iter().find(|target| target.display.is_primary).or_else(|| targets.first()).cloned().ok_or_else(|| not_found("primary display"))
}

#[cfg(all(target_os = "macos", feature = "macos-xcap"))]
fn list_display_targets() -> DriverResult<Vec<MacosDisplayTarget>> {
  let monitors = xcap::Monitor::all().map_err(|error| backend(format!("failed to enumerate displays: {error}")))?;
  display_targets_from_monitors(&monitors)
}

#[cfg(not(all(target_os = "macos", feature = "macos-xcap")))]
fn list_display_targets() -> DriverResult<Vec<MacosDisplayTarget>> {
  Err(DriverError::unsupported("display.list"))
}

#[cfg(all(target_os = "macos", feature = "macos-xcap"))]
fn display_capture(display: Display, image: RgbaImage, scale_factor: f64, backend: &str) -> DisplayCapture {
  DisplayCapture {
    capture: Capture {
      origin: Some(auv_driver_common::Position::in_screen(auv_driver_common::ScreenPoint::from(display.frame.origin))),
      image,
      bounds: display.frame,
      scale_factor,
      backend: backend.into(),
      fallback_reason: None,
    },
    display,
  }
}

#[cfg(all(target_os = "macos", feature = "macos-xcap"))]
pub fn capture_display_xcap(selector: Option<&str>) -> DriverResult<DisplayCapture> {
  let monitors = xcap::Monitor::all().map_err(|error| backend(format!("failed to enumerate displays: {error}")))?;
  let targets = display_targets_from_monitors(&monitors)?;
  let target = resolve_display_target(&targets, selector)?;
  let monitor = monitors.get(target.index).ok_or_else(|| not_found(format!("display index {}", target.index)))?;
  let image = monitor.capture_image().map_err(|error| backend(format!("failed to capture display: {error}")))?;
  let image = RgbaImage::from_raw(image.width(), image.height(), image.into_raw())
    .ok_or_else(|| backend("failed to decode captured display RGBA image"))?;
  let scale_factor = target.display.scale_factor;
  Ok(display_capture(target.display, image, scale_factor, "xcap.macos"))
}

#[cfg(not(all(target_os = "macos", feature = "macos-xcap")))]
pub fn capture_display_xcap(_selector: Option<&str>) -> DriverResult<DisplayCapture> {
  Err(DriverError::unsupported("display.capture"))
}

#[cfg(all(target_os = "macos", feature = "macos-xcap"))]
pub fn capture_region_xcap(selector: Option<&str>, region: Rect) -> DriverResult<RegionCapture> {
  let monitors = xcap::Monitor::all().map_err(|error| backend(format!("failed to enumerate displays: {error}")))?;
  let targets = display_targets_from_monitors(&monitors)?;
  let target = resolve_display_for_global_region(&targets, selector, region)?;
  // The backend addresses whole capture units, so cover a fractional region
  // (an OCR box, a layout rect) with whole units inside the display, and
  // report the covered rect as the capture bounds.
  let region = region.round_out().intersect(target.display.frame).ok_or_else(|| not_found("display containing region"))?;
  let monitor = monitors.get(target.index).ok_or_else(|| not_found(format!("display index {}", target.index)))?;
  let local_x = integral_capture_dimension("x", region.origin.x - target.display.frame.origin.x)?;
  let local_y = integral_capture_dimension("y", region.origin.y - target.display.frame.origin.y)?;
  let width = integral_positive_capture_dimension("width", region.size.width)?;
  let height = integral_positive_capture_dimension("height", region.size.height)?;
  let image = monitor
    .capture_region(local_x, local_y, width, height)
    .map_err(|error| backend(format!("failed to capture display region: {error}")))?;
  let image = RgbaImage::from_raw(image.width(), image.height(), image.into_raw())
    .ok_or_else(|| backend("failed to decode captured region RGBA image"))?;
  let capture = Capture {
    origin: Some(auv_driver_common::Position::in_screen(auv_driver_common::ScreenPoint::from(region.origin))),
    image,
    bounds: region,
    scale_factor: target.display.scale_factor,
    backend: "xcap.macos".to_string(),
    fallback_reason: None,
  };
  Ok(RegionCapture {
    display: target.display,
    capture,
  })
}

#[cfg(not(all(target_os = "macos", feature = "macos-xcap")))]
pub fn capture_region_xcap(_selector: Option<&str>, _region: Rect) -> DriverResult<RegionCapture> {
  Err(DriverError::unsupported("display.capture_region"))
}

#[cfg(all(target_os = "macos", feature = "macos-xcap"))]
fn display_targets_from_monitors(monitors: &[xcap::Monitor]) -> DriverResult<Vec<MacosDisplayTarget>> {
  if monitors.is_empty() {
    return Err(DriverError::PermissionDenied {
      permission: "screen_recording",
      message: None,
      recovery: Some(
        "xcap returned no displays; run `auv doctor --json` and grant Screen Recording to the terminal or agent process that launches AUV"
          .to_string(),
      ),
    });
  }
  monitors
    .iter()
    .enumerate()
    .map(|(index, monitor)| {
      let x = monitor.x().map_err(|error| backend(format!("failed to read display x: {error}")))? as f64;
      let y = monitor.y().map_err(|error| backend(format!("failed to read display y: {error}")))? as f64;
      let width = monitor.width().map_err(|error| backend(format!("failed to read display width: {error}")))? as f64;
      let height = monitor.height().map_err(|error| backend(format!("failed to read display height: {error}")))? as f64;
      let scale_factor = monitor.scale_factor().map_err(|error| backend(format!("failed to read display scale: {error}")))? as f64;
      let is_builtin = monitor.is_builtin().map_err(|error| backend(format!("failed to read display built-in flag: {error}")))?;
      let native_id = monitor.id().map_err(|error| backend(format!("failed to read display id: {error}")))?.to_string();
      Ok(MacosDisplayTarget {
        index,
        display: Display {
          id: native_id,
          name: Some(format!("display_{index}")),
          frame: Rect::new(x, y, width, height),
          coordinate_space: CoordinateSpace::Screen,
          scale_factor,
          is_primary: monitor.is_primary().map_err(|error| backend(format!("failed to read display primary flag: {error}")))?,
          is_builtin: Some(is_builtin),
        },
      })
    })
    .collect()
}

fn resolve_display_for_global_region(
  targets: &[MacosDisplayTarget],
  selector: Option<&str>,
  region: Rect,
) -> DriverResult<MacosDisplayTarget> {
  let selected = if selector.is_some() {
    vec![resolve_display_target(targets, selector)?]
  } else {
    targets.to_vec()
  };
  selected.into_iter().find(|target| rect_contains_rect(target.display.frame, region)).ok_or_else(|| not_found("display containing region"))
}

fn rect_contains_rect(container: Rect, candidate: Rect) -> bool {
  candidate.origin.x >= container.origin.x
    && candidate.origin.y >= container.origin.y
    && candidate.origin.x + candidate.size.width <= container.origin.x + container.size.width
    && candidate.origin.y + candidate.size.height <= container.origin.y + container.size.height
}

fn integral_capture_dimension(name: &str, value: f64) -> DriverResult<u32> {
  if value.fract() != 0.0 {
    return Err(invalid_input(format!("region {name} must be an integer in backend capture units")));
  }
  if value < 0.0 || value > u32::MAX as f64 {
    return Err(invalid_input(format!("region {name} is outside the capture backend range")));
  }
  Ok(value as u32)
}

fn integral_positive_capture_dimension(name: &str, value: f64) -> DriverResult<u32> {
  let integral = integral_capture_dimension(name, value)?;
  if integral == 0 {
    return Err(invalid_input(format!("region {name} must be positive")));
  }
  Ok(integral)
}

#[cfg(all(
  target_os = "macos",
  feature = "macos-xcap",
  feature = "macos-screencapturekit"
))]
pub fn capture_window(
  window: &Window,
  resolution: CaptureResolution,
  mut ax_size: impl FnMut(u32, i64) -> Option<WindowAxSize>,
) -> DriverResult<Capture> {
  // Prefer the Swift FFI ScreenCaptureKit path for typed window capture.
  //
  // Why this path exists instead of shelling out to `screencapture` or using
  // the Rust `xcap` crate as the primary backend:
  //
  // | Backend | Capture + OCR wall time observed locally | Runtime shape |
  // |---|---:|---|
  // | Swift FFI ScreenCaptureKit + Vision | ~1.28-1.60s | in-process, typed RGBA frame, no required PNG temp file |
  // | `screencapture` CLI + Vision | ~1.10-1.20s | subprocess + filesystem PNG handoff |
  // | `xcap` window/display capture + Vision | ~6-7s | in-process Rust API, but slow on this macOS target |
  //
  // `screencapture` can be slightly faster in the narrow benchmark, but it
  // forces process spawning and file handoff into the operation path. The FFI
  // path keeps capture and OCR composable in memory, avoids making artifact
  // writes part of primitive execution, and gives us a place to expose native
  // permission/error details. `xcap` stays as a fallback while the new native
  // path is still being proven across app/window states.
  match capture_window_swift(window, resolution, &mut ax_size) {
    Ok(capture) => Ok(capture),
    // The window itself is in the wrong state; xcap would capture the same frame.
    Err(error @ DriverError::StaleUiReference { .. }) => Err(error),
    Err(swift_error) => {
      let fallback_reason = swift_error.to_string();
      capture_window_xcap(window, Some(fallback_reason.clone())).map(|capture| capture.at_resolution(resolution)).map_err(|xcap_error| {
        backend(format!(
          "native Swift window capture failed before xcap fallback: {fallback_reason}; xcap fallback also failed: {xcap_error}"
        ))
      })
    }
  }
}

#[cfg(all(target_os = "macos", feature = "macos-screencapturekit"))]
pub fn capture_window_swift(
  window: &Window,
  resolution: CaptureResolution,
  ax_size: &mut impl FnMut(u32, i64) -> Option<WindowAxSize>,
) -> DriverResult<Capture> {
  let native_window_id = window
    .reference
    .id
    .parse::<i64>()
    .map_err(|error| invalid_input(format!("window ref {} was not a native macOS window id: {error}", window.reference.id)))?;
  let logical = resolution == CaptureResolution::Logical;
  let mut capture_and_classify = || -> DriverResult<_> {
    let capture = native::capture_window_rgba(native_window_id, logical).map_err(backend)?;
    let frame = classify_captured_frame(window.frame.size, capture.window_frame.size, || {
      window.process_id.and_then(|pid| ax_size(pid, native_window_id))
    });
    Ok((capture, frame))
  };
  // NOTICE(window-capture-frame-mismatch): ScreenCaptureKit sizes the image
  // from the window's frame at capture time. Mission Control, App Exposé and
  // minimize animations shrink that frame (and the window-server bounds) for a
  // moment; Mission Control frames come back black (measured 2026-10-08:
  // 955x558 pt for a 1644x960 pt window while AX kept 1644x960). Scaling such
  // a frame by `window.frame` gave captures a wrong `scale_factor` (1.17) and
  // garbage pixels. A transformed frame is retried once after the animation,
  // then reported as stale; a real resize (AX agrees with the captured size)
  // is captured at the new frame. Mission Control also returns some black
  // frames at the full frame size while it opens; those pass this check, and
  // only content checks can tell them apart from a dark window.
  let (mut capture, mut frame) = capture_and_classify()?;
  if let CapturedWindowFrame::Transformed { .. } = frame {
    thread::sleep(Duration::from_millis(300));
    (capture, frame) = capture_and_classify()?;
  }
  let bounds = match frame {
    CapturedWindowFrame::Resolved => window.frame,
    CapturedWindowFrame::Resized => capture.window_frame,
    CapturedWindowFrame::Transformed { minimized } => {
      let cause = if minimized {
        "the window is minimized"
      } else {
        "the window is shown by Mission Control, App Exposé or a window animation"
      };
      return Err(DriverError::StaleUiReference {
        message: format!(
          "window {} was {}x{} pt at capture time but resolved as {}x{} pt: {cause}",
          window.reference.id,
          capture.window_frame.size.width,
          capture.window_frame.size.height,
          window.frame.size.width,
          window.frame.size.height
        ),
        recovery: Some("retry once the window is shown normally again".to_string()),
      });
    }
  };
  let width = u32::try_from(capture.image_width).map_err(|error| backend(format!("native capture returned invalid width: {error}")))?;
  let height = u32::try_from(capture.image_height).map_err(|error| backend(format!("native capture returned invalid height: {error}")))?;
  let image =
    RgbaImage::from_raw(width, height, capture.rgba_bytes).ok_or_else(|| backend("failed to decode native captured window RGBA image"))?;
  let scale_factor = if bounds.size.width > 0.0 {
    f64::from(width) / bounds.size.width
  } else {
    1.0
  };
  Ok(Capture {
    origin: Some(auv_driver_common::Position::in_window(&window.reference, auv_driver_common::WindowPoint::new(0.0, 0.0))),
    image,
    bounds,
    scale_factor,
    backend: "macos.screencapturekit.ffi".to_string(),
    fallback_reason: None,
  })
}

/// How the frame ScreenCaptureKit captured a window at relates to the frame
/// the window was resolved with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CapturedWindowFrame {
  /// The captured frame is the resolved frame.
  Resolved,
  /// The window was resized since it was resolved; its application agrees
  /// with the captured size.
  Resized,
  /// The window server shows the window transformed (Mission Control, App
  /// Exposé, a minimize animation) or minimized. Also used when the
  /// application's size is unavailable, since a resize cannot be confirmed.
  Transformed { minimized: bool },
}

/// Classifies a captured window frame. `ax_size` is only consulted when the
/// sizes differ, since it costs an Accessibility round trip.
fn classify_captured_frame(resolved: Size, captured: Size, ax_size: impl FnOnce() -> Option<WindowAxSize>) -> CapturedWindowFrame {
  // A window resolved without a frame has nothing to compare against.
  if resolved.width <= 0.0 || resolved.height <= 0.0 || same_size(resolved, captured) {
    return CapturedWindowFrame::Resolved;
  }
  match ax_size() {
    Some(ax) if ax.minimized => CapturedWindowFrame::Transformed { minimized: true },
    Some(ax) if same_size(ax.size, captured) => CapturedWindowFrame::Resized,
    _ => CapturedWindowFrame::Transformed { minimized: false },
  }
}

/// Equal up to point rounding between ScreenCaptureKit, the window server and
/// Accessibility.
fn same_size(left: Size, right: Size) -> bool {
  (left.width - right.width).abs() <= 1.0 && (left.height - right.height).abs() <= 1.0
}

#[cfg(all(target_os = "macos", feature = "macos-xcap"))]
pub fn capture_window_xcap(window: &Window, fallback_reason: Option<String>) -> DriverResult<Capture> {
  let native_window_id = window
    .reference
    .id
    .parse::<u32>()
    .map_err(|error| invalid_input(format!("window ref {} was not a native macOS window id: {error}", window.reference.id)))?;
  let windows = xcap::Window::all().map_err(|error| backend(format!("failed to enumerate windows: {error}")))?;
  let xcap_window = windows
    .iter()
    .find(|candidate| candidate.id().is_ok_and(|id| id == native_window_id))
    .ok_or_else(|| not_found(format!("native window {}", window.reference.id)))?;
  let image = xcap_window.capture_image().map_err(|error| backend(format!("failed to capture window {}: {error}", window.reference.id)))?;
  let width = image.width();
  let height = image.height();
  let scale_factor = if window.frame.size.width > 0.0 {
    f64::from(width) / window.frame.size.width
  } else {
    1.0
  };
  let image = RgbaImage::from_raw(width, height, image.into_raw()).ok_or_else(|| backend("failed to decode captured window RGBA image"))?;
  Ok(Capture {
    origin: Some(auv_driver_common::Position::in_window(&window.reference, auv_driver_common::WindowPoint::new(0.0, 0.0))),
    image,
    bounds: window.frame,
    scale_factor,
    backend: "xcap.macos".to_string(),
    fallback_reason,
  })
}

#[cfg(not(all(target_os = "macos", feature = "macos-xcap")))]
pub fn capture_window(
  _window: &Window,
  _resolution: CaptureResolution,
  _ax_size: impl FnMut(u32, i64) -> Option<WindowAxSize>,
) -> DriverResult<Capture> {
  Err(DriverError::unsupported("capture_window"))
}

// NOTICE(sck-display-discovery): this existing filtered operation uses xcap
// target resolution. With ScreenCaptureKit alone, use its native display-id API;
// a different resolver belongs to the approved desktop extraction.
/// Capture a display without windows owned by this process, leaving those
/// windows visible. Requires ScreenCaptureKit screenshots (macOS 14+).
/// Applies the requested resolution after capturing native pixels.
/// Never falls back to an unfiltered screenshot if exclusion fails.
#[cfg(all(
  target_os = "macos",
  feature = "macos-screencapturekit",
  feature = "macos-xcap"
))]
pub fn capture_excluding_self(options: CaptureOptions) -> DriverResult<DisplayCapture> {
  if options.window.is_some() || options.region.is_some() || matches!(options.activation, Activation::ActivateFirst { .. }) {
    return Err(invalid_input("display.capture_excluding_self accepts only display selection and resolution"));
  }
  let target = resolve_display_target(&list_display_targets()?, options.display.as_deref())?;
  let id = target.display.id.parse::<u32>().map_err(|error| invalid_input(format!("invalid native display id: {error}")))?;
  let pixels = native::capture_display_rgba_without_self(id).map_err(backend)?;
  let width = u32::try_from(pixels.image_width).map_err(|error| backend(error.to_string()))?;
  let height = u32::try_from(pixels.image_height).map_err(|error| backend(error.to_string()))?;
  let image = RgbaImage::from_raw(width, height, pixels.rgba_bytes).ok_or_else(|| backend("invalid native display RGBA image"))?;
  let scale_factor = f64::from(width) / target.display.frame.size.width;
  let captured = display_capture(target.display, image, scale_factor, "macos.screencapturekit.excluding_self");
  Ok(DisplayCapture {
    capture: captured.capture.at_resolution(options.resolution),
    ..captured
  })
}

#[cfg(all(target_os = "macos", feature = "macos-screencapturekit"))]
mod binding;

/// The existing xcap capture backend.
#[cfg(feature = "macos-xcap")]
#[derive(Clone, Copy, Debug, Default)]
pub struct Xcap;
#[cfg(feature = "macos-xcap")]
impl auv_driver_capture_common::DisplayCaptureDriver for Xcap {
  fn capture_display(&mut self, options: CaptureOptions) -> DriverResult<DisplayCapture> {
    options.validate_display()?;
    let captured = capture_display_xcap(options.display.as_deref())?;
    Ok(DisplayCapture {
      capture: captured.capture.at_resolution(options.resolution),
      ..captured
    })
  }
}
#[cfg(feature = "macos-xcap")]
impl auv_driver_capture_common::RegionCaptureDriver for Xcap {
  fn capture_region(&mut self, options: CaptureOptions) -> DriverResult<RegionCapture> {
    let region = options.validate_region()?;
    let captured = capture_region_xcap(options.display.as_deref(), region)?;
    Ok(RegionCapture {
      capture: captured.capture.at_resolution(options.resolution),
      ..captured
    })
  }
}
#[cfg(all(target_os = "macos", feature = "macos-xcap"))]
impl auv_driver_capture_common::WindowCaptureDriver for Xcap {
  fn capture_window(&mut self, window: &Window, resolution: CaptureResolution) -> DriverResult<Capture> {
    capture_window_xcap(window, None).map(|capture| capture.at_resolution(resolution))
  }
}

/// ScreenCaptureKit window capture. The host supplies the existing AX size
/// query, an external accessibility boundary; capture invokes it only on mismatch.
#[cfg(feature = "macos-screencapturekit")]
pub struct ScreenCaptureKit<A> {
  ax_size: A,
}
#[cfg(feature = "macos-screencapturekit")]
impl<A> ScreenCaptureKit<A> {
  pub fn new(ax_size: A) -> Self {
    Self { ax_size }
  }
}
#[cfg(all(target_os = "macos", feature = "macos-screencapturekit"))]
impl<A: FnMut(u32, i64) -> Option<WindowAxSize>> auv_driver_capture_common::WindowCaptureDriver for ScreenCaptureKit<A> {
  fn capture_window(&mut self, window: &Window, resolution: CaptureResolution) -> DriverResult<Capture> {
    capture_window_swift(window, resolution, &mut self.ax_size)
  }
}

#[cfg(test)]
#[path = "policy_test.rs"]
mod tests;
