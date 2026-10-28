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

#[test]
fn background_delivery_quantization_demonstrates_out_of_bounds_at_perimeter_rounding() {
  use super::native::round_to_i32;

  // Window frame: origin (50.0, 150.0), size (400.0, 300.0).
  // Valid discrete screen pixel spans are [50..450) and [150..450).
  // Inside the window, discrete client coordinates must be in [0..400) and [0..300).
  let origin_x = 50.0;
  let origin_y = 150.0;
  let width = 400.0;
  let height = 300.0;

  // 1. Right boundary: point at 449.9 rounds up to 450, landing ON the right boundary (outside [50..450))
  let screen_x_edge = 449.9;
  let client_x_edge = round_to_i32(screen_x_edge) - round_to_i32(origin_x);
  assert_eq!(client_x_edge, 400);
  assert!(client_x_edge >= width as i32, "client x must be >= width (outside client area)");

  // In contrast, 449.4 rounds down to 449, remaining inside client area
  let screen_x_safe = 449.4;
  let client_x_safe = round_to_i32(screen_x_safe) - round_to_i32(origin_x);
  assert_eq!(client_x_safe, 399);
  assert!(client_x_safe < width as i32, "client x must be < width (inside client area)");

  // 2. Fractional window origin: origin (50.4, 150.4), right edge is 450.4
  let frac_origin_x = 50.4;
  let frac_screen_x = 449.8;
  let frac_client_x = round_to_i32(frac_screen_x) - round_to_i32(frac_origin_x);
  assert_eq!(frac_client_x, 400);
  assert!(frac_client_x >= width as i32);

  // 3. Negative coordinates on secondary monitor: origin (-1500.0, -1000.0), size (1000.0, 800.0)
  let neg_origin_x = -1500.0;
  let neg_screen_x = -500.2; // right edge is -500.0
  let neg_client_x = round_to_i32(neg_screen_x) - round_to_i32(neg_origin_x);
  assert_eq!(neg_client_x, 1000); // -500 - (-1500) = 1000 >= 1000, outside!

  // 4. Bottom boundary: y = 449.9 rounds to 450
  let screen_y_edge = 449.9;
  let client_y_edge = round_to_i32(screen_y_edge) - round_to_i32(origin_y);
  assert_eq!(client_y_edge, 300);
  assert!(client_y_edge >= height as i32, "client y must be >= height (outside client area)");
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
