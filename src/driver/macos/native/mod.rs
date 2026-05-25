// TODO(driver-crates): temporary root-native compatibility while legacy macOS
// command code migrates to typed `auv-driver-macos` session APIs.

pub(crate) mod ax_tree {
  pub(crate) use auv_driver_macos::native::ax_tree::*;
}

pub(crate) mod clipboard {
  pub(crate) use auv_driver_macos::native::clipboard::*;
}

pub(crate) mod ocr {
  pub(crate) use auv_driver_macos::native::ocr::*;
}

pub(crate) mod permission {
  pub(crate) use auv_driver_macos::native::permission::*;
}

pub(crate) mod pointer {
  pub(crate) use auv_driver_macos::native::pointer::*;
}

pub(crate) mod window {
  pub(crate) use auv_driver_macos::native::window::*;
}

pub(crate) mod overlay {
  use crate::model::AuvResult;

  pub(crate) fn show_cursor(x: f64, y: f64, label: &str) -> AuvResult<()> {
    auv_overlay_macos::show_cursor(x, y, label)
  }

  pub(crate) fn show_dual_cursor(x: f64, y: f64, label: &str, user_label: &str) -> AuvResult<()> {
    auv_overlay_macos::show_dual_cursor(x, y, label, user_label)
  }

  pub(crate) fn set_cursor(
    cursor_id: &str,
    x: f64,
    y: f64,
    label: &str,
    variant: &str,
  ) -> AuvResult<()> {
    auv_overlay_macos::set_cursor(cursor_id, x, y, label, variant)
  }

  pub(crate) fn move_cursor(
    cursor_id: &str,
    x: f64,
    y: f64,
    label: &str,
    variant: &str,
    duration_ms: u64,
  ) -> AuvResult<()> {
    auv_overlay_macos::move_cursor(cursor_id, x, y, label, variant, duration_ms)
  }

  pub(crate) fn move_dual_cursor(
    x: f64,
    y: f64,
    label: &str,
    user_label: &str,
    duration_ms: u64,
  ) -> AuvResult<()> {
    auv_overlay_macos::move_dual_cursor(x, y, label, user_label, duration_ms)
  }

  pub(crate) fn flash_cursor(x: f64, y: f64, label: &str, duration_ms: u64) -> AuvResult<()> {
    auv_overlay_macos::flash_cursor(x, y, label, duration_ms)
  }

  pub(crate) fn flash_cursor_id(
    cursor_id: &str,
    x: f64,
    y: f64,
    label: &str,
    duration_ms: u64,
  ) -> AuvResult<()> {
    auv_overlay_macos::flash_cursor_id(cursor_id, x, y, label, duration_ms)
  }

  pub(crate) fn hide_cursor_id(cursor_id: &str) -> AuvResult<()> {
    auv_overlay_macos::hide_cursor_id(cursor_id)
  }

  pub(crate) fn hide_cursor() -> AuvResult<()> {
    auv_overlay_macos::hide_cursor()
  }

  pub(crate) fn pump_events(duration_ms: u64) -> AuvResult<()> {
    auv_overlay_macos::pump_events(duration_ms)
  }

  pub(crate) fn shutdown() -> AuvResult<()> {
    auv_overlay_macos::shutdown()
  }
}
