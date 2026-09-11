use super::*;

#[test]
fn pairing_command_debug_redacts_bootstrap_token() {
  let command = PairingCommand::Connect {
    token: "bootstrap-secret".to_string(),
    device_id: None,
    label: "test".to_string(),
    profile: None,
    json: false,
  };
  let debug = format!("{command:?}");
  assert!(!debug.contains("bootstrap-secret"));
  assert!(debug.contains("[REDACTED]"));
}
