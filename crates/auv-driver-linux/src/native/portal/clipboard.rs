use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::os::fd::OwnedFd as StdOwnedFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use ashpd::desktop::Session;
use ashpd::desktop::clipboard::{Clipboard, SetSelectionOptions};
use ashpd::desktop::remote_desktop::RemoteDesktop;
use auv_driver_common::error::DriverResult;
use futures_lite::{StreamExt, future};

use crate::error::backend;

use super::request::{run, session_connection};

const TEXT_MIME: &str = "text/plain;charset=utf-8";
const FD_TRANSFER_TIMEOUT: Duration = Duration::from_secs(2);
const FD_TRANSFER_POLL_INTERVAL: Duration = Duration::from_millis(10);

pub struct PortalClipboard;

impl PortalClipboard {
  pub fn open(app_id: Option<&str>) -> DriverResult<ClipboardSession> {
    ClipboardSession::open(app_id)
  }
}

pub struct ClipboardSession {
  clipboard: Arc<Clipboard>,
  session: Arc<Session<RemoteDesktop>>,
  text: Arc<Mutex<String>>,
  owns_selection: bool,
  running: Arc<AtomicBool>,
  transfer_thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for ClipboardSession {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter.debug_struct("ClipboardSession").field("session", &self.session).finish_non_exhaustive()
  }
}

impl ClipboardSession {
  fn open(app_id: Option<&str>) -> DriverResult<Self> {
    let connection = session_connection(app_id)?;
    let remote_desktop = run("open clipboard RemoteDesktop", RemoteDesktop::with_connection(connection.clone()))?;
    let clipboard = Arc::new(run("open Clipboard", Clipboard::with_connection(connection))?);
    let session = Arc::new(run("create clipboard session", remote_desktop.create_session(Default::default()))?);
    let mut owner = Self {
      clipboard,
      session,
      text: Arc::new(Mutex::new(String::new())),
      owns_selection: false,
      running: Arc::new(AtomicBool::new(true)),
      transfer_thread: None,
    };
    run("request clipboard access", owner.clipboard.request(&owner.session, Default::default()))?;
    // TODO(linux-portal-clipboard-devices): explicit
    // `RemoteDesktop.SelectDevices(types=0)` was tested on GNOME Wayland but
    // did not return a portal response. Keep clipboard-only startup on
    // RequestClipboard+Start until an owner-approved input/libei slice defines
    // device selection policy.
    let selected =
      run("start clipboard session", async { remote_desktop.start(&owner.session, None, Default::default()).await?.response() })?;
    if !selected.is_clipboard_enabled() {
      return Err(backend("remote desktop portal started without clipboard access"));
    }
    owner.transfer_thread = Some(spawn_transfer_thread(Arc::clone(&owner.clipboard), Arc::clone(&owner.text), Arc::clone(&owner.running))?);
    Ok(owner)
  }

  pub fn snapshot(&mut self) -> DriverResult<String> {
    let fd = match run("read portal clipboard", self.clipboard.selection_read(&self.session, TEXT_MIME)) {
      Ok(fd) => fd,
      Err(error) => {
        if self.owns_selection {
          let text = self.text.lock().expect("clipboard owner text lock poisoned").clone();
          return Ok(text);
        }
        let message = error.to_string();
        if message.contains("NoSelection") || message.contains("No such selection") || message.contains("Failed to selection read") {
          return Ok(String::new());
        }
        return Err(backend(format!("failed to read portal clipboard text: {error}")));
      }
    };
    let std_fd = StdOwnedFd::from(fd);
    let mut file = File::from(std_fd);
    let bytes = read_fd_to_end(&mut file)?;
    String::from_utf8(bytes).map_err(|error| backend(format!("portal clipboard returned non-UTF-8 text: {error}")))
  }

  pub fn set_text(&mut self, text: &str) -> DriverResult<()> {
    *self.text.lock().expect("clipboard owner text lock poisoned") = text.to_string();
    run(
      "set clipboard selection",
      self.clipboard.set_selection(&self.session, SetSelectionOptions::default().set_mime_types(&[TEXT_MIME])),
    )?;
    self.owns_selection = true;
    Ok(())
  }
}

impl Drop for ClipboardSession {
  fn drop(&mut self) {
    self.running.store(false, Ordering::SeqCst);
    let _ = run("close clipboard session", self.session.close());
    if let Some(thread) = self.transfer_thread.take() {
      let _ = thread.join();
    }
  }
}

/// Each clipboard owner has a dedicated Portal connection with one session.
/// Poll the typed signal stream so shutdown does not leave a detached listener.
fn spawn_transfer_thread(clipboard: Arc<Clipboard>, text: Arc<Mutex<String>>, running: Arc<AtomicBool>) -> DriverResult<JoinHandle<()>> {
  let (ready_tx, ready_rx) = mpsc::channel();
  let thread_running = Arc::clone(&running);
  let handle = thread::spawn(move || {
    let transfers = match run("subscribe to clipboard transfers", clipboard.receive_selection_transfer::<RemoteDesktop>()) {
      Ok(transfers) => transfers,
      Err(error) => {
        let _ = ready_tx.send(Err(error));
        return;
      }
    };
    let mut transfers = std::pin::pin!(transfers);
    if ready_tx.send(Ok(())).is_err() {
      return;
    }
    while thread_running.load(Ordering::SeqCst) {
      let transfer = future::block_on(future::race(async { Some(transfers.next().await) }, async {
        async_io::Timer::after(Duration::from_millis(100)).await;
        None
      }));
      let Some(transfer) = transfer else {
        continue;
      };
      let Some((session, mime_type, serial)) = transfer else {
        break;
      };
      let result = if mime_type == TEXT_MIME {
        let payload = text.lock().expect("clipboard owner text lock poisoned").clone();
        run("open clipboard write fd", clipboard.selection_write(&session, serial))
          .and_then(|fd| write_fd_all(&mut File::from(StdOwnedFd::from(fd)), payload.as_bytes()))
      } else {
        Err(backend("unsupported clipboard MIME type"))
      };
      let _ = run("finish clipboard write", clipboard.selection_write_done(&session, serial, result.is_ok()));
    }
  });
  let ready = ready_rx
    .recv_timeout(FD_TRANSFER_TIMEOUT)
    .map_err(|error| backend(format!("clipboard transfer listener did not start: {error}")))
    .and_then(|result| result);
  if let Err(error) = ready {
    running.store(false, Ordering::SeqCst);
    let _ = handle.join();
    return Err(error);
  }
  Ok(handle)
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
        return Err(backend(format!("failed to read portal clipboard fd: {error}")));
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
        return Err(backend(format!("failed to write portal clipboard payload: {error}")));
      }
    }
  }
  Ok(())
}

#[cfg(test)]
#[path = "clipboard_test.rs"]
mod tests;
