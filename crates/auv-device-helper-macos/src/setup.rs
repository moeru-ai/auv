//! Per-user installation lifecycle for the signed macOS helper app.
//!
//! CLI and native-language adapters use this module instead of duplicating
//! archive extraction, signature checks, and ServiceManagement registration.

#[cfg(target_os = "macos")]
mod embedded {
  include!(concat!(env!("OUT_DIR"), "/embedded_macos_helper.rs"));

  pub(super) fn archive() -> Option<&'static [u8]> {
    ARCHIVE
  }
}

/// Signed helper app this frontend can install.
#[cfg(target_os = "macos")]
enum Payload<'a> {
  /// Archive embedded by the release pipeline; its app version must equal
  /// this crate's version.
  Archive(&'static [u8]),
  /// Unpacked app named by [`super::HELPER_APP_ENV`].
  App(&'a std::path::Path),
}

#[cfg(target_os = "macos")]
impl<'a> Payload<'a> {
  fn for_identity(identity: &'a super::HelperIdentity) -> Option<Self> {
    match &identity.source {
      Some(app) => Some(Self::App(app)),
      None => embedded::archive().map(Self::Archive),
    }
  }

  fn version(&self) -> Option<String> {
    match self {
      Self::Archive(_) => Some(env!("CARGO_PKG_VERSION").to_string()),
      Self::App(app) => installed_version(app),
    }
  }
}

/// User-facing name of the selected helper for messages.
#[cfg(target_os = "macos")]
fn helper_name() -> &'static str {
  super::identity().map(super::HelperIdentity::display_name).unwrap_or("macOS helper")
}

#[cfg(not(target_os = "macos"))]
fn helper_name() -> &'static str {
  "AUV Helper"
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
  Unsupported,
  NotInstalled,
  Invalid,
  UpdateRequired,
  FrontendOutdated,
  Installed,
  Busy,
  RequiresApproval,
  Running,
}

impl State {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Unsupported => "unsupported",
      Self::NotInstalled => "not-installed",
      Self::Invalid => "invalid",
      Self::UpdateRequired => "update-required",
      Self::FrontendOutdated => "frontend-outdated",
      Self::Installed => "installed",
      Self::Busy => "busy",
      Self::RequiresApproval => "requires-approval",
      Self::Running => "running",
    }
  }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Status {
  pub state: State,
  pub detail: Option<String>,
  /// Whether this frontend carries a helper app it can install: an archive
  /// embedded by a release build, or an app named by `AUV_MACOS_HELPER_APP`.
  pub helper_embedded: bool,
}

#[derive(Debug)]
pub enum Error {
  Unsupported,
  PayloadUnavailable,
  UserUnavailable(String),
  InvalidInstallation(String),
  FrontendOutdated(String),
  ArchiveRejected(String),
  ExtractionFailed(String),
  ServiceManagementFailed(String),
  LaunchFailed(String),
  RollbackFailed(String),
  AccessibilityResetFailed(String),
  RemovalFailed(String),
  OpenSettingsFailed(String),
  Io(std::io::Error),
}

impl std::fmt::Display for Error {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Unsupported => write!(formatter, "{} setup requires macOS 13 or later", helper_name()),
      Self::PayloadUnavailable => write!(
        formatter,
        "this build does not contain a signed {} app; use an official macOS release, set AUV_MACOS_HELPER_APP, or rebuild with AUV_MACOS_HELPER_APP_ARCHIVE_PATH",
        helper_name()
      ),
      Self::UserUnavailable(detail) => write!(formatter, "{} setup needs the logged-in macOS user: {detail}", helper_name()),
      Self::InvalidInstallation(detail) => write!(formatter, "the installed {} failed validation: {detail}", helper_name()),
      Self::FrontendOutdated(detail) => write!(formatter, "this AUV frontend is too old for the installed {}: {detail}", helper_name()),
      Self::ArchiveRejected(detail) => write!(formatter, "the {} app to install failed validation: {detail}", helper_name()),
      Self::ExtractionFailed(detail) => write!(formatter, "the {} app to install could not be staged: {detail}", helper_name()),
      Self::ServiceManagementFailed(detail) => write!(formatter, "{} registration failed: {detail}", helper_name()),
      Self::LaunchFailed(detail) => write!(formatter, "the registered {} did not become ready: {detail}", helper_name()),
      Self::RollbackFailed(detail) => write!(formatter, "{} update rollback failed: {detail}", helper_name()),
      Self::AccessibilityResetFailed(detail) => {
        write!(formatter, "{} Accessibility authorization could not be reset: {detail}", helper_name())
      }
      Self::RemovalFailed(detail) => write!(formatter, "{} could not be removed: {detail}", helper_name()),
      Self::OpenSettingsFailed(detail) => write!(formatter, "could not open macOS settings: {detail}"),
      Self::Io(error) => write!(formatter, "helper setup I/O failed: {error}"),
    }
  }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
  fn from(error: std::io::Error) -> Self {
    Self::Io(error)
  }
}

#[cfg(not(target_os = "macos"))]
pub fn status() -> Status {
  Status {
    state: State::Unsupported,
    detail: None,
    helper_embedded: false,
  }
}

