//! Windows DeviceLocalService pipe opening and connected-server identity.
//!
//! This module owns the Win32 handle and token calls. No HTTP/2 bytes reach a
//! pipe until the kernel-reported server process is verified to run as this
//! process's own user, matching the Unix socket's peer-UID check.

use std::io;
use std::mem::{align_of, size_of};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, IntoRawHandle, OwnedHandle};

use tokio::net::windows::named_pipe::NamedPipeClient;
use windows::Win32::Foundation::{ERROR_PIPE_BUSY, FALSE, HANDLE};
use windows::Win32::Security::{EqualSid, GetTokenInformation, PSID, TOKEN_QUERY, TOKEN_USER, TokenUser};
use windows::Win32::Storage::FileSystem::{
  CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_MODE, OPEN_EXISTING, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
};
use windows::Win32::System::Pipes::GetNamedPipeServerProcessId;
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::core::{HRESULT, PCWSTR};

// NOTICE(device-local-pipe-rights): GENERIC_WRITE includes the bit shared by
// FILE_APPEND_DATA and FILE_CREATE_PIPE_INSTANCE. Request only read/write
// data plus READ_CONTROL and SYNCHRONIZE, matching the listener's AU DACL.
// Remove this custom open only if Tokio exposes an exact-access client API.
// https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights
const CLIENT_ACCESS: u32 = 0x0012_0003;

pub(super) async fn open_verified(name: &str) -> io::Result<NamedPipeClient> {
  if !valid_name(name) {
    return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid Device-local pipe name"));
  }

  let path = format!(r"\\.\pipe\{name}");
  let wide = std::ffi::OsStr::new(&path).encode_wide().chain(Some(0)).collect::<Vec<_>>();
  let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);

  loop {
    match open_once(&wide) {
      Ok(client) => return Ok(client),
      Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32) && tokio::time::Instant::now() < deadline => {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
      }
      Err(error) => return Err(error),
    }
  }
}

fn open_once(wide: &[u16]) -> io::Result<NamedPipeClient> {
  // SAFETY: The local pipe path is NUL-terminated and remains live through
  // CreateFileW. The returned handle is immediately put under sole ownership.
  let raw = unsafe {
    CreateFileW(
      PCWSTR(wide.as_ptr()),
      CLIENT_ACCESS,
      FILE_SHARE_MODE(0),
      None,
      OPEN_EXISTING,
      FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
      HANDLE::default(),
    )
  }
  .map_err(|error| {
    if error.code() == HRESULT::from_win32(ERROR_PIPE_BUSY.0) {
      io::Error::from_raw_os_error(ERROR_PIPE_BUSY.0 as i32)
    } else {
      io::Error::other(error)
    }
  })?;
  // SAFETY: CreateFileW returned one owned handle, transferred here.
  let handle = unsafe { OwnedHandle::from_raw_handle(raw.0) };
  verify_server(HANDLE(handle.as_raw_handle()))?;
  // SAFETY: Tokio takes sole ownership of this overlapped pipe handle.
  unsafe { NamedPipeClient::from_raw_handle(handle.into_raw_handle()) }
}

fn valid_name(name: &str) -> bool {
  name.starts_with("auv-device-local-") && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn verify_server(pipe: HANDLE) -> io::Result<()> {
  let mut server_pid = 0u32;
  // SAFETY: The call writes one live u32 for this connected pipe handle.
  unsafe { GetNamedPipeServerProcessId(pipe, &mut server_pid) }.map_err(|_| identity_error())?;

  if server_pid == 0 {
    return Err(identity_error());
  }

  // SAFETY: The kernel supplied this PID for the connected pipe. The process
  // handle pins that process while its primary token is inspected.
  let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, server_pid) }.map_err(|_| identity_error())?;
  // SAFETY: OpenProcess returned one owned process handle.
  let process = unsafe { OwnedHandle::from_raw_handle(process.0) };
  let server = token_user(HANDLE(process.as_raw_handle()))?;
  // SAFETY: GetCurrentProcess returns a pseudo handle that needs no close.
  let current = token_user(unsafe { GetCurrentProcess() })?;

  // SAFETY: Both SIDs point into their live, aligned TOKEN_USER buffers.
  if unsafe { EqualSid(sid(&server), sid(&current)) }.is_err() {
    return Err(identity_error());
  }

  // A server that exits while checked cannot consume credentials. Re-read
  // the pipe association before transferring it into the gRPC transport.
  let mut current_pid = 0u32;
  unsafe { GetNamedPipeServerProcessId(pipe, &mut current_pid) }.map_err(|_| identity_error())?;

  if current_pid != server_pid {
    return Err(identity_error());
  }

  Ok(())
}

