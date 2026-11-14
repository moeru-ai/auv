//! The current user's shared local daemon for the default `auv invoke` and
//! `auv mcp` route: find the registered daemon, or start one.
//!
//! An auto-started daemon is `auv serve` with the user's state directory as
//! its store, registered in local discovery so later CLI calls and SDK
//! clients (`connect({ discover: true })`) share it, local listener only,
//! `os-user` authentication, and an idle exit counted from its last request.
//! See `docs/ai/references/invoke-cli/2026-11-04-invoke-local-runner-default-design.md`.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// Seconds an auto-started daemon (and the Windows logon task) keeps running
/// after its last request; `auv serve --idle-exit-after`.
pub(crate) const IDLE_EXIT_AFTER_SECONDS: u64 = 300;

/// NOTICE(local-daemon-start-deadline): a cold start opens the store and binds
/// the listener in well under a second on the test hosts; 10 s also covers a
/// Windows logon task that Task Scheduler starts in another session.
const START_DEADLINE: Duration = Duration::from_secs(10);
/// How long to keep waiting after our daemon exited: a concurrent first call
/// may have won the discovery lock and still be publishing its descriptor.
const LOSER_GRACE: Duration = Duration::from_secs(2);

/// A daemon the default route can use.
#[derive(Debug)]
pub(crate) struct LocalDaemon {
  /// The daemon's store, which also holds the CLI-side trace records of calls
  /// routed to it.
  pub(crate) store_root: PathBuf,
}

/// Returns the daemon the default route uses: the one `AUV_ENDPOINT` names
/// (never started here), the reachable registered daemon, or a newly started
/// one. A registered daemon that does not answer is replaced, never skipped
/// for the direct path: the new daemon's bind removes a stale Unix socket and
/// its publication replaces the stale descriptor.
pub(crate) async fn connect_or_start() -> Result<LocalDaemon, String> {
  let discovery = auv::discovery::default_path().map_err(|error| error.to_string())?;
  if let Some(endpoint) = std::env::var_os("AUV_ENDPOINT") {
    // Connection failures surface from the route itself, with their reason.
    let endpoint = endpoint.to_string_lossy();
    let descriptor = auv::discovery::read_descriptor(&discovery).ok().flatten();
    // REVIEW: the API does not report a daemon's store root, so an endpoint
    // that is not the registered daemon (an app's private daemon) records
    // CLI-side traces in the user state directory unless --store-root is set.
    let store_root = match descriptor.filter(|descriptor| descriptor.endpoint() == endpoint) {
      Some(descriptor) => store_root_of(&descriptor)?,
      None => auv::discovery::user_state_directory().map_err(|error| error.to_string())?,
    };
    return Ok(LocalDaemon { store_root });
  }
  if let Some(daemon) = reachable(&discovery).await? {
    return Ok(daemon);
  }
  start(&discovery).await
}

/// The registered daemon, when its descriptor names an endpoint that accepts
/// a connection.
async fn reachable(discovery: &Path) -> Result<Option<LocalDaemon>, String> {
  let Some(descriptor) = auv::discovery::read_descriptor(discovery).map_err(|error| error.to_string())? else {
    return Ok(None);
  };
  if auv::Client::connect(descriptor.endpoint(), None).await.is_err() {
    return Ok(None);
  }
  Ok(Some(LocalDaemon {
    store_root: store_root_of(&descriptor)?,
  }))
}

fn store_root_of(descriptor: &auv::discovery::Descriptor) -> Result<PathBuf, String> {
  match descriptor.store_root() {
    Some(root) => Ok(root.to_path_buf()),
    None => auv::discovery::user_state_directory().map_err(|error| error.to_string()),
  }
}

async fn start(discovery: &Path) -> Result<LocalDaemon, String> {
  #[cfg(windows)]
  if session::launch_for_session(session::current_session_id()?) == session::Launch::LogonTask {
    session::logon_task_started(crate::windows_daemon_task::start_user_daemon_task())?;
    return wait(discovery, None, None).await;
  }
  let store_root = auv::discovery::user_state_directory().map_err(|error| error.to_string())?;
  std::fs::create_dir_all(&store_root).map_err(|error| format!("failed to create the AUV store {}: {error}", store_root.display()))?;
  let log = crate::logging::log_directory()?.join("daemon-autostart.log");
  let child = spawn(&store_root, &log)?;
  wait(discovery, Some(child), Some(&log)).await
}