#[cfg(target_os = "macos")]
pub fn status() -> Status {
  let identity = match super::identity() {
    Ok(identity) => identity,
    Err(detail) => {
      return Status {
        state: State::Invalid,
        detail: Some(detail.to_string()),
        helper_embedded: false,
      };
    }
  };
  let embedded = Payload::for_identity(identity).is_some();
  if !supports_smappservice() {
    return Status {
      state: State::Unsupported,
      detail: Some("SMAppService requires macOS 13 or later".to_string()),
      helper_embedded: embedded,
    };
  }
  let Ok((uid, home)) = current_user() else {
    return Status {
      state: State::Invalid,
      detail: Some("the current macOS user account could not be resolved".to_string()),
      helper_embedded: embedded,
    };
  };
  let layout = identity.layout(&home);
  if !layout.app.exists() {
    return Status {
      state: State::NotInstalled,
      detail: None,
      helper_embedded: embedded,
    };
  }
  if let Err(detail) = verify_static_installation(identity, &home) {
    return Status {
      state: State::Invalid,
      detail: Some(detail),
      helper_embedded: embedded,
    };
  }
  // The static signature check covers Info.plist, so the declared epoch is
  // trustworthy here. A revoked helper is still genuine, so it is replaced
  // rather than reported as invalid.
  if !trusts_security_epoch(declared_security_epoch(&layout.app)) {
    return Status {
      state: State::UpdateRequired,
      detail: Some(format!(
        "the installed helper build is revoked; this AUV frontend requires security epoch {}",
        super::MIN_SECURITY_EPOCH
      )),
      helper_embedded: embedded,
    };
  }

  // Frontends only install and inspect the helper; the root daemon is its
  // sole caller. Usability therefore depends on the wire protocol, never on
  // whether this frontend's crate version equals the installed app's.
  let Some(protocols) = declared_protocols(&layout.app) else {
    return Status {
      state: State::Invalid,
      detail: Some("the installed helper does not declare its supported protocol range".to_string()),
      helper_embedded: embedded,
    };
  };
  let range = format!("{}-{}", protocols.start(), protocols.end());
  match compatibility(&protocols, super::PROTOCOL_VERSION) {
    Compatibility::Compatible => {}
    Compatibility::HelperOutdated => {
      return Status {
        state: State::UpdateRequired,
        detail: Some(format!("the installed helper supports protocols {range}; this AUV frontend needs {}", super::PROTOCOL_VERSION)),
        helper_embedded: embedded,
      };
    }
    Compatibility::FrontendOutdated => {
      return Status {
        state: State::FrontendOutdated,
        detail: Some(format!(
          "the installed helper supports protocols {range}; this AUV frontend speaks {}, so update this frontend",
          super::PROTOCOL_VERSION
        )),
        helper_embedded: embedded,
      };
    }
  }

  runtime_status(identity, &home, uid, &layout.binary, embedded)
}

/// Inspect ServiceManagement and socket readiness for an installation whose
/// files, signature, and version were already validated.
///
/// Registration polling calls this directly: the static checks hash the whole
/// bundle and spawn `sw_vers`/`plutil`, and their answer cannot change while
/// launchd starts the job.
#[cfg(target_os = "macos")]
fn runtime_status(identity: &super::HelperIdentity, home: &std::path::Path, uid: u32, binary: &std::path::Path, embedded: bool) -> Status {
  let service = match service_status(binary) {
    Ok(service) => service,
    Err(error) => {
      return Status {
        state: State::Invalid,
        detail: Some(error.to_string()),
        helper_embedded: embedded,
      };
    }
  };
  if service == ServiceStatus::RequiresApproval {
    return Status {
      state: State::RequiresApproval,
      detail: Some(format!("enable {} in System Settings > General > Login Items & Extensions", identity.display_name())),
      helper_embedded: embedded,
    };
  }
  if service != ServiceStatus::Enabled {
    return Status {
      state: State::Installed,
      detail: None,
      helper_embedded: embedded,
    };
  }

  match helper_readiness(identity, home, uid) {
    HelperReadiness::Ready => Status {
      state: State::Running,
      detail: None,
      helper_embedded: embedded,
    },
    HelperReadiness::NotReady => Status {
      state: State::Installed,
      detail: Some("ServiceManagement enabled the helper, but its private socket is not ready".to_string()),
      helper_embedded: embedded,
    },
    HelperReadiness::Busy => Status {
      state: State::Busy,
      detail: Some(format!("{} is handling another request; try again after it finishes", identity.display_name())),
      helper_embedded: embedded,
    },
    HelperReadiness::Revoked => Status {
      state: State::UpdateRequired,
      detail: Some("the running helper process is a revoked build; install again to replace it".to_string()),
      helper_embedded: embedded,
    },
    HelperReadiness::IdentityMismatch => Status {
      state: State::Invalid,
      detail: Some(format!(
        "the private socket peer does not match the installed {} identity; stop the conflicting helper registration",
        identity.display_name()
      )),
      helper_embedded: embedded,
    },
  }
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HelperReadiness {
  Ready,
  NotReady,
  Busy,
  IdentityMismatch,
  /// The process serving the socket is a revoked build, for example one that
  /// outlived an update of its bundle.
  Revoked,
}

#[cfg(target_os = "macos")]
impl HelperReadiness {
  fn from_identity(result: Result<(), super::IdentityError>) -> Self {
    match result {
      Ok(()) => Self::Ready,
      Err(super::IdentityError::PeerUnavailable) => Self::Busy,
      Err(super::IdentityError::Mismatch) => Self::IdentityMismatch,
      Err(super::IdentityError::Revoked) => Self::Revoked,
    }
  }
}

#[cfg(target_os = "macos")]
fn helper_readiness(identity: &super::HelperIdentity, home: &std::path::Path, uid: u32) -> HelperReadiness {
  use std::os::unix::fs::{FileTypeExt, MetadataExt};

  let socket = identity.socket_path(home);
  let valid_socket = std::fs::symlink_metadata(&socket)
    .is_ok_and(|metadata| metadata.file_type().is_socket() && metadata.uid() == uid && metadata.mode() & 0o077 == 0);
  if !valid_socket {
    return HelperReadiness::NotReady;
  }
  let Ok(stream) = std::os::unix::net::UnixStream::connect(socket) else {
    return HelperReadiness::NotReady;
  };
  HelperReadiness::from_identity(super::verify_installed_helper(identity, &stream, home))
}

#[cfg(not(target_os = "macos"))]
pub fn install() -> Result<Status, Error> {
  Err(Error::Unsupported)
}

#[cfg(target_os = "macos")]
pub fn install() -> Result<Status, Error> {
  if !supports_smappservice() {
    return Err(Error::Unsupported);
  }
  let identity = super::identity().map_err(|detail| Error::ArchiveRejected(detail.to_string()))?;
  let current = status();
  match current.state {
    State::Unsupported => return Err(Error::Unsupported),
    // Replacing an invalid app would execute its binary to unregister it.
    // `uninstall` removes such an app without running it.
    State::Invalid => {
      let detail = current.detail.unwrap_or_else(|| "unknown validation error".to_string());
      return Err(Error::InvalidInstallation(format!("{detail}; uninstall the helper, then install again")));
    }
    // Never downgrade: a newer daemon may depend on the installed protocols.
    State::FrontendOutdated => {
      return Err(Error::FrontendOutdated(current.detail.unwrap_or_else(|| "protocol not supported".to_string())));
    }
    State::NotInstalled | State::UpdateRequired => {}
    // Upgrading would cancel the in-flight request; a later install upgrades.
    State::Busy => return Ok(current),
    State::Installed | State::Running | State::RequiresApproval => {
      let (uid, home) = current_user()?;
      let layout = identity.layout(&home);
      let installed = installed_version(&layout.app);
      let upgrade = Payload::for_identity(identity)
        .and_then(|payload| payload.version())
        .is_some_and(|version| upgrades_compatible_helper(installed.as_deref(), &version));
      if !upgrade {
        if current.state != State::Installed {
          return Ok(current);
        }
        register(&layout.binary)?;
        return status_after_registration(identity, &home, uid, &layout.binary);
      }
    }
  }

  let payload = Payload::for_identity(identity).ok_or(Error::PayloadUnavailable)?;
  install_payload(identity, payload)
}

/// How the installed helper's declared protocol range relates to the
/// protocol this frontend's daemon speaks.
#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Compatibility {
  Compatible,
  /// The helper predates this protocol; the embedded helper can replace it.
  HelperOutdated,
  /// The helper dropped this protocol; replacing it would be a downgrade.
  FrontendOutdated,
}

