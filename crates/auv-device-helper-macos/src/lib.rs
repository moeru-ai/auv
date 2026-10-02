#![cfg(unix)]

//! Private, target-local transport for the macOS locked-session host.
//!
//! The remotely callable DeviceService never accepts a credential. Enrollment
//! and unlock reach this signed Aqua helper through a per-user Unix socket.

#[cfg(feature = "transport")]
use std::io::{Read, Write};
#[cfg(any(feature = "setup", feature = "transport"))]
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
#[cfg(feature = "transport")]
use std::time::Duration;

#[cfg(feature = "setup")]
pub mod setup;

#[cfg(all(target_os = "macos", any(feature = "setup", feature = "transport")))]
use core_foundation::{base::TCFType, data::CFData};
#[cfg(all(target_os = "macos", any(feature = "setup", feature = "transport")))]
use nix::sys::socket::{getsockopt, sockopt::LocalPeerToken};
#[cfg(all(target_os = "macos", any(feature = "setup", feature = "transport")))]
use security_framework::os::macos::code_signing::{Flags, GuestAttributes, SecCode, SecRequirement};
#[cfg(all(target_os = "macos", any(feature = "setup", feature = "transport")))]
use std::os::unix::fs::MetadataExt;

#[cfg(feature = "transport")]
const MAGIC: &[u8; 4] = b"AUVE";
/// Wire protocol this build's daemon client sends in every request header.
///
/// Setup frontends compare it with the installed helper's declared range;
/// crate versions never decide whether a helper is usable. Version 1 covers
/// operations 1–5 (`Enroll` through `Lock`). Add operations by raising
/// `SUPPORTED_PROTOCOLS`' end, and drop old daemons only by raising its start.
/// See `docs/ai/references/session-api/2026-10-02-macos-helper-protocol-compatibility.md`.
#[cfg(any(feature = "setup", feature = "transport"))]
pub(crate) const PROTOCOL_VERSION: u8 = 1;

/// Wire protocols this build's helper accepts. `package/Info.plist` declares
/// the same range as `AUVHelperProtocolMin` / `AUVHelperProtocolMax` so setup
/// can read it from the signed bundle without executing the helper.
#[cfg(any(feature = "host", all(test, feature = "setup")))]
pub(crate) const SUPPORTED_PROTOCOLS: std::ops::RangeInclusive<u8> = 1..=1;
#[cfg(feature = "transport")]
const MAX_PAYLOAD: usize = 1024;

#[cfg(all(target_os = "macos", any(feature = "setup", feature = "transport")))]
pub(crate) const BUNDLE_ID: &str = "ai.moeru.auv.helper";
#[cfg(all(target_os = "macos", any(feature = "setup", feature = "transport")))]
pub(crate) const EXPECTED_TEAM_ID: &str = "433DLLA855";
#[cfg(all(target_os = "macos", any(feature = "setup", feature = "transport")))]
pub(crate) const LAUNCH_AGENT_LABEL: &str = "ai.moeru.auv.helper";

/// Oldest helper security epoch this build trusts.
///
/// Each helper declares its epoch as the signed Info.plist string
/// `AUVHelperSecurityEpoch`. To revoke vulnerable helpers, raise the epoch in
/// `package/Info.plist` and this minimum together: daemons then reject every
/// older signed helper through the code requirement, and setup replaces it.
/// Older daemons keep accepting the newer helper, so revocation never blocks
/// a rolling update. Unlike `SUPPORTED_PROTOCOLS`, the epoch is a trust
/// decision, not a capability.
#[cfg(all(target_os = "macos", any(feature = "setup", feature = "transport")))]
pub(crate) const MIN_SECURITY_EPOCH: u32 = 1;

/// Code requirement for the signed helper's identity: bundle identifier and
/// pinned Team ID. `package/package.sh` checks the same identity with
/// `codesign --test-requirement` after signing.
#[cfg(all(target_os = "macos", any(feature = "setup", feature = "transport")))]
pub(crate) fn helper_identity_requirement() -> Result<SecRequirement, security_framework::base::Error> {
  helper_identity().parse()
}

