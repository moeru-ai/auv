//! An isolated D-Bus fixture validates connection attribution without a desktop.
use super::*;
use std::collections::HashMap;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

const PORTAL_DESTINATION: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
use std::io::BufRead;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct IdentityFixture(Arc<Mutex<HashMap<String, String>>>);

#[zbus::interface(name = "org.freedesktop.host.portal.Registry")]
impl IdentityFixture {
  #[zbus(property, name = "version")]
  fn version(&self) -> u32 {
    1
  }
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
  #[zbus(property, name = "version")]
  fn version(&self) -> u32 {
    2
  }
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
    let error = run("create session", async {
      let proxy = ashpd::desktop::remote_desktop::RemoteDesktop::with_connection(connection).await?;
      proxy.create_session(Default::default()).await
    })
    .unwrap_err();
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

#[derive(Clone, Default)]
struct DeliveryFixture(Arc<Mutex<Vec<String>>>, Arc<Mutex<Option<(i32, u32)>>>);

// Reply before returning the method call: consumers must already be subscribed
// and must inspect the response code, including an empty cancellation response.
async fn respond(
  connection: &zbus::Connection,
  header: &zbus::message::Header<'_>,
  options: &HashMap<String, OwnedValue>,
  code: u32,
  results: HashMap<&str, zbus::zvariant::Value<'_>>,
) -> zbus::fdo::Result<OwnedObjectPath> {
  let sender = header.sender().unwrap();
  let peer = sender.trim_start_matches(':').replace('.', "_");
  let token = <&str>::try_from(options.get("handle_token").unwrap()).unwrap();
  let path = OwnedObjectPath::try_from(format!("{PORTAL_PATH}/request/{peer}/{token}")).unwrap();
  connection.emit_signal(Some(sender.as_str()), &path, "org.freedesktop.portal.Request", "Response", &(code, results)).await?;
  Ok(path)
}

#[zbus::interface(name = "org.freedesktop.portal.RemoteDesktop")]
impl DeliveryFixture {
  #[zbus(property, name = "version")]
  fn version(&self) -> u32 {
    2
  }

  async fn create_session(
    &self,
    options: HashMap<String, OwnedValue>,
    #[zbus(header)] header: zbus::message::Header<'_>,
    #[zbus(connection)] connection: &zbus::Connection,
  ) -> zbus::fdo::Result<OwnedObjectPath> {
    let peer = header.sender().unwrap().trim_start_matches(':').replace('.', "_");
    let token = <&str>::try_from(options.get("session_handle_token").unwrap()).unwrap();
    let path = format!("{PORTAL_PATH}/session/{peer}/{token}");
    connection.object_server().at(path.as_str(), DeliverySession(self.clone())).await?;
    respond(connection, &header, &options, 0, HashMap::from([("session_handle", zbus::zvariant::Value::from(path.as_str()))])).await
  }

  async fn select_devices(
    &self,
    _session: OwnedObjectPath,
    options: HashMap<String, OwnedValue>,
    #[zbus(header)] header: zbus::message::Header<'_>,
    #[zbus(connection)] connection: &zbus::Connection,
  ) -> zbus::fdo::Result<OwnedObjectPath> {
    assert_eq!(u32::try_from(options.get("types").unwrap()).unwrap(), 3);
    let cancelled = self.0.lock().unwrap().iter().any(|event| event == "close");
    assert_eq!(u32::try_from(options.get("persist_mode").unwrap()).unwrap(), 2);
    assert_eq!(<&str>::try_from(options.get("restore_token").unwrap()).unwrap(), if cancelled { "replacement" } else { "initial" });
    self.0.lock().unwrap().push("select_devices".into());
    respond(connection, &header, &options, u32::from(cancelled), HashMap::new()).await
  }

  async fn start(
    &self,
    _session: OwnedObjectPath,
    parent: &str,
    options: HashMap<String, OwnedValue>,
    #[zbus(header)] header: zbus::message::Header<'_>,
    #[zbus(connection)] connection: &zbus::Connection,
  ) -> zbus::fdo::Result<OwnedObjectPath> {
    assert!(parent.is_empty());
    self.0.lock().unwrap().push("start".into());
    use zbus::zvariant::Value;
    let metadata = HashMap::from([
      ("position", Value::from((0_i32, 0_i32))),
      ("size", Value::from((800_i32, 600_i32))),
    ]);
    respond(
      connection,
      &header,
      &options,
      0,
      HashMap::from([
        ("restore_token", Value::from("replacement")),
        ("devices", Value::from(3_u32)),
        ("streams", Value::from(vec![(7_u32, metadata)])),
      ]),
    )
    .await
  }

