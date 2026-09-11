//! An isolated D-Bus fixture validates connection attribution without a desktop.
use super::*;
use std::io::BufRead;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct IdentityFixture(Arc<Mutex<HashMap<String, String>>>);

#[zbus::interface(name = "org.freedesktop.host.portal.Registry")]
impl IdentityFixture {
  fn register(
    &self,
    app_id: &str,
    _options: HashMap<String, OwnedValue>,
    #[zbus(header)] header: zbus::message::Header<'_>,
  ) -> zbus::fdo::Result<()> {
    let sender = header.sender().unwrap().to_string();
    let mut peers = self.0.lock().unwrap();
    if peers.insert(sender, app_id.to_string()).is_some() {
      return Err(zbus::fdo::Error::Failed("connection already registered".into()));
    }
    Ok(())
  }
}

struct DesktopFixture(IdentityFixture);

#[zbus::interface(name = "org.freedesktop.portal.RemoteDesktop")]
impl DesktopFixture {
  fn create_session(
    &self,
    _options: HashMap<String, OwnedValue>,
    #[zbus(header)] header: zbus::message::Header<'_>,
  ) -> zbus::fdo::Result<OwnedObjectPath> {
    let peers = self.0.0.lock().unwrap();
    let registered = peers.get(header.sender().unwrap().as_str()).map(String::as_str);
    // No permission request or input is sent by this fixture. Reaching this
    // controlled error proves Registry attribution on the Portal caller's peer.
    Err(zbus::fdo::Error::Failed(
      if registered == Some("ai.moeru.auv") {
        "identity verified"
      } else {
        "identity missing"
      }
      .into(),
    ))
  }
}

#[derive(Clone, Default)]
struct PermissionStoreFixture(Arc<Mutex<HashMap<String, Vec<String>>>>);

#[zbus::interface(name = "org.freedesktop.impl.portal.PermissionStore")]
impl PermissionStoreFixture {
  fn lookup(&self, table: &str, id: &str) -> (HashMap<String, Vec<String>>, OwnedValue) {
    assert_eq!((table, id), ("kde-authorized", "remote-desktop"));
    (self.0.lock().unwrap().clone(), OwnedValue::from(0_u32))
  }

  fn set_permission(&self, table: &str, create: bool, id: &str, app: &str, permissions: Vec<String>) {
    assert_eq!((table, id), ("kde-authorized", "remote-desktop"));
    assert!(create);
    self.0.lock().unwrap().insert(app.to_string(), permissions);
  }
}

struct PrivateBus(std::process::Child);
impl Drop for PrivateBus {
  fn drop(&mut self) {
    let _ = self.0.kill();
    let _ = self.0.wait();
  }
}

#[test]
#[ignore = "requires dbus-daemon; run with --ignored identity_is_registered_before_portal_calls_after_process_restart"]
fn identity_is_registered_before_portal_calls_after_process_restart() {
  if std::env::var_os("AUV_PORTAL_IDENTITY_TEST_CHILD").is_some() {
    let connection = session_connection(Some("ai.moeru.auv")).unwrap();
    let error = create_remote_desktop_session(&connection).unwrap_err();
    assert!(error.to_string().contains("identity verified"), "{error}");
    use crate::permission::{kde_authorization, set_kde_authorization};
    use auv_driver_common::permission::PermissionStatus;
    assert_eq!(kde_authorization("ai.moeru.auv").unwrap(), PermissionStatus::Missing);
    set_kde_authorization("ai.moeru.auv", true).unwrap();
    assert_eq!(kde_authorization("ai.moeru.auv").unwrap(), PermissionStatus::Granted);
    set_kde_authorization("ai.moeru.auv", false).unwrap();
    assert_eq!(kde_authorization("ai.moeru.auv").unwrap(), PermissionStatus::Missing);
    return;
  }
  let mut bus =
    PrivateBus(Command::new("dbus-daemon").args(["--session", "--nofork", "--print-address=1"]).stdout(Stdio::piped()).spawn().unwrap());
  let mut address = String::new();
  std::io::BufReader::new(bus.0.stdout.take().unwrap()).read_line(&mut address).unwrap();
  let fixture = IdentityFixture::default();
  let permissions = PermissionStoreFixture::default();
  permissions.0.lock().unwrap().insert("org.example.Other".into(), vec!["yes".into()]);
  let _service = zbus::blocking::connection::Builder::address(address.trim())
    .unwrap()
    .name(PORTAL_DESTINATION)
    .unwrap()
    .name("org.freedesktop.impl.portal.PermissionStore")
    .unwrap()
    .serve_at("/org/freedesktop/impl/portal/PermissionStore", permissions.clone())
    .unwrap()
    .serve_at(PORTAL_PATH, fixture.clone())
    .unwrap()
    .serve_at(PORTAL_PATH, DesktopFixture(fixture.clone()))
    .unwrap()
    .build()
    .unwrap();
  for _ in 0..2 {
    let output = Command::new(std::env::current_exe().unwrap())
      .args([
        "--ignored",
        "identity_is_registered_before_portal_calls_after_process_restart",
      ])
      .env("AUV_PORTAL_IDENTITY_TEST_CHILD", "1")
      .env("DBUS_SESSION_BUS_ADDRESS", address.trim())
      .output()
      .unwrap();
    assert!(output.status.success(), "{} {}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
  }
  assert_eq!(permissions.0.lock().unwrap().get("org.example.Other").unwrap(), &["yes"]);
  assert_eq!(fixture.0.lock().unwrap().len(), 2, "new processes must each register their own connection");
}
