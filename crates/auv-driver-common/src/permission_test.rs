use super::*;

#[test]
fn permission_status_serializes_as_snake_case() {
  assert_eq!(serde_json::to_value(PermissionStatus::Granted).expect("serialize"), serde_json::json!("granted"));
}

#[test]
fn permission_probe_round_trips() {
  let probe = PermissionProbe {
    screen_recording: PermissionStatus::Granted,
    screen_capture_kit: PermissionStatus::Granted,
    accessibility: PermissionStatus::Missing,
    automation_to_system_events: PermissionStatus::Unknown,
  };

  let encoded = serde_json::to_value(&probe).expect("serialize");
  assert_eq!(
    encoded,
    serde_json::json!({
      "screen_recording": "granted",
      "screen_capture_kit": "granted",
      "accessibility": "missing",
      "automation_to_system_events": "unknown",
    })
  );
  let decoded: PermissionProbe = serde_json::from_value(encoded).expect("deserialize");
  assert_eq!(decoded, probe);
}
