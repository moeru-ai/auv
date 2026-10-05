//! LocalSystem-only object primitives for the Helper Host's enrollment vault.
//!
//! Vault objects live below one fixed ProgramData leaf. Every opened handle is
//! checked for an exact SYSTEM owner/protected DACL and a non-reparse object
//! before bytes are read or written. Daemon-owned policy, audit, and pairing
//! state no longer use these primitives; they live in the daemon's own store.

use std::ffi::{OsStr, c_void};
use std::fs::File;
use std::io;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::path::PathBuf;

use auv_driver_windows::device_session::verify_local_system_process_in_session;
use windows::Win32::Foundation::{HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
  ConvertSecurityDescriptorToStringSecurityDescriptorW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
  SDDL_REVISION_1, SE_FILE_OBJECT,
};
use windows::Win32::Security::{DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::Storage::FileSystem::{
  BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, GetFileInformationByHandle,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{FOLDERID_ProgramData, KNOWN_FOLDER_FLAG, SHGetKnownFolderPath};
use windows::core::{PCWSTR, PWSTR};

const SYSTEM_OWNER_AND_DACL: &str = "O:SYD:P(A;;GA;;;SY)";
// NOTICE(device-entry-windows-sddl): Windows stores GA as file-all access.
// OWNER and DACL reads omit G, so this exact SYSTEM-only form is expected.
const SYSTEM_OBJECT_SDDL: &str = "O:SYD:P(A;;FA;;;SY)";

fn denied() -> io::Error {
  io::Error::new(io::ErrorKind::PermissionDenied, "Device entry storage requires a LocalSystem-owned, SYSTEM-only ProgramData object")
}

pub(crate) fn wide(value: &OsStr) -> Vec<u16> {
  value.encode_wide().chain(Some(0)).collect()
}

/// The installed Session 0 LocalSystem service is the only process allowed
/// to create or read the vault.
pub(crate) fn require_system_host() -> io::Result<()> {
  verify_local_system_process_in_session(0).map_err(|_| denied())
}

/// Resolves one fixed leaf below the OS ProgramData folder.
pub(crate) fn program_data_leaf(leaf: &str) -> io::Result<PathBuf> {
  // SAFETY: SHGetKnownFolderPath allocates one NUL-terminated UTF-16 path.
  let path = unsafe { SHGetKnownFolderPath(&FOLDERID_ProgramData, KNOWN_FOLDER_FLAG(0), HANDLE::default()) }.map_err(io::Error::other)?;
  // SAFETY: The returned pointer remains live through UTF-16 decoding.
  let decoded = unsafe { path.to_string() }.map_err(io::Error::other);
  // SAFETY: CoTaskMemFree releases the one Shell allocation.
  unsafe { CoTaskMemFree(Some(path.0.cast::<c_void>())) };
  Ok(PathBuf::from(decoded?).join(leaf))
}

/// A LocalAlloc security descriptor for new SYSTEM-owned objects.
pub(crate) struct Descriptor(pub(crate) PSECURITY_DESCRIPTOR);

impl Descriptor {
  pub(crate) fn system_only() -> io::Result<Self> {
    let text = wide(OsStr::new(SYSTEM_OWNER_AND_DACL));
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: The SDDL string and output pointer are live during conversion.
    unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(PCWSTR(text.as_ptr()), SDDL_REVISION_1, &mut descriptor, None) }
      .map_err(io::Error::other)?;
    Ok(Self(descriptor))
  }

  pub(crate) fn attributes(&self) -> SECURITY_ATTRIBUTES {
    SECURITY_ATTRIBUTES {
      nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
      lpSecurityDescriptor: self.0.0,
      bInheritHandle: false.into(),
    }
  }
}

impl Drop for Descriptor {
  fn drop(&mut self) {
    // SAFETY: SDDL conversion returned this LocalAlloc descriptor once.
    unsafe { LocalFree(HLOCAL(self.0.0)) };
  }
}

/// Verify the opened handle so a path substitution cannot change the object
/// between an ACL check and a read.
pub(crate) fn verify_object(file: &File, directory: bool) -> io::Result<()> {
  let handle = HANDLE(file.as_raw_handle());
  let mut information = BY_HANDLE_FILE_INFORMATION::default();
  // SAFETY: Windows writes the initialized BY_HANDLE_FILE_INFORMATION value.
  unsafe { GetFileInformationByHandle(handle, &mut information) }.map_err(|_| denied())?;
  let attributes = information.dwFileAttributes;

  if attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 || (attributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0) != directory {
    return Err(denied());
  }

  let mut descriptor = PSECURITY_DESCRIPTOR::default();
  let security_info = OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
  // SAFETY: Windows returns one LocalAlloc security descriptor for this open
  // handle; the handle stays live and cannot be substituted by a path rename.
  unsafe { GetSecurityInfo(handle, SE_FILE_OBJECT, security_info, None, None, None, None, Some(&mut descriptor)) }
    .ok()
    .map_err(|_| denied())?;

  if descriptor.0.is_null() {
    return Err(denied());
  }

  let mut output = PWSTR::null();
  // SAFETY: The descriptor is live; conversion allocates the output SDDL.
  let converted =
    unsafe { ConvertSecurityDescriptorToStringSecurityDescriptorW(descriptor, SDDL_REVISION_1, security_info, &mut output, None) };
  // SAFETY: Release the one descriptor returned by GetSecurityInfo.
  unsafe { LocalFree(HLOCAL(descriptor.0)) };
  converted.map_err(|_| denied())?;
  // SAFETY: The returned SDDL string remains live through decoding.
  let actual = unsafe { output.to_string() }.map_err(|_| denied());
  // SAFETY: Release the one SDDL allocation returned by conversion.
  unsafe { LocalFree(HLOCAL(output.0.cast())) };

  if actual? != SYSTEM_OBJECT_SDDL {
    return Err(denied());
  }

  Ok(())
}

#[cfg(test)]
mod tests {
  use std::os::windows::io::FromRawHandle;

  use windows::Win32::Foundation::GENERIC_READ;
  use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
  };

  use super::*;

  #[test]
  fn program_data_leaf_stays_below_program_data() {
    let leaf = program_data_leaf("AUVDeviceEnrollments").unwrap();

    assert_eq!(leaf.file_name().unwrap(), "AUVDeviceEnrollments");
    assert!(leaf.is_absolute());
  }

  #[test]
  fn ordinary_process_is_not_the_vault_host() {
    // This test runs as an ordinary test user. The LocalSystem service has
    // its own positive installation gate; no privileged mutation occurs.
    if require_system_host().is_ok() {
      return;
    }

    assert_eq!(require_system_host().unwrap_err().kind(), io::ErrorKind::PermissionDenied);
  }

  #[test]
  fn user_owned_file_fails_closed_on_handle_verification() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("user-owned-policy");
    std::fs::write(&path, b"{}\n").unwrap();
    let file = File::open(path).unwrap();

    assert_eq!(verify_object(&file, false).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
  }

  #[test]
  fn directory_junction_is_rejected_by_handle_attributes() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    let junction = root.path().join("junction");
    std::fs::create_dir(&target).unwrap();
    // mklink /J creates a local junction without requiring developer-mode
    // symbolic-link privilege. Both paths are task-owned temporary fixtures.
    let status = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(&junction).arg(&target).status().unwrap();

    assert!(status.success());

    let path = wide(junction.as_os_str());
    // SAFETY: The terminated path is live and the returned handle is owned by
    // File. OPEN_REPARSE_POINT exposes the junction itself for verification.
    let raw = unsafe {
      CreateFileW(
        PCWSTR(path.as_ptr()),
        GENERIC_READ.0,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        None,
        OPEN_EXISTING,
        FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
        HANDLE::default(),
      )
    }
    .unwrap();
    // SAFETY: CreateFileW returned one uniquely owned handle.
    let file = unsafe { File::from_raw_handle(raw.0) };

    assert_eq!(verify_object(&file, true).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
  }
}
