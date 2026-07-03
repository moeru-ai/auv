use std::collections::HashMap;
use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::os::fd::OwnedFd as StdOwnedFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use auv_driver::error::DriverResult;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedFd, OwnedObjectPath, OwnedValue, Value};

use crate::error::backend;

use super::request::{
  PORTAL_DESTINATION, create_remote_desktop_session, portal_proxy, session_connection,
};

const CLIPBOARD_INTERFACE: &str = "org.freedesktop.portal.Clipboard";
const REMOTE_DESKTOP_INTERFACE: &str = "org.freedesktop.portal.RemoteDesktop";
const TEXT_MIME: &str = "text/plain;charset=utf-8";
const FD_TRANSFER_TIMEOUT: Duration = Duration::from_secs(2);
const FD_TRANSFER_POLL_INTERVAL: Duration = Duration::from_millis(10);

pub struct PortalClipboard;

impl PortalClipboard {
  pub fn open() -> DriverResult<ClipboardSession> {
    ClipboardSession::open()
  }
}

pub struct ClipboardSession {
  connection: Connection,
  session_handle: OwnedObjectPath,
  text: Arc<Mutex<String>>,
  owns_selection: bool,
  running: Arc<AtomicBool>,
  transfer_thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for ClipboardSession {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter
      .debug_struct("ClipboardSession")
      .field("session_handle", &self.session_handle)
      .finish_non_exhaustive()
  }
}

impl ClipboardSession {
  fn open() -> DriverResult<Self> {
    let connection = session_connection()?;
    let session_handle = create_remote_desktop_session(&connection)?;
    {
      let clipboard = portal_proxy(&connection, CLIPBOARD_INTERFACE)?;
      let options: HashMap<&str, Value<'_>> = HashMap::new();
      clipboard
        .call_method("RequestClipboard", &(&session_handle, options))
        .map_err(|error| {
          backend(format!(
            "failed to request portal clipboard access: {error}"
          ))
        })?;
    }
    // TODO(linux-portal-clipboard-devices): explicit
    // `RemoteDesktop.SelectDevices(types=0)` was tested on GNOME Wayland but
    // did not return a portal response. Keep clipboard-only startup on
    // RequestClipboard+Start until an owner-approved input/libei slice defines
    // device selection policy.

    let results = start_remote_desktop(&connection, &session_handle)?;
    let clipboard_enabled = results
      .get("clipboard_enabled")
      .and_then(|value| bool::try_from(value).ok())
      .unwrap_or(true);
    if !clipboard_enabled {
      return Err(backend(
        "remote desktop portal started without clipboard access",
      ));
    }

    let text = Arc::new(Mutex::new(String::new()));
    let running = Arc::new(AtomicBool::new(true));
    let transfer_thread = Some(spawn_transfer_thread(
      connection.clone(),
      session_handle.clone(),
      Arc::clone(&text),
      Arc::clone(&running),
    )?);
    Ok(Self {
      connection,
      session_handle,
      text,
      owns_selection: false,
      running,
      transfer_thread,
    })
  }

  pub fn snapshot(&mut self) -> DriverResult<String> {
    let clipboard = portal_proxy(&self.connection, CLIPBOARD_INTERFACE)?;
    let fd: OwnedFd = match clipboard.call("SelectionRead", &(&self.session_handle, TEXT_MIME)) {
      Ok(fd) => fd,
      Err(error) => {
        if self.owns_selection {
          let text = self
            .text
            .lock()
            .expect("clipboard owner text lock poisoned")
            .clone();
          return Ok(text);
        }
        let message = error.to_string();
        if message.contains("NoSelection")
          || message.contains("No such selection")
          || message.contains("Failed to selection read")
        {
          return Ok(String::new());
        }
        Err(backend(format!(
          "failed to read portal clipboard text: {error}"
        )))
      }
    };
    let std_fd = StdOwnedFd::from(fd);
    let mut file = File::from(std_fd);
    let bytes = read_fd_to_end(&mut file)?;
    String::from_utf8(bytes)
      .map_err(|error| backend(format!("portal clipboard returned non-UTF-8 text: {error}")))
  }

