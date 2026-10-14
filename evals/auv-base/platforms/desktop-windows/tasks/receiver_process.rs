//! Parent side of the `win32-receiver` test object: starts it, speaks its JSON-line protocol, and
//! can end it abruptly to inject a window loss.
//!
//! **Why a receiver is started detached.** Whether a window may take the foreground is decided by
//! the OS from the *receiving process*: it must be the foreground process, have been started by
//! one, have got the last input event, or the foreground lock must have run out. A receiver this
//! harness starts normally is a descendant of the application the user is working in, so it can
//! hold a right that an independently launched, long-running application never has, and a click
//! that merely focuses one of its controls then really takes the foreground. [`Receiver::spawn_detached`]
//! starts the receiver with the shell as its parent and talks to it over a loopback socket, which
//! is the faithful model of the applications a background session targets. The piped
//! [`Receiver::spawn`] is kept as the control that shows the difference.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{
  CREATE_NO_WINDOW, CreateProcessW, DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, InitializeProcThreadAttributeList,
  LPPROC_THREAD_ATTRIBUTE_LIST, OpenProcess, PROC_THREAD_ATTRIBUTE_PARENT_PROCESS, PROCESS_CREATE_PROCESS, PROCESS_INFORMATION,
  STARTF_USESHOWWINDOW, STARTUPINFOEXW, TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{GetShellWindow, GetWindowThreadProcessId, SW_SHOWNOACTIVATE};
use windows::core::{PCWSTR, PWSTR};

/// Keeps a piped child from opening a console window on the user's desktop.
const CREATE_NO_WINDOW_FLAG: u32 = 0x0800_0000;

#[derive(Clone, Debug)]
pub struct ReceiverSpec {
  pub index: usize,
  pub title: String,
  pub left: i32,
  pub top: i32,
  pub width: i32,
  pub height: i32,
  pub color: String,
  /// Bottom of the Z-order, so ordinary windows cover the receiver.
  pub sink: bool,
}

impl ReceiverSpec {
  fn arguments(&self) -> Vec<String> {
    let mut arguments = vec![
      "--title".into(),
      self.title.clone(),
      "--left".into(),
      self.left.to_string(),
      "--top".into(),
      self.top.to_string(),
      "--width".into(),
      self.width.to_string(),
      "--height".into(),
      self.height.to_string(),
      "--color".into(),
      self.color.clone(),
      "--index".into(),
      self.index.to_string(),
    ];
    if self.sink {
      arguments.push("--sink".into());
    }
    arguments
  }
}

/// What the receiving application itself processed so far; see the receiver's module docs.
#[derive(Clone, Debug, Default, Deserialize, serde::Serialize, PartialEq, Eq)]
pub struct State {
  pub clicks: u64,
  pub first_line: i64,
  pub raw: Raw,
  pub activated: u64,
  pub mouse_activate: u64,
  pub focus_set: u64,
  pub minimized: bool,
  pub foreground: bool,
  /// Times the window became the foreground window, and times it handed it straight back.
  pub foreground_taken: u64,
  pub foreground_yielded: u64,
}

#[derive(Clone, Debug, Default, Deserialize, serde::Serialize, PartialEq, Eq)]
pub struct Raw {
  pub button_down: u64,
  pub button_up: u64,
  pub button_move: u64,
  pub button_wheel: u64,
  pub edit_down: u64,
  pub edit_wheel: u64,
  pub edit_hwheel: u64,
  pub edit_move: u64,
  pub frame_down: u64,
}

/// The strip of cells that paints the receiver's click count and scroll line.
#[derive(Clone, Debug, Deserialize)]
pub struct Strip {
  /// Screen position of the first cell's top-left corner.
  pub origin: [i32; 2],
  pub cell: [i32; 2],
  pub cells: i32,
}

