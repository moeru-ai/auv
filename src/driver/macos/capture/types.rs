use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Size {
  pub width: f64,
  pub height: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Rect {
  pub x: f64,
  pub y: f64,
  pub width: f64,
  pub height: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Scale2D {
  pub x: f64,
  pub y: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CaptureBackend {
  XcapMacos,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct DisplayDescriptor {
  pub display_ref: String,
  pub is_main: bool,
  pub is_builtin: bool,
  pub global_logical_bounds: Rect,
  pub visible_logical_bounds: Rect,
  pub physical_pixel_size: Size,
  pub scale_factor: f64,
  pub pixel_to_logical_scale: Scale2D,
  pub logical_to_pixel_scale: Scale2D,
  pub native_display_id: String,
  pub capture_backend: CaptureBackend,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct WindowDescriptor {
  pub window_ref: String,
  pub title: String,
  pub app_name: String,
  pub owner_bundle_id: Option<String>,
  pub owner_pid: Option<u32>,
  pub z_order: Option<i32>,
  pub is_focused: Option<bool>,
  pub is_minimized: Option<bool>,
  pub global_logical_bounds: Rect,
  pub display_ref: Option<String>,
  pub native_window_id: String,
  pub capture_backend: CaptureBackend,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CoordinateSpace {
  GlobalLogical,
  DisplayLogical,
  DisplayPhysical,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum CaptureSource {
  Display {
    display_ref: String,
    native_display_id: String,
  },
  Region {
    display_ref: String,
    native_display_id: String,
    input_space: CoordinateSpace,
  },
  Window {
    window_ref: String,
    display_ref: String,
    native_window_id: String,
    native_display_id: String,
  },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct CaptureContract {
  pub coordinate_contract_version: u32,
  pub capture_source: CaptureSource,
  pub capture_backend: CaptureBackend,
  pub include_shadow: bool,
  pub source_global_logical_bounds: Rect,
  pub source_physical_pixel_bounds: Rect,
  pub screenshot_pixel_size: Size,
  pub pixel_to_logical_scale: Scale2D,
  pub logical_to_pixel_scale: Scale2D,
  pub captured_at_unix_ms: u128,
}

pub(crate) mod capture_error {
  pub(crate) const UNSUPPORTED_BACKEND: &str = "capture.unsupported_backend";
  pub(crate) const PERMISSION_DENIED: &str = "capture.permission_denied";
  pub(crate) const DISPLAY_NOT_FOUND: &str = "capture.display_not_found";
  pub(crate) const STALE_DISPLAY_REF: &str = "capture.stale_display_ref";
  pub(crate) const STALE_WINDOW_REF: &str = "capture.stale_window_ref";
  pub(crate) const REGION_OUT_OF_BOUNDS: &str = "capture.region_out_of_bounds";
  pub(crate) const REGION_CROSSES_DISPLAYS: &str = "capture.region_crosses_displays";
  pub(crate) const COORDINATE_CONTRACT_STALE: &str = "capture.coordinate_contract_stale";
  pub(crate) const BACKEND_FAILED: &str = "capture.backend_failed";
}
