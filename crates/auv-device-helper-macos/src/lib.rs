#![cfg(unix)]

//! Private, target-local transport for the macOS locked-session host.
//!
//! The remotely callable DeviceService never accepts a credential. Enrollment
//! and unlock reach this signed Aqua helper through a per-user Unix socket.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(target_os = "macos")]
use core_foundation::{base::TCFType, data::CFData};
#[cfg(target_os = "macos")]
use nix::sys::socket::{getsockopt, sockopt::LocalPeerToken};
#[cfg(target_os = "macos")]
use security_framework::os::macos::code_signing::{Flags, GuestAttributes, SecCode, SecRequirement};
#[cfg(target_os = "macos")]
use std::os::unix::fs::MetadataExt;

const MAGIC: &[u8; 4] = b"AUVE";
const VERSION: u8 = 1;
const MAX_PAYLOAD: usize = 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostError {
  Unavailable,
  Unauthorized,
  InvalidRequest,
  StaleSession,
  NotLocked,
  AlreadyLocked,
  VaultUnavailable,
  /// Legacy status from an installed helper without staged input diagnostics.
  InputUnavailable,
  /// Fixed, non-secret stage preserved over the private helper socket.
  InputUnavailableAt(InputFailure),
  // TODO(device-entry-macos-rejection): loginwindow currently exposes no
  // confirmed credential-rejection signal to this helper. Add a distinct
  // status only after an installed-host gate proves one; a timeout stays
  // OutcomeUnverified and must not suspend the enrollment.
  OutcomeUnverified,
}

/// Private helper transport uses the driver's fixed, non-secret input stage.
/// The paired API still exposes only `OUTCOME_UNVERIFIED`.
pub use auv_driver_macos::device_session_unlock::InputFailure;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
enum Operation {
  Enroll = 1,
  Probe = 2,
  Remove = 3,
  Unlock = 4,
  // NOTICE(device-entry-macos-lock-ipc): Existing operation bytes keep their
  // meaning. Deploy daemon and helper together before exposing Lock; an old
  // helper rejects this byte as an invalid request.
  Lock = 5,
}

impl TryFrom<u8> for Operation {
  type Error = HostError;

  fn try_from(value: u8) -> Result<Self, Self::Error> {
    match value {
      value if value == Self::Enroll as u8 => Ok(Self::Enroll),
      value if value == Self::Probe as u8 => Ok(Self::Probe),
      value if value == Self::Remove as u8 => Ok(Self::Remove),
      value if value == Self::Unlock as u8 => Ok(Self::Unlock),
      value if value == Self::Lock as u8 => Ok(Self::Lock),
      _ => Err(HostError::InvalidRequest),
    }
  }
}

/// The containing directory is created by the user-session helper with mode
/// 0700. Callers must resolve the target account's home directory using OS
/// account data, never a home path supplied by the remote Device request.
pub fn socket_path(home: &Path) -> PathBuf {
  home.join("Library").join("Application Support").join("AUV").join("device-entry").join("host.sock")
}

/// Enroll one credential into the installed helper's own login Keychain.
/// The target-local gRPC layer must have already authorized this OS account.
pub fn enroll(home: &Path, uid: u32, credential: &[u8]) -> Result<(), HostError> {
  if credential.is_empty() || credential.len() > MAX_PAYLOAD {
    return Err(HostError::InvalidRequest);
  }

  call(home, Operation::Enroll, uid, credential)
}

/// Read a stored item under the actual helper identity without disclosing it.
/// This is a local readiness probe, not proof of retrieval while locked.
pub fn probe_locked(home: &Path, uid: u32, selector: &str) -> Result<(), HostError> {
  if !selector.starts_with("macos:") || selector.len() > MAX_PAYLOAD {
    return Err(HostError::InvalidRequest);
  }

  call(home, Operation::Probe, uid, selector.as_bytes())
}

pub fn remove(home: &Path, uid: u32) -> Result<(), HostError> {
  call(home, Operation::Remove, uid, &[])
}

/// Attempt one unlock of exactly this existing macOS console session.
/// The helper independently checks UID, session UUID, and locked state before
/// secret retrieval and observes the same session becoming usable afterward.
pub fn unlock(home: &Path, uid: u32, selector: &str) -> Result<(), HostError> {
  if !selector.starts_with("macos:") || selector.len() > MAX_PAYLOAD {
    return Err(HostError::InvalidRequest);
  }

  call(home, Operation::Unlock, uid, selector.as_bytes())
}

