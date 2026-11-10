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

  assert_eq!(error, "macos native list_windows failed: screen recording denied; recovery=grant Screen Recording permission");
}
