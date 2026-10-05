//! Installed Windows Helper lifecycle.
//!
//! The Helper is `auv-helper.exe --service`, a LocalSystem SCM service with no
//! network listener, pairing store, or daemon state. The AUV daemon is a
//! separate, ordinary `auv serve` process that calls the Helper over its
//! machine-local pipe. Installing the Helper never starts or configures it.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::mem::size_of;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use auv_device_helper_windows::{SERVICE_ARGUMENT, SERVICE_DISPLAY_NAME, SERVICE_NAME};
use serde::Serialize;
use windows::Win32::Foundation::{ERROR_SERVICE_DOES_NOT_EXIST, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::Storage::FileSystem::CreateDirectoryW;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};
use windows::Win32::UI::Shell::CommandLineToArgvW;
use windows::core::PCWSTR;
use windows_service::service::{Service, ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType, ServiceState, ServiceType};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

/// The 0.0.28 service that ran `auv.exe serve --windows-service` as LocalSystem.
/// Install replaces it; it is recognized only by its exact recorded command.
const LEGACY_SERVICE_NAME: &str = "AuvDevice";
const LEGACY_LISTEN_URI: &str = "http://127.0.0.1:9847";

mod embedded {
  include!(concat!(env!("OUT_DIR"), "/embedded_windows_helper.rs"));
}

#[derive(Debug)]
struct Layout {
  install_dir: PathBuf,
  helper: PathBuf,
  /// Helper-owned PIN vault, kept across install, upgrade, and uninstall.
  vault_dir: PathBuf,
  legacy_auv: PathBuf,
  legacy_store_root: PathBuf,
  legacy_bootstrap_dir: PathBuf,
  legacy_bootstrap_token: PathBuf,
}

impl Layout {
  fn resolve() -> Result<Self, String> {
    let program_files = std::env::var_os("ProgramFiles").ok_or("Windows did not provide ProgramFiles")?;
    let program_data = std::env::var_os("ProgramData").ok_or("Windows did not provide ProgramData")?;
    Ok(Self::from_roots(Path::new(&program_files), Path::new(&program_data)))
  }

  fn from_roots(program_files: &Path, program_data: &Path) -> Self {
    let install_dir = program_files.join("AUV");
    let legacy_bootstrap_dir = program_data.join("AUVBootstrap");
    Self {
      helper: install_dir.join("auv-helper.exe"),
      legacy_auv: install_dir.join("auv.exe"),
      install_dir,
      vault_dir: program_data.join("AUVDeviceEnrollments"),
      legacy_store_root: program_data.join("AUVDeviceEntry"),
      legacy_bootstrap_token: legacy_bootstrap_dir.join("pairing-token.txt"),
      legacy_bootstrap_dir,
    }
  }

  fn service_info(&self) -> ServiceInfo {
    ServiceInfo {
      name: OsString::from(SERVICE_NAME),
      display_name: OsString::from(SERVICE_DISPLAY_NAME),
      service_type: ServiceType::OWN_PROCESS,
      // A locked login can be unlocked only while the Helper is running; it
      // starts with Windows so no user action is needed after a reboot.
      start_type: ServiceStartType::AutoStart,
      error_control: ServiceErrorControl::Normal,
      executable_path: self.helper.clone(),
      launch_arguments: vec![SERVICE_ARGUMENT.into()],
      dependencies: Vec::new(),
      account_name: None,
      account_password: None,
    }
  }

  /// The exact command the 0.0.28 installer registered for `AuvDevice`.
  fn legacy_command(&self) -> Vec<OsString> {
    let store = self.legacy_store_root.as_os_str().to_owned();
    vec![
      self.legacy_auv.clone().into_os_string(),
      "serve".into(),
      "--windows-service".into(),
      "--listen".into(),
      LEGACY_LISTEN_URI.into(),
      "--store-root".into(),
      store,
      "--pairing-store".into(),
      self.legacy_store_root.join("pairings.json").into_os_string(),
    ]
  }
}

