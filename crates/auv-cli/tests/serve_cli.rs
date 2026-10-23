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

// ROOT CAUSE:
//
// Local IPC admitted every process of the daemon's OS user, so any process
// that read the private socket path from `ps` could drive the daemon. With
// --local-auth-allowed-methods launcher-token, local callers must present the
// launching process's token, which never appears in argv or the environment.
#[test]
fn serve_launcher_only_rejects_local_callers_without_the_token() {
  use std::io::Write as _;

  let directory = tempfile::tempdir().unwrap();
  let auv = env!("CARGO_BIN_EXE_auv");
  let mut daemon = Command::new(auv)
    .args([
      "serve",
      "--no-register",
      "--exit-on-stdin-close",
      "--local-auth-allowed-methods",
      "launcher-token",
      "--store-root",
    ])
    .arg(directory.path().join("store"))
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::inherit())
    .spawn()
    .unwrap();
  let mut stdin = daemon.stdin.take().unwrap();
  writeln!(stdin, "{}", "a".repeat(64)).unwrap();
  let mut ready = String::new();
  std::io::BufReader::new(daemon.stdout.take().unwrap()).read_line(&mut ready).unwrap();
  let endpoint = ready.trim().strip_prefix("auv serve: ").expect("private daemon endpoint").to_string();

  let stranger = Command::new(auv).args(["devices", "list", "--json"]).env("AUV_ENDPOINT", &endpoint).output().unwrap();

  drop(stdin);
  let _ = daemon.wait();
  assert!(!stranger.status.success(), "{}", String::from_utf8_lossy(&stranger.stdout));
  assert!(String::from_utf8_lossy(&stranger.stderr).contains("process that launched it"), "{}", String::from_utf8_lossy(&stranger.stderr));
}

#[test]
fn serve_refuses_to_start_with_a_malformed_launcher_token() {
  use std::io::Write as _;

  let directory = tempfile::tempdir().unwrap();
  let mut daemon = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "serve",
      "--no-register",
      "--exit-on-stdin-close",
      "--local-auth-allowed-methods",
      "launcher-token",
      "--store-root",
    ])
    .arg(directory.path().join("store"))
    .stdin(Stdio::piped())
    .stdout(Stdio::null())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap();
  writeln!(daemon.stdin.take().unwrap(), "not-a-key").unwrap();
  let output = daemon.wait_with_output().unwrap();
  assert!(!output.status.success());
  assert!(String::from_utf8_lossy(&output.stderr).contains("launcher token must be"), "{}", String::from_utf8_lossy(&output.stderr));
}

// The CLI could not present a credential on local IPC, so `auv` commands could
// not reach an app-started daemon even when that app handed them its token.
#[test]
fn cli_reaches_a_launcher_only_daemon_with_credential_stdin() {
  use std::io::Write as _;

  let directory = tempfile::tempdir().unwrap();
  let auv = env!("CARGO_BIN_EXE_auv");
  let token = "b".repeat(64);
  let mut daemon = Command::new(auv)
    .args([
      "serve",
      "--no-register",
      "--exit-on-stdin-close",
      "--local-auth-allowed-methods",
      "launcher-token",
      "--store-root",
    ])
    .arg(directory.path().join("store"))
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::inherit())
    .spawn()
    .unwrap();
  let mut daemon_stdin = daemon.stdin.take().unwrap();
  writeln!(daemon_stdin, "{token}").unwrap();
  let mut ready = String::new();
  std::io::BufReader::new(daemon.stdout.take().unwrap()).read_line(&mut ready).unwrap();
  let endpoint = ready.trim().strip_prefix("auv serve: ").expect("private daemon endpoint").to_string();

  let list = |credential: &str| {
    let mut cli = Command::new(auv)
      .args(["--credential-stdin", "devices", "list", "--json"])
      .env("AUV_ENDPOINT", &endpoint)
      .stdin(Stdio::piped())
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .spawn()
      .unwrap();
    writeln!(cli.stdin.take().unwrap(), "{credential}").unwrap();
    cli.wait_with_output().unwrap()
  };
  let accepted = list(&token);
  let rejected = list(&"c".repeat(64));

  drop(daemon_stdin);
  let _ = daemon.wait();
  assert!(accepted.status.success(), "{}", String::from_utf8_lossy(&accepted.stderr));
  assert!(String::from_utf8_lossy(&accepted.stdout).contains("device_id"));
  assert!(!rejected.status.success());
  assert!(String::from_utf8_lossy(&rejected.stderr).contains("process that launched it"), "{}", String::from_utf8_lossy(&rejected.stderr));
}