#[cfg(target_os = "macos")]
fn compatibility(declared: &std::ops::RangeInclusive<u8>, protocol: u8) -> Compatibility {
  if protocol > *declared.end() {
    Compatibility::HelperOutdated
  } else if protocol < *declared.start() {
    Compatibility::FrontendOutdated
  } else {
    Compatibility::Compatible
  }
}

/// Whether `install` should replace a compatible helper with the payload one.
///
/// Replacement only moves forward, so frontends at different versions converge
/// on the newest helper instead of replacing each other's. An unreadable
/// installed version keeps the compatible helper rather than guessing.
#[cfg(target_os = "macos")]
fn upgrades_compatible_helper(installed: Option<&str>, embedded: &str) -> bool {
  let parse = |value: &str| semver::Version::parse(value).ok();
  match (installed.and_then(parse), parse(embedded)) {
    (Some(installed), Some(embedded)) => embedded > installed,
    _ => false,
  }
}

/// Unregister and remove the installed helper app for the current user.
///
/// Enrollment remains in the user's login Keychain. The Accessibility
/// decision for the helper bundle is reset before only the app bundle is
/// removed; other AUV Application Support content is preserved.
#[cfg(not(target_os = "macos"))]
pub fn uninstall() -> Result<Status, Error> {
  Err(Error::Unsupported)
}

/// Unregister and remove the installed helper app for the current user.
///
/// Enrollment remains in the user's login Keychain. The Accessibility
/// decision for the helper bundle is reset before only the app bundle is
/// removed; other AUV Application Support content is preserved.
#[cfg(target_os = "macos")]
pub fn uninstall() -> Result<Status, Error> {
  if !supports_smappservice() {
    return Err(Error::Unsupported);
  }

  let identity = super::identity().map_err(|detail| Error::InvalidInstallation(detail.to_string()))?;
  let (_, home) = current_user()?;
  let layout = identity.layout(&home);
  let validation = if layout.app.exists() {
    verify_static_installation(identity, &home)
  } else {
    Ok(())
  };
  let retained =
    remove_installed_app(&layout.app, validation, || unregister(&layout.binary), || reset_accessibility_authorization(&identity.bundle_id))?;
  let mut status = status();
  if let Some(reason) = retained {
    status.detail = Some(format!(
      "the removed app failed validation ({reason}), so it was not run to unregister its LaunchAgent; a later install reuses that registration"
    ));
  }
  Ok(status)
}

#[cfg(not(target_os = "macos"))]
pub fn open_accessibility_settings() -> Result<(), Error> {
  Err(Error::Unsupported)
}

#[cfg(target_os = "macos")]
pub fn open_accessibility_settings() -> Result<(), Error> {
  // NOTICE(macos-accessibility-settings-url): Apple exposes no typed API for
  // this pane. Remove this URL when ServiceManagement/TCC provides one.
  let result = std::process::Command::new("/usr/bin/open")
    .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
    .status()
    .map_err(Error::Io)?;
  if result.success() {
    Ok(())
  } else {
    Err(Error::OpenSettingsFailed(format!("open exited with {result}")))
  }
}

#[cfg(not(target_os = "macos"))]
pub fn open_background_items_settings() -> Result<(), Error> {
  Err(Error::Unsupported)
}

