#![cfg(target_os = "macos")]

use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use auv_base_evals::window_management;
use auv_driver_common::Driver;
use auv_driver_macos::MacosDriver;

struct Scratch(PathBuf);

impl Drop for Scratch {
  fn drop(&mut self) {
    let _ = fs::remove_dir_all(&self.0);
  }
}

struct Receiver(Child);

impl Drop for Receiver {
  fn drop(&mut self) {
    let _ = self.0.kill();
    let _ = self.0.wait();
  }
}

/// `set_frame`, minimize, maximize and focus on a throwaway AppKit window.
#[test]
#[ignore = "opens a dedicated AppKit window, moves it and takes the foreground; requires Accessibility"]
fn appkit_window_frame_state_and_focus() {
  let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
  let directory = Scratch(std::env::temp_dir().join(format!("auv-window-receiver-{}-{nonce}", std::process::id())));
  fs::create_dir(&directory.0).unwrap();

  let executable = directory.0.join("receiver");
  let compilation = Command::new("swiftc")
    .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("platforms/desktop-macos/test-objects/appkit/window.swift"))
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

  window_management::run(&session.window(), &window.reference.id).unwrap();
}
