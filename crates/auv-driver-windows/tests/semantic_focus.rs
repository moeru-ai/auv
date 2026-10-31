//! Live Win32 semantic focus acceptance test.
//!
//! Verifies:
//! 1. Delivery: `focus_text` resolves target via query and delivers native focus.
//! 2. Independent focus readback: both UIA identity/focused check and Win32
//!    receiver `GetFocus()` on the window thread independently confirm focus.
//! 3. Dynamic mutable property change on focus: when `WM_SETFOCUS` triggers dynamic
//!    text and geometry mutations on the control, the driver successfully confirms
//!    focus via native element identity (`CompareElements`) and `GetFocusedElement`
//!    rather than falsely failing on mutable presentation attribute shifts.
#![cfg(target_os = "windows")]

use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use auv_driver_common::accessibility::{AxTextSelector, FocusTextOptions};
use auv_driver_common::error::DriverError;
use auv_driver_common::input::InputDeliveryPath;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateSolidBrush, DeleteObject};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
  CallWindowProcW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, ES_AUTOHSCROLL, GWLP_WNDPROC, GetClientRect,
  GetMessageW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW, HMENU, MSG, MoveWindow, PostMessageW, PostQuitMessage, RegisterClassW,
  SetWindowLongPtrW, SetWindowTextW, TranslateMessage, UnregisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CLOSE, WM_DESTROY,
  WM_SETFOCUS, WNDCLASSW, WNDPROC, WS_CHILD, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE,
};
use windows::core::{PCWSTR, w};

const WM_APP_SET_INITIAL_FOCUS: u32 = WM_APP + 1;
const WM_APP_QUERY_STATE: u32 = WM_APP + 2;

static EDIT_TARGET_PROC: AtomicIsize = AtomicIsize::new(0);
static WM_SETFOCUS_COUNT: AtomicU32 = AtomicU32::new(0);

struct WindowState {
  focused_hwnd: isize,
  target_text: String,
  target_rect: [i32; 4],
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
  match msg {
    WM_APP_SET_INITIAL_FOCUS => {
      let edit_dummy = HWND(wparam.0 as *mut _);
      unsafe {
        let _ = SetFocus(edit_dummy);
      };
      return LRESULT(0);
    }
    WM_APP_QUERY_STATE => {
      let focused = unsafe { GetFocus() };
      let edit_target = HWND(wparam.0 as *mut _);
      let text = get_hwnd_text(edit_target);
      let mut rect = RECT::default();
      unsafe {
        let _ = GetWindowRect(edit_target, &mut rect);
      };
      let state = Box::new(WindowState {
        focused_hwnd: focused.0 as isize,
        target_text: text,
        target_rect: [rect.left, rect.top, rect.right, rect.bottom],
      });
      let sender = unsafe { Box::from_raw(lparam.0 as *mut mpsc::Sender<WindowState>) };
      let _ = sender.send(*state);
      return LRESULT(0);
    }
    WM_CLOSE => {
      let _ = unsafe { DestroyWindow(hwnd) };
      return LRESULT(0);
    }
    WM_DESTROY => {
      unsafe { PostQuitMessage(0) };
      return LRESULT(0);
    }
    _ => {}
  }
  unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

unsafe extern "system" fn edit_target_subclass_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
  if msg == WM_SETFOCUS {
    WM_SETFOCUS_COUNT.fetch_add(1, Ordering::SeqCst);
    // Dynamically mutate control properties upon focus receipt:
    // 1. Text / value mutation:
    unsafe {
      let _ = SetWindowTextW(hwnd, w!("Dynamically Mutated Value Upon Focus 8888"));
      // 2. Geometry / bounds mutation:
      let mut rect = RECT::default();
      let _ = GetClientRect(hwnd, &mut rect);
      let _ = MoveWindow(hwnd, 40, 120, 320, 50, true);
    }
  }
  let original: WNDPROC = unsafe { std::mem::transmute::<isize, WNDPROC>(EDIT_TARGET_PROC.load(Ordering::Relaxed)) };
  unsafe { CallWindowProcW(original, hwnd, msg, wparam, lparam) }
}

fn get_hwnd_text(hwnd: HWND) -> String {
  unsafe {
    let len = GetWindowTextLengthW(hwnd).max(0) as usize;
    let mut buf = vec![0u16; len + 1];
    let copied = GetWindowTextW(hwnd, &mut buf).max(0) as usize;
    String::from_utf16_lossy(&buf[..copied])
  }
}

struct TestApp {
  frame: HWND,
  edit_dummy: HWND,
  edit_target: HWND,
  thread_handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for TestApp {
  fn drop(&mut self) {
    unsafe {
      let _ = PostMessageW(self.frame, WM_CLOSE, WPARAM(0), LPARAM(0));
    }
    if let Some(h) = self.thread_handle.take() {
      let _ = h.join();
    }
  }
}

impl TestApp {
  fn query_state(&self) -> WindowState {
    let (tx, rx) = mpsc::channel();
    let boxed_tx = Box::into_raw(Box::new(tx));
    unsafe {
      let _ = PostMessageW(self.frame, WM_APP_QUERY_STATE, WPARAM(self.edit_target.0 as usize), LPARAM(boxed_tx as isize));
    }
    rx.recv_timeout(Duration::from_secs(3)).expect("query window thread state timed out")
  }