#[cfg(target_os = "macos")]
pub fn open_background_items_settings() -> Result<(), Error> {
  let identity = super::identity().map_err(|detail| Error::InvalidInstallation(detail.to_string()))?;
  let (_, home) = current_user()?;
  let binary = identity.layout(&home).binary;
  if binary.exists() {
    verify_static_installation(identity, &home).map_err(Error::InvalidInstallation)?;
    run_helper(&binary, "--service-management-open-settings").map(|_| ())
  } else {
    let result =
      std::process::Command::new("/usr/bin/open").arg("x-apple.systempreferences:com.apple.LoginItems-Settings.extension").status()?;
    if result.success() {
      Ok(())
    } else {
      Err(Error::OpenSettingsFailed(format!("open exited with {result}")))
    }
  }
}

#[cfg(target_os = "macos")]
fn reset_accessibility_authorization(bundle_id: &str) -> Result<(), Error> {
  let output = std::process::Command::new("/usr/bin/tccutil").args(["reset", "Accessibility", bundle_id]).output().map_err(Error::Io)?;
  if output.status.success() {
    Ok(())
  } else {
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(Error::AccessibilityResetFailed(if detail.is_empty() {
      format!("tccutil exited with {}", output.status)
    } else {
      detail
    }))
  }
}

/// Remove the helper app after unregistering it and resetting its
/// Accessibility decision.
///
/// `validation` is the static check of the installed app. A failed check
/// skips unregistration, because that runs the app's own binary, but still
/// removes the bundle so an invalid install is never a dead end. The skipped
/// reason is returned so callers can report the retained registration.
#[cfg(target_os = "macos")]
fn remove_installed_app(
  app: &std::path::Path,
  validation: Result<(), String>,
  unregister_current: impl FnOnce() -> Result<(), Error>,
  reset_accessibility: impl FnOnce() -> Result<(), Error>,
) -> Result<Option<String>, Error> {
  let installed = app.exists();
  let retained = match validation {
    Ok(()) if installed => {
      unregister_current()?;
      None
    }
    Ok(()) => None,
    Err(reason) => Some(reason),
  };
  reset_accessibility()?;
  if installed {
    std::fs::remove_dir_all(app).map_err(|error| Error::RemovalFailed(error.to_string()))?;
  }
  Ok(retained)
}

#[cfg(target_os = "macos")]
fn current_user() -> Result<(u32, std::path::PathBuf), Error> {
  let uid = nix::unistd::geteuid();
  if uid.is_root() {
    return Err(Error::UserUnavailable("setup must run in the logged-in user's session, not as root".to_string()));
  }
  let user = nix::unistd::User::from_uid(uid)
    .map_err(|error| Error::UserUnavailable(format!("cannot resolve the current user: {error}")))?
    .ok_or_else(|| Error::UserUnavailable("the current user has no account record".to_string()))?;
  Ok((uid.as_raw(), user.dir))
}

#[cfg(target_os = "macos")]
fn supports_smappservice() -> bool {
  let output = std::process::Command::new("/usr/bin/sw_vers").arg("-productVersion").output();
  output
    .ok()
    .filter(|output| output.status.success())
    .and_then(|output| String::from_utf8(output.stdout).ok())
    .and_then(|version| version.split('.').next()?.parse::<u32>().ok())
    .is_some_and(|major| major >= 13)
}

#[cfg(target_os = "macos")]
fn verify_static_installation(identity: &super::HelperIdentity, home: &std::path::Path) -> Result<(), String> {
  let layout = super::verify_installed_files(identity, home).map_err(|_| "paths, ownership, or permissions do not match".to_string())?;
  verify_app_signature(identity, &layout.app)
}

#[cfg(target_os = "macos")]
fn verify_app_signature(identity: &super::HelperIdentity, app: &std::path::Path) -> Result<(), String> {
  use core_foundation::url::CFURL;
  use security_framework::os::macos::code_signing::{Flags, SecStaticCode};

  let path = CFURL::from_path(app, true).ok_or_else(|| "the app path is not a file URL".to_string())?;
  let code = SecStaticCode::from_path(&path, Flags::NONE).map_err(|error| format!("cannot inspect code signature ({error})"))?;
  // Identity only: a revoked but genuine helper is replaced, not rejected as
  // tampered, so `status` checks its security epoch separately.
  let requirement = identity.identity_requirement().map_err(|error| format!("cannot create code requirement ({error})"))?;
  code.check_validity(Flags::CHECK_ALL_ARCHITECTURES, &requirement).map_err(|error| format!("code signature does not match ({error})"))
}

#[cfg(target_os = "macos")]
fn installed_version(app: &std::path::Path) -> Option<String> {
  info_value(&app.join("Contents").join("Info.plist"), "CFBundleShortVersionString")
}

/// Whether the daemon built with this frontend trusts a helper declaring
/// `epoch`. A missing epoch is untrusted, matching the code requirement.
#[cfg(target_os = "macos")]
fn trusts_security_epoch(epoch: Option<u32>) -> bool {
  epoch.is_some_and(|epoch| epoch >= super::MIN_SECURITY_EPOCH)
}

/// Security epoch declared by the signed bundle's Info.plist.
#[cfg(target_os = "macos")]
fn declared_security_epoch(app: &std::path::Path) -> Option<u32> {
  info_value(&app.join("Contents").join("Info.plist"), "AUVHelperSecurityEpoch")?.parse().ok()
}

/// Protocol range declared by the signed bundle's Info.plist.
#[cfg(target_os = "macos")]
fn declared_protocols(app: &std::path::Path) -> Option<std::ops::RangeInclusive<u8>> {
  let info = app.join("Contents").join("Info.plist");
  let min = info_value(&info, "AUVHelperProtocolMin")?.parse().ok()?;
  let max = info_value(&info, "AUVHelperProtocolMax")?.parse().ok()?;
  (min <= max).then_some(min..=max)
}

