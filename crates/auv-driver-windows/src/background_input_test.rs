use super::native::{make_lparam, make_wheel_wparam};

#[test]
fn make_lparam_packs_low_and_high_words() {
  assert_eq!(make_lparam(10, 20).0, (20 << 16) | 10);
}

#[test]
fn make_lparam_zero_extends_instead_of_sign_extends() {
  // A coordinate whose low 16 bits have the top bit set (e.g. y=40000) must
  // not corrupt the packed value the way a sign-extending cast would.
  let packed = make_lparam(0, 40_000).0;
  assert_eq!(packed, 40_000 << 16);
}

#[test]
fn checked_client_lparam_accepts_signed_16bit_range_and_roundtrips() {
  use super::native::checked_client_lparam;
  use windows::Win32::Foundation::POINT;

  for (x, y) in [
    (0, 0),
    (i16::MIN as i32, 0),
    (i16::MAX as i32, 0),
    (i16::MIN as i32, i16::MAX as i32),
    (i16::MAX as i32, i16::MIN as i32),
  ] {
    let lparam = checked_client_lparam(POINT { x, y }).expect("valid i16 coordinate must succeed");
    let decoded_x = (lparam.0 as u32 & 0xFFFF) as i16 as i32;
    let decoded_y = ((lparam.0 as u32 >> 16) & 0xFFFF) as i16 as i32;
    assert_eq!((decoded_x, decoded_y), (x, y), "roundtrip mismatch for ({x}, {y})");
  }
}

#[test]
fn checked_client_lparam_rejects_out_of_range_coordinates() {
  use super::native::checked_client_lparam;
  use windows::Win32::Foundation::POINT;

  for (x, y) in [
    (32_768, 0),
    (-32_769, 0),
    (0, 40_000),
    (0, -40_000),
    (40_000, 40_000),
  ] {
    let err = checked_client_lparam(POINT { x, y }).expect_err(&format!("({x}, {y}) must be rejected"));
    assert!(err.to_string().contains("signed 16-bit message range"), "error message must describe range violation: {err}");
  }
}

#[test]
fn make_wheel_wparam_places_delta_in_high_word() {
  assert_eq!(make_wheel_wparam(120).0, 120 << 16);
}

#[test]
fn make_wheel_wparam_preserves_negative_delta() {
  let packed = make_wheel_wparam(-120).0;
  let high_word = (packed >> 16) as u16 as i16;
  assert_eq!(high_word, -120);
}

#[test]
fn background_click_flags_carry_shift_and_control() {
  assert_eq!(
    super::native::mouse_modifier_flags(auv_driver_common::ClickModifiers {
      shift: true,
      control: true,
      ..Default::default()
    }),
    0x0004 | 0x0008
  );
  assert_eq!(super::native::mouse_modifier_flags(Default::default()), 0);
}

