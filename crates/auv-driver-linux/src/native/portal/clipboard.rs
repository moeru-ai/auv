use std::collections::HashMap;
use std::fs::File;
use std::os::fd::OwnedFd as StdOwnedFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use ashpd::desktop::Session;
use ashpd::desktop::clipboard::{Clipboard, SetSelectionOptions};
use ashpd::desktop::remote_desktop::RemoteDesktop;
use auv_driver_common::error::DriverResult;
use futures_lite::io::{AsyncReadExt, AsyncWriteExt};
use futures_lite::{StreamExt, future};
use zbus::zvariant::OwnedValue;

use crate::error::backend;

use super::request::{run, run_with_consent, session_connection};

const TEXT_MIME: &str = "text/plain;charset=utf-8";
const FD_TRANSFER_TIMEOUT: Duration = Duration::from_secs(2);
// NOTICE: an application answering Ctrl+V asks for the selection within a few
// event-loop turns; Chrome reads in several rounds and inserts a later one
// (seen on X11 in the OSWorld runs). Restoring the previous clipboard before
// the reader finished made the portal reject the pending SelectionWrite
// ("Transfer serial 1 doesn't match any transfer request") and nothing was
// pasted. One second to start, then until reads stop for 200 ms, at most five
// seconds, mirrors the X11 driver work (#361).
const PASTE_READ_START: Duration = Duration::from_secs(1);
const PASTE_READ_QUIET: Duration = Duration::from_millis(200);
const PASTE_READ_LIMIT: Duration = Duration::from_secs(5);
/// How long to wait for the compositor's own read of a selection we just set,
/// and how quiet reads must be before a paste shortcut is sent.
const OWN_SELECTION_READ_WAIT: Duration = Duration::from_millis(300);
const OWN_SELECTION_QUIET: Duration = Duration::from_millis(100);
/// How long a new clipboard session waits for the portal to announce the
/// current selection owner before treating the clipboard as empty.
const OWNER_ANNOUNCE_WAIT: Duration = Duration::from_millis(300);

pub struct PortalClipboard;

impl PortalClipboard {
  pub fn open(app_id: Option<&ashpd::AppID>) -> DriverResult<ClipboardSession> {
    ClipboardSession::open(app_id)
  }
}

/// What the latest `SelectionOwnerChanged` said about the selection.
#[derive(Clone, Debug)]
struct AnnouncedOwner {
  mime_types: Vec<String>,
  session_is_owner: bool,
}

pub struct ClipboardSession {
  clipboard: Arc<Clipboard>,
  session: Arc<Session<RemoteDesktop>>,
  text: Arc<Mutex<String>>,
  owns_selection: bool,
  /// The latest `SelectionOwnerChanged`, or `None` before the portal
  /// announced an owner.
  owner: Arc<Mutex<Option<AnnouncedOwner>>>,
  /// Selection reads this session served, and when the latest one finished.
  served: Arc<Mutex<(u64, Option<std::time::Instant>)>>,
  opened_at: std::time::Instant,
  running: Arc<AtomicBool>,
  transfer_thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for ClipboardSession {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter.debug_struct("ClipboardSession").field("session", &self.session).finish_non_exhaustive()
  }
}

impl ClipboardSession {
  fn open(app_id: Option<&ashpd::AppID>) -> DriverResult<Self> {
    let connection = session_connection(app_id)?;
    let remote_desktop = run("open clipboard RemoteDesktop", RemoteDesktop::with_connection(connection.clone()))?;
    let clipboard = Arc::new(run("open Clipboard", Clipboard::with_connection(connection.clone()))?);
    let session = Arc::new(run("create clipboard session", remote_desktop.create_session(Default::default()))?);
    let mut owner = Self {
      clipboard,
      session,
      text: Arc::new(Mutex::new(String::new())),
      owns_selection: false,
      owner: Arc::new(Mutex::new(None)),
      served: Arc::new(Mutex::new((0, None))),
      opened_at: std::time::Instant::now(),
      running: Arc::new(AtomicBool::new(true)),
      transfer_thread: None,
    };
    // Subscribe before Start: mutter announces the current owner once, right
    // after the portal enables the clipboard, and a later subscription misses it.
    owner.transfer_thread = Some(spawn_transfer_thread(
      connection,
      Arc::clone(&owner.clipboard),
      Arc::clone(&owner.text),
      Arc::clone(&owner.owner),
      Arc::clone(&owner.served),
      Arc::clone(&owner.running),
    )?);
    run("request clipboard access", owner.clipboard.request(&owner.session, Default::default()))?;
    // TODO(linux-portal-clipboard-devices): explicit
    // `RemoteDesktop.SelectDevices(types=0)` was tested on GNOME Wayland but
    // did not return a portal response. Keep clipboard-only startup on
    // RequestClipboard+Start until an owner-approved input/libei slice defines
    // device selection policy.
    let selected = run_with_consent("start clipboard session", async {
      remote_desktop.start(&owner.session, None, Default::default()).await?.response()
    })?;
    if !selected.is_clipboard_enabled() {
      return Err(backend("remote desktop portal started without clipboard access"));
    }
    owner.opened_at = std::time::Instant::now();
    Ok(owner)
  }

