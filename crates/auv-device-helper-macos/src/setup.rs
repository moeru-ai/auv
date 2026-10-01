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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
  Unsupported,
  NotInstalled,
  Invalid,
  UpdateRequired,
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
  pub helper_embedded: bool,
}

#[derive(Debug)]
pub enum Error {
  Unsupported,
  PayloadUnavailable,
  InvalidInstallation(String),
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
      Self::Unsupported => formatter.write_str("AUV Helper setup requires macOS 13 or later"),
      Self::PayloadUnavailable => formatter.write_str(
        "this development build does not contain a signed AUV Helper app; use an official macOS release or rebuild with AUV_MACOS_HELPER_APP_ARCHIVE_PATH",
      ),
      Self::InvalidInstallation(detail) => write!(formatter, "the installed AUV Helper failed validation: {detail}"),
      Self::ArchiveRejected(detail) => write!(formatter, "the embedded AUV Helper failed validation: {detail}"),
      Self::ExtractionFailed(detail) => write!(formatter, "the embedded AUV Helper could not be extracted: {detail}"),
      Self::ServiceManagementFailed(detail) => write!(formatter, "AUV Helper registration failed: {detail}"),
      Self::LaunchFailed(detail) => write!(formatter, "the registered AUV Helper did not become ready: {detail}"),
      Self::RollbackFailed(detail) => write!(formatter, "AUV Helper update rollback failed: {detail}"),
      Self::AccessibilityResetFailed(detail) => write!(formatter, "AUV Helper Accessibility authorization could not be reset: {detail}"),
      Self::RemovalFailed(detail) => write!(formatter, "AUV Helper could not be removed: {detail}"),
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
  let embedded = embedded::archive().is_some();
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
  let layout = super::installed_layout(&home);
  if !layout.app.exists() {
    return Status {
      state: State::NotInstalled,
      detail: None,
      helper_embedded: embedded,
    };
  }
  if let Err(detail) = verify_static_installation(&home) {
    return Status {
      state: State::Invalid,
      detail: Some(detail),
      helper_embedded: embedded,
    };
  }
  if installed_version(&layout.app).as_deref() != Some(env!("CARGO_PKG_VERSION")) {
    return Status {
      state: State::UpdateRequired,
      detail: Some(format!("installed version does not match {}", env!("CARGO_PKG_VERSION"))),
      helper_embedded: embedded,
    };
  }

