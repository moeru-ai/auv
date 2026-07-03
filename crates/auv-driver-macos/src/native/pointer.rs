// File: src/driver/macos/native/pointer.rs
#[cfg(target_os = "macos")]
use super::binding::ffi::{
  NativeActionResponse, NativeMouseLocationResponse, NativeTeachClickResponse,
  click_point as native_click_point, current_mouse_location as native_current_mouse_location,
  move_point as native_move_point, scroll_point as native_scroll_point,
  teach_next_click as native_teach_next_click,
};
use super::types::AuvResult;

#[derive(Clone, Debug)]
pub struct TaughtClick {
  pub x: f64,
  pub y: f64,
  pub button_code: i32,
  pub captured_at_unix_ms: i64,
}

#[cfg(target_os = "macos")]
pub fn click_point(
  x: f64,
  y: f64,
  button_code: i32,
  click_count: i64,
  click_interval_ms: u64,
) -> AuvResult<()> {
  action_result(
    "click_point",
    native_click_point(x, y, button_code, click_count, click_interval_ms),
  )
}

#[cfg(not(target_os = "macos"))]
pub fn click_point(
  _x: f64,
  _y: f64,
  _button_code: i32,
  _click_count: i64,
  _click_interval_ms: u64,
) -> AuvResult<()> {
  Err("macOS native pointer click is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
pub fn move_point(x: f64, y: f64, button_code: i32) -> AuvResult<()> {
  action_result("move_point", native_move_point(x, y, button_code))
}

#[cfg(not(target_os = "macos"))]
pub fn move_point(_x: f64, _y: f64, _button_code: i32) -> AuvResult<()> {
  Err("macOS native pointer move is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
pub fn scroll_point(x: f64, y: f64, delta_x: f64, delta_y: f64) -> AuvResult<()> {
  action_result("scroll_point", native_scroll_point(x, y, delta_x, delta_y))
}

#[cfg(target_os = "macos")]
pub fn current_mouse_logical_point() -> AuvResult<(f64, f64)> {
  mouse_location_result("current_mouse_location", native_current_mouse_location())
}

#[cfg(target_os = "macos")]
pub fn teach_next_click(prompt: &str, timeout_ms: u64) -> AuvResult<TaughtClick> {
  teach_click_result(
    "teach_next_click",
    native_teach_next_click(prompt.to_string(), timeout_ms),
  )
}

#[cfg(not(target_os = "macos"))]
pub fn scroll_point(_x: f64, _y: f64, _delta_x: f64, _delta_y: f64) -> AuvResult<()> {
  Err("macOS native pointer scroll is unsupported on this target".to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn current_mouse_logical_point() -> AuvResult<(f64, f64)> {
  Err("macOS native mouse location is unsupported on this target".to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn teach_next_click(_prompt: &str, _timeout_ms: u64) -> AuvResult<TaughtClick> {
  Err("macOS native teach click capture is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
fn action_result(operation: &str, response: NativeActionResponse) -> AuvResult<()> {
  super::error::native_result(
    operation,
    response.ok.then_some(()),
    response.error_message,
    response.recovery_hint,
  )
}

#[cfg(target_os = "macos")]
fn mouse_location_result(
  operation: &str,
  response: NativeMouseLocationResponse,
) -> AuvResult<(f64, f64)> {
  super::error::native_result(
    operation,
    response
      .error_message
      .is_none()
      .then_some((response.x, response.y)),
    response.error_message,
    response.recovery_hint,
  )
}

#[cfg(target_os = "macos")]
fn teach_click_result(
  operation: &str,
  response: NativeTeachClickResponse,
) -> AuvResult<TaughtClick> {
  super::error::native_result(
    operation,
    response.error_message.is_none().then_some(TaughtClick {
      x: response.x,
      y: response.y,
      button_code: response.button_code,
      captured_at_unix_ms: response.captured_at_unix_ms,
    }),
    response.error_message,
    response.recovery_hint,
  )
}

#[cfg(test)]
mod tests {
  #[cfg(target_os = "macos")]
  use super::*;

  #[cfg(target_os = "macos")]
  #[test]
  fn action_result_includes_operation_name() {
    let error = action_result(
      "click_point",
      NativeActionResponse {
        ok: false,
        error_message: Some("event creation failed".to_string()),
        recovery_hint: Some("grant Accessibility permission".to_string()),
      },
    )
    .unwrap_err();

    assert!(error.contains("click_point"));
    assert!(error.contains("event creation failed"));
  }
}
