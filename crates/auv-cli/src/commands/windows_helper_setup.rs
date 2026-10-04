//! Installed Windows service and helper lifecycle.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::mem::size_of;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;
use windows::Win32::Foundation::{ERROR_SERVICE_DOES_NOT_EXIST, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::Storage::FileSystem::CreateDirectoryW;
use windows::Win32::UI::Shell::CommandLineToArgvW;
use windows::core::PCWSTR;
use windows_service::service::{
  ServiceAccess, ServiceErrorControl, ServiceExitCode, ServiceInfo, ServiceStartType, ServiceState, ServiceType,
};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

const SERVICE_NAME: &str = "AuvDevice";
const DISPLAY_NAME: &str = "AUV Device Helper";
const LISTEN_URI: &str = "http://127.0.0.1:9847";

mod embedded {
  include!(concat!(env!("OUT_DIR"), "/embedded_windows_helper.rs"));
}

#[derive(Debug)]
struct Layout {
  install_dir: PathBuf,
  auv: PathBuf,
  helper: PathBuf,
  store_root: PathBuf,
  bootstrap_dir: PathBuf,
  bootstrap_token: PathBuf,
}

impl Layout {
  fn resolve() -> Result<Self, String> {
    let program_files = std::env::var_os("ProgramFiles").ok_or("Windows did not provide ProgramFiles")?;
    let program_data = std::env::var_os("ProgramData").ok_or("Windows did not provide ProgramData")?;
    Ok(Self::from_roots(Path::new(&program_files), Path::new(&program_data)))
  }

  fn from_roots(program_files: &Path, program_data: &Path) -> Self {
    let install_dir = program_files.join("AUV");
    let bootstrap_dir = program_data.join("AUVBootstrap");
    Self {
      auv: install_dir.join("auv.exe"),
      helper: install_dir.join("auv-helper.exe"),
      install_dir,
      store_root: program_data.join("AUVDeviceEntry"),
      bootstrap_token: bootstrap_dir.join("pairing-token.txt"),
      bootstrap_dir,
    }
  }

  fn service_info(&self) -> ServiceInfo {
    ServiceInfo {
      name: OsString::from(SERVICE_NAME),
      display_name: OsString::from(DISPLAY_NAME),
      service_type: ServiceType::OWN_PROCESS,
      // Unlock must remain available after reboot and before an interactive
      // user can start a foreground process.
      start_type: ServiceStartType::AutoStart,
      error_control: ServiceErrorControl::Normal,
      executable_path: self.auv.clone(),
      launch_arguments: vec![
        "serve".into(),
        "--windows-service".into(),
        "--listen".into(),
        LISTEN_URI.into(),
        "--store-root".into(),
        self.store_root.as_os_str().to_owned(),
        "--pairing-store".into(),
        self.store_root.join("pairings.json").into_os_string(),
      ],
      dependencies: Vec::new(),
      account_name: None,
      account_password: None,
    }
  }
}

#[derive(Debug, Serialize)]
struct Status {
  state: &'static str,
  service: &'static str,
  service_running: bool,
  auv_installed: bool,
  helper_installed: bool,
  install_directory: String,
  store_root: String,
  bootstrap_token_file: Option<String>,
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
  let source_auv = std::env::current_exe().map_err(|error| format!("failed to locate auv.exe: {error}"))?;
  let helper = embedded::EXECUTABLE
    .ok_or("this auv.exe does not contain the Windows helper; install AUV from an official release, Scoop, or proto before running setup")?;
  require_regular_file(&source_auv, "auv.exe")?;

  if layout.install_dir.exists() {
    return Err(format!(
      "refusing to replace preexisting installation directory {}; uninstall the owned AUV installation first",
      layout.install_dir.display()
    ));
  }
  if layout.bootstrap_dir.exists() {
    return Err(format!(
      "refusing to replace preexisting bootstrap directory {}; remove it after verifying its contents",
      layout.bootstrap_dir.display()
    ));
  }

  let manager = ServiceManager::local_computer(None::<&OsStr>, ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE)
    .map_err(|error| format!("failed to open Windows Service Control Manager (run an elevated terminal): {error}"))?;
  if open_service(&manager, SERVICE_NAME, ServiceAccess::QUERY_STATUS)?.is_some() {
    return Err(format!("refusing to replace preexisting Windows service {SERVICE_NAME}"));
  }

  create_protected_install_directory(&layout.install_dir)?;
  if let Err(error) = create_protected_install_directory(&layout.bootstrap_dir) {
    let _ = fs::remove_dir(&layout.install_dir);
    return Err(error);
  }
  let result = (|| {
    fs::copy(&source_auv, &layout.auv).map_err(|error| format!("failed to install auv.exe: {error}"))?;
    fs::write(&layout.helper, helper).map_err(|error| format!("failed to install embedded auv-helper.exe: {error}"))?;
    issue_first_pairing_token(&manager, &layout)?;
    let service = manager
      .create_service(
        &layout.service_info(),
        ServiceAccess::QUERY_STATUS | ServiceAccess::START | ServiceAccess::STOP | ServiceAccess::DELETE | ServiceAccess::CHANGE_CONFIG,
      )
      .map_err(|error| format!("failed to register {SERVICE_NAME}: {error}"))?;
    service
      .set_description("Provides AUV Device session, lock, and unlock operations through a protected LocalSystem host.")
      .map_err(|error| format!("failed to describe {SERVICE_NAME}: {error}"))?;
    service.start::<&OsStr>(&[]).map_err(|error| format!("failed to start {SERVICE_NAME}: {error}"))?;
    wait_for_state(&service, ServiceState::Running, Duration::from_secs(30))
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
    let _ = fs::remove_file(&layout.helper);
    let _ = fs::remove_file(&layout.auv);
    let _ = fs::remove_dir(&layout.install_dir);
    let _ = fs::remove_file(&layout.bootstrap_token);
    let _ = fs::remove_dir(&layout.bootstrap_dir);
    return Err(error);
  }

  let status = inspect(&layout)?;
  print_status(&status, json)?;
  Ok(if status.state == "ready" { 0 } else { 1 })
}

pub fn uninstall(json: bool) -> Result<i32, String> {
  let layout = Layout::resolve()?;
  let manager = ServiceManager::local_computer(None::<&OsStr>, ServiceManagerAccess::CONNECT)
    .map_err(|error| format!("failed to open Windows Service Control Manager (run an elevated terminal): {error}"))?;

  if let Some(service) = open_service(
    &manager,
    SERVICE_NAME,
    ServiceAccess::QUERY_STATUS | ServiceAccess::QUERY_CONFIG | ServiceAccess::STOP | ServiceAccess::DELETE,
  )? {
    require_owned_service(&service, &layout)?;
    if service.query_status().map_err(|error| format!("failed to query {SERVICE_NAME}: {error}"))?.current_state != ServiceState::Stopped {
      service.stop().map_err(|error| format!("failed to stop {SERVICE_NAME}: {error}"))?;
      wait_for_state(&service, ServiceState::Stopped, Duration::from_secs(30))?;
    }
    service.delete().map_err(|error| format!("failed to unregister {SERVICE_NAME}: {error}"))?;
  }

  remove_owned_installation(&layout)?;
  remove_bootstrap_files(&layout)?;
  // Device pairing, policy, audit, and credential enrollment are durable user
  // state. Uninstall intentionally preserves the SYSTEM-only ProgramData root.
  // TODO(windows-helper-purge): add an explicit, separately confirmed purge
  // command only when the owner approves destructive credential removal.
  let status = inspect(&layout)?;
  print_status(&status, json)?;
  Ok(0)
}

pub fn clear_bootstrap_token() -> Result<i32, String> {
  let layout = Layout::resolve()?;
  remove_bootstrap_files(&layout)?;
  println!("bootstrap pairing token file removed");
  Ok(0)
}

fn inspect(layout: &Layout) -> Result<Status, String> {
  let auv_installed = regular_file(&layout.auv);
  let helper_installed = regular_file(&layout.helper);
  let manager = ServiceManager::local_computer(None::<&OsStr>, ServiceManagerAccess::CONNECT)
    .map_err(|error| format!("failed to open Windows Service Control Manager: {error}"))?;
  let service = open_service(&manager, SERVICE_NAME, ServiceAccess::QUERY_STATUS | ServiceAccess::QUERY_CONFIG)?;
  let service_running =
    service.as_ref().and_then(|service| service.query_status().ok()).is_some_and(|status| status.current_state == ServiceState::Running);
  let service_owned = service.as_ref().is_some_and(|service| require_owned_service(service, layout).is_ok());
  let service_name = if service.is_some() {
    "installed"
  } else {
    "absent"
  };
  let ready = auv_installed && helper_installed && service_running && service_owned;
  let detail = if ready {
    None
  } else if service.is_some() && !service_owned {
    Some(format!("{SERVICE_NAME} exists but is not the AUV service owned by this installation"))
  } else if service.is_some() && !service_running {
    Some(format!("{SERVICE_NAME} is installed but not running"))
  } else if auv_installed != helper_installed {
    Some("the installed auv.exe and auv-helper.exe pair is incomplete".into())
  } else {
    Some("Windows helper is not installed".into())
  };

  Ok(Status {
    state: if ready {
      "ready"
    } else if service.is_none() && !auv_installed && !helper_installed {
      "not_installed"
    } else {
      "degraded"
    },
    service: service_name,
    service_running,
    auv_installed,
    helper_installed,
    install_directory: layout.install_dir.display().to_string(),
    store_root: layout.store_root.display().to_string(),
    bootstrap_token_file: regular_file(&layout.bootstrap_token).then(|| layout.bootstrap_token.display().to_string()),
    detail,
  })
}

fn open_service(manager: &ServiceManager, name: &str, access: ServiceAccess) -> Result<Option<windows_service::service::Service>, String> {
  match manager.open_service(name, access) {
    Ok(service) => Ok(Some(service)),
    Err(windows_service::Error::Winapi(error)) if error.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST.0 as i32) => Ok(None),
    Err(error) => Err(format!("failed to open Windows service {name}: {error}")),
  }
}

fn issue_first_pairing_token(manager: &ServiceManager, layout: &Layout) -> Result<(), String> {
  let info = ServiceInfo {
    name: OsString::from(super::windows_service::BOOTSTRAP_SERVICE_NAME),
    display_name: OsString::from("AUV Device Helper Bootstrap"),
    service_type: ServiceType::OWN_PROCESS,
    start_type: ServiceStartType::OnDemand,
    error_control: ServiceErrorControl::Normal,
    executable_path: layout.auv.clone(),
    launch_arguments: vec![
      "windows-bootstrap-pairing-token".into(),
      "--output".into(),
      layout.bootstrap_token.as_os_str().to_owned(),
    ],
    dependencies: Vec::new(),
    account_name: None,
    account_password: None,
  };
  let service = manager
    .create_service(&info, ServiceAccess::QUERY_STATUS | ServiceAccess::START | ServiceAccess::STOP | ServiceAccess::DELETE)
    .map_err(|error| format!("failed to register temporary pairing bootstrap service: {error}"))?;
  let result = (|| {
    service.start::<&OsStr>(&[]).map_err(|error| format!("failed to start pairing bootstrap service: {error}"))?;
    wait_for_bootstrap(&service, &layout.bootstrap_token, Duration::from_secs(30))
  })();
  let deleted = service.delete().map_err(|error| format!("failed to unregister temporary pairing bootstrap service: {error}"));
  result.and(deleted)
}

fn wait_for_bootstrap(service: &windows_service::service::Service, token_file: &Path, timeout: Duration) -> Result<(), String> {
  let deadline = Instant::now() + timeout;
  loop {
    let status = service.query_status().map_err(|error| format!("failed to query pairing bootstrap service: {error}"))?;
    if status.current_state == ServiceState::Stopped && regular_file(token_file) {
      return if status.exit_code == ServiceExitCode::Win32(0) {
        Ok(())
      } else {
        Err(format!("pairing bootstrap service failed with {:?}", status.exit_code))
      };
    }
    if status.current_state == ServiceState::Stopped && status.exit_code != ServiceExitCode::Win32(0) {
      return Err(format!("pairing bootstrap service failed with {:?}", status.exit_code));
    }
    if Instant::now() >= deadline {
      return Err(format!("timed out waiting for pairing bootstrap token at {}", token_file.display()));
    }
    std::thread::sleep(Duration::from_millis(250));
  }
}

fn require_owned_service(service: &windows_service::service::Service, layout: &Layout) -> Result<(), String> {
  let config = service.query_config().map_err(|error| format!("failed to inspect {SERVICE_NAME}: {error}"))?;
  let command = config.executable_path.to_string_lossy();
  if !service_command_is_owned(&command, layout)
    || config.start_type != ServiceStartType::AutoStart
    || !config.account_name.as_deref().is_some_and(is_local_system_account)
  {
    return Err(format!("refusing to manage {SERVICE_NAME}: its executable, arguments, account, or startup type does not match AUV"));
  }
  Ok(())
}

fn service_command_is_owned(command: &str, layout: &Layout) -> bool {
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
    return false;
  }
  let arguments = Arguments(raw);
  let actual = (0..count as usize)
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
    .collect::<Vec<_>>();
  let info = layout.service_info();
  let expected = std::iter::once(info.executable_path.into_os_string()).chain(info.launch_arguments).collect::<Vec<_>>();
  actual == expected
}

