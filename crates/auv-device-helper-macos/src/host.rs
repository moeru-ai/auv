//! Per-user Aqua host for the existing locked macOS console session.

mod session;
mod vault;

use std::fs::{self, DirBuilder};
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rustix::process::{geteuid, getuid};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use zeroize::Zeroizing;

use crate::{HostError, InputFailure, MAGIC, MAX_PAYLOAD, Operation, SUPPORTED_PROTOCOLS, socket_path};

pub async fn serve() -> Result<(), HostError> {
  let uid = getuid().as_raw();

  if uid == 0 || geteuid().as_raw() != uid {
    return Err(HostError::Unauthorized);
  }

  let home = PathBuf::from(std::env::var_os("HOME").ok_or(HostError::Unavailable)?);
  prepare_socket_dir(&home, uid)?;
  let path = socket_path(&home);

  if let Ok(metadata) = fs::symlink_metadata(&path) {
    if !metadata.file_type().is_socket() || metadata.uid() != uid {
      return Err(HostError::Unauthorized);
    }

    fs::remove_file(&path).map_err(|_| HostError::Unavailable)?;
  }

  let listener = UnixListener::bind(&path).map_err(|_| HostError::Unavailable)?;
  fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).map_err(|_| HostError::Unavailable)?;
  // A serial accept loop prevents concurrent attempts for this one account.
  // The daemon still serializes at the Device policy boundary.
  loop {
    let (mut stream, _) = listener.accept().await.map_err(|_| HostError::Unavailable)?;
    let started = Instant::now();
    let deadline = started + REQUEST_DEADLINE;
    let result = match tokio::time::timeout_at(deadline.into(), read_request(&mut stream, uid)).await {
      Ok(Ok((operation, payload))) => {
        let home = home.clone();

        match run_before(deadline, move || execute(operation, &payload, &home, uid, started)).await {
          Some(result) => result,
          // NOTICE(device-entry-macos-deadline): Keychain and HID calls are
          // synchronous and cannot be canceled. Exiting is the only way to
          // guarantee no input lands after the daemon stops waiting. The
          // daemon reads EOF as an unverified outcome, and launchd KeepAlive
          // restarts this helper. Remove only if the native calls become
          // cancelable.
          None => std::process::exit(2),
        }
      }
      Ok(Err(error)) => Err(error),
      Err(_) => Err(HostError::Unavailable),
    };
    let status = match result {
      Ok(()) => 0,
      Err(error) => status(error),
    };
    let _ = stream.write_all(&[status]).await;
  }
}

// Covers socket reads, the unlock posting budget, and same-session readback
// in `session::unlock`. The daemon client's read timeout must stay longer.
const REQUEST_DEADLINE: Duration = Duration::from_secs(18);

/// Runs blocking helper work off the current-thread runtime so the deadline
/// can fire while that work is still executing. `None` means the work is
/// still running and the caller must not report any result for it.
async fn run_before<F>(deadline: Instant, work: F) -> Option<Result<(), HostError>>
where
  F: FnOnce() -> Result<(), HostError> + Send + 'static,
{
  match tokio::time::timeout_at(deadline.into(), tokio::task::spawn_blocking(work)).await {
    Ok(Ok(result)) => Some(result),
    Ok(Err(_)) => Some(Err(HostError::Unavailable)),
    Err(_) => None,
  }
}

fn prepare_socket_dir(home: &Path, uid: u32) -> Result<(), HostError> {
  if !home.is_absolute() {
    return Err(HostError::Unavailable);
  }

  let library = home.join("Library");
  let support = library.join("Application Support");
  let auv = support.join("AUV");
  let entry = auv.join("device-entry");

  for path in [home, &library, &support, &auv, &entry] {
    if !path.exists() {
      DirBuilder::new().mode(0o700).create(path).map_err(|_| HostError::Unavailable)?;
    }

    let metadata = fs::symlink_metadata(path).map_err(|_| HostError::Unavailable)?;

    if !metadata.file_type().is_dir() || metadata.uid() != uid || metadata.mode() & 0o022 != 0 {
      return Err(HostError::Unauthorized);
    }
  }

  // The private directory restricts discovery. The protocol admits only
  // the root daemon so every vault mutation has matching policy metadata.
  fs::set_permissions(entry, fs::Permissions::from_mode(0o700)).map_err(|_| HostError::Unavailable)
}

async fn read_request(stream: &mut UnixStream, uid: u32) -> Result<(Operation, Zeroizing<Vec<u8>>), HostError> {
  let peer = stream.peer_cred().map_err(|_| HostError::Unauthorized)?.uid();
  // Every operation must pass through the root daemon's local principal
  // check, metadata generation, account lock, and audit boundary. A user
  // process reaching its own socket cannot bypass those checks by directly
  // replacing/removing a credential or triggering locked-session input.
  if peer != 0 {
    return Err(HostError::Unauthorized);
  }

  let mut header = [0_u8; 12];
  stream.read_exact(&mut header).await.map_err(|_| HostError::InvalidRequest)?;
  let (operation, length) = decode_header(&header, uid)?;
  let mut payload = Zeroizing::new(vec![0_u8; length]);
  stream.read_exact(&mut payload).await.map_err(|_| HostError::InvalidRequest)?;
  Ok((operation, payload))
}