  let service = match service_status(&layout.binary) {
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
      detail: Some("enable AUV Helper in System Settings > General > Login Items & Extensions".to_string()),
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

  match helper_readiness(&home, uid) {
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
      detail: Some("AUV Helper is handling another request; try again after it finishes".to_string()),
      helper_embedded: embedded,
    },
    HelperReadiness::IdentityMismatch => Status {
      state: State::Invalid,
      detail: Some(
        "the private socket peer does not match the installed AUV Helper identity; stop the conflicting helper registration".to_string(),
      ),
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
}

#[cfg(target_os = "macos")]
impl HelperReadiness {
  fn from_identity(result: Result<(), super::IdentityError>) -> Self {
    match result {
      Ok(()) => Self::Ready,
      Err(super::IdentityError::PeerUnavailable) => Self::Busy,
      Err(super::IdentityError::Mismatch) => Self::IdentityMismatch,
    }
  }
}

#[cfg(target_os = "macos")]
fn helper_readiness(home: &std::path::Path, uid: u32) -> HelperReadiness {
  use std::os::unix::fs::{FileTypeExt, MetadataExt};

  let socket = super::socket_path(home);
  let valid_socket = std::fs::symlink_metadata(&socket)
    .is_ok_and(|metadata| metadata.file_type().is_socket() && metadata.uid() == uid && metadata.mode() & 0o077 == 0);
  if !valid_socket {
    return HelperReadiness::NotReady;
  }
  let Ok(stream) = std::os::unix::net::UnixStream::connect(socket) else {
    return HelperReadiness::NotReady;
  };
  HelperReadiness::from_identity(super::verify_installed_helper(&stream, home))
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
  match status() {
    current if matches!(current.state, State::Running | State::Busy | State::RequiresApproval) => return Ok(current),
    current if current.state == State::Invalid => {
      return Err(Error::InvalidInstallation(current.detail.unwrap_or_else(|| "unknown validation error".to_string())));
    }
    current if current.state == State::Installed => {
      let (_, home) = current_user()?;
      let layout = super::installed_layout(&home);
      register(&layout.binary)?;
      return status_after_registration();
    }
    _ => {}
  }

  let archive = embedded::archive().ok_or(Error::PayloadUnavailable)?;
  install_embedded_app(archive)
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

  let (_, home) = current_user()?;
  let layout = super::installed_layout(&home);
  if layout.app.exists() {
    verify_static_installation(&home).map_err(Error::InvalidInstallation)?;
    unregister(&layout.binary)?;
  }

  remove_installed_app(&layout.app, reset_accessibility_authorization)?;
  Ok(status())
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
  let (_, home) = current_user()?;
  let binary = super::installed_layout(&home).binary;
  if binary.exists() {
    verify_static_installation(&home).map_err(Error::InvalidInstallation)?;
    service_command(&binary, "--service-management-open-settings").map(|_| ())
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
fn reset_accessibility_authorization() -> Result<(), Error> {
  let output =
    std::process::Command::new("/usr/bin/tccutil").args(["reset", "Accessibility", super::BUNDLE_ID]).output().map_err(Error::Io)?;
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

#[cfg(target_os = "macos")]
fn remove_installed_app(app: &std::path::Path, reset_accessibility: impl FnOnce() -> Result<(), Error>) -> Result<(), Error> {
  reset_accessibility()?;
  if app.exists() {
    std::fs::remove_dir_all(app).map_err(|error| Error::RemovalFailed(error.to_string()))?;
  }
  Ok(())
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ServiceStatus {
  NotRegistered,
  Enabled,
  RequiresApproval,
  NotFound,
}

#[cfg(target_os = "macos")]
fn current_user() -> Result<(u32, std::path::PathBuf), Error> {
  let uid = nix::unistd::geteuid();
  if uid.is_root() {
    return Err(Error::InvalidInstallation("setup must run in the logged-in user's session, not as root".to_string()));
  }
  let user = nix::unistd::User::from_uid(uid)
    .map_err(|error| Error::InvalidInstallation(format!("cannot resolve the current user: {error}")))?
    .ok_or_else(|| Error::InvalidInstallation("the current user has no account record".to_string()))?;
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
fn verify_static_installation(home: &std::path::Path) -> Result<(), String> {
  let layout = super::verify_installed_files(home).map_err(|_| "paths, ownership, or permissions do not match".to_string())?;
  verify_app_signature(&layout.app)
}

#[cfg(target_os = "macos")]
fn verify_app_signature(app: &std::path::Path) -> Result<(), String> {
  use core_foundation::url::CFURL;
  use security_framework::os::macos::code_signing::{Flags, SecRequirement, SecStaticCode};

  let path = CFURL::from_path(app, true).ok_or_else(|| "the app path is not a file URL".to_string())?;
  let code = SecStaticCode::from_path(&path, Flags::NONE).map_err(|error| format!("cannot inspect code signature ({error})"))?;
  let requirement: SecRequirement = format!(
    "identifier \"{}\" and anchor apple generic and certificate leaf[subject.OU] = \"{}\"",
    super::BUNDLE_ID,
    super::EXPECTED_TEAM_ID
  )
  .parse()
  .map_err(|error| format!("cannot create code requirement ({error})"))?;
  code.check_validity(Flags::CHECK_ALL_ARCHITECTURES, &requirement).map_err(|error| format!("code signature does not match ({error})"))
}

#[cfg(target_os = "macos")]
fn installed_version(app: &std::path::Path) -> Option<String> {
  let output = std::process::Command::new("/usr/bin/plutil")
    .args(["-extract", "CFBundleShortVersionString", "raw", "-o", "-"])
    .arg(app.join("Contents").join("Info.plist"))
    .output()
    .ok()?;
  output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "macos")]
fn service_status(binary: &std::path::Path) -> Result<ServiceStatus, Error> {
  let output = service_command(binary, "--service-management-status")?;
  match output.as_str() {
    "not-registered" => Ok(ServiceStatus::NotRegistered),
    "enabled" => Ok(ServiceStatus::Enabled),
    "requires-approval" => Ok(ServiceStatus::RequiresApproval),
    "not-found" => Ok(ServiceStatus::NotFound),
    value => Err(Error::ServiceManagementFailed(format!("unexpected status {value:?}"))),
  }
}

#[cfg(target_os = "macos")]
fn register(binary: &std::path::Path) -> Result<ServiceStatus, Error> {
  let output = service_command(binary, "--service-management-register")?;
  match output.as_str() {
    "enabled" => Ok(ServiceStatus::Enabled),
    "requires-approval" => Ok(ServiceStatus::RequiresApproval),
    value => Err(Error::ServiceManagementFailed(format!("registration returned {value:?}"))),
  }
}

#[cfg(target_os = "macos")]
fn unregister(binary: &std::path::Path) -> Result<(), Error> {
  let status = service_status(binary)?;
  if matches!(status, ServiceStatus::Enabled | ServiceStatus::RequiresApproval) {
    let output = service_command(binary, "--service-management-unregister")?;
    ensure_unregistered(&output)?;
  }
  Ok(())
}

#[cfg(target_os = "macos")]
fn ensure_unregistered(output: &str) -> Result<(), Error> {
  if matches!(output, "not-registered" | "not-found") {
    Ok(())
  } else {
    Err(Error::ServiceManagementFailed(format!("unregistration returned {output:?}")))
  }
}

#[cfg(target_os = "macos")]
fn service_command(binary: &std::path::Path, argument: &str) -> Result<String, Error> {
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
fn install_embedded_app(archive: &[u8]) -> Result<Status, Error> {
  use std::io::Write;
  use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

  let (_, home) = current_user()?;
  let layout = super::installed_layout(&home);
  let root = layout.app.parent().ok_or_else(|| Error::InvalidInstallation("the install root has no parent".to_string()))?;
  std::fs::create_dir_all(root)?;
  std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;

  let work = tempfile::Builder::new().prefix(".helper-install.").tempdir_in(root)?;
  let archive_path = work.path().join("AUV Helper.zip");
  let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&archive_path)?;
  file.write_all(archive)?;
  file.sync_all()?;

  let extracted = work.path().join("extracted");
  std::fs::create_dir(&extracted)?;
  let result = std::process::Command::new("/usr/bin/ditto").args(["-x", "-k"]).arg(&archive_path).arg(&extracted).status()?;
  if !result.success() {
    return Err(Error::ExtractionFailed(format!("ditto exited with {result}")));
  }
  let candidate = extracted.join("AUV Helper.app");
  verify_app_signature(&candidate).map_err(Error::ArchiveRejected)?;
  if installed_version(&candidate).as_deref() != Some(env!("CARGO_PKG_VERSION")) {
    return Err(Error::ArchiveRejected(format!("embedded app version does not match {}", env!("CARGO_PKG_VERSION"))));
  }
  let assessment = std::process::Command::new("/usr/sbin/spctl").args(["--assess", "--type", "execute"]).arg(&candidate).output()?;
  if !assessment.status.success() {
    return Err(Error::ArchiveRejected(String::from_utf8_lossy(&assessment.stderr).trim().to_string()));
  }

  let backup = work.path().join("Previous AUV Helper.app");
  let result = replace_installed_app(
    &layout.app,
    &candidate,
    &backup,
    || unregister(&layout.binary),
    || register(&layout.binary).map(|_| ()),
    status_after_registration,
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
    if had_previous && let Err(rollback) = restore_previous_app(installed, backup, &mut register_current) {
      return Err(Error::RollbackFailed(format!("replacement failed ({error}); {rollback}")));
    }
    return Err(Error::Io(error));
  }

  let activation = register_current().and_then(|()| wait_until_ready());
  match activation {
    Ok(status) => Ok(status),
    Err(error) => match restore_previous_app(installed, backup, &mut register_current) {
      Ok(()) => Err(error),
      Err(rollback) => Err(Error::RollbackFailed(format!("activation failed ({error}); {rollback}"))),
    },
  }
}

#[cfg(target_os = "macos")]
fn restore_previous_app(
  installed: &std::path::Path,
  backup: &std::path::Path,
  register_current: &mut impl FnMut() -> Result<(), Error>,
) -> Result<(), Error> {
  if installed.exists() {
    std::fs::remove_dir_all(installed).map_err(|error| Error::RollbackFailed(format!("cannot remove replacement: {error}")))?;
  }
  if backup.exists() {
    std::fs::rename(backup, installed).map_err(|error| Error::RollbackFailed(format!("cannot restore previous app: {error}")))?;
    register_current().map_err(|error| Error::RollbackFailed(format!("previous app was restored but could not be registered: {error}")))?;
  }
  Ok(())
}

#[cfg(target_os = "macos")]
fn status_after_registration() -> Result<Status, Error> {
  wait_for_registration(status, || std::thread::sleep(std::time::Duration::from_millis(100)))
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
  use std::cell::Cell;
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
    assert!(super::ensure_unregistered("not-found").is_ok());
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

    let result = super::replace_installed_app(
      &installed,
      &candidate,
      &backup,
      || Ok(()),
      || Ok(()),
      || Err(super::Error::LaunchFailed("socket was not ready".to_string())),
    );

    assert!(matches!(result, Err(super::Error::LaunchFailed(_))));
    assert_eq!(fs::read_to_string(installed.join("version")).unwrap(), "old");
    assert!(!backup.exists());
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
    let layout = crate::installed_layout(home.path());
    fs::create_dir_all(layout.binary.parent().unwrap()).unwrap();
    fs::create_dir_all(layout.launch_agent.parent().unwrap()).unwrap();
    fs::write(&layout.binary, []).unwrap();
    fs::write(&layout.launch_agent, []).unwrap();
    let socket = crate::socket_path(home.path());
    fs::create_dir_all(socket.parent().unwrap()).unwrap();
    let _listener = UnixListener::bind(&socket).unwrap();
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();

    assert_eq!(super::helper_readiness(home.path(), nix::unistd::geteuid().as_raw()), super::HelperReadiness::IdentityMismatch);
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

  #[test]
  fn uninstall_removes_only_the_helper_app_and_preserves_enrollment_storage() {
    let root = tempfile::tempdir().unwrap();
    let app = root.path().join("AUV Helper.app");
    let entry = root.path().join("device-entry");
    fs::create_dir(&app).unwrap();
    fs::create_dir(&entry).unwrap();
    fs::write(entry.join("enrollment-marker"), "preserved").unwrap();
    let reset = Cell::new(false);

    super::remove_installed_app(&app, || {
      reset.set(true);
      Ok(())
    })
    .unwrap();

    assert!(reset.get());
    assert!(!app.exists());
    assert_eq!(fs::read_to_string(entry.join("enrollment-marker")).unwrap(), "preserved");
  }

  #[test]
  fn uninstall_keeps_the_app_when_accessibility_reset_fails() {
    let root = tempfile::tempdir().unwrap();
    let app = root.path().join("AUV Helper.app");
    fs::create_dir(&app).unwrap();

    let result = super::remove_installed_app(&app, || Err(super::Error::AccessibilityResetFailed("denied".to_string())));

    assert!(matches!(result, Err(super::Error::AccessibilityResetFailed(_))));
    assert!(app.exists());
  }
}
