use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use super::super::*;
use super::protocol::{
  OverlayDaemonAck, OverlayDaemonCommand, parse_overlay_ack, serialize_overlay_command,
};

pub(crate) struct OverlayDaemon {
  child: Child,
  stdin: Option<ChildStdin>,
  stdout: BufReader<ChildStdout>,
  stderr: Option<ChildStderr>,
  script_path: PathBuf,
}

impl OverlayDaemon {
  pub(crate) fn spawn() -> AuvResult<Self> {
    let script_path = temp_file_path("overlay-cursor-daemon", "swift");
    fs::write(&script_path, OVERLAY_CURSOR_DAEMON_SCRIPT).map_err(|error| {
      format!(
        "failed to write overlay daemon script {}: {error}",
        script_path.display()
      )
    })?;

    let mut child = Command::new(XCRUN_BINARY)
      .arg("swift")
      .arg(&script_path)
      .stdin(Stdio::piped())
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .spawn()
      .or_else(|xcrun_error| {
        if xcrun_error.kind() == std::io::ErrorKind::NotFound {
          Command::new("swift")
            .arg(&script_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        } else {
          Err(xcrun_error)
        }
      })
      .map_err(|error| format!("failed to spawn overlay daemon: {error}"))?;

    let stdin = child
      .stdin
      .take()
      .ok_or_else(|| "overlay daemon stdin was not piped".to_string())?;
    let stdout = child
      .stdout
      .take()
      .ok_or_else(|| "overlay daemon stdout was not piped".to_string())?;
    let stderr = child.stderr.take();

    Ok(Self {
      child,
      stdin: Some(stdin),
      stdout: BufReader::new(stdout),
      stderr,
      script_path,
    })
  }

  pub(crate) fn pid(&self) -> u32 {
    self.child.id()
  }

  pub(crate) fn send(&mut self, command: OverlayDaemonCommand) -> AuvResult<OverlayDaemonAck> {
    let payload = serialize_overlay_command(&command)?;
    let stdin = self
      .stdin
      .as_mut()
      .ok_or_else(|| "overlay daemon stdin is already closed".to_string())?;
    writeln!(stdin, "{payload}")
      .and_then(|_| stdin.flush())
      .map_err(|error| format!("failed to send overlay daemon command: {error}"))?;

    let mut line = String::new();
    let bytes = self
      .stdout
      .read_line(&mut line)
      .map_err(|error| format!("failed to read overlay daemon ack: {error}"))?;
    if bytes == 0 {
      return Err(self.daemon_exit_error());
    }

    let ack = parse_overlay_ack(&line)?;
    if ack.ok {
      Ok(ack)
    } else {
      Err(
        ack
          .error
          .unwrap_or_else(|| format!("overlay daemon reported failed event {}", ack.event)),
      )
    }
  }

  pub(crate) fn close_stdin(&mut self) {
    self.stdin.take();
  }

  pub(crate) fn shutdown(&mut self) -> AuvResult<()> {
    if self.stdin.is_some() {
      let _ = self.send(OverlayDaemonCommand::Shutdown);
    }
    self.close_stdin();
    self.wait_or_kill(Duration::from_millis(750))
  }

  fn daemon_exit_error(&mut self) -> String {
    let mut stderr = String::new();
    if let Some(mut pipe) = self.stderr.take() {
      let _ = pipe.read_to_string(&mut stderr);
    }
    let stderr = stderr.trim();
    if stderr.is_empty() {
      "overlay daemon exited before emitting an ack".to_string()
    } else {
      format!("overlay daemon exited before emitting an ack: {stderr}")
    }
  }

  fn wait_or_kill(&mut self, timeout: Duration) -> AuvResult<()> {
    let started_at = Instant::now();
    loop {
      match self.child.try_wait() {
        Ok(Some(_status)) => return Ok(()),
        Ok(None) if started_at.elapsed() < timeout => {
          thread::sleep(Duration::from_millis(25));
        }
        Ok(None) => {
          self
            .child
            .kill()
            .map_err(|error| format!("failed to kill overlay daemon after timeout: {error}"))?;
          let _ = self.child.wait();
          return Ok(());
        }
        Err(error) => {
          return Err(format!("failed to wait for overlay daemon: {error}"));
        }
      }
    }
  }
}

impl Drop for OverlayDaemon {
  fn drop(&mut self) {
    let _ = self.shutdown();
    let _ = fs::remove_file(&self.script_path);
  }
}