  pub fn snapshot(&mut self) -> DriverResult<String> {
    match self.announced_owner() {
      Some(owner) if owner.session_is_owner => return Ok(self.text.lock().expect("clipboard owner text lock poisoned").clone()),
      Some(owner) if !owner.mime_types.iter().any(|mime_type| mime_type == TEXT_MIME) => return Ok(String::new()),
      Some(_) => {}
      None if self.owns_selection => return Ok(self.text.lock().expect("clipboard owner text lock poisoned").clone()),
      // TODO(linux-portal-clipboard-announce): a portal backend that never
      // announces the current owner after Start makes every snapshot empty, so
      // the restore clears the user's clipboard. Verified only on GNOME 48,
      // which announces; revisit with evidence from KDE or wlroots portals.
      None => return Ok(String::new()),
    }
    let fd = match run("read portal clipboard", self.clipboard.selection_read(&self.session, TEXT_MIME)) {
      Ok(fd) => fd,
      Err(error) => {
        let message = error.to_string();
        if message.contains("NoSelection") || message.contains("No such selection") || message.contains("Failed to selection read") {
          return Ok(String::new());
        }
        return Err(backend(format!("failed to read portal clipboard text: {error}")));
      }
    };
    let std_fd = StdOwnedFd::from(fd);
    let file = File::from(std_fd);
    let bytes = read_fd_to_end(file)?;
    String::from_utf8(bytes).map_err(|error| backend(format!("portal clipboard returned non-UTF-8 text: {error}")))
  }

  /// The announced selection owner, waiting briefly after Start for the
  /// portal's first announcement.
  ///
  /// NOTICE: xdg-desktop-portal-gnome 48.0 crashes (SIGSEGV/SIGTRAP) when
  /// `SelectionRead` finds no selection owner, and xdg-desktop-portal then
  /// answers every later `RequestClipboard` with "Invalid state" until it is
  /// restarted. Read only after the portal announced an owner with text;
  /// without an announcement the clipboard counts as empty. Seen on Debian 13
  /// GNOME 48 with xdg-desktop-portal 1.20.3 (`journalctl --user -u
  /// xdg-desktop-portal-gnome`: "No selection owner available" followed by
  /// status=11/SEGV). Remove once supported GNOME releases survive a read
  /// without an owner.
  fn announced_owner(&self) -> Option<AnnouncedOwner> {
    if let Some(remaining) = OWNER_ANNOUNCE_WAIT.checked_sub(self.opened_at.elapsed()) {
      thread::sleep(remaining);
    }
    self.owner.lock().expect("clipboard owner lock poisoned").clone()
  }

  /// Number of selection reads this session has served so far.
  pub fn served_reads(&self) -> u64 {
    self.served.lock().expect("clipboard served lock poisoned").0
  }

  /// Waits for the reads that installing our own selection triggers to pass,
  /// and returns the read count to compare a paste reader against.
  ///
  /// NOTICE: mutter's clipboard manager (`meta-clipboard-manager.c`) reads
  /// every new selection owner's text right away. Counting that read as the
  /// paste reader would restore the clipboard before the focused application
  /// read it.
  pub fn settle_own_selection(&self, since: u64) -> u64 {
    let started = std::time::Instant::now();
    loop {
      let (count, last) = *self.served.lock().expect("clipboard served lock poisoned");
      let settled = count > since && last.is_some_and(|last| last.elapsed() >= OWN_SELECTION_QUIET);
      if settled || (count == since && started.elapsed() >= OWN_SELECTION_READ_WAIT) || started.elapsed() >= PASTE_READ_LIMIT {
        return count;
      }
      thread::sleep(Duration::from_millis(10));
    }
  }

