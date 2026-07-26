use auv_driver_common::permission::PermissionStatus;

use super::*;

#[test]
fn default_probe_is_all_unknown() {
  let probe = WindowsPermissionProbe::default();
  assert_eq!(probe.elevated, PermissionStatus::Unknown);
  assert_eq!(probe.ui_access, PermissionStatus::Unknown);
  assert_eq!(probe.interactive_session, PermissionStatus::Unknown);
}

// Live smoke test: the token and session queries must succeed (resolve to a
// concrete Granted/Missing) when run as a normal interactive process, proving
// the FFI calls are wired correctly. The environment (admin vs not) is not
// asserted, only that each signal was determinable.
#[cfg(target_os = "windows")]
#[test]
fn probe_resolves_signals_on_windows() {
  let probe = probe();
  assert_ne!(probe.elevated, PermissionStatus::Unknown);
  assert_ne!(probe.ui_access, PermissionStatus::Unknown);
  assert_ne!(probe.interactive_session, PermissionStatus::Unknown);
}
