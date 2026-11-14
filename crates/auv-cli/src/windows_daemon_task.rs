//! Per-user Windows logon task that runs the AUV daemon in the user's
//! interactive session.
//!
//! `auv setup windows-helper install` registers it for the user who ran
//! setup, and the CLI starts it on demand from sessions that cannot reach the
//! desktop (Session 0 over SSH). The task runs `auv serve` with the user's
//! limited token: an elevated daemon would break the Helper's caller SID ==
//! target SID rule and give same-user callers an elevated process. See
//! `docs/ai/references/invoke-cli/2026-11-04-invoke-local-runner-default-design.md`.
//!
//! The task is registered from an XML definition through the Task Scheduler
//! COM API (`ITaskService`). `schtasks.exe` cannot set `RunLevel`, the
//! interactive-token logon type and the execution limit together without the
//! same XML, and its `/query` output is localized text; COM returns typed
//! state and HRESULTs.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{E_ACCESSDENIED, ERROR_FILE_NOT_FOUND, RPC_E_CHANGED_MODE};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize};
use windows::Win32::System::TaskScheduler::{
  IExecAction, IRegisteredTask, ITaskFolder, ITaskService, TASK_CREATE_OR_UPDATE, TASK_LOGON_INTERACTIVE_TOKEN, TASK_LOGON_TYPE,
  TASK_RUNLEVEL_LUA, TASK_RUNLEVEL_TYPE, TASK_STATE, TASK_STATE_DISABLED, TASK_STATE_QUEUED, TASK_STATE_READY, TASK_STATE_RUNNING,
  TaskScheduler,
};
use windows::core::{BSTR, HRESULT, Interface as _, VARIANT};

/// *Provisional* name of the logon task in the Task Scheduler root folder.
// NOTICE: One machine-wide name means one registered user per machine; a
// second user's install refuses the first user's task as foreign. A
// per-session or per-user daemon is out of scope in the design.
pub const TASK_NAME: &str = "AUV Daemon";

/// `SCHED_S_TASK_HAS_NOT_RUN`: the task was registered but never started.
const TASK_HAS_NOT_RUN: i32 = 0x0004_1303;

/// The daemon logon task of one user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DaemonTask {
  name: String,
  user_sid: String,
  executable: PathBuf,
  working_directory: PathBuf,
  arguments: String,
}

impl DaemonTask {
  /// The task for the user running this process, starting the `auv.exe`
  /// that runs now.
  pub(crate) fn for_current_user() -> Result<Self, String> {
    let user_sid =
      auv_api_server::windows_pipe::current_user_sid().map_err(|error| format!("failed to read the current Windows user SID: {error}"))?;
    let executable = std::env::current_exe().map_err(|error| format!("failed to resolve the auv executable: {error}"))?;
    let store_root = auv::discovery::user_state_directory().map_err(|error| error.to_string())?;
    let home = directories::BaseDirs::new().ok_or("failed to resolve the current user's profile directory")?.home_dir().to_path_buf();
    Ok(Self::new(TASK_NAME, user_sid, &stable_executable(&executable), home, &store_root))
  }

  fn new(name: &str, user_sid: String, executable: &Path, working_directory: PathBuf, store_root: &Path) -> Self {
    // Registered in discovery (no `--no-register`), local named pipe only (no
    // `--remote-bind`), `os-user` local authentication by default, and the
    // same idle exit as a daemon the CLI starts: the CLI runs the task again
    // on demand. A task registered before the idle exit compares as stale
    // (`current: false`), and `install` repairs it.
    let arguments = [
      "serve".into(),
      "--store-root".into(),
      store_root.as_os_str().to_owned(),
      "--idle-exit-after".into(),
      crate::local_daemon::IDLE_EXIT_AFTER_SECONDS.to_string().into(),
    ];
    Self {
      name: name.into(),
      user_sid,
      executable: executable.to_path_buf(),
      working_directory,
      arguments: command_line(&arguments),
    }
  }

  pub(crate) fn name(&self) -> &str {
    &self.name
  }

