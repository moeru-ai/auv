//! Text and rich clipboard snapshot/restore/set via the Win32 clipboard.
//!
//! Mirrors the macOS driver's `ClipboardApi`, which models the clipboard as a
//! single text payload: `snapshot` reads the current text, `restore` writes a
//! previously captured snapshot back, and `set_text` installs new text. Windows
//! exposes the clipboard directly through `user32`/`kernel32`, so these are
//! real reads and writes of `CF_UNICODETEXT` rather than keystroke proxies.
//!
//! `snapshot`/`restore`/`set_text` stay text-only to match the macOS surface.
//! [`snapshot_rich`]/[`restore_rich`] are a Windows-only addition that capture
//! and restore every memory-backed clipboard format present (files, images,
//! HTML/RTF, and other registered formats), not just `CF_UNICODETEXT`. GDI
//! object-backed formats (`CF_BITMAP`, `CF_METAFILEPICT`, `CF_PALETTE`,
//! `CF_ENHMETAFILE`) are skipped: duplicating those handles needs GDI object
//! duplication (e.g. `CopyImage`), not a raw memory copy, and is deferred
//! until an owner-approved slice needs it. [`snapshot_rich`] enforces a
//! per-format and total byte cap (see `MAX_FORMAT_BYTES`/
//! `MAX_TOTAL_SNAPSHOT_BYTES` in `native`) so a huge payload fails with a
//! clear error instead of risking extreme memory use.

use auv_driver_common::error::DriverResult;

/// A captured clipboard payload for every memory-backed format present at
/// snapshot time. Opaque: use [`snapshot_rich`] to create one and
/// [`restore_rich`] to replay it.
pub struct ClipboardSnapshot {
  entries: Vec<ClipboardFormatEntry>,
}

struct ClipboardFormatEntry {
  format: u32,
  bytes: Vec<u8>,
}

impl std::fmt::Debug for ClipboardSnapshot {
  // Lists captured format ids and byte lengths only; clipboard content may be
  // sensitive and should not be dumped into logs.
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter.debug_list().entries(self.entries.iter().map(|entry| (entry.format, entry.bytes.len()))).finish()
  }
}

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

/// Captures every memory-backed clipboard format currently present, for exact
/// format-preserving restore. See the module docs for skipped GDI formats.
pub fn snapshot_rich() -> DriverResult<ClipboardSnapshot> {
  native::snapshot_all_formats()
}

/// Replaces the current clipboard contents with exactly the formats captured
/// by [`snapshot_rich`].
pub fn restore_rich(snapshot: &ClipboardSnapshot) -> DriverResult<()> {
  native::restore_all_formats(&snapshot.entries)
}

