//! Windows SCM host for the existing AUV daemon.

use std::ffi::OsString;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use clap::Args;
use tokio_util::sync::CancellationToken;
use windows_service::service::{ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult, ServiceStatusHandle};
use windows_service::service_dispatcher;
use zeroize::Zeroize;

use super::serve::{ServeArgs, host_options, run_listeners_with_shutdown};

const SERVICE_NAME: &str = "AuvDevice";
pub(crate) const BOOTSTRAP_SERVICE_NAME: &str = "AuvDeviceBootstrap";
static CONFIG: OnceLock<(ServeArgs, PathBuf)> = OnceLock::new();
static BOOTSTRAP_OUTPUT: OnceLock<PathBuf> = OnceLock::new();

windows_service::define_windows_service!(service_entry, service_main);
windows_service::define_windows_service!(bootstrap_entry, bootstrap_main);

#[derive(Clone, Debug, Args)]
pub struct BootstrapArgs {
  /// Fixed protected file receiving the one-time token.
  #[arg(long, value_name = "PATH")]
  output: PathBuf,
}

/// Dispatch the process to the SCM. The installer must register this exact
/// service name and pass `serve --windows-service` in its image path.
pub fn run(args: ServeArgs, project_root: PathBuf) -> Result<i32, String> {
  CONFIG.set((args, project_root)).map_err(|_| "AUV service was already dispatched".to_string())?;
  service_dispatcher::start(SERVICE_NAME, service_entry)
    .map_err(|error| format!("failed to join Windows Service Control Manager: {error}"))?;
  Ok(0)
}

/// One-shot, offline bootstrap for an installer running as LocalSystem before
/// the SCM service starts. Only the token digest enters the protected store;
/// the caller must redirect stdout into an administrator-and-SYSTEM-only file.
pub fn run_bootstrap(args: BootstrapArgs) -> Result<i32, String> {
  let expected = bootstrap_token_path()?;
  if args.output != expected {
    return Err(format!("Windows bootstrap output must be {}", expected.display()));
  }
  BOOTSTRAP_OUTPUT.set(args.output).map_err(|_| "AUV bootstrap service was already dispatched".to_string())?;
  service_dispatcher::start(BOOTSTRAP_SERVICE_NAME, bootstrap_entry)
    .map_err(|error| format!("failed to join Windows Service Control Manager for pairing bootstrap: {error}"))?;
  Ok(0)
}

fn bootstrap_main(_scm_arguments: Vec<OsString>) {
  if let Err(error) = serve_bootstrap() {
    eprintln!("AUV Windows bootstrap service failed: {error}");
  }
}

fn serve_bootstrap() -> Result<(), String> {
  let status = service_control_handler::register(BOOTSTRAP_SERVICE_NAME, |control| match control {
    ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
    _ => ServiceControlHandlerResult::NotImplemented,
  })
  .map_err(|error| format!("failed to register bootstrap SCM control handler: {error}"))?;
  report(&status, ServiceState::StartPending, ServiceControlAccept::empty(), 0, 1, Duration::from_secs(30))?;
  let result = issue_bootstrap_token();
  let exit_code = if result.is_ok() { 0 } else { 1 };
  let stopped = report(&status, ServiceState::Stopped, ServiceControlAccept::empty(), exit_code, 0, Duration::ZERO);
  result.and(stopped)
}

fn issue_bootstrap_token() -> Result<(), String> {
  require_local_system_session_zero()?;
  let mut token = auv_daemon::issue_windows_bootstrap_token()?;
  let output = BOOTSTRAP_OUTPUT.get().ok_or("AUV bootstrap output is missing")?;
  let output = (|| {
    let mut file = std::fs::OpenOptions::new()
      .write(true)
      .create_new(true)
      .open(output)
      .map_err(|error| format!("failed to create protected bootstrap token file: {error}"))?;
    file.write_all(token.as_bytes()).map_err(|error| format!("failed to write bootstrap token: {error}"))?;
    file.write_all(b"\n").map_err(|error| format!("failed to terminate bootstrap token: {error}"))?;
    file.sync_all().map_err(|error| format!("failed to persist bootstrap token: {error}"))
  })();
  token.zeroize();
  output?;
  Ok(())
}

fn bootstrap_token_path() -> Result<PathBuf, String> {
  let program_data = std::env::var_os("ProgramData").ok_or("Windows did not provide ProgramData")?;
  Ok(PathBuf::from(program_data).join("AUVBootstrap").join("pairing-token.txt"))
}

fn validate(args: &ServeArgs) -> Result<(), String> {
  validate_with_root(args, &auv_daemon::windows_device_entry_store_root()?)
}

