#![cfg(unix)]

//! Private, target-local transport for the macOS locked-session host.
//!
//! The remotely callable DeviceService never accepts a credential. Enrollment
//! and unlock reach this signed Aqua helper through a per-user Unix socket.

#[cfg(feature = "transport")]
use std::io::{Read, Write};
#[cfg(any(feature = "setup", feature = "transport"))]
use std::os::unix::net::UnixStream;
use std::path::Path;
#[cfg(target_os = "macos")]
use std::path::PathBuf;
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

/// Bundle identifier of the official AUV helper. The LaunchAgent label and
/// plist name always equal the helper's bundle identifier.
#[cfg(all(target_os = "macos", any(feature = "setup", feature = "transport")))]
pub(crate) const OFFICIAL_BUNDLE_ID: &str = "ai.moeru.auv.helper";
#[cfg(all(target_os = "macos", any(feature = "setup", feature = "transport")))]
const OFFICIAL_TEAM_ID: &str = "433DLLA855";

/// Names an unpacked, signed helper app shipped by the frontend, for example
/// inside an application that embeds AUV. When set, AUV installs and trusts
/// that app instead of the official AUV Helper; see [`HelperIdentity`].
pub const HELPER_APP_ENV: &str = "AUV_MACOS_HELPER_APP";

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

/// Signed identity of the helper app this frontend installs and trusts.
///
/// Without [`HELPER_APP_ENV`], this is the official `AUV Helper.app` signed by
/// the AUV Team ID and installed from the archive embedded in release builds.
/// With it, the bundle identifier and Team ID come from the shipped app's own
/// valid Apple-issued signature, so an application embedding AUV can ship a
/// helper under its own name, icon, and signing team without rebuilding AUV.
///
/// NOTICE(helper-identity-trust-anchor): The trusted identity is whatever the
/// process launching AUV selects. That is no weaker than before: a caller who
/// controls this environment already controls the daemon that sends the
/// enrollment credential. The identity still pins one bundle identifier and
/// Team ID, so another process on the user's socket path is rejected.
#[cfg(all(target_os = "macos", any(feature = "setup", feature = "transport")))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HelperIdentity {
  pub(crate) bundle_id: String,
  pub(crate) team_id: String,
  /// Bundle file name, for example `AUV Helper.app`.
  pub(crate) app_name: String,
  /// Directory under `~/Library/Application Support` holding the installed app
  /// and its private socket.
  support_dir: String,
  /// Unpacked app shipped by the frontend; `None` installs the embedded archive.
  pub(crate) source: Option<PathBuf>,
}

#[cfg(all(target_os = "macos", any(feature = "setup", feature = "transport")))]
impl HelperIdentity {
  pub(crate) fn official() -> Self {
    Self {
      bundle_id: OFFICIAL_BUNDLE_ID.to_string(),
      team_id: OFFICIAL_TEAM_ID.to_string(),
      app_name: "AUV Helper.app".to_string(),
      support_dir: "AUV".to_string(),
      source: None,
    }
  }

  /// Read the identity of a shipped helper app from its code signature.
  ///
  /// The app must be validly signed by an Apple-issued certificate with a
  /// Team ID. It installs under `~/Library/Application Support/<bundle id>`,
  /// so it never shares a socket or bundle path with another helper.
  pub(crate) fn from_app(app: &Path) -> Result<Self, String> {
    use core_foundation::url::CFURL;
    use security_framework::os::macos::code_signing::SecStaticCode;

    if !app.is_absolute() {
      return Err("the helper app path must be absolute".to_string());
    }
    let app_name = app
      .file_name()
      .and_then(|name| name.to_str())
      .filter(|name| name.len() > ".app".len() && name.ends_with(".app"))
      .ok_or_else(|| "the helper app path must name an .app bundle".to_string())?
      .to_string();
    let url = CFURL::from_path(app, true).ok_or_else(|| "the helper app path is not a file URL".to_string())?;
    let code = SecStaticCode::from_path(&url, Flags::NONE).map_err(|error| format!("cannot inspect the helper app signature ({error})"))?;
    let anchor: SecRequirement = "anchor apple generic".parse().map_err(|error| format!("cannot create code requirement ({error})"))?;
    code
      .check_validity(Flags::CHECK_ALL_ARCHITECTURES, &anchor)
      .map_err(|error| format!("the helper app is not validly signed by an Apple-issued certificate ({error})"))?;
    let (bundle_id, team_id) = signing_identity(&code)?;
    // Both values are formatted into a code requirement and a path, so accept
    // only the characters Apple permits in bundle identifiers and Team IDs.
    let valid_bundle_id =
      !bundle_id.is_empty() && !bundle_id.starts_with('.') && bundle_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    if !valid_bundle_id {
      return Err(format!("the helper app is signed with an unsupported identifier {bundle_id:?}"));
    }
    if team_id.is_empty() || !team_id.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) {
      return Err(format!("the helper app is signed with an unsupported Team ID {team_id:?}"));
    }

