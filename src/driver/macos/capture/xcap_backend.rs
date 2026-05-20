use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::path::PathBuf;

use super::types::{
  CaptureBackend, CaptureContract, CaptureSource, CoordinateSpace, DisplayDescriptor, Rect,
  Scale2D, Size, WindowDescriptor, capture_error,
};
use crate::driver::macos::screenshot_temp_path;
use crate::model::{AuvResult, now_millis};

fn backend_failed(error: impl std::fmt::Display) -> String {
  format!("{}: {error}", capture_error::BACKEND_FAILED)
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ResolvedRegion {
  pub display_index: usize,
  pub display_local_logical: Rect,
  pub source_global_logical_bounds: Rect,
}

pub(crate) fn scale_from_logical_and_physical(
  logical: &Rect,
  physical: &Size,
) -> AuvResult<(Scale2D, Scale2D)> {
  if logical.width <= 0.0
    || logical.height <= 0.0
    || physical.width <= 0.0
    || physical.height <= 0.0
  {
    return Err(format!(
      "{}: logical and physical display sizes must be positive",
      capture_error::BACKEND_FAILED
    ));
  }

  let logical_to_pixel = Scale2D {
    x: physical.width / logical.width,
    y: physical.height / logical.height,
  };
  let pixel_to_logical = Scale2D {
    x: logical.width / physical.width,
    y: logical.height / physical.height,
  };

  Ok((pixel_to_logical, logical_to_pixel))
}

pub(crate) fn list_displays() -> AuvResult<Vec<DisplayDescriptor>> {
  let monitors = xcap::Monitor::all().map_err(backend_failed)?;
  descriptors_from_monitors(&monitors)
}

pub(crate) fn capture_main_display_to_path(label: &str) -> AuvResult<(PathBuf, CaptureContract)> {
  capture_display_to_path(label, None, None, true)
}

pub(crate) fn capture_display_to_path(
  label: &str,
  display_ref: Option<&str>,
  native_display_id: Option<&str>,
  main: bool,
) -> AuvResult<(PathBuf, CaptureContract)> {
  let monitors = xcap::Monitor::all().map_err(backend_failed)?;
  let displays = descriptors_from_monitors(&monitors)?;
  let display_index = resolve_display_index(&displays, display_ref, native_display_id, main)?;
  let display = displays
    .get(display_index)
    .ok_or_else(|| {
      format!(
        "{}: resolved display index {} is missing from the display descriptor list",
        capture_error::STALE_DISPLAY_REF,
        display_index
      )
    })?
    .clone();
  let monitor = monitors.get(display_index).ok_or_else(|| {
    format!(
      "{}: display {} disappeared before capture",
      capture_error::STALE_DISPLAY_REF,
      display.display_ref
    )
  })?;
  let image = monitor.capture_image().map_err(map_xcap_capture_error)?;
  let path = screenshot_temp_path(label);
  let screenshot_pixel_size = save_rgba_image(image, &path)?;
  let (pixel_to_logical_scale, logical_to_pixel_scale) =
    scale_from_logical_and_physical(&display.global_logical_bounds, &screenshot_pixel_size)?;
  let contract = CaptureContract {
    coordinate_contract_version: 1,
    capture_source: CaptureSource::Display {
      display_ref: display.display_ref.clone(),
      native_display_id: display.native_display_id.clone(),
    },
    capture_backend: CaptureBackend::XcapMacos,
    include_shadow: false,
    source_global_logical_bounds: display.global_logical_bounds.clone(),
    source_physical_pixel_bounds: Rect {
      x: 0.0,
      y: 0.0,
      width: screenshot_pixel_size.width,
      height: screenshot_pixel_size.height,
    },
    screenshot_pixel_size,
    pixel_to_logical_scale,
    logical_to_pixel_scale,
    captured_at_unix_ms: now_millis(),
  };
  Ok((path, contract))
}

pub(crate) fn descriptor_from_window(
  index: usize,
  window: &xcap::Window,
  displays: &[DisplayDescriptor],
  bundle_ids: &HashMap<u32, String>,
) -> AuvResult<WindowDescriptor> {
  let owner_pid = window.pid().map_err(backend_failed)?;
  let bounds = Rect {
    x: window.x().map_err(backend_failed)? as f64,
    y: window.y().map_err(backend_failed)? as f64,
    width: window.width().map_err(backend_failed)? as f64,
    height: window.height().map_err(backend_failed)? as f64,
  };
  let display_ref = display_index_for_window(displays, &bounds)
    .ok()
    .map(|display_index| displays[display_index].display_ref.clone());

  Ok(WindowDescriptor {
    window_ref: format!("window_{index}"),
    title: window.title().map_err(backend_failed)?,
    app_name: window.app_name().map_err(backend_failed)?,
    owner_bundle_id: bundle_ids.get(&owner_pid).cloned(),
    owner_pid: Some(owner_pid),
    z_order: Some(window.z().map_err(backend_failed)?),
    is_focused: Some(window.is_focused().map_err(backend_failed)?),
    is_minimized: Some(window.is_minimized().map_err(backend_failed)?),
    global_logical_bounds: bounds,
    display_ref,
    native_window_id: window.id().map_err(backend_failed)?.to_string(),
    capture_backend: CaptureBackend::XcapMacos,
  })
}

pub(crate) fn capture_window_native_id_to_path(
  label: &str,
  native_window_id: &str,
  window_number: i64,
) -> AuvResult<(PathBuf, CaptureContract, WindowDescriptor)> {
  let displays = list_displays()?;
  let windows = xcap::Window::all().map_err(|error| {
    format!(
      "{}: failed to re-enumerate windows before capture: {error}",
      capture_error::BACKEND_FAILED
    )
  })?;
  for window in &windows {
    let Ok(id) = window.id() else {
      continue;
    };
    if id.to_string() != native_window_id {
      continue;
    }

    let pids = [window.pid().map_err(|error| {
      format!(
        "{}: failed to read refreshed window pid: {error}",
        capture_error::STALE_WINDOW_REF
      )
    })?]
    .into_iter()
    .collect::<HashSet<_>>();
    let bundle_ids = bundle_ids_by_pid(&pids).unwrap_or_else(|_| HashMap::new());
    let selected = descriptor_from_window(
      window_number.max(0) as usize,
      window,
      &displays,
      &bundle_ids,
    )
    .map_err(|error| {
      format!(
        "{}: failed to refresh selected window descriptor: {error}",
        capture_error::STALE_WINDOW_REF
      )
    })?;
    let display_ref = selected.display_ref.clone().ok_or_else(|| {
      format!(
        "{}: refreshed window is not fully contained by one display",
        capture_error::STALE_WINDOW_REF
      )
    })?;
    let display = displays
      .iter()
      .find(|display| display.display_ref == display_ref)
      .ok_or_else(|| {
        format!(
          "{}: refreshed window display {} is missing from the display list",
          capture_error::STALE_DISPLAY_REF,
          display_ref
        )
      })?;
    let image = window.capture_image().map_err(map_xcap_capture_error)?;
    let path = screenshot_temp_path(label);
    let screenshot_pixel_size = save_rgba_image(image, &path)?;
    let pixel_to_logical_scale = Scale2D {
      x: selected.global_logical_bounds.width / screenshot_pixel_size.width,
      y: selected.global_logical_bounds.height / screenshot_pixel_size.height,
    };
    let logical_to_pixel_scale = Scale2D {
      x: screenshot_pixel_size.width / selected.global_logical_bounds.width,
      y: screenshot_pixel_size.height / selected.global_logical_bounds.height,
    };
    let contract = CaptureContract {
      coordinate_contract_version: 1,
      capture_source: CaptureSource::Window {
        window_ref: selected.window_ref.clone(),
        display_ref: display_ref.clone(),
        native_window_id: selected.native_window_id.clone(),
        native_display_id: display.native_display_id.clone(),
      },
      capture_backend: CaptureBackend::XcapMacos,
      include_shadow: false,
      source_global_logical_bounds: selected.global_logical_bounds.clone(),
      source_physical_pixel_bounds: Rect {
        x: (selected.global_logical_bounds.x - display.global_logical_bounds.x)
          * display.logical_to_pixel_scale.x,
        y: (selected.global_logical_bounds.y - display.global_logical_bounds.y)
          * display.logical_to_pixel_scale.y,
        width: screenshot_pixel_size.width,
        height: screenshot_pixel_size.height,
      },
      screenshot_pixel_size,
      pixel_to_logical_scale,
      logical_to_pixel_scale,
      captured_at_unix_ms: now_millis(),
    };
    return Ok((path, contract, selected));
  }

  Err(format!(
    "{}: selected window {} disappeared before capture",
    capture_error::STALE_WINDOW_REF,
    native_window_id
  ))
}

/// Fallback window capture for windows where `kCGWindowSharingState == 0`.
///
/// xcap filters those out during enumeration, but `CGWindowListCreateImage` can
/// still capture them when Screen Recording is granted.  We call the CoreGraphics
/// API directly (in-process) so the TCC permission from the auv-cli process applies.
///
/// `logical_bounds` must be in global macOS screen coordinates (y=0 at top), the
/// same coordinate space that `kCGWindowBounds` returns.
pub(crate) fn capture_window_cg_to_path(
  label: &str,
  window_number: i64,
  logical_bounds: &Rect,
  displays: &[DisplayDescriptor],
) -> AuvResult<(PathBuf, CaptureContract, WindowDescriptor)> {
  #[cfg(target_os = "macos")]
  {
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    #[allow(deprecated)]
    use objc2_core_graphics::CGWindowListCreateImage;
    use objc2_core_graphics::{
      CGDataProvider, CGImage, CGWindowID, CGWindowImageOption, CGWindowListOption,
    };

    let cg_rect = CGRect {
      origin: CGPoint {
        x: logical_bounds.x,
        y: logical_bounds.y,
      },
      size: CGSize {
        width: logical_bounds.width,
        height: logical_bounds.height,
      },
    };
    let window_id = window_number.max(0) as CGWindowID;

    #[allow(deprecated)]
    let image = CGWindowListCreateImage(
      cg_rect,
      CGWindowListOption::OptionIncludingWindow,
      window_id,
      CGWindowImageOption::Default,
    )
    .ok_or_else(|| {
      format!(
        "{}: CGWindowListCreateImage returned nil for window {}",
        capture_error::BACKEND_FAILED,
        window_number
      )
    })?;

    let (pixel_width, pixel_height, rgba) = {
      let w = CGImage::width(Some(&image));
      let h = CGImage::height(Some(&image));
      let data_provider = CGImage::data_provider(Some(&image));
      let data = CGDataProvider::data(data_provider.as_deref())
        .ok_or_else(|| {
          format!(
            "{}: failed to get pixel data for window {}",
            capture_error::BACKEND_FAILED,
            window_number
          )
        })?
        .to_vec();
      let bytes_per_row = CGImage::bytes_per_row(Some(&image));
      let mut buffer = Vec::with_capacity(w * h * 4);
      for row in data.chunks_exact(bytes_per_row) {
        buffer.extend_from_slice(&row[..w * 4]);
      }
      for bgra in buffer.chunks_exact_mut(4) {
        bgra.swap(0, 2);
      }
      let img = image::RgbaImage::from_raw(w as u32, h as u32, buffer).ok_or_else(|| {
        format!(
          "{}: failed to build RgbaImage from CG data for window {}",
          capture_error::BACKEND_FAILED,
          window_number
        )
      })?;
      (w as f64, h as f64, img)
    };

    if pixel_width <= 0.0 || pixel_height <= 0.0 {
      return Err(format!(
        "{}: CGWindowListCreateImage returned zero-size image for window {}",
        capture_error::BACKEND_FAILED,
        window_number
      ));
    }

    let path = screenshot_temp_path(label);
    save_rgba_image(rgba, &path)?;

    let screenshot_pixel_size = Size {
      width: pixel_width,
      height: pixel_height,
    };
    let source_global_logical_bounds = logical_bounds.clone();
    let logical_x = logical_bounds.x;
    let logical_y = logical_bounds.y;
    let logical_width = logical_bounds.width;
    let logical_height = logical_bounds.height;
    let pixel_to_logical_scale = Scale2D {
      x: logical_width / pixel_width,
      y: logical_height / pixel_height,
    };
    let logical_to_pixel_scale = Scale2D {
      x: pixel_width / logical_width,
      y: pixel_height / logical_height,
    };

    let display_ref = display_index_for_window(displays, &source_global_logical_bounds)
      .ok()
      .map(|idx| displays[idx].display_ref.clone());
    let native_display_id = display_ref
      .as_ref()
      .and_then(|dref| displays.iter().find(|d| &d.display_ref == dref))
      .map(|d| d.native_display_id.clone());

    let source_physical_pixel_bounds = display_ref
      .as_ref()
      .and_then(|dref| displays.iter().find(|d| &d.display_ref == dref))
      .map(|display| Rect {
        x: (logical_x - display.global_logical_bounds.x) * display.logical_to_pixel_scale.x,
        y: (logical_y - display.global_logical_bounds.y) * display.logical_to_pixel_scale.y,
        width: pixel_width,
        height: pixel_height,
      })
      .unwrap_or(Rect {
        x: 0.0,
        y: 0.0,
        width: pixel_width,
        height: pixel_height,
      });

    let window_ref = format!("window_{}", window_number.max(0));
    let capture_source = CaptureSource::Window {
      window_ref: window_ref.clone(),
      display_ref: display_ref.clone().unwrap_or_default(),
      native_window_id: window_number.to_string(),
      native_display_id: native_display_id.clone().unwrap_or_default(),
    };
    let contract = CaptureContract {
      coordinate_contract_version: 1,
      capture_source,
      capture_backend: CaptureBackend::XcapMacos,
      include_shadow: false,
      source_global_logical_bounds: source_global_logical_bounds.clone(),
      source_physical_pixel_bounds,
      screenshot_pixel_size,
      pixel_to_logical_scale,
      logical_to_pixel_scale,
      captured_at_unix_ms: crate::model::now_millis(),
    };
    let descriptor = WindowDescriptor {
      window_ref,
      title: String::new(),
      app_name: String::new(),
      owner_bundle_id: None,
      owner_pid: None,
      z_order: None,
      is_focused: None,
      is_minimized: None,
      global_logical_bounds: source_global_logical_bounds,
      display_ref,
      native_window_id: window_number.to_string(),
      capture_backend: CaptureBackend::XcapMacos,
    };

    return Ok((path, contract, descriptor));
  }
  #[cfg(not(target_os = "macos"))]
  Err(format!(
    "{}: CG window capture is only supported on macOS",
    capture_error::BACKEND_FAILED
  ))
}

pub(crate) fn bundle_ids_by_pid(pids: &HashSet<u32>) -> AuvResult<HashMap<u32, String>> {
  crate::driver::macos::native::window::bundle_ids_by_pid(pids).map_err(backend_failed)
}

pub(crate) fn descriptors_from_monitors(
  monitors: &[xcap::Monitor],
) -> AuvResult<Vec<DisplayDescriptor>> {
  if monitors.is_empty() {
    return Err(format!(
      "{}: no displays were reported by the capture backend",
      capture_error::DISPLAY_NOT_FOUND
    ));
  }

  monitors
    .iter()
    .enumerate()
    .map(|(index, monitor)| {
      let x = monitor.x().map_err(backend_failed)? as f64;
      let y = monitor.y().map_err(backend_failed)? as f64;
      let width = monitor.width().map_err(backend_failed)? as f64;
      let height = monitor.height().map_err(backend_failed)? as f64;
      let scale_factor = monitor.scale_factor().map_err(backend_failed)? as f64;
      let global_logical_bounds = Rect {
        x,
        y,
        width,
        height,
      };
      let visible_logical_bounds = global_logical_bounds.clone();
      let physical_pixel_size = Size {
        width: width * scale_factor,
        height: height * scale_factor,
      };
      let (pixel_to_logical_scale, logical_to_pixel_scale) =
        scale_from_logical_and_physical(&global_logical_bounds, &physical_pixel_size)?;

      Ok(DisplayDescriptor {
        display_ref: format!("display_{index}"),
        is_main: monitor.is_primary().map_err(backend_failed)?,
        is_builtin: monitor.is_builtin().map_err(backend_failed)?,
        global_logical_bounds,
        visible_logical_bounds,
        physical_pixel_size,
        scale_factor,
        pixel_to_logical_scale,
        logical_to_pixel_scale,
        native_display_id: monitor.id().map_err(backend_failed)?.to_string(),
        capture_backend: CaptureBackend::XcapMacos,
      })
    })
    .collect()
}

pub(crate) fn resolve_display_index(
  displays: &[DisplayDescriptor],
  display_ref: Option<&str>,
  display_id: Option<&str>,
  main: bool,
) -> AuvResult<usize> {
  if let Some(display_ref) = display_ref {
    return displays
      .iter()
      .position(|display| display.display_ref == display_ref)
      .ok_or_else(|| {
        format!(
          "{}: display_ref {} was not reported by the capture backend",
          capture_error::DISPLAY_NOT_FOUND,
          display_ref
        )
      });
  }

  if let Some(display_id) = display_id {
    return displays
      .iter()
      .position(|display| display.native_display_id == display_id)
      .ok_or_else(|| {
        format!(
          "{}: native display id {} was not reported by the capture backend",
          capture_error::DISPLAY_NOT_FOUND,
          display_id
        )
      });
  }

  if main && let Some(index) = displays.iter().position(|display| display.is_main) {
    return Ok(index);
  }

  if displays.is_empty() {
    return Err(format!(
      "{}: no displays were reported by the capture backend",
      capture_error::DISPLAY_NOT_FOUND
    ));
  }

  Ok(0)
}

pub(crate) fn display_index_for_window(
  displays: &[DisplayDescriptor],
  window_bounds: &Rect,
) -> AuvResult<usize> {
  let containing_displays = displays
    .iter()
    .enumerate()
    .filter(|(_, display)| rect_contains_rect(&display.global_logical_bounds, window_bounds))
    .collect::<Vec<_>>();

  if containing_displays.len() != 1 {
    return Err(format!(
      "{}: window bounds are not fully contained by exactly one display",
      capture_error::STALE_WINDOW_REF
    ));
  }

  Ok(containing_displays[0].0)
}

pub(crate) fn resolve_region(
  displays: &[DisplayDescriptor],
  input: Rect,
  coordinate_space: CoordinateSpace,
  display_ref: Option<&str>,
  display_id: Option<&str>,
) -> AuvResult<ResolvedRegion> {
  if input.width <= 0.0 || input.height <= 0.0 {
    return Err(format!(
      "{}: capture region size must be positive",
      capture_error::REGION_OUT_OF_BOUNDS
    ));
  }

  match coordinate_space {
    CoordinateSpace::GlobalLogical => resolve_global_logical_region(displays, input),
    CoordinateSpace::DisplayLogical => {
      require_display_selector(display_ref, display_id, "display_logical")?;
      let display_index = resolve_display_index(displays, display_ref, display_id, false)?;
      resolve_display_logical_region(displays, display_index, input)
    }
    CoordinateSpace::DisplayPhysical => {
      require_display_selector(display_ref, display_id, "display_physical")?;
      let display_index = resolve_display_index(displays, display_ref, display_id, false)?;
      let display = displays.get(display_index).ok_or_else(|| {
        format!(
          "{}: resolved display index {} is missing from the display descriptor list",
          capture_error::STALE_DISPLAY_REF,
          display_index
        )
      })?;
      resolve_display_logical_region(
        displays,
        display_index,
        Rect {
          x: input.x * display.pixel_to_logical_scale.x,
          y: input.y * display.pixel_to_logical_scale.y,
          width: input.width * display.pixel_to_logical_scale.x,
          height: input.height * display.pixel_to_logical_scale.y,
        },
      )
    }
  }
}

pub(crate) fn project_capture_pixel_to_global_logical(
  contract: &CaptureContract,
  screenshot_x: f64,
  screenshot_y: f64,
) -> AuvResult<(f64, f64)> {
  if !screenshot_x.is_finite() || !screenshot_y.is_finite() {
    return Err(format!(
      "{}: screenshot point must be finite",
      capture_error::COORDINATE_CONTRACT_STALE
    ));
  }
  if screenshot_x < 0.0
    || screenshot_y < 0.0
    || screenshot_x >= contract.screenshot_pixel_size.width
    || screenshot_y >= contract.screenshot_pixel_size.height
  {
    return Err(format!(
      "{}: screenshot point ({:.3}, {:.3}) is outside capture pixels {:.0}x{:.0}",
      capture_error::COORDINATE_CONTRACT_STALE,
      screenshot_x,
      screenshot_y,
      contract.screenshot_pixel_size.width,
      contract.screenshot_pixel_size.height
    ));
  }

  Ok((
    contract.source_global_logical_bounds.x + screenshot_x * contract.pixel_to_logical_scale.x,
    contract.source_global_logical_bounds.y + screenshot_y * contract.pixel_to_logical_scale.y,
  ))
}

fn resolve_global_logical_region(
  displays: &[DisplayDescriptor],
  input: Rect,
) -> AuvResult<ResolvedRegion> {
  let containing_displays = displays
    .iter()
    .enumerate()
    .filter(|(_, display)| rect_contains_rect(&display.global_logical_bounds, &input))
    .collect::<Vec<_>>();

  if containing_displays.len() != 1 {
    let intersecting_count = displays
      .iter()
      .filter(|display| rect_intersects_rect(&display.global_logical_bounds, &input))
      .count();
    if intersecting_count > 1 {
      return Err(format!(
        "{}: global logical region crosses display boundaries",
        capture_error::REGION_CROSSES_DISPLAYS
      ));
    }
    return Err(format!(
      "{}: global logical region is not fully contained by one display",
      capture_error::REGION_OUT_OF_BOUNDS
    ));
  }

  let (display_index, display) = containing_displays[0];
  Ok(ResolvedRegion {
    display_index,
    display_local_logical: Rect {
      x: input.x - display.global_logical_bounds.x,
      y: input.y - display.global_logical_bounds.y,
      width: input.width,
      height: input.height,
    },
    source_global_logical_bounds: input,
  })
}

fn resolve_display_logical_region(
  displays: &[DisplayDescriptor],
  display_index: usize,
  input: Rect,
) -> AuvResult<ResolvedRegion> {
  let display = displays.get(display_index).ok_or_else(|| {
    format!(
      "{}: resolved display index {} is missing from the display descriptor list",
      capture_error::STALE_DISPLAY_REF,
      display_index
    )
  })?;
  let display_local_bounds = Rect {
    x: 0.0,
    y: 0.0,
    width: display.global_logical_bounds.width,
    height: display.global_logical_bounds.height,
  };
  if !rect_contains_rect(&display_local_bounds, &input) {
    return Err(format!(
      "{}: display-local region is outside {}",
      capture_error::REGION_OUT_OF_BOUNDS,
      display.display_ref
    ));
  }

  Ok(ResolvedRegion {
    display_index,
    display_local_logical: input.clone(),
    source_global_logical_bounds: Rect {
      x: display.global_logical_bounds.x + input.x,
      y: display.global_logical_bounds.y + input.y,
      width: input.width,
      height: input.height,
    },
  })
}

fn rect_contains_rect(container: &Rect, candidate: &Rect) -> bool {
  candidate.x >= container.x
    && candidate.y >= container.y
    && candidate.x + candidate.width <= container.x + container.width
    && candidate.y + candidate.height <= container.y + container.height
}

fn rect_intersects_rect(a: &Rect, b: &Rect) -> bool {
  a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.y + b.height && a.y + a.height > b.y
}

fn require_display_selector(
  display_ref: Option<&str>,
  display_id: Option<&str>,
  coordinate_space: &str,
) -> AuvResult<()> {
  if display_ref.is_some() || display_id.is_some() {
    return Ok(());
  }
  Err(format!(
    "{}: {} coordinates require display_ref or display_id",
    capture_error::DISPLAY_NOT_FOUND,
    coordinate_space
  ))
}

pub(crate) fn map_xcap_capture_error(error: impl std::fmt::Display) -> String {
  let rendered = error.to_string();
  let normalized = rendered.to_ascii_lowercase();
  let code = if normalized.contains("invalidcaptureregion")
    || normalized.contains("invalid capture region")
  {
    capture_error::REGION_OUT_OF_BOUNDS
  } else if normalized.contains("notsupported") || normalized.contains("not supported") {
    capture_error::UNSUPPORTED_BACKEND
  } else if normalized.contains("permission")
    || normalized.contains("denied")
    || normalized.contains("not authorized")
  {
    capture_error::PERMISSION_DENIED
  } else {
    capture_error::BACKEND_FAILED
  };
  format!("{code}: {rendered}")
}

pub(crate) fn save_rgba_image(image: image::RgbaImage, path: &Path) -> AuvResult<Size> {
  let size = Size {
    width: image.width() as f64,
    height: image.height() as f64,
  };
  image.save(path).map_err(|error| {
    format!(
      "{}: failed to save captured image {}: {error}",
      capture_error::BACKEND_FAILED,
      path.display()
    )
  })?;
  Ok(size)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn test_display(display_ref: &str, native_display_id: &str, is_main: bool) -> DisplayDescriptor {
    test_display_with_bounds(
      display_ref,
      native_display_id,
      is_main,
      Rect {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 50.0,
      },
    )
  }

  fn test_display_with_bounds(
    display_ref: &str,
    native_display_id: &str,
    is_main: bool,
    bounds: Rect,
  ) -> DisplayDescriptor {
    let physical_pixel_size = Size {
      width: bounds.width * 2.0,
      height: bounds.height * 2.0,
    };
    DisplayDescriptor {
      display_ref: display_ref.to_string(),
      is_main,
      is_builtin: false,
      global_logical_bounds: bounds.clone(),
      visible_logical_bounds: bounds,
      physical_pixel_size,
      scale_factor: 2.0,
      pixel_to_logical_scale: Scale2D { x: 0.5, y: 0.5 },
      logical_to_pixel_scale: Scale2D { x: 2.0, y: 2.0 },
      native_display_id: native_display_id.to_string(),
      capture_backend: CaptureBackend::XcapMacos,
    }
  }

  fn test_display_contract() -> CaptureContract {
    CaptureContract {
      coordinate_contract_version: 1,
      capture_source: CaptureSource::Display {
        display_ref: "display_0".to_string(),
        native_display_id: "100".to_string(),
      },
      capture_backend: CaptureBackend::XcapMacos,
      include_shadow: false,
      source_global_logical_bounds: Rect {
        x: 3000.0,
        y: -100.0,
        width: 100.0,
        height: 50.0,
      },
      source_physical_pixel_bounds: Rect {
        x: 0.0,
        y: 0.0,
        width: 200.0,
        height: 100.0,
      },
      screenshot_pixel_size: Size {
        width: 200.0,
        height: 100.0,
      },
      pixel_to_logical_scale: Scale2D { x: 0.5, y: 0.5 },
      logical_to_pixel_scale: Scale2D { x: 2.0, y: 2.0 },
      captured_at_unix_ms: 1,
    }
  }

  #[test]
  fn resolves_by_display_ref() {
    let displays = vec![
      test_display("display_0", "100", false),
      test_display("display_1", "101", true),
    ];

    let index = resolve_display_index(&displays, Some("display_1"), None, false).unwrap();

    assert_eq!(index, 1);
  }

  #[test]
  fn resolves_main_display() {
    let displays = vec![
      test_display("display_0", "100", false),
      test_display("display_1", "101", true),
    ];

    let index = resolve_display_index(&displays, None, None, true).unwrap();

    assert_eq!(index, 1);
  }

  #[test]
  fn missing_display_ref_has_capture_error_code() {
    let displays = vec![test_display("display_0", "100", true)];

    let error = resolve_display_index(&displays, Some("display_9"), None, false)
      .expect_err("missing display_ref should fail");

    assert!(error.contains(capture_error::DISPLAY_NOT_FOUND));
  }

  #[test]
  fn resolves_global_region_on_negative_origin_display() {
    let displays = vec![
      test_display("display_0", "100", true),
      test_display_with_bounds(
        "display_1",
        "101",
        false,
        Rect {
          x: -120.0,
          y: 10.0,
          width: 120.0,
          height: 80.0,
        },
      ),
    ];

    let resolved = resolve_region(
      &displays,
      Rect {
        x: -100.0,
        y: 20.0,
        width: 40.0,
        height: 30.0,
      },
      CoordinateSpace::GlobalLogical,
      None,
      None,
    )
    .unwrap();

    assert_eq!(resolved.display_index, 1);
    assert_eq!(
      resolved.display_local_logical,
      Rect {
        x: 20.0,
        y: 10.0,
        width: 40.0,
        height: 30.0,
      }
    );
    assert_eq!(
      resolved.source_global_logical_bounds,
      Rect {
        x: -100.0,
        y: 20.0,
        width: 40.0,
        height: 30.0,
      }
    );
  }

  #[test]
  fn rejects_region_crossing_displays() {
    let displays = vec![
      test_display_with_bounds(
        "display_0",
        "100",
        true,
        Rect {
          x: 0.0,
          y: 0.0,
          width: 100.0,
          height: 100.0,
        },
      ),
      test_display_with_bounds(
        "display_1",
        "101",
        false,
        Rect {
          x: 100.0,
          y: 0.0,
          width: 100.0,
          height: 100.0,
        },
      ),
    ];

    let error = resolve_region(
      &displays,
      Rect {
        x: 90.0,
        y: 10.0,
        width: 20.0,
        height: 20.0,
      },
      CoordinateSpace::GlobalLogical,
      None,
      None,
    )
    .expect_err("region crossing displays should fail");

    assert!(error.contains(capture_error::REGION_CROSSES_DISPLAYS));
  }

  #[test]
  fn assigns_window_to_single_display() {
    let displays = vec![
      test_display_with_bounds(
        "display_0",
        "100",
        true,
        Rect {
          x: 0.0,
          y: 0.0,
          width: 100.0,
          height: 100.0,
        },
      ),
      test_display_with_bounds(
        "display_1",
        "101",
        false,
        Rect {
          x: 100.0,
          y: 0.0,
          width: 100.0,
          height: 100.0,
        },
      ),
    ];

    let index = display_index_for_window(
      &displays,
      &Rect {
        x: 110.0,
        y: 10.0,
        width: 40.0,
        height: 30.0,
      },
    )
    .unwrap();

    assert_eq!(index, 1);
  }

  #[test]
  fn rejects_window_spanning_displays() {
    let displays = vec![
      test_display_with_bounds(
        "display_0",
        "100",
        true,
        Rect {
          x: 0.0,
          y: 0.0,
          width: 100.0,
          height: 100.0,
        },
      ),
      test_display_with_bounds(
        "display_1",
        "101",
        false,
        Rect {
          x: 100.0,
          y: 0.0,
          width: 100.0,
          height: 100.0,
        },
      ),
    ];

    let error = display_index_for_window(
      &displays,
      &Rect {
        x: 90.0,
        y: 10.0,
        width: 20.0,
        height: 20.0,
      },
    )
    .expect_err("window crossing displays should fail");

    assert!(error.contains(capture_error::STALE_WINDOW_REF));
  }

  #[test]
  fn converts_display_physical_to_logical() {
    let displays = vec![test_display("display_0", "100", true)];

    let resolved = resolve_region(
      &displays,
      Rect {
        x: 20.0,
        y: 10.0,
        width: 40.0,
        height: 20.0,
      },
      CoordinateSpace::DisplayPhysical,
      Some("display_0"),
      None,
    )
    .unwrap();

    assert_eq!(
      resolved.display_local_logical,
      Rect {
        x: 10.0,
        y: 5.0,
        width: 20.0,
        height: 10.0,
      }
    );
    assert_eq!(
      resolved.source_global_logical_bounds,
      Rect {
        x: 10.0,
        y: 5.0,
        width: 20.0,
        height: 10.0,
      }
    );
  }

  #[test]
  fn display_logical_region_requires_display_selector() {
    let displays = vec![test_display("display_0", "100", true)];

    let error = resolve_region(
      &displays,
      Rect {
        x: 0.0,
        y: 0.0,
        width: 10.0,
        height: 10.0,
      },
      CoordinateSpace::DisplayLogical,
      None,
      None,
    )
    .expect_err("display-local coordinates need an explicit display");

    assert!(error.contains(capture_error::DISPLAY_NOT_FOUND));
  }

  #[test]
  fn rejects_global_region_outside_display_bounds() {
    let displays = vec![test_display_with_bounds(
      "display_0",
      "100",
      true,
      Rect {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 100.0,
      },
    )];

    let error = resolve_region(
      &displays,
      Rect {
        x: 120.0,
        y: 10.0,
        width: 10.0,
        height: 10.0,
      },
      CoordinateSpace::GlobalLogical,
      None,
      None,
    )
    .expect_err("outside region should fail");

    assert!(error.contains(capture_error::REGION_OUT_OF_BOUNDS));
  }

  #[test]
  fn scale_from_retina_display_sizes() {
    let logical = Rect {
      x: 0.0,
      y: 0.0,
      width: 3008.0,
      height: 1692.0,
    };
    let physical = Size {
      width: 6016.0,
      height: 3384.0,
    };

    let (pixel_to_logical, logical_to_pixel) =
      scale_from_logical_and_physical(&logical, &physical).unwrap();

    assert_eq!(pixel_to_logical, Scale2D { x: 0.5, y: 0.5 });
    assert_eq!(logical_to_pixel, Scale2D { x: 2.0, y: 2.0 });
  }

  #[test]
  fn projects_capture_pixels_to_global_logical_point() {
    let (x, y) = project_capture_pixel_to_global_logical(&test_display_contract(), 20.0, 10.0)
      .expect("capture projection should succeed");

    assert_eq!((x, y), (3010.0, -95.0));
  }

  #[test]
  fn rejects_capture_pixel_projection_outside_screenshot() {
    let error = project_capture_pixel_to_global_logical(&test_display_contract(), 201.0, 10.0)
      .expect_err("outside screenshot point should fail");

    assert!(error.contains(capture_error::COORDINATE_CONTRACT_STALE));
  }

  #[test]
  fn rejects_capture_pixel_projection_on_exclusive_edge() {
    let x_error = project_capture_pixel_to_global_logical(&test_display_contract(), 200.0, 10.0)
      .expect_err("right edge should be outside screenshot pixels");
    let y_error = project_capture_pixel_to_global_logical(&test_display_contract(), 20.0, 100.0)
      .expect_err("bottom edge should be outside screenshot pixels");

    assert!(x_error.contains(capture_error::COORDINATE_CONTRACT_STALE));
    assert!(y_error.contains(capture_error::COORDINATE_CONTRACT_STALE));
  }

  #[test]
  fn rejects_zero_sized_display() {
    let logical = Rect {
      x: 0.0,
      y: 0.0,
      width: 0.0,
      height: 1692.0,
    };
    let physical = Size {
      width: 6016.0,
      height: 3384.0,
    };

    let error = scale_from_logical_and_physical(&logical, &physical)
      .expect_err("zero-sized logical display should fail");

    assert!(error.contains(capture_error::BACKEND_FAILED));
  }
}
