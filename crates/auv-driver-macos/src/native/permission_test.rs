use super::*;

#[test]
fn permission_status_label_maps_granted() {
  assert_eq!(permission_status_label(NativePermissionStatus::Granted), "granted");
}

#[test]
fn permission_status_label_maps_missing() {
  assert_eq!(permission_status_label(NativePermissionStatus::Missing), "missing");
}
