use super::*;

#[test]
fn permission_status_label_maps_granted() {
  assert_eq!(permission_status_label(NativePermissionStatus::Granted), "granted");
}

#[test]
fn permission_status_label_maps_missing() {
  assert_eq!(permission_status_label(NativePermissionStatus::Missing), "missing");
}

// ROOT CAUSE:
//
// If the ScreenCaptureKit callback missed the probe deadline, `auv doctor`
// reported a missing permission because the native probe collapsed timeout
// and permission denial into the same boolean result.
//
// Before the fix, only granted and missing crossed the native boundary.
// The fix keeps operational probe failures distinct from permission state.
#[test]
fn permission_status_label_preserves_probe_failures() {
  assert_eq!(permission_status_label(NativePermissionStatus::TimedOut), "timed_out");
  assert_eq!(permission_status_label(NativePermissionStatus::Failed), "failed");
}