/// Identity requirement plus `MIN_SECURITY_EPOCH`, for code the daemon trusts.
///
/// NOTICE(helper-security-epoch-requirement): The requirement language
/// compares a quoted constant with a string Info.plist value numerically
/// (`"10" >= "2"` holds), but never matches an integer-typed value, so the
/// Info.plist entry must be a `<string>`. Verified with
/// `codesign -v -R='info[AUVHelperSecurityEpoch] >= "2"'` on macOS 26.3.
#[cfg(all(target_os = "macos", any(feature = "setup", feature = "transport")))]
pub(crate) fn helper_requirement() -> Result<SecRequirement, security_framework::base::Error> {
  format!("{} and {}", helper_identity(), security_epoch_clause(MIN_SECURITY_EPOCH)).parse()
}

#[cfg(all(target_os = "macos", any(feature = "setup", feature = "transport")))]
fn security_epoch_clause(minimum: u32) -> String {
  format!("info[AUVHelperSecurityEpoch] >= \"{minimum}\"")
}

#[cfg(all(target_os = "macos", any(feature = "setup", feature = "transport")))]
fn helper_identity() -> String {
  format!("identifier \"{BUNDLE_ID}\" and anchor apple generic and certificate leaf[subject.OU] = \"{EXPECTED_TEAM_ID}\"")
}

/// ServiceManagement registration state for the helper's LaunchAgent.
///
/// The helper's `--service-management-*` commands print `as_str` on stdout
/// and setup parses it, so both sides of that process boundary share this
/// type instead of mirroring string tables.
#[cfg(all(target_os = "macos", any(feature = "setup", feature = "host")))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceStatus {
  NotRegistered,
  Enabled,
  RequiresApproval,
  NotFound,
}

#[cfg(all(target_os = "macos", any(feature = "setup", feature = "host")))]
impl ServiceStatus {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::NotRegistered => "not-registered",
      Self::Enabled => "enabled",
      Self::RequiresApproval => "requires-approval",
      Self::NotFound => "not-found",
    }
  }

  pub fn parse(value: &str) -> Option<Self> {
    [
      Self::NotRegistered,
      Self::Enabled,
      Self::RequiresApproval,
      Self::NotFound,
    ]
    .into_iter()
    .find(|status| status.as_str() == value)
  }

  /// SMAppService reports `not-found` rather than `not-registered` after the
  /// final registration is removed; both mean no job can launch.
  pub fn is_unregistered(self) -> bool {
    matches!(self, Self::NotRegistered | Self::NotFound)
  }
}

#[cfg(all(target_os = "macos", any(feature = "setup", feature = "transport")))]
pub(crate) struct InstalledLayout {
  pub(crate) app: PathBuf,
  pub(crate) binary: PathBuf,
  pub(crate) launch_agent: PathBuf,
}

#[cfg(all(target_os = "macos", any(feature = "setup", feature = "transport")))]
pub(crate) fn installed_layout(home: &Path) -> InstalledLayout {
  let root = home.join("Library").join("Application Support").join("AUV");
  let app = root.join("AUV Helper.app");
  InstalledLayout {
    binary: app.join("Contents").join("MacOS").join("auv-device-helper-macos"),
    launch_agent: app.join("Contents").join("Library").join("LaunchAgents").join(format!("{LAUNCH_AGENT_LABEL}.plist")),
    app,
  }
}

#[cfg(all(target_os = "macos", any(feature = "setup", feature = "transport")))]
pub(crate) fn verify_installed_files(home: &Path) -> Result<InstalledLayout, IdentityError> {
  let layout = installed_layout(home);
  let root = layout.app.parent().ok_or(IdentityError::Mismatch)?;
  let contents = layout.app.join("Contents");
  let macos = contents.join("MacOS");
  let library = contents.join("Library");
  let launch_agents = library.join("LaunchAgents");
  let uid = std::fs::symlink_metadata(home).map_err(|_| IdentityError::Mismatch)?.uid();

  for path in [
    root,
    layout.app.as_path(),
    contents.as_path(),
    macos.as_path(),
    library.as_path(),
    launch_agents.as_path(),
    layout.binary.as_path(),
    layout.launch_agent.as_path(),
  ] {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| IdentityError::Mismatch)?;
    let expected_type = if path == layout.binary || path == layout.launch_agent {
      metadata.file_type().is_file()
    } else {
      metadata.file_type().is_dir()
    };

    if metadata.uid() != uid || metadata.mode() & 0o022 != 0 || !expected_type {
      return Err(IdentityError::Mismatch);
    }
  }

  Ok(layout)
}

#[cfg(any(feature = "setup", feature = "transport"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IdentityError {
  PeerUnavailable,
  Mismatch,
  /// A genuine helper whose security epoch is below `MIN_SECURITY_EPOCH`.
  Revoked,
}