// ROOT CAUSE:
//
// `invoke` with a Device selector resolved the Device and its implicit Run
// through `Client::from_context`, which never saw the `--credential-stdin`
// bearer, so a launcher-only daemon rejected the call even with the right
// token. The selected context now carries the credential to every connection
// of the invoke: Device resolution, the Runner call, and stopping the Run.
#[test]
fn cli_invokes_a_selected_device_with_credential_stdin() {
  use std::io::Write as _;

  let directory = tempfile::tempdir().unwrap();
  let auv = env!("CARGO_BIN_EXE_auv");
  let token = "d".repeat(64);
  let mut daemon = Command::new(auv)
    .args([
      "serve",
      "--no-register",
      "--exit-on-stdin-close",
      "--local-auth-allowed-methods",
      "launcher-token",
      "--store-root",
    ])
    .arg(directory.path().join("store"))
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::inherit())
    .spawn()
    .unwrap();
  let mut daemon_stdin = daemon.stdin.take().unwrap();
  writeln!(daemon_stdin, "{token}").unwrap();
  let mut ready = String::new();
  std::io::BufReader::new(daemon.stdout.take().unwrap()).read_line(&mut ready).unwrap();
  let endpoint = ready.trim().strip_prefix("auv serve: ").expect("private daemon endpoint").to_string();

  let cli = |credential: &str, args: &[&str]| {
    let mut cli = Command::new(auv)
      .arg("--credential-stdin")
      .args(args)
      .env("AUV_ENDPOINT", &endpoint)
      .stdin(Stdio::piped())
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .spawn()
      .unwrap();
    writeln!(cli.stdin.take().unwrap(), "{credential}").unwrap();
    cli.wait_with_output().unwrap()
  };
  let devices = cli(&token, &["devices", "list", "--json"]);
  let devices: serde_json::Value = serde_json::from_slice(&devices.stdout).expect("Device list JSON");
  let device_id = devices
    .as_array()
    .and_then(|devices| devices.iter().find(|device| device["local"] == true))
    .and_then(|device| device["device_id"].as_str())
    .expect("local Device ID")
    .to_string();
  let invoke = [
    "--device-id",
    &device_id,
    "invoke",
    "display.list",
    "--json",
  ];
  let accepted = cli(&token, &invoke);
  let rejected = cli(&"e".repeat(64), &invoke);

  drop(daemon_stdin);
  let _ = daemon.wait();
  let accepted_stderr = String::from_utf8_lossy(&accepted.stderr);
  assert!(!accepted_stderr.contains("process that launched it"), "{accepted_stderr}");
  // Linux CI has no display session, so only check the listed displays where
  // the runners have one.
  if cfg!(any(target_os = "macos", target_os = "windows")) {
    assert!(accepted.status.success(), "{accepted_stderr}");
    let accepted: serde_json::Value = serde_json::from_slice(&accepted.stdout).expect("invoke result JSON");
    assert_eq!(accepted["status"], "completed", "{accepted}");
  }
  assert!(!rejected.status.success());
  assert!(String::from_utf8_lossy(&rejected.stderr).contains("process that launched it"), "{}", String::from_utf8_lossy(&rejected.stderr));
}