#[cfg(target_os = "windows")]
mod native {
  use auv_driver_common::error::DriverResult;
  use windows::Win32::Foundation::{GetLastError, GlobalFree, HANDLE, HGLOBAL, HWND, SetLastError, WIN32_ERROR};
  use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard, SetClipboardData,
  };
  use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock};

  use super::ClipboardFormatEntry;
  use crate::error::backend;

  // NOTICE: standard Win32 clipboard format for null-terminated UTF-16 text.
  // Defined locally (value 13) to avoid pulling the heavy `Win32_System_Ole`
  // feature just for the `CF_UNICODETEXT` constant.
  const CF_UNICODETEXT: u32 = 13;

  // Standard Win32 clipboard formats backed by a GDI object handle rather than
  // global memory. `GlobalLock`/`GlobalSize` are only valid on HGLOBAL handles,
  // so these are skipped by the rich snapshot/restore path (see module docs).
  const CF_BITMAP: u32 = 2;
  const CF_METAFILEPICT: u32 = 3;
  const CF_PALETTE: u32 = 9;
  const CF_ENHMETAFILE: u32 = 14;

  // NOTICE: bounds per-format and total rich-snapshot bytes so a huge
  // clipboard payload (e.g. a large uncompressed DIB image, or a custom
  // format) can't drive extreme memory use or an OOM-abort inside `to_vec()`.
  // Generous enough for a multi-megapixel uncompressed screenshot plus a
  // handful of smaller companion formats (HTML/RTF/HDROP), while still
  // failing fast on pathological outliers.
  const MAX_FORMAT_BYTES: usize = 64 * 1024 * 1024;
  const MAX_TOTAL_SNAPSHOT_BYTES: usize = 256 * 1024 * 1024;

  fn is_memory_backed_format(format: u32) -> bool {
    !matches!(format, CF_BITMAP | CF_METAFILEPICT | CF_PALETTE | CF_ENHMETAFILE)
  }

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

  pub(super) fn snapshot_all_formats() -> DriverResult<super::ClipboardSnapshot> {
    let _guard = open_clipboard()?;
    let mut entries = Vec::new();
    let mut total_bytes = 0usize;
    let mut format = 0u32;
    loop {
      // EnumClipboardFormats returns 0 both at the end of the sequence and on
      // error; clear the last-error code first so it can disambiguate the two.
      unsafe { SetLastError(WIN32_ERROR(0)) };
      format = unsafe { EnumClipboardFormats(format) };
      if format == 0 {
        let last_error = unsafe { GetLastError() };
        if last_error.0 != 0 {
          return Err(backend(format!("failed to enumerate clipboard formats (error code {})", last_error.0)));
        }
        break;
      }
      if !is_memory_backed_format(format) {
        continue;
      }
      if let Some(bytes) = read_current_format_bytes(format)? {
        total_bytes = total_bytes.checked_add(bytes.len()).ok_or_else(|| backend("clipboard snapshot total size overflowed"))?;
        if total_bytes > MAX_TOTAL_SNAPSHOT_BYTES {
          return Err(backend(format!(
            "clipboard snapshot exceeds the {MAX_TOTAL_SNAPSHOT_BYTES}-byte total size limit across formats; aborting rich snapshot"
          )));
        }
        entries.push(ClipboardFormatEntry { format, bytes });
      }
    }
    Ok(super::ClipboardSnapshot { entries })
  }

  pub(super) fn restore_all_formats(entries: &[ClipboardFormatEntry]) -> DriverResult<()> {
    let _guard = open_clipboard()?;
    unsafe { EmptyClipboard() }.map_err(|error| backend(format!("failed to clear clipboard: {error}")))?;
    for entry in entries {
      write_format_bytes(entry.format, &entry.bytes)?;
    }
    Ok(())
  }

  /// Reads the raw bytes behind `format`. Must be called while the clipboard is
  /// already open. Returns `Ok(None)` when the format has no data handle, or
  /// when the handle can't be read — some formats (e.g. `CF_LOCALE`) can be
  /// registered via delayed rendering by an owner that no longer provides
  /// data, so an unreadable format is skipped rather than failing the whole
  /// snapshot. Returns `Err` when the format's payload exceeds
  /// `MAX_FORMAT_BYTES`, rather than risking an OOM-abort inside `to_vec()`.
  fn read_current_format_bytes(format: u32) -> DriverResult<Option<Vec<u8>>> {
    let Ok(handle) = (unsafe { GetClipboardData(format) }) else {
      return Ok(None);
    };
    if handle.0.is_null() {
      return Ok(None);
    }
    let global = HGLOBAL(handle.0);
    // Checked before locking/copying so an oversized payload is rejected
    // without ever allocating a matching-size `Vec<u8>`.
    let size = unsafe { GlobalSize(global) };
    if size > MAX_FORMAT_BYTES {
      return Err(backend(format!(
        "clipboard format {format} payload of {size} bytes exceeds the {MAX_FORMAT_BYTES}-byte per-format snapshot limit"
      )));
    }
    let pointer = unsafe { GlobalLock(global) } as *const u8;
    if pointer.is_null() {
      return Ok(None);
    }
    let bytes = unsafe { std::slice::from_raw_parts(pointer, size) }.to_vec();
    // GlobalUnlock returns an error once the lock count reaches zero even on
    // success, so the result is intentionally ignored here.
    let _ = unsafe { GlobalUnlock(global) };
    Ok(Some(bytes))
  }

  /// Installs `bytes` as `format`'s clipboard payload. Must be called while the
  /// clipboard is already open, after `EmptyClipboard` for the first format in
  /// a batch restore.
  fn write_format_bytes(format: u32, bytes: &[u8]) -> DriverResult<()> {
    let global = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) }
      .map_err(|error| backend(format!("failed to allocate memory for format {format}: {error}")))?;
    let destination = unsafe { GlobalLock(global) } as *mut u8;
    if destination.is_null() {
      let _ = unsafe { GlobalFree(global) };
      return Err(backend(format!("failed to lock clipboard memory for format {format}")));
    }
    unsafe {
      std::ptr::copy_nonoverlapping(bytes.as_ptr(), destination, bytes.len());
    }
    let _ = unsafe { GlobalUnlock(global) };

    // On success the system takes ownership of the global memory; only free it
    // ourselves if SetClipboardData fails to take it.
    if let Err(error) = unsafe { SetClipboardData(format, HANDLE(global.0)) } {
      let _ = unsafe { GlobalFree(global) };
      return Err(backend(format!("failed to set clipboard format {format}: {error}")));
    }
    Ok(())
  }
}

#[cfg(not(target_os = "windows"))]
mod native {
  use auv_driver_common::error::{DriverError, DriverResult};

  use super::ClipboardFormatEntry;

  pub(super) fn read_text() -> DriverResult<String> {
    Err(DriverError::unsupported("clipboard.snapshot"))
  }

  pub(super) fn write_text(_text: &str) -> DriverResult<()> {
    Err(DriverError::unsupported("clipboard.set_text"))
  }

  pub(super) fn snapshot_all_formats() -> DriverResult<super::ClipboardSnapshot> {
    Err(DriverError::unsupported("clipboard.snapshot_rich"))
  }

  pub(super) fn restore_all_formats(_entries: &[ClipboardFormatEntry]) -> DriverResult<()> {
    Err(DriverError::unsupported("clipboard.restore_rich"))
  }
}

#[cfg(all(test, target_os = "windows"))]
#[path = "clipboard_test.rs"]
mod tests;
