pub use auv_driver_common::*;

#[cfg(feature = "overlay")]
pub use auv_driver_overlay as overlay;

use std::ops::{Deref, DerefMut};

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
use auv_driver_common::{DriverError, PlatformKind};

#[derive(Clone, Debug, Default)]
pub struct LocalDriver {
  #[cfg(target_os = "linux")]
  inner: auv_driver_linux::LinuxDriver,
  #[cfg(target_os = "macos")]
  inner: auv_driver_macos::MacosDriver,
  #[cfg(target_os = "windows")]
  inner: auv_driver_windows::WindowsDriver,
}

impl LocalDriver {
  pub fn new() -> Self {
    Self {
      #[cfg(target_os = "linux")]
      inner: auv_driver_linux::LinuxDriver::new(),
      #[cfg(target_os = "macos")]
      inner: auv_driver_macos::MacosDriver::new(),
      #[cfg(target_os = "windows")]
      inner: auv_driver_windows::WindowsDriver::new(),
    }
  }

  #[cfg(target_os = "linux")]
  pub fn with_linux_portal_state_root(mut self, root: std::path::PathBuf) -> Self {
    self.inner = self.inner.with_portal_state_root(root);
    self
  }
}

#[derive(Clone, Debug)]
pub enum LocalDriverSession {
  #[cfg(target_os = "linux")]
  Linux(auv_driver_linux::LinuxDriverSession),
  #[cfg(target_os = "macos")]
  Macos(auv_driver_macos::MacosDriverSession),
  #[cfg(target_os = "windows")]
  Windows(auv_driver_windows::WindowsDriverSession),
}

pub fn open_local() -> DriverResult<LocalDriverSession> {
  LocalDriver::new().open_local()
}

#[cfg(target_os = "linux")]
impl Deref for LocalDriverSession {
  type Target = auv_driver_linux::LinuxDriverSession;

  fn deref(&self) -> &Self::Target {
    match self {
      Self::Linux(session) => session,
    }
  }
}

#[cfg(target_os = "macos")]
impl Deref for LocalDriverSession {
  type Target = auv_driver_macos::MacosDriverSession;

  fn deref(&self) -> &Self::Target {
    match self {
      Self::Macos(session) => session,
    }
  }
}

#[cfg(target_os = "windows")]
impl Deref for LocalDriverSession {
  type Target = auv_driver_windows::WindowsDriverSession;

  fn deref(&self) -> &Self::Target {
    match self {
      Self::Windows(session) => session,
    }
  }
}

#[cfg(target_os = "linux")]
impl DerefMut for LocalDriverSession {
  fn deref_mut(&mut self) -> &mut Self::Target {
    match self {
      Self::Linux(session) => session,
    }
  }
}

#[cfg(target_os = "macos")]
impl DerefMut for LocalDriverSession {
  fn deref_mut(&mut self) -> &mut Self::Target {
    match self {
      Self::Macos(session) => session,
    }
  }
}

#[cfg(target_os = "windows")]
impl DerefMut for LocalDriverSession {
  fn deref_mut(&mut self) -> &mut Self::Target {
    match self {
      Self::Windows(session) => session,
    }
  }
}

impl Driver for LocalDriver {
  type Session = LocalDriverSession;

  fn descriptor(&self) -> DriverDescriptor {
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    {
      return self.inner.descriptor();
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
      unsupported_local_descriptor()
    }
  }

  fn open_local(&self) -> DriverResult<Self::Session> {
    #[cfg(target_os = "linux")]
    {
      return self.inner.open_local().map(LocalDriverSession::Linux);
    }

    #[cfg(target_os = "macos")]
    {
      return self.inner.open_local().map(LocalDriverSession::Macos);
    }

    #[cfg(target_os = "windows")]
    {
      return self.inner.open_local().map(LocalDriverSession::Windows);
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
      Err(DriverError::unsupported("driver.open_local"))
    }
  }
}

impl DriverSession for LocalDriverSession {
  fn descriptor(&self) -> DriverDescriptor {
    match self {
      #[cfg(target_os = "linux")]
      Self::Linux(session) => session.descriptor(),
      #[cfg(target_os = "macos")]
      Self::Macos(session) => session.descriptor(),
      #[cfg(target_os = "windows")]
      Self::Windows(session) => session.descriptor(),
    }
  }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn unsupported_local_descriptor() -> DriverDescriptor {
  DriverDescriptor {
    id: "unsupported.local",
    platform: PlatformKind::Remote,
    description: "unsupported local driver",
  }
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
