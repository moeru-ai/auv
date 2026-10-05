//! Private machine-local protocol between an AUV daemon and the installed
//! Windows Helper Host.
//!
//! The daemon owns listeners, pairing, Device policy, audit, and enrollment
//! metadata. The Helper Host is a LocalSystem service in Session 0 that owns
//! only the privileged capabilities behind them: physical-console observation,
//! the protected PIN vault, and the one-shot `auv-helper.exe` worker that locks
//! or unlocks one exact existing login. It has no network listener, pairing
//! store, or policy of its own.
//!
//! Every account-scoped request names its target account SID. The host serves
//! it only when that SID equals the SID of the connected caller's token, so a
//! daemon can act only on its own user's console login.

pub use auv_driver_windows::device_session::{ConsoleLockState, ConsoleSession};

#[cfg(all(windows, feature = "host"))]
pub mod host;
#[cfg(all(windows, feature = "host"))]
mod storage;
#[cfg(all(windows, feature = "host"))]
mod vault;

/// SCM name of the installed Helper Host service.
pub const SERVICE_NAME: &str = "AuvHelper";
/// Display name shown by the Windows Services console.
pub const SERVICE_DISPLAY_NAME: &str = "AUV Helper";
/// The only `auv-helper.exe` argument that starts the SCM-hosted Helper Host.
pub const SERVICE_ARGUMENT: &str = "--service";

/// Wire protocol this build speaks. A host rejects any other value with
/// `ProtocolUnsupported`, which the daemon reports as `HOST_INCOMPATIBLE`.
// TODO(windows-helper-protocol-range): Accept a range only when a second
// protocol version ships; until then daemon and helper must match exactly.
pub const PROTOCOL_VERSION: u16 = 1;

const MAGIC: [u8; 4] = *b"AUVH";
const MAX_PAYLOAD: usize = 1024;
#[cfg(windows)]
const PIPE_PATH: &str = r"\\.\pipe\auv-helper";
#[cfg_attr(not(windows), allow(dead_code))]
const LOCAL_SYSTEM_SID: &str = "S-1-5-18";

/// Stable, secret-free failure reported by the Helper Host or its client.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum HostError {
  #[error("the AUV Helper is not installed or not running")]
  Unavailable,
  #[error("the AUV Helper pipe server is not the LocalSystem Helper Host")]
  Untrusted,
  #[error("the AUV Helper speaks an unsupported protocol version")]
  ProtocolUnsupported,
  #[error("the AUV Helper rejected a malformed request")]
  InvalidRequest,
  #[error("the requested account is not the caller's own account")]
  Unauthorized,
  #[error("the selected console login changed")]
  StaleSession,
  #[error("the selected console login is not locked")]
  NotLocked,
  #[error("the account has no enrolled credential")]
  NotEnrolled,
  #[error("the credential cannot be stored")]
  InvalidCredential,
  #[error("the protected credential vault is unavailable")]
  VaultUnavailable,
  #[error("the console lock or unlock outcome could not be verified")]
  Unverified,
}

impl HostError {
  fn code(self) -> u8 {
    match self {
      Self::Unavailable => 1,
      Self::Untrusted => 2,
      Self::ProtocolUnsupported => 3,
      Self::InvalidRequest => 4,
      Self::Unauthorized => 5,
      Self::StaleSession => 6,
      Self::NotLocked => 7,
      Self::NotEnrolled => 8,
      Self::InvalidCredential => 9,
      Self::VaultUnavailable => 10,
      Self::Unverified => 11,
    }
  }

  fn from_code(code: u8) -> Self {
    match code {
      2 => Self::Untrusted,
      3 => Self::ProtocolUnsupported,
      4 => Self::InvalidRequest,
      5 => Self::Unauthorized,
      6 => Self::StaleSession,
      7 => Self::NotLocked,
      8 => Self::NotEnrolled,
      9 => Self::InvalidCredential,
      10 => Self::VaultUnavailable,
      11 => Self::Unverified,
      // An unknown code from a newer host still fails closed.
      _ => Self::Unavailable,
    }
  }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Operation {
  /// Read the physical console login. Not account-scoped; returns no secret.
  Observe = 1,
  Enroll = 2,
  /// Prove the vault item is retrievable while the selected login is locked.
  Probe = 3,
  Remove = 4,
  Unlock = 5,
  Lock = 6,
}

impl TryFrom<u8> for Operation {
  type Error = HostError;

