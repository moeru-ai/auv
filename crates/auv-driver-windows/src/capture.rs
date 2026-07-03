//! Screen and region capture backed by the `xcap` crate.
//!
//! This mirrors the macOS driver's `xcap`-based display capture path, producing
//! the shared [`Capture`] type so vision and inspection consumers stay
//! backend-agnostic. Window capture uses Win32 GDI `PrintWindow` against the
//! resolved `HWND` so a single occluded or background window can be captured
//! without compositing the whole display.

#[cfg(target_os = "windows")]
use crate::error::backend;
use crate::error::{invalid_input, not_found};
use auv_driver::capture::{Capture, DisplayCapture, RegionCapture};
use auv_driver::display::{Display, ObservedDisplays};
#[cfg(not(target_os = "windows"))]
use auv_driver::error::DriverError;
use auv_driver::error::DriverResult;
use auv_driver::geometry::{CoordinateSpace, Rect};
use auv_driver::window::Window;

/// Capture backend tag recorded on every produced display/region [`Capture`].
#[cfg(target_os = "windows")]
const CAPTURE_BACKEND: &str = "xcap.windows";

/// Capture backend tag recorded on every produced window [`Capture`].
#[cfg(target_os = "windows")]
const WINDOW_CAPTURE_BACKEND: &str = "printwindow.windows";

/// Pairs a shared [`Display`] with its `xcap` monitor index.
#[derive(Clone, Debug)]
struct DisplayTarget {
  index: usize,
  display: Display,
}

#[cfg(target_os = "windows")]
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

#[cfg(not(target_os = "windows"))]
pub fn list_displays() -> DriverResult<ObservedDisplays> {
  Err(DriverError::unsupported("display.list"))
}

