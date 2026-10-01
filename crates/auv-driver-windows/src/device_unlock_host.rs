//! LocalSystem console worker placement and one-shot, local-only secret transfer.
//!
//! This is an internal host primitive, not a service registration. The caller
//! must run under the installed LocalSystem service identity. No remote request
//! may carry a secret or choose the worker executable.

use crate::device_session::ConsoleSession;

#[derive(Debug, thiserror::Error)]
pub enum HostError {
  #[error("the Windows unlock host is unavailable")]
  Unavailable,
  #[error("the selected console login changed")]
  StaleSession,
  #[error("the selected console is not locked")]
  NotLocked,
  #[error("the worker could not be started in the selected console session")]
  WorkerUnavailable,
  #[error("the local worker identity could not be verified")]
  WorkerIdentity,
  #[error("the local credential transfer failed")]
  TransferFailed,
  #[error("the console unlock outcome could not be verified")]
  Unverified,
}

/// Start one console worker for this exact locked login and transfer the
/// credential over a one-shot local pipe. The caller owns credential storage
/// and retrieval; the worker executable is resolved beside this installed
/// service process, never from a request.
pub fn unlock_with_worker(target: &ConsoleSession, credential: &str) -> Result<ConsoleSession, HostError> {
  native::unlock_with_worker(target, credential)
}

/// Entrypoint used only by the separately installed Windows worker executable.
/// Worker arguments contain a session identity and random pipe name, no secret.
pub fn run_worker(pipe_name: &str, session_id: u32, logon_time: i64, account_sid: &str) -> Result<(), HostError> {
  native::run_worker(pipe_name, session_id, logon_time, account_sid)
}

#[cfg(not(target_os = "windows"))]
mod native {
  use super::HostError;
  use crate::device_session::ConsoleSession;

  pub(super) fn unlock_with_worker(_: &ConsoleSession, _: &str) -> Result<ConsoleSession, HostError> {
    Err(HostError::Unavailable)
  }

  pub(super) fn run_worker(_: &str, _: u32, _: i64, _: &str) -> Result<(), HostError> {
    Err(HostError::Unavailable)
  }
}

#[cfg(target_os = "windows")]
mod native {
  use std::ffi::c_void;
  use std::mem::size_of;
  use std::os::windows::ffi::OsStrExt;
  use std::path::Path;
  use std::time::Duration;