fn validate_with_root(args: &ServeArgs, expected_root: &std::path::Path) -> Result<(), String> {
  let root = args.store_root.as_ref().ok_or("--windows-service requires --store-root")?;
  let pairing = args.pairing_store.as_ref().ok_or("--windows-service requires --pairing-store")?;

  if !root.is_absolute() || !pairing.is_absolute() {
    return Err("Windows service store paths must be absolute".into());
  }

  if root != expected_root || pairing != &root.join("pairings.json") {
    return Err("Windows service requires --store-root at ProgramData\\AUVDeviceEntry and --pairing-store at its pairings.json".into());
  }

  // TODO(windows-service-installed-gate): Protected Windows PairingStore has
  // native unit coverage; require installed LocalSystem, ACL, and listener
  // verification before enabling a live Device-entry service.
  let [listener] = args.listeners.as_slice() else {
    return Err("Windows service requires exactly one explicit http://LOOPBACK_IP:PORT --listen URI".into());
  };

  let address = listener
    .strip_prefix("http://")
    .ok_or("Windows service requires an http://LOOPBACK_IP:PORT --listen URI")?
    .parse::<std::net::SocketAddr>()
    .map_err(|error| format!("invalid Windows service --listen URI: {error}"))?;
  // TODO(windows-service-device-router): Keep the privileged service on
  // loopback until a dedicated Device-only router is reviewed and approved.
  if !address.ip().is_loopback() || address.port() == 0 {
    return Err("Windows service --listen must use a loopback IP and nonzero port".into());
  }

  if args.discovery_file.is_some() || args.daemon_idle_timeout.is_some() || !args.runner_providers.is_empty() {
    return Err("Windows service does not accept --discovery-file, --daemon-idle-timeout, or --runner-provider".into());
  }

  Ok(())
}

fn service_main(_scm_arguments: Vec<OsString>) {
  if let Err(error) = serve_service() {
    eprintln!("AUV Windows service failed: {error}");
  }
}

fn serve_service() -> Result<(), String> {
  let (args, project_root) = CONFIG.get().ok_or("AUV service configuration is missing")?.clone();
  let shutdown = CancellationToken::new();
  let handles = Arc::new(Mutex::new(None::<ServiceStatusHandle>));
  let handler_shutdown = shutdown.clone();
  let handler_handles = Arc::clone(&handles);
  let status = service_control_handler::register(SERVICE_NAME, move |control| match control {
    ServiceControl::Stop => {
      if let Ok(guard) = handler_handles.lock()
        && let Some(handle) = *guard
      {
        let _ = report(&handle, ServiceState::StopPending, ServiceControlAccept::empty(), 0, 1, Duration::from_secs(30));
      }

      handler_shutdown.cancel();
      ServiceControlHandlerResult::NoError
    }
    ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
    _ => ServiceControlHandlerResult::NotImplemented,
  })
  .map_err(|error| format!("failed to register SCM control handler: {error}"))?;
  *handles.lock().map_err(|_| "SCM status lock was poisoned")? = Some(status);

  let result = (|| {
    report(&status, ServiceState::StartPending, ServiceControlAccept::empty(), 0, 1, Duration::from_secs(30))?;
    validate(&args)?;
    require_local_system_session_zero()?;
    let options = service_options(args)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
      .enable_all()
      .build()
      .map_err(|error| format!("failed to create Windows service runtime: {error}"))?;

    let heartbeat_shutdown = shutdown.clone();
    let heartbeat_status = status;
    runtime.spawn(async move {
      heartbeat_shutdown.cancelled().await;
      let mut ticker = tokio::time::interval(Duration::from_secs(10));
      ticker.tick().await;

      for checkpoint in 2.. {
        ticker.tick().await;

        if report(&heartbeat_status, ServiceState::StopPending, ServiceControlAccept::empty(), 0, checkpoint, Duration::from_secs(30))
          .is_err()
        {
          break;
        }
      }
    });
    runtime.block_on(run_listeners_with_shutdown(options, &project_root, shutdown.clone(), || {
      if shutdown.is_cancelled() {
        return Err("service stopped during startup".into());
      }

      report(&status, ServiceState::Running, ServiceControlAccept::STOP, 0, 0, Duration::ZERO)
    }))?;
    Ok::<(), String>(())
  })();

  let exit_code = if result.is_ok() { 0 } else { 1 };
  let stopped = report(&status, ServiceState::Stopped, ServiceControlAccept::empty(), exit_code, 0, Duration::ZERO);
  result.and(stopped)
}

fn service_options(args: ServeArgs) -> Result<super::serve::HostOptions, String> {
  let mut options = host_options(args)?;
  options.publish_discovery = false;
  options.local_driver_runner = false;
  options.emit_bound_endpoints = false;
  options.enable_device_entry = true;
  Ok(options)
}

