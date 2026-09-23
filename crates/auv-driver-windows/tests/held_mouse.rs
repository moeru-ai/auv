//! Opt-in native receipts for foreground and posted-message held input.
#![cfg(target_os = "windows")]

use auv_driver_common::{
  CoordinateSpace, Driver, InputTarget, MouseButton, MouseCubicBezierSegment, MouseMotionOptions, MoveMouseRequest, Point, Rect, Window,
  WindowRef,
  mouse_input::{InputCancellation, with_input_cancellation},
};
use auv_driver_windows::WindowsDriver;
use std::{
  cell::RefCell,
  sync::Arc,
  time::{Duration, Instant},
};
use windows::{
  Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
    Graphics::Gdi::ClientToScreen,
    UI::{Input::KeyboardAndMouse::GetAsyncKeyState, WindowsAndMessaging::*},
  },
  core::w,
};

thread_local! {
  static RECEIPTS: RefCell<Vec<(u32, usize, i32, i32)>> = const { RefCell::new(Vec::new()) };
}

unsafe extern "system" fn receive(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
  if matches!(message, WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONDOWN | WM_RBUTTONUP | WM_MBUTTONDOWN | WM_MBUTTONUP) {
    let x = (lp.0 & 0xffff) as i16 as i32;
    let y = ((lp.0 >> 16) & 0xffff) as i16 as i32;
    RECEIPTS.with(|r| r.borrow_mut().push((message, wp.0, x, y)));
    return LRESULT(0);
  }
  // SAFETY: Forward unchanged native callback arguments; no borrowed state escapes.
  unsafe { DefWindowProcW(hwnd, message, wp, lp) }
}

struct Receiver(HWND);
impl Drop for Receiver {
  fn drop(&mut self) {
    // SAFETY: The fixture owns this HWND and class, both destroyed on their thread.
    unsafe {
      let _ = DestroyWindow(self.0);
      let _ = UnregisterClassW(w!("AuvHeldReceiver"), None);
    }
  }
}

fn pump(duration: Duration) {
  let deadline = Instant::now() + duration;
  loop {
    let mut message = MSG::default();
    // SAFETY: MSG is initialized; only this fixture thread's queue is dispatched.
    unsafe {
      while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
        DispatchMessageW(&message);
      }
    }
    if Instant::now() >= deadline {
      break;
    }
    std::thread::sleep(Duration::from_millis(3));
  }
}

#[test]
#[ignore = "creates a hidden Win32 receiver and posts held input; run explicitly"]
fn background_receives_held_buttons_and_cleanup() {
  run(false);
}

#[test]
#[ignore = "moves pointer; requires an unlocked interactive desktop and no operator input"]
fn foreground_receives_held_buttons_and_cleanup() {
  run(true);
}

