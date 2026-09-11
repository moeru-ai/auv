use auv_driver_common::permission::{PermissionProbe, PermissionStatus};

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
      screen_recording: if self.screencast.available == PermissionStatus::Missing {
        PermissionStatus::Missing
      } else {
        PermissionStatus::Unknown
      },
      screen_capture_kit: PermissionStatus::Unknown,
      accessibility: PermissionStatus::Unknown,
      automation_to_system_events: if self.remote_desktop.available == PermissionStatus::Missing {
        PermissionStatus::Missing
      } else {
        PermissionStatus::Unknown
      },
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
  match zbus::blocking::connection::Builder::session().and_then(|builder| builder.method_timeout(std::time::Duration::from_secs(3)).build())
  {
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
  match proxy.get_property::<u32>("version") {
    Ok(version) => PortalInterfaceProbe {
      available: PermissionStatus::Granted,
      version: Some(version),
      details: Some("interface available; user authorization has not been tested".into()),
    },
    Err(error) => {
      // GLib reports a missing interface as InvalidArgs for this fixed, valid
      // Properties.Get request; retain its details instead of hiding the error.
      let missing = matches!(
        zbus::fdo::Error::from(error.clone()),
        zbus::fdo::Error::InvalidArgs(_)
          | zbus::fdo::Error::UnknownInterface(_)
          | zbus::fdo::Error::UnknownMethod(_)
          | zbus::fdo::Error::UnknownProperty(_)
      );
      PortalInterfaceProbe {
        available: if missing {
          PermissionStatus::Missing
        } else {
          PermissionStatus::Unknown
        },
        version: None,
        details: Some(error.to_string()),
      }
    }
  }
}

#[cfg(test)]
#[path = "permission_test.rs"]
mod tests;

/// Portal IDs are desktop-entry basenames, never paths or shell expressions.
pub(crate) fn validate_app_id(app_id: &str) -> auv_driver_common::DriverResult<()> {
  if app_id.split('.').count() < 3
    || app_id.split('.').any(|part| part.is_empty() || !part.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-'))
  {
    return Err(crate::error::invalid_input("Portal app ID must be a reverse-domain desktop ID with at least three nonempty components"));
  }
  Ok(())
}

/// Verifies the configured identity using the same connection setup as input.
#[cfg(target_os = "linux")]
pub fn verify_portal_identity(app_id: &str) -> auv_driver_common::DriverResult<()> {
  let _connection = crate::native::portal::session_connection(Some(app_id))?;
  Ok(())
}

/// Reads the KDE-specific per-application allow rule, not a restore token.
#[cfg(target_os = "linux")]
pub fn kde_authorization(app_id: &str) -> auv_driver_common::DriverResult<PermissionStatus> {
  validate_app_id(app_id)?;
  let connection = zbus::blocking::connection::Builder::session()
    .and_then(|builder| builder.method_timeout(std::time::Duration::from_secs(3)).build())
    .map_err(|error| crate::error::backend(error.to_string()))?;
  let proxy = zbus::blocking::Proxy::new(
    &connection,
    "org.freedesktop.impl.portal.PermissionStore",
    "/org/freedesktop/impl/portal/PermissionStore",
    "org.freedesktop.impl.portal.PermissionStore",
  )
  .map_err(|error| crate::error::backend(error.to_string()))?;
  let result = proxy.call::<_, _, (std::collections::HashMap<String, Vec<String>>, zbus::zvariant::OwnedValue)>(
    "Lookup",
    &("kde-authorized", "remote-desktop"),
  );
  match result {
    Ok((permissions, _)) => Ok(if permissions.get(app_id).is_some_and(|values| values.iter().any(|value| value == "yes")) {
      PermissionStatus::Granted
    } else {
      PermissionStatus::Missing
    }),
    Err(zbus::Error::MethodError(name, _, _)) if name.as_str() == "org.freedesktop.portal.Error.NotFound" => Ok(PermissionStatus::Missing),
    Err(error) => Err(crate::error::backend(format!("failed to read KDE application authorization: {error}"))),
  }
}

/// Updates only this application's KDE rule. Call only from explicit setup or
/// revocation; ordinary driver operations never change permission-store rules.
#[cfg(target_os = "linux")]
pub fn set_kde_authorization(app_id: &str, allow: bool) -> auv_driver_common::DriverResult<()> {
  validate_app_id(app_id)?;
  let connection = zbus::blocking::connection::Builder::session()
    .and_then(|builder| builder.method_timeout(std::time::Duration::from_secs(3)).build())
    .map_err(|error| crate::error::backend(error.to_string()))?;
  let proxy = zbus::blocking::Proxy::new(
    &connection,
    "org.freedesktop.impl.portal.PermissionStore",
    "/org/freedesktop/impl/portal/PermissionStore",
    "org.freedesktop.impl.portal.PermissionStore",
  )
  .map_err(|error| crate::error::backend(error.to_string()))?;
  let permissions: Vec<&str> = if allow { vec!["yes"] } else { vec![] };
  proxy
    .call::<_, _, ()>("SetPermission", &("kde-authorized", true, "remote-desktop", app_id, permissions))
    .map_err(|error| crate::error::backend(format!("failed to update KDE application authorization: {error}")))
}
