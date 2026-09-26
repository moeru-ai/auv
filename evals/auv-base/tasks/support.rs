//! Process, file, and receiver boundaries shared by evaluation tasks.

use std::path::{Path, PathBuf};
use std::process::{ExitCode, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::{Instant, sleep, timeout};

pub static CANCELLED: AtomicBool = AtomicBool::new(false);

/// Install cooperative cancellation before polling a task, so its cleanup can
/// finish after Ctrl+C. Keep verdict-to-exit-status mapping common to both tasks.
pub async fn run(task: impl std::future::Future<Output = Result<bool>>) -> Result<ExitCode> {
  let interrupt = tokio::spawn(async {
    if tokio::signal::ctrl_c().await.is_ok() {
      CANCELLED.store(true, Ordering::Relaxed);
    }
  });

  let result = task.await;
  interrupt.abort();

  Ok(if result? {
    ExitCode::SUCCESS
  } else {
    ExitCode::FAILURE
  })
}

pub fn root() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn objects() -> PathBuf {
  root().join("platforms/desktop-universal/test-objects")
}

pub fn repository() -> PathBuf {
  root().parent().unwrap().parent().unwrap().to_path_buf()
}

pub fn binary(name: &str) -> PathBuf {
  repository().join("target/debug").join(name)
}

pub fn now_ms() -> u64 {
  SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64
}

pub fn path(value: &Path) -> String {
  value.to_string_lossy().into_owned()
}

pub async fn pause(ms: u64) -> Result<()> {
  let end = Instant::now() + Duration::from_millis(ms);

  loop {
    ensure!(!CANCELLED.load(Ordering::Relaxed), "evaluation interrupted");
    if Instant::now() >= end {
      return Ok(());
    }

    sleep(end.saturating_duration_since(Instant::now()).min(Duration::from_millis(50))).await;
  }
}

pub fn read_json(file: &Path) -> Result<Value> {
  if !file.exists() {
    return Ok(json!({}));
  }

  serde_json::from_slice(&std::fs::read(file)?).with_context(|| format!("read {}", file.display()))
}

pub fn write_json(file: &Path, value: &impl serde::Serialize) -> Result<()> {
  let temporary = file.with_extension("tmp");
  let mut bytes = serde_json::to_vec_pretty(value)?;
  bytes.push(b'\n');

  std::fs::write(&temporary, bytes)?;
  std::fs::rename(temporary, file)?;

  Ok(())
}

pub fn new_output(directory: &Path) -> Result<PathBuf> {
  ensure!(!directory.exists(), "output already exists: {} (choose a fresh directory)", directory.display());
  std::fs::create_dir_all(directory)?;

  Ok(directory.canonicalize()?)
}

pub async fn output(program: &Path, args: &[String]) -> Result<String> {
  let result = timeout(Duration::from_secs(30), Command::new(program).args(args).kill_on_drop(true).output())
    .await
    .with_context(|| format!("{} timed out", program.display()))??;

  ensure!(
    result.status.success(),
    "{} failed: {}{}",
    program.display(),
    String::from_utf8_lossy(&result.stderr),
    String::from_utf8_lossy(&result.stdout)
  );

  Ok(String::from_utf8(result.stdout)?.trim().to_owned())
}

pub fn launch(program: &Path, args: &[String], log: &Path) -> Result<Child> {
  let log = std::fs::File::create(log)?;

  Command::new(program)
    .args(args)
    .stdin(Stdio::null())
    .stdout(log.try_clone()?)
    .stderr(log)
    .kill_on_drop(true)
    .spawn()
    .with_context(|| format!("launch {}", program.display()))
}

pub async fn stop(child: &mut Child) {
  if child.try_wait().ok().flatten().is_none() {
    let _ = child.kill().await;
  }

  let _ = child.wait().await;
}

/// JSON-lines child owns its stdin/stdout and is killed if an error drops it.
/// Normal shutdown closes stdin first so the Rust sender can release held keys.
pub struct Process {
  pub child: Child,
  input: Option<ChildStdin>,
  reader: BufReader<ChildStdout>,
}

impl Process {
  pub fn spawn(program: &Path, args: &[String], log: &Path) -> Result<Self> {
    let mut child = Command::new(program)
      .args(args)
      .stdin(Stdio::piped())
      .stdout(Stdio::piped())
      .stderr(std::fs::File::create(log)?)
      .kill_on_drop(true)
      .spawn()?;

    let input = child.stdin.take();
    let reader = BufReader::new(child.stdout.take().context("child stdout unavailable")?);

    Ok(Self {
      child,
      input,
      reader,
    })
  }

  pub async fn request(&mut self, value: Value) -> Result<Value> {
    let mut bytes = serde_json::to_vec(&value)?;
    bytes.push(b'\n');

    let input = self.input.as_mut().context("child stdin closed")?;
    input.write_all(&bytes).await?;
    input.flush().await?;

    let mut line = String::new();
    let count = timeout(Duration::from_secs(15), self.reader.read_line(&mut line)).await.context("child reply timed out")??;
    ensure!(count != 0, "child exited without a reply");

    let response: Value = serde_json::from_str(&line)?;
    if let Some(error) = response.get("error") {
      bail!("child: {error}");
    }

    Ok(response)
  }

  pub async fn close(&mut self) {
    self.input.take();

    if timeout(Duration::from_secs(5), self.child.wait()).await.is_err() {
      stop(&mut self.child).await;
    }
  }
}

pub async fn invoke(executable: &Path, directory: &Path, args: &[String]) -> Result<Value> {
  let mut command = vec!["invoke".into()];
  command.extend_from_slice(args);
  command.extend([
    "--json".into(),
    "--no-overlay".into(),
    "--store-root".into(),
    path(&directory.join("runs")),
  ]);

  Ok(serde_json::from_str(&output(executable, &command).await?)?)
}

pub fn find_window(value: &Value, title: &str) -> Option<Value> {
  match value {
    Value::Object(object) => {
      if object.get("title").is_some_and(|v| v == title) && object.contains_key("reference") {
        return Some(value.clone());
      }

      object.values().find_map(|v| find_window(v, title))
    }
    Value::Array(values) => values.iter().find_map(|v| find_window(v, title)),
    _ => None,
  }
}
