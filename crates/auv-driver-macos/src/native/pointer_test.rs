#[cfg(target_os = "macos")]
use super::action_result;
#[cfg(target_os = "macos")]
use crate::native::binding::ffi::NativeActionResponse;

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