#[cfg(target_os = "macos")]
fn info_value(info: &std::path::Path, key: &str) -> Option<String> {
  let output = std::process::Command::new("/usr/bin/plutil").args(["-extract", key, "raw", "-o", "-"]).arg(info).output().ok()?;
  output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "macos")]
use super::ServiceStatus;

#[cfg(target_os = "macos")]
fn service_status(binary: &std::path::Path) -> Result<ServiceStatus, Error> {
  service_command(binary, "--service-management-status")
}

#[cfg(target_os = "macos")]
fn register(binary: &std::path::Path) -> Result<ServiceStatus, Error> {
  match service_command(binary, "--service-management-register")? {
    status @ (ServiceStatus::Enabled | ServiceStatus::RequiresApproval) => Ok(status),
    status => Err(Error::ServiceManagementFailed(format!("registration returned {:?}", status.as_str()))),
  }
}

#[cfg(target_os = "macos")]
fn unregister(binary: &std::path::Path) -> Result<(), Error> {
  if matches!(service_status(binary)?, ServiceStatus::Enabled | ServiceStatus::RequiresApproval) {
    let status = service_command(binary, "--service-management-unregister")?;
    if !status.is_unregistered() {
      return Err(Error::ServiceManagementFailed(format!("unregistration returned {:?}", status.as_str())));
    }
  }
  Ok(())
}

/// Run a status-reporting `--service-management-*` command of the validated
/// helper binary and parse the status it prints.
#[cfg(target_os = "macos")]
fn service_command(binary: &std::path::Path, argument: &str) -> Result<ServiceStatus, Error> {
  let value = run_helper(binary, argument)?;
  ServiceStatus::parse(&value).ok_or_else(|| Error::ServiceManagementFailed(format!("{argument} returned {value:?}")))
}

#[cfg(target_os = "macos")]
fn run_helper(binary: &std::path::Path, argument: &str) -> Result<String, Error> {
  let output = std::process::Command::new(binary).arg(argument).output().map_err(Error::Io)?;
  if output.status.success() {
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
  } else {
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(Error::ServiceManagementFailed(if detail.is_empty() {
      format!("{} exited with {}", binary.display(), output.status)
    } else {
      detail
    }))
  }
}

#[cfg(target_os = "macos")]
fn install_payload(identity: &super::HelperIdentity, payload: Payload) -> Result<Status, Error> {
  use std::io::Write;
  use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

  // TODO(helper-setup-install-lock): Two concurrent installs for one user
  // (for example the CLI and the N-API package) race on the final renames.
  // Add a per-user lock under the install root when a frontend can trigger
  // setup without user action; manual setup is serialized by the user today.
  let (uid, home) = current_user()?;
  let layout = identity.layout(&home);
  let root = layout.app.parent().ok_or_else(|| Error::InvalidInstallation("the install root has no parent".to_string()))?;
  std::fs::create_dir_all(root)?;
  std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;

  let work = tempfile::Builder::new().prefix(".helper-install.").tempdir_in(root)?;
  let extracted = work.path().join("extracted");
  std::fs::create_dir(&extracted)?;
  let candidate = extracted.join(&identity.app_name);
  match payload {
    Payload::Archive(archive) => {
      let archive_path = work.path().join("helper.zip");
      let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&archive_path)?;
      file.write_all(archive)?;
      file.sync_all()?;
      let result = std::process::Command::new("/usr/bin/ditto").args(["-x", "-k"]).arg(&archive_path).arg(&extracted).status()?;
      if !result.success() {
        return Err(Error::ExtractionFailed(format!("ditto exited with {result}")));
      }
      if installed_version(&candidate).as_deref() != Some(env!("CARGO_PKG_VERSION")) {
        return Err(Error::ArchiveRejected(format!("embedded app version does not match {}", env!("CARGO_PKG_VERSION"))));
      }
    }
    // Copy instead of registering the shipped app in place: its containing
    // application may be replaced by an updater, while TCC and
    // ServiceManagement need a stable, user-owned bundle path.
    Payload::App(source) => {
      let result = std::process::Command::new("/usr/bin/ditto").arg(source).arg(&candidate).status()?;
      if !result.success() {
        return Err(Error::ExtractionFailed(format!("ditto exited with {result}")));
      }
    }
  }
  verify_app_signature(identity, &candidate).map_err(Error::ArchiveRejected)?;
  let assessment = std::process::Command::new("/usr/sbin/spctl").args(["--assess", "--type", "execute"]).arg(&candidate).output()?;
  if !assessment.status.success() {
    return Err(Error::ArchiveRejected(String::from_utf8_lossy(&assessment.stderr).trim().to_string()));
  }

  let backup = work.path().join(format!("Previous {}", identity.app_name));
  let result = replace_installed_app(
    &layout.app,
    &candidate,
    &backup,
    || unregister(&layout.binary),
    || register(&layout.binary).map(|_| ()),
    || status_after_registration(identity, &home, uid, &layout.binary),
  );
  match result {
    Err(Error::RollbackFailed(detail)) if backup.exists() => {
      let retained = work.keep();
      Err(Error::RollbackFailed(format!("{detail}; previous helper retained at {}", retained.display())))
    }
    result => result,
  }
}

