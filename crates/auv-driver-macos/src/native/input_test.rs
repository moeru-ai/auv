#[cfg(target_os = "macos")]
use super::action_result;
#[cfg(target_os = "macos")]
use crate::native::binding::ffi::NativeActionResponse;

#[cfg(target_os = "macos")]
#[test]
fn action_result_includes_operation_name() {
  let error = action_result(
    "type_text_in_window",
    NativeActionResponse {
      ok: false,
      error_message: Some("failed to create keyboard event".to_string()),
      recovery_hint: Some("grant Accessibility permission".to_string()),
    },
  )
  .unwrap_err();

  assert!(error.contains("type_text_in_window"));
  assert!(error.contains("failed to create keyboard event"));
}
