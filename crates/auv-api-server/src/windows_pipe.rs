//! Owner-only Windows named pipes shared by the daemon API, the
//! DeviceLocalService pipe, and daemon-owned Runner IPC.
//!
//! The DACL names the current process user SID instead of the `OW` (owner
//! rights) alias. An elevated administrator's token makes BUILTIN\Administrators
//! the default owner of new objects, so `OW` would admit every elevated
//! administrator, not only the user who started AUV.

use std::ffi::c_void;
use std::io;
use std::mem::{align_of, size_of};
use std::os::windows::io::AsRawHandle;

use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use windows::Win32::Foundation::{BOOL, CloseHandle, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
  ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
  GetTokenInformation, IsWellKnownSid, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser, WinAnonymousSid,
};
use windows::Win32::System::Pipes::GetNamedPipeClientProcessId;
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::core::{PCWSTR, PWSTR};

/// Creates a pipe instance that only LocalSystem and the current user may
/// open. Network clients are rejected independently of the DACL.
pub fn create_owner_only(path: &str, first: bool) -> io::Result<NamedPipeServer> {
  let sddl = format!("D:P(A;;GA;;;SY)(A;;GA;;;{})", current_user_sid()?);
  let sddl = sddl.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
  let mut descriptor = PSECURITY_DESCRIPTOR::default();
  // SAFETY: The UTF-16 buffer is NUL-terminated and stays live for this call;
  // descriptor is a live output whose allocation the guard frees.
  unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(PCWSTR(sddl.as_ptr()), SDDL_REVISION_1, &mut descriptor, None) }
    .map_err(io::Error::other)?;
  let descriptor = SecurityDescriptor(descriptor);
  let mut attributes = SECURITY_ATTRIBUTES {
    nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
    lpSecurityDescriptor: descriptor.0.0,
    bInheritHandle: BOOL(0),
  };
  let mut options = ServerOptions::new();
  options.first_pipe_instance(first).reject_remote_clients(true);
  // SAFETY: Tokio forwards the pointer to CreateNamedPipeW, which copies the
  // descriptor synchronously; the guard outlives this call.
  unsafe { options.create_with_security_attributes_raw(path, (&raw mut attributes).cast()) }
}

/// Process ID of the client connected to this pipe instance.
pub fn client_process_id(pipe: &NamedPipeServer) -> io::Result<u32> {
  let mut process_id = 0u32;
  // SAFETY: The handle belongs to a live, connected server instance.
  unsafe { GetNamedPipeClientProcessId(HANDLE(pipe.as_raw_handle()), &mut process_id) }.map_err(io::Error::other)?;
  Ok(process_id)
}

fn current_user_sid() -> io::Result<String> {
  let mut raw_token = HANDLE::default();
  // SAFETY: The pseudo-handle is always valid; the output token is owned and
  // closed by the guard.
  unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw_token) }.map_err(io::Error::other)?;
  let token = Token(raw_token);
  token_user_sid(token.0)
}

/// String SID of a token's user. Anonymous identities are rejected.
pub(crate) fn token_user_sid(token: HANDLE) -> io::Result<String> {
  let mut bytes = 0u32;
  // SAFETY: A null output buffer asks Windows for the required TOKEN_USER size.
  let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut bytes) };

  if bytes < size_of::<TOKEN_USER>() as u32 || align_of::<TOKEN_USER>() > align_of::<usize>() {
    return Err(identity_error());
  }

  let mut data = vec![0usize; (bytes as usize).div_ceil(size_of::<usize>())];
  // SAFETY: The word buffer is aligned and large enough for TOKEN_USER and its
  // embedded SID; it stays live through SID conversion below.
  unsafe { GetTokenInformation(token, TokenUser, Some(data.as_mut_ptr().cast()), bytes, &mut bytes) }.map_err(|_| identity_error())?;

  if (bytes as usize) < size_of::<TOKEN_USER>() {
    return Err(identity_error());
  }

  // SAFETY: Windows initialized an aligned TOKEN_USER in `data`.
  let token_user = unsafe { data.as_ptr().cast::<TOKEN_USER>().read() };

  if token_user.User.Sid.0.is_null() {
    return Err(identity_error());
  }

  // SAFETY: The SID remains valid in `data`.
  if unsafe { IsWellKnownSid(token_user.User.Sid, WinAnonymousSid) }.as_bool() {
    return Err(identity_error());
  }

  let mut raw_sid = PWSTR::null();
  // SAFETY: The SID remains live in `data`; Windows returns one LocalAlloc
  // UTF-16 string, which the guard frees after conversion.
  unsafe { ConvertSidToStringSidW(token_user.User.Sid, &mut raw_sid) }.map_err(|_| identity_error())?;

  if raw_sid.is_null() {
    return Err(identity_error());
  }

  let sid = LocalString(raw_sid);
  // SAFETY: ConvertSidToStringSidW returned a NUL-terminated UTF-16 string.
  let value = unsafe { sid.0.to_string() }.map_err(|_| identity_error())?;

  if !value.starts_with("S-1-") {
    return Err(identity_error());
  }

  Ok(value)
}

