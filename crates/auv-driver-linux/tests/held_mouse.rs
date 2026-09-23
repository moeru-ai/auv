//! Independent live GTK receipt; opt-in because this moves the desktop pointer.
#![cfg(target_os = "linux")]

use std::{
  io::{BufRead, BufReader, Write},
  process::{Child, Command, Stdio},
  sync::{Arc, mpsc},
  time::{Duration, Instant},
};

use auv_driver_common::{
  Driver, InputTarget, MouseButton, MouseCubicBezierSegment, MouseMotionOptions, MoveMouseRequest, Point,
  mouse_input::{InputCancellation, with_input_cancellation},
};
use auv_driver_linux::{InputBackend, LinuxDriver};

struct Receiver {
  child: Child,
  events: mpsc::Receiver<String>,
}

impl Receiver {
  fn read_until(&mut self, marker: &str) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut lines = Vec::new();
    loop {
      let line = self.events.recv_timeout(deadline.saturating_duration_since(Instant::now())).expect("GTK receipt deadline");
      if line.starts_with(marker) {
        if marker == "status" {
          assert_eq!(line, "status 1", "receiver lost foreground; stop injection");
        }
        return lines;
      }
      lines.push(line);
    }
  }

  fn checkpoint(&mut self) -> Vec<String> {
    writeln!(self.child.stdin.as_mut().unwrap(), "status").unwrap();
    self.read_until("status")
  }
}

impl Drop for Receiver {
  fn drop(&mut self) {
    let _ = self.child.kill();
    let _ = self.child.wait();
  }
}

#[test]
#[ignore = "requires unlocked GNOME/GTK4 desktop and explicit AUV_HELD_BACKEND=uinput or portal; moves pointer"]
fn gtk_receives_held_buttons_and_cleanup() {
  let backend = match std::env::var("AUV_HELD_BACKEND").as_deref() {
    Ok("uinput") => InputBackend::Uinput,
    Ok("portal") => InputBackend::Portal,
    _ => panic!("select AUV_HELD_BACKEND=uinput or portal explicitly"),
  };
  let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/held_mouse_receiver.py");
  let mut child = Command::new("python3").arg(script).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
  let stdout = child.stdout.take().unwrap();
  let (send, events) = mpsc::channel();
  let readiness = Arc::new(InputCancellation::default());
  let reader_readiness = readiness.clone();
  std::thread::spawn(move || {
    for line in BufReader::new(stdout).lines() {
      let line = line.unwrap();
      if line == "inactive" {
        reader_readiness.cancel();
      }
      if send.send(line).is_err() {
        break;
      }
    }
    reader_readiness.cancel();
  });
  let mut receiver = Receiver { child, events };
  receiver.read_until("ready");
  std::thread::sleep(Duration::from_millis(600));
  receiver.checkpoint();
  let session = LinuxDriver::new().with_input_backend(backend).open_local().unwrap();
  let input = session.input();
  let mouse = input.create_mouse().unwrap();
  let start = Point::new(300.0, 300.0);
  let end = Point::new(380.0, 340.0);
  let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    with_input_cancellation(readiness, || {
      for (button, gtk_button, held_mask) in [
        (MouseButton::Left, 1, 1 << 8),
        (MouseButton::Right, 3, 1 << 10),
        (MouseButton::Middle, 2, 1 << 9),
      ] {
        for case in ["cross-call", "hold", "watchdog", "cancel", "drag"] {
          receiver.checkpoint();
          let cancel = Arc::new(InputCancellation::default());
          match case {
            "cross-call" => {
              input.mouse_down(&InputTarget::Foreground, mouse, start, button, Duration::from_secs(3)).unwrap();
              std::thread::sleep(Duration::from_millis(80));
              // Check foreground before moving an already-held pointer; cleanup
              // still runs if the receiver has lost readiness.
              let first = receiver.checkpoint();
              assert!(first.iter().any(|line| line.starts_with(&format!("down {gtk_button} "))), "{first:?}");
              input.move_mouse_to(mouse, end).unwrap();
              std::thread::sleep(Duration::from_millis(80));
              input.mouse_up(mouse).unwrap();
              std::thread::sleep(Duration::from_millis(100));
              let mut receipts = first;
              receipts.extend(receiver.checkpoint());
              verify(&receipts, gtk_button, held_mask, true, end);
              println!("{backend:?} {button:?} {case}: {receipts:?}");
              continue;
            }
            "hold" => {
              input.hold_mouse(&InputTarget::Foreground, mouse, start, button, Duration::from_millis(150)).unwrap();
            }
            "watchdog" => {
              input.mouse_down(&InputTarget::Foreground, mouse, start, button, Duration::from_millis(180)).unwrap();
              std::thread::sleep(Duration::from_millis(450));
            }
            "cancel" => {
              with_input_cancellation(cancel.clone(), || {
                input.mouse_down(&InputTarget::Foreground, mouse, start, button, Duration::from_secs(5))
              })
              .unwrap();
              std::thread::sleep(Duration::from_millis(100));
              cancel.cancel();
              std::thread::sleep(Duration::from_millis(200));
            }
            "drag" => {
              let mut request = MoveMouseRequest::direct(start);
              request.mouse = mouse;
              request.curve.segments.push(MouseCubicBezierSegment {
                control_1: Point::new(20.0, 10.0),
                control_2: Point::new(60.0, 30.0),
                end: Point::new(80.0, 40.0),
              });
              request.options = MouseMotionOptions {
                duration: Duration::from_millis(250),
                sample_rate_hz: 30,
                curve_tolerance: 0.1,
              };
              input.drag_mouse(request, button).unwrap();
            }
            _ => unreachable!(),
          }
          std::thread::sleep(Duration::from_millis(100));
          let receipts = receiver.checkpoint();
          verify(&receipts, gtk_button, held_mask, case == "drag", if case == "drag" { end } else { start });
          println!("{backend:?} {button:?} {case}: {receipts:?}");
          // Idempotent up must not create another native release after cleanup.
          input.mouse_up(mouse).unwrap();
          std::thread::sleep(Duration::from_millis(40));
          assert!(receiver.checkpoint().iter().all(|line| !line.starts_with("up ")));
        }
      }
    })
  }));
  input.remove_mouse(mouse).expect("release test-owned input before receiver closes");
  if let Err(panic) = result {
    std::panic::resume_unwind(panic);
  }
}

fn verify(receipts: &[String], button: u32, held_mask: u32, moves: bool, end: Point) {
  let transitions: Vec<_> = receipts.iter().filter(|line| line.starts_with("down ") || line.starts_with("up ")).collect();
  assert_eq!(transitions.len(), 2, "exactly one down/up: {receipts:?}");
  assert!(transitions[0].starts_with(&format!("down {button} ")), "{receipts:?}");
  assert!(transitions[1].starts_with(&format!("up {button} ")), "{receipts:?}");
  let up: Vec<_> = transitions[1].split_whitespace().collect();
  assert!((up[2].parse::<f64>().unwrap() - end.x).abs() < 3.0, "{receipts:?}");
  assert!((up[3].parse::<f64>().unwrap() - end.y).abs() < 3.0, "{receipts:?}");
  if moves {
    assert!(
      receipts.iter().any(|line| {
        let parts: Vec<_> = line.split_whitespace().collect();
        parts[0] == "move" && parts[1].parse::<u32>().unwrap() & held_mask != 0
      }),
      "held motion missing: {receipts:?}"
    );
  }
}