/// Lock exactly this existing, usable macOS console session.
/// The helper sends the lock shortcut and independently reads back its state.
pub fn lock(home: &Path, uid: u32, selector: &str) -> Result<(), HostError> {
  if !selector.starts_with("macos:") || selector.len() > MAX_PAYLOAD {
    return Err(HostError::InvalidRequest);
  }

  call(home, Operation::Lock, uid, selector.as_bytes())
}

fn call(home: &Path, operation: Operation, uid: u32, payload: &[u8]) -> Result<(), HostError> {
  let mut stream = UnixStream::connect(socket_path(home)).map_err(|_| HostError::Unavailable)?;
  verify_installed_helper(&stream)?;
  // NOTICE(device-entry-macos-deadline): The helper exits at its 18-second
  // request deadline instead of replying for unfinished input. This longer
  // read timeout is only a backstop for a stopped helper process.
  stream.set_read_timeout(Some(Duration::from_secs(25))).map_err(|_| HostError::Unavailable)?;
  stream.set_write_timeout(Some(Duration::from_secs(20))).map_err(|_| HostError::Unavailable)?;
  let length = u16::try_from(payload.len()).map_err(|_| HostError::InvalidRequest)?;
  let mut header = [0_u8; 12];
  header[..4].copy_from_slice(MAGIC);
  header[4] = VERSION;
  header[5] = operation as u8;
  header[6..10].copy_from_slice(&uid.to_be_bytes());
  header[10..12].copy_from_slice(&length.to_be_bytes());
  stream.write_all(&header).map_err(|_| HostError::Unavailable)?;
  stream.write_all(payload).map_err(|_| HostError::Unavailable)?;
  let mut response = [0_u8; 1];
  stream.read_exact(&mut response).map_err(|_| unanswered(operation))?;
  decode_status(response[0])
}

/// A sent request without a status may already have posted unlock or lock
/// input, so the caller must not treat it as a clean failure. Other
/// operations have no OS input effect.
fn unanswered(operation: Operation) -> HostError {
  match operation {
    Operation::Unlock | Operation::Lock => HostError::OutcomeUnverified,
    Operation::Enroll | Operation::Probe | Operation::Remove => HostError::Unavailable,
  }
}

fn decode_status(status: u8) -> Result<(), HostError> {
  match status {
    0 => Ok(()),
    1 => Err(HostError::Unauthorized),
    2 => Err(HostError::InvalidRequest),
    3 => Err(HostError::StaleSession),
    4 => Err(HostError::NotLocked),
    5 => Err(HostError::VaultUnavailable),
    6 => Err(HostError::InputUnavailable),
    7 => Err(HostError::OutcomeUnverified),
    9 => Err(HostError::InputUnavailableAt(InputFailure::InvalidRequest)),
    10 => Err(HostError::InputUnavailableAt(InputFailure::IdentityMismatch)),
    11 => Err(HostError::InputUnavailableAt(InputFailure::PermissionMissing)),
    12 => Err(HostError::InputUnavailableAt(InputFailure::SessionChanged)),
    13 => Err(HostError::InputUnavailableAt(InputFailure::EventUnavailable)),
    14 => Err(HostError::InputUnavailableAt(InputFailure::WakeUnavailable)),
    15 => Err(HostError::InputUnavailableAt(InputFailure::FocusUnavailable)),
    16 => Err(HostError::InputUnavailableAt(InputFailure::FocusLost)),
    17 => Err(HostError::InputUnavailableAt(InputFailure::DeadlineExceeded)),
    18 => Err(HostError::AlreadyLocked),
    _ => Err(HostError::Unavailable),
  }
}

#[cfg(not(target_os = "macos"))]
fn verify_installed_helper(_stream: &UnixStream) -> Result<(), HostError> {
  Err(HostError::Unavailable)
}

