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
//!
//! [`lend_text`] puts text on the clipboard by delayed rendering: a window of
//! this process owns the clipboard and hands the text over only when another
//! program asks for it. That request is the one signal Windows gives that
//! pasted text was taken, so a paste can give the user's clipboard back as soon
//! as the text was read rather than after a guessed delay.

use std::time::{Duration, Instant};

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

/// Who asked for text lent by [`lend_text`] first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClipboardReader {
  /// The process owning the window the reader opened the clipboard with.
  Process(u32),
  /// The reader opened the clipboard without a window, so Windows does not say who it is.
  Unknown,
}

/// The first request for lent text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ClipboardRead {
  pub(crate) reader: ClipboardReader,
  pub(crate) at: Instant,
}

/// Text lent to the clipboard by [`lend_text`]. Dropping the loan closes the window that owns the
/// clipboard; if that window still owns it, the text is left on the clipboard for good.
pub(crate) struct ClipboardLoan {
  inner: native::Loan,
}

impl ClipboardLoan {
  /// Waits up to `timeout` for the first request for the text. Later requests are not reported:
  /// once the text has been handed over, Windows serves it without asking again.
  pub(crate) fn wait_for_read(&self, timeout: Duration) -> Option<ClipboardRead> {
    self.inner.wait_for_read(timeout)
  }
}

/// Replaces the clipboard's contents with `text` by delayed rendering. The text is also marked
/// to stay out of clipboard history, cloud clipboard and clipboard monitors, because it is only
/// passing through. The caller restores the user's clipboard afterwards; if this fails partway,
/// the clipboard may already be empty.
pub(crate) fn lend_text(text: &str) -> DriverResult<ClipboardLoan> {
  native::lend_text(text).map(|inner| ClipboardLoan { inner })
}