pub(crate) fn identity_error() -> io::Error {
  io::Error::new(io::ErrorKind::PermissionDenied, "verified named-pipe client SID is required")
}

struct Token(HANDLE);
impl Drop for Token {
  fn drop(&mut self) {
    // SAFETY: OpenProcessToken or OpenThreadToken returned one owned handle.
    let _ = unsafe { CloseHandle(self.0) };
  }
}

struct LocalString(PWSTR);
impl Drop for LocalString {
  fn drop(&mut self) {
    // SAFETY: ConvertSidToStringSidW returned one LocalAlloc allocation.
    let _ = unsafe { LocalFree(HLOCAL(self.0.0.cast::<c_void>())) };
  }
}

struct SecurityDescriptor(PSECURITY_DESCRIPTOR);
impl Drop for SecurityDescriptor {
  fn drop(&mut self) {
    // SAFETY: ConvertStringSecurityDescriptorToSecurityDescriptorW returned
    // one LocalAlloc allocation.
    let _ = unsafe { LocalFree(HLOCAL(self.0.0)) };
  }
}

#[cfg(test)]
mod tests {
  use windows::Win32::Security::Authorization::{ConvertSecurityDescriptorToStringSecurityDescriptorW, GetSecurityInfo, SE_KERNEL_OBJECT};
  use windows::Win32::Security::DACL_SECURITY_INFORMATION;

  use super::*;

  /// Windows abbreviates some SIDs in SDDL (the built-in Administrator
  /// account becomes `LA`), so compare both sides after one round trip.
  fn canonical_dacl(sddl: &str) -> String {
    let sddl = sddl.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: The UTF-16 buffer is NUL-terminated; the guard frees the output.
    unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(PCWSTR(sddl.as_ptr()), SDDL_REVISION_1, &mut descriptor, None) }.unwrap();
    dacl_text(&SecurityDescriptor(descriptor))
  }

  fn dacl_text(descriptor: &SecurityDescriptor) -> String {
    let mut text = PWSTR::null();
    // SAFETY: The descriptor is live; Windows returns one LocalAlloc string.
    unsafe {
      ConvertSecurityDescriptorToStringSecurityDescriptorW(descriptor.0, SDDL_REVISION_1, DACL_SECURITY_INFORMATION, &mut text, None)
    }
    .unwrap();
    let text = LocalString(text);
    // SAFETY: Windows returned a NUL-terminated UTF-16 string.
    unsafe { text.0.to_string() }.unwrap()
  }

  fn dacl_of(pipe: &NamedPipeServer) -> String {
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: The handle is a live pipe instance; Windows allocates the
    // descriptor, which the guard frees.
    let status = unsafe {
      GetSecurityInfo(
        HANDLE(pipe.as_raw_handle()),
        SE_KERNEL_OBJECT,
        DACL_SECURITY_INFORMATION,
        None,
        None,
        None,
        None,
        Some(&mut descriptor),
      )
    };
    assert!(status.is_ok(), "{status:?}");
    dacl_text(&SecurityDescriptor(descriptor))
  }

  // ROOT CAUSE:
  //
  // Runner pipes used the default named-pipe DACL, which grants read access
  // to Everyone and Anonymous. Any local user could connect first, receive the
  // daemon's requests, and stop the Runner from starting. The daemon pipe used
  // `OW`, which an elevated administrator's token maps to every elevated
  // administrator. Owner-only pipes now name exactly SYSTEM and this user.
  #[tokio::test]
  async fn owner_only_pipe_admits_only_system_and_the_current_user() {
    let path = format!(r"\\.\pipe\auv-owner-only-acl-{}", std::process::id());
    let pipe = create_owner_only(&path, true).unwrap();
    let user = current_user_sid().unwrap();
    assert_eq!(dacl_of(&pipe), canonical_dacl(&format!("D:P(A;;FA;;;SY)(A;;FA;;;{user})")));
  }

  #[tokio::test]
  async fn client_process_id_reports_the_connected_process() {
    let path = format!(r"\\.\pipe\auv-owner-only-pid-{}", std::process::id());
    let pipe = create_owner_only(&path, true).unwrap();
    let _client = tokio::net::windows::named_pipe::ClientOptions::new().open(&path).unwrap();
    pipe.connect().await.unwrap();
    assert_eq!(client_process_id(&pipe).unwrap(), std::process::id());
  }
}