  fn try_from(value: u8) -> Result<Self, HostError> {
    [
      Self::Observe,
      Self::Enroll,
      Self::Probe,
      Self::Remove,
      Self::Unlock,
      Self::Lock,
    ]
    .into_iter()
    .find(|operation| *operation as u8 == value)
    .ok_or(HostError::InvalidRequest)
  }
}

/// Read the physical console login through the Helper Host. Only LocalSystem
/// can read a session token's account SID, so the daemon cannot do this itself.
pub fn observe() -> Result<Option<ConsoleSession>, HostError> {
  let response = call(Operation::Observe, &[])?;
  wire::decode_observed(&response)
}

/// Store this account's PIN in the protected vault. The result is PENDING
/// until `probe_locked` succeeds for a locked login of the same account.
pub fn enroll(account_sid: &str, pin: &str) -> Result<(), HostError> {
  let mut payload = zeroize::Zeroizing::new(Vec::new());
  wire::put_text(&mut payload, account_sid)?;
  wire::put_text(&mut payload, pin)?;
  call(Operation::Enroll, &payload).map(drop)
}

/// Delete this account's vault item. A missing item is reported as
/// `NotEnrolled` so the daemon can decide whether that is a retry.
pub fn remove(account_sid: &str) -> Result<(), HostError> {
  let mut payload = Vec::new();
  wire::put_text(&mut payload, account_sid)?;
  call(Operation::Remove, &payload).map(drop)
}

/// Prove LocalSystem retrieval of this login's PIN while it remains locked.
pub fn probe_locked(target: &ConsoleSession) -> Result<(), HostError> {
  call(Operation::Probe, &wire::encode_target(target)?).map(drop)
}

/// Unlock exactly this locked login. The host re-observes the login, retrieves
/// the PIN, runs the worker, and reads back the same login as usable.
pub fn unlock(target: &ConsoleSession) -> Result<(), HostError> {
  call(Operation::Unlock, &wire::encode_target(target)?).map(drop)
}

/// Lock exactly this usable login and read it back as locked.
pub fn lock(target: &ConsoleSession) -> Result<(), HostError> {
  call(Operation::Lock, &wire::encode_target(target)?).map(drop)
}

/// The single authorization rule for account-scoped requests.
// TODO(windows-helper-admin): An elevated administrator acting on another
// account is not served. Add it only with an owner-approved cross-account
// enrollment or unlock design; the daemon would need the same rule.
#[cfg_attr(not(all(windows, feature = "host")), allow(dead_code))]
fn authorize(caller_sid: &str, account_sid: &str) -> Result<(), HostError> {
  if caller_sid == LOCAL_SYSTEM_SID || !valid_sid(caller_sid) || caller_sid != account_sid {
    return Err(HostError::Unauthorized);
  }

  Ok(())
}

#[cfg_attr(not(all(windows, feature = "host")), allow(dead_code))]
fn valid_sid(sid: &str) -> bool {
  sid.starts_with("S-1-") && sid.len() <= 128 && sid.bytes().all(|byte| byte.is_ascii_digit() || byte == b'S' || byte == b'-')
}

#[cfg(windows)]
fn call(operation: Operation, payload: &[u8]) -> Result<zeroize::Zeroizing<Vec<u8>>, HostError> {
  use std::io::{Read, Write};

  let mut pipe = client::open_verified()?;
  let request = zeroize::Zeroizing::new(wire::encode_request(operation, payload)?);
  pipe.write_all(&request).map_err(|_| HostError::Unavailable)?;
  let mut header = [0u8; 9];
  pipe.read_exact(&mut header).map_err(|_| HostError::Unavailable)?;
  let length = wire::decode_response_header(&header)?;
  let mut body = zeroize::Zeroizing::new(vec![0u8; length]);
  pipe.read_exact(&mut body).map_err(|_| HostError::Unavailable)?;
  Ok(body)
}

#[cfg(not(windows))]
fn call(_operation: Operation, _payload: &[u8]) -> Result<zeroize::Zeroizing<Vec<u8>>, HostError> {
  Err(HostError::Unavailable)
}

/// Length-delimited framing shared by the client and host.
///
/// Request: `MAGIC | protocol u16 | operation u8 | length u16 | payload`.
/// Response: `MAGIC | protocol u16 | status u8 | length u16 | payload`.
/// All integers are little-endian. Status 0 is success; any other value is a
/// `HostError` code. Text fields are a u16 byte length followed by UTF-8.
mod wire {
  use super::{ConsoleLockState, ConsoleSession, HostError, MAGIC, MAX_PAYLOAD, Operation, PROTOCOL_VERSION};

