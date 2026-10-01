//! LocalSystem-owned Windows credential enrollment store.
//!
//! Machine-scope DPAPI is encryption at rest, not an account boundary. The
//! protected directory and every blob therefore require a SYSTEM-only DACL.
//! An enrollment is only PENDING after writing; readiness requires a separate
//! locked-session retrieval under the installed LocalSystem identity.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::mem::size_of;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GENERIC_READ, GENERIC_WRITE, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{
  BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom, CRYPT_INTEGER_BLOB, CRYPTPROTECT_LOCAL_MACHINE, CRYPTPROTECT_UI_FORBIDDEN,
  CryptProtectData, CryptUnprotectData,
};
use windows::Win32::Storage::FileSystem::{
  CREATE_NEW, CreateDirectoryW, CreateFileW, DELETE, FILE_ATTRIBUTE_NORMAL, FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS,
  FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_MODE, FILE_SHARE_READ, FILE_SHARE_WRITE, FileDispositionInfo, MOVEFILE_REPLACE_EXISTING,
  MoveFileExW, OPEN_EXISTING, SetFileInformationByHandle,
};
use windows::Win32::System::Memory::LocalSize;
use windows::core::PCWSTR;
use zeroize::{Zeroize, Zeroizing};

use auv_driver_windows::device_session::{ConsoleLockState, ConsoleSession, observe_console};

use super::storage_windows::{Descriptor, program_data_leaf, require_system_host, verify_object, wide};

#[derive(Debug, thiserror::Error)]
pub(super) enum VaultError {
  #[error("the Windows enrollment vault is unavailable")]
  Unavailable,
  #[error("the Windows enrollment vault permissions are invalid")]
  Permissions,
  #[error("the account SID is invalid")]
  InvalidAccount,
  #[error("this account is not enrolled")]
  NotEnrolled,
  #[error("the protected credential cannot be retrieved")]
  RetrievalFailed,
  #[error("the selected account does not have a locked console session")]
  NotLocked,
}

const MAX_BLOB: u64 = 4096;

fn sid_name(sid: &str) -> Result<String, VaultError> {
  if !sid.starts_with("S-1-") || sid.len() > 128 || !sid.bytes().all(|byte| byte.is_ascii_digit() || byte == b'S' || byte == b'-') {
    return Err(VaultError::InvalidAccount);
  }

  Ok(format!("{sid}.dpapi"))
}

fn vault_dir() -> Result<(PathBuf, File), VaultError> {
  require_system_host().map_err(|_| VaultError::Permissions)?;
  // NOTICE(device-entry-windows-vault-leaf): Installed hosts already hold
  // enrollments under this leaf, separate from the policy/audit root.
  let root = program_data_leaf("AUVDeviceEnrollments").map_err(|_| VaultError::Unavailable)?;
  let descriptor = Descriptor::system_only().map_err(|_| VaultError::Unavailable)?;
  let root_wide = wide(root.as_os_str());
  // SAFETY: This only creates the leaf below the OS ProgramData directory,
  // with a protected DACL. An existing leaf is checked below before use.
  match unsafe { CreateDirectoryW(PCWSTR(root_wide.as_ptr()), Some(&descriptor.attributes())) } {
    Ok(()) => {}
    Err(error) if error.code() == ERROR_ALREADY_EXISTS.to_hresult() => {}
    Err(_) => return Err(VaultError::Unavailable),
  }

  let raw = unsafe {
    CreateFileW(
      PCWSTR(root_wide.as_ptr()),
      GENERIC_READ.0,
      FILE_SHARE_READ | FILE_SHARE_WRITE,
      None,
      OPEN_EXISTING,
      FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
      HANDLE::default(),
    )
  }
  .map_err(|_| VaultError::Permissions)?;
  // SAFETY: CreateFileW returned one owned directory handle.
  let directory = unsafe { File::from_raw_handle(raw.0) };
  verify_object(&directory, true).map_err(|_| VaultError::Permissions)?;
  Ok((root, directory))
}

