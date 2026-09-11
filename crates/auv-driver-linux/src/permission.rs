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
  use ashpd::desktop::{remote_desktop::RemoteDesktop, screencast::Screencast, screenshot::ScreenshotProxy};
  match crate::native::portal::session_connection(None) {
    Ok(connection) => {
      // NOTICE: ashpd 0.13 defaults version() to 1 for some Properties.Get
      // failures. Read the property through its typed proxy so a timeout cannot
      // become an availability claim (ashpd 0.13.13 `src/proxy.rs`). Remove
      // when ashpd preserves that error.
      let screencast = probe_interface(async {
        let proxy = Screencast::with_connection(connection.clone()).await?;
        Ok(proxy.get_property::<u32>("version").await?)
      });
      let remote_desktop = probe_interface(async {
        let proxy = RemoteDesktop::with_connection(connection.clone()).await?;
        Ok(proxy.get_property::<u32>("version").await?)
      });
      let screenshot = probe_interface(async {
        let proxy = ScreenshotProxy::with_connection(connection).await?;
        Ok(proxy.get_property::<u32>("version").await?)
      });
      (PermissionStatus::Granted, screencast, remote_desktop, screenshot)
    }
    Err(error) => {
      let missing = PortalInterfaceProbe {
        available: PermissionStatus::Unknown,
        version: None,
        details: Some(error.to_string()),
      };
      (PermissionStatus::Unknown, missing.clone(), missing.clone(), missing)
    }
  }
}

#[cfg(target_os = "linux")]
fn probe_interface(probe: impl std::future::Future<Output = ashpd::Result<u32>>) -> PortalInterfaceProbe {
  // Keep ashpd's typed error until availability is classified; a Portal request
  // timeout or bus failure does not establish that an interface is absent.
  match crate::native::portal::run("probe Portal interface", async { Ok(probe.await) }) {
    Ok(Ok(version)) => PortalInterfaceProbe {
      available: PermissionStatus::Granted,
      version: Some(version),
      details: Some("interface available; user authorization has not been tested".into()),
    },
    Ok(Err(error)) => PortalInterfaceProbe {
      available: if matches!(error, ashpd::Error::PortalNotFound(_)) {
        PermissionStatus::Missing
      } else {
        PermissionStatus::Unknown
      },
      version: None,
      details: Some(error.to_string()),
    },
    Err(error) => PortalInterfaceProbe {
      available: PermissionStatus::Unknown,
      version: None,
      details: Some(error.to_string()),
    },
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

// NOTICE: ashpd 0.13 has no PermissionStore client. This KDE-specific table
// remains behind a typed zbus proxy until ashpd exposes that client interface.
#[cfg(target_os = "linux")]
#[zbus::proxy(
  interface = "org.freedesktop.impl.portal.PermissionStore",
  default_service = "org.freedesktop.impl.portal.PermissionStore",
  default_path = "/org/freedesktop/impl/portal/PermissionStore",
  gen_async = false
)]
trait PermissionStore {
  fn lookup(&self, table: &str, id: &str) -> zbus::Result<(std::collections::HashMap<String, Vec<String>>, zbus::zvariant::OwnedValue)>;
  fn set_permission(&self, table: &str, create: bool, id: &str, app: &str, permissions: &[&str]) -> zbus::Result<()>;
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
  let proxy = PermissionStoreProxy::new(&connection).map_err(|error| crate::error::backend(error.to_string()))?;
  let result = proxy.lookup("kde-authorized", "remote-desktop");
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
  let proxy = PermissionStoreProxy::new(&connection).map_err(|error| crate::error::backend(error.to_string()))?;
  let permissions: Vec<&str> = if allow { vec!["yes"] } else { vec![] };
  proxy
    .set_permission("kde-authorized", true, "remote-desktop", app_id, &permissions)
    .map_err(|error| crate::error::backend(format!("failed to update KDE application authorization: {error}")))
}