fn report(
  handle: &ServiceStatusHandle,
  state: ServiceState,
  controls: ServiceControlAccept,
  exit_code: u32,
  checkpoint: u32,
  wait_hint: Duration,
) -> Result<(), String> {
  handle
    .set_service_status(ServiceStatus {
      service_type: ServiceType::OWN_PROCESS,
      current_state: state,
      controls_accepted: controls,
      exit_code: ServiceExitCode::Win32(exit_code),
      checkpoint,
      wait_hint,
      process_id: None,
    })
    .map_err(|error| format!("failed to report SCM state {state:?}: {error}"))
}

fn require_local_system_session_zero() -> Result<(), String> {
  use std::mem::{align_of, size_of};
  use windows::Win32::Foundation::{CloseHandle, HANDLE};
  use windows::Win32::Security::{GetTokenInformation, IsWellKnownSid, TOKEN_QUERY, TOKEN_USER, TokenUser, WinLocalSystemSid};
  use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
  use windows::Win32::System::Threading::{GetCurrentProcess, GetCurrentProcessId, OpenProcessToken};

  struct Token(HANDLE);
  impl Drop for Token {
    fn drop(&mut self) {
      // SAFETY: OpenProcessToken returned this owned handle.
      let _ = unsafe { CloseHandle(self.0) };
    }
  }

  let mut session = u32::MAX;
  // SAFETY: ProcessIdToSessionId writes one live u32.
  unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) }.map_err(|error| error.to_string())?;

  if session != 0 {
    return Err("Windows service must run in Session 0".into());
  }

  let mut raw = HANDLE::default();
  // SAFETY: OpenProcessToken gives one owned handle on success.
  unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) }.map_err(|error| error.to_string())?;
  let token = Token(raw);
  let mut bytes = 0u32;
  // SAFETY: A null buffer queries the required TokenUser size.
  let _ = unsafe { GetTokenInformation(token.0, TokenUser, None, 0, &mut bytes) };

  if bytes < size_of::<TOKEN_USER>() as u32 || align_of::<TOKEN_USER>() > align_of::<usize>() {
    return Err("Windows service token user information is invalid".into());
  }

  let mut data = vec![0usize; (bytes as usize).div_ceil(size_of::<usize>())];
  // SAFETY: The usize buffer is aligned and sized for TOKEN_USER and its SID.
  unsafe { GetTokenInformation(token.0, TokenUser, Some(data.as_mut_ptr().cast()), bytes, &mut bytes) }
    .map_err(|error| error.to_string())?;

  if (bytes as usize) < size_of::<TOKEN_USER>() {
    return Err("Windows service token user information is truncated".into());
  }

  // SAFETY: GetTokenInformation wrote a complete TOKEN_USER header.
  let user = unsafe { data.as_ptr().cast::<TOKEN_USER>().read() };

  if user.User.Sid.0.is_null() || !unsafe { IsWellKnownSid(user.User.Sid, WinLocalSystemSid) }.as_bool() {
    return Err("Windows service must run as LocalSystem".into());
  }

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn service_requires_explicit_private_store_and_listener() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let mut args = ServeArgs {
      id: None,
      listeners: vec!["http://127.0.0.1:9847".into()],
      pairing_store: Some(root.join("pairings.json")),
      store_root: Some(root.to_path_buf()),
      discovery_file: None,
      no_discovery: false,
      daemon_idle_timeout: None,
      runner_providers: Vec::new(),
      windows_service: true,
    };

    assert!(validate_with_root(&args, root).is_ok());

    let service = service_options(args.clone()).unwrap();

    assert!(!service.local_driver_runner);
    assert!(!service.publish_discovery);
    assert!(!service.emit_bound_endpoints);
    assert!(service.enable_device_entry);

    let absent_root = root.join("new-store");
    args.store_root = Some(absent_root.clone());
    args.pairing_store = Some(absent_root.join("pairings.json"));

    assert!(validate_with_root(&args, &absent_root).is_ok());

    args.store_root = Some(root.to_path_buf());
    args.pairing_store = Some(root.join("pairings.json"));

    args.listeners.clear();

    assert!(validate_with_root(&args, root).unwrap_err().contains("--listen"));

    args.listeners.push("http://127.0.0.1:9847".into());
    args.listeners[0] = "http://0.0.0.0:9847".into();

    assert!(validate_with_root(&args, root).unwrap_err().contains("loopback"));

    args.listeners[0] = "http://127.0.0.1:9847".into();
    args.pairing_store = Some(root.join("nested").join("pairings.json"));

    assert!(validate_with_root(&args, root).unwrap_err().contains("pairings.json"));

    args.pairing_store = Some(root.join("pairings.json"));
    args.runner_providers.push(root.join("provider.json"));

    assert!(validate_with_root(&args, root).unwrap_err().contains("--runner-provider"));
  }
}