/// Starts `auv serve` detached from this terminal: it must outlive this call
/// and must not receive the terminal's hangup or Ctrl+C.
fn spawn(store_root: &Path, log: &Path) -> Result<std::process::Child, String> {
  let executable = std::env::current_exe().map_err(|error| format!("failed to resolve the auv executable: {error}"))?;
  crate::logging::create_private_directory(log.parent().expect("log file has a directory"))?;
  let output = std::fs::OpenOptions::new()
    .create(true)
    .append(true)
    .open(log)
    .map_err(|error| format!("failed to open the daemon start log {}: {error}", log.display()))?;
  let errors = output.try_clone().map_err(|error| format!("failed to open the daemon start log {}: {error}", log.display()))?;
  let mut command = std::process::Command::new(executable);
  command
    .arg("serve")
    .arg("--store-root")
    .arg(store_root)
    .arg("--idle-exit-after")
    .arg(IDLE_EXIT_AFTER_SECONDS.to_string())
    // The working directory must not pin wherever the first call ran.
    .current_dir(store_root)
    .stdin(std::process::Stdio::null())
    .stdout(output)
    .stderr(errors);
  #[cfg(unix)]
  {
    use std::os::unix::process::CommandExt as _;
    // SAFETY: the hook runs in the forked child before exec and calls only
    // setsid, which is async-signal-safe and touches no parent state.
    unsafe {
      command.pre_exec(|| rustix::process::setsid().map(drop).map_err(std::io::Error::from));
    }
  }
  #[cfg(windows)]
  {
    use std::os::windows::process::CommandExt as _;
    // NOTICE(windows-daemon-detach): DETACHED_PROCESS drops the console, so
    // closing it does not end the daemon; a new process group keeps Ctrl+C
    // there. The child stays in this process's job object, if any (no
    // CREATE_BREAKAWAY_FROM_JOB, which fails when the job forbids it).
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
  }
  command.spawn().map_err(|error| format!("failed to start the AUV daemon: {error}"))
}

/// Waits until discovery names a reachable daemon. The descriptor may come
/// from `child` or from a concurrent call's daemon that won the discovery
/// lock, in which case `child` exits.
async fn wait(discovery: &Path, mut child: Option<std::process::Child>, log: Option<&Path>) -> Result<LocalDaemon, String> {
  let started = tokio::time::Instant::now();
  let mut exited: Option<(tokio::time::Instant, std::process::ExitStatus)> = None;
  loop {
    if let Some(daemon) = reachable(discovery).await? {
      return Ok(daemon);
    }
    if exited.is_none()
      && let Some(status) = child.as_mut().and_then(|child| child.try_wait().ok().flatten())
    {
      exited = Some((tokio::time::Instant::now(), status));
    }
    let see_log = log.map(|log| format!("; see {}", log.display())).unwrap_or_default();
    if let Some((at, status)) = exited
      && at.elapsed() >= LOSER_GRACE
    {
      return Err(format!(
        "the AUV daemon started for this call exited ({status}) without a reachable daemon in {}{see_log}; use --no-daemon to run in this process",
        discovery.display()
      ));
    }
    if started.elapsed() >= START_DEADLINE {
      return Err(format!(
        "no reachable AUV daemon was published in {} within {} s{see_log}; use --no-daemon to run in this process",
        discovery.display(),
        START_DEADLINE.as_secs()
      ));
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
  }
}

/// Session 0 (SSH, services) has no desktop: a daemon started there could not
/// capture or send input, so the user's logon task starts it in their
/// interactive session instead.
#[cfg(windows)]
mod session {
  #[derive(Debug, PartialEq, Eq)]
  pub(super) enum Launch {
    Spawn,
    LogonTask,
  }

  pub(super) fn launch_for_session(session_id: u32) -> Launch {
    if session_id == 0 {
      Launch::LogonTask
    } else {
      Launch::Spawn
    }
  }

  pub(super) fn current_session_id() -> Result<u32, String> {
    let mut session = 0;
    // SAFETY: the out pointer is a live local for this synchronous call.
    unsafe {
      ::windows::Win32::System::RemoteDesktop::ProcessIdToSessionId(::windows::Win32::System::Threading::GetCurrentProcessId(), &mut session)
    }
    .map_err(|error| format!("failed to read this process's Windows session: {error}"))?;
    Ok(session)
  }

  pub(super) fn logon_task_started(started: Result<bool, String>) -> Result<(), String> {
    match started {
      Ok(true) => Ok(()),
      Ok(false) => Err(
        "this process runs in Windows Session 0 (SSH or a service), where a daemon cannot reach the desktop, and the AUV daemon logon task is not registered; run `auv setup windows-helper install` as administrator, or use --no-daemon to run in this process".to_string(),
      ),
      Err(error) => Err(format!("{error}; use --no-daemon to run in this process")),
    }
  }

  #[cfg(test)]
  mod tests {
    use super::*;

    #[test]
    fn session_zero_starts_the_logon_task_instead_of_a_daemon_without_a_desktop() {
      assert_eq!(launch_for_session(0), Launch::LogonTask);
      assert_eq!(launch_for_session(1), Launch::Spawn);
      assert_eq!(launch_for_session(2), Launch::Spawn);
    }

    #[test]
    fn a_missing_logon_task_names_setup_and_the_direct_path() {
      assert_eq!(logon_task_started(Ok(true)), Ok(()));
      let error = logon_task_started(Ok(false)).unwrap_err();
      assert!(error.contains("auv setup windows-helper install"), "{error}");
      assert!(error.contains("--no-daemon"), "{error}");
    }
  }
}