#[derive(Debug, Serialize)]
struct Status {
  state: &'static str,
  service: &'static str,
  service_running: bool,
  helper_installed: bool,
  install_directory: String,
  vault_directory: String,
  /// The 0.0.28 `AuvDevice` daemon service, which install replaces.
  legacy_service: &'static str,
  /// Pairings, policy, and audit kept from the 0.0.28 service; not imported.
  legacy_store_root: Option<String>,
  detail: Option<String>,
}

pub fn status(json: bool) -> Result<i32, String> {
  let layout = Layout::resolve()?;
  let status = inspect(&layout)?;
  print_status(&status, json)?;
  Ok(if status.state == "ready" { 0 } else { 1 })
}

pub fn install(json: bool) -> Result<i32, String> {
  let layout = Layout::resolve()?;
  let helper = embedded::EXECUTABLE
    .ok_or("this auv.exe does not contain the Windows helper; install AUV from an official release, Scoop, or proto before running setup")?;
  let manager = ServiceManager::local_computer(None::<&OsStr>, ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE)
    .map_err(|error| format!("failed to open Windows Service Control Manager (run an elevated terminal): {error}"))?;

  // TODO(windows-helper-upgrade): Replacing an existing AuvHelper in place
  // needs a stop/replace/start rollback contract; until then reinstall.
  if open_service(&manager, SERVICE_NAME, ServiceAccess::QUERY_STATUS)?.is_some() {
    return Err(format!("{SERVICE_NAME} is already installed; run `auv setup windows-helper uninstall` first to replace it"));
  }

  let legacy = open_service(
    &manager,
    LEGACY_SERVICE_NAME,
    ServiceAccess::QUERY_STATUS | ServiceAccess::QUERY_CONFIG | ServiceAccess::START | ServiceAccess::STOP | ServiceAccess::DELETE,
  )?;

  match &legacy {
    Some(service) => {
      require_owned(service, LEGACY_SERVICE_NAME, &layout.legacy_command())?;
      require_known_entries(&layout.install_dir, &[&layout.legacy_auv, &layout.helper])?;
    }
    None if layout.install_dir.exists() => {
      return Err(format!(
        "refusing to replace preexisting installation directory {}; uninstall the owned AUV installation first",
        layout.install_dir.display()
      ));
    }
    None => create_protected_install_directory(&layout.install_dir)?,
  }

  // The 0.0.28 service runs the same one-shot worker arguments, so a
  // restarted legacy service can keep using the new auv-helper.exe.
  let legacy_was_running = match &legacy {
    Some(service) => stop(service, LEGACY_SERVICE_NAME)?,
    None => false,
  };
  let result = (|| {
    fs::write(&layout.helper, helper).map_err(|error| format!("failed to install embedded auv-helper.exe: {error}"))?;
    let service = manager
      .create_service(
        &layout.service_info(),
        ServiceAccess::QUERY_STATUS | ServiceAccess::START | ServiceAccess::STOP | ServiceAccess::DELETE | ServiceAccess::CHANGE_CONFIG,
      )
      .map_err(|error| format!("failed to register {SERVICE_NAME}: {error}"))?;
    service
      .set_description("Performs AUV Device lock, unlock, and PIN storage for the logged-in user's own AUV daemon.")
      .map_err(|error| format!("failed to describe {SERVICE_NAME}: {error}"))?;
    service.start::<&OsStr>(&[]).map_err(|error| format!("failed to start {SERVICE_NAME}: {error}"))?;
    wait_for_state(&service, SERVICE_NAME, ServiceState::Running, Duration::from_secs(30))
  })();

  if let Err(error) = result {
    // Leave an SCM entry only when deletion itself fails; the returned error
    // keeps that recovery detail visible to the administrator.
    if let Ok(Some(service)) =
      open_service(&manager, SERVICE_NAME, ServiceAccess::STOP | ServiceAccess::DELETE | ServiceAccess::QUERY_STATUS)
    {
      let _ = service.stop();
      let _ = service.delete();
    }

    match &legacy {
      Some(service) if legacy_was_running => {
        let _ = service.start::<&OsStr>(&[]);
      }
      Some(_) => {}
      None => {
        let _ = fs::remove_file(&layout.helper);
        let _ = fs::remove_dir(&layout.install_dir);
      }
    }

    return Err(error);
  }

  if let Some(service) = legacy {
    service.delete().map_err(|error| format!("{SERVICE_NAME} is running, but failed to unregister {LEGACY_SERVICE_NAME}: {error}"))?;
    remove_known_file(&layout.legacy_auv)?;
    remove_legacy_bootstrap_files(&layout)?;
  }

  let status = inspect(&layout)?;
  print_status(&status, json)?;
  Ok(if status.state == "ready" { 0 } else { 1 })
}