  /// Task Scheduler 1.2 XML for this task.
  fn definition_xml(&self) -> String {
    let sid = xml_escape(&self.user_sid);
    let command = xml_escape(&self.executable.to_string_lossy());
    let arguments = xml_escape(&self.arguments);
    let working_directory = xml_escape(&self.working_directory.to_string_lossy());
    // NOTICE: `ExecutionTimeLimit` defaults to 72 hours, after which Task
    // Scheduler terminates the daemon; `PT0S` disables the limit. Priority
    // defaults to 7 (below normal); 5 keeps the daemon at normal priority
    // because it serves interactive input and capture. `IgnoreNew` makes an
    // on-demand run while the daemon is running a no-op instead of a second
    // daemon that would lose the discovery lock.
    // https://learn.microsoft.com/en-us/windows/win32/taskschd/task-scheduler-schema
    format!(
      r#"<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Runs the AUV daemon (auv serve) in this user's interactive session. Registered by `auv setup windows-helper install`.</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{sid}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{sid}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>5</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{command}</Command>
      <Arguments>{arguments}</Arguments>
      <WorkingDirectory>{working_directory}</WorkingDirectory>
    </Exec>
  </Actions>
</Task>
"#
    )
  }

  /// Register or replace this task. The principal is the interactive token of
  /// `user_sid`, so no password is stored and the elevated token of the
  /// registering process is never used.
  pub(crate) fn register(&self) -> Result<(), String> {
    let scheduler = Scheduler::connect()?;
    let folder = scheduler.root()?;
    // SAFETY: COM is initialized for this thread by `Scheduler`; all BSTR
    // and VARIANT arguments live for the synchronous call.
    unsafe {
      folder.RegisterTask(
        &BSTR::from(self.name.as_str()),
        &BSTR::from(self.definition_xml()),
        TASK_CREATE_OR_UPDATE.0,
        &VARIANT::from(self.user_sid.as_str()),
        &VARIANT::new(),
        TASK_LOGON_INTERACTIVE_TOKEN,
        &VARIANT::new(),
      )
    }
    .map(drop)
    .map_err(|error| format!("failed to register the {} scheduled task: {error}", self.name))
  }

  /// Read the registered task with this name and whether it is this task.
  pub(crate) fn inspect(&self) -> Result<TaskStatus, String> {
    let scheduler = Scheduler::connect()?;
    let folder = scheduler.root()?;
    let Some(task) = get_task(&folder, &self.name)? else {
      return Ok(TaskStatus::default());
    };
    let read = |error: windows::core::Error| format!("failed to inspect the {} scheduled task: {error}", self.name);
    // SAFETY: COM is initialized for this thread by `Scheduler`; the task
    // interface is live for these synchronous property reads.
    let (enabled, state, last_result) =
      unsafe { (task.Enabled().map_err(read)?.as_bool(), task.State().map_err(read)?, task.LastTaskResult().map_err(read)?) };
    let (owned, current) = self.compare(&task).map_err(read)?;
    Ok(TaskStatus {
      registered: true,
      owned,
      current,
      enabled,
      state: state_name(state),
      last_run_result: (last_result != TASK_HAS_NOT_RUN).then(|| format!("0x{:08X}", last_result as u32)),
    })
  }

  /// Delete the task with this name. Returns false when none is registered.
  /// A running daemon started by the task keeps running: deleting a task does
  /// not stop its instances, and `IRegisteredTask::Stop` would terminate the
  /// daemon without its graceful shutdown.
  pub(crate) fn unregister(&self) -> Result<bool, String> {
    let scheduler = Scheduler::connect()?;
    let folder = scheduler.root()?;
    // SAFETY: COM is initialized for this thread by `Scheduler`.
    match unsafe { folder.DeleteTask(&BSTR::from(self.name.as_str()), 0) } {
      Ok(()) => Ok(true),
      Err(error) if is_not_found(error.code()) => Ok(false),
      Err(error) => Err(format!("failed to unregister the {} scheduled task: {error}", self.name)),
    }
  }

