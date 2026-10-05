//! LocalSystem Helper Host: the SCM service behind `auv-helper.exe --service`.
//!
//! It serves one fixed, machine-local named pipe. Each connection carries one
//! request. The caller SID comes from impersonating the pipe client after its
//! request was read, never from the request. Requests run one at a time, so
//! two daemons cannot interleave worker launches or vault writes.

use std::fs::File;
use std::io::{Read, Write};
use std::mem::size_of;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use auv_driver_windows::device_session::{ConsoleLockState, ConsoleSession, ConsoleSessionError, observe_console};
use auv_driver_windows::device_unlock_host::{self as worker, lock_with_worker, unlock_with_worker};
use windows::Win32::Foundation::{BOOL, GENERIC_READ, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, RevertToSelf, SECURITY_ATTRIBUTES, TOKEN_QUERY};
use windows::Win32::Storage::FileSystem::{
  CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_SHARE_MODE, FlushFileBuffers, OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
};
use windows::Win32::System::Pipes::{
  ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, ImpersonateNamedPipeClient, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
  PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows::Win32::System::Threading::{GetCurrentThread, OpenThreadToken};
use windows::core::PCWSTR;
use zeroize::Zeroizing;

use crate::vault::{self, VaultError};
use crate::wire::{self, Reader, Target};
use crate::{HostError, LOCAL_SYSTEM_SID, Operation, PIPE_PATH, SERVICE_NAME, authorize};

// NOTICE(windows-helper-pipe-acl): An ordinary user's daemon must reach this
// LocalSystem pipe. Authenticated users get only READ_CONTROL, SYNCHRONIZE,
// FILE_READ_ATTRIBUTES, FILE_READ_DATA, and FILE_WRITE_DATA (0x00120083);
// FILE_CREATE_PIPE_INSTANCE is withheld so no user can add a competing
// instance. An installed-host matrix for the former LocalSystem
// DeviceLocalService pipe showed CreateFileW needs FILE_READ_ATTRIBUTES (0x80)
// even when the client requests 0x00120003. A LocalSystem object defaults to
// System integrity, which rejects Medium-integrity writers, so the label is
// lowered to Medium explicitly.
// https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights
// https://learn.microsoft.com/en-us/windows/win32/secauthz/mandatory-integrity-control
const PIPE_SDDL: &str = "D:P(A;;GA;;;SY)(A;;0x00120083;;;AU)S:(ML;;NW;;;ME)";
// NOTICE(windows-helper-instances): Each connection gets a thread. Bounding
// instances caps threads held by clients that connect and never write; a
// local user can still occupy them, which is a local denial of service only.
const MAX_INSTANCES: u32 = 16;
const ANONYMOUS_SID: &str = "S-1-5-7";
const MAX_PIN_UTF16_UNITS: usize = 128;

static DISPATCH: Mutex<()> = Mutex::new(());

/// Serve the pipe until `stop` is set and `wake` has unblocked the listener.
/// `on_ready` runs once the host identity is verified and the first pipe
/// instance accepts connections, so SCM `Running` means the Helper is usable.
pub fn serve(stop: &AtomicBool, on_ready: impl FnOnce()) -> Result<(), String> {
  crate::storage::require_system_host().map_err(|_| "the AUV Helper Host must run as LocalSystem in Session 0".to_string())?;
  // FIRST_PIPE_INSTANCE fails if any process already owns this name, so a
  // squatter makes the service fail to start instead of serving requests.
  let mut next = create_instance(true).map_err(|error| format!("failed to create the AUV Helper pipe: {error}"))?;
  on_ready();

  loop {
    // SAFETY: `next` is a live, owned server instance. A client that
    // connected before this call reports ERROR_PIPE_CONNECTED, which is fine.
    let _ = unsafe { ConnectNamedPipe(HANDLE(next.as_raw_handle()), None) };

    if stop.load(Ordering::Acquire) {
      break;
    }

    let connected = next;
    next = loop {
      match create_instance(false) {
        Ok(instance) => break instance,
        Err(_) if !stop.load(Ordering::Acquire) => std::thread::sleep(Duration::from_millis(100)),
        Err(error) => return Err(format!("failed to create the next AUV Helper pipe instance: {error}")),
      }
    };
    std::thread::spawn(move || handle(connected));
  }

  // Let an in-flight lock or unlock finish so its worker is not stranded.
  drop(DISPATCH.lock());
  Ok(())
}

/// Unblock a listener waiting in `ConnectNamedPipe` after `stop` was set.
pub fn wake() {
  let path = PIPE_PATH.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
  // SAFETY: The NUL-terminated path is live; the handle is closed at once.
  let opened = unsafe {
    CreateFileW(PCWSTR(path.as_ptr()), GENERIC_READ.0, FILE_SHARE_MODE(0), None, OPEN_EXISTING, Default::default(), HANDLE::default())
  };

  if let Ok(handle) = opened {
    // SAFETY: CreateFileW returned one owned handle.
    drop(unsafe { OwnedHandle::from_raw_handle(handle.0) });
  }
}

fn create_instance(first: bool) -> std::io::Result<File> {
  struct Descriptor(PSECURITY_DESCRIPTOR);
  impl Drop for Descriptor {
    fn drop(&mut self) {
      // SAFETY: The SDDL conversion allocated this descriptor with LocalAlloc.
      unsafe { LocalFree(HLOCAL(self.0.0)) };
    }
  }

  let sddl = PIPE_SDDL.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
  let mut raw = PSECURITY_DESCRIPTOR::default();
  // SAFETY: The SDDL buffer is NUL-terminated; `raw` receives one allocation.
  unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(PCWSTR(sddl.as_ptr()), SDDL_REVISION_1, &mut raw, None) }
    .map_err(std::io::Error::other)?;
  let descriptor = Descriptor(raw);
  let attributes = SECURITY_ATTRIBUTES {
    nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
    lpSecurityDescriptor: descriptor.0.0,
    bInheritHandle: BOOL(0),
  };
  let path = PIPE_PATH.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
  let mode = if first {
    PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE
  } else {
    PIPE_ACCESS_DUPLEX
  };
  // SAFETY: The path and attributes are live; Windows copies the descriptor.
  let handle = unsafe {
    CreateNamedPipeW(
      PCWSTR(path.as_ptr()),
      mode,
      PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
      MAX_INSTANCES,
      4096,
      4096,
      0,
      Some(&attributes),
    )
  };

  if handle.is_invalid() {
    return Err(std::io::Error::last_os_error());
  }

  // SAFETY: CreateNamedPipeW returned one owned server handle.
  Ok(unsafe { File::from_raw_handle(handle.0) })
}