  #[cfg_attr(not(windows), allow(dead_code))]
  pub(crate) fn encode_request(operation: Operation, payload: &[u8]) -> Result<Vec<u8>, HostError> {
    frame(operation as u8, payload)
  }

  #[cfg_attr(not(all(windows, feature = "host")), allow(dead_code))]
  pub(crate) fn encode_response(result: Result<&[u8], HostError>) -> Vec<u8> {
    let (status, payload) = match result {
      Ok(payload) => (0, payload),
      Err(error) => (error.code(), &[][..]),
    };
    // A host-built payload is bounded below MAX_PAYLOAD; fall back to a
    // fixed failure instead of panicking the serving thread.
    frame(status, payload).unwrap_or_else(|_| frame(HostError::Unavailable.code(), &[]).expect("empty frame"))
  }

  fn frame(code: u8, payload: &[u8]) -> Result<Vec<u8>, HostError> {
    if payload.len() > MAX_PAYLOAD {
      return Err(HostError::InvalidRequest);
    }

    let mut bytes = Vec::with_capacity(9 + payload.len());
    bytes.extend_from_slice(&MAGIC);
    bytes.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    bytes.push(code);
    bytes.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    bytes.extend_from_slice(payload);
    Ok(bytes)
  }

  /// Validate a request header and return its operation and payload length.
  #[cfg_attr(not(all(windows, feature = "host")), allow(dead_code))]
  pub(crate) fn decode_request_header(header: &[u8; 9]) -> Result<(Operation, usize), HostError> {
    let (code, length) = decode_header(header)?;
    Ok((Operation::try_from(code)?, length))
  }

  #[cfg_attr(not(windows), allow(dead_code))]
  pub(crate) fn decode_response_header(header: &[u8; 9]) -> Result<usize, HostError> {
    match decode_header(header)? {
      (0, length) => Ok(length),
      (code, _) => Err(HostError::from_code(code)),
    }
  }

  fn decode_header(header: &[u8; 9]) -> Result<(u8, usize), HostError> {
    if header[..4] != MAGIC {
      return Err(HostError::InvalidRequest);
    }

    if u16::from_le_bytes([header[4], header[5]]) != PROTOCOL_VERSION {
      return Err(HostError::ProtocolUnsupported);
    }

    let length = u16::from_le_bytes([header[7], header[8]]) as usize;

    if length > MAX_PAYLOAD {
      return Err(HostError::InvalidRequest);
    }

    Ok((header[6], length))
  }

  pub(crate) fn put_text(bytes: &mut Vec<u8>, text: &str) -> Result<(), HostError> {
    let length = u16::try_from(text.len()).map_err(|_| HostError::InvalidRequest)?;
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    Ok(())
  }

  /// A cursor over one received payload; trailing bytes are rejected.
  pub(crate) struct Reader<'a>(&'a [u8]);

