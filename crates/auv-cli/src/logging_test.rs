use std::time::{Duration, SystemTime};

use super::remove_expired;

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

fn touch(path: &std::path::Path, modified: SystemTime) {
  let file = std::fs::File::create(path).unwrap();
  file.set_modified(modified).unwrap();
}

#[test]
fn expiry_removes_only_old_log_files() {
  let directory = tempfile::tempdir().unwrap();
  let now = SystemTime::now();
  touch(&directory.path().join("daemon.2026-10-01.log"), now - 10 * DAY);
  touch(&directory.path().join("runner-abc.2026-10-20.log"), now - DAY);
  touch(&directory.path().join("notes.txt"), now - 30 * DAY);
  std::fs::create_dir(directory.path().join("old.log")).unwrap();

  assert_eq!(remove_expired(directory.path(), 7 * DAY, now).unwrap(), 1);
  let mut left = std::fs::read_dir(directory.path()).unwrap().map(|entry| entry.unwrap().file_name()).collect::<Vec<_>>();
  left.sort();
  assert_eq!(left, ["notes.txt", "old.log", "runner-abc.2026-10-20.log"]);
}

#[test]
fn the_default_filter_admits_every_auv_crate_and_no_dependency() {
  // ROOT CAUSE:
  //
  // If the filter named only `auv_cli`, daemon, API server, and driver logs
  // were dropped, and the daemon wrote almost nothing.
  //
  // The fix admits every `auv*` target and keeps dependencies off, because
  // HTTP/gRPC dependency logs can carry bearer credentials.
  use tracing_subscriber::layer::SubscriberExt as _;
  let output = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
  let writer = {
    let output = output.clone();
    move || Writer(output.clone())
  };
  let subscriber = tracing_subscriber::registry().with(tracing_subscriber::Layer::with_filter(
    tracing_subscriber::fmt::layer().with_writer(writer).with_ansi(false),
    tracing_subscriber::EnvFilter::new(super::DEFAULT_FILE_FILTER),
  ));
  tracing::subscriber::with_default(subscriber, || {
    tracing::info!(target: "auv_daemon::daemon::runner", "daemon line");
    tracing::info!(target: "auv_api_server::rest", "server line");
    tracing::info!(target: "h2::proto", "dependency line");
    tracing::debug!(target: "auv_daemon", "debug line");
  });
  let text = String::from_utf8(output.lock().unwrap().clone()).unwrap();
  assert!(text.contains("daemon line") && text.contains("server line"), "{text}");
  assert!(!text.contains("dependency line") && !text.contains("debug line"), "{text}");
}

struct Writer(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Writer {
  fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
    self.0.lock().unwrap().extend_from_slice(bytes);
    Ok(bytes.len())
  }
  fn flush(&mut self) -> std::io::Result<()> {
    Ok(())
  }
}
