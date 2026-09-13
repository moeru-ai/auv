use super::native::{make_lparam, make_wheel_wparam, wheel_amount};

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
fn wheel_amount_scales_by_wheel_delta_unit() {
  assert_eq!(wheel_amount(1.0), 120);
  assert_eq!(wheel_amount(-0.5), -60);
}

#[test]
fn wheel_amount_treats_non_finite_delta_as_zero() {
  assert_eq!(wheel_amount(f64::NAN), 0);
  assert_eq!(wheel_amount(f64::INFINITY), 0);
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
    WINDOW_EX_STYLE, WM_LBUTTONDOWN, WM_LBUTTONUP, WNDCLASSW, WS_OVERLAPPEDWINDOW,
  };
  use windows::core::w;
  thread_local! {
    static RECEIPTS: RefCell<Vec<(u32, usize)>> = const { RefCell::new(Vec::new()) };
  }
  unsafe extern "system" fn receive(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if matches!(message, WM_LBUTTONDOWN | WM_LBUTTONUP) {
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
    app_bundle_id: None,
    process_id: None,
    frame: Rect::new(100.0, 100.0, 320.0, 200.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: false,
    is_visible: false,
  };
  RECEIPTS.with(|receipts| receipts.borrow_mut().clear());
  for modifiers in [
    ClickModifiers {
      shift: true,
      control: true,
      ..Default::default()
    },
    ClickModifiers::default(),
  ] {
    super::click_at_window(&window, Point::new(f64::from(point.x), f64::from(point.y)), Click::Single, modifiers).unwrap();
    let mut message = MSG::default();
    // SAFETY: Dispatch only messages for the live test window on its owner thread.
    unsafe {
      while PeekMessageW(&mut message, receiver.0, 0, 0, PM_REMOVE).as_bool() {
        DispatchMessageW(&message);
      }
    }
  }
  RECEIPTS.with(|receipts| {
    assert_eq!(
      *receipts.borrow(),
      [
        (WM_LBUTTONDOWN, 0x000d),
        (WM_LBUTTONUP, 0x000c),
        (WM_LBUTTONDOWN, 0x0001),
        (WM_LBUTTONUP, 0x0000),
      ]
    )
  });
}
