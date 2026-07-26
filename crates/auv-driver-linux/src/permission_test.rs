use super::*;

#[test]
fn portal_probe_maps_to_shared_permission_probe() {
  let probe = LinuxPortalProbe {
    screencast: PortalInterfaceProbe {
      available: PermissionStatus::Granted,
      version: Some(6),
      details: None,
    },
    remote_desktop: PortalInterfaceProbe {
      available: PermissionStatus::Missing,
      version: None,
      details: None,
    },
    ..LinuxPortalProbe::default()
  };

  let shared = probe.as_permission_probe();

  assert_eq!(shared.screen_recording, PermissionStatus::Granted);
  assert_eq!(shared.automation_to_system_events, PermissionStatus::Missing);
}