/// Independent Win32 receipt oracle; requires a Windows desktop session.
#[test]
#[ignore = "creates a dedicated Win32 window and dispatches posted mouse messages"]
fn window_receives_modified_click_messages_without_modifier_carryover() {
  use auv_driver_common::{Click, ClickModifiers, CoordinateSpace, Point, Rect, Window, WindowRef};
  use std::cell::RefCell;
  use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
  use windows::Win32::Graphics::Gdi::ClientToScreen;
  use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, RegisterClassW, UnregisterClassW,
    WINDOW_EX_STYLE, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDBLCLK, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_RBUTTONDBLCLK,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WNDCLASSW, WS_OVERLAPPEDWINDOW,
  };
  use windows::core::w;
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
    // SAFETY: Forward the unchanged arguments supplied by the OS window procedure.
    unsafe { DefWindowProcW(hwnd, message, wp, lp) }
  }
  struct Receiver(HWND);
  impl Drop for Receiver {
    fn drop(&mut self) {
      // SAFETY: This test owns the window and registered class on this thread.
      unsafe {
        let _ = DestroyWindow(self.0);
        let _ = UnregisterClassW(w!("AuvClickModifiersReceiver"), None);
      }
    }
  }
  let class = WNDCLASSW {
    lpfnWndProc: Some(receive),
    lpszClassName: w!("AuvClickModifiersReceiver"),
    ..Default::default()
  };
  // SAFETY: The class name is static and the callback remains valid throughout
  // the window lifetime. This receiver stays hidden; no foreground input is used.
  let receiver = unsafe {
    assert_ne!(RegisterClassW(&class), 0);
    Receiver(
      CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        class.lpszClassName,
        w!("AUV click modifier test"),
        WS_OVERLAPPEDWINDOW,
        100,
        100,
        320,
        200,
        None,
        None,
        None,
        None,
      )
      .unwrap(),
    )
  };
  let mut point = POINT { x: 20, y: 20 };
  // SAFETY: The receiver is live and point is a writable initialized POINT.
  assert!(unsafe { ClientToScreen(receiver.0, &mut point) }.as_bool());
  let window = Window {
    reference: WindowRef {
      id: (receiver.0.0 as usize).to_string(),
    },
    title: None,
    app_name: None,
    application_id: None,
    process_id: None,
    frame: Rect::new(100.0, 100.0, 320.0, 200.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: false,
    is_visible: false,
  };
  for (button, down, up, double, held) in [
    (auv_driver_common::MouseButton::Left, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_LBUTTONDBLCLK, 1),
    (auv_driver_common::MouseButton::Right, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_RBUTTONDBLCLK, 2),
    (auv_driver_common::MouseButton::Middle, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MBUTTONDBLCLK, 16),
  ] {
    for (click, presses) in [
      (Click::Single, vec![down]),
      (
        Click::Double {
          interval: std::time::Duration::from_millis(10),
        },
        vec![down, double],
      ),
      (
        Click::Repeated {
          count: 3,
          interval: std::time::Duration::from_millis(10),
        },
        vec![down, down, down],
      ),
    ] {
      RECEIPTS.with(|receipts| receipts.borrow_mut().clear());
      for modifiers in [
        ClickModifiers {
          shift: true,
          control: true,
          ..Default::default()
        },
        ClickModifiers::default(),
      ] {
        super::click_at_window(&window, Point::new(f64::from(point.x), f64::from(point.y)), button, click.clone(), modifiers).unwrap();
        let mut message = MSG::default();
        // SAFETY: Dispatch only messages for the live test window on its owner thread.
        unsafe {
          while PeekMessageW(&mut message, receiver.0, 0, 0, PM_REMOVE).as_bool() {
            DispatchMessageW(&message);
          }
        }
      }
      // Posted double clicks must reach the receiver as DBLCLK, while repeated
      // clicks stay independent down/up pairs. The next call clears modifiers.
      let expected: Vec<_> = [0x000c, 0]
        .into_iter()
        .flat_map(|modifiers| presses.iter().flat_map(move |press| [(*press, held | modifiers), (up, modifiers)]))
        .collect();
      RECEIPTS.with(|receipts| assert_eq!(*receipts.borrow(), expected, "{button:?} {click:?}"));
    }
  }
}

