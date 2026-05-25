pub type AuvResult<T> = Result<T, String>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeOverlayError {
  pub operation: String,
  pub message: String,
  pub recovery_hint: String,
}

pub fn native_error_to_auv(error: NativeOverlayError) -> String {
  format!(
    "macos native overlay {} failed: {}; recovery={}",
    error.operation, error.message, error.recovery_hint
  )
}

pub(crate) fn native_result<T>(
  operation: &str,
  value: Option<T>,
  error_message: Option<String>,
  recovery_hint: Option<String>,
) -> AuvResult<T> {
  match value {
    Some(value) => Ok(value),
    None => Err(native_error_to_auv(NativeOverlayError {
      operation: operation.to_string(),
      message: error_message.unwrap_or_else(|| "unknown native overlay error".to_string()),
      recovery_hint: recovery_hint.unwrap_or_else(|| "retry or run auv doctor".to_string()),
    })),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn native_result_returns_value_when_present() {
    let value = native_result("show_overlay_cursor", Some(7), None, None).unwrap();
    assert_eq!(value, 7);
  }

  #[test]
  fn native_result_formats_operation_message_and_recovery_hint() {
    let error = native_result::<i32>(
      "show_overlay_cursor",
      None,
      Some("window creation failed".to_string()),
      Some("retry overlay visualization".to_string()),
    )
    .unwrap_err();

    assert_eq!(
      error,
      "macos native overlay show_overlay_cursor failed: window creation failed; recovery=retry overlay visualization"
    );
  }
}