#[cfg(target_os = "macos")]
fn replace_installed_app(
  installed: &std::path::Path,
  candidate: &std::path::Path,
  backup: &std::path::Path,
  mut unregister_current: impl FnMut() -> Result<(), Error>,
  mut register_current: impl FnMut() -> Result<(), Error>,
  wait_until_ready: impl FnOnce() -> Result<Status, Error>,
) -> Result<Status, Error> {
  let had_previous = installed.exists();
  if had_previous {
    unregister_current()?;
    if let Err(error) = std::fs::rename(installed, backup) {
      return match register_current() {
        Ok(()) => Err(Error::Io(error)),
        Err(restart) => Err(Error::RollbackFailed(format!("backup move failed ({error}); previous app could not be restarted: {restart}"))),
      };
    }
  }

  if let Err(error) = std::fs::rename(candidate, installed) {
    if had_previous && let Err(rollback) = restore_previous_app(installed, backup, &mut unregister_current, &mut register_current) {
      return Err(Error::RollbackFailed(format!("replacement failed ({error}); {rollback}")));
    }
    return Err(Error::Io(error));
  }

  let activation = register_current().and_then(|()| wait_until_ready());
  match activation {
    Ok(status) => Ok(status),
    Err(error) => match restore_previous_app(installed, backup, &mut unregister_current, &mut register_current) {
      Ok(()) => Err(error),
      Err(rollback) => Err(Error::RollbackFailed(format!("activation failed ({error}); {rollback}"))),
    },
  }
}

#[cfg(target_os = "macos")]
fn restore_previous_app(
  installed: &std::path::Path,
  backup: &std::path::Path,
  unregister_current: &mut impl FnMut() -> Result<(), Error>,
  register_current: &mut impl FnMut() -> Result<(), Error>,
) -> Result<(), Error> {
  if installed.exists() {
    // The replacement may already be registered and running. Registering the
    // restored app is a no-op while the job is still enabled, so stop the
    // replacement first; otherwise its process outlives its deleted bundle and
    // a fresh install leaves a registration for a missing app.
    unregister_current().map_err(|error| Error::RollbackFailed(format!("cannot stop replacement: {error}")))?;
    std::fs::remove_dir_all(installed).map_err(|error| Error::RollbackFailed(format!("cannot remove replacement: {error}")))?;
  }
  if backup.exists() {
    std::fs::rename(backup, installed).map_err(|error| Error::RollbackFailed(format!("cannot restore previous app: {error}")))?;
    register_current().map_err(|error| Error::RollbackFailed(format!("previous app was restored but could not be registered: {error}")))?;
  }
  Ok(())
}

#[cfg(target_os = "macos")]
fn status_after_registration(
  identity: &super::HelperIdentity,
  home: &std::path::Path,
  uid: u32,
  binary: &std::path::Path,
) -> Result<Status, Error> {
  let embedded = Payload::for_identity(identity).is_some();
  wait_for_registration(
    || runtime_status(identity, home, uid, binary, embedded),
    || std::thread::sleep(std::time::Duration::from_millis(100)),
  )
}