fn handle(mut pipe: File) {
  let response = match read_request(&mut pipe) {
    Ok((operation, payload)) => match caller_sid(&pipe) {
      Ok(caller) => {
        let _serial = DISPATCH.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        dispatch(&caller, operation, &payload)
      }
      Err(error) => Err(error),
    },
    Err(error) => Err(error),
  };
  let bytes = Zeroizing::new(wire::encode_response(response.as_deref().map_err(|error| *error)));
  let _ = pipe.write_all(&bytes);
  // SAFETY: The connected server handle is live until `pipe` drops.
  let _ = unsafe { FlushFileBuffers(HANDLE(pipe.as_raw_handle())) };
  let _ = unsafe { DisconnectNamedPipe(HANDLE(pipe.as_raw_handle())) };
}

fn read_request(pipe: &mut File) -> Result<(Operation, Zeroizing<Vec<u8>>), HostError> {
  let mut header = [0u8; 9];
  pipe.read_exact(&mut header).map_err(|_| HostError::InvalidRequest)?;
  let (operation, length) = wire::decode_request_header(&header)?;
  let mut payload = Zeroizing::new(vec![0u8; length]);
  pipe.read_exact(&mut payload).map_err(|_| HostError::InvalidRequest)?;
  Ok((operation, payload))
}