  /// Compare a registered task with this one. Returns whether its principal
  /// is this user (`owned`) and whether it is also exactly this task
  /// (`current`): one exec action with this user's daemon arguments, run with
  /// the limited interactive token. The executable path is not compared, so an
  /// `auv.exe` from another install location can still manage the task.
  fn compare(&self, task: &IRegisteredTask) -> windows::core::Result<(bool, bool)> {
    // SAFETY: COM is initialized for this thread by the caller's `Scheduler`;
    // every out pointer is a live local for its synchronous call.
    unsafe {
      let definition = task.Definition()?;
      let principal = definition.Principal()?;
      let mut user_id = BSTR::new();
      principal.UserId(&mut user_id)?;
      // NOTICE: `register` passes the SID, and Task Scheduler stores and
      // returns the principal as given (`schtasks /query /xml` on the Windows
      // test host shows the SID). A task registered with an account name is
      // treated as another user's; resolving names to SIDs is not needed for
      // tasks this module registers.
      if !user_id.to_string().eq_ignore_ascii_case(&self.user_sid) {
        return Ok((false, false));
      }
      let mut logon_type = TASK_LOGON_TYPE::default();
      principal.LogonType(&mut logon_type)?;
      let mut run_level = TASK_RUNLEVEL_TYPE::default();
      principal.RunLevel(&mut run_level)?;
      let actions = definition.Actions()?;
      let mut count = 0;
      actions.Count(&mut count)?;
      if logon_type != TASK_LOGON_INTERACTIVE_TOKEN || run_level != TASK_RUNLEVEL_LUA || count != 1 {
        return Ok((true, false));
      }
      let Ok(exec) = actions.get_Item(1)?.cast::<IExecAction>() else {
        return Ok((true, false));
      };
      let mut arguments = BSTR::new();
      exec.Arguments(&mut arguments)?;
      Ok((true, arguments.to_string() == self.arguments))
    }
  }
}

/// Registered state of the daemon task, for `windows-helper status`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TaskStatus {
  pub registered: bool,
  /// The registered task runs as this user, so this user may replace or
  /// delete it.
  pub owned: bool,
  /// The registered task is also exactly this user's current AUV daemon task;
  /// an owned task that is not current has a stale action or principal
  /// settings and is repaired by registering it again.
  pub current: bool,
  pub enabled: bool,
  pub state: &'static str,
  /// `LastTaskResult` as an HRESULT or exit code; `None` before the first run.
  pub last_run_result: Option<String>,
}

/// Start the current user's daemon logon task now. Returns false when the
/// task is not registered.
///
/// Task Scheduler starts the task in the user's interactive session, also
/// when the caller runs in Session 0 (SSH, services). The user must be
/// logged on; the call does not wait for the daemon to publish discovery.
// TODO(windows-daemon-task-without-admin): only `auv setup windows-helper
// install` (administrator) registers the task; a non-administrator path that
// registers only the task needs an owner-approved slice.
pub fn start_user_daemon_task() -> Result<bool, String> {
  start(TASK_NAME)
}

fn start(name: &str) -> Result<bool, String> {
  let scheduler = Scheduler::connect()?;
  let folder = scheduler.root()?;
  let Some(task) = get_task(&folder, name)? else {
    return Ok(false);
  };
  // SAFETY: COM is initialized for this thread by `Scheduler`; the empty
  // VARIANT carries no task parameters.
  unsafe { task.Run(&VARIANT::new()) }.map_err(|error| format!("failed to start the {name} scheduled task: {error}"))?;
  Ok(true)
}

/// An initialized COM apartment and a connected Task Scheduler service. Bind
/// it before any interface it returns, so COM stays initialized until those
/// interfaces drop.
struct Scheduler {
  service: ITaskService,
  _com: Com,
}

impl Scheduler {
  fn connect() -> Result<Self, String> {
    let com = Com::initialize()?;
    // SAFETY: COM is initialized above; the CLSID names the in-process
    // Task Scheduler client, and empty VARIANTs connect to the local machine
    // as the current user.
    let service = unsafe {
      let service: ITaskService = CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)
        .map_err(|error| format!("failed to create the Task Scheduler client: {error}"))?;
      service
        .Connect(&VARIANT::new(), &VARIANT::new(), &VARIANT::new(), &VARIANT::new())
        .map_err(|error| format!("failed to connect to Task Scheduler: {error}"))?;
      service
    };
    Ok(Self { service, _com: com })
  }

  fn root(&self) -> Result<ITaskFolder, String> {
    // SAFETY: the service is connected and COM stays initialized while `self` lives.
    unsafe { self.service.GetFolder(&BSTR::from("\\")) }.map_err(|error| format!("failed to open the Task Scheduler root folder: {error}"))
  }
}

