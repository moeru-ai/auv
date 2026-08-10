#[cfg(target_os = "macos")]
use super::binding::ffi::{
  NativeActionResponse, click_window_point as native_click_window_point, hotkey_foreground as native_hotkey_foreground,
  hotkey_in_window as native_hotkey_in_window, press_key_foreground as native_press_key_foreground,
  press_key_in_window as native_press_key_in_window, scroll_window_point as native_scroll_window_point,
  type_text_foreground as native_type_text_foreground, type_text_in_window as native_type_text_in_window,
};
use super::types::AuvResult;

#[cfg(target_os = "macos")]
pub fn click_window_point(
  pid: i64,
  window_number: i64,
  screen_x: f64,
  screen_y: f64,
  window_x: f64,
  window_y: f64,
  button_code: i32,
  click_count: i64,
  click_interval_ms: u64,
  window_strategy_code: i32,
) -> AuvResult<()> {
  action_result(
    "click_window_point",
    native_click_window_point(
      pid,
      window_number,
      screen_x,
      screen_y,
      window_x,
      window_y,
      button_code,
      click_count,
      click_interval_ms,
      window_strategy_code,
    ),
  )
}

#[cfg(not(target_os = "macos"))]
pub fn click_window_point(
  _pid: i64,
  _window_number: i64,
  _screen_x: f64,
  _screen_y: f64,
  _window_x: f64,
  _window_y: f64,
  _button_code: i32,
  _click_count: i64,
  _click_interval_ms: u64,
  _window_strategy_code: i32,
) -> AuvResult<()> {
  Err("macOS native window-targeted click is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
pub fn type_text_in_window(pid: i64, window_number: i64, text: String, inter_char_delay_ms: u64) -> AuvResult<()> {
  action_result("type_text_in_window", native_type_text_in_window(pid, window_number, text, inter_char_delay_ms))
}

#[cfg(not(target_os = "macos"))]
pub fn type_text_in_window(_pid: i64, _window_number: i64, _text: String, _inter_char_delay_ms: u64) -> AuvResult<()> {
  Err("macOS native window-targeted text input is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
pub fn type_text_foreground(text: String, inter_char_delay_ms: u64) -> AuvResult<()> {
  action_result("type_text_foreground", native_type_text_foreground(text, inter_char_delay_ms))
}

#[cfg(not(target_os = "macos"))]
pub fn type_text_foreground(_text: String, _inter_char_delay_ms: u64) -> AuvResult<()> {
  Err("macOS native foreground text input is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
pub fn scroll_window_point(
  pid: i64,
  window_number: i64,
  screen_x: f64,
  screen_y: f64,
  window_x: f64,
  window_y: f64,
  delta_x: f64,
  delta_y: f64,
) -> AuvResult<()> {
  action_result(
    "scroll_window_point",
    native_scroll_window_point(pid, window_number, screen_x, screen_y, window_x, window_y, delta_x, delta_y),
  )
}

#[cfg(not(target_os = "macos"))]
pub fn scroll_window_point(
  _pid: i64,
  _window_number: i64,
  _screen_x: f64,
  _screen_y: f64,
  _window_x: f64,
  _window_y: f64,
  _delta_x: f64,
  _delta_y: f64,
) -> AuvResult<()> {
  Err("macOS native window-targeted scroll is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
pub fn press_key_in_window(pid: i64, window_number: i64, key_code: i32) -> AuvResult<()> {
  action_result("press_key_in_window", native_press_key_in_window(pid, window_number, key_code))
}

#[cfg(not(target_os = "macos"))]
pub fn press_key_in_window(_pid: i64, _window_number: i64, _key_code: i32) -> AuvResult<()> {
  Err("macOS native window-targeted key press is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
pub fn press_key_foreground(key_code: i32) -> AuvResult<()> {
  action_result("press_key_foreground", native_press_key_foreground(key_code))
}

#[cfg(not(target_os = "macos"))]
pub fn press_key_foreground(_key_code: i32) -> AuvResult<()> {
  Err("macOS native foreground key press is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
pub fn hotkey_in_window(
  pid: i64,
  window_number: i64,
  key_code: i32,
  command: bool,
  shift: bool,
  option: bool,
  control: bool,
) -> AuvResult<()> {
  action_result("hotkey_in_window", native_hotkey_in_window(pid, window_number, key_code, command, shift, option, control))
}

#[cfg(not(target_os = "macos"))]
pub fn hotkey_in_window(
  _pid: i64,
  _window_number: i64,
  _key_code: i32,
  _command: bool,
  _shift: bool,
  _option: bool,
  _control: bool,
) -> AuvResult<()> {
  Err("macOS native window-targeted hotkey is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
pub fn hotkey_foreground(key_code: i32, command: bool, shift: bool, option: bool, control: bool) -> AuvResult<()> {
  action_result("hotkey_foreground", native_hotkey_foreground(key_code, command, shift, option, control))
}

#[cfg(not(target_os = "macos"))]
pub fn hotkey_foreground(_key_code: i32, _command: bool, _shift: bool, _option: bool, _control: bool) -> AuvResult<()> {
  Err("macOS native foreground hotkey is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
fn action_result(operation: &str, response: NativeActionResponse) -> AuvResult<()> {
  super::error::native_result(operation, response.ok.then_some(()), response.error_message, response.recovery_hint)
}

#[cfg(test)]
#[path = "input_test.rs"]
mod tests;
