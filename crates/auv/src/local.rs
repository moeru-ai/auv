//! First-party local driver configuration and explicit Linux Portal setup.
//!
//! Library hosts can configure `auv_driver::LocalDriver` with their own identity.
//! This module owns the AUV executable's identity and persistent authorization.

use auv_driver::{Driver, DriverResult, LocalDriver, LocalDriverSession};
use std::path::PathBuf;

/// Stable desktop-entry identity used by AUV's direct and Runner frontends.
pub const PORTAL_APP_ID: &str = "ai.moeru.auv";

/// Creates an AUV-owned driver. Runner callers can retain their existing state
/// directory; direct invocations use the user's durable AUV state directory.
pub fn driver(portal_state_root: Option<PathBuf>) -> DriverResult<LocalDriver> {
  let driver = LocalDriver::new();
  #[cfg(target_os = "linux")]
  {
    let backend = match std::env::var("AUV_LINUX_INPUT_BACKEND").as_deref() {
      Ok("portal") | Err(std::env::VarError::NotPresent) => auv_driver::LinuxInputBackend::Portal,
      Ok("uinput") => auv_driver::LinuxInputBackend::Uinput,
      _ => return Err(config_error("AUV_LINUX_INPUT_BACKEND must be portal or uinput")),
    };
    Ok(driver.with_linux_input_backend(backend).with_linux_portal_app_id(PORTAL_APP_ID.to_string())?.with_linux_portal_state_root(
      match portal_state_root {
        Some(root) => root,
        None => default_portal_state_root()?,
      },
    ))
  }
  #[cfg(not(target_os = "linux"))]
  {
    let _ = portal_state_root;
    Ok(driver)
  }
}

/// Opens a direct AUV operation with the same identity policy as its Runner.
pub fn open() -> DriverResult<LocalDriverSession> {
  driver(None)?.open_local()
}

#[cfg(target_os = "linux")]
fn config_error(message: impl Into<String>) -> auv_driver::DriverError {
  auv_driver::DriverError::InvalidInput {
    message: message.into(),
  }
}

#[cfg(target_os = "linux")]
pub fn default_portal_state_root() -> DriverResult<PathBuf> {
  let directories =
    directories::ProjectDirs::from("ai", "moeru", "auv").ok_or_else(|| config_error("cannot resolve the AUV user state directory"))?;
  Ok(directories.state_dir().unwrap_or_else(|| directories.data_local_dir()).join("portal"))
}

#[cfg(target_os = "linux")]
fn desktop_entry() -> DriverResult<PathBuf> {
  let directories = directories::BaseDirs::new().ok_or_else(|| config_error("cannot resolve the user data directory"))?;
  Ok(directories.data_dir().join("applications").join(format!("{PORTAL_APP_ID}.desktop")))
}

/// Installs only AUV's user desktop entry. It grants no capture or input rights.
#[cfg(target_os = "linux")]
pub fn setup_portal() -> DriverResult<PathBuf> {
  let path = desktop_entry()?;
  std::fs::create_dir_all(path.parent().expect("desktop entry has a parent"))
    .map_err(|error| config_error(format!("cannot create desktop-entry directory: {error}")))?;
  let executable = std::env::current_exe().map_err(|error| config_error(format!("cannot resolve AUV executable: {error}")))?;
  std::fs::write(&path, desktop_entry_contents(&executable)?)
    .map_err(|error| config_error(format!("cannot install {}: {error}", path.display())))?;
  Ok(path)
}

/// Exec resolves against the desktop service's PATH, which often excludes an
/// SDK-bundled or locally built AUV binary. Use the installing executable.
/// https://specifications.freedesktop.org/desktop-entry/latest/exec-variables.html
#[cfg(target_os = "linux")]
fn desktop_entry_contents(executable: &std::path::Path) -> DriverResult<String> {
  let executable = executable.to_str().ok_or_else(|| config_error("desktop entry requires a UTF-8 executable path"))?;
  if executable.contains('=') || executable.chars().any(char::is_control) {
    return Err(config_error("desktop entry executable path cannot contain '=' or control characters"));
  }
  // Desktop string unescaping precedes Exec argument unquoting.
  let mut argument = String::from("\"");
  for character in executable.chars() {
    match character {
      '\\' => argument.push_str(r"\\\\"),
      '"' => argument.push_str(r#"\\""#),
      '`' => argument.push_str(r"\\`"),
      '$' => argument.push_str(r"\\$"),
      '%' => argument.push_str("%%"),
      other => argument.push(other),
    }
  }
  argument.push('"');
  Ok(format!("[Desktop Entry]\nType=Application\nName=AUV\nComment=Application Use Via\nExec={argument}\nNoDisplay=true\nTerminal=false\n"))
}

#[cfg(target_os = "linux")]
#[derive(serde::Serialize)]
pub struct PortalStatus {
  pub app_id: &'static str,
  pub desktop_entry: PathBuf,
  pub desktop_entry_installed: bool,
  pub state_root: PathBuf,
  pub capture_token_present: bool,
  pub input_token_present: bool,
  pub identity_registered: bool,
  pub identity_error: Option<String>,
  pub interfaces: auv_driver::LinuxPortalProbe,
  pub kde_authorization: Option<auv_driver::PermissionStatus>,
  pub kde_error: Option<String>,
}

/// Presence of a token or Portal interface is not proof of restored permission.
#[cfg(target_os = "linux")]
pub fn portal_status(state_root: Option<PathBuf>) -> DriverResult<PortalStatus> {
  let path = desktop_entry()?;
  let state_root = match state_root {
    Some(root) => root,
    None => default_portal_state_root()?,
  };
  let interfaces = auv_driver::probe_portals();
  let identity = auv_driver::verify_portal_identity(PORTAL_APP_ID);
  let kde = interfaces.desktop.as_deref().is_some_and(|desktop| desktop.split(':').any(|part| part.eq_ignore_ascii_case("KDE")));
  let authorization = kde.then(|| auv_driver::kde_authorization(PORTAL_APP_ID));
  Ok(PortalStatus {
    app_id: PORTAL_APP_ID,
    desktop_entry_installed: path.is_file(),
    desktop_entry: path,
    capture_token_present: state_root.join("screencast-token").is_file(),
    input_token_present: state_root.join("remote-desktop-input-token").is_file(),
    state_root,
    identity_registered: identity.is_ok(),
    identity_error: identity.err().map(|error| error.to_string()),
    interfaces,
    kde_authorization: authorization.as_ref().and_then(|value| value.as_ref().ok().copied()),
    kde_error: authorization.and_then(Result::err).map(|error| error.to_string()),
  })
}

/// Explicit authorization can show Portal consent dialogs, but sends no input.
#[cfg(target_os = "linux")]
pub fn authorize_portal(state_root: Option<PathBuf>) -> DriverResult<()> {
  let session = driver(state_root)?.open_local()?;
  session.permission().authorize_portals()
}

/// KDE has a separate application-level allow rule. Ordinary operations never
/// call this function; revocation removes this rule, not existing Portal grants.
#[cfg(target_os = "linux")]
pub fn set_kde_portal_authorization(allow: bool) -> DriverResult<()> {
  let status = portal_status(None)?;
  if status.kde_authorization.is_none() && status.kde_error.is_none() {
    return Err(config_error("KDE Portal authorization requires a KDE desktop session"));
  }
  if !status.identity_registered {
    return Err(config_error(status.identity_error.unwrap_or_else(|| "Portal identity is not registered".into())));
  }
  auv_driver::set_kde_authorization(PORTAL_APP_ID, allow)
}
