//! Durable file publication for daemon-owned Windows state.
//!
//! Pairing and Device entry metadata replace a synced temporary file with its
//! final name. The daemon runs as the logged-in user, so these files inherit
//! the user-private ACL of the daemon store under the user profile.

use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW};
use windows::core::PCWSTR;

/// Atomically replace `destination` with the closed, synced `source`.
pub(crate) fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
  let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
  let destination: Vec<u16> = destination.as_os_str().encode_wide().chain(Some(0)).collect();
  // NOTICE(windows-durable-replace): std::fs cannot open a directory for
  // fsync on Windows. Publish the synced file with WRITE_THROUGH instead of
  // the Unix rename-plus-directory-fsync sequence. Revisit if a reviewed
  // directory durability primitive becomes available.
  // https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw
  // SAFETY: Both NUL-terminated paths remain live for this synchronous call,
  // and the caller has closed the temporary file after sync_all.
  unsafe { MoveFileExW(PCWSTR(source.as_ptr()), PCWSTR(destination.as_ptr()), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) }
    .map_err(io::Error::other)
}
