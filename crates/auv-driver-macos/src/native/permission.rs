// File: src/driver/macos/native/permission.rs
#[cfg(target_os = "macos")]
use super::binding::ffi::{NativePermissionProbeResponse, NativePermissionStatus, probe_permissions};
use super::types::AuvResult;

#[cfg(target_os = "macos")]
pub fn probe_native_permissions() -> AuvResult<NativePermissionProbe> {
  Ok(NativePermissionProbe::from(probe_permissions()))
}

#[cfg(not(target_os = "macos"))]
pub fn probe_native_permissions() -> AuvResult<NativePermissionProbe> {
  Err("macOS native permission probe is unsupported on this target".to_string())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativePermissionProbe {
  pub screen_recording: &'static str,
  pub screen_capture_kit: &'static str,
  pub accessibility: &'static str,
}

#[cfg(target_os = "macos")]
impl From<NativePermissionProbeResponse> for NativePermissionProbe {
  fn from(value: NativePermissionProbeResponse) -> Self {
    Self {
      screen_recording: permission_status_label(value.screen_recording),
      screen_capture_kit: permission_status_label(value.screen_capture_kit),
      accessibility: permission_status_label(value.accessibility),
    }
  }
}

#[cfg(target_os = "macos")]
fn permission_status_label(status: NativePermissionStatus) -> &'static str {
  match status {
    NativePermissionStatus::Granted => "granted",
    NativePermissionStatus::Missing => "missing",
  }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
  use super::*;

  #[test]
  fn permission_status_label_maps_granted() {
    assert_eq!(permission_status_label(NativePermissionStatus::Granted), "granted");
  }

  #[test]
  fn permission_status_label_maps_missing() {
    assert_eq!(permission_status_label(NativePermissionStatus::Missing), "missing");
  }
}