/// Where the receiver's controls are on screen, so input can be aimed at them.
#[derive(Clone, Debug, Deserialize)]
pub struct Layout {
  pub hwnd: String,
  pub strip: Strip,
  pub pid: u32,
  pub button_hwnd: String,
  pub edit_hwnd: String,
  pub window_rect: [i32; 4],
  pub button_center: [i32; 2],
  pub edit_center: [i32; 2],
  /// Screen rectangles `[left, top, right, bottom]` of the push button and the label, whose
  /// captions are text a capture can be searched for.
  #[serde(default)]
  pub button_rect: [i32; 4],
  #[serde(default)]
  pub label_rect: [i32; 4],
  #[serde(default)]
  pub edit_rect: [i32; 4],
}

/// A process the harness started detached and holds by handle.
pub(super) struct OwnedProcess(HANDLE);

// SAFETY: a process handle is just a kernel object reference; it is used from one owner at a time.
unsafe impl Send for OwnedProcess {}

impl OwnedProcess {
  pub(super) fn exited(&self) -> bool {
    // SAFETY: a zero-timeout wait on a handle this value owns.
    unsafe { WaitForSingleObject(self.0, 0) == WAIT_OBJECT_0 }
  }

  /// Ends the process and waits for it to be gone. `TerminateProcess` only starts the teardown, and
  /// the process's windows are destroyed as part of it, so a caller that checks for the window right
  /// after would still find it. The process object is signaled once its windows are gone.
  pub(super) fn terminate(&self) {
    // SAFETY: ending the process this value owns and waiting on its handle.
    unsafe {
      let _ = TerminateProcess(self.0, 1);
      let _ = WaitForSingleObject(self.0, 2000);
    }
  }
}

impl Drop for OwnedProcess {
  fn drop(&mut self) {
    // SAFETY: releasing the handle this value owns.
    unsafe {
      let _ = CloseHandle(self.0);
    }
  }
}

enum Link {
  Piped {
    child: Child,
    stdin: Option<ChildStdin>,
  },
  Detached {
    process: OwnedProcess,
    stream: Option<TcpStream>,
  },
}

pub struct Receiver {
  pub spec: ReceiverSpec,
  pub layout: Layout,
  link: Link,
  lines: mpsc::Receiver<Value>,
}

/// The receiver binary built next to the running evaluation binary.
pub fn receiver_binary() -> Result<PathBuf> {
  let path = std::env::current_exe()?.with_file_name("win32-receiver.exe");
  if !path.exists() {
    bail!("{} not found; build it with `cargo build --release -p auv-base-evals --bin win32-receiver`", path.display());
  }
  Ok(path)
}

/// Reads JSON lines from `reader` into a channel until the receiver goes away.
fn forward_lines(reader: impl BufRead + Send + 'static) -> mpsc::Receiver<Value> {
  let (sender, lines) = mpsc::channel();
  std::thread::spawn(move || {
    for line in reader.lines() {
      let Ok(line) = line else { break };
      if let Ok(value) = serde_json::from_str::<Value>(&line)
        && sender.send(value).is_err()
      {
        break;
      }
    }
  });
  lines
}

fn ready_layout(lines: &mpsc::Receiver<Value>) -> Result<Layout> {
  let ready = lines.recv_timeout(Duration::from_secs(10)).context("receiver did not report ready")?;
  anyhow::ensure!(ready["event"] == "ready", "unexpected first receiver line: {ready}");
  serde_json::from_value(ready).context("receiver ready line")
}

pub(super) fn quote(argument: &str) -> String {
  if argument.is_empty() || argument.contains([' ', '\t', '"']) {
    format!("\"{}\"", argument.replace('"', "\\\""))
  } else {
    argument.to_string()
  }
}

