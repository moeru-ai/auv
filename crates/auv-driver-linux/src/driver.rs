use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use auv_driver_common::{Driver, DriverDescriptor, DriverResult, DriverSession};

use crate::descriptor::{LinuxDriverDescriptor, linux_driver_descriptor};
use crate::native::portal::{ClipboardSession, InputSession, RestoreTokenStore, ScreenCastSession};

#[derive(Clone, Debug, Default)]
pub struct LinuxDriver {
  portal_state_root: Option<PathBuf>,
  portal_app_id: Option<String>,
}

impl LinuxDriver {
  pub fn new() -> Self {
    Self::default()
  }

  /// Persists opaque portal restore tokens below a daemon-owned state root.
  pub fn with_portal_state_root(mut self, root: PathBuf) -> Self {
    self.portal_state_root = Some(root);
    self
  }

  /// Associates every Portal D-Bus connection with the caller's desktop ID.
  /// The caller must install a matching desktop entry before opening a Portal.
  pub fn with_portal_app_id(mut self, app_id: String) -> auv_driver_common::DriverResult<Self> {
    crate::permission::validate_app_id(&app_id)?;
    self.portal_app_id = Some(app_id);
    Ok(self)
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
  // TODO(linux-portal-clipboard-restore): persistent clipboard authorization
  // is deferred with that shared-session slice because GNOME currently hangs
  // when the standalone clipboard session calls SelectDevices with no devices.
  pub(crate) clipboard_session: Option<ClipboardSession>,
  pub(crate) input_session: Option<InputSession>,
  pub(crate) screencast_session: Option<ScreenCastSession>,
  pub(crate) restore_tokens: Option<RestoreTokenStore>,
  pub(crate) portal_app_id: Option<String>,
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
      state: Arc::new(Mutex::new(LinuxDriverSessionState {
        restore_tokens: self.portal_state_root.clone().map(RestoreTokenStore::new),
        portal_app_id: self.portal_app_id.clone(),
        ..Default::default()
      })),
    })
  }
}

impl DriverSession for LinuxDriverSession {
  fn descriptor(&self) -> DriverDescriptor {
    self.linux_descriptor().as_driver_descriptor()
  }
}

#[cfg(test)]
#[path = "driver_test.rs"]
mod tests;