/// Per-call COM initialization. A thread that already chose another
/// apartment model keeps it and is not uninitialized here.
struct Com {
  initialized: bool,
}

impl Com {
  fn initialize() -> Result<Self, String> {
    // SAFETY: reserved must be null; a successful call is balanced in Drop.
    let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if result == RPC_E_CHANGED_MODE {
      return Ok(Self { initialized: false });
    }
    result.ok().map_err(|error| format!("failed to initialize COM: {error}"))?;
    Ok(Self { initialized: true })
  }
}

impl Drop for Com {
  fn drop(&mut self) {
    if self.initialized {
      // SAFETY: balances the successful CoInitializeEx on this thread.
      unsafe { CoUninitialize() };
    }
  }
}

fn get_task(folder: &ITaskFolder, name: &str) -> Result<Option<IRegisteredTask>, String> {
  // SAFETY: the folder belongs to a live `Scheduler` on this thread.
  match unsafe { folder.GetTask(&BSTR::from(name)) } {
    Ok(task) => Ok(Some(task)),
    Err(error) if is_not_found(error.code()) => Ok(None),
    Err(error) if error.code() == E_ACCESSDENIED => {
      Err(format!("the {name} scheduled task exists but this account cannot read it; run an elevated terminal"))
    }
    Err(error) => Err(format!("failed to open the {name} scheduled task: {error}")),
  }
}

fn is_not_found(code: HRESULT) -> bool {
  code == ERROR_FILE_NOT_FOUND.to_hresult()
}

fn state_name(state: TASK_STATE) -> &'static str {
  match state {
    TASK_STATE_DISABLED => "disabled",
    TASK_STATE_QUEUED => "queued",
    TASK_STATE_READY => "ready",
    TASK_STATE_RUNNING => "running",
    _ => "unknown",
  }
}

/// The path the task should start for `executable`.
///
/// NOTICE: Scoop installs each version under `<scoop>\apps\auv\<version>\`
/// and points the `current` junction at the active one. A task naming a
/// versioned directory would keep starting the old version after `scoop
/// update` and break once Scoop removes it, so a versioned Scoop path is
/// mapped to `current` when that junction holds the same executable name.
/// Any other path, such as a release archive or `target\debug`, is used as is.
fn stable_executable(executable: &Path) -> PathBuf {
  let current = (|| {
    let file_name = executable.file_name()?;
    let version = executable.parent()?;
    let app = version.parent()?;
    let apps = app.parent()?;
    let is = |path: &Path, name: &str| path.file_name().is_some_and(|value| value.eq_ignore_ascii_case(name));
    if !is(app, "auv") || !is(apps, "apps") || is(version, "current") {
      return None;
    }
    let current = app.join("current").join(file_name);
    current.is_file().then_some(current)
  })();
  current.unwrap_or_else(|| executable.to_path_buf())
}

/// Join arguments into one Windows command line that `CommandLineToArgvW`
/// and the MSVC runtime split back into the same arguments.
/// https://learn.microsoft.com/en-us/cpp/c-language/parsing-c-command-line-arguments
fn command_line(arguments: &[OsString]) -> String {
  let mut line = String::new();
  for argument in arguments {
    if !line.is_empty() {
      line.push(' ');
    }
    let argument = argument.to_string_lossy();
    if !argument.is_empty() && !argument.contains([' ', '\t', '"']) {
      line.push_str(&argument);
      continue;
    }
    line.push('"');
    let mut backslashes = 0;
    for character in argument.chars() {
      match character {
        '\\' => backslashes += 1,
        '"' => {
          line.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
          line.push('"');
          backslashes = 0;
        }
        _ => {
          line.extend(std::iter::repeat_n('\\', backslashes));
          line.push(character);
          backslashes = 0;
        }
      }
    }
    line.extend(std::iter::repeat_n('\\', backslashes * 2));
    line.push('"');
  }
  line
}