#[cfg(feature = "transport")]
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
  /// The helper does not accept this daemon's wire protocol version.
  ProtocolUnsupported,
  /// The installed helper is genuine but below `MIN_SECURITY_EPOCH`.
  /// Detected by the daemon client before sending; never a wire status.
  Revoked,
}

/// Private helper transport uses the driver's fixed, non-secret input stage.
/// The paired API still exposes only `OUTCOME_UNVERIFIED`.
#[cfg(feature = "transport")]
pub use auv_driver_macos::device_session_unlock::InputFailure;

#[cfg(feature = "transport")]
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

#[cfg(feature = "transport")]
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
#[cfg(feature = "transport")]
pub fn enroll(home: &Path, uid: u32, credential: &[u8]) -> Result<(), HostError> {
  if credential.is_empty() || credential.len() > MAX_PAYLOAD {
    return Err(HostError::InvalidRequest);
  }

  call(home, Operation::Enroll, uid, credential)
}

/// Read a stored item under the actual helper identity without disclosing it.
/// This is a local readiness probe, not proof of retrieval while locked.
#[cfg(feature = "transport")]
pub fn probe_locked(home: &Path, uid: u32, selector: &str) -> Result<(), HostError> {
  if !selector.starts_with("macos:") || selector.len() > MAX_PAYLOAD {
    return Err(HostError::InvalidRequest);
  }

  call(home, Operation::Probe, uid, selector.as_bytes())
}

#[cfg(feature = "transport")]
pub fn remove(home: &Path, uid: u32) -> Result<(), HostError> {
  call(home, Operation::Remove, uid, &[])
}

/// Attempt one unlock of exactly this existing macOS console session.
/// The helper independently checks UID, session UUID, and locked state before
/// secret retrieval and observes the same session becoming usable afterward.
#[cfg(feature = "transport")]
pub fn unlock(home: &Path, uid: u32, selector: &str) -> Result<(), HostError> {
  if !selector.starts_with("macos:") || selector.len() > MAX_PAYLOAD {
    return Err(HostError::InvalidRequest);
  }

  call(home, Operation::Unlock, uid, selector.as_bytes())
}

/// Lock exactly this existing, usable macOS console session.
/// The helper sends the lock shortcut and independently reads back its state.
#[cfg(feature = "transport")]
pub fn lock(home: &Path, uid: u32, selector: &str) -> Result<(), HostError> {
  if !selector.starts_with("macos:") || selector.len() > MAX_PAYLOAD {
    return Err(HostError::InvalidRequest);
  }

  call(home, Operation::Lock, uid, selector.as_bytes())
}

#[cfg(feature = "transport")]
fn call(home: &Path, operation: Operation, uid: u32, payload: &[u8]) -> Result<(), HostError> {
  let mut stream = UnixStream::connect(socket_path(home)).map_err(|_| HostError::Unavailable)?;
  verify_installed_helper(&stream, home).map_err(|error| match error {
    IdentityError::PeerUnavailable => HostError::Unavailable,
    IdentityError::Mismatch => HostError::Unauthorized,
    IdentityError::Revoked => HostError::Revoked,
  })?;
  // NOTICE(device-entry-macos-deadline): The helper exits at its 18-second
  // request deadline instead of replying for unfinished input. This longer
  // read timeout is only a backstop for a stopped helper process.
  stream.set_read_timeout(Some(Duration::from_secs(25))).map_err(|_| HostError::Unavailable)?;
  stream.set_write_timeout(Some(Duration::from_secs(20))).map_err(|_| HostError::Unavailable)?;
  let length = u16::try_from(payload.len()).map_err(|_| HostError::InvalidRequest)?;
  let mut header = [0_u8; 12];
  header[..4].copy_from_slice(MAGIC);
  header[4] = PROTOCOL_VERSION;
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
#[cfg(feature = "transport")]
fn unanswered(operation: Operation) -> HostError {
  match operation {
    Operation::Unlock | Operation::Lock => HostError::OutcomeUnverified,
    Operation::Enroll | Operation::Probe | Operation::Remove => HostError::Unavailable,
  }
}

#[cfg(feature = "transport")]
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
    19 => Err(HostError::ProtocolUnsupported),
    _ => Err(HostError::Unavailable),
  }
}

#[cfg(all(
  not(target_os = "macos"),
  any(feature = "setup", feature = "transport")
))]
fn verify_installed_helper(_stream: &UnixStream, _home: &Path) -> Result<(), IdentityError> {
  Err(IdentityError::PeerUnavailable)
}