/// The connected client's token SID, read after its request bytes so the
/// impersonated context is the one that sent them.
fn caller_sid(pipe: &File) -> Result<String, HostError> {
  struct Revert;
  impl Drop for Revert {
    fn drop(&mut self) {
      // NOTICE(windows-helper-revert): A thread left impersonating the
      // client would run later work with the client's identity. Abort
      // rather than continue if Windows refuses to revert.
      // SAFETY: RevertToSelf has no pointer arguments.
      if unsafe { RevertToSelf() }.is_err() {
        std::process::abort();
      }
    }
  }

  // SAFETY: The pipe is a live server end whose read just completed.
  unsafe { ImpersonateNamedPipeClient(HANDLE(pipe.as_raw_handle())) }.map_err(|_| HostError::Unauthorized)?;
  let _revert = Revert;
  let mut token = HANDLE::default();
  // SAFETY: Open the impersonation token as self so the service identity,
  // not the client, needs access to the token object.
  unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &mut token) }.map_err(|_| HostError::Unauthorized)?;
  // SAFETY: OpenThreadToken returned one owned token handle.
  let token = unsafe { OwnedHandle::from_raw_handle(token.0) };
  let sid = crate::token::user_sid(HANDLE(token.as_raw_handle())).map_err(|_| HostError::Unauthorized)?;

  if sid == ANONYMOUS_SID || sid == LOCAL_SYSTEM_SID {
    return Err(HostError::Unauthorized);
  }

  Ok(sid)
}

fn dispatch(caller: &str, operation: Operation, payload: &[u8]) -> Result<Vec<u8>, HostError> {
  match operation {
    Operation::Observe => {
      Reader::new(payload).finish()?;
      let current = observe_console().map_err(console_error)?;
      wire::encode_observed(current.as_ref())
    }
    Operation::Enroll => {
      let mut reader = Reader::new(payload);
      let account_sid = reader.text()?;
      let pin = Zeroizing::new(reader.text()?.to_owned());
      reader.finish()?;
      authorize(caller, account_sid)?;

      if pin.is_empty() || pin.chars().any(char::is_control) || pin.encode_utf16().count() > MAX_PIN_UTF16_UNITS {
        return Err(HostError::InvalidCredential);
      }

      vault::enroll(account_sid, &pin).map_err(vault_error)?;
      Ok(Vec::new())
    }
    Operation::Remove => {
      let mut reader = Reader::new(payload);
      let account_sid = reader.text()?;
      reader.finish()?;
      authorize(caller, account_sid)?;
      vault::remove(account_sid).map_err(vault_error)?;
      Ok(Vec::new())
    }
    Operation::Probe => {
      let target = wire::decode_target(payload)?;
      authorize(caller, &target.account_sid)?;
      let current = selected(&target, ConsoleLockState::Locked)?;
      vault::verify_while_locked(&current).map_err(vault_error)?;
      Ok(Vec::new())
    }
    Operation::Unlock => {
      let target = wire::decode_target(payload)?;
      authorize(caller, &target.account_sid)?;
      let current = selected(&target, ConsoleLockState::Locked)?;
      // Retrieval stays in this LocalSystem process; the driver receives the
      // PIN only for the one-shot pipe transfer to its worker.
      let pin = vault::retrieve(&current.account_sid).map_err(vault_error)?;
      unlock_with_worker(&current, &pin).map_err(worker_error)?;
      Ok(Vec::new())
    }
    Operation::Lock => {
      let target = wire::decode_target(payload)?;
      authorize(caller, &target.account_sid)?;
      let current = selected(&target, ConsoleLockState::Usable)?;
      lock_with_worker(&current).map_err(worker_error)?;
      Ok(Vec::new())
    }
  }
}