fn xml_escape(value: &str) -> String {
  let mut escaped = String::with_capacity(value.len());
  for character in value.chars() {
    match character {
      '&' => escaped.push_str("&amp;"),
      '<' => escaped.push_str("&lt;"),
      '>' => escaped.push_str("&gt;"),
      '"' => escaped.push_str("&quot;"),
      '\'' => escaped.push_str("&apos;"),
      _ => escaped.push(character),
    }
  }
  escaped
}

#[cfg(test)]
mod tests {
  use super::*;

  fn task() -> DaemonTask {
    DaemonTask::new(
      TASK_NAME,
      "S-1-5-21-1-2-3-1001".into(),
      Path::new(r"C:\Users\A & B\scoop\apps\auv\current\auv.exe"),
      PathBuf::from(r"C:\Users\A & B"),
      Path::new(r"C:\Users\A & B\AppData\Local\moeru\auv\data"),
    )
  }

  #[test]
  fn daemon_task_runs_registered_local_serve_with_the_user_store() {
    let task = task();

    assert_eq!(task.arguments, r#"serve --store-root "C:\Users\A & B\AppData\Local\moeru\auv\data" --idle-exit-after 300"#);
    for flag in [
      "--no-register",
      "--remote-bind",
      "--listen",
      "--daemon-idle-timeout",
    ] {
      assert!(!task.arguments.contains(flag), "{flag}");
    }
    let xml = task.definition_xml();
    assert!(xml.contains(r"<Command>C:\Users\A &amp; B\scoop\apps\auv\current\auv.exe</Command>"));
    assert!(xml.contains(
      r#"<Arguments>serve --store-root &quot;C:\Users\A &amp; B\AppData\Local\moeru\auv\data&quot; --idle-exit-after 300</Arguments>"#
    ));
    assert!(xml.contains(r"<WorkingDirectory>C:\Users\A &amp; B</WorkingDirectory>"));
  }

  #[test]
  fn daemon_task_starts_at_this_users_logon_with_the_limited_interactive_token() {
    let xml = task().definition_xml();

    // ROOT CAUSE:
    //
    // Setup runs elevated. A task with `HighestAvailable` or a
    // password/S4U logon would run the daemon elevated or outside the
    // user's interactive session, which breaks the Helper's caller SID ==
    // target SID rule and cannot reach the desktop.
    assert!(xml.contains("<Principal id=\"Author\">\n      <UserId>S-1-5-21-1-2-3-1001</UserId>\n      <LogonType>InteractiveToken</LogonType>\n      <RunLevel>LeastPrivilege</RunLevel>"));
    assert!(!xml.contains("HighestAvailable"));
    // A logon trigger without a UserId fires at every user's logon.
    assert!(xml.contains("<LogonTrigger>\n      <Enabled>true</Enabled>\n      <UserId>S-1-5-21-1-2-3-1001</UserId>"));
    assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
    assert!(xml.contains("<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>"));
    assert!(xml.contains("<AllowStartOnDemand>true</AllowStartOnDemand>"));
  }

  #[test]
  fn command_line_quotes_paths_for_windows_argument_splitting() {
    let line = command_line(&[
      "serve".into(),
      r"C:\a b\".into(),
      String::new().into(),
      r#"x"y"#.into(),
    ]);

    assert_eq!(line, r#"serve "C:\a b\\" "" "x\"y""#);
  }

  #[test]
  fn versioned_scoop_executable_maps_to_the_current_junction() {
    let root = tempfile::tempdir().expect("temporary directory");
    let app = root.path().join("scoop").join("apps").join("auv");
    for version in ["0.0.40", "current"] {
      std::fs::create_dir_all(app.join(version)).expect("app directory");
      std::fs::write(app.join(version).join("auv.exe"), b"MZ").expect("executable");
    }
    let versioned = app.join("0.0.40").join("auv.exe");
    let current = app.join("current").join("auv.exe");
    let release = root.path().join("auv-0.0.40").join("auv.exe");

    assert_eq!(stable_executable(&versioned), current);
    assert_eq!(stable_executable(&current), current);
    assert_eq!(stable_executable(&release), release);
  }
}
