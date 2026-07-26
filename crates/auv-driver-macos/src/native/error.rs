use super::types::AuvResult;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeDriverError {
  pub operation: String,
  pub message: String,
  pub recovery_hint: String,
}

pub fn native_error_to_auv(error: NativeDriverError) -> String {
  format!("macos native {} failed: {}; recovery={}", error.operation, error.message, error.recovery_hint)
}

pub fn native_result<T>(operation: &str, value: Option<T>, error_message: Option<String>, recovery_hint: Option<String>) -> AuvResult<T> {
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
#[path = "error_test.rs"]
mod tests;
