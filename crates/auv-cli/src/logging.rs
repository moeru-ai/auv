//! Operational logs: per-process diagnostics, separate from run recording.
//!
//! Interactive commands log to stderr only when asked (`-v`). The daemon
//! (`auv serve`) and its Runners also write dated files under the user log
//! directory, which the daemon expires. These logs explain why a process
//! failed; they are not trace records and carry no format promise. See
//! `docs/ai/references/inspect/2026-10-21-trace-and-log-responsibility-split.md`.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::{EnvFilter, Layer as _};

/// Overrides the log directory, for tests and relocated installs.
pub(crate) const LOG_DIR_ENV: &str = "AUV_LOG_DIR";
/// Replaces the file filter, in `EnvFilter` syntax (default `off,auv=info`).
pub(crate) const LOG_FILTER_ENV: &str = "AUV_LOG";
/// Days a log file is kept after its last write.
pub(crate) const LOG_RETENTION_DAYS_ENV: &str = "AUV_LOG_RETENTION_DAYS";

/// NOTICE(log-retention): a week covers a bug report filed after a weekend
/// without letting a long-lived daemon fill the disk; there is no size cap.
const DEFAULT_RETENTION_DAYS: u64 = 7;
const EXPIRY_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

// Keep dependency-level HTTP/gRPC tracing disabled: it may include request
// metadata or bodies containing bootstrap and bearer credentials. Every AUV
// crate's target starts with `auv`, which `EnvFilter` matches by prefix.
const DEFAULT_FILE_FILTER: &str = "off,auv=info";

static LOG_DIRECTORY: OnceLock<PathBuf> = OnceLock::new();

/// Installs the process subscriber: stderr at `verbosity`, and, for a
/// long-lived process named by `file`, `<log dir>/<file>.<YYYY-MM-DD>.log`.
///
/// Call it once the process knows what it is: per-layer filters must be part
/// of the subscriber from the start (`tracing-subscriber` cannot reload a
/// filtered layer). A log file that cannot be opened is reported on stderr and
/// the process continues; it would only have had stderr before file logs.
pub(crate) fn init(verbosity: u8, file: Option<&str>) {
  let stderr = stderr_level(verbosity).map(|level| {
    tracing_subscriber::fmt::layer().with_writer(std::io::stderr).with_target(false).with_filter(EnvFilter::new(format!("off,auv={level}")))
  });
  let file = file.and_then(|name| match file_layer(name) {
    Ok((layer, directory)) => {
      let _ = LOG_DIRECTORY.set(directory);
      Some(layer)
    }
    Err(error) => {
      eprintln!("warning: logs are not written to a file: {error}");
      None
    }
  });
  let _ = tracing_subscriber::registry().with(file).with(stderr).try_init();
}

/// The directory this process writes logs to, if it writes any.
pub(crate) fn directory() -> Option<&'static Path> {
  LOG_DIRECTORY.get().map(PathBuf::as_path)
}

fn stderr_level(verbosity: u8) -> Option<&'static str> {
  match verbosity {
    0 => None,
    1 => Some("info"),
    2 => Some("debug"),
    _ => Some("trace"),
  }
}

fn file_layer<S>(name: &str) -> Result<(impl tracing_subscriber::Layer<S>, PathBuf), String>
where
  S: tracing::Subscriber + for<'span> tracing_subscriber::registry::LookupSpan<'span>,
{
  let directory = log_directory()?;
  create_private_directory(&directory)?;
  let appender = tracing_appender::rolling::Builder::new()
    .rotation(tracing_appender::rolling::Rotation::DAILY)
    .filename_prefix(name)
    .filename_suffix("log")
    .build(&directory)
    .map_err(|error| format!("failed to open log file in {}: {error}", directory.display()))?;
  let filter = std::env::var(LOG_FILTER_ENV).unwrap_or_else(|_| DEFAULT_FILE_FILTER.to_string());
  let filter = EnvFilter::try_new(&filter).map_err(|error| format!("invalid {LOG_FILTER_ENV} filter {filter:?}: {error}"))?;
  // Writes are synchronous: logs are low-volume, and a background writer
  // would lose the last lines when the process exits through `exit`.
  let layer = tracing_subscriber::fmt::layer().with_writer(appender).with_ansi(false).with_filter(filter);
  Ok((layer, directory))
}

/// Expires old log files now and then every few hours, for the daemon's life.
pub(crate) fn spawn_expiry(directory: PathBuf) {
  let retention = retention();
  tokio::spawn(async move {
    let mut interval = tokio::time::interval(EXPIRY_INTERVAL);
    loop {
      interval.tick().await;
      let directory = directory.clone();
      let removed = tokio::task::spawn_blocking(move || remove_expired(&directory, retention, SystemTime::now())).await;
      match removed {
        Ok(Ok(0)) => {}
        Ok(Ok(count)) => tracing::info!(count, "expired old log files"),
        Ok(Err(error)) => tracing::warn!(%error, "log expiry failed"),
        Err(error) => tracing::warn!(%error, "log expiry task failed"),
      }
    }
  });
}

fn retention() -> Duration {
  let days = std::env::var(LOG_RETENTION_DAYS_ENV)
    .ok()
    .and_then(|value| value.parse::<u64>().ok())
    .filter(|days| *days > 0)
    .unwrap_or(DEFAULT_RETENTION_DAYS);
  Duration::from_secs(days.saturating_mul(24 * 60 * 60))
}

/// Deletes `*.log` files in `directory` last modified before `now - retention`
/// and returns how many it removed. Other files are never touched.
fn remove_expired(directory: &Path, retention: Duration, now: SystemTime) -> std::io::Result<usize> {
  let Some(cutoff) = now.checked_sub(retention) else {
    return Ok(0);
  };
  let mut removed = 0;
  for entry in std::fs::read_dir(directory)? {
    let entry = entry?;
    let path = entry.path();
    let metadata = entry.metadata()?;
    if !metadata.is_file() || path.extension().is_none_or(|extension| extension != "log") {
      continue;
    }
    if metadata.modified()? < cutoff {
      std::fs::remove_file(&path)?;
      removed += 1;
    }
  }
  Ok(removed)
}

fn log_directory() -> Result<PathBuf, String> {
  if let Some(directory) = std::env::var_os(LOG_DIR_ENV) {
    return Ok(PathBuf::from(directory));
  }
  // The same per-user root as discovery and the Linux portal state
  // (`auv::discovery::default_path`); XDG puts logs under the state directory.
  let directories = directories::ProjectDirs::from("ai", "moeru", "auv").ok_or("cannot resolve the AUV user state directory for logs")?;
  Ok(directories.state_dir().unwrap_or_else(|| directories.data_local_dir()).join("logs"))
}

fn create_private_directory(directory: &Path) -> Result<(), String> {
  std::fs::create_dir_all(directory).map_err(|error| format!("failed to create log directory {}: {error}", directory.display()))?;
  // Logs name paths, apps, and Device ids; keep them to the owning user.
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
      .map_err(|error| format!("failed to restrict log directory {}: {error}", directory.display()))?;
  }
  Ok(())
}

#[cfg(test)]
#[path = "logging_test.rs"]
mod tests;
