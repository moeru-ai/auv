// File: src/driver/macos/capture/xcap_backend.rs
//! xcap-based screen/window capture backend.
//!
//! This module interfaces with `xcap` to list displays/windows and capture
//! screenshots, while producing AUV's normalized coordinate contracts
//! (logical/pixel transforms and capture-source descriptors).
//!
//! Boundary: this is about coordinate correctness + capture provenance, not UI
//! interpretation (OCR/AX) or action semantics.

use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub use auv_driver_macos::capture::geometry::{
  project_capture_pixel_to_global_logical, resolve_display_index, resolve_region,
  scale_from_logical_and_physical,
};

use super::types::{
  CaptureBackend, CaptureContract, CaptureSource, DisplayDescriptor, Rect, Size, capture_error,
};
use crate::model::AuvResult;

fn backend_failed(error: impl std::fmt::Display) -> String {
  format!("{}: {error}", capture_error::BACKEND_FAILED)
}

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn now_millis() -> u64 {
  let millis = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap_or_default()
    .as_millis();
  u64::try_from(millis).unwrap_or(u64::MAX)
}

pub fn screenshot_temp_path(label: &str) -> PathBuf {
  let sequence = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
  std::env::temp_dir().join(format!(
    "auv-{}-{}-{}-{}.png",
    sanitize_file_component(label),
    now_millis(),
    std::process::id(),
    sequence
  ))
}

fn sanitize_file_component(raw: &str) -> String {
  let sanitized: String = raw
    .chars()
    .map(|character| {
      if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
        character
      } else {
        '-'
      }
    })
    .collect();
  let trimmed = sanitized.trim_matches('-');
  if trimmed.is_empty() {
    "artifact".to_string()
  } else {
    trimmed.to_string()
  }
}

pub fn list_displays() -> AuvResult<Vec<DisplayDescriptor>> {
  let monitors = xcap::Monitor::all().map_err(backend_failed)?;
  descriptors_from_monitors(&monitors)
}

pub fn capture_main_display_to_path(label: &str) -> AuvResult<(PathBuf, CaptureContract)> {
  capture_display_to_path(label, None, None, true)
}

pub fn capture_display_to_path(
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

pub fn descriptors_from_monitors(monitors: &[xcap::Monitor]) -> AuvResult<Vec<DisplayDescriptor>> {
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

pub fn map_xcap_capture_error(error: impl std::fmt::Display) -> String {
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

pub fn save_rgba_image(image: image::RgbaImage, path: &Path) -> AuvResult<Size> {
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
  use auv_driver_macos::capture::geometry::display_index_for_window;

  use crate::driver::macos::capture::types::CoordinateSpace;
  use crate::driver::macos::capture::types::Scale2D;

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