/// Starts `command_line` with the shell as its parent process, so the new process holds no
/// right to take the foreground, as an application the user launched earlier would not.
pub(super) fn create_with_shell_parent(command_line: &str) -> Result<OwnedProcess> {
  auv_driver_windows::desktop::ensure_input_desktop();
  // SAFETY: plain Win32 process creation; every handle and buffer outlives the calls that use
  // it, and the attribute list and parent handle are released before returning.
  unsafe {
    let mut shell_pid = 0u32;
    GetWindowThreadProcessId(GetShellWindow(), Some(&mut shell_pid));
    let parent = OpenProcess(PROCESS_CREATE_PROCESS, false, shell_pid).context("open the shell process")?;

    let mut size = 0usize;
    let _ = InitializeProcThreadAttributeList(LPPROC_THREAD_ATTRIBUTE_LIST::default(), 1, 0, &mut size);
    let mut buffer = vec![0u8; size];
    let list = LPPROC_THREAD_ATTRIBUTE_LIST(buffer.as_mut_ptr() as *mut _);
    InitializeProcThreadAttributeList(list, 1, 0, &mut size).context("initialize the attribute list")?;
    UpdateProcThreadAttribute(
      list,
      0,
      PROC_THREAD_ATTRIBUTE_PARENT_PROCESS as usize,
      Some(&parent as *const HANDLE as *const core::ffi::c_void),
      std::mem::size_of::<HANDLE>(),
      None,
      None,
    )
    .context("set the parent process")?;

    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    startup.lpAttributeList = list;
    // An application's first `ShowWindow` uses this instead of what it asks for, so a program
    // that shows its window normally still does not activate it.
    startup.StartupInfo.dwFlags = STARTF_USESHOWWINDOW;
    startup.StartupInfo.wShowWindow = SW_SHOWNOACTIVATE.0 as u16;
    let mut line: Vec<u16> = command_line.encode_utf16().chain(std::iter::once(0)).collect();
    let mut info = PROCESS_INFORMATION::default();
    let created = CreateProcessW(
      PCWSTR::null(),
      PWSTR(line.as_mut_ptr()),
      None,
      None,
      false,
      EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW,
      None,
      PCWSTR::null(),
      &startup.StartupInfo,
      &mut info,
    );
    DeleteProcThreadAttributeList(list);
    let _ = CloseHandle(parent);
    created.context("create the detached receiver")?;
    let _ = CloseHandle(info.hThread);
    Ok(OwnedProcess(info.hProcess))
  }
}

impl Receiver {
  /// Starts the receiver as a child with piped stdio. It is a descendant of this process, so the OS
  /// may let it take the foreground; see the module docs. Prefer [`Self::spawn_detached`].
  pub fn spawn(binary: &Path, spec: &ReceiverSpec, log: &Path) -> Result<Self> {
    let mut child = Command::new(binary)
      .args(spec.arguments())
      .stdin(Stdio::piped())
      .stdout(Stdio::piped())
      .stderr(std::fs::File::create(log)?)
      .creation_flags(CREATE_NO_WINDOW_FLAG)
      .spawn()
      .with_context(|| format!("launch {}", binary.display()))?;
    let stdin = child.stdin.take();
    let lines = forward_lines(BufReader::new(child.stdout.take().context("receiver stdout unavailable")?));
    let layout = ready_layout(&lines)?;
    Ok(Self {
      spec: spec.clone(),
      layout,
      link: Link::Piped { child, stdin },
      lines,
    })
  }

  /// Starts the receiver with the shell as its parent, as an independently launched application
  /// would run, and connects to it over a loopback socket it dials.
  pub fn spawn_detached(binary: &Path, spec: &ReceiverSpec) -> Result<Self> {
    Self::spawn_detached_with(binary, spec, &[])
  }

