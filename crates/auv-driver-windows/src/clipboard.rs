//! Text clipboard snapshot/restore/set via the Win32 clipboard.
//!
//! Mirrors the macOS driver's `ClipboardApi`, which models the clipboard as a
//! single text payload: `snapshot` reads the current text, `restore` writes a
//! previously captured snapshot back, and `set_text` installs new text. Windows
//! exposes the clipboard directly through `user32`/`kernel32`, so these are
//! real reads and writes of `CF_UNICODETEXT` rather than keystroke proxies.
//!
//! Snapshots are intentionally text-only to match the macOS surface. Non-text
//! clipboard formats (bitmaps, files, custom formats) are left untouched on
//! read and replaced on write.
// TODO(windows-clipboard-rich-formats): rich/non-text snapshot+restore is
// deferred until an owner-approved slice needs format-preserving capture; the
// macOS surface is text-only today and this mirror keeps that contract.

use auv_driver::error::DriverResult;

/// Reads the current clipboard text. Returns an empty string when the clipboard
/// holds no Unicode text, mirroring the macOS text-only snapshot.
pub fn snapshot() -> DriverResult<String> {
  native::read_text()
}

/// Writes `snapshot` back to the clipboard as Unicode text.
pub fn restore(snapshot: &str) -> DriverResult<()> {
  native::write_text(snapshot)
}

/// Installs `text` as the clipboard's Unicode text payload.
pub fn set_text(text: &str) -> DriverResult<()> {
  native::write_text(text)
}

#[cfg(target_os = "windows")]
mod native {
  use auv_driver::error::DriverResult;
  use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
  use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard, SetClipboardData,
  };
  use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};

  use crate::error::backend;

  // NOTICE: standard Win32 clipboard format for null-terminated UTF-16 text.
  // Defined locally (value 13) to avoid pulling the heavy `Win32_System_Ole`
  // feature just for the `CF_UNICODETEXT` constant.
  const CF_UNICODETEXT: u32 = 13;

  /// Closes the clipboard when the current operation finishes, even on an early
  /// error return, so a failed read/write never leaves it open for the process.
  struct ClipboardGuard;

  impl Drop for ClipboardGuard {
    fn drop(&mut self) {
      // The clipboard was opened successfully to construct this guard; ignore
      // the close result because there is no useful recovery on drop.
      let _ = unsafe { CloseClipboard() };
    }
  }

  fn open_clipboard() -> DriverResult<ClipboardGuard> {
    unsafe { OpenClipboard(HWND::default()) }.map_err(|error| backend(format!("failed to open clipboard: {error}")))?;
    Ok(ClipboardGuard)
  }

  pub(super) fn read_text() -> DriverResult<String> {
    let _guard = open_clipboard()?;
    if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT) }.is_err() {
      return Ok(String::new());
    }
    let handle = unsafe { GetClipboardData(CF_UNICODETEXT) }.map_err(|error| backend(format!("failed to read clipboard text: {error}")))?;
    if handle.0.is_null() {
      return Ok(String::new());
    }
    let global = HGLOBAL(handle.0);
    let pointer = unsafe { GlobalLock(global) } as *const u16;
    if pointer.is_null() {
      return Err(backend("failed to lock clipboard memory for reading"));
    }
    let text = unsafe { read_wide_string(pointer) };
    // GlobalUnlock returns an error once the lock count reaches zero even on
    // success, so the result is intentionally ignored here.
    let _ = unsafe { GlobalUnlock(global) };
    Ok(text)
  }

  pub(super) fn write_text(text: &str) -> DriverResult<()> {
    let mut units: Vec<u16> = text.encode_utf16().collect();
    units.push(0); // CF_UNICODETEXT must be null-terminated.
    let byte_len = std::mem::size_of_val(units.as_slice());

    let _guard = open_clipboard()?;
    unsafe { EmptyClipboard() }.map_err(|error| backend(format!("failed to clear clipboard: {error}")))?;

    let global =
      unsafe { GlobalAlloc(GMEM_MOVEABLE, byte_len) }.map_err(|error| backend(format!("failed to allocate clipboard memory: {error}")))?;
    let destination = unsafe { GlobalLock(global) } as *mut u16;
    if destination.is_null() {
      let _ = unsafe { GlobalFree(global) };
      return Err(backend("failed to lock clipboard memory for writing"));
    }
    unsafe {
      std::ptr::copy_nonoverlapping(units.as_ptr(), destination, units.len());
    }
    let _ = unsafe { GlobalUnlock(global) };

    // On success the system takes ownership of the global memory; only free it
    // ourselves if SetClipboardData fails to take it.
    if let Err(error) = unsafe { SetClipboardData(CF_UNICODETEXT, HANDLE(global.0)) } {
      let _ = unsafe { GlobalFree(global) };
      return Err(backend(format!("failed to set clipboard text: {error}")));
    }
    Ok(())
  }

  /// Reads a null-terminated UTF-16 string starting at `pointer`. The caller
  /// must guarantee `pointer` references locked, null-terminated clipboard
  /// memory for the duration of the read.
  unsafe fn read_wide_string(pointer: *const u16) -> String {
    let mut length = 0usize;
    while unsafe { *pointer.add(length) } != 0 {
      length += 1;
    }
    let slice = unsafe { std::slice::from_raw_parts(pointer, length) };
    String::from_utf16_lossy(slice)
  }
}

#[cfg(not(target_os = "windows"))]
mod native {
  use auv_driver::error::{DriverError, DriverResult};

  pub(super) fn read_text() -> DriverResult<String> {
    Err(DriverError::unsupported("clipboard.snapshot"))
  }

  pub(super) fn write_text(_text: &str) -> DriverResult<()> {
    Err(DriverError::unsupported("clipboard.set_text"))
  }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
  use super::*;

  // Live smoke test against the real Win32 clipboard. It saves and restores the
  // user's existing clipboard text so the roundtrip leaves the clipboard
  // unchanged.
  #[test]
  fn set_then_snapshot_roundtrips_and_restores_original() {
    let original = snapshot().expect("snapshot original clipboard");

    let sentinel = "auv clipboard roundtrip \u{2713}";
    set_text(sentinel).expect("set clipboard text");
    assert_eq!(snapshot().expect("snapshot sentinel"), sentinel);

    restore(&original).expect("restore original clipboard");
    assert_eq!(snapshot().expect("snapshot restored"), original);
  }
}