  impl<'a> Reader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
      Self(bytes)
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], HostError> {
      if self.0.len() < count {
        return Err(HostError::InvalidRequest);
      }

      let (head, tail) = self.0.split_at(count);
      self.0 = tail;
      Ok(head)
    }

    fn u8(&mut self) -> Result<u8, HostError> {
      Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, HostError> {
      Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("four bytes")))
    }

    fn i64(&mut self) -> Result<i64, HostError> {
      Ok(i64::from_le_bytes(self.take(8)?.try_into().expect("eight bytes")))
    }

    pub(crate) fn text(&mut self) -> Result<&'a str, HostError> {
      let length = u16::from_le_bytes(self.take(2)?.try_into().expect("two bytes")) as usize;
      std::str::from_utf8(self.take(length)?).map_err(|_| HostError::InvalidRequest)
    }

    pub(crate) fn finish(self) -> Result<(), HostError> {
      if self.0.is_empty() {
        Ok(())
      } else {
        Err(HostError::InvalidRequest)
      }
    }
  }

  /// A selected login as sent by the daemon. The host trusts none of these
  /// fields; it compares them with its own fresh observation.
  #[derive(Debug, Eq, PartialEq)]
  pub(crate) struct Target {
    pub session_id: u32,
    pub logon_time: i64,
    pub account_sid: String,
  }

  pub(crate) fn encode_target(session: &ConsoleSession) -> Result<Vec<u8>, HostError> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&session.session_id.to_le_bytes());
    bytes.extend_from_slice(&session.logon_time.to_le_bytes());
    put_text(&mut bytes, &session.account_sid)?;
    Ok(bytes)
  }

  #[cfg_attr(not(all(windows, feature = "host")), allow(dead_code))]
  pub(crate) fn decode_target(bytes: &[u8]) -> Result<Target, HostError> {
    let mut reader = Reader::new(bytes);
    let target = Target {
      session_id: reader.u32()?,
      logon_time: reader.i64()?,
      account_sid: reader.text()?.to_owned(),
    };
    reader.finish()?;
    Ok(target)
  }

  #[cfg_attr(not(all(windows, feature = "host")), allow(dead_code))]
  pub(crate) fn encode_observed(session: Option<&ConsoleSession>) -> Result<Vec<u8>, HostError> {
    let Some(session) = session else {
      return Ok(vec![0]);
    };

    let mut bytes = vec![1];
    bytes.extend_from_slice(&session.session_id.to_le_bytes());
    bytes.extend_from_slice(&session.logon_time.to_le_bytes());
    bytes.push(match session.lock_state {
      ConsoleLockState::Locked => 1,
      ConsoleLockState::Usable => 2,
      ConsoleLockState::Unknown => 0,
    });
    put_text(&mut bytes, &session.account_sid)?;
    put_text(&mut bytes, &session.domain)?;
    put_text(&mut bytes, &session.user)?;
    Ok(bytes)
  }

  pub(crate) fn decode_observed(bytes: &[u8]) -> Result<Option<ConsoleSession>, HostError> {
    let mut reader = Reader::new(bytes);

    if reader.u8()? == 0 {
      reader.finish()?;
      return Ok(None);
    }

    let session = ConsoleSession {
      session_id: reader.u32()?,
      logon_time: reader.i64()?,
      lock_state: match reader.u8()? {
        1 => ConsoleLockState::Locked,
        2 => ConsoleLockState::Usable,
        _ => ConsoleLockState::Unknown,
      },
      account_sid: reader.text()?.to_owned(),
      domain: reader.text()?.to_owned(),
      user: reader.text()?.to_owned(),
    };
    reader.finish()?;
    Ok(Some(session))
  }
}

#[cfg(windows)]
mod client {
  use std::fs::File;
  use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
  use std::time::{Duration, Instant};

