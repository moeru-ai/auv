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
      description: "test driver",
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
      description: "test session",
    }
  }
}

#[test]
fn driver_traits_use_typed_sessions() -> DriverResult<()> {
  let driver = TestDriver;
  let session = driver.open_local()?;

  assert_eq!(driver.descriptor().id, "test");
  assert_eq!(session.descriptor().description, "test session");

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
