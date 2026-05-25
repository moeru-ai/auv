// File: src/driver/macos/native/error.rs
use crate::model::AuvResult;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NativeDriverError {
  pub(crate) operation: String,
  pub(crate) message: String,
  pub(crate) recovery_hint: String,
}

pub(crate) fn native_error_to_auv(error: NativeDriverError) -> String {
  format!(
    "macos native {} failed: {}; recovery={}",
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
    None => Err(native_error_to_auv(NativeDriverError {
      operation: operation.to_string(),
      message: error_message.unwrap_or_else(|| "unknown native error".to_string()),
      recovery_hint: recovery_hint.unwrap_or_else(|| "retry or run auv doctor".to_string()),
    })),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn native_result_returns_value_when_present() {
    let value = native_result("list_windows", Some(7), None, None).unwrap();
    assert_eq!(value, 7);
  }

  #[test]
  fn native_result_formats_operation_message_and_recovery_hint() {
    let error = native_result::<i32>(
      "list_windows",
      None,
      Some("screen recording denied".to_string()),
      Some("grant Screen Recording permission".to_string()),
    )
    .unwrap_err();

    assert_eq!(
      error,
      "macos native list_windows failed: screen recording denied; recovery=grant Screen Recording permission"
    );
  }
}
