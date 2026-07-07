//! Text clipboard snapshot/restore/set through the XDG desktop portal.
//!
//! Clipboard access is attached to a RemoteDesktop portal session. The public
//! functions keep the same text-only contract as the other desktop drivers,
//! while the portal session lifecycle is owned under `native::portal`.

use std::sync::{Arc, Mutex};

use auv_driver::error::DriverResult;

use crate::driver::LinuxDriverSessionState;
use crate::native::portal::{ClipboardSession, PortalClipboard};

/// Reads the current clipboard text. Returns an empty string when the active
/// clipboard owner has no `text/plain;charset=utf-8` payload.
pub fn snapshot(state: &Arc<Mutex<LinuxDriverSessionState>>) -> DriverResult<String> {
  with_clipboard_session(state, |session| session.snapshot())
}

/// Writes `snapshot` back to the clipboard as UTF-8 text.
pub fn restore(state: &Arc<Mutex<LinuxDriverSessionState>>, snapshot: &str) -> DriverResult<()> {
  write_text(state, snapshot)
}

/// Installs `text` as the clipboard's UTF-8 text payload.
pub fn set_text(state: &Arc<Mutex<LinuxDriverSessionState>>, text: &str) -> DriverResult<()> {
  write_text(state, text)
}

fn write_text(state: &Arc<Mutex<LinuxDriverSessionState>>, text: &str) -> DriverResult<()> {
  with_clipboard_session(state, |session| session.set_text(text))
}

fn with_clipboard_session<T>(
  state: &Arc<Mutex<LinuxDriverSessionState>>,
  operation: impl FnOnce(&mut ClipboardSession) -> DriverResult<T>,
) -> DriverResult<T> {
  let mut state = state.lock().expect("linux driver session state poisoned");
  if state.clipboard_session.is_none() {
    state.clipboard_session = Some(PortalClipboard::open()?);
  }
  operation(state.clipboard_session.as_mut().expect("clipboard session was just initialized"))
}