fn open_checked_file(path: &Path, access: u32) -> Result<File, VaultError> {
  let path_wide = wide(path.as_os_str());
  // SAFETY: OPEN_REPARSE_POINT exposes a link itself for handle-based
  // rejection. No sharing keeps the verified object stable during use.
  let raw = unsafe {
    CreateFileW(
      PCWSTR(path_wide.as_ptr()),
      access,
      FILE_SHARE_MODE(0),
      None,
      OPEN_EXISTING,
      FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
      HANDLE::default(),
    )
  }
  .map_err(|_| VaultError::Permissions)?;
  // SAFETY: CreateFileW returned one uniquely owned file handle.
  let file = unsafe { File::from_raw_handle(raw.0) };
  verify_object(&file, false).map_err(|_| VaultError::Permissions)?;
  Ok(file)
}

fn item_path(sid: &str) -> Result<(PathBuf, File), VaultError> {
  let (root, guard) = vault_dir()?;
  Ok((root.join(sid_name(sid)?), guard))
}

fn protect(sid: &str, credential: &str) -> Result<Zeroizing<Vec<u8>>, VaultError> {
  let mut plain = Zeroizing::new(credential.as_bytes().to_vec());
  let input = CRYPT_INTEGER_BLOB {
    cbData: plain.len() as u32,
    pbData: plain.as_mut_ptr(),
  };
  let mut entropy_bytes = sid.as_bytes().to_vec();
  let entropy = CRYPT_INTEGER_BLOB {
    cbData: entropy_bytes.len() as u32,
    pbData: entropy_bytes.as_mut_ptr(),
  };
  let mut output = CRYPT_INTEGER_BLOB::default();
  // SAFETY: Both input buffers stay live; DPAPI allocates output with LocalAlloc.
  let protected = unsafe {
    CryptProtectData(&input, PCWSTR::null(), Some(&entropy), None, None, CRYPTPROTECT_LOCAL_MACHINE | CRYPTPROTECT_UI_FORBIDDEN, &mut output)
  };

  if protected.is_err() {
    // SAFETY: DPAPI may have set an output allocation before returning an
    // error; release it if present. This output is encrypted, not plaintext.
    unsafe { LocalFree(HLOCAL(output.pbData.cast())) };

    return Err(VaultError::Unavailable);
  }

  if output.pbData.is_null() || output.cbData == 0 || output.cbData as u64 > MAX_BLOB {
    unsafe { LocalFree(HLOCAL(output.pbData.cast())) };

    return Err(VaultError::Unavailable);
  }

  // SAFETY: DPAPI initialized output.cbData bytes at output.pbData.
  let encrypted = Zeroizing::new(unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec());
  // SAFETY: Release exactly the allocation returned by DPAPI.
  unsafe { LocalFree(HLOCAL(output.pbData.cast())) };
  entropy_bytes.zeroize();
  Ok(encrypted)
}