#[cfg(target_os = "macos")]
fn verify_installed_helper(stream: &UnixStream) -> Result<(), HostError> {
  // A user-writable socket path alone cannot authenticate the process that
  // accepted it. Bind dynamic code validation to the kernel's socket peer
  // audit token, which includes a PID version, before sending a credential.
  const INSTALL_ROOT: &str = "/Library/Application Support/AUV";
  const EXPECTED_TEAM_ID: &str = "433DLLA855";
  // The installer pins the reviewed certificate Team ID in a root-owned
  // file, so the daemon and helper can be built independently.
  let root = Path::new(INSTALL_ROOT);
  let support = root.parent().ok_or(HostError::Unauthorized)?;
  let library = support.parent().ok_or(HostError::Unauthorized)?;
  let app = root.join("AUV Device Entry Host.app");
  let contents = app.join("Contents");
  let macos = contents.join("MacOS");
  let binary = macos.join("auv-device-helper-macos");
  let team_file = root.join("device-entry-host.team-id");

  for path in [
    library,
    support,
    root,
    app.as_path(),
    contents.as_path(),
    macos.as_path(),
    binary.as_path(),
    team_file.as_path(),
  ] {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| HostError::Unauthorized)?;
    let expected_type = if path == binary || path == team_file {
      metadata.file_type().is_file()
    } else {
      metadata.file_type().is_dir()
    };

    if metadata.uid() != 0 || metadata.mode() & 0o022 != 0 || !expected_type {
      return Err(HostError::Unauthorized);
    }
  }

  let team_id = std::fs::read_to_string(&team_file).map_err(|_| HostError::Unauthorized)?;
  let team_id = team_id.trim_end_matches('\n');

  if team_id != EXPECTED_TEAM_ID {
    return Err(HostError::Unauthorized);
  }

  let code = code_for_socket_peer(stream)?;
  let requirement: SecRequirement =
    format!("identifier \"dev.moeru.auv.device-entry-host\" and anchor apple generic and certificate leaf[subject.OU] = \"{team_id}\"")
      .parse()
      .map_err(|_| HostError::Unauthorized)?;
  code.check_validity(Flags::NONE, &requirement).map_err(|_| HostError::Unauthorized)?;
  let actual = code.path(Flags::NONE).ok().and_then(|url| url.to_path()).ok_or(HostError::Unauthorized)?;
  let actual = std::fs::canonicalize(actual).map_err(|_| HostError::Unauthorized)?;

  if actual != binary && actual != app {
    return Err(HostError::Unauthorized);
  }

  Ok(())
}

#[cfg(target_os = "macos")]
fn code_for_socket_peer(stream: &UnixStream) -> Result<SecCode, HostError> {
  // NOTICE: macOS `sys/un.h` defines LOCAL_PEERTOKEN as the socket peer's
  // audit token; Security.framework accepts kSecGuestAttributeAudit for
  // SecCodeCopyGuestWithAttributes. An unsupported lookup fails closed.
  // https://developer.apple.com/documentation/security/guest-attribute-dictionary-keys
  let token = getsockopt(stream, LocalPeerToken).map_err(|_| HostError::Unauthorized)?;
  // Audit tokens are opaque. Copy their native memory representation into
  // CFData for Security.framework without extracting/reusing a numeric PID.
  let mut bytes = [0_u8; 32];

  for (chunk, value) in bytes.chunks_exact_mut(4).zip(token.val) {
    chunk.copy_from_slice(&value.to_ne_bytes());
  }

  let token_data = CFData::from_buffer(&bytes);
  let mut attributes = GuestAttributes::new();
  attributes.set_audit_token(token_data.as_concrete_TypeRef());
  SecCode::copy_guest_with_attribues(None, &attributes, Flags::NONE).map_err(|_| HostError::Unauthorized)
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
  use super::*;

  #[test]
  fn socket_audit_token_identifies_the_connected_process() {
    let (left, _right) = UnixStream::pair().unwrap();
    let code = code_for_socket_peer(&left).unwrap();
    let path = code.path(Flags::NONE).unwrap().to_path().unwrap();

    assert_eq!(std::fs::canonicalize(path).unwrap(), std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap());
  }

  #[test]
  fn unanswered_unlock_is_unverified_not_a_clean_failure() {
    // The helper exits at its deadline without replying; any unlock input it
    // posted before that point may still have taken effect.
    assert_eq!(unanswered(Operation::Unlock), HostError::OutcomeUnverified);
    assert_eq!(unanswered(Operation::Probe), HostError::Unavailable);
  }
}

#[cfg(target_os = "macos")]
pub mod host;
