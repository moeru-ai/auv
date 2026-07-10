pub use auv_driver_common::*;

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
    summary: "unsupported local driver",
  }
}

#[cfg(test)]
mod tests {
  use std::ops::Deref;

  use crate::{Driver, DriverDescriptor, DriverResult, DriverSession, LocalDriver, PlatformKind};

  #[derive(Clone, Copy)]
  struct TestDriver;

  #[derive(Clone, Copy)]
  struct TestSession;

  impl Driver for TestDriver {
    type Session = TestSession;

    fn descriptor(&self) -> DriverDescriptor {
      DriverDescriptor {
        id: "test",
        platform: PlatformKind::Fixture,
        summary: "test driver",
      }
    }

    fn open_local(&self) -> DriverResult<Self::Session> {
      Ok(TestSession)
    }
  }

  impl DriverSession for TestSession {
    fn descriptor(&self) -> DriverDescriptor {
      DriverDescriptor {
        id: "test-session",
        platform: PlatformKind::Fixture,
        summary: "test session",
      }
    }
  }

  #[test]
  fn driver_traits_use_typed_sessions() -> DriverResult<()> {
    let driver = TestDriver;
    let session = driver.open_local()?;

    assert_eq!(driver.descriptor().id, "test");
    assert_eq!(session.descriptor().summary, "test session");

    let _ = PlatformKind::Macos;
    let _ = PlatformKind::Windows;
    let _ = PlatformKind::Linux;
    let _ = PlatformKind::Android;
    let _ = PlatformKind::Ios;
    let _ = PlatformKind::Browser;
    let _ = PlatformKind::Fixture;
    let _ = PlatformKind::Remote;

    Ok(())
  }

  #[test]
  fn local_driver_descriptor_matches_target_platform() {
    let descriptor = LocalDriver::new().descriptor();

    assert_eq!(descriptor.id, expected_driver_id());
    assert_eq!(descriptor.platform, expected_platform());
  }

  #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
  #[test]
  fn open_local_returns_target_platform_session() -> DriverResult<()> {
    let session = crate::open_local()?;
    let descriptor = session.descriptor();

    assert_eq!(descriptor.id, expected_driver_id());
    assert_eq!(descriptor.platform, expected_platform());

    Ok(())
  }

  #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
  #[test]
  fn open_local_dereferences_to_target_platform_session() -> DriverResult<()> {
    let session = crate::open_local()?;

    assert_eq!(Deref::deref(&session).descriptor().id, expected_driver_id());

    Ok(())
  }

  #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
  #[test]
  fn open_local_rejects_targets_without_local_desktop_driver() {
    let error = crate::open_local().expect_err("non-desktop target should not have a local desktop driver");

    assert_eq!(error.to_string(), "driver.open_local is not supported by this driver");
  }

  #[cfg(target_os = "linux")]
  fn expected_platform() -> PlatformKind {
    PlatformKind::Linux
  }

  #[cfg(target_os = "macos")]
  fn expected_platform() -> PlatformKind {
    PlatformKind::Macos
  }

  #[cfg(target_os = "windows")]
  fn expected_platform() -> PlatformKind {
    PlatformKind::Windows
  }

  #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
  fn expected_platform() -> PlatformKind {
    PlatformKind::Remote
  }

  #[cfg(target_os = "linux")]
  fn expected_driver_id() -> &'static str {
    "linux.desktop"
  }

  #[cfg(target_os = "macos")]
  fn expected_driver_id() -> &'static str {
    "macos.desktop"
  }

  #[cfg(target_os = "windows")]
  fn expected_driver_id() -> &'static str {
    "windows.desktop"
  }

  #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
  fn expected_driver_id() -> &'static str {
    "unsupported.local"
  }
}
