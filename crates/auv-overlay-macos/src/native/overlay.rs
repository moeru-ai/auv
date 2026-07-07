#[cfg(target_os = "macos")]
use std::cell::RefCell;

#[cfg(target_os = "macos")]
use super::binding::ffi::{NativeActionResponse, NativeOverlayController, make_overlay_controller};
use crate::AuvResult;

#[cfg(target_os = "macos")]
thread_local! {
  static OVERLAY_CONTROLLER: RefCell<Option<NativeOverlayController>> = const { RefCell::new(None) };
}

#[cfg(target_os = "macos")]
pub(crate) fn show_cursor(x: f64, y: f64, label: &str) -> AuvResult<()> {
  with_controller("show_overlay_cursor", |controller| controller.show_overlay_cursor(x, y, label.to_string()))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn show_cursor(_x: f64, _y: f64, _label: &str) -> AuvResult<()> {
  Err("macOS native overlay is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
pub(crate) fn show_dual_cursor(x: f64, y: f64, label: &str, user_label: &str) -> AuvResult<()> {
  with_controller("show_overlay_dual_cursor", |controller| {
    controller.show_overlay_dual_cursor(x, y, label.to_string(), user_label.to_string())
  })
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn show_dual_cursor(_x: f64, _y: f64, _label: &str, _user_label: &str) -> AuvResult<()> {
  Err("macOS native overlay is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
pub(crate) fn set_cursor(cursor_id: &str, x: f64, y: f64, label: &str, variant: &str) -> AuvResult<()> {
  with_controller("set_overlay_cursor", |controller| {
    controller.set_overlay_cursor(cursor_id.to_string(), x, y, label.to_string(), variant.to_string())
  })
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn set_cursor(_cursor_id: &str, _x: f64, _y: f64, _label: &str, _variant: &str) -> AuvResult<()> {
  Err("macOS native overlay is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
pub(crate) fn move_cursor(cursor_id: &str, x: f64, y: f64, label: &str, variant: &str, duration_ms: u64) -> AuvResult<()> {
  with_controller("move_overlay_cursor", |controller| {
    controller.move_overlay_cursor(cursor_id.to_string(), x, y, label.to_string(), variant.to_string(), duration_ms)
  })
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn move_cursor(_cursor_id: &str, _x: f64, _y: f64, _label: &str, _variant: &str, _duration_ms: u64) -> AuvResult<()> {
  Err("macOS native overlay is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
pub(crate) fn move_dual_cursor(x: f64, y: f64, label: &str, user_label: &str, duration_ms: u64) -> AuvResult<()> {
  with_controller("move_overlay_dual_cursor", |controller| {
    controller.move_overlay_dual_cursor(x, y, label.to_string(), user_label.to_string(), duration_ms)
  })
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn move_dual_cursor(_x: f64, _y: f64, _label: &str, _user_label: &str, _duration_ms: u64) -> AuvResult<()> {
  Err("macOS native overlay is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
pub(crate) fn flash_cursor(x: f64, y: f64, label: &str, duration_ms: u64) -> AuvResult<()> {
  with_controller("flash_overlay_cursor", |controller| controller.flash_overlay_cursor(x, y, label.to_string(), duration_ms))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn flash_cursor(_x: f64, _y: f64, _label: &str, _duration_ms: u64) -> AuvResult<()> {
  Err("macOS native overlay is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
pub(crate) fn flash_cursor_id(cursor_id: &str, x: f64, y: f64, label: &str, duration_ms: u64) -> AuvResult<()> {
  with_controller("flash_overlay_cursor_id", |controller| {
    controller.flash_overlay_cursor_id(cursor_id.to_string(), x, y, label.to_string(), duration_ms)
  })
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn flash_cursor_id(_cursor_id: &str, _x: f64, _y: f64, _label: &str, _duration_ms: u64) -> AuvResult<()> {
  Err("macOS native overlay is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
pub(crate) fn hide_cursor_id(cursor_id: &str) -> AuvResult<()> {
  with_controller("hide_overlay_cursor_id", |controller| controller.hide_overlay_cursor_id(cursor_id.to_string()))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn hide_cursor_id(_cursor_id: &str) -> AuvResult<()> {
  Err("macOS native overlay is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
pub(crate) fn hide_cursor() -> AuvResult<()> {
  with_controller("hide_overlay_cursor", |controller| controller.hide_overlay_cursor())
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn hide_cursor() -> AuvResult<()> {
  Err("macOS native overlay is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
pub(crate) fn pump_events(duration_ms: u64) -> AuvResult<()> {
  action_result("pump_overlay_events", super::binding::ffi::pump_overlay_events(duration_ms))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn pump_events(_duration_ms: u64) -> AuvResult<()> {
  Err("macOS native overlay is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
pub(crate) fn shutdown() -> AuvResult<()> {
  OVERLAY_CONTROLLER.with(|cell| {
    let mut controller = cell.borrow_mut();
    let Some(controller) = controller.take() else {
      return Ok(());
    };
    action_result("shutdown_overlay_cursor", controller.shutdown_overlay_cursor())
  })
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn shutdown() -> AuvResult<()> {
  Err("macOS native overlay is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
fn with_controller(operation: &str, action: impl FnOnce(&NativeOverlayController) -> NativeActionResponse) -> AuvResult<()> {
  OVERLAY_CONTROLLER.with(|cell| {
    if cell.borrow().is_none() {
      *cell.borrow_mut() = Some(make_overlay_controller());
    }
    let controller = cell.borrow();
    let controller = controller.as_ref().ok_or_else(|| "failed to initialize native overlay controller".to_string())?;
    action_result(operation, action(controller))
  })
}

#[cfg(target_os = "macos")]
fn action_result(operation: &str, response: NativeActionResponse) -> AuvResult<()> {
  crate::error::native_result(operation, response.ok.then_some(()), response.error_message, response.recovery_hint)
}