  fn notify_keyboard_keysym(
    &self,
    _session: OwnedObjectPath,
    _options: HashMap<String, OwnedValue>,
    key: i32,
    state: u32,
  ) -> zbus::fdo::Result<()> {
    self.0.lock().unwrap().push(format!("key:{key}:{state}"));
    if *self.1.lock().unwrap() == Some((key, state)) {
      return Err(zbus::fdo::Error::Failed("injected key reply failure".into()));
    }
    Ok(())
  }

  fn notify_pointer_motion_absolute(&self, _session: OwnedObjectPath, _options: HashMap<String, OwnedValue>, stream: u32, x: f64, y: f64) {
    assert_eq!((stream, x, y), (7, 20.0, 30.0));
    self.0.lock().unwrap().push("motion".into());
  }

  fn notify_pointer_button(&self, _session: OwnedObjectPath, _options: HashMap<String, OwnedValue>, button: i32, state: u32) {
    self.0.lock().unwrap().push(format!("button:{button}:{state}"));
  }
}

struct DeliveryScreenCast(DeliveryFixture);
#[zbus::interface(name = "org.freedesktop.portal.ScreenCast")]
impl DeliveryScreenCast {
  #[zbus(property, name = "version")]
  fn version(&self) -> u32 {
    5
  }