  #[allow(dead_code)]
  fn reset_focus_to_dummy(&self) {
    unsafe {
      let _ = PostMessageW(self.frame, WM_APP_SET_INITIAL_FOCUS, WPARAM(self.edit_dummy.0 as usize), LPARAM(0));
    }
    std::thread::sleep(Duration::from_millis(100));
  }
}

fn spawn_test_window(title: &str) -> TestApp {
  let (tx, rx) = mpsc::channel();
  let title_owned = title.to_string();

  static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(500);
  let class_id = COUNTER.fetch_add(1, Ordering::SeqCst);
  let class_str = format!("AuvSemanticFocusTestWindow_{class_id}");
  let class_name: Vec<u16> = class_str.encode_utf16().chain(std::iter::once(0)).collect();
  let class_name_clone = class_name.clone();

  let thread_handle = std::thread::spawn(move || {
    auv_driver_windows::desktop::ensure_input_desktop();
    let brush = unsafe { CreateSolidBrush(COLORREF(0x00F0_F0F0)) };
    let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }.unwrap().into();

    let class = WNDCLASSW {
      lpfnWndProc: Some(window_proc),
      lpszClassName: PCWSTR(class_name_clone.as_ptr()),
      hbrBackground: brush,
      hInstance: instance,
      ..Default::default()
    };
    unsafe { RegisterClassW(&class) };

    let title_utf16: Vec<u16> = title_owned.encode_utf16().chain(std::iter::once(0)).collect();

    let frame = unsafe {
      CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        PCWSTR(class_name_clone.as_ptr()),
        PCWSTR(title_utf16.as_ptr()),
        WS_OVERLAPPEDWINDOW | WS_VISIBLE,
        150,
        150,
        450,
        300,
        None,
        None,
        instance,
        None,
      )
    }
    .expect("create test frame window");

    unsafe {
      let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(frame, windows::Win32::UI::WindowsAndMessaging::SW_SHOW);
      let _ = windows::Win32::Graphics::Gdi::UpdateWindow(frame);
    }

    // Child 1: Dummy edit to hold initial focus
    let edit_dummy = unsafe {
      CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("EDIT"),
        w!("Initial Dummy Control"),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
        20,
        20,
        200,
        30,
        frame,
        HMENU(101 as *mut _),
        instance,
        None,
      )
    }
    .expect("create dummy edit control");

    // Child 2: Semantic focus target with initial text
    let edit_target = unsafe {
      CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("EDIT"),
        w!("Search Target Box Initial"),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
        20,
        70,
        250,
        35,
        frame,
        HMENU(102 as *mut _),
        instance,
        None,
      )
    }
    .expect("create target edit control");

    // Subclass target edit
    unsafe {
      let prev_proc = SetWindowLongPtrW(edit_target, GWLP_WNDPROC, edit_target_subclass_proc as *const () as usize as isize);
      EDIT_TARGET_PROC.store(prev_proc, Ordering::Relaxed);
      // Give initial focus to dummy
      let _ = SetFocus(edit_dummy);
    }

    let _ = tx.send((frame.0 as isize, edit_dummy.0 as isize, edit_target.0 as isize));

    let mut msg = MSG::default();
    unsafe {
      while GetMessageW(&mut msg, None, 0, 0).as_bool() {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
      }
      let _ = UnregisterClassW(PCWSTR(class_name_clone.as_ptr()), instance);
      let _ = DeleteObject(brush);
    }
  });

  let (raw_frame, raw_dummy, raw_target) = rx.recv().expect("receive hwnds from window thread");
  TestApp {
    frame: HWND(raw_frame as *mut _),
    edit_dummy: HWND(raw_dummy as *mut _),
    edit_target: HWND(raw_target as *mut _),
    thread_handle: Some(thread_handle),
  }
}