/// The aligned TOKEN_USER buffer for one process; its SID points inside it.
fn token_user(process: HANDLE) -> io::Result<Vec<usize>> {
  let mut token = HANDLE::default();
  // SAFETY: The process handle is live; Windows writes one owned token.
  unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }.map_err(|_| identity_error())?;
  // SAFETY: OpenProcessToken returned one owned token handle.
  let token = unsafe { OwnedHandle::from_raw_handle(token.0) };
  let mut bytes = 0u32;
  // SAFETY: A null output buffer requests the required TOKEN_USER size.
  let _ = unsafe { GetTokenInformation(HANDLE(token.as_raw_handle()), TokenUser, None, 0, &mut bytes) };

  if bytes < size_of::<TOKEN_USER>() as u32 || bytes > 64 * 1024 || align_of::<TOKEN_USER>() > align_of::<usize>() {
    return Err(identity_error());
  }

  let mut data = vec![0usize; (bytes as usize).div_ceil(size_of::<usize>())];
  // SAFETY: The word buffer is aligned and large enough for TOKEN_USER and
  // its SID.
  unsafe { GetTokenInformation(HANDLE(token.as_raw_handle()), TokenUser, Some(data.as_mut_ptr().cast()), bytes, &mut bytes) }
    .map_err(|_| identity_error())?;

  if (bytes as usize) < size_of::<TOKEN_USER>() || sid(&data).0.is_null() {
    return Err(identity_error());
  }

  Ok(data)
}

fn sid(token_user: &[usize]) -> PSID {
  // SAFETY: `token_user` came from `token_user` above, which checked that an
  // aligned TOKEN_USER was initialized at its start.
  unsafe { token_user.as_ptr().cast::<TOKEN_USER>().read() }.User.Sid
}

fn identity_error() -> io::Error {
  io::Error::new(io::ErrorKind::PermissionDenied, "Device-local pipe server must run as this user")
}

#[cfg(test)]
mod tests {
  use tokio::net::windows::named_pipe::ServerOptions;

  use super::*;

  #[test]
  fn client_rights_exclude_pipe_instance_creation() {
    assert_eq!(CLIENT_ACCESS & 0x0000_0003, 0x0000_0003);
    assert_eq!(CLIENT_ACCESS & 0x0000_0004, 0);
    assert_eq!(CLIENT_ACCESS & 0x0012_0000, 0x0012_0000);
  }

  #[test]
  fn only_local_device_pipe_names_are_accepted() {
    assert!(valid_name("auv-device-local-0123"));
    assert!(!valid_name(r"\\host\pipe\auv-device-local-0123"));
    assert!(!valid_name("other-pipe"));
    assert!(!valid_name("auv-device-local-a\\b"));
  }

  #[tokio::test]
  async fn accepts_a_server_running_as_this_user() {
    // The per-user daemon serves DeviceLocalService as the same account as
    // its CLI; the former LocalSystem-only check rejected exactly this case.
    let name = format!("auv-device-local-test-{}", std::process::id());
    let path = format!(r"\\.\pipe\{name}");
    let server = ServerOptions::new().first_pipe_instance(true).reject_remote_clients(true).create(&path).unwrap();
    let accepted = tokio::spawn(async move { server.connect().await.map(|()| server) });

    open_verified(&name).await.unwrap();
    accepted.await.unwrap().unwrap();
  }
}
