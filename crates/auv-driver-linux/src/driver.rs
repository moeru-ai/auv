use std::sync::{Arc, Mutex};

use auv_driver_common::{Driver, DriverDescriptor, DriverResult, DriverSession};

use crate::descriptor::{LinuxDriverDescriptor, linux_driver_descriptor};
use crate::native::portal::{ClipboardSession, InputSession, ScreenCastSession};

#[derive(Clone, Copy, Debug, Default)]
pub struct LinuxDriver;

impl LinuxDriver {
  pub fn new() -> Self {
    Self
  }

  pub fn linux_descriptor(&self) -> LinuxDriverDescriptor {
    linux_driver_descriptor()
  }
}

#[derive(Clone, Debug)]
pub struct LinuxDriverSession {
  pub(crate) state: Arc<Mutex<LinuxDriverSessionState>>,
}

#[derive(Debug, Default)]
pub(crate) struct LinuxDriverSessionState {
  // TODO(linux-portal-remote-desktop-shared-session): input and clipboard use
  // separate RemoteDesktop sessions, so live validation still requests those
  // permissions separately. Merge only after an owner-approved slice defines
  // combined RequestClipboard/SelectDevices/Start and clipboard transfer
  // thread ownership.
  pub(crate) clipboard_session: Option<ClipboardSession>,
  pub(crate) input_session: Option<InputSession>,
  pub(crate) screencast_session: Option<ScreenCastSession>,
}

impl LinuxDriverSession {
  pub fn linux_descriptor(&self) -> LinuxDriverDescriptor {
    linux_driver_descriptor()
  }
}

impl Driver for LinuxDriver {
  type Session = LinuxDriverSession;

  fn descriptor(&self) -> DriverDescriptor {
    self.linux_descriptor().as_driver_descriptor()
  }

  fn open_local(&self) -> DriverResult<Self::Session> {
    Ok(LinuxDriverSession {
      state: Arc::new(Mutex::new(LinuxDriverSessionState::default())),
    })
  }
}

impl DriverSession for LinuxDriverSession {
  fn descriptor(&self) -> DriverDescriptor {
    self.linux_descriptor().as_driver_descriptor()
  }
}

#[cfg(test)]
mod tests {
  use auv_driver_common::{Driver, DriverSession, PlatformKind};

  use crate::LinuxDriver;

  #[test]
  fn descriptor_uses_desktop_namespace() {
    let descriptor = LinuxDriver::new().linux_descriptor();

    assert_eq!(descriptor.id, "linux.desktop");
    assert_eq!(descriptor.platform, PlatformKind::Linux);
  }

  #[test]
  fn session_exposes_driver_descriptor() {
    let driver = LinuxDriver::new();
    let session = driver.open_local().expect("session should open");

    assert_eq!(session.linux_descriptor(), driver.linux_descriptor());
    assert_eq!(DriverSession::descriptor(&session), driver.linux_descriptor().as_driver_descriptor());
  }
}
