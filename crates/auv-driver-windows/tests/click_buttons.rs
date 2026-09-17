//! Live receipt evidence for foreground input; run only in an unlocked desktop.
#![cfg(target_os = "windows")]

use std::{
  cell::RefCell,
  time::{Duration, Instant},
};

use auv_driver_common::{Click, ClickModifiers, InputDeliveryPath, MouseButton, Point};
use auv_driver_windows::input;
use windows::{
  Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
    Graphics::Gdi::ClientToScreen,
    UI::{
      Input::KeyboardAndMouse::{GetAsyncKeyState, GetDoubleClickTime},
      WindowsAndMessaging::*,
    },
  },
  core::w,
};

thread_local! {
  static RECEIPTS: RefCell<Vec<(u32, usize)>> = const { RefCell::new(Vec::new()) };
}

unsafe extern "system" fn receive(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
  if matches!(
    message,
    WM_LBUTTONDOWN
      | WM_LBUTTONUP
      | WM_LBUTTONDBLCLK
      | WM_RBUTTONDOWN
      | WM_RBUTTONUP
      | WM_RBUTTONDBLCLK
      | WM_MBUTTONDOWN
      | WM_MBUTTONUP
      | WM_MBUTTONDBLCLK
  ) {
    RECEIPTS.with(|receipts| receipts.borrow_mut().push((message, wp.0)));
    return LRESULT(0);
  }
  // SAFETY: Forward unchanged OS callback arguments to the default procedure.
  unsafe { DefWindowProcW(hwnd, message, wp, lp) }
}

struct Receiver(HWND);
impl Drop for Receiver {
  fn drop(&mut self) {
    // SAFETY: The test owns both resources and drops them on the window thread.
    unsafe {
      let _ = DestroyWindow(self.0);
      let _ = UnregisterClassW(w!("AuvForegroundButtonsReceiver"), None);
    }
  }
}

/// Pump on the owning thread so receipt includes Windows input translation,
/// including modifier flags and native double-click detection.
fn pump_for(duration: Duration) {
  let until = Instant::now() + duration;
  loop {
    let mut message = MSG::default();
    // SAFETY: MSG is initialized; dispatch only this dedicated test thread's queue.
    unsafe {
      while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
        DispatchMessageW(&message);
      }
    }
    if Instant::now() >= until {
      break;
    }
    std::thread::sleep(Duration::from_millis(5));
  }
}

#[test]
#[ignore = "moves the pointer and injects clicks into a visible receiver; needs an unlocked interactive desktop"]
fn foreground_receives_buttons_counts_and_modifiers() {
  let class = WNDCLASSW {
    style: CS_DBLCLKS,
    lpfnWndProc: Some(receive),
    lpszClassName: w!("AuvForegroundButtonsReceiver"),
    ..Default::default()
  };
  // SAFETY: Static class strings and callback outlive the window; Receiver owns
  // destruction on this thread. No application-owned window is used.
  let receiver = unsafe {
    assert_ne!(RegisterClassW(&class), 0);
    Receiver(
      CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        class.lpszClassName,
        w!("AUV foreground button test"),
        WS_OVERLAPPEDWINDOW | WS_VISIBLE,
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
  // SAFETY: The HWND is owned and live. Refuse injection if activation fails.
  unsafe {
    let _ = SetForegroundWindow(receiver.0);
  }
  pump_for(Duration::from_millis(500));
  // SAFETY: Read-only foreground HWND query; receiver remains live.
  assert_eq!(unsafe { GetForegroundWindow() }, receiver.0, "activate AUV foreground button test and retry");
  let mut point = POINT { x: 150, y: 150 };
  // SAFETY: Live HWND and initialized writable POINT.
  assert!(unsafe { ClientToScreen(receiver.0, &mut point) }.as_bool());
  let point = Point::new(f64::from(point.x), f64::from(point.y));
  let old_point = input::current_position().unwrap();
  // SAFETY: Read-only system setting; no pointer parameters.
  let reset = Duration::from_millis(u64::from(unsafe { GetDoubleClickTime() }) + 100);
  for (button, down, up, double, held) in [
    (MouseButton::Left, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_LBUTTONDBLCLK, 1),
    (MouseButton::Right, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_RBUTTONDBLCLK, 2),
    (MouseButton::Middle, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MBUTTONDBLCLK, 16),
  ] {
    for (click, presses) in [
      (Click::Single, vec![down]),
      (
        Click::Double {
          interval: Duration::from_millis(50),
        },
        vec![down, double],
      ),
      (
        Click::Repeated {
          count: 3,
          interval: reset,
        },
        vec![down, down, down],
      ),
    ] {
      for (modifiers, flags) in [
        (
          ClickModifiers {
            shift: true,
            control: true,
            ..Default::default()
          },
          0x000c,
        ),
        (ClickModifiers::default(), 0),
      ] {
        // Separate cases past the system double-click interval. Repeated input
        // also uses that interval so its receipt is three independent clicks.
        pump_for(reset);
        // SAFETY: Read-only OS queries. Avoid injecting into an unrelated window
        // or merging with physical buttons/modifiers held by the operator.
        unsafe {
          assert_eq!(GetForegroundWindow(), receiver.0, "test window lost foreground");
          for key in [0x01, 0x02, 0x04, 0x10, 0x11, 0x12, 0x5b, 0x5c] {
            assert!(GetAsyncKeyState(key) >= 0, "button/modifier already held: {key}");
          }
        }
        RECEIPTS.with(|receipts| receipts.borrow_mut().clear());
        // Keep the independent receiver responsive while the synchronous driver
        // waits between clicks. A blocked receiver distorts native input routing.
        let requested_click = click.clone();
        let delivery = std::thread::spawn(move || input::click_at(point, button, requested_click, modifiers));
        while !delivery.is_finished() {
          pump_for(Duration::from_millis(10));
        }
        let result = delivery.join().unwrap().unwrap();
        assert_eq!(result.selected_path, InputDeliveryPath::ForegroundSystemEvents);
        assert!(!result.verified, "driver delivery must not claim semantic verification");
        pump_for(Duration::from_millis(250));
        let expected: Vec<_> = presses.iter().flat_map(|press| [(*press, held | flags), (up, flags)]).collect();
        RECEIPTS.with(|receipts| assert_eq!(*receipts.borrow(), expected, "{button:?} {click:?} {modifiers:?}"));
        // SAFETY: Read-only OS state confirms modifier and button release.
        unsafe {
          for key in [0x01, 0x02, 0x04, 0x10, 0x11] {
            assert!(GetAsyncKeyState(key) >= 0, "button/modifier remains held: {key}");
          }
        }
        println!("PASS {button:?} {click:?} {modifiers:?}: {expected:?}");
      }
    }
  }
  input::move_to(old_point).unwrap();
}
