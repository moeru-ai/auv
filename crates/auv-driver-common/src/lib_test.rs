use crate::{Driver, DriverDescriptor, DriverResult, DriverSession, PlatformKind};

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