#[cfg(target_os = "windows")]
mod native {
  use std::cell::RefCell;
  use std::ffi::c_void;
  use std::sync::mpsc;
  use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};
  use std::thread::JoinHandle;
  use std::time::{Duration, Instant};

  use auv_driver_common::error::DriverResult;
  use windows::Win32::Foundation::{
    GetLastError, GlobalFree, HANDLE, HGLOBAL, HINSTANCE, HWND, LPARAM, LRESULT, SetLastError, WIN32_ERROR, WPARAM,
  };
  use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardOwner, GetOpenClipboardWindow,
    IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
  };
  use windows::Win32::System::LibraryLoader::GetModuleHandleW;
  use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock};
  use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, GetWindowThreadProcessId, HWND_MESSAGE, MSG,
    PostMessageW, PostQuitMessage, RegisterClassExW, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_DESTROY,
    WM_RENDERALLFORMATS, WM_RENDERFORMAT, WNDCLASSEXW,
  };
  use windows::core::{PCWSTR, w};

  use super::{ClipboardFormatEntry, ClipboardRead, ClipboardReader};
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

  /// Opens the clipboard for `owner`, which becomes its owner on `EmptyClipboard`; a null `owner`
  /// leaves the clipboard without one.
  fn open_clipboard(owner: HWND) -> DriverResult<ClipboardGuard> {
    for _ in 0..10 {
      if unsafe { OpenClipboard(owner) }.is_ok() {
        return Ok(ClipboardGuard);
      }
      std::thread::sleep(std::time::Duration::from_millis(10));
    }
    unsafe { OpenClipboard(owner) }.map_err(|error| backend(format!("failed to open clipboard: {error}")))?;
    Ok(ClipboardGuard)
  }

  pub(super) fn read_text() -> DriverResult<String> {
    let _guard = open_clipboard(HWND::default())?;
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

    let _guard = open_clipboard(HWND::default())?;
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
    let _guard = open_clipboard(HWND::default())?;
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
    let _guard = open_clipboard(HWND::default())?;
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

  /// What the owner window of a loan shares with the thread that lent the text.
  struct Lent {
    /// The text as null-terminated UTF-16, the layout `CF_UNICODETEXT` takes.
    units: Vec<u16>,
    read: Mutex<Option<ClipboardRead>>,
    read_changed: Condvar,
  }

  thread_local! {
    /// The loan served by the owner window on this thread. Window messages are handled on the
    /// thread that created the window, so the window procedure finds its loan here.
    static LENT: RefCell<Option<Arc<Lent>>> = const { RefCell::new(None) };
  }

  pub(super) struct Loan {
    /// The owner window, as an integer because `HWND` is not `Send`.
    window: isize,
    thread: Option<JoinHandle<()>>,
    lent: Arc<Lent>,
  }

  impl Loan {
    pub(super) fn wait_for_read(&self, timeout: Duration) -> Option<ClipboardRead> {
      let read = self.lent.read.lock().unwrap_or_else(PoisonError::into_inner);
      let (read, _) =
        self.lent.read_changed.wait_timeout_while(read, timeout, |read| read.is_none()).unwrap_or_else(PoisonError::into_inner);
      *read
    }
  }

  impl Drop for Loan {
    fn drop(&mut self) {
      // Closing the window ends its thread. A window that still owns the clipboard is first asked
      // to render what it promised (`WM_RENDERALLFORMATS`), so the text stays on the clipboard.
      let _ = unsafe { PostMessageW(HWND(self.window as *mut c_void), WM_CLOSE, WPARAM(0), LPARAM(0)) };
      if let Some(thread) = self.thread.take() {
        let _ = thread.join();
      }
    }
  }

  pub(super) fn lend_text(text: &str) -> DriverResult<Loan> {
    let mut units: Vec<u16> = text.encode_utf16().collect();
    units.push(0);
    let lent = Arc::new(Lent {
      units,
      read: Mutex::new(None),
      read_changed: Condvar::new(),
    });
    let (ready, owned) = mpsc::channel();
    let owner_lent = lent.clone();
    let thread = std::thread::Builder::new()
      .name("auv-clipboard-owner".into())
      .spawn(move || own_clipboard(owner_lent, ready))
      .map_err(|error| backend(format!("failed to start the clipboard owner thread: {error}")))?;
    match owned.recv() {
      Ok(Ok(window)) => Ok(Loan {
        window,
        thread: Some(thread),
        lent,
      }),
      Ok(Err(error)) => {
        let _ = thread.join();
        Err(error)
      }
      Err(_) => {
        let _ = thread.join();
        Err(backend("the clipboard owner thread ended before it took the clipboard"))
      }
    }
  }

  /// The owner thread: creates a message-only window, makes it the clipboard owner with the text
  /// promised, and serves requests for it until the loan closes the window.
  fn own_clipboard(lent: Arc<Lent>, ready: mpsc::Sender<DriverResult<isize>>) {
    LENT.with(|slot| *slot.borrow_mut() = Some(lent));
    let window = match owner_window() {
      Ok(window) => window,
      Err(error) => {
        let _ = ready.send(Err(error));
        return;
      }
    };
    if let Err(error) = promise_text(window) {
      let _ = ready.send(Err(error));
      let _ = unsafe { DestroyWindow(window) };
      return;
    }
    let _ = ready.send(Ok(window.0 as isize));
    let mut message = MSG::default();
    // GetMessageW returns -1 on failure, which `as_bool` would read as a message.
    while unsafe { GetMessageW(&mut message, None, 0, 0) }.0 > 0 {
      unsafe {
        let _ = TranslateMessage(&message);
        DispatchMessageW(&message);
      }
    }
  }

  fn owner_window() -> DriverResult<HWND> {
    const CLASS: PCWSTR = w!("AuvClipboardOwner");
    static REGISTERED: OnceLock<bool> = OnceLock::new();
    let instance: HINSTANCE =
      unsafe { GetModuleHandleW(None) }.map_err(|error| backend(format!("failed to read the module handle: {error}")))?.into();
    let registered = *REGISTERED.get_or_init(|| {
      let class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(owner_proc),
        hInstance: instance,
        lpszClassName: CLASS,
        ..Default::default()
      };
      let atom = unsafe { RegisterClassExW(&class) };
      atom != 0
    });
    if !registered {
      return Err(backend("failed to register the clipboard owner window class"));
    }
    // SAFETY: a registered class and static strings; the window is message-only (never shown)
    // and is created on the thread that runs its message loop.
    unsafe { CreateWindowExW(WINDOW_EX_STYLE(0), CLASS, w!(""), WINDOW_STYLE(0), 0, 0, 0, 0, HWND_MESSAGE, None, instance, None) }
      .map_err(|error| backend(format!("failed to create the clipboard owner window: {error}")))
  }

  /// Empties the clipboard into `window`'s ownership and promises the text without handing it
  /// over, plus the formats that keep it out of clipboard history, cloud clipboard and monitors.
  // NOTICE: the three registered formats are documented in "Clipboard Formats", "Cloud Clipboard
  // and Clipboard History Formats" (https://learn.microsoft.com/windows/win32/dataxchg/clipboard-formats).
  // Only their presence matters for the first; the other two take a DWORD, 0 to opt out.
  fn promise_text(window: HWND) -> DriverResult<()> {
    let _guard = open_clipboard(window)?;
    unsafe { EmptyClipboard() }.map_err(|error| backend(format!("failed to clear clipboard: {error}")))?;
    // A null handle promises the data (delayed rendering). windows-rs reports the null it
    // returns as an error whose code is the cleared last error.
    unsafe { SetLastError(WIN32_ERROR(0)) };
    if let Err(error) = unsafe { SetClipboardData(CF_UNICODETEXT, HANDLE(std::ptr::null_mut())) }
      && error.code().0 != 0
    {
      return Err(backend(format!("failed to promise clipboard text: {error}")));
    }
    for name in [
      w!("ExcludeClipboardContentFromMonitorProcessing"),
      w!("CanIncludeInClipboardHistory"),
      w!("CanUploadToCloudClipboard"),
    ] {
      let format = unsafe { RegisterClipboardFormatW(name) };
      if format == 0 {
        return Err(backend(format!("failed to register a clipboard format: {}", unsafe { GetLastError() }.0)));
      }
      write_format_bytes(format, &0u32.to_ne_bytes())?;
    }
    Ok(())
  }

  unsafe extern "system" fn owner_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
      // A reader asked for the text. It has the clipboard open and waits in GetClipboardData, so
      // the text is set without opening the clipboard.
      WM_RENDERFORMAT if wparam.0 as u32 == CF_UNICODETEXT => {
        LENT.with(|slot| {
          if let Some(lent) = slot.borrow().as_ref() {
            let _ = render_text(lent);
            note_read(lent);
          }
        });
        LRESULT(0)
      }
      // The window is closing while it still owns the clipboard: hand the text over for good.
      // NOTICE: the open is retried. A clipboard listener can hold the clipboard open right
      // then; on 2026-10-12 VirtualBox's shared clipboard (`VBoxHeadless.exe`) did, on every
      // change, and a single OpenClipboard failed with access denied.
      WM_RENDERALLFORMATS => {
        if let Ok(_guard) = open_clipboard(window)
          && unsafe { GetClipboardOwner() }.is_ok_and(|owner| owner == window)
        {
          LENT.with(|slot| {
            if let Some(lent) = slot.borrow().as_ref() {
              let _ = render_text(lent);
            }
          });
        }
        LRESULT(0)
      }
      WM_DESTROY => {
        unsafe { PostQuitMessage(0) };
        LRESULT(0)
      }
      _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
  }

  fn render_text(lent: &Lent) -> DriverResult<()> {
    // SAFETY: a byte view of the live, initialized UTF-16 buffer.
    let bytes = unsafe { std::slice::from_raw_parts(lent.units.as_ptr().cast::<u8>(), std::mem::size_of_val(lent.units.as_slice())) };
    write_format_bytes(CF_UNICODETEXT, bytes)
  }

  /// Records the first request: the process of the window the reader opened the clipboard with.
  fn note_read(lent: &Lent) {
    let mut read = lent.read.lock().unwrap_or_else(PoisonError::into_inner);
    if read.is_some() {
      return;
    }
    // windows-rs reports a reader that opened the clipboard without a window as an error.
    let mut process = 0u32;
    if let Ok(reader) = unsafe { GetOpenClipboardWindow() } {
      unsafe { GetWindowThreadProcessId(reader, Some(&mut process)) };
    }
    *read = Some(ClipboardRead {
      reader: if process == 0 {
        ClipboardReader::Unknown
      } else {
        ClipboardReader::Process(process)
      },
      at: Instant::now(),
    });
    lent.read_changed.notify_all();
  }
}

#[cfg(not(target_os = "windows"))]
mod native {
  use std::time::Duration;

  use auv_driver_common::error::{DriverError, DriverResult};

  use super::{ClipboardFormatEntry, ClipboardRead};

  pub(super) struct Loan;

  impl Loan {
    pub(super) fn wait_for_read(&self, _timeout: Duration) -> Option<ClipboardRead> {
      None
    }
  }

  pub(super) fn lend_text(_text: &str) -> DriverResult<Loan> {
    Err(DriverError::unsupported("clipboard.lend_text"))
  }

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