fn execute(operation: Operation, payload: &[u8], home: &Path, uid: u32, started: Instant) -> Result<(), HostError> {
  match operation {
    Operation::Enroll if !payload.is_empty() => {
      let secret = std::str::from_utf8(payload).map_err(|_| HostError::InvalidRequest)?;

      if secret.chars().any(char::is_control) {
        return Err(HostError::InvalidRequest);
      }

      vault::enroll(home, uid, payload)
    }
    Operation::Probe | Operation::Unlock | Operation::Lock if !payload.is_empty() => {
      let selector = std::str::from_utf8(payload).map_err(|_| HostError::InvalidRequest)?;

      match operation {
        Operation::Probe => session::probe_locked(home, uid, selector),
        Operation::Unlock => {
          // The native primitive clears retained input without a preliminary
          // Return. DeviceService verifies the same session after this helper
          // returns; see the installed gate in the macOS session reference.
          session::unlock(home, uid, selector, started)
        }
        Operation::Lock => session::lock(uid, selector, started),
        _ => unreachable!("only session operations enter this branch"),
      }
    }
    Operation::Remove if payload.is_empty() => vault::remove(home, uid),
    _ => Err(HostError::InvalidRequest),
  }
}

fn decode_header(header: &[u8; 12], uid: u32) -> Result<(Operation, usize), HostError> {
  if &header[..4] != MAGIC || u32::from_be_bytes(header[6..10].try_into().unwrap()) != uid {
    return Err(HostError::Unauthorized);
  }
  // A version outside the declared range is a deployment mismatch, not an
  // identity failure; report it distinctly so the daemon can say so.
  if !SUPPORTED_PROTOCOLS.contains(&header[4]) {
    return Err(HostError::ProtocolUnsupported);
  }

  let length = usize::from(u16::from_be_bytes(header[10..12].try_into().unwrap()));

  if length > MAX_PAYLOAD {
    return Err(HostError::InvalidRequest);
  }

  Ok((Operation::try_from(header[5])?, length))
}

