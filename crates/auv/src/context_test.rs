use super::AuvContext;

#[test]
fn plugin_context_is_inline_additive_json_without_secrets_or_version() {
  let decoded: AuvContext =
    serde_json::from_str(r#"{"device_id":"device_01H","run_id":"run_01H","daemon_endpoint":"unix:///tmp/auv.sock","future_field":true}"#)
      .expect("decode additive context");
  assert_eq!(decoded.device_id.as_deref(), Some("device_01H"));
  assert_eq!(decoded.run_id.as_deref(), Some("run_01H"));

  let encoded = serde_json::to_value(decoded).expect("encode context");
  assert_eq!(encoded["daemon_endpoint"], "unix:///tmp/auv.sock");
  assert!(encoded.get("version").is_none());
  assert!(encoded.get("token").is_none());
  assert!(encoded.get("credential").is_none());
}
