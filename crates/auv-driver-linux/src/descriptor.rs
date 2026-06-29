use auv_driver::{DriverDescriptor, PlatformKind};

/// Capabilities exposed by the first Linux desktop driver slice.
///
/// The list deliberately excludes RemoteDesktop input until this crate owns a
/// complete portal/libei session lifecycle and can produce trustworthy
/// `InputActionResult` evidence.
pub const LINUX_DESKTOP_CAPABILITIES: &[&str] = &[
  "desktop.list-displays",
  "desktop.capture-display",
  "desktop.capture-region",
  "desktop.list-windows",
  "desktop.capture-window",
  "desktop.probe-permissions",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinuxDriverDescriptor {
  pub id: &'static str,
  pub platform: PlatformKind,
  pub summary: &'static str,
}

impl LinuxDriverDescriptor {
  pub fn as_driver_descriptor(&self) -> DriverDescriptor {
    DriverDescriptor {
      id: self.id,
      platform: self.platform,
      summary: self.summary,
    }
  }
}

pub fn linux_driver_descriptor() -> LinuxDriverDescriptor {
  LinuxDriverDescriptor {
    id: "linux.desktop",
    platform: PlatformKind::Linux,
    summary: "Linux desktop driver: display/window capture and Wayland portal readiness probes.",
  }
}