  use windows::Win32::Foundation::{ERROR_PIPE_BUSY, FALSE, HANDLE};
  use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_MODE, OPEN_EXISTING, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT};
  use windows::Win32::System::Pipes::{GetNamedPipeServerProcessId, GetNamedPipeServerSessionId, WaitNamedPipeW};
  use windows::Win32::System::Threading::{OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION};
  use windows::core::{HRESULT, PCWSTR};

  use super::{HostError, LOCAL_SYSTEM_SID, PIPE_PATH};

  // NOTICE(windows-helper-pipe-rights): Request only read/write data plus
  // READ_CONTROL and SYNCHRONIZE. GENERIC_WRITE would include the bit shared
  // with FILE_CREATE_PIPE_INSTANCE, which the host DACL deliberately denies.
  // https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights
  const CLIENT_ACCESS: u32 = 0x0012_0003;

  /// Open the Helper Host pipe and verify, before any request byte is sent,
  /// that the kernel-reported server is a LocalSystem process in Session 0.
  /// A squatting user process cannot receive an enrollment PIN.
  pub(super) fn open_verified() -> Result<File, HostError> {
    let path = PIPE_PATH.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    // Unlock holds a host thread for up to the worker deadline; a busy pipe
    // means all instances are serving and is retried until this deadline.
    let deadline = Instant::now() + Duration::from_secs(30);

    loop {
      // SAFETY: The path is NUL-terminated and live for the call. The
      // returned handle is moved into an owning wrapper immediately.
      let opened = unsafe {
        CreateFileW(
          PCWSTR(path.as_ptr()),
          CLIENT_ACCESS,
          FILE_SHARE_MODE(0),
          None,
          OPEN_EXISTING,
          SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
          HANDLE::default(),
        )
      };

      match opened {
        Ok(raw) => {
          // SAFETY: CreateFileW returned one owned handle.
          let handle = unsafe { OwnedHandle::from_raw_handle(raw.0) };
          verify_server(HANDLE(handle.as_raw_handle()))?;
          return Ok(File::from(handle));
        }
        Err(error) if error.code() == HRESULT::from_win32(ERROR_PIPE_BUSY.0) && Instant::now() < deadline => {
          // SAFETY: The path is NUL-terminated and live for the wait.
          let _ = unsafe { WaitNamedPipeW(PCWSTR(path.as_ptr()), 1_000) };
        }
        Err(_) => return Err(HostError::Unavailable),
      }
    }
  }

  fn verify_server(pipe: HANDLE) -> Result<(), HostError> {
    let mut server_pid = 0u32;
    let mut server_session = u32::MAX;
    // SAFETY: Both calls write one live u32 for this connected pipe handle.
    unsafe { GetNamedPipeServerProcessId(pipe, &mut server_pid) }.map_err(|_| HostError::Untrusted)?;
    unsafe { GetNamedPipeServerSessionId(pipe, &mut server_session) }.map_err(|_| HostError::Untrusted)?;

    if server_pid == 0 || server_session != 0 {
      return Err(HostError::Untrusted);
    }

    // SAFETY: The kernel supplied this PID for the connected pipe. The
    // process handle pins that process while its token is inspected.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, server_pid) }.map_err(|_| HostError::Untrusted)?;
    // SAFETY: OpenProcess returned one owned process handle.
    let process = unsafe { OwnedHandle::from_raw_handle(process.0) };
    let mut token = HANDLE::default();
    // SAFETY: The process handle is live; Windows writes one owned token.
    unsafe { OpenProcessToken(HANDLE(process.as_raw_handle()), windows::Win32::Security::TOKEN_QUERY, &mut token) }
      .map_err(|_| HostError::Untrusted)?;
    // SAFETY: OpenProcessToken returned one owned token handle.
    let token = unsafe { OwnedHandle::from_raw_handle(token.0) };

    if super::token::user_sid(HANDLE(token.as_raw_handle())).map_err(|_| HostError::Untrusted)? != LOCAL_SYSTEM_SID {
      return Err(HostError::Untrusted);
    }

    // A server that exited while checked cannot have been replaced by a
    // different process on this same connected handle; re-read to be sure.
    let mut current_pid = 0u32;
    unsafe { GetNamedPipeServerProcessId(pipe, &mut current_pid) }.map_err(|_| HostError::Untrusted)?;

    if current_pid != server_pid {
      return Err(HostError::Untrusted);
    }

    Ok(())
  }
}

#[cfg(windows)]
mod token {
  use std::ffi::c_void;
  use std::mem::{align_of, size_of};

  use windows::Win32::Foundation::{HANDLE, HLOCAL, LocalFree};
  use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
  use windows::Win32::Security::{GetTokenInformation, TOKEN_USER, TokenUser};
  use windows::core::PWSTR;

