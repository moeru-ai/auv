use auv_driver::permission::{PermissionProbe, PermissionStatus};

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PortalInterfaceProbe {
  pub available: PermissionStatus,
  pub version: Option<u32>,
  pub details: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LinuxPortalProbe {
  pub wayland_session: PermissionStatus,
  pub session_type: Option<String>,
  pub desktop: Option<String>,
  pub portal_bus: PermissionStatus,
  pub screencast: PortalInterfaceProbe,
  pub remote_desktop: PortalInterfaceProbe,
  pub screenshot: PortalInterfaceProbe,
}

impl LinuxPortalProbe {
  pub fn as_permission_probe(&self) -> PermissionProbe {
    PermissionProbe {
      screen_recording: self.screencast.available,
      screen_capture_kit: PermissionStatus::Unknown,
      accessibility: PermissionStatus::Unknown,
      automation_to_system_events: self.remote_desktop.available,
    }
  }
}

pub fn probe_portals() -> LinuxPortalProbe {
  let session_type = std::env::var("XDG_SESSION_TYPE").ok();
  let desktop = std::env::var("XDG_CURRENT_DESKTOP").ok().or_else(|| std::env::var("DESKTOP_SESSION").ok());
  let wayland_session =
    if session_type.as_deref().is_some_and(|value| value.eq_ignore_ascii_case("wayland")) || std::env::var_os("WAYLAND_DISPLAY").is_some() {
      PermissionStatus::Granted
    } else {
      PermissionStatus::Missing
    };

  #[cfg(target_os = "linux")]
  let (portal_bus, screencast, remote_desktop, screenshot) = probe_portal_bus();
  #[cfg(not(target_os = "linux"))]
  let (portal_bus, screencast, remote_desktop, screenshot) = (
    PermissionStatus::Missing,
    PortalInterfaceProbe {
      available: PermissionStatus::Missing,
      version: None,
      details: Some("not running on Linux".to_string()),
    },
    PortalInterfaceProbe {
      available: PermissionStatus::Missing,
      version: None,
      details: Some("not running on Linux".to_string()),
    },
    PortalInterfaceProbe {
      available: PermissionStatus::Missing,
      version: None,
      details: Some("not running on Linux".to_string()),
    },
  );

  LinuxPortalProbe {
    wayland_session,
    session_type,
    desktop,
    portal_bus,
    screencast,
    remote_desktop,
    screenshot,
  }
}

#[cfg(target_os = "linux")]
fn probe_portal_bus() -> (PermissionStatus, PortalInterfaceProbe, PortalInterfaceProbe, PortalInterfaceProbe) {
  match zbus::blocking::Connection::session() {
    Ok(connection) => {
      let screencast = probe_interface(&connection, "org.freedesktop.portal.ScreenCast");
      let remote_desktop = probe_interface(&connection, "org.freedesktop.portal.RemoteDesktop");
      let screenshot = probe_interface(&connection, "org.freedesktop.portal.Screenshot");
      (PermissionStatus::Granted, screencast, remote_desktop, screenshot)
    }
    Err(error) => {
      let missing = PortalInterfaceProbe {
        available: PermissionStatus::Unknown,
        version: None,
        details: Some(format!("failed to connect to session bus: {error}")),
      };
      (PermissionStatus::Unknown, missing.clone(), missing.clone(), missing)
    }
  }
}

#[cfg(target_os = "linux")]
fn probe_interface(connection: &zbus::blocking::Connection, interface: &'static str) -> PortalInterfaceProbe {
  let proxy = match zbus::blocking::Proxy::new(connection, "org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop", interface) {
    Ok(proxy) => proxy,
    Err(error) => {
      return PortalInterfaceProbe {
        available: PermissionStatus::Missing,
        version: None,
        details: Some(format!("failed to create {interface} proxy: {error}")),
      };
    }
  };
  let version = proxy.get_property::<u32>("version").ok();
  let available = if version.is_some() {
    PermissionStatus::Granted
  } else {
    PermissionStatus::Unknown
  };
  PortalInterfaceProbe {
    available,
    version,
    details: None,
  }
}

#[cfg(test)]
mod tests {
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
}