fn status(error: HostError) -> u8 {
  match error {
    HostError::Unavailable => 8,
    HostError::Unauthorized => 1,
    HostError::InvalidRequest => 2,
    HostError::StaleSession => 3,
    HostError::NotLocked => 4,
    HostError::AlreadyLocked => 18,
    HostError::VaultUnavailable => 5,
    HostError::InputUnavailable => 6,
    HostError::InputUnavailableAt(InputFailure::InvalidRequest) => 9,
    HostError::InputUnavailableAt(InputFailure::IdentityMismatch) => 10,
    HostError::InputUnavailableAt(InputFailure::PermissionMissing) => 11,
    HostError::InputUnavailableAt(InputFailure::SessionChanged) => 12,
    HostError::InputUnavailableAt(InputFailure::EventUnavailable) => 13,
    HostError::InputUnavailableAt(InputFailure::WakeUnavailable) => 14,
    HostError::InputUnavailableAt(InputFailure::FocusUnavailable) => 15,
    HostError::InputUnavailableAt(InputFailure::FocusLost) => 16,
    HostError::InputUnavailableAt(InputFailure::DeadlineExceeded) => 17,
    HostError::InputUnavailableAt(InputFailure::Unavailable) => 6,
    HostError::OutcomeUnverified => 7,
    HostError::ProtocolUnsupported => 19,
    // The client detects revocation before connecting; the helper never
    // reports it, so it shares the generic unavailable byte.
    HostError::Revoked => 8,
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::PROTOCOL_VERSION;

  #[test]
  fn request_for_another_uid_is_rejected_before_vault_access() {
    let uid = getuid().as_raw();
    let mut header = [0_u8; 12];
    header[..4].copy_from_slice(MAGIC);
    header[4] = PROTOCOL_VERSION;
    header[5] = Operation::Probe as u8;
    header[6..10].copy_from_slice(&uid.wrapping_add(1).to_be_bytes());

    assert_eq!(decode_header(&header, uid), Err(HostError::Unauthorized));
  }

  #[test]
  fn unknown_operation_is_an_invalid_request() {
    let uid = getuid().as_raw();
    let mut header = [0_u8; 12];
    header[..4].copy_from_slice(MAGIC);
    header[4] = PROTOCOL_VERSION;
    header[5] = 6;
    header[6..10].copy_from_slice(&uid.to_be_bytes());

    assert_eq!(decode_header(&header, uid), Err(HostError::InvalidRequest));
    assert_eq!(status(HostError::InvalidRequest), 2);
  }

  #[test]
  fn lock_operation_and_already_locked_status_round_trip() {
    let uid = getuid().as_raw();
    let mut header = [0_u8; 12];
    header[..4].copy_from_slice(MAGIC);
    header[4] = PROTOCOL_VERSION;
    header[6..10].copy_from_slice(&uid.to_be_bytes());
    header[5] = Operation::Lock as u8;

    assert_eq!(decode_header(&header, uid), Ok((Operation::Lock, 0)));
    assert_eq!(status(HostError::AlreadyLocked), 18);
    assert_eq!(crate::decode_status(18), Err(HostError::AlreadyLocked));
  }

  // ROOT CAUSE:
  //
  // A request header with an unsupported protocol version was rejected as
  // `Unauthorized`, so a daemon/helper deployment mismatch looked like an
  // identity failure. It now round-trips as `ProtocolUnsupported`.
  #[test]
  fn unsupported_protocol_version_is_reported_distinctly() {
    let uid = getuid().as_raw();
    let mut header = [0_u8; 12];
    header[..4].copy_from_slice(MAGIC);
    header[4] = SUPPORTED_PROTOCOLS.end() + 1;
    header[5] = Operation::Probe as u8;
    header[6..10].copy_from_slice(&uid.to_be_bytes());

    assert_eq!(decode_header(&header, uid), Err(HostError::ProtocolUnsupported));
    assert_eq!(crate::decode_status(status(HostError::ProtocolUnsupported)), Err(HostError::ProtocolUnsupported));
  }

  #[test]
  fn unsupported_protocol_from_another_uid_is_still_unauthorized() {
    let uid = getuid().as_raw();
    let mut header = [0_u8; 12];
    header[..4].copy_from_slice(MAGIC);
    header[4] = SUPPORTED_PROTOCOLS.end() + 1;
    header[5] = Operation::Probe as u8;
    header[6..10].copy_from_slice(&uid.wrapping_add(1).to_be_bytes());

    assert_eq!(decode_header(&header, uid), Err(HostError::Unauthorized));
  }

  #[test]
  fn private_unlock_stages_survive_the_helper_response_byte() {
    for stage in [
      InputFailure::InvalidRequest,
      InputFailure::IdentityMismatch,
      InputFailure::PermissionMissing,
      InputFailure::SessionChanged,
      InputFailure::EventUnavailable,
      InputFailure::WakeUnavailable,
      InputFailure::FocusUnavailable,
      InputFailure::FocusLost,
      InputFailure::DeadlineExceeded,
    ] {
      let error = HostError::InputUnavailableAt(stage);

      assert_eq!(crate::decode_status(status(error)), Err(error));
    }

    assert_eq!(crate::decode_status(6), Err(HostError::InputUnavailable));
  }

  // ROOT CAUSE:
  //
  // If a Keychain read or HID post blocked, the 18-second request timeout
  // never fired because the helper ran that synchronous work inline on its
  // current-thread runtime.
  //
  // Before the fix, the daemon gave up at its own read timeout and released
  // its account lock while the helper could still type the credential.
  // The fix runs the work off the runtime so the deadline fires, and the
  // helper then exits instead of reporting a result for unfinished input.
  #[tokio::test]
  async fn deadline_fires_while_blocking_work_is_still_running() {
    let (release, blocked) = std::sync::mpsc::channel::<()>();
    let started = Instant::now();
    let result = run_before(started + Duration::from_millis(50), move || {
      let _ = blocked.recv_timeout(Duration::from_secs(10));
      Ok(())
    })
    .await;

    assert!(result.is_none());
    assert!(started.elapsed() < Duration::from_secs(5));

    drop(release);
  }

  #[tokio::test]
  async fn completed_blocking_work_reports_its_result() {
    let result = run_before(Instant::now() + Duration::from_secs(5), || Err(HostError::NotLocked)).await;

    assert_eq!(result, Some(Err(HostError::NotLocked)));
  }

  #[tokio::test]
  async fn same_uid_cannot_bypass_daemon_policy_for_any_operation() {
    let uid = getuid().as_raw();

    if uid == 0 {
      return;
    }

    for operation in [
      Operation::Enroll,
      Operation::Probe,
      Operation::Remove,
      Operation::Unlock,
      Operation::Lock,
    ] {
      let (mut client, mut server) = UnixStream::pair().unwrap();
      let mut header = [0_u8; 12];
      header[..4].copy_from_slice(MAGIC);
      header[4] = PROTOCOL_VERSION;
      header[5] = operation as u8;
      header[6..10].copy_from_slice(&uid.to_be_bytes());
      client.write_all(&header).await.unwrap();

      assert_eq!(read_request(&mut server, uid).await.map(|(operation, _)| operation), Err(HostError::Unauthorized));
    }
  }
}
