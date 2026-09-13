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

  // Interface presence never proves that a consent request was granted.
  assert_eq!(shared.screen_recording, PermissionStatus::Unknown);
  assert_eq!(shared.automation_to_system_events, PermissionStatus::Missing);
}

#[test]
fn portal_identity_rejects_paths_and_empty_ids() {
  assert!(validate_app_id("ai.moeru.auv").is_ok());
  for id in [
    "",
    "../ai.moeru.auv",
    "ai..auv",
    "ai.moeru/auv",
    "ai moeru.auv",
  ] {
    assert!(validate_app_id(id).is_err(), "{id}");
  }
}