pub fn uninstall(json: bool) -> Result<i32, String> {
  let layout = Layout::resolve()?;
  let manager = ServiceManager::local_computer(None::<&OsStr>, ServiceManagerAccess::CONNECT)
    .map_err(|error| format!("failed to open Windows Service Control Manager (run an elevated terminal): {error}"))?;
  let access = ServiceAccess::QUERY_STATUS | ServiceAccess::QUERY_CONFIG | ServiceAccess::STOP | ServiceAccess::DELETE;

  for (name, command) in [
    (SERVICE_NAME, owned_command(&layout)),
    (LEGACY_SERVICE_NAME, layout.legacy_command()),
  ] {
    if let Some(service) = open_service(&manager, name, access)? {
      require_owned(&service, name, &command)?;
      stop(&service, name)?;
      service.delete().map_err(|error| format!("failed to unregister {name}: {error}"))?;
    }
  }

  if layout.install_dir.exists() {
    require_known_entries(&layout.install_dir, &[&layout.legacy_auv, &layout.helper])?;
    remove_known_file(&layout.helper)?;
    remove_known_file(&layout.legacy_auv)?;
    fs::remove_dir(&layout.install_dir).map_err(|error| format!("failed to remove {}: {error}", layout.install_dir.display()))?;
  }

  remove_legacy_bootstrap_files(&layout)?;
  // Enrolled PINs are durable user state in the Helper-owned vault, and a
  // 0.0.28 store may still hold pairings and audit. Both are preserved.
  // TODO(windows-helper-purge): add an explicit, separately confirmed purge
  // command only when the owner approves destructive credential removal.
  let status = inspect(&layout)?;
  print_status(&status, json)?;
  Ok(0)
}

fn inspect(layout: &Layout) -> Result<Status, String> {
  let helper_installed = regular_file(&layout.helper);
  let manager = ServiceManager::local_computer(None::<&OsStr>, ServiceManagerAccess::CONNECT)
    .map_err(|error| format!("failed to open Windows Service Control Manager: {error}"))?;
  let service = open_service(&manager, SERVICE_NAME, ServiceAccess::QUERY_STATUS | ServiceAccess::QUERY_CONFIG)?;
  let legacy = open_service(&manager, LEGACY_SERVICE_NAME, ServiceAccess::QUERY_STATUS)?;
  let service_running =
    service.as_ref().and_then(|service| service.query_status().ok()).is_some_and(|status| status.current_state == ServiceState::Running);
  let service_owned = service.as_ref().is_some_and(|service| require_owned(service, SERVICE_NAME, &owned_command(layout)).is_ok());
  let ready = helper_installed && service_running && service_owned && legacy.is_none();
  // TODO(windows-helper-legacy-import): The 0.0.28 SYSTEM-only store is kept
  // but not imported into a daemon store. Importing needs an owner-approved
  // mapping from one machine store to a per-user daemon store.
  let legacy_store_root = layout.legacy_store_root.exists().then(|| layout.legacy_store_root.display().to_string());
  let detail = if ready {
    legacy_store_root.as_ref().map(|_| {
      "0.0.28 pairings, policy, and audit were kept but not imported; pair clients again with the per-user daemon and re-run `auv device-local enroll`".to_string()
    })
  } else if legacy.is_some() {
    Some(format!("the 0.0.28 {LEGACY_SERVICE_NAME} daemon service is still installed; run `auv setup windows-helper install` to replace it"))
  } else if service.is_some() && !service_owned {
    Some(format!("{SERVICE_NAME} exists but is not the AUV Helper service owned by this installation"))
  } else if service.is_some() && !service_running {
    Some(format!("{SERVICE_NAME} is installed but not running"))
  } else if service.is_some() {
    Some("auv-helper.exe is missing from the installation directory".into())
  } else {
    Some("AUV Helper is not installed".into())
  };

  Ok(Status {
    state: if ready {
      "ready"
    } else if service.is_none() && legacy.is_none() && !helper_installed {
      "not_installed"
    } else {
      "degraded"
    },
    service: if service.is_some() {
      "installed"
    } else {
      "absent"
    },
    service_running,
    helper_installed,
    install_directory: layout.install_dir.display().to_string(),
    vault_directory: layout.vault_dir.display().to_string(),
    legacy_service: if legacy.is_some() {
      "installed"
    } else {
      "absent"
    },
    legacy_store_root,
    detail,
  })
}