fn run(foreground: bool) {
  // Both opt-in cases share a native class and desktop; do not run them concurrently.
  static RECEIVER_TEST: std::sync::Mutex<()> = std::sync::Mutex::new(());
  let _serial = RECEIVER_TEST.lock().unwrap();
  let class = WNDCLASSW {
    lpfnWndProc: Some(receive),
    lpszClassName: w!("AuvHeldReceiver"),
    ..Default::default()
  };
  // SAFETY: Static strings and callback outlive the window. Receiver owns cleanup.
  let receiver = unsafe {
    assert_ne!(RegisterClassW(&class), 0);
    Receiver(
      CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        class.lpszClassName,
        w!("AUV held mouse receiver"),
        WS_OVERLAPPEDWINDOW
          | if foreground {
            WS_VISIBLE
          } else {
            WINDOW_STYLE::default()
          },
        100,
        100,
        640,
        480,
        None,
        None,
        None,
        None,
      )
      .unwrap(),
    )
  };
  if foreground {
    // SAFETY: Activation is limited to our live receiver; readiness is checked below.
    unsafe {
      let _ = SetForegroundWindow(receiver.0);
    }
  }
  pump(Duration::from_millis(500));
  let mut native_point = POINT { x: 120, y: 120 };
  // SAFETY: Live owned HWND and initialized writable POINT.
  assert!(unsafe { ClientToScreen(receiver.0, &mut native_point) }.as_bool());
  let start = Point::new(native_point.x as f64, native_point.y as f64);
  let end = Point::new(start.x + 80.0, start.y + 40.0);
  let session = WindowsDriver::default().open_local().unwrap();
  let mouse = session.input().create_mouse().unwrap();
  let old_point = foreground.then(|| session.input().current_position().unwrap());
  let target = if foreground {
    InputTarget::Foreground
  } else {
    InputTarget::Window(Window {
      reference: WindowRef {
        id: (receiver.0.0 as usize).to_string(),
      },
      title: None,
      app_name: None,
      app_bundle_id: None,
      process_id: Some(std::process::id()),
      frame: Rect::new(100.0, 100.0, 640.0, 480.0),
      coordinate_space: CoordinateSpace::Screen,
      is_main: false,
      is_visible: false,
    })
  };
  let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    for (button, down, up, mask) in [
      (MouseButton::Left, WM_LBUTTONDOWN, WM_LBUTTONUP, 1),
      (MouseButton::Right, WM_RBUTTONDOWN, WM_RBUTTONUP, 2),
      (MouseButton::Middle, WM_MBUTTONDOWN, WM_MBUTTONUP, 16),
    ] {
      for case in ["cross-call", "hold", "watchdog", "cancel", "drag"] {
        let hwnd = receiver.0.0 as usize;
        let guard = move || {
          if foreground {
            // SAFETY: Read-only OS state queries; no pointers are dereferenced.
            assert_eq!(unsafe { GetForegroundWindow() }.0 as usize, hwnd, "receiver lost foreground");
          }
        };
        guard();
        if foreground {
          for key in [1, 2, 4, 16, 17, 18, 91, 92] {
            // SAFETY: Read-only native virtual-key query. Refuse operator-held input.
            assert_eq!(unsafe { GetAsyncKeyState(key) } & i16::MIN, 0, "operator-held key/button {key}");
          }
        }
        RECEIPTS.with(|r| r.borrow_mut().clear());
        let target = target.clone();
        let worker_session = session.clone();
        let readiness = Arc::new(InputCancellation::default());
        let worker_readiness = readiness.clone();
        let worker = std::thread::spawn(move || {
          with_input_cancellation(worker_readiness, || {
            let input = worker_session.input();
            guard();
            match case {
              "cross-call" => {
                input.mouse_down(&target, mouse, start, button, Duration::from_secs(3)).unwrap();
                std::thread::sleep(Duration::from_millis(80));
                guard();
                input.move_mouse_to(mouse, end).unwrap();
                std::thread::sleep(Duration::from_millis(80));
                input.mouse_up(mouse).unwrap();
              }
              "hold" => {
                input.hold_mouse(&target, mouse, start, button, Duration::from_millis(150)).unwrap();
              }
              "watchdog" => {
                input.mouse_down(&target, mouse, start, button, Duration::from_millis(180)).unwrap();
                std::thread::sleep(Duration::from_millis(450));
              }
              "cancel" => {
                let cancel = Arc::new(InputCancellation::default());
                with_input_cancellation(cancel.clone(), || input.mouse_down(&target, mouse, start, button, Duration::from_secs(5))).unwrap();
                std::thread::sleep(Duration::from_millis(100));
                cancel.cancel();
                std::thread::sleep(Duration::from_millis(200));
              }
              "drag" => {
                let mut request = MoveMouseRequest::direct(start);
                request.mouse = mouse;
                request.target = Some(target);
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
          })
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        while !worker.is_finished() {
          assert!(Instant::now() < deadline, "sender deadline");
          pump(Duration::from_millis(10));
          // SAFETY: Read-only foreground query. Cancel active hold/drag when
          // the receiver loses ownership, including desktop switches.
          if foreground && unsafe { GetForegroundWindow() }.0 as usize != hwnd {
            readiness.cancel();
          }
        }
        worker.join().unwrap();
        pump(Duration::from_millis(100));
        guard();
        let receipts = RECEIPTS.with(|r| r.borrow().clone());
        println!("foreground={foreground} {button:?} {case}: {receipts:?}");
        let transitions: Vec<_> = receipts.iter().filter(|r| r.0 != WM_MOUSEMOVE).collect();
        assert_eq!(transitions.len(), 2, "exact down/up: {receipts:?}");
        assert_eq!((transitions[0].0, transitions[0].1), (down, mask));
        assert_eq!((transitions[1].0, transitions[1].1), (up, 0));
        let moved = matches!(case, "cross-call" | "drag");
        let expected = if moved { (200, 160) } else { (120, 120) };
        assert!(
          (transitions[1].2 - expected.0).abs() <= 2 && (transitions[1].3 - expected.1).abs() <= 2,
          "release coordinates: {receipts:?}"
        );
        if moved {
          assert!(receipts.iter().any(|r| r.0 == WM_MOUSEMOVE && r.1 == mask), "held motion: {receipts:?}");
        }
        RECEIPTS.with(|r| r.borrow_mut().clear());
        session.input().mouse_up(mouse).unwrap();
        pump(Duration::from_millis(50));
        assert!(RECEIPTS.with(|r| r.borrow().iter().all(|r| r.0 == WM_MOUSEMOVE)), "idempotent up posted twice");
      }
    }
  }));
  session.input().remove_mouse(mouse).expect("release test-owned input");
  if result.is_ok() {
    if let Some(point) = old_point {
      session.input().move_to(point).unwrap();
    }
  }
  if let Err(panic) = result {
    std::panic::resume_unwind(panic);
  }
}