    Ok(Self {
      support_dir: bundle_id.clone(),
      bundle_id,
      team_id,
      app_name,
      source: Some(app.to_path_buf()),
    })
  }

  /// User-facing helper name, for example `AUV Helper`.
  pub(crate) fn display_name(&self) -> &str {
    self.app_name.strip_suffix(".app").unwrap_or(&self.app_name)
  }

  pub(crate) fn layout(&self, home: &Path) -> InstalledLayout {
    let app = support_root(home, &self.support_dir).join(&self.app_name);
    InstalledLayout {
      binary: app.join("Contents").join("MacOS").join(HELPER_EXECUTABLE),
      launch_agent: app.join("Contents").join("Library").join("LaunchAgents").join(format!("{}.plist", self.bundle_id)),
      app,
    }
  }

  pub(crate) fn socket_path(&self, home: &Path) -> PathBuf {
    socket_path(&support_root(home, &self.support_dir))
  }

  /// Code requirement for the signed helper's identity: bundle identifier and
  /// pinned Team ID. `package/package.sh` checks the same identity with
  /// `codesign --test-requirement` after signing.
  pub(crate) fn identity_requirement(&self) -> Result<SecRequirement, security_framework::base::Error> {
    self.identity_clause().parse()
  }

  /// Identity requirement plus `MIN_SECURITY_EPOCH`, for code the daemon trusts.
  ///
  /// NOTICE(helper-security-epoch-requirement): The requirement language
  /// compares a quoted constant with a string Info.plist value numerically
  /// (`"10" >= "2"` holds), but never matches an integer-typed value, so the
  /// Info.plist entry must be a `<string>`. Verified with
  /// `codesign -v -R='info[AUVHelperSecurityEpoch] >= "2"'` on macOS 26.3.
  pub(crate) fn requirement(&self) -> Result<SecRequirement, security_framework::base::Error> {
    format!("{} and {}", self.identity_clause(), security_epoch_clause(MIN_SECURITY_EPOCH)).parse()
  }

  fn identity_clause(&self) -> String {
    format!("identifier \"{}\" and anchor apple generic and certificate leaf[subject.OU] = \"{}\"", self.bundle_id, self.team_id)
  }
}

/// The helper identity selected for this process; see [`HelperIdentity`].
#[cfg(all(target_os = "macos", any(feature = "setup", feature = "transport")))]
pub(crate) fn identity() -> Result<&'static HelperIdentity, &'static str> {
  static IDENTITY: std::sync::OnceLock<Result<HelperIdentity, String>> = std::sync::OnceLock::new();
  IDENTITY
    .get_or_init(|| match std::env::var_os(HELPER_APP_ENV).filter(|value| !value.is_empty()) {
      Some(app) => {
        let app = PathBuf::from(app);
        HelperIdentity::from_app(&app).map_err(|error| format!("{HELPER_APP_ENV}={}: {error}", app.display()))
      }
      None => Ok(HelperIdentity::official()),
    })
    .as_ref()
    .map_err(String::as_str)
}

