//! Live receipt evidence for foreground input; run only in an unlocked desktop.
#![cfg(target_os = "windows")]

use std::{
  cell::RefCell,
  time::{Duration, Instant},
};

use auv_driver_common::{
  Click, ClickModifiers, ClickOptions, CoordinateSpace, Driver, InputActionResult, InputDeliveryPath, InputPolicy, MouseButton,
  MoveMouseRequest, Point, Rect, Scroll, ScrollOptions, Window, WindowPoint, WindowRef, input::WindowInput,
};
use auv_driver_windows::{WindowsDriver, WindowsDriverSession, input};
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
      | WM_MOUSEWHEEL
      | WM_MOUSEHWHEEL
  ) {
    RECEIPTS.with(|receipts| receipts.borrow_mut().push((message, wp.0)));
    return LRESULT(0);
  }
  // SAFETY: Forward unchanged OS callback arguments to the default procedure.
  unsafe { DefWindowProcW(hwnd, message, wp, lp) }
}

struct Receiver {
  hwnd: HWND,
  original_pointer: Point,
  original_foreground: HWND,
}

impl Receiver {
  fn open() -> Self {
    let original_pointer = input::current_position().expect("read pointer before opening receiver");
    // SAFETY: read-only foreground query before this fixture creates a window.
    let original_foreground = unsafe { GetForegroundWindow() };
    let class = WNDCLASSW {
      style: CS_DBLCLKS,
      lpfnWndProc: Some(receive),
      lpszClassName: w!("AuvForegroundButtonsReceiver"),
      ..Default::default()
    };
    // SAFETY: Static class strings and callback outlive the window; Receiver owns
    // destruction on this thread. No application-owned window is used.
    unsafe {
      assert_ne!(RegisterClassW(&class), 0);
      Self {
        hwnd: CreateWindowExW(
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
        original_pointer,
        original_foreground,
      }
    }
  }
}

impl Drop for Receiver {
  fn drop(&mut self) {
    // SAFETY: The test owns both resources and drops them on the window thread.
    unsafe {
      let restore_foreground = GetForegroundWindow() == self.hwnd;
      let _ = SetCursorPos(self.original_pointer.x as i32, self.original_pointer.y as i32);
      let _ = DestroyWindow(self.hwnd);
      let _ = UnregisterClassW(w!("AuvForegroundButtonsReceiver"), None);
      if restore_foreground && IsWindow(self.original_foreground).as_bool() {
        let _ = SetForegroundWindow(self.original_foreground);
      }
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

/// Reproduces activation failure with a destroyed owned HWND over a still-live independent receiver.
/// All diagnostic reads occur before Receiver drops and restores the pointer/foreground.
fn assert_activation_failure_refuses_input(
  operation: &str,
  action: impl FnOnce(&WindowsDriverSession, &Window, WindowPoint) -> auv_driver_common::DriverResult<InputActionResult>,
) {
  let receiver = Receiver::open();
  // SAFETY: This test owns the class and both windows. The second window is destroyed before
  // the public Driver call; its old rectangle is deliberately over the live receiving window.
  let closed = unsafe {
    let hwnd = CreateWindowExW(
      WINDOW_EX_STYLE::default(),
      w!("AuvForegroundButtonsReceiver"),
      w!("AUV closed foreground target"),
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
    .expect("create owned target");
    DestroyWindow(hwnd).expect("close owned target");
    assert!(!IsWindow(hwnd).as_bool(), "target did not close");
    let _ = SetForegroundWindow(receiver.hwnd);
    hwnd
  };
  let deadline = Instant::now() + Duration::from_secs(2);
  loop {
    pump_for(Duration::from_millis(10));
    // SAFETY: read-only foreground query; the receiver remains live on this thread.
    if unsafe { GetForegroundWindow() } == receiver.hwnd {
      break;
    }
    assert!(Instant::now() < deadline, "owned receiver did not obtain foreground; refuse injection");
  }
  let window = Window {
    reference: WindowRef {
      id: (closed.0 as isize).to_string(),
    },
    title: Some("AUV closed foreground target".into()),
    app_name: Some("foreground-admission-test".into()),
    application_id: None,
    process_id: Some(std::process::id()),
    frame: Rect::new(100.0, 100.0, 640.0, 480.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
  };
  let session = WindowsDriver::default().open_local().expect("open driver session");
  let before = input::current_position().expect("read pointer before operation");
  let point = if before == Point::new(250.0, 250.0) {
    WindowPoint::new(200.0, 150.0)
  } else {
    WindowPoint::new(150.0, 150.0)
  };
  // SAFETY: Read-only checks. No physical button/modifier may merge with test input, and the
  // stale HWND must still be destroyed before the operation under investigation.
  unsafe {
    assert!(!IsWindow(closed).as_bool(), "closed target handle was reused");
    assert_eq!(GetForegroundWindow(), receiver.hwnd);
    for key in [0x01, 0x02, 0x04, 0x10, 0x11, 0x12, 0x5b, 0x5c] {
      assert!(GetAsyncKeyState(key) >= 0, "button/modifier already held: {key}");
    }
  }
  RECEIPTS.with(|receipts| receipts.borrow_mut().clear());
  let result = action(&session, &window, point);
  pump_for(Duration::from_millis(150));
  let after = input::current_position().expect("read pointer after operation");
  let events = RECEIPTS.with(|receipts| receipts.borrow().clone());
  // SAFETY: Read-only foreground query before any fixture cleanup.
  let foreground_unchanged = unsafe { GetForegroundWindow() } == receiver.hwnd;
  println!(
    "{operation}: result={result:?}; receiver_events={events:?}; pointer={before:?}->{after:?}; foreground_unchanged={foreground_unchanged}"
  );
  assert!(result.is_err(), "activation failure must refuse global {operation} input");
  assert!(events.is_empty(), "activation failure delivered {operation} to the live receiver");
  assert_eq!(after, before, "activation failure moved the shared pointer during {operation}");
  assert!(foreground_unchanged, "activation failure disturbed the live foreground during {operation}");
}

// https://github.com/moeru-ai/auv/pull/429#issuecomment-6481267529
// ROOT CAUSE:
//
// If foreground activation failed, the window operation recorded a failed attempt and still
// sent global input. A closed target could therefore click, scroll, or drag a different window.
// Before the fix, delivery succeeded against the independent live receiver.
// The fix returns the activation failure before entering global input delivery.
#[test]
#[ignore = "may reproduce global input into an owned receiver; requires an isolated interactive desktop"]
fn foreground_activation_failure_refuses_click_issue_429() {
  assert_activation_failure_refuses_input("click", |session, window, point| {
    session.window().click(
      window,
      point,
      ClickOptions {
        policy: InputPolicy::ForegroundPreferred,
        ..Default::default()
      },
    )
  });
}

// https://github.com/moeru-ai/auv/pull/429#issuecomment-6481267529
#[test]
#[ignore = "may reproduce global input into an owned receiver; requires an isolated interactive desktop"]
fn foreground_activation_failure_refuses_scroll_issue_429() {
  assert_activation_failure_refuses_input("scroll", |session, window, point| {
    session.window().scroll(
      window,
      point,
      Scroll {
        delta_x: 0.0,
        delta_y: -120.0,
      },
      ScrollOptions {
        policy: InputPolicy::ForegroundPreferred,
        ..Default::default()
      },
    )
  });
}

// https://github.com/moeru-ai/auv/pull/429#issuecomment-6481267529
#[test]
#[ignore = "may reproduce global input into an owned receiver; requires an isolated interactive desktop"]
fn foreground_activation_failure_refuses_drag_issue_429() {
  assert_activation_failure_refuses_input("drag", |session, window, point| {
    let screen = session.window().to_screen_point(window, point)?.point();
    session
      .window()
      .drag(window, MoveMouseRequest::direct(screen), MouseButton::Left, InputPolicy::ForegroundPreferred)
      .map(|(_, result)| result)
  });
}

// Positive control for the same three public WindowApi routes as the closed-target regressions.
#[test]
#[ignore = "moves the pointer and injects input into an owned receiver; requires an isolated interactive desktop"]
fn foreground_window_inputs_reach_owned_receiver() {
  let receiver = Receiver::open();
  // SAFETY: activating only the live window this test owns.
  unsafe {
    let _ = SetForegroundWindow(receiver.hwnd);
  }
  pump_for(Duration::from_millis(500));
  // SAFETY: read-only foreground query before accepting any injection.
  assert_eq!(unsafe { GetForegroundWindow() }, receiver.hwnd);
  let session = WindowsDriver::default().open_local().expect("open driver session");
  let id = (receiver.hwnd.0 as isize).to_string();
  let window =
    session.window().list().expect("list windows").into_iter().find(|window| window.reference.id == id).expect("find owned receiver");
  let point = WindowPoint::new(150.0, 150.0);
  // Separate the complete click and drag gestures past native double-click detection, as the
  // existing button/count control does. No input operation is retried.
  // SAFETY: Read-only system setting with no pointer arguments.
  let reset = Duration::from_millis(u64::from(unsafe { GetDoubleClickTime() }) + 100);
  for (operation, expected) in [
    ("click", vec![WM_LBUTTONDOWN, WM_LBUTTONUP]),
    ("scroll", vec![WM_MOUSEWHEEL]),
    ("drag", vec![WM_LBUTTONDOWN, WM_LBUTTONUP]),
  ] {
    pump_for(reset);
    // SAFETY: no input may go to a different foreground window or merge with held user keys.
    unsafe {
      assert_eq!(GetForegroundWindow(), receiver.hwnd);
      for key in [0x01, 0x02, 0x04, 0x10, 0x11, 0x12, 0x5b, 0x5c] {
        assert!(GetAsyncKeyState(key) >= 0, "button/modifier already held: {key}");
      }
    }
    RECEIPTS.with(|receipts| receipts.borrow_mut().clear());
    let result = match operation {
      "click" => session.window().click(
        &window,
        point,
        ClickOptions {
          policy: InputPolicy::ForegroundPreferred,
          ..Default::default()
        },
      ),
      "scroll" => session.window().scroll(
        &window,
        point,
        Scroll {
          delta_x: 0.0,
          delta_y: -120.0,
        },
        ScrollOptions {
          policy: InputPolicy::ForegroundPreferred,
          ..Default::default()
        },
      ),
      "drag" => {
        let screen = session.window().to_screen_point(&window, point).expect("map owned point").point();
        session
          .window()
          .drag(&window, MoveMouseRequest::direct(screen), MouseButton::Left, InputPolicy::ForegroundPreferred)
          .map(|(_, result)| result)
      }
      _ => unreachable!(),
    }
    .expect("valid foreground operation failed");
    pump_for(Duration::from_millis(150));
    let events = RECEIPTS.with(|receipts| receipts.borrow().iter().map(|(message, _)| *message).collect::<Vec<_>>());
    assert_eq!(result.selected_path, InputDeliveryPath::ForegroundSystemEvents);
    assert!(!result.verified, "delivery does not establish semantic verification");
    assert_eq!(result.attempts.len(), 2);
    assert!(result.attempts.iter().all(|attempt| attempt.succeeded));
    assert_eq!(events, expected, "valid foreground {operation} missed the owned receiver");
    println!("valid {operation}: receiver_messages={events:?}; result={result:?}");
  }
}

#[test]
#[ignore = "moves the pointer and injects clicks into a visible receiver; needs an unlocked interactive desktop"]
fn foreground_receives_buttons_counts_and_modifiers() {
  let receiver = Receiver::open();
  // SAFETY: The HWND is owned and live. Refuse injection if activation fails.
  unsafe {
    let _ = SetForegroundWindow(receiver.hwnd);
  }
  pump_for(Duration::from_millis(500));
  // SAFETY: Read-only foreground HWND query; receiver remains live.
  assert_eq!(unsafe { GetForegroundWindow() }, receiver.hwnd, "activate AUV foreground button test and retry");
  let mut point = POINT { x: 150, y: 150 };
  // SAFETY: Live HWND and initialized writable POINT.
  assert!(unsafe { ClientToScreen(receiver.hwnd, &mut point) }.as_bool());
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
          assert_eq!(GetForegroundWindow(), receiver.hwnd, "test window lost foreground");
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
