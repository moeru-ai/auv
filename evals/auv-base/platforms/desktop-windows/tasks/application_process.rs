//! An admitted launcher and its descendants share one kill-on-close Windows job.

use std::io::Write;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Result, ensure};
use serde_json::{Value, json};
use windows::Win32::Foundation::{BOOL, HANDLE};
use windows::Win32::System::JobObjects::{
  AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
  JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation, QueryInformationJobObject,
  SetInformationJobObject, TerminateJobObject,
};
use windows::Win32::System::Threading::{CREATE_NO_WINDOW, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::core::PCWSTR;

pub(super) struct Processes {
  job: OwnedHandle,
  children: Vec<Child>,
}

impl Processes {
  pub(super) fn new() -> Result<Self> {
    // SAFETY: an unnamed, non-inheritable job is transferred once to OwnedHandle.
    let raw = unsafe { CreateJobObjectW(None, PCWSTR::null()) }?;
    let job = unsafe { OwnedHandle::from_raw_handle(raw.0) };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: limits has the exact information-class layout and stays live for the call.
    unsafe {
      SetInformationJobObject(raw, JobObjectExtendedLimitInformation, std::ptr::from_ref(&limits).cast(), size_of_val(&limits) as u32)?;
    }
    Ok(Self {
      job,
      children: Vec::new(),
    })
  }

  fn handle(&self) -> HANDLE {
    HANDLE(self.job.as_raw_handle())
  }

  /// Helpers wait on stdin before creating an app or inspecting a window. Assign
  /// the helper first, so all later CreateProcess descendants inherit this job.
  pub(super) fn start(&mut self, mut command: Command) -> Result<usize> {
    command.stdin(Stdio::piped()).creation_flags(CREATE_NO_WINDOW.0);
    let child = command.spawn()?;
    self.children.push(child);
    let index = self.children.len() - 1;
    // SAFETY: both handles remain owned throughout assignment. No breakaway flag is enabled.
    unsafe { AssignProcessToJobObject(self.handle(), HANDLE(self.children[index].as_raw_handle())) }?;
    self.children[index].stdin.take().expect("admission pipe").write_all(b"\n")?;
    Ok(index)
  }

  pub(super) fn contains(&self, process_id: u32) -> Result<bool> {
    // SAFETY: query access to a live process, retained until membership is checked.
    let Ok(raw) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }) else {
      return Ok(false);
    };
    let process = unsafe { OwnedHandle::from_raw_handle(raw.0) };
    let mut included = BOOL::default();
    // SAFETY: owned process/job handles and writable BOOL; this checks actual job membership.
    unsafe { IsProcessInJob(HANDLE(process.as_raw_handle()), self.handle(), &mut included) }?;
    Ok(included.as_bool())
  }

  fn active(&self) -> Result<u32> {
    let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    // SAFETY: info has the exact queried layout, size, and writable lifetime.
    unsafe {
      QueryInformationJobObject(
        self.handle(),
        JobObjectBasicAccountingInformation,
        std::ptr::from_mut(&mut info).cast(),
        size_of_val(&info) as u32,
        None,
      )?;
    }
    Ok(info.ActiveProcesses)
  }

  pub(super) fn wait(&mut self, index: usize, limit: Duration) -> Result<i32> {
    let deadline = Instant::now() + limit;
    loop {
      if let Some(status) = self.children[index].try_wait()? {
        return Ok(status.code().unwrap_or(-1));
      }
      if Instant::now() >= deadline {
        // The original Child handle remains here until finish verifies the entire job.
        let _ = self.children[index].kill();
        anyhow::bail!("application inspector exceeded its deadline; termination requested");
      }
      std::thread::sleep(Duration::from_millis(20));
    }
  }

  pub(super) fn finish(&mut self) -> Result<Value> {
    // SAFETY: this job contains only helpers admitted by this controller and their children.
    let terminate_error = unsafe { TerminateJobObject(self.handle(), 1) }.err().map(|error| error.to_string());
    for child in &mut self.children {
      child.stdin.take(); // An unassigned helper exits at admission EOF without launching an app.
      if child.try_wait()?.is_none() {
        let _ = child.kill();
      }
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
      let active = self.active()?;
      let roots_exited =
        self.children.iter_mut().try_fold(true, |all, child| -> std::io::Result<bool> { Ok(child.try_wait()?.is_some() && all) })?;
      if active == 0 && roots_exited {
        return Ok(json!({"active_processes": active, "roots_exited": roots_exited, "terminate_error": terminate_error}));
      }
      ensure!(Instant::now() < deadline, "owned application processes did not all exit: active={active}, roots_exited={roots_exited}");
      std::thread::sleep(Duration::from_millis(20));
    }
  }
}

impl Drop for Processes {
  fn drop(&mut self) {
    // SAFETY: last-resort teardown is confined to this controller's job. A Drop
    // request alone is not a verified cleanup receipt; finish supplies that gate.
    unsafe {
      let _ = TerminateJobObject(self.handle(), 1);
    }
    for child in &mut self.children {
      child.stdin.take();
      let _ = child.kill();
    }
  }
}