/// Signing identifier and Team ID of validly signed static code.
#[cfg(all(target_os = "macos", any(feature = "setup", feature = "transport")))]
fn signing_identity(code: &security_framework::os::macos::code_signing::SecStaticCode) -> Result<(String, String), String> {
  use core_foundation::base::{CFType, TCFType};
  use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
  use core_foundation::string::{CFString, CFStringRef};

  // NOTICE: `security-framework` 3.7 does not wrap SecCodeCopySigningInformation.
  // The function and keys are public Security.framework API since macOS 10.5
  // (Team ID since 10.9); kSecCSSigningInformation is `1 << 1` in
  // `Security/SecCode.h`. Remove this block if the crate gains a wrapper.
  #[link(name = "Security", kind = "framework")]
  unsafe extern "C" {
    fn SecCodeCopySigningInformation(code: *const std::ffi::c_void, flags: u32, information: *mut CFDictionaryRef) -> i32;
    static kSecCodeInfoIdentifier: CFStringRef;
    static kSecCodeInfoTeamIdentifier: CFStringRef;
  }
  const SIGNING_INFORMATION: u32 = 1 << 1;

  let mut information: CFDictionaryRef = std::ptr::null();
  // SAFETY: `code` is a live SecStaticCodeRef and `information` is a valid
  // out-pointer. On success the caller owns the returned dictionary.
  let status = unsafe { SecCodeCopySigningInformation(code.as_concrete_TypeRef().cast(), SIGNING_INFORMATION, &mut information) };
  if status != 0 || information.is_null() {
    return Err(format!("cannot read the helper app signing information (OSStatus {status})"));
  }
  // SAFETY: The dictionary was returned under the create rule; the keys are
  // immutable framework constants.
  let information: CFDictionary<CFString, CFType> = unsafe { CFDictionary::wrap_under_create_rule(information) };
  let value = |key: CFStringRef| {
    // SAFETY: See above; the key outlives this borrow.
    let key = unsafe { CFString::wrap_under_get_rule(key) };
    information.find(&key).and_then(|value| value.downcast::<CFString>()).map(|value| value.to_string())
  };
  // SAFETY: Reading immutable extern framework constants.
  let (identifier, team) = unsafe { (kSecCodeInfoIdentifier, kSecCodeInfoTeamIdentifier) };
  let identifier = value(identifier).ok_or_else(|| "the helper app signature has no identifier".to_string())?;
  let team = value(team).ok_or_else(|| "the helper app signature has no Team ID".to_string())?;
  Ok((identifier, team))
}