/// Verifies that native click rejects client coordinates exceeding the signed 16-bit
/// Win32 message contract before any WM_*BUTTONDOWN/UP/DBLCLK messages are dispatched.
#[test]
#[ignore = "creates a dedicated Win32 window and dispatches posted mouse messages"]
fn background_click_rejects_client_coordinates_exceeding_signed_16bit_without_posting() {
  use auv_driver_common::{Click, ClickModifiers, CoordinateSpace, Point, Rect, Window, WindowRef};
  use std::cell::RefCell;
  use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
  use windows::Win32::Graphics::Gdi::ClientToScreen;
  use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, RegisterClassW, UnregisterClassW,
    WINDOW_EX_STYLE, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WNDCLASSW, WS_OVERLAPPEDWINDOW,
  };
  use windows::core::w;

  thread_local! {
    static RECEIPTS: RefCell<Vec<(u32, usize)>> = const { RefCell::new(Vec::new()) };
  }
  unsafe extern "system" fn receive(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if matches!(message, WM_LBUTTONDOWN | WM_LBUTTONUP | WM_LBUTTONDBLCLK) {
      RECEIPTS.with(|r| r.borrow_mut().push((message, wp.0)));
      return LRESULT(0);
    }
    // SAFETY: Forward the unchanged arguments supplied by the OS window procedure.
    unsafe { DefWindowProcW(hwnd, message, wp, lp) }
  }
  struct Receiver(HWND);
  impl Drop for Receiver {
    fn drop(&mut self) {
      // SAFETY: This test owns the window and registered class on this thread.
      unsafe {
        let _ = DestroyWindow(self.0);
        let _ = UnregisterClassW(w!("AuvI16CoordRangeReceiver"), None);
      }
    }
  }
  let class = WNDCLASSW {
    lpfnWndProc: Some(receive),
    lpszClassName: w!("AuvI16CoordRangeReceiver"),
    ..Default::default()
  };
  let receiver = unsafe {
    assert_ne!(RegisterClassW(&class), 0);
    Receiver(
      CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        class.lpszClassName,
        w!("AUV coordinate range test"),
        WS_OVERLAPPEDWINDOW,
        100,
        100,
        320,
        200,
        None,
        None,
        None,
        None,
      )
      .unwrap(),
    )
  };
  let window = Window {
    reference: WindowRef {
      id: (receiver.0.0 as usize).to_string(),
    },
    title: None,
    app_name: None,
    application_id: None,
    process_id: None,
    frame: Rect::new(100.0, 100.0, 320.0, 200.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: false,
    is_visible: false,
  };

  let mut client_origin = POINT { x: 0, y: 0 };
  assert!(unsafe { ClientToScreen(receiver.0, &mut client_origin) }.as_bool());

  let drain_messages = || {
    let mut message = MSG::default();
    unsafe {
      while PeekMessageW(&mut message, receiver.0, 0, 0, PM_REMOVE).as_bool() {
        DispatchMessageW(&message);
      }
    }
  };

  // 1. Positive baseline: valid coordinate successfully delivers messages
  RECEIPTS.with(|r| r.borrow_mut().clear());
  super::click_at_window(
    &window,
    Point::new(f64::from(client_origin.x + 20), f64::from(client_origin.y + 20)),
    auv_driver_common::MouseButton::Left,
    Click::Single,
    ClickModifiers::default(),
  )
  .expect("valid coordinate must succeed");
  drain_messages();
  RECEIPTS.with(|r| {
    assert_eq!(r.borrow().len(), 2, "single click delivers DOWN and UP");
  });

  // 2. Negative rejections: out-of-range coordinates fail without posting any messages
  let test_cases = [
    ("x exceeds i16", Point::new(f64::from(client_origin.x + 40_000), f64::from(client_origin.y))),
    ("y exceeds i16", Point::new(f64::from(client_origin.x), f64::from(client_origin.y + 40_000))),
    ("negative underflow", Point::new(f64::from(client_origin.x - 40_000), f64::from(client_origin.y))),
  ];

  let click_variants = [
    Click::Single,
    Click::Double {
      interval: std::time::Duration::from_millis(10),
    },
    Click::Repeated {
      count: 3,
      interval: std::time::Duration::from_millis(10),
    },
  ];

  for (label, screen_pt) in test_cases {
    for click in &click_variants {
      RECEIPTS.with(|r| r.borrow_mut().clear());
      let err = super::click_at_window(&window, screen_pt, auv_driver_common::MouseButton::Left, click.clone(), ClickModifiers::default())
        .expect_err("out-of-range coordinate must be rejected before delivery");
      assert!(err.to_string().contains("signed 16-bit message range"));
      drain_messages();
      RECEIPTS.with(|r| {
        assert!(r.borrow().is_empty(), "no message may be posted for {label} ({click:?})");
      });
    }
  }
}