  /// [`Self::spawn_detached`] with receiver options the spec does not carry, such as `--topmost`
  /// and `--foreground-input`.
  pub fn spawn_detached_with(binary: &Path, spec: &ReceiverSpec, extra: &[&str]) -> Result<Self> {
    let listener = TcpListener::bind("127.0.0.1:0").context("listen on loopback")?;
    listener.set_nonblocking(true)?;
    let address = listener.local_addr()?;
    let mut command_line = quote(&binary.display().to_string());
    let extra = extra.iter().map(|argument| (*argument).to_string());
    for argument in spec.arguments().into_iter().chain(extra).chain(["--connect".into(), address.to_string()]) {
      command_line.push(' ');
      command_line.push_str(&quote(&argument));
    }
    let process = create_with_shell_parent(&command_line)?;

    let started = Instant::now();
    let stream = loop {
      match listener.accept() {
        Ok((stream, _)) => break stream,
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
          if process.exited() {
            bail!("the detached receiver exited before connecting");
          }
          if started.elapsed() > Duration::from_secs(10) {
            process.terminate();
            bail!("the detached receiver never connected");
          }
          std::thread::sleep(Duration::from_millis(10));
        }
        Err(error) => return Err(error.into()),
      }
    };
    stream.set_nonblocking(false)?;
    let _ = stream.set_nodelay(true);
    let lines = forward_lines(BufReader::new(stream.try_clone()?));
    let layout = ready_layout(&lines)?;
    Ok(Self {
      spec: spec.clone(),
      layout,
      link: Link::Detached {
        process,
        stream: Some(stream),
      },
      lines,
    })
  }

  /// Sends one request and waits for its reply. The reply is produced on the receiver's window
  /// thread behind every message already posted to it.
  pub fn request(&mut self, command: Value) -> Result<Value> {
    let mut bytes = serde_json::to_vec(&command)?;
    bytes.push(b'\n');
    match &mut self.link {
      Link::Piped { stdin, .. } => {
        let stdin = stdin.as_mut().context("receiver stdin is closed")?;
        stdin.write_all(&bytes)?;
        stdin.flush()?;
      }
      Link::Detached { stream, .. } => {
        let stream = stream.as_mut().context("receiver socket is closed")?;
        stream.write_all(&bytes)?;
        stream.flush()?;
      }
    }
    let reply = self.lines.recv_timeout(Duration::from_secs(5)).context("receiver did not reply (it may have exited)")?;
    if reply["ok"] == false {
      bail!("receiver refused {command}: {}", reply["error"]);
    }
    Ok(reply)
  }

  pub fn state(&mut self) -> Result<State> {
    Ok(serde_json::from_value(self.request(json!({"cmd": "state"}))?)?)
  }

  pub fn refresh_layout(&mut self) -> Result<Layout> {
    self.layout = serde_json::from_value(self.request(json!({"cmd": "layout"}))?)?;
    Ok(self.layout.clone())
  }

  /// Hands the foreground back to `to`. Harness cleanup after a diagnosed steal, never an
  /// operation under test.
  pub fn yield_foreground(&mut self, to: isize) -> Result<bool> {
    Ok(self.request(json!({"cmd": "yield_foreground", "to": to.to_string()}))?["handed"] == true)
  }

  pub fn hwnd(&self) -> isize {
    self.layout.hwnd.parse().unwrap_or(0)
  }

  pub fn is_detached(&self) -> bool {
    matches!(self.link, Link::Detached { .. })
  }

  fn exited(&mut self) -> bool {
    match &mut self.link {
      Link::Piped { child, .. } => matches!(child.try_wait(), Ok(Some(_))),
      Link::Detached { process, .. } => process.exited(),
    }
  }

  fn close_channel(&mut self) {
    match &mut self.link {
      Link::Piped { stdin, .. } => *stdin = None,
      Link::Detached { stream, .. } => {
        if let Some(stream) = stream.take() {
          let _ = stream.shutdown(std::net::Shutdown::Both);
        }
      }
    }
  }

  /// Ends the receiver process without letting its window close politely, as a crash would.
  pub fn kill(&mut self) {
    match &mut self.link {
      Link::Piped { child, .. } => {
        let _ = child.kill();
        let _ = child.wait();
      }
      Link::Detached { process, .. } => process.terminate(),
    }
    self.close_channel();
  }

  /// Asks the window to close itself and waits for the process to end.
  pub fn close(&mut self) -> Result<()> {
    let _ = self.request(json!({"cmd": "close"}));
    self.close_channel();
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
      if self.exited() {
        return Ok(());
      }
      std::thread::sleep(Duration::from_millis(20));
    }
    self.kill();
    Ok(())
  }
}

impl Drop for Receiver {
  fn drop(&mut self) {
    self.close_channel();
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
      if self.exited() {
        return;
      }
      std::thread::sleep(Duration::from_millis(20));
    }
    self.kill();
  }
}