#[cfg(all(target_os = "macos", any(feature = "setup", feature = "transport")))]
fn security_epoch_clause(minimum: u32) -> String {
  format!("info[AUVHelperSecurityEpoch] >= \"{minimum}\"")
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

/// Executable name inside every helper bundle; `package/package.sh` keeps it
/// fixed when it renames the app.
#[cfg(all(target_os = "macos", any(feature = "setup", feature = "transport")))]
const HELPER_EXECUTABLE: &str = "auv-device-helper-macos";

/// `~/Library/Application Support/<support_dir>`, which holds an installed
/// helper app and its private socket directory.
#[cfg(all(target_os = "macos", any(feature = "setup", feature = "transport")))]
pub(crate) fn support_root(home: &Path, support_dir: &str) -> PathBuf {
  home.join("Library").join("Application Support").join(support_dir)
}

#[cfg(all(target_os = "macos", any(feature = "setup", feature = "transport")))]
pub(crate) fn verify_installed_files(identity: &HelperIdentity, home: &Path) -> Result<InstalledLayout, IdentityError> {
  let layout = identity.layout(home);
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

/// Private socket of the helper installed under `support_root`.
///
/// The containing directory is created by the user-session helper with mode
/// 0700. Callers must resolve the target account's home directory using OS
/// account data, never a home path supplied by the remote Device request.
#[cfg(all(target_os = "macos", any(feature = "setup", feature = "transport")))]
pub(crate) fn socket_path(support_root: &Path) -> PathBuf {
  support_root.join("device-entry").join("host.sock")
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
  let identity = identity().map_err(|_| HostError::Unavailable)?;
  let mut stream = UnixStream::connect(identity.socket_path(home)).map_err(|_| HostError::Unavailable)?;
  verify_installed_helper(identity, &stream, home).map_err(|error| match error {
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
pub(crate) fn verify_installed_helper(identity: &HelperIdentity, stream: &UnixStream, home: &Path) -> Result<(), IdentityError> {
  // A user-writable socket path alone cannot authenticate the process that
  // accepted it. Bind dynamic code validation to the kernel's socket peer
  // audit token, which includes a PID version, before sending a credential.
  // The identity's Team ID and designated requirement reject modifications to
  // the per-user app even though its containing directory is user-owned.
  // The security epoch rejects older signed helpers once they are revoked;
  // see `MIN_SECURITY_EPOCH`.
  let layout = verify_installed_files(identity, home)?;

  let code = code_for_socket_peer(stream)?;
  let requirement = identity.identity_requirement().map_err(|_| IdentityError::Mismatch)?;
  code.check_validity(Flags::NONE, &requirement).map_err(|_| IdentityError::Mismatch)?;
  // Checked separately so a revoked genuine helper is reported as such
  // instead of looking like a foreign process.
  let trusted = identity.requirement().map_err(|_| IdentityError::Mismatch)?;
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
    let real_layout = HelperIdentity::official().layout(&real_home);
    std::fs::create_dir_all(real_layout.binary.parent().unwrap()).unwrap();
    std::fs::write(&real_layout.binary, []).unwrap();
    symlink(&real_home, &linked_home).unwrap();

    let linked_layout = HelperIdentity::official().layout(&linked_home);
    let actual = std::fs::canonicalize(&real_layout.binary).unwrap();
    assert!(installed_helper_path_matches(&actual, &linked_layout).unwrap());
  }

  // The official helper keeps the paths released before identities became
  // selectable, so installed helpers and their sockets stay reachable.
  #[test]
  fn official_identity_keeps_the_released_install_and_socket_paths() {
    let home = Path::new("/Users/someone");
    let identity = HelperIdentity::official();
    let layout = identity.layout(home);

    assert_eq!(layout.app, Path::new("/Users/someone/Library/Application Support/AUV/AUV Helper.app"));
    assert_eq!(
      layout.launch_agent,
      Path::new("/Users/someone/Library/Application Support/AUV/AUV Helper.app/Contents/Library/LaunchAgents/ai.moeru.auv.helper.plist")
    );
    assert_eq!(identity.socket_path(home), Path::new("/Users/someone/Library/Application Support/AUV/device-entry/host.sock"));
    assert_eq!(identity.display_name(), "AUV Helper");
  }

  #[test]
  fn shipped_helper_installs_under_its_bundle_identifier() {
    let home = Path::new("/Users/someone");
    let identity = HelperIdentity {
      bundle_id: "com.example.computer-use.helper".to_string(),
      team_id: "ABCDE12345".to_string(),
      app_name: "Example Computer Use.app".to_string(),
      support_dir: "com.example.computer-use.helper".to_string(),
      source: Some(PathBuf::from("/Applications/Example.app/Contents/Library/Helpers/Example Computer Use.app")),
    };
    let layout = identity.layout(home);
    let root = Path::new("/Users/someone/Library/Application Support/com.example.computer-use.helper");

    assert_eq!(layout.app, root.join("Example Computer Use.app"));
    assert_eq!(layout.binary, root.join("Example Computer Use.app/Contents/MacOS/auv-device-helper-macos"));
    assert_eq!(
      layout.launch_agent,
      root.join("Example Computer Use.app/Contents/Library/LaunchAgents/com.example.computer-use.helper.plist")
    );
    assert_eq!(identity.socket_path(home), root.join("device-entry/host.sock"));
    assert_eq!(
      identity.identity_clause(),
      "identifier \"com.example.computer-use.helper\" and anchor apple generic and certificate leaf[subject.OU] = \"ABCDE12345\""
    );
  }

  // An ad-hoc signature carries no Team ID and no Apple-issued anchor, so it
  // must never become a trusted helper identity.
  #[test]
  fn shipped_helper_must_be_signed_by_an_apple_issued_certificate() {
    let root = tempfile::tempdir().unwrap();
    let app = root.path().join("Example Computer Use.app");
    let macos = app.join("Contents").join("MacOS");
    std::fs::create_dir_all(&macos).unwrap();
    std::fs::copy("/usr/bin/true", macos.join(HELPER_EXECUTABLE)).unwrap();
    std::fs::write(
      app.join("Contents").join("Info.plist"),
      format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>{HELPER_EXECUTABLE}</string><key>CFBundleIdentifier</key><string>com.example.computer-use.helper</string></dict></plist>"
      ),
    )
    .unwrap();
    let signed = std::process::Command::new("/usr/bin/codesign").args(["--force", "--sign", "-"]).arg(&app).output().unwrap();
    assert!(signed.status.success(), "{}", String::from_utf8_lossy(&signed.stderr));

    let error = HelperIdentity::from_app(&app).unwrap_err();
    assert!(error.contains("Apple-issued"), "{error}");
  }

  #[test]
  fn shipped_helper_path_must_be_an_absolute_app_bundle() {
    assert!(HelperIdentity::from_app(Path::new("Example.app")).unwrap_err().contains("absolute"));
    assert!(HelperIdentity::from_app(Path::new("/tmp/example")).unwrap_err().contains(".app"));
  }

  // Apple platform code is Apple-anchored but has no Team ID; reading its
  // signing information exercises the native lookup without a Developer ID.
  #[test]
  fn shipped_helper_requires_a_team_id() {
    let error = HelperIdentity::from_app(Path::new("/System/Applications/Calculator.app")).unwrap_err();
    assert!(error.contains("no Team ID"), "{error}");
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