/// Persist an OS credential under the stable account SID. The local management
/// service must authorize its real peer SID before calling this function. A
/// successful write is PENDING until `verify_while_locked` succeeds.
// The DeviceLocalService enrollment backend holds the per-SID account lock
// across metadata invalidation, this write, and publication of PENDING.
pub(super) fn enroll(sid: &str, credential: &str) -> Result<(), VaultError> {
  if credential.is_empty() || credential.chars().any(char::is_control) || credential.encode_utf16().count() > 128 {
    return Err(VaultError::RetrievalFailed);
  }

  let (destination, _root_guard) = item_path(sid)?;
  let encrypted = protect(sid, credential)?;
  let mut nonce = [0u8; 16];
  // SAFETY: The system RNG writes only to the live nonce.
  if unsafe { BCryptGenRandom(None, &mut nonce, BCRYPT_USE_SYSTEM_PREFERRED_RNG) }.is_err() {
    return Err(VaultError::Unavailable);
  }

  let mut name = String::new();

  for byte in nonce {
    use std::fmt::Write as _;
    write!(&mut name, "{byte:02x}").map_err(|_| VaultError::Unavailable)?;
  }

  let temporary = destination.with_extension(format!("{name}.tmp"));
  let descriptor = Descriptor::system_only().map_err(|_| VaultError::Unavailable)?;
  let temporary_wide = wide(temporary.as_os_str());
  // SAFETY: Create a new item with its own SYSTEM-only DACL. CREATE_NEW
  // prevents overwriting any preexisting path or following a link.
  let raw = unsafe {
    CreateFileW(
      PCWSTR(temporary_wide.as_ptr()),
      GENERIC_WRITE.0,
      FILE_SHARE_MODE(0),
      Some(&descriptor.attributes()),
      CREATE_NEW,
      FILE_ATTRIBUTE_NORMAL,
      HANDLE::default(),
    )
  }
  .map_err(|_| VaultError::Unavailable)?;
  // SAFETY: File now exclusively owns the successful CreateFileW handle.
  let mut file = unsafe { File::from_raw_handle(raw.0) };
  let written = file.write_all(&encrypted).and_then(|_| file.sync_all());
  drop(file);

  if written.is_err() {
    let _ = fs::remove_file(&temporary);

    return Err(VaultError::Unavailable);
  }

  let destination_wide = wide(destination.as_os_str());
  // SAFETY: Both paths are fixed children of the ACL-verified vault root.
  // Replace is atomic on this local filesystem; the new item carries the
  // SYSTEM-only DACL from its own CreateFileW call.
  let moved = unsafe { MoveFileExW(PCWSTR(temporary_wide.as_ptr()), PCWSTR(destination_wide.as_ptr()), MOVEFILE_REPLACE_EXISTING) };

  if moved.is_err() {
    let _ = fs::remove_file(&temporary);

    return Err(VaultError::Unavailable);
  }

  open_checked_file(&destination, GENERIC_READ.0)?;
  // Write-time readback proves the service identity can decrypt now. Policy
  // must still wait for verify_while_locked before calling enrollment READY.
  retrieve(sid).map(|_| ())
}

/// Delete one account's protected credential after local peer authorization.
pub(super) fn remove(sid: &str) -> Result<(), VaultError> {
  let (path, _root_guard) = item_path(sid)?;

  if !path.exists() {
    return Err(VaultError::NotEnrolled);
  }

  let file = open_checked_file(&path, GENERIC_READ.0 | DELETE.0)?;
  let disposition = FILE_DISPOSITION_INFO {
    DeleteFile: true.into(),
  };
  // SAFETY: The open, exclusively held handle is the exact checked object.
  // Delete-on-close avoids checking one path and deleting its replacement.
  unsafe {
    SetFileInformationByHandle(
      HANDLE(file.as_raw_handle()),
      FileDispositionInfo,
      (&raw const disposition).cast(),
      size_of::<FILE_DISPOSITION_INFO>() as u32,
    )
  }
  .map_err(|_| VaultError::Unavailable)
}

/// Host-internal retrieval. Never expose this through DeviceService, CLI,
/// tracing, audit, or a remote request/response.
pub(super) fn retrieve(sid: &str) -> Result<Zeroizing<String>, VaultError> {
  let (path, _root_guard) = item_path(sid)?;

  if !path.exists() {
    return Err(VaultError::NotEnrolled);
  }

  let mut file = open_checked_file(&path, GENERIC_READ.0)?;
  let metadata = file.metadata().map_err(|_| VaultError::RetrievalFailed)?;

  if metadata.len() == 0 || metadata.len() > MAX_BLOB {
    return Err(VaultError::RetrievalFailed);
  }

  let mut encrypted = Zeroizing::new(vec![0u8; metadata.len() as usize]);
  file.read_exact(&mut encrypted).map_err(|_| VaultError::RetrievalFailed)?;
  let input = CRYPT_INTEGER_BLOB {
    cbData: encrypted.len() as u32,
    pbData: encrypted.as_mut_ptr(),
  };
  let mut entropy_bytes = sid.as_bytes().to_vec();
  let entropy = CRYPT_INTEGER_BLOB {
    cbData: entropy_bytes.len() as u32,
    pbData: entropy_bytes.as_mut_ptr(),
  };
  let mut output = CRYPT_INTEGER_BLOB::default();
  // SAFETY: Input and entropy remain live; DPAPI allocates the output.
  let unprotected = unsafe { CryptUnprotectData(&input, None, Some(&entropy), None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut output) };

  if unprotected.is_err() {
    wipe_dpapi_plain(&mut output);

    return Err(VaultError::RetrievalFailed);
  }

  // SAFETY: LocalSize inspects the successful DPAPI LocalAlloc allocation.
  let allocated = if output.pbData.is_null() {
    0
  } else {
    unsafe { LocalSize(HLOCAL(output.pbData.cast())) }
  };

  if output.pbData.is_null() || output.cbData == 0 || output.cbData > 512 || output.cbData as usize > allocated {
    wipe_dpapi_plain(&mut output);

    return Err(VaultError::RetrievalFailed);
  }

  // SAFETY: DPAPI initialized the returned allocation for output.cbData.
  let plain = Zeroizing::new(unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec());
  wipe_dpapi_plain(&mut output);
  entropy_bytes.zeroize();
  let value = std::str::from_utf8(&plain).map_err(|_| VaultError::RetrievalFailed)?.to_owned();
  Ok(Zeroizing::new(value))
}