fn is_local_system_account(account: &OsStr) -> bool {
  let account = account.to_string_lossy();
  account.eq_ignore_ascii_case("LocalSystem")
    || account.eq_ignore_ascii_case(r"NT AUTHORITY\SYSTEM")
    || account.eq_ignore_ascii_case(r".\LocalSystem")
}

fn wait_for_state(service: &windows_service::service::Service, expected: ServiceState, timeout: Duration) -> Result<(), String> {
  let deadline = Instant::now() + timeout;
  loop {
    let status = service.query_status().map_err(|error| format!("failed to query {SERVICE_NAME}: {error}"))?;
    if status.current_state == expected {
      return Ok(());
    }
    if Instant::now() >= deadline {
      return Err(format!("timed out waiting for {SERVICE_NAME} to reach {expected:?}; current state is {:?}", status.current_state));
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

fn remove_owned_installation(layout: &Layout) -> Result<(), String> {
  if !layout.install_dir.exists() {
    return Ok(());
  }
  let mut unexpected = Vec::new();
  for entry in fs::read_dir(&layout.install_dir).map_err(|error| format!("failed to inspect {}: {error}", layout.install_dir.display()))? {
    let path = entry.map_err(|error| format!("failed to inspect AUV installation entry: {error}"))?.path();
    if path != layout.auv && path != layout.helper {
      unexpected.push(path);
    }
  }
  if !unexpected.is_empty() {
    return Err(format!(
      "refusing to remove {} because it contains unexpected entries: {}",
      layout.install_dir.display(),
      unexpected.iter().map(|path| path.display().to_string()).collect::<Vec<_>>().join(", ")
    ));
  }
  if layout.helper.exists() {
    fs::remove_file(&layout.helper).map_err(|error| format!("failed to remove {}: {error}", layout.helper.display()))?;
  }
  if layout.auv.exists() {
    fs::remove_file(&layout.auv).map_err(|error| format!("failed to remove {}: {error}", layout.auv.display()))?;
  }
  fs::remove_dir(&layout.install_dir).map_err(|error| format!("failed to remove {}: {error}", layout.install_dir.display()))
}

fn remove_bootstrap_files(layout: &Layout) -> Result<(), String> {
  if !layout.bootstrap_dir.exists() {
    return Ok(());
  }
  for entry in
    fs::read_dir(&layout.bootstrap_dir).map_err(|error| format!("failed to inspect {}: {error}", layout.bootstrap_dir.display()))?
  {
    let path = entry.map_err(|error| format!("failed to inspect bootstrap entry: {error}"))?.path();
    if path != layout.bootstrap_token {
      return Err(format!("refusing to remove {} because it contains unexpected entry {}", layout.bootstrap_dir.display(), path.display()));
    }
  }
  if layout.bootstrap_token.exists() {
    fs::remove_file(&layout.bootstrap_token).map_err(|error| format!("failed to remove {}: {error}", layout.bootstrap_token.display()))?;
  }
  fs::remove_dir(&layout.bootstrap_dir).map_err(|error| format!("failed to remove {}: {error}", layout.bootstrap_dir.display()))
}

fn require_regular_file(path: &Path, label: &str) -> Result<(), String> {
  if regular_file(path) {
    Ok(())
  } else {
    Err(format!("required {label} is missing or not a regular file at {}", path.display()))
  }
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
    println!("auv_installed\t{}", status.auv_installed);
    println!("helper_installed\t{}", status.helper_installed);
    println!("install_directory\t{}", status.install_directory);
    println!("store_root\t{}", status.store_root);
    if let Some(path) = &status.bootstrap_token_file {
      println!("bootstrap_token_file\t{path}");
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
  fn service_plan_installs_both_binaries_and_fixed_private_store() {
    let layout = Layout::from_roots(Path::new(r"C:\Program Files"), Path::new(r"C:\ProgramData"));
    let info = layout.service_info();

    assert_eq!(layout.auv, PathBuf::from(r"C:\Program Files").join("AUV").join("auv.exe"));
    assert_eq!(layout.helper, PathBuf::from(r"C:\Program Files").join("AUV").join("auv-helper.exe"));
    assert_eq!(layout.store_root, PathBuf::from(r"C:\ProgramData").join("AUVDeviceEntry"));
    assert_eq!(layout.bootstrap_token, PathBuf::from(r"C:\ProgramData").join("AUVBootstrap").join("pairing-token.txt"));
    assert_eq!(info.start_type, ServiceStartType::AutoStart);
    assert!(info.account_name.is_none());
    assert_eq!(
      info.launch_arguments,
      [
        "serve",
        "--windows-service",
        "--listen",
        "http://127.0.0.1:9847",
        "--store-root",
        r"C:\ProgramData\AUVDeviceEntry",
        "--pairing-store",
        r"C:\ProgramData\AUVDeviceEntry\pairings.json",
      ]
      .map(OsString::from)
    );
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
  fn owned_service_command_rejects_a_different_listener_or_store() {
    let layout = Layout::from_roots(Path::new(r"C:\Program Files"), Path::new(r"C:\ProgramData"));
    let valid = r#""C:\Program Files\AUV\auv.exe" serve --windows-service --listen http://127.0.0.1:9847 --store-root "C:\ProgramData\AUVDeviceEntry" --pairing-store "C:\ProgramData\AUVDeviceEntry\pairings.json""#;

    assert!(service_command_is_owned(valid, &layout));
    assert!(!service_command_is_owned(&valid.replace(":9847", ":9848"), &layout));
    assert!(!service_command_is_owned(&valid.replace("AUVDeviceEntry", "ForeignStore"), &layout));
  }
}
