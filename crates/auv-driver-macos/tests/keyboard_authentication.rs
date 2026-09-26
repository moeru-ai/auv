#![cfg(target_os = "macos")]

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

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