fn owned_command(layout: &Layout) -> Vec<OsString> {
  let info = layout.service_info();
  std::iter::once(info.executable_path.into_os_string()).chain(info.launch_arguments).collect()
}

fn open_service(manager: &ServiceManager, name: &str, access: ServiceAccess) -> Result<Option<Service>, String> {
  match manager.open_service(name, access) {
    Ok(service) => Ok(Some(service)),
    Err(windows_service::Error::Winapi(error)) if error.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST.0 as i32) => Ok(None),
    Err(error) => Err(format!("failed to open Windows service {name}: {error}")),
  }
}

/// Refuse to manage a same-named service that AUV did not register.
fn require_owned(service: &Service, name: &str, expected: &[OsString]) -> Result<(), String> {
  let config = service.query_config().map_err(|error| format!("failed to inspect {name}: {error}"))?;
  if command_arguments(&config.executable_path.to_string_lossy()).as_deref() != Some(expected)
    || config.start_type != ServiceStartType::AutoStart
    || !config.account_name.as_deref().is_some_and(is_local_system_account)
  {
    return Err(format!("refusing to manage {name}: its executable, arguments, account, or startup type does not match AUV"));
  }
  Ok(())
}

/// Stop a service, wait for its process to exit, and report whether it had
/// been running.
fn stop(service: &Service, name: &str) -> Result<bool, String> {
  let status = service.query_status().map_err(|error| format!("failed to query {name}: {error}"))?;

  if status.current_state == ServiceState::Stopped {
    return Ok(false);
  }

  // SAFETY: OpenProcess returns one owned handle for the SCM-reported PID,
  // which pins that process object until the handle closes.
  let process = status
    .process_id
    .and_then(|pid| unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }.ok())
    .map(|handle| unsafe { OwnedHandle::from_raw_handle(handle.0) });
  service.stop().map_err(|error| format!("failed to stop {name}: {error}"))?;
  wait_for_state(service, name, ServiceState::Stopped, Duration::from_secs(30))?;

  // NOTICE(windows-service-stop-exit): SCM reports Stopped before the
  // service process exits and releases its image file, so an immediate
  // auv-helper.exe replacement or removal failed with ERROR_ACCESS_DENIED
  // on the Windows test host. Wait for the exact process to exit.
  if let Some(process) = process {
    // SAFETY: The owned process handle stays live for this bounded wait.
    unsafe { WaitForSingleObject(HANDLE(process.as_raw_handle()), 10_000) };
  }

  Ok(true)
}

/// Split an SCM image path the way Windows splits a process command line.
fn command_arguments(command: &str) -> Option<Vec<OsString>> {
  struct Arguments(*mut windows::core::PWSTR);
  impl Drop for Arguments {
    fn drop(&mut self) {
      // SAFETY: CommandLineToArgvW allocated this array with LocalAlloc.
      let _ = unsafe { LocalFree(HLOCAL(self.0.cast())) };
    }
  }

  let wide = OsStr::new(command).encode_wide().chain(Some(0)).collect::<Vec<_>>();
  let mut count = 0i32;
  // SAFETY: wide is NUL-terminated and count is live for the call.
  let raw = unsafe { CommandLineToArgvW(PCWSTR(wide.as_ptr()), &mut count) };
  if raw.is_null() || count <= 0 {
    return None;
  }
  let arguments = Arguments(raw);
  Some(
    (0..count as usize)
      .map(|index| {
        // SAFETY: CommandLineToArgvW returned count NUL-terminated pointers.
        let pointer = unsafe { *arguments.0.add(index) };
        let mut length = 0;
        // SAFETY: each returned argument is NUL-terminated.
        while unsafe { *pointer.0.add(length) } != 0 {
          length += 1;
        }
        // SAFETY: length was found within this NUL-terminated argument.
        OsString::from_wide(unsafe { std::slice::from_raw_parts(pointer.0, length) })
      })
      .collect(),
  )
}