#[cfg(target_os = "windows")]
pub fn capture_display(selector: Option<&str>) -> DriverResult<DisplayCapture> {
  let monitors = xcap::Monitor::all()
    .map_err(|error| backend(format!("failed to enumerate displays: {error}")))?;
  let targets = display_targets_from_monitors(&monitors)?;
  let target = resolve_display_target(&targets, selector)?;
  let monitor = monitors
    .get(target.index)
    .ok_or_else(|| not_found(format!("display index {}", target.index)))?;
  let image = monitor
    .capture_image()
    .map_err(|error| backend(format!("failed to capture display: {error}")))?;
  let image = image::RgbaImage::from_raw(image.width(), image.height(), image.into_raw())
    .ok_or_else(|| backend("failed to decode captured display RGBA image"))?;
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

#[cfg(not(target_os = "windows"))]
pub fn capture_display(_selector: Option<&str>) -> DriverResult<DisplayCapture> {
  Err(DriverError::unsupported("display.capture"))
}

#[cfg(target_os = "windows")]
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

#[cfg(not(target_os = "windows"))]
pub fn capture_region(_selector: Option<&str>, _region: Rect) -> DriverResult<RegionCapture> {
  Err(DriverError::unsupported("display.capture_region"))
}

#[cfg(target_os = "windows")]
pub fn capture_window(window: &Window) -> DriverResult<Capture> {
  let pixels = window_native::capture_window_rgba(window)?;
  // NOTICE: scale_factor is derived against the DWM frame width like the macOS
  // driver, and `bounds` reports the DWM frame. PrintWindow renders the full
  // window rect (including any non-client border), so the captured pixel extent
  // can differ from the DWM frame by the border inset. This is acceptable for a
  // uniform-scale mapping; a per-monitor DPI / border-accurate mapping is a
  // follow-up if window-OCR coordinate precision requires it.
  // TODO(windows-window-capture-dpi): tighten bounds/border/DPI mapping when an
  // owner-approved window-OCR precision requirement lands.
  let scale_factor = if window.frame.size.width > 0.0 {
    f64::from(pixels.width) / window.frame.size.width
  } else {
    1.0
  };
  let image = image::RgbaImage::from_raw(pixels.width, pixels.height, pixels.rgba)
    .ok_or_else(|| backend("failed to decode captured window RGBA image"))?;
  Ok(Capture {
    image,
    bounds: window.frame,
    scale_factor,
    backend: WINDOW_CAPTURE_BACKEND.to_string(),
    fallback_reason: None,
  })
}

#[cfg(not(target_os = "windows"))]
pub fn capture_window(_window: &Window) -> DriverResult<Capture> {
  Err(DriverError::unsupported("window.capture"))
}

#[cfg(target_os = "windows")]
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
          // NOTICE: xcap exposes `is_builtin` on macOS; on Windows the concept
          // is not meaningful, so the shared `is_builtin` flag stays None.
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
  if value < 0.0 || value > u32::MAX as f64 {
    return Err(invalid_input(format!(
      "region {name} is outside the capture backend range"
    )));
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

/// Win32 GDI window capture via `PrintWindow`.
///
/// Kept in a dedicated submodule so the FFI buffer handling and GDI object
/// lifetimes stay isolated from the cross-platform capture orchestration above.
#[cfg(target_os = "windows")]
mod window_native {
  use std::ffi::c_void;
  use std::mem::size_of;

  use auv_driver::error::DriverResult;
  use auv_driver::window::Window;
  use windows::Win32::Foundation::{HWND, RECT};
  use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleBitmap, CreateCompatibleDC,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, HDC, ReleaseDC, SelectObject,
  };
  use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
  use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, PW_RENDERFULLCONTENT};

  use crate::error::{backend, invalid_input};
  use crate::window::window_handle;

  /// Top-down RGBA pixels captured from a single window.
  pub(super) struct WindowPixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
  }

  pub(super) fn capture_window_rgba(window: &Window) -> DriverResult<WindowPixels> {
    let hwnd = window_handle(window)?;
    let (width, height) = window_pixel_size(hwnd)?;
    let bgra = print_window_bgra(hwnd, width, height)?;
    Ok(WindowPixels {
      width: width as u32,
      height: height as u32,
      rgba: bgra_to_rgba(bgra),
    })
  }

  fn window_pixel_size(hwnd: HWND) -> DriverResult<(i32, i32)> {
    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut rect) }
      .map_err(|error| backend(format!("GetWindowRect failed: {error}")))?;
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 {
      return Err(invalid_input(format!(
        "window has non-capturable size {width}x{height}"
      )));
    }
    Ok((width, height))
  }

  /// Renders the window into an off-screen DIB and reads back the raw BGRA
  /// pixels. GDI objects are released before any error is propagated.
  fn print_window_bgra(hwnd: HWND, width: i32, height: i32) -> DriverResult<Vec<u8>> {
    let window_dc = unsafe { GetDC(hwnd) };
    if window_dc.is_invalid() {
      return Err(backend("GetDC returned a null device context"));
    }
    let result = render_into_dib(hwnd, window_dc, width, height);
    unsafe { ReleaseDC(hwnd, window_dc) };
    result
  }

  fn render_into_dib(hwnd: HWND, window_dc: HDC, width: i32, height: i32) -> DriverResult<Vec<u8>> {
    let memory_dc = unsafe { CreateCompatibleDC(window_dc) };
    if memory_dc.is_invalid() {
      return Err(backend("CreateCompatibleDC failed"));
    }
    let bitmap = unsafe { CreateCompatibleBitmap(window_dc, width, height) };
    if bitmap.is_invalid() {
      let _ = unsafe { DeleteDC(memory_dc) };
      return Err(backend("CreateCompatibleBitmap failed"));
    }
    let previous = unsafe { SelectObject(memory_dc, bitmap) };

    // PW_RENDERFULLCONTENT renders DirectComposition/hardware-accelerated
    // surfaces (e.g. browsers) that a plain PrintWindow would leave blank.
    let printed = unsafe { PrintWindow(hwnd, memory_dc, PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT)) };

    let mut info = BITMAPINFO {
      bmiHeader: BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        // Negative height requests a top-down row order so the buffer matches
        // the RGBA image layout expected by the shared Capture type.
        biHeight: -height,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
      },
      ..Default::default()
    };
    let mut buffer = vec![0u8; width as usize * height as usize * 4];
    let scanlines = unsafe {
      GetDIBits(
        memory_dc,
        bitmap,
        0,
        height as u32,
        Some(buffer.as_mut_ptr() as *mut c_void),
        &mut info,
        DIB_RGB_COLORS,
      )
    };

    unsafe {
      SelectObject(memory_dc, previous);
      let _ = DeleteObject(bitmap);
      let _ = DeleteDC(memory_dc);
    }

    if !printed.as_bool() {
      return Err(backend("PrintWindow reported failure"));
    }
    if scanlines == 0 {
      return Err(backend("GetDIBits returned no scanlines"));
    }
    Ok(buffer)
  }

  /// Converts GDI's little-endian BGRA bytes to RGBA in place. GDI writes the
  /// alpha byte as 0 for opaque windows, so it is forced to fully opaque.
  fn bgra_to_rgba(mut buffer: Vec<u8>) -> Vec<u8> {
    for pixel in buffer.chunks_exact_mut(4) {
      pixel.swap(0, 2);
      pixel[3] = 0xff;
    }
    buffer
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn target(index: usize, id: &str, frame: Rect, is_primary: bool) -> DisplayTarget {
    DisplayTarget {
      index,
      display: Display {
        id: id.to_string(),
        name: Some(format!("display_{index}")),
        frame,
        coordinate_space: CoordinateSpace::Screen,
        scale_factor: 1.0,
        is_primary,
        is_builtin: None,
      },
    }
  }

  #[test]
  fn resolve_display_target_prefers_primary_without_selector() {
    let targets = vec![
      target(0, "a", Rect::new(0.0, 0.0, 100.0, 100.0), false),
      target(1, "b", Rect::new(100.0, 0.0, 100.0, 100.0), true),
    ];

    let resolved = resolve_display_target(&targets, None).expect("primary should resolve");

    assert_eq!(resolved.index, 1);
  }

  #[test]
  fn resolve_display_target_matches_selector_by_id_or_name() {
    let targets = vec![
      target(0, "a", Rect::new(0.0, 0.0, 100.0, 100.0), true),
      target(1, "b", Rect::new(100.0, 0.0, 100.0, 100.0), false),
    ];

    assert_eq!(
      resolve_display_target(&targets, Some("b")).unwrap().index,
      1
    );
    assert_eq!(
      resolve_display_target(&targets, Some("display_1"))
        .unwrap()
        .index,
      1
    );
    assert!(resolve_display_target(&targets, Some("missing")).is_err());
  }

  #[test]
  fn resolve_display_for_region_selects_containing_display() {
    let targets = vec![
      target(0, "a", Rect::new(0.0, 0.0, 100.0, 100.0), true),
      target(1, "b", Rect::new(100.0, 0.0, 100.0, 100.0), false),
    ];

    let region = Rect::new(110.0, 10.0, 20.0, 20.0);
    let resolved =
      resolve_display_for_region(&targets, None, region).expect("region is within display b");

    assert_eq!(resolved.index, 1);
  }

  #[test]
  fn resolve_display_for_region_rejects_region_spanning_displays() {
    let targets = vec![target(0, "a", Rect::new(0.0, 0.0, 100.0, 100.0), true)];

    let region = Rect::new(50.0, 50.0, 100.0, 10.0);

    assert!(resolve_display_for_region(&targets, None, region).is_err());
  }

  #[test]
  fn integral_capture_dimension_rejects_fractional_and_negative() {
    assert!(integral_capture_dimension("x", 10.5).is_err());
    assert!(integral_capture_dimension("x", -1.0).is_err());
    assert_eq!(integral_capture_dimension("x", 12.0).unwrap(), 12);
  }

  #[test]
  fn integral_positive_capture_dimension_rejects_zero() {
    assert!(integral_positive_capture_dimension("width", 0.0).is_err());
    assert_eq!(
      integral_positive_capture_dimension("width", 4.0).unwrap(),
      4
    );
  }
}