#[cfg(all(target_os = "macos", any(feature = "setup", feature = "transport")))]
fn verify_installed_helper(stream: &UnixStream, home: &Path) -> Result<(), IdentityError> {
  // A user-writable socket path alone cannot authenticate the process that
  // accepted it. Bind dynamic code validation to the kernel's socket peer
  // audit token, which includes a PID version, before sending a credential.
  // The pinned Team ID and designated requirement reject modifications to
  // the per-user app even though its containing directory is user-owned.
  // The security epoch rejects older signed helpers once they are revoked;
  // see `MIN_SECURITY_EPOCH`.
  let layout = verify_installed_files(home)?;

  let code = code_for_socket_peer(stream)?;
  let identity = helper_identity_requirement().map_err(|_| IdentityError::Mismatch)?;
  code.check_validity(Flags::NONE, &identity).map_err(|_| IdentityError::Mismatch)?;
  // Checked separately so a revoked genuine helper is reported as such
  // instead of looking like a foreign process.
  let trusted = helper_requirement().map_err(|_| IdentityError::Mismatch)?;
  code.check_validity(Flags::NONE, &trusted).map_err(|_| IdentityError::Revoked)?;
  let actual = code.path(Flags::NONE).ok().and_then(|url| url.to_path()).ok_or(IdentityError::Mismatch)?;

  if !installed_helper_path_matches(&actual, &layout)? {
    return Err(IdentityError::Mismatch);
  }

  Ok(())
}

#[cfg(all(target_os = "macos", any(feature = "setup", feature = "transport")))]
fn installed_helper_path_matches(actual: &Path, layout: &InstalledLayout) -> Result<bool, IdentityError> {
  let actual = std::fs::canonicalize(actual).map_err(|_| IdentityError::Mismatch)?;
  let binary = std::fs::canonicalize(&layout.binary).map_err(|_| IdentityError::Mismatch)?;
  let app = std::fs::canonicalize(&layout.app).map_err(|_| IdentityError::Mismatch)?;
  Ok(actual == binary || actual == app)
}

#[cfg(all(target_os = "macos", any(feature = "setup", feature = "transport")))]
fn code_for_socket_peer(stream: &UnixStream) -> Result<SecCode, IdentityError> {
  // NOTICE: macOS `sys/un.h` defines LOCAL_PEERTOKEN as the socket peer's
  // audit token; Security.framework accepts kSecGuestAttributeAudit for
  // SecCodeCopyGuestWithAttributes. An unsupported lookup fails closed.
  // https://developer.apple.com/documentation/security/guest-attribute-dictionary-keys
  // NOTICE: A client may finish `connect` while this helper's serial accept
  // loop is still draining an earlier connection. macOS returns ENOTCONN for
  // LOCAL_PEERTOKEN in that short window. Retry only that transient error;
  // remove this workaround if Darwin guarantees the token before `accept`.
  let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
  let token = loop {
    match getsockopt(stream, LocalPeerToken) {
      Ok(token) => break token,
      Err(nix::errno::Errno::ENOTCONN) if std::time::Instant::now() < deadline => {
        std::thread::sleep(std::time::Duration::from_millis(10));
      }
      Err(_) => return Err(IdentityError::PeerUnavailable),
    }
  };
  // Audit tokens are opaque. Copy their native memory representation into
  // CFData for Security.framework without extracting/reusing a numeric PID.
  let mut bytes = [0_u8; 32];

  for (chunk, value) in bytes.chunks_exact_mut(4).zip(token.val) {
    chunk.copy_from_slice(&value.to_ne_bytes());
  }

  let token_data = CFData::from_buffer(&bytes);
  let mut attributes = GuestAttributes::new();
  attributes.set_audit_token(token_data.as_concrete_TypeRef());
  SecCode::copy_guest_with_attribues(None, &attributes, Flags::NONE).map_err(|_| IdentityError::Mismatch)
}

#[cfg(all(
  test,
  target_os = "macos",
  any(feature = "setup", feature = "transport")
))]
mod tests {
  use super::*;

  #[test]
  fn socket_audit_token_identifies_the_connected_process() {
    let (left, _right) = UnixStream::pair().unwrap();
    let code = code_for_socket_peer(&left).unwrap();
    let path = code.path(Flags::NONE).unwrap().to_path().unwrap();

    assert_eq!(std::fs::canonicalize(path).unwrap(), std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap());
  }

