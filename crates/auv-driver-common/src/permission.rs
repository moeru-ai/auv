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
#[path = "permission_test.rs"]
mod tests;