  use windows::Win32::Foundation::{
    CloseHandle, ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, GENERIC_READ, HANDLE, HLOCAL, LocalFree, WAIT_OBJECT_0,
  };
  use windows::Win32::Security::Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
  use windows::Win32::Security::Cryptography::{BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom};
  use windows::Win32::Security::{
    DuplicateTokenEx, SECURITY_ATTRIBUTES, SecurityImpersonation, SetTokenInformation, TOKEN_ALL_ACCESS, TOKEN_DUPLICATE, TOKEN_QUERY,
    TokenPrimary, TokenSessionId,
  };
  use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, FILE_SHARE_MODE, OPEN_EXISTING, PIPE_ACCESS_OUTBOUND, ReadFile,
    WriteFile,
  };
  use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
  use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
    PIPE_WAIT,
  };
  use windows::Win32::System::Threading::{
    CreateEventW, CreateProcessAsUserW, GetCurrentProcess, GetExitCodeProcess, OpenProcessToken, PROCESS_INFORMATION, STARTUPINFOW,
    TerminateProcess, WaitForSingleObject,
  };
  use windows::core::{PCWSTR, PWSTR};
  use zeroize::Zeroizing;

  use super::HostError;
  use crate::device_session::{ConsoleLockState, ConsoleSession, observe_console, unlock_existing_session};

  struct OwnedHandle(HANDLE);
  impl Drop for OwnedHandle {
    fn drop(&mut self) {
      // SAFETY: This guard exclusively owns one successful Win32 handle.
      let _ = unsafe { CloseHandle(self.0) };
    }
  }

  struct SecurityDescriptor(windows::Win32::Security::PSECURITY_DESCRIPTOR);
  impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
      // SAFETY: ConvertStringSecurityDescriptorToSecurityDescriptorW allocated
      // this descriptor with LocalAlloc; it is freed after pipe creation.
      unsafe { LocalFree(HLOCAL(self.0.0)) };
    }
  }

  struct Worker {
    process: OwnedHandle,
    _thread: OwnedHandle,
    pid: u32,
    completed: bool,
  }

  const PIPE_PAYLOAD_BYTES: usize = 258;
  impl Drop for Worker {
    fn drop(&mut self) {
      if !self.completed {
        // SAFETY: This is the exact process created for the one-shot request.
        // Failing closed prevents a stranded worker reading a later pipe.
        let _ = unsafe { TerminateProcess(self.process.0, 1) };
      }
    }
  }

  fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
  }

  fn checked_target(target: &ConsoleSession) -> Result<(), HostError> {
    let current = observe_console().map_err(|_| HostError::Unverified)?.ok_or(HostError::StaleSession)?;

    if !target.same_login(&current) {
      return Err(HostError::StaleSession);
    }

    if current.lock_state != ConsoleLockState::Locked {
      return Err(HostError::NotLocked);
    }

    Ok(())
  }

  pub(super) fn unlock_with_worker(target: &ConsoleSession, credential: &str) -> Result<ConsoleSession, HostError> {
    checked_target(target)?;
    let worker_executable = std::env::current_exe().map_err(|_| HostError::Unavailable)?.with_file_name("auv-device-unlock-worker.exe");

    if !worker_executable.is_absolute() || credential.is_empty() || credential.encode_utf16().count() > 128 {
      return Err(HostError::Unavailable);
    }

    let mut nonce = [0u8; 16];
    // SAFETY: The system RNG writes only to this initialized, live nonce.
    if unsafe { BCryptGenRandom(None, &mut nonce, BCRYPT_USE_SYSTEM_PREFERRED_RNG) }.is_err() {
      return Err(HostError::Unavailable);
    }

    let pipe_name = format!("\\\\.\\pipe\\auv-device-unlock-{}-{}", std::process::id(), hex(&nonce));
    let pipe = restricted_pipe(&pipe_name)?;
    let mut worker = launch_worker(&worker_executable, &pipe_name, target)?;
    connect_worker(&pipe, &worker)?;
    checked_target(target)?;

    let mut payload = Zeroizing::new(vec![0u8; PIPE_PAYLOAD_BYTES]);
    let units = Zeroizing::new(credential.encode_utf16().collect::<Vec<_>>());
    payload[..2].copy_from_slice(&(units.len() as u16).to_le_bytes());

    for (index, unit) in units.iter().enumerate() {
      payload[2 + index * 2..4 + index * 2].copy_from_slice(&unit.to_le_bytes());
    }

    write_payload(&pipe, &payload)?;
    // The worker performs its own WTS readback. The host independently checks
    // the same login after the worker exits; neither an input count nor an exit
    // code alone establishes an unlock.
    // SAFETY: The process handle is owned and remains live through this wait.
    if unsafe { WaitForSingleObject(worker.process.0, Duration::from_secs(15).as_millis() as u32) } != WAIT_OBJECT_0 {
      return Err(HostError::WorkerUnavailable);
    }

    worker.completed = true;
    let mut exit = 1u32;
    // SAFETY: The exit-code pointer is live and the process handle is valid.
    unsafe { GetExitCodeProcess(worker.process.0, &mut exit) }.map_err(|_| HostError::Unverified)?;

    if exit != 0 {
      return Err(HostError::Unverified);
    }

    let current = observe_console().map_err(|_| HostError::Unverified)?.ok_or(HostError::StaleSession)?;

    if !target.same_login(&current) {
      return Err(HostError::StaleSession);
    }

    if current.lock_state != ConsoleLockState::Usable {
      return Err(HostError::Unverified);
    }

    Ok(current)
  }

  fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    bytes
      .iter()
      .flat_map(|byte| {
        [
          DIGITS[(byte >> 4) as usize] as char,
          DIGITS[(byte & 15) as usize] as char,
        ]
      })
      .collect()
  }

  fn restricted_pipe(name: &str) -> Result<OwnedHandle, HostError> {
    let sddl = wide(std::ffi::OsStr::new("D:P(A;;GA;;;SY)"));
    let mut descriptor = windows::Win32::Security::PSECURITY_DESCRIPTOR::default();
    // SAFETY: The SDDL pointer and out-pointer are live. The resulting LocalAlloc
    // descriptor remains live while CreateNamedPipeW copies its security data.
    unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(PCWSTR(sddl.as_ptr()), SDDL_REVISION_1, &mut descriptor, None) }
      .map_err(|_| HostError::Unavailable)?;

    let descriptor = SecurityDescriptor(descriptor);
    let attributes = SECURITY_ATTRIBUTES {
      nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
      lpSecurityDescriptor: descriptor.0.0,
      bInheritHandle: false.into(),
    };
    let name = wide(std::ffi::OsStr::new(name));
    // SAFETY: The UTF-16 name and security descriptor are live for this call.
    // One instance and FILE_FLAG_FIRST_PIPE_INSTANCE reject pre-created pipes.
    let handle = unsafe {
      CreateNamedPipeW(
        PCWSTR(name.as_ptr()),
        PIPE_ACCESS_OUTBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE | FILE_FLAG_OVERLAPPED,
        PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
        1,
        512,
        512,
        0,
        Some(&attributes),
      )
    };

    if handle.is_invalid() {
      Err(HostError::Unavailable)
    } else {
      Ok(OwnedHandle(handle))
    }
  }

  fn launch_worker(exe: &Path, pipe_name: &str, target: &ConsoleSession) -> Result<Worker, HostError> {
    // Stable SIDs from Windows contain only this alphabet. This also keeps the
    // non-secret command line unambiguous to the Windows argument parser.
    if !target.account_sid.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-') {
      return Err(HostError::WorkerUnavailable);
    }

    let exe_wide = wide(exe.as_os_str());
    let arguments = format!("{} {} {} {}", pipe_name, target.session_id, target.logon_time, target.account_sid);
    let command = format!("\"{}\" {arguments}", exe.display());
    let mut command_wide = wide(std::ffi::OsStr::new(&command));
    let mut raw_token = HANDLE::default();
    // SAFETY: This call opens the current process token into one owned handle.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_DUPLICATE | TOKEN_QUERY, &mut raw_token) }
      .map_err(|_| HostError::WorkerIdentity)?;

    let token = OwnedHandle(raw_token);
    let mut primary = HANDLE::default();
    // SAFETY: The LocalSystem service token is live. The duplicated primary
    // token is owned here and modified only for the selected console session.
    unsafe { DuplicateTokenEx(token.0, TOKEN_ALL_ACCESS, None, SecurityImpersonation, TokenPrimary, &mut primary) }
      .map_err(|_| HostError::WorkerIdentity)?;

    let primary = OwnedHandle(primary);
    // SAFETY: SetTokenInformation reads one live u32 session ID.
    unsafe { SetTokenInformation(primary.0, TokenSessionId, (&target.session_id as *const u32).cast::<c_void>(), size_of::<u32>() as u32) }
      .map_err(|_| HostError::WorkerIdentity)?;

    let mut desktop = wide(std::ffi::OsStr::new("winsta0\\default"));
    let startup = STARTUPINFOW {
      cb: size_of::<STARTUPINFOW>() as u32,
      lpDesktop: PWSTR(desktop.as_mut_ptr()),
      ..Default::default()
    };
    let mut process = PROCESS_INFORMATION::default();
    // SAFETY: All pointers are live. Handles are not inherited, and no secret
    // is in the command line or environment. OS grants this process the
    // selected session through the adjusted LocalSystem primary token.
    unsafe {
      CreateProcessAsUserW(
        primary.0,
        PCWSTR(exe_wide.as_ptr()),
        PWSTR(command_wide.as_mut_ptr()),
        None,
        None,
        false,
        Default::default(),
        None,
        PCWSTR::null(),
        &startup,
        &mut process,
      )
    }
    .map_err(|_| HostError::WorkerUnavailable)?;
    Ok(Worker {
      process: OwnedHandle(process.hProcess),
      _thread: OwnedHandle(process.hThread),
      pid: process.dwProcessId,
      completed: false,
    })
  }

  fn connect_worker(pipe: &OwnedHandle, worker: &Worker) -> Result<(), HostError> {
    // This overlapped connection has a deadline, so a failed worker cannot
    // strand the serving thread before any credential is sent.
    // SAFETY: This creates one owned unnamed event for the overlapped call.
    let event = OwnedHandle(unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.map_err(|_| HostError::Unavailable)?);
    let mut overlapped = OVERLAPPED {
      hEvent: event.0,
      ..Default::default()
    };
    // SAFETY: The pipe and OVERLAPPED remain live through completion/cancel.
    let pending = match unsafe { ConnectNamedPipe(pipe.0, Some(&mut overlapped)) } {
      Ok(()) => false,
      Err(error) if error.code() == ERROR_PIPE_CONNECTED.to_hresult() => false,
      Err(error) if error.code() == ERROR_IO_PENDING.to_hresult() => true,
      Err(_) => return Err(HostError::WorkerUnavailable),
    };

    if pending {
      // SAFETY: The event remains live for the wait.
      if unsafe { WaitForSingleObject(event.0, 10_000) } != WAIT_OBJECT_0 {
        // SAFETY: Cancel the exact pending operation before its stack-owned
        // OVERLAPPED is dropped; GetOverlappedResult waits for final completion.
        let _ = unsafe { CancelIoEx(pipe.0, Some(&overlapped)) };
        let mut ignored = 0;
        let _ = unsafe { GetOverlappedResult(pipe.0, &overlapped, &mut ignored, true) };

        return Err(HostError::WorkerUnavailable);
      }

      let mut ignored = 0;
      // SAFETY: Completed event and live OVERLAPPED identify this connect.
      unsafe { GetOverlappedResult(pipe.0, &overlapped, &mut ignored, false) }.map_err(|_| HostError::WorkerUnavailable)?;
    }

    let mut pid = 0u32;
    // SAFETY: GetNamedPipeClientProcessId writes one u32 for the connected peer.
    unsafe { GetNamedPipeClientProcessId(pipe.0, &mut pid) }.map_err(|_| HostError::WorkerIdentity)?;

    if pid != worker.pid {
      return Err(HostError::WorkerIdentity);
    }

    Ok(())
  }

  fn write_payload(pipe: &OwnedHandle, payload: &[u8]) -> Result<(), HostError> {
    // One fixed-size write hides credential length for the unlock mode.
    // SAFETY: The payload and OVERLAPPED remain live until completion/cancel.
    let event = OwnedHandle(unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.map_err(|_| HostError::TransferFailed)?);
    let mut overlapped = OVERLAPPED {
      hEvent: event.0,
      ..Default::default()
    };
    let mut written = 0u32;
    let pending = match unsafe { WriteFile(pipe.0, Some(payload), Some(&mut written), Some(&mut overlapped)) } {
      Ok(()) => false,
      Err(error) if error.code() == ERROR_IO_PENDING.to_hresult() => true,
      Err(_) => return Err(HostError::TransferFailed),
    };

    if pending {
      // SAFETY: The event remains live until the operation has completed.
      if unsafe { WaitForSingleObject(event.0, 5_000) } != WAIT_OBJECT_0 {
        let _ = unsafe { CancelIoEx(pipe.0, Some(&overlapped)) };
        let _ = unsafe { GetOverlappedResult(pipe.0, &overlapped, &mut written, true) };

        return Err(HostError::TransferFailed);
      }

      unsafe { GetOverlappedResult(pipe.0, &overlapped, &mut written, false) }.map_err(|_| HostError::TransferFailed)?;
    }

    if written as usize != payload.len() {
      return Err(HostError::TransferFailed);
    }

    Ok(())
  }

  fn read_payload(pipe_name: &str) -> Result<(OwnedHandle, Zeroizing<Vec<u8>>), HostError> {
    let pipe_wide = wide(std::ffi::OsStr::new(pipe_name));
    // SAFETY: The validated path is a local pipe. The server compares this
    // client's process ID to the exact one-shot worker before writing.
    let pipe = OwnedHandle(
      unsafe {
        CreateFileW(
          PCWSTR(pipe_wide.as_ptr()),
          GENERIC_READ.0,
          FILE_SHARE_MODE(0),
          None,
          OPEN_EXISTING,
          Default::default(),
          HANDLE::default(),
        )
      }
      .map_err(|_| HostError::WorkerUnavailable)?,
    );
    let mut payload = Zeroizing::new(vec![0u8; PIPE_PAYLOAD_BYTES]);
    let mut offset = 0usize;

    while offset < payload.len() {
      let mut read = 0u32;
      // SAFETY: ReadFile writes only to the remaining initialized slice.
      unsafe { ReadFile(pipe.0, Some(&mut payload[offset..]), Some(&mut read), None) }.map_err(|_| HostError::TransferFailed)?;

      if read == 0 {
        return Err(HostError::TransferFailed);
      }

      offset += read as usize;
    }

    Ok((pipe, payload))
  }

  pub(super) fn run_worker(pipe_name: &str, session_id: u32, logon_time: i64, account_sid: &str) -> Result<(), HostError> {
    if !pipe_name.starts_with("\\\\.\\pipe\\auv-device-unlock-") || pipe_name.len() > 128 {
      return Err(HostError::WorkerUnavailable);
    }

    let target = ConsoleSession {
      session_id,
      logon_time,
      account_sid: account_sid.to_owned(),
      domain: String::new(),
      user: String::new(),
      lock_state: ConsoleLockState::Locked,
    };
    checked_target(&target)?;
    let (_pipe, payload) = read_payload(pipe_name)?;
    let length = u16::from_le_bytes([payload[0], payload[1]]) as usize;

    if length == 0 || length > 128 {
      return Err(HostError::TransferFailed);
    }

    let mut units = Zeroizing::new(Vec::with_capacity(length));

    for index in 0..length {
      units.push(u16::from_le_bytes([payload[2 + index * 2], payload[3 + index * 2]]));
    }

    let credential = Zeroizing::new(String::from_utf16(&units).map_err(|_| HostError::TransferFailed)?);
    checked_target(&target)?;
    unlock_existing_session(&target, &credential).map_err(|_| HostError::Unverified)?;
    Ok(())
  }
}