  /// The string SID of a token's user, for comparison with stored SIDs.
  pub(crate) fn user_sid(token: HANDLE) -> Result<String, ()> {
    let mut bytes = 0u32;
    // SAFETY: A null output buffer requests the required TOKEN_USER size.
    let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut bytes) };

    if bytes < size_of::<TOKEN_USER>() as u32 || bytes > 64 * 1024 || align_of::<TOKEN_USER>() > align_of::<usize>() {
      return Err(());
    }

    // TOKEN_USER has pointer alignment; a word buffer satisfies it.
    let mut data = vec![0usize; (bytes as usize).div_ceil(size_of::<usize>())];
    // SAFETY: The aligned buffer holds at least `bytes` bytes.
    unsafe { GetTokenInformation(token, TokenUser, Some(data.as_mut_ptr().cast()), bytes, &mut bytes) }.map_err(|_| ())?;
    // SAFETY: Windows initialized an aligned TOKEN_USER at the buffer start;
    // its SID points into the same live buffer.
    let user = unsafe { data.as_ptr().cast::<TOKEN_USER>().read() };

    if user.User.Sid.0.is_null() {
      return Err(());
    }

    let mut raw = PWSTR::null();
    // SAFETY: The SID is live in `data`; Windows allocates one LocalAlloc string.
    unsafe { ConvertSidToStringSidW(user.User.Sid, &mut raw) }.map_err(|_| ())?;

    if raw.is_null() {
      return Err(());
    }

    // SAFETY: The returned string is NUL-terminated and live until LocalFree.
    let sid = unsafe { raw.to_string() }.map_err(|_| ());
    // SAFETY: Release exactly the allocation returned by the conversion.
    unsafe { LocalFree(HLOCAL(raw.0.cast::<c_void>())) };
    sid
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn session(lock_state: ConsoleLockState) -> ConsoleSession {
    ConsoleSession {
      session_id: 5,
      logon_time: 133_000_000_000,
      account_sid: "S-1-5-21-1-2-3-1001".into(),
      domain: "DESKTOP".into(),
      user: "user".into(),
      lock_state,
    }
  }

  #[test]
  fn account_requests_are_served_only_for_the_callers_own_sid() {
    assert_eq!(authorize("S-1-5-21-1-2-3-1001", "S-1-5-21-1-2-3-1001"), Ok(()));
    assert_eq!(authorize("S-1-5-21-1-2-3-1002", "S-1-5-21-1-2-3-1001"), Err(HostError::Unauthorized));
    // LocalSystem is the host's own identity, never a daemon account.
    assert_eq!(authorize(LOCAL_SYSTEM_SID, LOCAL_SYSTEM_SID), Err(HostError::Unauthorized));
    assert_eq!(authorize("not-a-sid", "not-a-sid"), Err(HostError::Unauthorized));
  }

  #[test]
  fn observed_console_round_trips_without_loss() {
    for state in [
      ConsoleLockState::Locked,
      ConsoleLockState::Usable,
      ConsoleLockState::Unknown,
    ] {
      let expected = session(state);
      let decoded = wire::decode_observed(&wire::encode_observed(Some(&expected)).unwrap()).unwrap().unwrap();

      assert_eq!(decoded, expected);
    }

    assert_eq!(wire::decode_observed(&wire::encode_observed(None).unwrap()).unwrap(), None);
  }

  #[test]
  fn target_carries_only_the_login_identity() {
    let target = wire::decode_target(&wire::encode_target(&session(ConsoleLockState::Locked)).unwrap()).unwrap();

    assert_eq!(
      target,
      wire::Target {
        session_id: 5,
        logon_time: 133_000_000_000,
        account_sid: "S-1-5-21-1-2-3-1001".into(),
      }
    );
  }

  #[test]
  fn trailing_or_truncated_payloads_are_rejected() {
    let mut bytes = wire::encode_target(&session(ConsoleLockState::Locked)).unwrap();
    bytes.push(0);

    assert_eq!(wire::decode_target(&bytes), Err(HostError::InvalidRequest));
    assert_eq!(wire::decode_target(&bytes[..6]), Err(HostError::InvalidRequest));
  }

  #[test]
  fn a_different_protocol_version_is_reported_as_incompatible() {
    let mut request = wire::encode_request(Operation::Observe, &[]).unwrap();
    let header: [u8; 9] = request[..9].try_into().unwrap();

    assert_eq!(wire::decode_request_header(&header), Ok((Operation::Observe, 0)));

    request[4] = request[4].wrapping_add(1);
    let header: [u8; 9] = request[..9].try_into().unwrap();

    assert_eq!(wire::decode_request_header(&header), Err(HostError::ProtocolUnsupported));
    assert_eq!(wire::decode_response_header(&header), Err(HostError::ProtocolUnsupported));
  }

  #[test]
  fn response_status_maps_back_to_the_same_error() {
    for error in [
      HostError::Untrusted,
      HostError::ProtocolUnsupported,
      HostError::InvalidRequest,
      HostError::Unauthorized,
      HostError::StaleSession,
      HostError::NotLocked,
      HostError::NotEnrolled,
      HostError::InvalidCredential,
      HostError::VaultUnavailable,
      HostError::Unverified,
    ] {
      let response = wire::encode_response(Err(error));
      let header: [u8; 9] = response[..9].try_into().unwrap();

      assert_eq!(wire::decode_response_header(&header), Err(error));
    }
  }

  #[test]
  fn unknown_operations_are_rejected_before_dispatch() {
    let mut request = wire::encode_request(Operation::Lock, &[]).unwrap();
    request[6] = 0;
    let header: [u8; 9] = request[..9].try_into().unwrap();

    assert_eq!(wire::decode_request_header(&header), Err(HostError::InvalidRequest));
  }
}
