#![cfg(target_os = "macos")]

use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use auv_driver_common::{Driver, InputPolicy, InputTarget, PressKeysOptions, TypeTextOptions};
use auv_driver_macos::MacosDriver;

struct Scratch(PathBuf);

impl Drop for Scratch {
  fn drop(&mut self) {
    let _ = fs::remove_dir_all(&self.0);
  }
}

#[test]
fn native_keyboard_authentication_prepares_before_one_submission() {
  let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
  let directory = Scratch(std::env::temp_dir().join(format!("auv-keyboard-auth-{}-{nonce}", std::process::id())));
  fs::create_dir(&directory.0).unwrap();
  let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  let executable = directory.0.join("contract-tests");
  let compilation = Command::new("swiftc")
    .arg(root.join("native/swift/Sources/AuvMacosNative/EventPosting.swift"))
    .arg(root.join("tests/fixtures/keyboard_authentication.swift"))
    .arg("-o")
    .arg(&executable)
    .output()
    .unwrap();
  assert!(compilation.status.success(), "{}", String::from_utf8_lossy(&compilation.stderr));
  let result = Command::new(executable).output().unwrap();
  assert!(
    result.status.success(),
    "stdout: {}\nstderr: {}",
    String::from_utf8_lossy(&result.stdout),
    String::from_utf8_lossy(&result.stderr)
  );
  eprintln!("{}", String::from_utf8_lossy(&result.stdout));
}

struct Receiver(Child);

impl Drop for Receiver {
  fn drop(&mut self) {
    let _ = self.0.kill();
    let _ = self.0.wait();
  }
}

/// A real NSTextView and its event log are the oracle, not the driver's result.
#[test]
#[ignore = "opens a dedicated AppKit receiver and sends background keyboard input; requires Accessibility"]
fn appkit_receives_background_text_and_modifier_keys_once() {
  let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
  let directory = Scratch(std::env::temp_dir().join(format!("auv-keyboard-receiver-{}-{nonce}", std::process::id())));
  fs::create_dir(&directory.0).unwrap();
  let executable = directory.0.join("receiver");
  let compilation = Command::new("swiftc")
    .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/keyboard_receiver.swift"))
    .arg("-o")
    .arg(&executable)
    .output()
    .unwrap();
  assert!(compilation.status.success(), "{}", String::from_utf8_lossy(&compilation.stderr));
  let receiver = Receiver(Command::new(executable).arg(&directory.0).spawn().unwrap());
  let deadline = Instant::now() + Duration::from_secs(10);
  while !directory.0.join("ready").exists() {
    assert!(Instant::now() < deadline, "receiver startup timed out");
    std::thread::sleep(Duration::from_millis(50));
  }
  let session = MacosDriver::new().open_local().unwrap();
  let window = session
    .window()
    .list()
    .unwrap()
    .into_iter()
    .find(|window| window.process_id == Some(receiver.0.id()))
    .expect("receiver window must be observable");
  let result = session
    .window()
    .type_text(
      &window,
      "A猫",
      TypeTextOptions {
        policy: InputPolicy::BackgroundOnly,
        ..Default::default()
      },
    )
    .unwrap();
  assert!(!result.verified);
  session
    .input()
    .press_keys(
      &InputTarget::Window(window.clone()),
      PressKeysOptions {
        keys: vec!["shift".into(), "b".into()],
        ..Default::default()
      },
      InputPolicy::BackgroundOnly,
      false,
    )
    .unwrap();

  let deadline = Instant::now() + Duration::from_secs(5);
  while fs::read_to_string(directory.0.join("text")).unwrap_or_default() != "A猫B" {
    assert!(
      Instant::now() < deadline,
      "receiver text: {:?}; events: {}",
      fs::read_to_string(directory.0.join("text")),
      fs::read_to_string(directory.0.join("events.jsonl")).unwrap()
    );
    std::thread::sleep(Duration::from_millis(50));
  }
  // Observe the complete up events and any duplicate delivery after text changes.
  std::thread::sleep(Duration::from_millis(300));
  assert_eq!(fs::read_to_string(directory.0.join("text")).unwrap(), "A猫B");
  let events: Vec<serde_json::Value> =
    fs::read_to_string(directory.0.join("events.jsonl")).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
  eprintln!("AppKit keyboard receipt: {events:?}");
  let downs: Vec<_> = events.iter().filter(|event| event["type"] == 10).collect();
  let ups: Vec<_> = events.iter().filter(|event| event["type"] == 11).collect();
  assert_eq!(downs.len(), 3, "one down per character/key: {events:?}");
  assert_eq!(ups.len(), 3, "one up per character/key: {events:?}");
  assert_eq!(downs.iter().map(|event| event["characters"].as_str().unwrap()).collect::<Vec<_>>(), ["A", "猫", "B"]);
  assert_eq!(downs[2]["flags"].as_u64().unwrap() & (1 << 17), 1 << 17);
  for event in &events {
    assert_eq!(event["window"].as_u64().unwrap().to_string(), window.reference.id);
    assert_eq!(event["active"], false, "receiver became foreground: {events:?}");
  }
}