  async fn select_sources(
    &self,
    _session: OwnedObjectPath,
    options: HashMap<String, OwnedValue>,
    #[zbus(header)] header: zbus::message::Header<'_>,
    #[zbus(connection)] connection: &zbus::Connection,
  ) -> zbus::fdo::Result<OwnedObjectPath> {
    assert_eq!(u32::try_from(options.get("types").unwrap()).unwrap(), 1);
    assert_eq!(u32::try_from(options.get("cursor_mode").unwrap()).unwrap(), 1);
    assert!(bool::try_from(options.get("multiple").unwrap()).unwrap());
    self.0.0.lock().unwrap().push("select_sources".into());
    respond(connection, &header, &options, 0, HashMap::new()).await
  }
}

struct DeliverySession(DeliveryFixture);
#[zbus::interface(name = "org.freedesktop.portal.Session")]
impl DeliverySession {
  fn close(&self) {
    self.0.0.lock().unwrap().push("close".into());
  }
}

#[test]
#[ignore = "requires dbus-daemon; run with --include-ignored"]
fn modified_click_reaches_portal_and_cancelled_selection_closes_session() {
  if std::env::var_os("AUV_PORTAL_DELIVERY_TEST_CHILD").is_some() {
    use crate::native::portal::PortalInput;
    use auv_driver_common::{geometry::Point, input::Click};
    let directory = tempfile::tempdir().unwrap();
    let token_path = directory.path().join("remote-desktop-input-token");
    std::fs::write(&token_path, "initial").unwrap();
    let store = crate::native::portal::RestoreTokenStore::new(directory.path().to_path_buf());
    use auv_driver_common::{ClickModifiers, Driver, DriverError};
    let session = crate::LinuxDriver::new().with_portal_state_root(directory.path().to_path_buf()).open_local().unwrap();
    session.input().move_to(Point::new(20.0, 30.0)).unwrap();
    assert_eq!(std::fs::read_to_string(&token_path).unwrap(), "replacement");
    // ROOT CAUSE: every input error dropped the session. A rejected coordinate
    // must preserve authorization: the next valid click uses the same session.
    assert!(matches!(session.input().move_to(Point::new(900.0, 30.0)), Err(DriverError::InvalidInput { .. })));
    session
      .input()
      .click_at(
        Point::new(20.0, 30.0),
        Click::Single,
        ClickModifiers {
          shift: true,
          control: true,
          ..Default::default()
        },
      )
      .unwrap();
    drop(session);
    assert!(PortalInput::open(Some(&store), None).is_err(), "a cancelled selection must not continue to Start");
    return;
  }
  let mut bus =
    PrivateBus(Command::new("dbus-daemon").args(["--session", "--nofork", "--print-address=1"]).stdout(Stdio::piped()).spawn().unwrap());
  let mut address = String::new();
  std::io::BufReader::new(bus.0.stdout.take().unwrap()).read_line(&mut address).unwrap();
  let fixture = DeliveryFixture::default();
  let _service = zbus::blocking::connection::Builder::address(address.trim())
    .unwrap()
    .name(PORTAL_DESTINATION)
    .unwrap()
    .serve_at(PORTAL_PATH, fixture.clone())
    .unwrap()
    .serve_at(PORTAL_PATH, DeliveryScreenCast(fixture.clone()))
    .unwrap()
    .build()
    .unwrap();
  let output = Command::new(std::env::current_exe().unwrap())
    .args([
      "--ignored",
      "modified_click_reaches_portal_and_cancelled_selection_closes_session",
    ])
    .env("AUV_PORTAL_DELIVERY_TEST_CHILD", "1")
    .env("DBUS_SESSION_BUS_ADDRESS", address.trim())
    .env_remove("WAYLAND_DISPLAY")
    .output()
    .unwrap();
  assert!(output.status.success(), "{} {}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
  assert_eq!(
    *fixture.0.lock().unwrap(),
    [
      "select_devices",
      "select_sources",
      "start",
      "motion",
      "motion",
      "key:65505:1",
      "key:65507:1",
      "button:272:1",
      "button:272:0",
      "key:65507:0",
      "key:65505:0",
      "close",
      "select_devices",
      "close",
    ]
  );
}

// ROOT CAUSE:
// A modifier press error returned before cleanup, and release errors were ignored.
// The receiver records even failed replies: delivery may precede a bus failure.
#[test]
#[ignore = "requires dbus-daemon; run with --include-ignored"]
fn keyboard_failure_releases_all_attempted_keys_and_reports_release_error() {
  if let Ok(case) = std::env::var("AUV_PORTAL_KEYBOARD_TEST_CHILD") {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("remote-desktop-input-token"), "initial").unwrap();
    let store = crate::native::portal::RestoreTokenStore::new(directory.path().to_path_buf());
    let mut session = crate::native::portal::PortalInput::open(Some(&store), None).unwrap();
    let result = if case == "ordinary" {
      session.key_press(97)
    } else {
      session.key_chord(&[65505, 65507], 97)
    };
    assert!(result.unwrap_err().to_string().contains("injected key reply failure"));
    return;
  }
  for (case, failed, expected) in [
    ("modifier", (65507, 1), vec!["key:65505:1", "key:65507:1", "key:65507:0", "key:65505:0"]),
    (
      "release",
      (65507, 0),
      vec![
        "key:65505:1",
        "key:65507:1",
        "key:97:1",
        "key:97:0",
        "key:65507:0",
        "key:65505:0",
      ],
    ),
    ("ordinary", (97, 1), vec!["key:97:1", "key:97:0"]),
  ] {
    let mut bus =
      PrivateBus(Command::new("dbus-daemon").args(["--session", "--nofork", "--print-address=1"]).stdout(Stdio::piped()).spawn().unwrap());
    let mut address = String::new();
    std::io::BufReader::new(bus.0.stdout.take().unwrap()).read_line(&mut address).unwrap();
    let fixture = DeliveryFixture::default();
    *fixture.1.lock().unwrap() = Some(failed);
    let _service = zbus::blocking::connection::Builder::address(address.trim())
      .unwrap()
      .name(PORTAL_DESTINATION)
      .unwrap()
      .serve_at(PORTAL_PATH, fixture.clone())
      .unwrap()
      .serve_at(PORTAL_PATH, DeliveryScreenCast(fixture.clone()))
      .unwrap()
      .build()
      .unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
      .args([
        "--ignored",
        "keyboard_failure_releases_all_attempted_keys_and_reports_release_error",
      ])
      .env("AUV_PORTAL_KEYBOARD_TEST_CHILD", case)
      .env("DBUS_SESSION_BUS_ADDRESS", address.trim())
      .env_remove("WAYLAND_DISPLAY")
      .output()
      .unwrap();
    assert!(output.status.success(), "{case}: {} {}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let events = fixture.0.lock().unwrap();
    assert_eq!(events.iter().filter(|event| event.starts_with("key:")).map(String::as_str).collect::<Vec<_>>(), expected, "{case}");
  }
}