#[cfg(target_os = "macos")]
fn wait_for_registration(mut inspect: impl FnMut() -> Status, mut wait: impl FnMut()) -> Result<Status, Error> {
  for _ in 0..20 {
    let current = inspect();
    if matches!(current.state, State::Running | State::RequiresApproval) {
      return Ok(current);
    }
    wait();
  }

  let current = inspect();
  if current.state == State::Invalid {
    Err(Error::InvalidInstallation(current.detail.unwrap_or_else(|| "unknown validation error".to_string())))
  } else {
    Err(Error::LaunchFailed(current.detail.unwrap_or_else(|| "the private socket did not become ready".to_string())))
  }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
  use std::cell::{Cell, RefCell};
  use std::fs;
  use std::os::unix::fs::PermissionsExt;
  use std::os::unix::net::UnixListener;

  // ROOT CAUSE:
  //
  // After SMAppService removed the final registration, it reported `not-found`
  // rather than `not-registered`, so a successful update was treated as a
  // failed unregistration. Both terminal states now mean no service remains.
  #[test]
  fn unregister_accepts_service_management_not_found_status() {
    assert!(crate::ServiceStatus::parse("not-found").is_some_and(crate::ServiceStatus::is_unregistered));
  }

  // ROOT CAUSE:
  //
  // The previous app lived inside a temporary directory that was discarded
  // before readiness was known. The replacement now remains transactional
  // until its socket is ready, and a failed activation restores the old app.
  #[test]
  fn update_restores_previous_helper_when_replacement_never_becomes_ready() {
    let root = tempfile::tempdir().unwrap();
    let installed = root.path().join("AUV Helper.app");
    let candidate = root.path().join("Candidate.app");
    let backup = root.path().join("Previous.app");
    fs::create_dir(&installed).unwrap();
    fs::write(installed.join("version"), "old").unwrap();
    fs::create_dir(&candidate).unwrap();
    fs::write(candidate.join("version"), "new").unwrap();

    let calls = RefCell::new(Vec::new());

    let result = super::replace_installed_app(
      &installed,
      &candidate,
      &backup,
      || {
        let version = fs::read_to_string(installed.join("version")).unwrap();
        calls.borrow_mut().push(format!("unregister {version}"));
        Ok(())
      },
      || {
        let version = fs::read_to_string(installed.join("version")).unwrap();
        calls.borrow_mut().push(format!("register {version}"));
        Ok(())
      },
      || Err(super::Error::LaunchFailed("socket was not ready".to_string())),
    );

    assert!(matches!(result, Err(super::Error::LaunchFailed(_))));
    assert_eq!(fs::read_to_string(installed.join("version")).unwrap(), "old");
    assert!(!backup.exists());
    assert_eq!(
      calls.into_inner(),
      [
        "unregister old",
        "register new",
        "unregister new",
        "register old"
      ]
    );
  }

  // ROOT CAUSE:
  //
  // When a fresh install never became ready, rollback deleted the new app
  // without unregistering it. ServiceManagement kept an enabled job for a
  // missing bundle, and a live replacement process kept running.
  //
  // The fix stops the replacement before removing it.
  #[test]
  fn failed_fresh_install_unregisters_the_replacement_before_removing_it() {
    let root = tempfile::tempdir().unwrap();
    let installed = root.path().join("AUV Helper.app");
    let candidate = root.path().join("Candidate.app");
    let backup = root.path().join("Previous.app");
    fs::create_dir(&candidate).unwrap();
    let calls = RefCell::new(Vec::new());

    let result = super::replace_installed_app(
      &installed,
      &candidate,
      &backup,
      || {
        calls.borrow_mut().push(format!("unregister present={}", installed.exists()));
        Ok(())
      },
      || {
        calls.borrow_mut().push("register".to_string());
        Ok(())
      },
      || Err(super::Error::LaunchFailed("socket was not ready".to_string())),
    );

    assert!(matches!(result, Err(super::Error::LaunchFailed(_))));
    assert!(!installed.exists());
    assert_eq!(calls.into_inner(), ["register", "unregister present=true"]);
  }

  #[test]
  fn rollback_keeps_the_replacement_when_it_cannot_be_stopped() {
    let root = tempfile::tempdir().unwrap();
    let installed = root.path().join("AUV Helper.app");
    let candidate = root.path().join("Candidate.app");
    let backup = root.path().join("Previous.app");
    fs::create_dir(&installed).unwrap();
    fs::write(installed.join("version"), "old").unwrap();
    fs::create_dir(&candidate).unwrap();
    fs::write(candidate.join("version"), "new").unwrap();
    let unregistrations = Cell::new(0);

    let result = super::replace_installed_app(
      &installed,
      &candidate,
      &backup,
      || {
        unregistrations.set(unregistrations.get() + 1);
        if unregistrations.get() == 1 {
          Ok(())
        } else {
          Err(super::Error::ServiceManagementFailed("denied".to_string()))
        }
      },
      || Ok(()),
      || Err(super::Error::LaunchFailed("socket was not ready".to_string())),
    );

    // A running replacement must not lose its bundle; the caller retains the
    // backup because it still exists.
    assert!(matches!(result, Err(super::Error::RollbackFailed(_))));
    assert_eq!(fs::read_to_string(installed.join("version")).unwrap(), "new");
    assert_eq!(fs::read_to_string(backup.join("version")).unwrap(), "old");
  }

  // ROOT CAUSE:
  //
  // The existing service was unregistered before its app was moved aside. If
  // that rename failed, the old app remained intact but no longer ran. A
  // failed replacement must re-register the still-installed previous app.
  #[test]
  fn update_restarts_previous_helper_when_backup_move_fails() {
    let root = tempfile::tempdir().unwrap();
    let installed = root.path().join("AUV Helper.app");
    let candidate = root.path().join("Candidate.app");
    let backup = root.path().join("missing-parent").join("Previous.app");
    fs::create_dir(&installed).unwrap();
    fs::write(installed.join("version"), "old").unwrap();
    fs::create_dir(&candidate).unwrap();
    let registrations = Cell::new(0);

    let result = super::replace_installed_app(
      &installed,
      &candidate,
      &backup,
      || Ok(()),
      || {
        registrations.set(registrations.get() + 1);
        Ok(())
      },
      || panic!("a replacement that could not be staged must not be activated"),
    );

    assert!(matches!(result, Err(super::Error::Io(_))));
    assert_eq!(registrations.get(), 1);
    assert_eq!(fs::read_to_string(installed.join("version")).unwrap(), "old");
  }

  // ROOT CAUSE:
  //
  // Readiness previously meant only that some process accepted the shared
  // socket path. A stale or unrelated helper could therefore make status say
  // `running` even though Device calls rejected its code identity.
  #[test]
  fn readiness_rejects_a_socket_owned_by_the_wrong_process_identity() {
    let home = tempfile::Builder::new().prefix("ah").tempdir_in("/tmp").unwrap();
    let identity = crate::HelperIdentity::official();
    let layout = identity.layout(home.path());
    fs::create_dir_all(layout.binary.parent().unwrap()).unwrap();
    fs::create_dir_all(layout.launch_agent.parent().unwrap()).unwrap();
    fs::write(&layout.binary, []).unwrap();
    fs::write(&layout.launch_agent, []).unwrap();
    let socket = identity.socket_path(home.path());
    fs::create_dir_all(socket.parent().unwrap()).unwrap();
    let _listener = UnixListener::bind(&socket).unwrap();
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();

    assert_eq!(super::helper_readiness(&identity, home.path(), nix::unistd::geteuid().as_raw()), super::HelperReadiness::IdentityMismatch);
  }

  // ROOT CAUSE:
  //
  // While the serial helper handles an unlock or lock, a new connection can
  // remain queued without a LOCAL_PEERTOKEN. That temporary busy state was
  // previously reported as an identity mismatch, causing status to say the
  // installation was invalid and install to reject it.
  #[test]
  fn unavailable_peer_reports_busy_instead_of_an_identity_mismatch() {
    assert_eq!(super::HelperReadiness::from_identity(Err(crate::IdentityError::PeerUnavailable)), super::HelperReadiness::Busy);
  }

  // ROOT CAUSE:
  //
  // A newly registered helper that kept a connection queued without accepting
  // it was reported as successfully installed because `busy` counted as ready.
  // Persistent busy now fails activation so an update can restore the old app.
  #[test]
  fn registration_that_remains_busy_fails_activation() {
    let result = super::wait_for_registration(
      || super::Status {
        state: super::State::Busy,
        detail: Some("helper did not accept the readiness connection".to_string()),
        helper_embedded: true,
      },
      || {},
    );

    assert!(matches!(result, Err(super::Error::LaunchFailed(detail)) if detail.contains("did not accept")));
  }

  // Setup reads the protocol range from the signed bundle instead of running
  // the helper, so the packaged Info.plist must match the host's constant.
  #[test]
  fn packaged_info_plist_declares_the_supported_protocol_range() {
    let info = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("package").join("Info.plist");
    let min = super::info_value(&info, "AUVHelperProtocolMin").and_then(|value| value.parse::<u8>().ok());
    let max = super::info_value(&info, "AUVHelperProtocolMax").and_then(|value| value.parse::<u8>().ok());

    assert_eq!(min, Some(*crate::SUPPORTED_PROTOCOLS.start()));
    assert_eq!(max, Some(*crate::SUPPORTED_PROTOCOLS.end()));
    assert!(crate::SUPPORTED_PROTOCOLS.contains(&crate::PROTOCOL_VERSION));
  }

  // A packaged helper below the minimum epoch would be rejected by the very
  // daemon built with it.
  #[test]
  fn packaged_info_plist_declares_a_trusted_security_epoch_as_a_string() {
    let info = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("package").join("Info.plist");
    let output = std::process::Command::new("/usr/bin/plutil")
      .args(["-extract", "AUVHelperSecurityEpoch", "xml1", "-o", "-"])
      .arg(&info)
      .output()
      .unwrap();
    let epoch = super::info_value(&info, "AUVHelperSecurityEpoch").and_then(|value| value.parse::<u32>().ok());

    assert!(String::from_utf8_lossy(&output.stdout).contains("<string>"), "code requirements match only string values");
    assert!(epoch.is_some_and(|epoch| epoch >= crate::MIN_SECURITY_EPOCH));
  }

  #[test]
  fn helpers_below_the_minimum_security_epoch_are_not_trusted() {
    let minimum = crate::MIN_SECURITY_EPOCH;

    assert!(super::trusts_security_epoch(Some(minimum)));
    assert!(super::trusts_security_epoch(Some(minimum + 1)));
    assert!(!super::trusts_security_epoch(Some(minimum - 1)));
    assert!(!super::trusts_security_epoch(None));
  }

  #[test]
  fn compatibility_depends_only_on_the_declared_protocol_range() {
    use super::Compatibility::{Compatible, FrontendOutdated, HelperOutdated};

    assert_eq!(super::compatibility(&(1..=3), 2), Compatible);
    assert_eq!(super::compatibility(&(1..=1), 2), HelperOutdated);
    assert_eq!(super::compatibility(&(2..=3), 1), FrontendOutdated);
  }

  // ROOT CAUSE:
  //
  // Setup accepted an installed helper only when its bundle version equalled
  // the calling frontend's crate version. Two frontends at different versions
  // (for example a global `auv` and an app bundling `@auv-js/cli`) therefore
  // replaced each other's helper on every install, downgrading the newer one.
  //
  // The fix keeps any protocol-compatible helper and only upgrades forward.
  #[test]
  fn frontends_at_different_versions_do_not_replace_each_others_helper() {
    assert!(!super::upgrades_compatible_helper(Some("0.0.25"), "0.0.22"));
    assert!(super::upgrades_compatible_helper(Some("0.0.22"), "0.0.25"));
    assert!(!super::upgrades_compatible_helper(Some("0.0.22"), "0.0.22"));
  }

  #[test]
  fn an_unreadable_installed_version_keeps_the_compatible_helper() {
    assert!(!super::upgrades_compatible_helper(None, "0.0.25"));
    assert!(!super::upgrades_compatible_helper(Some("not-a-version"), "0.0.25"));
  }

  #[test]
  fn uninstall_removes_only_the_helper_app_and_preserves_enrollment_storage() {
    let root = tempfile::tempdir().unwrap();
    let app = root.path().join("AUV Helper.app");
    let entry = root.path().join("device-entry");
    fs::create_dir(&app).unwrap();
    fs::create_dir(&entry).unwrap();
    fs::write(entry.join("enrollment-marker"), "preserved").unwrap();
    let reset = Cell::new(false);
    let unregistered = Cell::new(false);

    let retained = super::remove_installed_app(
      &app,
      Ok(()),
      || {
        unregistered.set(true);
        Ok(())
      },
      || {
        reset.set(true);
        Ok(())
      },
    )
    .unwrap();

    assert_eq!(retained, None);
    assert!(unregistered.get());
    assert!(reset.get());
    assert!(!app.exists());
    assert_eq!(fs::read_to_string(entry.join("enrollment-marker")).unwrap(), "preserved");
  }

  #[test]
  fn uninstall_keeps_the_app_when_accessibility_reset_fails() {
    let root = tempfile::tempdir().unwrap();
    let app = root.path().join("AUV Helper.app");
    fs::create_dir(&app).unwrap();

    let result = super::remove_installed_app(&app, Ok(()), || Ok(()), || Err(super::Error::AccessibilityResetFailed("denied".to_string())));

    assert!(matches!(result, Err(super::Error::AccessibilityResetFailed(_))));
    assert!(app.exists());
  }

  // ROOT CAUSE:
  //
  // If the installed app failed static validation, `install` rejected it as
  // invalid and `uninstall` rejected it before removal, so neither command
  // could recover and the user had to delete the bundle by hand.
  //
  // The fix removes an invalid app without executing its binary.
  #[test]
  fn uninstall_removes_an_invalid_app_without_running_it() {
    let root = tempfile::tempdir().unwrap();
    let app = root.path().join("AUV Helper.app");
    fs::create_dir(&app).unwrap();

    let retained = super::remove_installed_app(
      &app,
      Err("code signature does not match".to_string()),
      || panic!("an app that failed validation must not be executed to unregister it"),
      || Ok(()),
    )
    .unwrap();

    assert_eq!(retained.as_deref(), Some("code signature does not match"));
    assert!(!app.exists());
  }
}