#[test]
fn win32_semantic_focus_acceptance_with_dynamic_property_changes() {
  println!("\n=== Starting Win32 Semantic Focus Real Acceptance Test ===");

  WM_SETFOCUS_COUNT.store(0, Ordering::SeqCst);
  let app_title = "AUV Win32 Semantic Focus Acceptance Window";
  let app = spawn_test_window(app_title);

  // Give the window time to be visible and registered in the desktop shell
  std::thread::sleep(Duration::from_millis(300));

  // Determine current executable name for application targeting
  let current_exe = std::env::current_exe().expect("current executable path");
  let app_name = current_exe.file_name().expect("exe file name").to_str().expect("utf8 file name");
  println!("Target executable: {app_name}");

  // Verify initial state
  let initial_state = app.query_state();
  assert_eq!(initial_state.focused_hwnd, app.edit_dummy.0 as isize, "Initial focus must reside on the dummy control, not the target");
  assert_eq!(initial_state.target_text, "Search Target Box Initial");
  assert_eq!(WM_SETFOCUS_COUNT.load(Ordering::SeqCst), 0);
  println!("1. Initial state verified: dummy focused, target text='{}'", initial_state.target_text);

  // 1. Missing query refusal test (contract check)
  println!("2. Testing missing query refusal...");
  let missing_result = auv_driver_windows::accessibility::focus_text(FocusTextOptions {
    app: app_name.into(),
    selector: AxTextSelector::Query("Nonexistent Absent Control 9999".into()),
    expected_role: None,
  });
  assert!(matches!(missing_result, Err(DriverError::NotFound { .. })), "Unmatched query must return NotFound: {missing_result:?}");
  let state_after_missing = app.query_state();
  assert_eq!(state_after_missing.focused_hwnd, app.edit_dummy.0 as isize, "Missing query must not alter keyboard focus");
  assert_eq!(WM_SETFOCUS_COUNT.load(Ordering::SeqCst), 0);
  println!("   Missing query correctly refused without delivery side-effects.");

  // 2. Role filter contract check: query matches, but role is mismatched
  println!("3. Testing role mismatch contract check...");
  let role_mismatch_result = auv_driver_windows::accessibility::focus_text(FocusTextOptions {
    app: app_name.into(),
    selector: AxTextSelector::Query("Search Target Box Initial".into()),
    expected_role: Some("Button".into()),
  });
  assert!(
    matches!(role_mismatch_result, Err(DriverError::NotFound { .. })),
    "Role mismatch must refuse match without fallback: {role_mismatch_result:?}"
  );
  let state_after_mismatch = app.query_state();
  assert_eq!(state_after_mismatch.focused_hwnd, app.edit_dummy.0 as isize, "Role mismatch must not alter keyboard focus");
  assert_eq!(WM_SETFOCUS_COUNT.load(Ordering::SeqCst), 0);
  println!("   Role filter mismatch correctly refused without delivery side-effects.");

  // 3. Real semantic focus delivery with dynamic property mutations
  println!("4. Delivering semantic focus to target control with dynamic property mutation...");
  let focus_result = auv_driver_windows::accessibility::focus_text(FocusTextOptions {
    app: app_name.into(),
    selector: AxTextSelector::Query("Search Target Box Initial".into()),
    expected_role: Some("Edit".into()),
  })
  .expect("semantic focus delivery and readback must succeed");

  println!("   focus_text returned: path={}, role={}", focus_result.path, focus_result.role);

  // Delivery assertion
  assert_eq!(focus_result.role, "Edit");
  assert!(focus_result.input_action_result.verified, "Input action must be verified");
  assert_eq!(focus_result.input_action_result.selected_path, InputDeliveryPath::AxFocus, "Delivery path must be AxFocus");

  // Independent Win32 focus verification on receiver thread
  let state_after_focus = app.query_state();
  assert_eq!(
    state_after_focus.focused_hwnd, app.edit_target.0 as isize,
    "Win32 GetFocus() on window thread must independently confirm target control is focused"
  );
  assert!(WM_SETFOCUS_COUNT.load(Ordering::SeqCst) >= 1, "Target control must have received at least one WM_SETFOCUS");

  // Dynamic property change verification
  assert_eq!(
    state_after_focus.target_text, "Dynamically Mutated Value Upon Focus 8888",
    "Target control text must have mutated dynamically upon WM_SETFOCUS"
  );
  assert_ne!(
    state_after_focus.target_rect, initial_state.target_rect,
    "Target control bounds must have mutated dynamically upon WM_SETFOCUS"
  );

  println!("5. Independent focus readback verified:");
  println!("   - Target received WM_SETFOCUS: count = {}", WM_SETFOCUS_COUNT.load(Ordering::SeqCst));
  println!("   - Win32 GetFocus() HWND = 0x{:x} (target HWND = 0x{:x})", state_after_focus.focused_hwnd, app.edit_target.0 as usize);
  println!("   - Post-focus mutated text = '{}'", state_after_focus.target_text);
  println!("   - Post-focus mutated rect = {:?}", state_after_focus.target_rect);
  println!("   - Driver AxFocusResult verified = {}", focus_result.input_action_result.verified);

  // 6. Idempotent re-focus when target is already focused
  println!("6. Testing re-focusing already focused target with mutated query...");
  let refocus_result = auv_driver_windows::accessibility::focus_text(FocusTextOptions {
    app: app_name.into(),
    selector: AxTextSelector::Query("Dynamically Mutated Value Upon Focus 8888".into()),
    expected_role: Some("Edit".into()),
  })
  .expect("re-focusing already focused control must succeed");
  assert!(refocus_result.input_action_result.verified);
  let state_after_refocus = app.query_state();
  assert_eq!(state_after_refocus.focused_hwnd, app.edit_target.0 as isize);
  println!("   Re-focus succeeded and verified: target remained focused.");

  println!("=== Acceptance Test PASSED successfully ===");
}