fn wipe_dpapi_plain(output: &mut CRYPT_INTEGER_BLOB) {
  if output.pbData.is_null() {
    return;
  }

  // SAFETY: DPAPI returns a LocalAlloc allocation. On an error, cbData might
  // not be consistent with the allocation; LocalSize bounds all writes.
  unsafe {
    let allocated = LocalSize(HLOCAL(output.pbData.cast()));

    for index in 0..(output.cbData as usize).min(allocated) {
      output.pbData.add(index).write_volatile(0);
    }

    LocalFree(HLOCAL(output.pbData.cast()));
  }

  output.pbData = std::ptr::null_mut();
  output.cbData = 0;
}

/// Prove a LocalSystem process can decrypt this account's item while its
/// current physical-console session remains locked. No credential is returned.
pub(super) fn verify_while_locked(target: &ConsoleSession) -> Result<(), VaultError> {
  let current = observe_console().map_err(|_| VaultError::NotLocked)?.ok_or(VaultError::NotLocked)?;

  if !target.same_login(&current) || current.lock_state != ConsoleLockState::Locked {
    return Err(VaultError::NotLocked);
  }

  retrieve(&current.account_sid)?;
  let after = observe_console().map_err(|_| VaultError::NotLocked)?.ok_or(VaultError::NotLocked)?;

  if !target.same_login(&after) || after.lock_state != ConsoleLockState::Locked {
    return Err(VaultError::NotLocked);
  }

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use windows::Win32::Security::{IsWellKnownSid, WinLocalSystemSid};

  #[test]
  fn vault_read_rejects_user_owned_file() {
    let path = std::env::temp_dir().join(format!("auv-vault-handle-check-{}", std::process::id()));
    fs::write(&path, b"not a protected credential").unwrap();
    let result = open_checked_file(&path, GENERIC_READ.0);
    let denied = matches!(&result, Err(VaultError::Permissions));
    drop(result);
    fs::remove_file(&path).unwrap();

    assert!(denied);
  }

  #[test]
  fn vault_descriptor_sets_system_owner_explicitly() {
    // ROOT CAUSE:
    //
    // A LocalSystem process created the vault directory with Administrators
    // as its default owner when the descriptor specified only a DACL.
    // Before the fix, the first enrollment left an empty, unusable vault.
    // The creation descriptor now names SYSTEM as owner before any PIN write.
    let descriptor = Descriptor::system_only().expect("vault security descriptor");
    let mut owner = windows::Win32::Security::PSID::default();
    let mut defaulted = windows::Win32::Foundation::BOOL::default();
    // SAFETY: The descriptor stays live while Windows returns a borrowed
    // owner SID pointer into it and a Boolean default-owner flag.
    unsafe { windows::Win32::Security::GetSecurityDescriptorOwner(descriptor.0, &mut owner, &mut defaulted) }.expect("descriptor owner");

    assert!(!defaulted.as_bool());
    assert!(unsafe { IsWellKnownSid(owner, WinLocalSystemSid) }.as_bool());
  }
}
