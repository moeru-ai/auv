#![cfg(target_os = "macos")]

use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use auv_driver_common::{ClickModifiers, ClickOptions, Driver, InputPolicy, WindowClickStrategy, WindowInput, WindowPoint};
use auv_driver_macos::MacosDriver;

struct Receiver {
  child: Child,
  directory: PathBuf,
}

impl Drop for Receiver {
  fn drop(&mut self) {
    let _ = self.child.kill();
    let _ = self.child.wait();
    let _ = fs::remove_dir_all(&self.directory);
  }
}

/// Live evidence for one AppKit receiver. Requires a logged-in macOS GUI,
/// Accessibility permission and window-list access. Not cross-toolkit support.
#[test]
#[ignore = "opens a dedicated AppKit receiver and delivers real mouse events"]
fn appkit_receives_click_modifiers_on_background_and_foreground_routes() {
  let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
  let directory = std::env::temp_dir().join(format!("auv-click-modifiers-{}-{nonce}", std::process::id()));
  fs::create_dir(&directory).unwrap();
  let executable = directory.join("receiver");
  let compilation = Command::new("swiftc")
    .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/click_modifiers.swift"))
    .arg("-o")
    .arg(&executable)
    .output()
    .unwrap();
  assert!(compilation.status.success(), "{}", String::from_utf8_lossy(&compilation.stderr));
  let receiver = Receiver {
    child: Command::new(&executable).arg(&directory).spawn().unwrap(),
    directory,
  };
  let deadline = Instant::now() + Duration::from_secs(10);
  while !receiver.directory.join("ready").exists() {
    assert!(Instant::now() < deadline, "receiver startup timed out");
    std::thread::sleep(Duration::from_millis(50));
  }
  let session = MacosDriver::new().open_local().unwrap();
  let window = session
    .window()
    .list()
    .unwrap()
    .into_iter()
    .find(|window| window.process_id == Some(receiver.child.id()))
    .expect("receiver window must be visible to the driver");
  let event_path = receiver.directory.join("events.jsonl");
  let modifier_mask = (1_u64 << 17) | (1 << 18) | (1 << 19) | (1 << 20);
  for (policy, strategy) in [
    (InputPolicy::BackgroundOnly, WindowClickStrategy::PidTargeted),
    (InputPolicy::BackgroundOnly, WindowClickStrategy::ChromiumCompatible),
    (InputPolicy::ForegroundPreferred, WindowClickStrategy::PidTargeted),
  ] {
    for modifiers in [
      ClickModifiers {
        shift: true,
        control: true,
        alt: true,
        meta: true,
      },
      ClickModifiers::default(),
    ] {
      fs::write(&event_path, "").unwrap();
      let result = session
        .window()
        .click(
          &window,
          WindowPoint::new(100.0, 100.0),
          ClickOptions {
            policy,
            window_strategy: strategy,
            modifiers,
            ..Default::default()
          },
        )
        .unwrap();
      assert!(!result.verified);
      // Allow asynchronous AppKit dispatch to settle; the oracle is its log.
      let deadline = Instant::now() + Duration::from_secs(3);
      let events = loop {
        std::thread::sleep(Duration::from_millis(100));
        let events: Vec<serde_json::Value> = fs::read_to_string(&event_path)
          .unwrap()
          .split_inclusive('\n')
          .filter(|line| line.ends_with('\n'))
          .map(|line| serde_json::from_str(line).unwrap())
          .collect();
        if events.len() >= 2 {
          break events;
        }
        assert!(Instant::now() < deadline, "no click pair received: {policy:?}/{strategy:?}; {events:?}");
      };
      eprintln!("{policy:?}/{strategy:?} modifiers={modifiers:?}: {events:?}");
      let expected_flags = if modifiers.is_empty() {
        0
      } else {
        modifier_mask
      };
      assert!(events.iter().any(|event| matches!(event["type"].as_u64(), Some(1 | 3))), "missing down: {events:?}");
      assert!(events.iter().any(|event| matches!(event["type"].as_u64(), Some(2 | 4))), "missing up: {events:?}");
      for event in &events {
        assert_eq!(event["flags"].as_u64().unwrap() & modifier_mask, expected_flags, "{policy:?}/{strategy:?}: {event}");
        assert_eq!(event["window"].as_u64().unwrap().to_string(), window.reference.id);
      }
    }
  }
}