fn is_local_system_account(account: &OsStr) -> bool {
  let account = account.to_string_lossy();
  account.eq_ignore_ascii_case("LocalSystem")
    || account.eq_ignore_ascii_case(r"NT AUTHORITY\SYSTEM")
    || account.eq_ignore_ascii_case(r".\LocalSystem")
}

fn wait_for_state(service: &Service, name: &str, expected: ServiceState, timeout: Duration) -> Result<(), String> {
  let deadline = Instant::now() + timeout;
  loop {
    let status = service.query_status().map_err(|error| format!("failed to query {name}: {error}"))?;
    if status.current_state == expected {
      return Ok(());
    }
    if Instant::now() >= deadline {
      return Err(format!("timed out waiting for {name} to reach {expected:?}; current state is {:?}", status.current_state));
    }
    std::thread::sleep(Duration::from_millis(250));
  }
}

fn create_protected_install_directory(path: &Path) -> Result<(), String> {
  struct Descriptor(PSECURITY_DESCRIPTOR);
  impl Drop for Descriptor {
    fn drop(&mut self) {
      // SAFETY: the SDDL conversion allocated this descriptor with LocalAlloc.
      let _ = unsafe { LocalFree(HLOCAL(self.0.0)) };
    }
  }

  let sddl = OsStr::new("O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)").encode_wide().chain(Some(0)).collect::<Vec<_>>();
  let mut raw = PSECURITY_DESCRIPTOR::default();
  // SAFETY: sddl is NUL-terminated and raw receives one LocalAlloc-owned descriptor.
  unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(PCWSTR(sddl.as_ptr()), SDDL_REVISION_1, &mut raw, None) }
    .map_err(|error| format!("failed to build protected AUV directory ACL: {error}"))?;
  let descriptor = Descriptor(raw);
  let attributes = SECURITY_ATTRIBUTES {
    nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
    lpSecurityDescriptor: descriptor.0.0,
    bInheritHandle: false.into(),
  };
  let path_wide = path.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();
  // SAFETY: the path is NUL-terminated and Windows copies the security
  // descriptor during this synchronous CreateDirectoryW call.
  unsafe { CreateDirectoryW(PCWSTR(path_wide.as_ptr()), Some(&attributes)) }
    .map_err(|error| format!("failed to create protected directory {}: {error}", path.display()))
}

/// Refuse to touch a directory that holds anything AUV did not install.
fn require_known_entries(directory: &Path, known: &[&Path]) -> Result<(), String> {
  if !directory.exists() {
    return Ok(());
  }
  let mut unexpected = Vec::new();
  for entry in fs::read_dir(directory).map_err(|error| format!("failed to inspect {}: {error}", directory.display()))? {
    let path = entry.map_err(|error| format!("failed to inspect AUV installation entry: {error}"))?.path();
    if !known.contains(&path.as_path()) {
      unexpected.push(path);
    }
  }
  if !unexpected.is_empty() {
    return Err(format!(
      "refusing to modify {} because it contains unexpected entries: {}",
      directory.display(),
      unexpected.iter().map(|path| path.display().to_string()).collect::<Vec<_>>().join(", ")
    ));
  }
  Ok(())
}

fn remove_known_file(path: &Path) -> Result<(), String> {
  if path.exists() {
    fs::remove_file(path).map_err(|error| format!("failed to remove {}: {error}", path.display()))?;
  }
  Ok(())
}

