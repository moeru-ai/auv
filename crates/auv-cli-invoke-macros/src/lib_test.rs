use super::{namespace_for_group_literal, validate_attr_key};

#[test]
fn namespace_for_group_literal_accepts_supported_groups() {
  assert_eq!(namespace_for_group_literal("\"screen\""), Ok("Screen"));
  assert_eq!(namespace_for_group_literal("\"mediaControl\""), Ok("MediaControl"));
  assert_eq!(namespace_for_group_literal("\"scan\""), Ok("Scan"));
  assert_eq!(namespace_for_group_literal("\"game\""), Ok("Game"));
}

#[test]
fn namespace_for_group_literal_rejects_unknown_groups() {
  let error = namespace_for_group_literal("\"media_control\"").expect_err("unknown groups should fail during macro expansion");

  assert!(error.contains("invoke_command unknown group"));
  assert!(error.contains("mediaControl"));
}

#[test]
fn validate_attr_key_rejects_execution_metadata_keys() {
  for key in [
    "driver",
    "operation",
    "disturbance",
    "max_disturbance",
    "artifacts",
    "signals",
    "verification",
    "operation_namespace",
  ] {
    let error = validate_attr_key(key).expect_err("old execution metadata should be rejected");

    assert!(error.contains("invoke_command unknown attribute"));
    assert!(error.contains(key));
    assert!(error.contains("id, group, description, args"));
  }
}