  pub fn set_text(&mut self, text: &str) -> DriverResult<()> {
    *self
      .text
      .lock()
      .expect("clipboard owner text lock poisoned") = text.to_string();
    let clipboard = portal_proxy(&self.connection, CLIPBOARD_INTERFACE)?;
    let mut options = HashMap::new();
    options.insert("mime_types", Value::from(vec![TEXT_MIME]));
    clipboard
      .call_method("SetSelection", &(&self.session_handle, options))
      .map_err(|error| backend(format!("failed to set portal clipboard selection: {error}")))?;
    self.owns_selection = true;
    Ok(())
  }
}

impl Drop for ClipboardSession {
  fn drop(&mut self) {
    self.running.store(false, Ordering::SeqCst);
    let _ = close_session(&self.connection, &self.session_handle);
    // NOTICE(linux-portal-clipboard-thread): zbus blocking signal iteration has
    // no cheap cancellation hook. Closing the portal session causes future
    // transfer requests to stop; the thread is intentionally detached.
    let _ = self.transfer_thread.take();
  }
}

fn start_remote_desktop(
  connection: &Connection,
  session_handle: &OwnedObjectPath,
) -> DriverResult<HashMap<String, OwnedValue>> {
  let handle_token = super::request::portal_token("start");
  let request = portal_request_proxy(connection, &handle_token)?;
  let proxy = portal_proxy(connection, REMOTE_DESKTOP_INTERFACE)?;
  let mut options = HashMap::new();
  options.insert("handle_token", Value::from(handle_token.as_str()));
  proxy
    .call_method("Start", &(session_handle, "", options))
    .map_err(|error| {
      backend(format!(
        "failed to start remote desktop portal session: {error}"
      ))
    })?;
  wait_response(&request, REMOTE_DESKTOP_INTERFACE, "Start")
}

fn spawn_transfer_thread(
  connection: Connection,
  session_handle: OwnedObjectPath,
  text: Arc<Mutex<String>>,
  running: Arc<AtomicBool>,
) -> DriverResult<JoinHandle<()>> {
  let (ready_tx, ready_rx) = mpsc::channel();
  let handle = thread::spawn(move || {
    let Ok(clipboard) = portal_proxy(&connection, CLIPBOARD_INTERFACE) else {
      let _ = ready_tx.send(Err("failed to create clipboard signal proxy".to_string()));
      return;
    };
    let Ok(mut transfers) = clipboard.receive_signal("SelectionTransfer") else {
      let _ = ready_tx.send(Err(
        "failed to subscribe to clipboard transfers".to_string(),
      ));
      return;
    };
    let _ = ready_tx.send(Ok(()));
    while running.load(Ordering::SeqCst) {
      let Some(message) = transfers.next() else {
        break;
      };
      let Ok((transfer_session, mime_type, serial)) = message
        .body()
        .deserialize::<(OwnedObjectPath, String, u32)>()
      else {
        continue;
      };
      if transfer_session != session_handle || mime_type != TEXT_MIME {
        let _ = selection_write_done(&connection, &session_handle, serial, false);
        continue;
      }
      let payload = text
        .lock()
        .expect("clipboard owner text lock poisoned")
        .clone();
      let result =
        write_selection_payload(&connection, &session_handle, serial, payload.as_bytes());
      let _ = selection_write_done(&connection, &session_handle, serial, result.is_ok());
    }
  });
  match ready_rx.recv_timeout(FD_TRANSFER_TIMEOUT) {
    Ok(Ok(())) => {}
    Ok(Err(error)) => return Err(backend(error)),
    Err(error) => {
      return Err(backend(format!(
        "timed out waiting for clipboard transfer listener: {error}"
      )));
    }
  }
  Ok(handle)
}

fn write_selection_payload(
  connection: &Connection,
  session_handle: &OwnedObjectPath,
  serial: u32,
  payload: &[u8],
) -> DriverResult<()> {
  let clipboard = portal_proxy(connection, CLIPBOARD_INTERFACE)?;
  let fd: OwnedFd = clipboard
    .call("SelectionWrite", &(session_handle, serial))
    .map_err(|error| backend(format!("failed to open portal clipboard write fd: {error}")))?;
  let std_fd = StdOwnedFd::from(fd);
  let mut file = File::from(std_fd);
  write_fd_all(&mut file, payload)?;
  Ok(())
}

