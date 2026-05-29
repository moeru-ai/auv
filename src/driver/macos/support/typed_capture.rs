use auv_driver::Driver as _;
use auv_driver::geometry::{CoordinateSpace as TypedCoordinateSpace, Rect as TypedRect};
use auv_driver::window::{Window as TypedWindow, WindowRef as TypedWindowRef};

use super::super::{DriverCall, WindowCandidate, now_millis};
use super::call::{app_identifier, parse_window_selection};
use super::display::maybe_activate_target_app_for_observation;
use crate::driver::macos::capture::types::{
  CaptureBackend, CaptureContract, CaptureSource, DisplayDescriptor, Rect, Scale2D, Size,
};
use crate::driver::macos::support::{
  parse_app_selector, resolve_app_ref, resolve_window_candidate, retry_window_capture_operation,
  window_capture_readiness_diagnostic,
};
use crate::model::AuvResult;
use auv_driver_macos::types::ScreenshotDimensions;

#[derive(Clone, Debug)]
pub(crate) struct TypedWindowCaptureObservation {
  pub(crate) candidate: WindowCandidate,
  pub(crate) capture: auv_driver::capture::Capture,
  pub(crate) contract: CaptureContract,
  pub(crate) dimensions: ScreenshotDimensions,
  pub(crate) display_ref: String,
}

pub(crate) fn capture_window_with_typed_session(
  call: &DriverCall,
  label: &str,
) -> AuvResult<TypedWindowCaptureObservation> {
  let app = app_identifier(call)
    .filter(|value| !value.is_empty())
    .ok_or_else(|| {
      "operation requires --target <application-id> or --app <application-id>".to_string()
    })?;
  let selection = parse_window_selection(call)?;
  let _ = maybe_activate_target_app_for_observation(call)?;
  let selector = parse_app_selector(&app)?;
  let displays = crate::driver::macos::capture::xcap_backend::list_displays()?;
  retry_window_capture_operation(|| {
    let snapshot = crate::driver::macos::observe::observe_windows_snapshot(128, &app)?;
    let resolved_app = resolve_app_ref(&snapshot, &selector)?;
    let candidate = resolve_window_candidate(&snapshot, &resolved_app, &displays, &selection)?;
    if selection.has_selector() && !candidate.is_fully_contained_in_display {
      return Err(window_capture_readiness_diagnostic(&candidate, &displays));
    }
    let native_window_id = candidate.native_window_id.as_deref().ok_or_else(|| {
      "resolved window candidate has no native window id; inspect `debug.listWindows`".to_string()
    })?;
    let display_ref = candidate.display_ref.clone().ok_or_else(|| {
      "resolved window candidate is not fully contained by one display".to_string()
    })?;
    let display = displays
      .iter()
      .find(|display| display.display_ref == display_ref)
      .ok_or_else(|| format!("resolved window display {display_ref} is missing"))?;
    let typed_window = typed_window_from_candidate(&candidate, native_window_id);
    let driver = auv_driver_macos::MacosDriver::new();
    let session = driver
      .open_local()
      .map_err(|error| format!("failed to open typed macOS driver session: {error}"))?;
    let capture = session
      .window()
      .capture(&typed_window)
      .map_err(|error| typed_capture_error(error, native_window_id))?;
    let dimensions = ScreenshotDimensions {
      width: i64::from(capture.image.width()),
      height: i64::from(capture.image.height()),
    };
    let contract =
      capture_contract_for_typed_window(&candidate, &capture, &dimensions, &display_ref, display)?;
    let _ = label;
    Ok(TypedWindowCaptureObservation {
      candidate,
      capture,
      contract,
      dimensions,
      display_ref,
    })
  })
}

fn typed_capture_error(error: auv_driver::DriverError, native_window_id: &str) -> String {
  format!(
    "{}: typed window capture failed for native window {native_window_id}: {error}",
    crate::driver::macos::capture::types::capture_error::BACKEND_FAILED
  )
}

fn typed_window_from_candidate(candidate: &WindowCandidate, native_window_id: &str) -> TypedWindow {
  let window = &candidate.window_ref;
  TypedWindow {
    reference: TypedWindowRef {
      id: native_window_id.to_string(),
    },
    title: (!window.title.is_empty()).then(|| window.title.clone()),
    app_name: (!window.app_name.is_empty()).then(|| window.app_name.clone()),
    app_bundle_id: (!window.owner_bundle_id.is_empty()).then(|| window.owner_bundle_id.clone()),
    process_id: u32::try_from(window.owner_pid).ok(),
    frame: TypedRect::new(
      window.bounds.x as f64,
      window.bounds.y as f64,
      window.bounds.width as f64,
      window.bounds.height as f64,
    ),
    coordinate_space: TypedCoordinateSpace::Screen,
    is_main: candidate.is_main_candidate,
    is_visible: true,
  }
}

fn capture_contract_for_typed_window(
  candidate: &WindowCandidate,
  capture: &auv_driver::capture::Capture,
  dimensions: &ScreenshotDimensions,
  display_ref: &str,
  display: &DisplayDescriptor,
) -> AuvResult<CaptureContract> {
  let bounds = &candidate.window_ref.bounds;
  let source_global_logical_bounds = Rect {
    x: bounds.x as f64,
    y: bounds.y as f64,
    width: bounds.width as f64,
    height: bounds.height as f64,
  };
  let screenshot_pixel_size = Size {
    width: dimensions.width as f64,
    height: dimensions.height as f64,
  };
  if screenshot_pixel_size.width <= 0.0 || screenshot_pixel_size.height <= 0.0 {
    return Err("typed window capture returned an empty image".to_string());
  }
  Ok(CaptureContract {
    coordinate_contract_version: 1,
    capture_source: CaptureSource::Window {
      window_ref: format!("window_{}", candidate.window_ref.window_number),
      display_ref: display_ref.to_string(),
      native_window_id: candidate.native_window_id.clone().unwrap_or_default(),
      native_display_id: display.native_display_id.clone(),
    },
    capture_backend: capture_backend_from_typed(&capture.backend),
    include_shadow: false,
    source_global_logical_bounds: source_global_logical_bounds.clone(),
    source_physical_pixel_bounds: Rect {
      x: (source_global_logical_bounds.x - display.global_logical_bounds.x)
        * display.logical_to_pixel_scale.x,
      y: (source_global_logical_bounds.y - display.global_logical_bounds.y)
        * display.logical_to_pixel_scale.y,
      width: screenshot_pixel_size.width,
      height: screenshot_pixel_size.height,
    },
    screenshot_pixel_size: screenshot_pixel_size.clone(),
    pixel_to_logical_scale: Scale2D {
      x: source_global_logical_bounds.width / screenshot_pixel_size.width,
      y: source_global_logical_bounds.height / screenshot_pixel_size.height,
    },
    logical_to_pixel_scale: Scale2D {
      x: screenshot_pixel_size.width / source_global_logical_bounds.width,
      y: screenshot_pixel_size.height / source_global_logical_bounds.height,
    },
    captured_at_unix_ms: now_millis(),
  })
}

fn capture_backend_from_typed(backend: &str) -> CaptureBackend {
  if backend == "macos.screencapturekit.ffi" {
    CaptureBackend::ScreenCaptureKitMacos
  } else {
    CaptureBackend::XcapMacos
  }
}
