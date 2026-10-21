use std::io::BufRead as _;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

// ROOT CAUSE:
//
// An app that owns `auv serve` stopped it only from its own quit path. When
// the app was killed or its dev server restarted it, the daemon was
// reparented and kept serving desktop control for days.
//
// Before the fix, the daemon had no way to observe its owner. With
// --exit-on-stdin-close it treats the closed stdin pipe as a termination
// request and runs its normal shutdown path.
#[test]
fn serve_exits_cleanly_when_its_owner_closes_stdin() {
  let directory = tempfile::tempdir().unwrap();
  let mut daemon = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "serve",
      "--no-register",
      "--exit-on-stdin-close",
      "--store-root",
    ])
    .arg(directory.path().join("store"))
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::inherit())
    .spawn()
    .unwrap();
  let mut ready = String::new();
  std::io::BufReader::new(daemon.stdout.take().unwrap()).read_line(&mut ready).unwrap();
  assert!(ready.starts_with("auv serve: "), "{ready}");

  drop(daemon.stdin.take());

  let deadline = Instant::now() + Duration::from_secs(15);
  let status = loop {
    if let Some(status) = daemon.try_wait().unwrap() {
      break status;
    }
    if Instant::now() > deadline {
      let _ = daemon.kill();
      panic!("daemon kept running after stdin closed");
    }
    std::thread::sleep(Duration::from_millis(25));
  };
  assert!(status.success(), "{status}");
}