fn read_fd_to_end(file: &mut File) -> DriverResult<Vec<u8>> {
  let started = Instant::now();
  let mut bytes = Vec::new();
  let mut buffer = [0_u8; 8192];
  loop {
    match file.read(&mut buffer) {
      Ok(0) => return Ok(bytes),
      Ok(read) => bytes.extend_from_slice(&buffer[..read]),
      Err(error) if error.kind() == ErrorKind::WouldBlock => {
        if started.elapsed() >= FD_TRANSFER_TIMEOUT {
          return Err(backend("timed out reading portal clipboard fd"));
        }
        thread::sleep(FD_TRANSFER_POLL_INTERVAL);
      }
      Err(error) => {
        return Err(backend(format!(
          "failed to read portal clipboard fd: {error}"
        )));
      }
    }
  }
}

fn write_fd_all(file: &mut File, payload: &[u8]) -> DriverResult<()> {
  let started = Instant::now();
  let mut written = 0;
  while written < payload.len() {
    match file.write(&payload[written..]) {
      Ok(0) => return Err(backend("portal clipboard write fd closed early")),
      Ok(count) => written += count,
      Err(error) if error.kind() == ErrorKind::WouldBlock => {
        if started.elapsed() >= FD_TRANSFER_TIMEOUT {
          return Err(backend("timed out writing portal clipboard fd"));
        }
        thread::sleep(FD_TRANSFER_POLL_INTERVAL);
      }
      Err(error) => {
        return Err(backend(format!(
          "failed to write portal clipboard payload: {error}"
        )));
      }
    }
  }
  Ok(())
}

fn selection_write_done(
  connection: &Connection,
  session_handle: &OwnedObjectPath,
  serial: u32,
  success: bool,
) -> DriverResult<()> {
  let clipboard = portal_proxy(connection, CLIPBOARD_INTERFACE)?;
  clipboard
    .call_method("SelectionWriteDone", &(session_handle, serial, success))
    .map_err(|error| backend(format!("failed to finish portal clipboard write: {error}")))?;
  Ok(())
}

fn close_session(connection: &Connection, session_handle: &OwnedObjectPath) -> DriverResult<()> {
  let session = Proxy::new(
    connection,
    PORTAL_DESTINATION,
    session_handle.clone(),
    "org.freedesktop.portal.Session",
  )
  .map_err(|error| backend(format!("failed to create portal session proxy: {error}")))?;
  session
    .call_method("Close", &())
    .map_err(|error| backend(format!("failed to close portal session: {error}")))?;
  Ok(())
}

fn portal_request_proxy<'a>(
  connection: &'a Connection,
  handle_token: &str,
) -> DriverResult<Proxy<'a>> {
  let unique_name = connection
    .unique_name()
    .ok_or_else(|| backend("session bus connection has no unique name"))?
    .trim_start_matches(':')
    .replace('.', "_");
  let path = format!("/org/freedesktop/portal/desktop/request/{unique_name}/{handle_token}");
  Proxy::new(
    connection,
    PORTAL_DESTINATION,
    path,
    "org.freedesktop.portal.Request",
  )
  .map_err(|error| backend(format!("failed to create portal request proxy: {error}")))
}

fn wait_response(
  request: &Proxy<'_>,
  interface: &'static str,
  method: &'static str,
) -> DriverResult<HashMap<String, OwnedValue>> {
  let mut responses = request.receive_signal("Response").map_err(|error| {
    backend(format!(
      "failed to subscribe to {interface}.{method} response: {error}"
    ))
  })?;
  let response = responses
    .next()
    .ok_or_else(|| backend(format!("{interface}.{method} did not return a response")))?;
  let (code, results): (u32, HashMap<String, OwnedValue>) =
    response.body().deserialize().map_err(|error| {
      backend(format!(
        "failed to decode {interface}.{method} response: {error}"
      ))
    })?;
  if code == 0 {
    Ok(results)
  } else {
    Err(portal_response_error(interface, method, code))
  }
}

fn portal_response_error(
  interface: &'static str,
  method: &'static str,
  code: u32,
) -> auv_driver::error::DriverError {
  let reason = match code {
    1 => "cancelled or denied by the portal",
    2 => "failed",
    _ => "returned an unknown response code",
  };
  backend(format!(
    "{interface}.{method} {reason} (response code {code})"
  ))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn text_mime_matches_portal_plain_text_contract() {
    assert_eq!(TEXT_MIME, "text/plain;charset=utf-8");
  }
}