  /// Waits until a paste shortcut's reader has read the selection: for the
  /// first read after `since` to start, then for reads to stop. Returns whether
  /// anything read the selection.
  pub fn wait_for_reader(&self, since: u64) -> bool {
    let started = std::time::Instant::now();
    loop {
      let (count, last) = *self.served.lock().expect("clipboard served lock poisoned");
      let elapsed = started.elapsed();
      if count == since && elapsed >= PASTE_READ_START {
        return false;
      }
      if count > since && (last.is_some_and(|last| last.elapsed() >= PASTE_READ_QUIET) || elapsed >= PASTE_READ_LIMIT) {
        return true;
      }
      thread::sleep(Duration::from_millis(20));
    }
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
fn spawn_transfer_thread(
  connection: zbus::Connection,
  clipboard: Arc<Clipboard>,
  text: Arc<Mutex<String>>,
  owner: Arc<Mutex<Option<AnnouncedOwner>>>,
  served: Arc<Mutex<(u64, Option<std::time::Instant>)>>,
  running: Arc<AtomicBool>,
) -> DriverResult<JoinHandle<()>> {
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
    // NOTICE: ashpd's typed `receive_selection_owner_changed` drops GNOME's
    // signal: xdg-desktop-portal 1.20.3 with -gnome 48.0 sends `mime_types` as
    // a struct `(as)` instead of the specified `as` (seen with `dbus-monitor
    // --session interface=org.freedesktop.portal.Clipboard`), so deserializing
    // it fails and the owner announcement never arrived. Read the raw options.
    // The portal addresses SelectionOwnerChanged to the session's own
    // connection, and this connection holds exactly one clipboard session.
    let owner_changes = match run("subscribe to clipboard owner changes", async {
      let proxy = zbus::Proxy::new(
        &connection,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.Clipboard",
      )
      .await?;
      Ok(proxy.receive_signal("SelectionOwnerChanged").await?)
    }) {
      Ok(owner_changes) => owner_changes,
      Err(error) => {
        let _ = ready_tx.send(Err(error));
        return;
      }
    };
    let mut transfers = std::pin::pin!(transfers);
    let mut owner_changes = std::pin::pin!(owner_changes);
    if ready_tx.send(Ok(())).is_err() {
      return;
    }
    enum Event<T, O> {
      Transfer(Option<T>),
      Owner(Option<O>),
      Idle,
    }
    while thread_running.load(Ordering::SeqCst) {
      let event = future::block_on(future::race(
        async { Event::Transfer(transfers.next().await) },
        future::race(async { Event::Owner(owner_changes.next().await) }, async {
          async_io::Timer::after(Duration::from_millis(100)).await;
          Event::Idle
        }),
      ));
      let transfer = match event {
        Event::Idle => continue,
        Event::Owner(Some(message)) => {
          if let Ok((_, options)) = message.body().deserialize::<(zbus::zvariant::OwnedObjectPath, HashMap<String, OwnedValue>)>() {
            *owner.lock().expect("clipboard owner lock poisoned") = Some(announced_owner_from(&options));
          }
          continue;
        }
        // Both signal streams end together when the connection closes.
        Event::Owner(None) => break,
        Event::Transfer(transfer) => transfer,
      };
      let Some((session, mime_type, serial)) = transfer else {
        break;
      };
      let result = if mime_type == TEXT_MIME {
        let payload = text.lock().expect("clipboard owner text lock poisoned").clone();
        run("open clipboard write fd", clipboard.selection_write(&session, serial))
          .and_then(|fd| write_fd_all(File::from(StdOwnedFd::from(fd)), payload.as_bytes()))
      } else {
        Err(backend("unsupported clipboard MIME type"))
      };
      let _ = run("finish clipboard write", clipboard.selection_write_done(&session, serial, result.is_ok()));
      let mut served = served.lock().expect("clipboard served lock poisoned");
      *served = (served.0 + 1, Some(std::time::Instant::now()));
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

// NOTICE: Portal FDs are not necessarily nonblocking. Mutter's
// `src/backends/meta-clipboard-session.c` returns a blocking SelectionWrite pipe.
// Async registers nonblocking IO; racing the complete transfer also bounds
// continuously partial progress so joining the clipboard worker can finish.
fn read_fd_to_end(file: File) -> DriverResult<Vec<u8>> {
  let mut file = async_io::Async::new(file).map_err(|error| backend(format!("failed to register portal clipboard fd: {error}")))?;
  future::block_on(future::race(
    async {
      let mut bytes = Vec::new();
      file.read_to_end(&mut bytes).await.map_err(|error| backend(format!("failed to read portal clipboard fd: {error}")))?;
      Ok(bytes)
    },
    async {
      async_io::Timer::after(FD_TRANSFER_TIMEOUT).await;
      Err(backend("timed out reading portal clipboard fd"))
    },
  ))
}

fn write_fd_all(file: File, payload: &[u8]) -> DriverResult<()> {
  let mut file = async_io::Async::new(file).map_err(|error| backend(format!("failed to register portal clipboard fd: {error}")))?;
  future::block_on(future::race(
    async { file.write_all(payload).await.map_err(|error| backend(format!("failed to write portal clipboard payload: {error}"))) },
    async {
      async_io::Timer::after(FD_TRANSFER_TIMEOUT).await;
      Err(backend("timed out writing portal clipboard payload"))
    },
  ))
}

/// Reads `SelectionOwnerChanged` options, accepting `mime_types` both as the
/// specified `as` and as the `(as)` struct GNOME sends.
fn announced_owner_from(options: &HashMap<String, OwnedValue>) -> AnnouncedOwner {
  let mime_types = options
    .get("mime_types")
    .and_then(|value| {
      Vec::<String>::try_from(value.try_clone().ok()?)
        .ok()
        .or_else(|| <(Vec<String>,)>::try_from(value.try_clone().ok()?).ok().map(|(mime_types,)| mime_types))
    })
    .unwrap_or_default();
  let session_is_owner = options.get("session_is_owner").and_then(|value| bool::try_from(value).ok()).unwrap_or(false);
  AnnouncedOwner {
    mime_types,
    session_is_owner,
  }
}

#[cfg(test)]
#[path = "clipboard_test.rs"]
mod tests;