  // ROOT CAUSE:
  //
  // A Unix client can finish `connect` before the serial helper accepts that
  // connection. macOS returns ENOTCONN for LOCAL_PEERTOKEN during that window,
  // which previously made a healthy helper intermittently look unauthorized.
  #[test]
  fn socket_audit_token_waits_for_the_server_to_accept() {
    use std::os::unix::net::UnixListener;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("peer.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let blocker = UnixStream::connect(&socket).unwrap();
    let (accepted_tx, accepted_rx) = mpsc::channel();
    let accepting = thread::spawn(move || {
      let first = listener.accept().unwrap().0;
      accepted_tx.send(()).unwrap();
      thread::sleep(Duration::from_millis(100));
      drop(first);
      listener.accept().unwrap().0
    });
    accepted_rx.recv().unwrap();
    let client = UnixStream::connect(&socket).unwrap();

    let code = code_for_socket_peer(&client).unwrap();
    let accepted = accepting.join().unwrap();
    let path = code.path(Flags::NONE).unwrap().to_path().unwrap();

    drop(accepted);
    drop(blocker);
    assert_eq!(std::fs::canonicalize(path).unwrap(), std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap());
  }

  // ROOT CAUSE:
  //
  // Security.framework returns a canonical peer path, while the expected app
  // path was previously compared in its unresolved home-directory form. A
  // symlinked home therefore rejected the correctly installed helper.
  #[test]
  fn installed_helper_path_accepts_a_symlinked_home_directory() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let real_home = root.path().join("real-home");
    let linked_home = root.path().join("linked-home");
    let real_layout = installed_layout(&real_home);
    std::fs::create_dir_all(real_layout.binary.parent().unwrap()).unwrap();
    std::fs::write(&real_layout.binary, []).unwrap();
    symlink(&real_home, &linked_home).unwrap();

    let linked_layout = installed_layout(&linked_home);
    let actual = std::fs::canonicalize(&real_layout.binary).unwrap();
    assert!(installed_helper_path_matches(&actual, &linked_layout).unwrap());
  }

  // Guards NOTICE(helper-security-epoch-requirement): revocation relies on the
  // requirement language comparing string epochs numerically.
  #[test]
  fn security_epoch_clause_compares_string_epochs_numerically() {
    use security_framework::os::macos::code_signing::SecStaticCode;

    let root = tempfile::tempdir().unwrap();
    let requirement: SecRequirement = security_epoch_clause(2).parse().unwrap();
    let satisfies = |name: &str, epoch: &str| {
      let app = root.path().join(format!("{name}.app"));
      let macos = app.join("Contents").join("MacOS");
      std::fs::create_dir_all(&macos).unwrap();
      std::fs::copy("/usr/bin/true", macos.join("probe")).unwrap();
      std::fs::write(
        app.join("Contents").join("Info.plist"),
        format!(
          "<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>probe</string><key>CFBundleIdentifier</key><string>test.auv.epoch</string><key>AUVHelperSecurityEpoch</key>{epoch}</dict></plist>"
        ),
      )
      .unwrap();
      let signed = std::process::Command::new("/usr/bin/codesign").args(["--force", "--sign", "-"]).arg(&app).output().unwrap();
      assert!(signed.status.success(), "{}", String::from_utf8_lossy(&signed.stderr));
      let url = core_foundation::url::CFURL::from_path(&app, true).unwrap();
      SecStaticCode::from_path(&url, Flags::NONE).unwrap().check_validity(Flags::NONE, &requirement).is_ok()
    };

    assert!(!satisfies("one", "<string>1</string>"));
    assert!(satisfies("two", "<string>2</string>"));
    assert!(satisfies("ten", "<string>10</string>"), "epochs must compare numerically, not lexically");
    assert!(!satisfies("integer", "<integer>10</integer>"), "integer-typed epochs never match");
  }

  #[test]
  #[cfg(feature = "transport")]
  fn unanswered_unlock_is_unverified_not_a_clean_failure() {
    // The helper exits at its deadline without replying; any unlock input it
    // posted before that point may still have taken effect.
    assert_eq!(unanswered(Operation::Unlock), HostError::OutcomeUnverified);
    assert_eq!(unanswered(Operation::Probe), HostError::Unavailable);
  }
}

#[cfg(all(target_os = "macos", feature = "host"))]
pub mod host;

#[cfg(all(target_os = "macos", feature = "host"))]
pub mod service_management;
