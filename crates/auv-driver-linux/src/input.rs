use std::time::Duration;

use auv_driver::error::{DriverError, DriverResult};
use auv_driver::geometry::Point;
use auv_driver::input::{
  Click, DisturbanceLevel, InputActionResult, InputAttempt, InputDeliveryPath, KeyPressOptions,
  Scroll, TypeTextOptions,
};

/// NOTICE(linux-remote-desktop-input): Wayland input emulation is reserved for
/// the portal/libei session slice. Returning typed unsupported results keeps
/// the current driver honest while validate can still prove portal readiness.
pub fn unsupported_result(operation: &'static str) -> DriverResult<InputActionResult> {
  Err(DriverError::Unsupported { operation })
}

pub fn click_at(_point: Point, _click: Click) -> DriverResult<InputActionResult> {
  unsupported_result("input.click_at")
}

pub fn scroll_at(
  _point: Point,
  _scroll: Scroll,
  _settle: Duration,
) -> DriverResult<InputActionResult> {
  unsupported_result("input.scroll_at")
}

pub fn type_text(_text: &str, _options: TypeTextOptions) -> DriverResult<InputActionResult> {
  unsupported_result("input.type_text")
}

pub fn press_key(_options: KeyPressOptions) -> DriverResult<InputActionResult> {
  unsupported_result("input.press_key")
}

pub fn reserved_input_result(reason: impl Into<String>) -> InputActionResult {
  let reason = reason.into();
  InputActionResult {
    selected_path: InputDeliveryPath::Unsupported,
    attempts: vec![InputAttempt::failure(
      InputDeliveryPath::Unsupported,
      reason.clone(),
    )],
    fallback_reason: Some(reason),
    mouse_disturbance: DisturbanceLevel::None,
    focus_disturbance: DisturbanceLevel::None,
    clipboard_disturbance: DisturbanceLevel::None,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn reserved_result_uses_shared_input_schema() {
    let result = reserved_input_result("not wired yet");

    assert_eq!(result.selected_path, InputDeliveryPath::Unsupported);
    assert_eq!(result.attempts.len(), 1);
  }
}
