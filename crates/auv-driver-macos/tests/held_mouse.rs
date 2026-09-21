//! Explicit native receipt probe; never runs as part of an unattended unit suite.
#![cfg(target_os = "macos")]
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use auv_driver_common::{Driver, InputTarget, MouseButton, Point};
use auv_driver_macos::MacosDriver;

struct Receiver(Child);
impl Drop for Receiver {
  fn drop(&mut self) {
    let _ = self.0.kill();
    let _ = self.0.wait();
  }
}

#[test]
#[ignore = "opens a bounded AppKit receiver and sends window-targeted input; requires Accessibility"]
fn background_down_drag_up_reaches_the_same_appkit_receiver() {
  let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/held_mouse_receiver.swift");
  let mut receiver = Receiver(Command::new("swift").arg(script).stdout(Stdio::piped()).spawn().expect("start AppKit receiver"));
  let stdout = receiver.0.stdout.take().unwrap();
  let (send, events) = mpsc::channel();
  std::thread::spawn(move || {
    for line in BufReader::new(stdout).lines() {
      if send.send(line.unwrap()).is_err() {
        break;
      }
    }
  });
  assert_eq!(events.recv_timeout(Duration::from_secs(10)).unwrap(), "ready");
  let session = MacosDriver::default().open_local().unwrap();
  let window = session
    .window()
    .list()
    .unwrap()
    .into_iter()
    .find(|window| window.process_id == Some(receiver.0.id()) && window.title.as_deref() == Some("AUV held mouse receiver"))
    .expect("observed receiver window");
  let point = Point::new(window.frame.origin.x + 100., window.frame.origin.y + 100.);
  let mouse = session.input().create_mouse().unwrap();
  let target = InputTarget::Window(window);
  let result = (|| {
    session.input().mouse_down(&target, mouse, point, MouseButton::Left, Duration::from_secs(3))?;
    session.input().move_mouse_to(mouse, Point::new(point.x + 40., point.y + 20.))?;
    session.input().mouse_up(mouse)?;
    Ok::<_, auv_driver_common::DriverError>(())
  })();
  let cleanup = session.input().remove_mouse(mouse);
  result.unwrap();
  cleanup.unwrap();
  assert_eq!(events.recv_timeout(Duration::from_secs(3)).unwrap(), "down");
  assert_eq!(events.recv_timeout(Duration::from_secs(3)).unwrap(), "drag");
  assert_eq!(events.recv_timeout(Duration::from_secs(3)).unwrap(), "up");
}
