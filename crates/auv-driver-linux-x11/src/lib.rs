//! Legacy X11 compatibility for existing benchmark desktops.
//!
//! Continued use is not recommended: prefer `auv-driver-linux` on Wayland.
//! This crate implements the shared driver contracts. `auv-driver::LocalDriver`
//! selects it only for a pure X11 process environment. It never starts an X
//! server or changes process environment variables. See the crate README for
//! setup and evidence limits.

use auv_driver_common::{Driver, DriverDescriptor, DriverResult, DriverSession, PlatformKind};

#[cfg(target_os = "linux")]
mod capture;
#[cfg(target_os = "linux")]
mod input;
#[cfg(target_os = "linux")]
mod session;
#[cfg(target_os = "linux")]
pub use capture::DisplayApi;
#[cfg(target_os = "linux")]
pub use input::InputApi;
#[cfg(target_os = "linux")]
pub use session::X11DriverSession;

/// Explicit opt-in to a legacy X11 session using `DISPLAY` and Xauthority.
///
/// Use one display per process. New deployments should use the Wayland driver.
#[derive(Clone, Debug, Default)]
pub struct X11Driver;

/// Unavailable session type on non-Linux targets; opening always fails.
#[cfg(not(target_os = "linux"))]
#[derive(Clone, Debug)]
pub struct X11DriverSession;

impl Driver for X11Driver {
  type Session = X11DriverSession;

  fn descriptor(&self) -> DriverDescriptor {
    descriptor()
  }

  fn open_local(&self) -> DriverResult<Self::Session> {
    #[cfg(target_os = "linux")]
    {
      session::open()
    }
    #[cfg(not(target_os = "linux"))]
    {
      Err(auv_driver_common::DriverError::unsupported("linux.x11.open_local"))
    }
  }
}

impl DriverSession for X11DriverSession {
  fn descriptor(&self) -> DriverDescriptor {
    descriptor()
  }
}

fn descriptor() -> DriverDescriptor {
  DriverDescriptor {
    id: "linux.x11",
    platform: PlatformKind::Linux,
    description: "Legacy X11 display capture and foreground XTEST input; not recommended for new deployments.",
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn descriptor_identifies_explicit_legacy_backend() {
    assert_eq!(X11Driver.descriptor().id, "linux.x11");
    assert_eq!(X11Driver.descriptor().platform, PlatformKind::Linux);
  }

  #[cfg(not(target_os = "linux"))]
  #[test]
  fn opening_on_other_platforms_does_not_control_the_host() {
    assert!(matches!(X11Driver.open_local(), Err(auv_driver_common::DriverError::Unsupported { .. })));
  }
}