/// Remove the 0.0.28 first-pairing token directory. The daemon now issues
/// pairing tokens only over its owner channel, so the file has no consumer.
fn remove_legacy_bootstrap_files(layout: &Layout) -> Result<(), String> {
  if !layout.legacy_bootstrap_dir.exists() {
    return Ok(());
  }
  require_known_entries(&layout.legacy_bootstrap_dir, &[&layout.legacy_bootstrap_token])?;
  remove_known_file(&layout.legacy_bootstrap_token)?;
  fs::remove_dir(&layout.legacy_bootstrap_dir)
    .map_err(|error| format!("failed to remove {}: {error}", layout.legacy_bootstrap_dir.display()))
}

fn regular_file(path: &Path) -> bool {
  fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_file())
}

fn print_status(status: &Status, json: bool) -> Result<(), String> {
  if json {
    println!("{}", serde_json::to_string_pretty(status).map_err(|error| format!("failed to encode helper status: {error}"))?);
  } else {
    println!("state\t{}", status.state);
    println!("service\t{}", status.service);
    println!("service_running\t{}", status.service_running);
    println!("helper_installed\t{}", status.helper_installed);
    println!("install_directory\t{}", status.install_directory);
    println!("vault_directory\t{}", status.vault_directory);
    println!("legacy_service\t{}", status.legacy_service);
    if let Some(path) = &status.legacy_store_root {
      println!("legacy_store_root\t{path}");
    }
    if let Some(detail) = &status.detail {
      println!("detail\t{detail}");
    }
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn helper_service_runs_only_the_installed_helper_host() {
    let layout = Layout::from_roots(Path::new(r"C:\Program Files"), Path::new(r"C:\ProgramData"));
    let info = layout.service_info();

    assert_eq!(info.name, OsString::from("AuvHelper"));
    assert_eq!(info.executable_path, PathBuf::from(r"C:\Program Files").join("AUV").join("auv-helper.exe"));
    assert_eq!(info.launch_arguments, [OsString::from("--service")]);
    assert_eq!(info.start_type, ServiceStartType::AutoStart);
    assert!(info.account_name.is_none());
    // ROOT CAUSE:
    //
    // The 0.0.28 helper service ran `auv.exe serve --windows-service`, so
    // installing the Helper also installed a daemon with its own listener,
    // pairing store, and state. The Helper command now carries none of them.
    for argument in &info.launch_arguments {
      for daemon_argument in ["serve", "--listen", "--store-root", "--pairing-store"] {
        assert_ne!(argument, daemon_argument);
      }
    }
  }

  #[test]
  fn configured_embedded_helper_is_a_pe_executable() {
    let Some(helper) = embedded::EXECUTABLE else {
      // Source and Cargo installs intentionally have no release-built payload.
      return;
    };

    assert!(helper.starts_with(b"MZ"));
    assert!(helper.len() > 2);
  }

  #[test]
  fn local_system_config_accepts_windows_service_account_spellings_only() {
    assert!(is_local_system_account(OsStr::new("LocalSystem")));
    assert!(is_local_system_account(OsStr::new(r"NT AUTHORITY\SYSTEM")));
    assert!(!is_local_system_account(OsStr::new(r".\Administrator")));
  }

  #[test]
  fn owned_commands_match_only_their_exact_image_path() {
    let layout = Layout::from_roots(Path::new(r"C:\Program Files"), Path::new(r"C:\ProgramData"));
    let helper = r#""C:\Program Files\AUV\auv-helper.exe" --service"#;
    let legacy = r#""C:\Program Files\AUV\auv.exe" serve --windows-service --listen http://127.0.0.1:9847 --store-root "C:\ProgramData\AUVDeviceEntry" --pairing-store "C:\ProgramData\AUVDeviceEntry\pairings.json""#;

    assert_eq!(command_arguments(helper), Some(owned_command(&layout)));
    assert_ne!(command_arguments(&helper.replace("--service", "--lock")), Some(owned_command(&layout)));
    assert_eq!(command_arguments(legacy), Some(layout.legacy_command()));
    assert_ne!(command_arguments(&legacy.replace(":9847", ":9848")), Some(layout.legacy_command()));
    assert_ne!(command_arguments(&legacy.replace("AUVDeviceEntry", "ForeignStore")), Some(layout.legacy_command()));
  }
}
