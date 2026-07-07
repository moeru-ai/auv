use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionStatus {
  Granted,
  Missing,
  #[default]
  Unknown,
}

impl PermissionStatus {
  pub const fn as_str(self) -> &'static str {
    match self {
      Self::Granted => "granted",
      Self::Missing => "missing",
      Self::Unknown => "unknown",
    }
  }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionProbe {
  pub screen_recording: PermissionStatus,
  pub screen_capture_kit: PermissionStatus,
  pub accessibility: PermissionStatus,
  pub automation_to_system_events: PermissionStatus,
}

#[cfg(test)]
mod tests {
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
}