/// Re-observe the console and return it only if it is still the requested
/// login, account, and lock state.
fn selected(target: &Target, state: ConsoleLockState) -> Result<ConsoleSession, HostError> {
  let current = observe_console().map_err(console_error)?.ok_or(HostError::StaleSession)?;

  if current.session_id != target.session_id || current.logon_time != target.logon_time || current.account_sid != target.account_sid {
    return Err(HostError::StaleSession);
  }

  match (state, current.lock_state) {
    (expected, actual) if expected == actual => Ok(current),
    (ConsoleLockState::Locked, _) => Err(HostError::NotLocked),
    _ => Err(HostError::StaleSession),
  }
}

fn console_error(error: ConsoleSessionError) -> HostError {
  match error {
    ConsoleSessionError::ConsoleTransition => HostError::StaleSession,
    ConsoleSessionError::InconsistentRecord
    | ConsoleSessionError::UnsupportedPlatform
    | ConsoleSessionError::QueryFailed(_)
    | ConsoleSessionError::IdentityUnverified => HostError::Unavailable,
  }
}

fn vault_error(error: VaultError) -> HostError {
  match error {
    VaultError::NotEnrolled => HostError::NotEnrolled,
    VaultError::NotLocked => HostError::NotLocked,
    VaultError::InvalidAccount => HostError::InvalidRequest,
    VaultError::Unavailable | VaultError::Permissions | VaultError::RetrievalFailed => HostError::VaultUnavailable,
  }
}

fn worker_error(error: worker::HostError) -> HostError {
  match error {
    worker::HostError::StaleSession => HostError::StaleSession,
    worker::HostError::NotLocked => HostError::NotLocked,
    worker::HostError::Unverified => HostError::Unverified,
    worker::HostError::Unavailable
    | worker::HostError::WorkerUnavailable
    | worker::HostError::WorkerIdentity
    | worker::HostError::TransferFailed => HostError::Unavailable,
  }
}

/// Run as the SCM-registered `AuvHelper` service until SCM stops it.
pub fn run_service() -> Result<(), String> {
  windows_service::service_dispatcher::start(SERVICE_NAME, ffi_service_main)
    .map_err(|error| format!("failed to connect to the Windows Service Control Manager: {error}"))
}

windows_service::define_windows_service!(ffi_service_main, service_main);

static STOP: AtomicBool = AtomicBool::new(false);

fn service_main(_arguments: Vec<std::ffi::OsString>) {
  use windows_service::service::{ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType};
  use windows_service::service_control_handler::{self, ServiceControlHandlerResult};

  let status = |state: ServiceState, controls: ServiceControlAccept, exit_code: u32, wait_hint: Duration| ServiceStatus {
    service_type: ServiceType::OWN_PROCESS,
    current_state: state,
    controls_accepted: controls,
    exit_code: ServiceExitCode::Win32(exit_code),
    checkpoint: 0,
    process_id: None,
    wait_hint,
  };
  let Ok(handle) = service_control_handler::register(SERVICE_NAME, |control| match control {
    ServiceControl::Stop => {
      STOP.store(true, Ordering::Release);
      wake();
      ServiceControlHandlerResult::NoError
    }
    ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
    _ => ServiceControlHandlerResult::NotImplemented,
  }) else {
    return;
  };

  let _ = handle.set_service_status(status(ServiceState::StartPending, ServiceControlAccept::empty(), 0, Duration::from_secs(10)));
  // Report Running only after the pipe exists. Setup treats Running as
  // installed; reporting it first hid identity and pipe-creation failures.
  let result = serve(&STOP, || {
    let _ = handle.set_service_status(status(ServiceState::Running, ServiceControlAccept::STOP, 0, Duration::ZERO));
  });
  // Unlock waits up to the worker deadline; tell SCM before draining it.
  let _ = handle.set_service_status(status(ServiceState::StopPending, ServiceControlAccept::empty(), 0, Duration::from_secs(30)));

  if let Err(error) = &result {
    eprintln!("AUV Helper Host stopped: {error}");
  }

  let exit_code = if result.is_ok() { 0 } else { 1 };
  let _ = handle.set_service_status(status(ServiceState::Stopped, ServiceControlAccept::empty(), exit_code, Duration::ZERO));
}
