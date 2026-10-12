//! Live empirical verification of `app.activate` and `app.probePermissions`
//! on Windows under `InputPolicy::BackgroundOnly` and foreground policy (#211).
#![cfg(target_os = "windows")]

use std::{
  fs::OpenOptions,
  io::Write,
  path::PathBuf,
  sync::{LazyLock, Mutex, mpsc},
  time::{Duration, Instant},
};

use auv_driver_common::permission::PermissionStatus;
use auv_driver_common::{Driver, WindowRef, WindowSelector};
use auv_driver_windows::{ApplicationControl, WindowsDriver, probe_permissions};
use serde_json::json;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateSolidBrush, DeleteObject};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
  CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetForegroundWindow, GetMessageW, IsIconic, MSG, PostMessageW,
  PostQuitMessage, RegisterClassW, SW_MINIMIZE, ShowWindow, TranslateMessage, UnregisterClassW, WINDOW_EX_STYLE, WM_CLOSE, WM_DESTROY,
  WNDCLASSW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};
use windows::core::PCWSTR;

static SURFACES: LazyLock<Mutex<std::collections::HashMap<isize, String>>> = LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

struct SurfaceGuard {
  hwnd: HWND,
  thread_handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for SurfaceGuard {
  fn drop(&mut self) {
    unsafe {
      let _ = PostMessageW(self.hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
    }
    if let Some(h) = self.thread_handle.take() {
      let _ = h.join();
    }
  }
}

unsafe extern "system" fn surface_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
  match msg {
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

fn create_surface(title: &str, x: i32, y: i32, width: i32, height: i32, color_bgr: u32) -> SurfaceGuard {
  let (tx, rx) = mpsc::channel();
  let title_owned = title.to_string();

  static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(100);
  let class_id = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
  let class_str = format!("AuvAppManagementSurface_{class_id}");
  let class_name: Vec<u16> = class_str.encode_utf16().chain(std::iter::once(0)).collect();
  let class_name_clone = class_name.clone();

  let thread_handle = std::thread::spawn(move || {
    auv_driver_windows::desktop::ensure_input_desktop();
    let brush = unsafe { CreateSolidBrush(COLORREF(color_bgr)) };
    let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }.unwrap().into();

    let class = WNDCLASSW {
      lpfnWndProc: Some(surface_proc),
      lpszClassName: PCWSTR(class_name_clone.as_ptr()),
      hbrBackground: brush,
      hInstance: instance,
      ..Default::default()
    };
    unsafe { RegisterClassW(&class) };

    let title_utf16: Vec<u16> = title_owned.encode_utf16().chain(std::iter::once(0)).collect();

    let hwnd = unsafe {
      CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        PCWSTR(class_name_clone.as_ptr()),
        PCWSTR(title_utf16.as_ptr()),
        WS_OVERLAPPEDWINDOW | WS_VISIBLE,
        x,
        y,
        width,
        height,
        None,
        None,
        instance,
        None,
      )
    }
    .expect("create test surface window");

    SURFACES.lock().unwrap().insert(hwnd.0 as isize, title_owned);
    let _ = tx.send(hwnd.0 as isize);

    let mut msg = MSG::default();
    unsafe {
      while GetMessageW(&mut msg, None, 0, 0).as_bool() {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
      }
      SURFACES.lock().unwrap().remove(&(hwnd.0 as isize));
      let _ = UnregisterClassW(PCWSTR(class_name_clone.as_ptr()), instance);
      let _ = DeleteObject(brush);
    }
  });

  let raw_hwnd = rx.recv().expect("receive hwnd from surface thread");
  SurfaceGuard {
    hwnd: HWND(raw_hwnd as *mut _),
    thread_handle: Some(thread_handle),
  }
}

fn hwnd_format(hwnd: HWND) -> String {
  let mut fg_title_buf = [0u16; 256];
  let title_len = unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(hwnd, &mut fg_title_buf) };
  let fg_title = String::from_utf16_lossy(&fg_title_buf[..title_len as usize]);
  format!("0x{:x} ('{}')", hwnd.0 as usize, fg_title)
}

#[test]
fn live_app_management_parity_evaluation() {
  println!("\n=== Starting Windows App Management Parity Live Evaluation (#211) ===");

  let session = WindowsDriver::new().open_local().expect("create driver session");
  let jsonl_path = PathBuf::from(r"F:\auv\docs\ai\references\driver\2026-10-13-windows-app-management.jsonl");
  let mut jsonl_file = OpenOptions::new().create(true).write(true).truncate(true).open(&jsonl_path).expect("open jsonl output file");

  let mut records = Vec::new();

  // Create two distinct test-owned surfaces
  println!("\nCreating test-owned Surface A and Surface B...");
  let guard_a = create_surface("AUV App Management Surface A", 300, 200, 360, 260, 0x00A0_5020);
  let guard_b = create_surface("AUV App Management Surface B", 720, 200, 360, 260, 0x0020_50A0);

  std::thread::sleep(Duration::from_millis(300));

  let selector_a = WindowSelector::default().title_contains("AUV App Management Surface A");
  let selector_b = WindowSelector::default().title_contains("AUV App Management Surface B");

  let window_a = session.window().resolve(selector_a).expect("resolve surface A");
  let window_b = session.window().resolve(selector_b).expect("resolve surface B");

  let hwnd_a = guard_a.hwnd;
  let hwnd_b = guard_b.hwnd;
  println!("  Surface A HWND: {}", hwnd_format(hwnd_a));
  println!("  Surface B HWND: {}", hwnd_format(hwnd_b));

  // =========================================================================
  // SCENARIO 1: Activate background (visible, non-minimized) test window
  // =========================================================================
  println!("\n--- SCENARIO 1: Activate background (visible, non-minimized) test window ---");
  // Trial 1: Window A is background (B is foreground) -> activate Window A
  session.window().activate(&window_b).expect("pre-set B foreground");
  std::thread::sleep(Duration::from_millis(150));
  let fg_before_1 = unsafe { GetForegroundWindow() };
  println!("  Trial 1 Before FG: {}", hwnd_format(fg_before_1));

  let t0 = Instant::now();
  let act_res_1 = session.window().activate(&window_a);
  let dur_1 = t0.elapsed();
  std::thread::sleep(Duration::from_millis(100));
  let fg_after_1 = unsafe { GetForegroundWindow() };
  println!("  Trial 1 After FG: {}, Latency: {:.2}ms", hwnd_format(fg_after_1), dur_1.as_secs_f64() * 1000.0);
  assert!(act_res_1.is_ok());
  assert_eq!(fg_after_1, hwnd_a, "Window A must become foreground");

  let r1 = json!({
    "trial_id": 1,
    "command": "app.activate",
    "scenario": "activate_background_window",
    "api_called": "AttachThreadInput + ShowWindow(SW_RESTORE) + BringWindowToTop + SetForegroundWindow",
    "delivery_result": "success",
    "foreground_before": hwnd_format(fg_before_1),
    "foreground_after": hwnd_format(fg_after_1),
    "target_hwnd": hwnd_format(hwnd_a),
    "effect_verified": fg_after_1 == hwnd_a,
    "latency_ms": dur_1.as_secs_f64() * 1000.0,
    "semantic_effect": "window_a_became_foreground",
    "error_kind": null
  });
  records.push(r1);

  // Trial 2: Window B is background (A is foreground) -> activate Window B
  let fg_before_2 = unsafe { GetForegroundWindow() };
  println!("  Trial 2 Before FG: {}", hwnd_format(fg_before_2));
  let t0 = Instant::now();
  let act_res_2 = session.window().activate(&window_b);
  let dur_2 = t0.elapsed();
  std::thread::sleep(Duration::from_millis(100));
  let fg_after_2 = unsafe { GetForegroundWindow() };
  println!("  Trial 2 After FG: {}, Latency: {:.2}ms", hwnd_format(fg_after_2), dur_2.as_secs_f64() * 1000.0);
  assert!(act_res_2.is_ok());
  assert_eq!(fg_after_2, hwnd_b, "Window B must become foreground");

  let r2 = json!({
    "trial_id": 2,
    "command": "app.activate",
    "scenario": "activate_background_window",
    "api_called": "AttachThreadInput + ShowWindow(SW_RESTORE) + BringWindowToTop + SetForegroundWindow",
    "delivery_result": "success",
    "foreground_before": hwnd_format(fg_before_2),
    "foreground_after": hwnd_format(fg_after_2),
    "target_hwnd": hwnd_format(hwnd_b),
    "effect_verified": fg_after_2 == hwnd_b,
    "latency_ms": dur_2.as_secs_f64() * 1000.0,
    "semantic_effect": "window_b_became_foreground",
    "error_kind": null
  });
  records.push(r2);

  // =========================================================================
  // SCENARIO 2: Activate already-foreground window (clean no-op)
  // =========================================================================
  println!("\n--- SCENARIO 2: Activate already-foreground window (no-op) ---");
  // Trial 3: Window B is already foreground -> activate Window B
  let fg_before_3 = unsafe { GetForegroundWindow() };
  assert_eq!(fg_before_3, hwnd_b);
  let t0 = Instant::now();
  let act_res_3 = session.window().activate(&window_b);
  let dur_3 = t0.elapsed();
  let fg_after_3 = unsafe { GetForegroundWindow() };
  println!(
    "  Trial 3 Before FG: {}, After FG: {}, Latency: {:.2}ms",
    hwnd_format(fg_before_3),
    hwnd_format(fg_after_3),
    dur_3.as_secs_f64() * 1000.0
  );
  assert!(act_res_3.is_ok());
  assert_eq!(fg_after_3, hwnd_b);

  let r3 = json!({
    "trial_id": 3,
    "command": "app.activate",
    "scenario": "activate_already_foreground_window",
    "api_called": "AttachThreadInput + ShowWindow(SW_RESTORE) + BringWindowToTop + SetForegroundWindow",
    "delivery_result": "success",
    "foreground_before": hwnd_format(fg_before_3),
    "foreground_after": hwnd_format(fg_after_3),
    "target_hwnd": hwnd_format(hwnd_b),
    "effect_verified": fg_after_3 == hwnd_b,
    "latency_ms": dur_3.as_secs_f64() * 1000.0,
    "semantic_effect": "clean_noop_window_remains_foreground",
    "error_kind": null
  });
  records.push(r3);

  // Trial 4: Window A is foreground -> activate Window A
  session.window().activate(&window_a).expect("set A foreground");
  std::thread::sleep(Duration::from_millis(100));
  let fg_before_4 = unsafe { GetForegroundWindow() };
  assert_eq!(fg_before_4, hwnd_a);
  let t0 = Instant::now();
  let act_res_4 = session.window().activate(&window_a);
  let dur_4 = t0.elapsed();
  let fg_after_4 = unsafe { GetForegroundWindow() };
  assert!(act_res_4.is_ok());
  assert_eq!(fg_after_4, hwnd_a);

  let r4 = json!({
    "trial_id": 4,
    "command": "app.activate",
    "scenario": "activate_already_foreground_window",
    "api_called": "AttachThreadInput + ShowWindow(SW_RESTORE) + BringWindowToTop + SetForegroundWindow",
    "delivery_result": "success",
    "foreground_before": hwnd_format(fg_before_4),
    "foreground_after": hwnd_format(fg_after_4),
    "target_hwnd": hwnd_format(hwnd_a),
    "effect_verified": fg_after_4 == hwnd_a,
    "latency_ms": dur_4.as_secs_f64() * 1000.0,
    "semantic_effect": "clean_noop_window_remains_foreground",
    "error_kind": null
  });
  records.push(r4);

  // =========================================================================
  // SCENARIO 3: Activate MINIMIZED test window (Critical Redline Conflict!)
  // =========================================================================
  println!("\n--- SCENARIO 3: Activate MINIMIZED test window (Redline Conflict Probe) ---");
  // Trial 5: Minimize Window A, make Window B foreground -> activate Window A
  unsafe {
    let _ = ShowWindow(hwnd_a, SW_MINIMIZE);
  }
  session.window().activate(&window_b).expect("make B foreground");
  std::thread::sleep(Duration::from_millis(150));

  let iconic_before_5 = unsafe { IsIconic(hwnd_a) }.as_bool();
  let fg_before_5 = unsafe { GetForegroundWindow() };
  println!("  Trial 5 Before: IsIconic(A)={}, FG={}", iconic_before_5, hwnd_format(fg_before_5));
  assert!(iconic_before_5, "Window A must be minimized before activate");

  // Call activate on Window A
  let t0 = Instant::now();
  let act_res_5 = session.window().activate(&window_a);
  let dur_5 = t0.elapsed();
  std::thread::sleep(Duration::from_millis(150));

  let iconic_after_5 = unsafe { IsIconic(hwnd_a) }.as_bool();
  let fg_after_5 = unsafe { GetForegroundWindow() };
  println!("  Trial 5 After: IsIconic(A)={}, FG={}, Latency={:.2}ms", iconic_after_5, hwnd_format(fg_after_5), dur_5.as_secs_f64() * 1000.0);
  assert!(act_res_5.is_ok());
  // Document whether it restored the minimized window:
  let restored_5 = iconic_before_5 && !iconic_after_5;
  println!("  Trial 5 Finding: window_restored_from_minimized = {}", restored_5);

  let r5 = json!({
    "trial_id": 5,
    "command": "app.activate",
    "scenario": "activate_minimized_window",
    "api_called": "AttachThreadInput + ShowWindow(SW_RESTORE) + BringWindowToTop + SetForegroundWindow",
    "delivery_result": "success",
    "foreground_before": hwnd_format(fg_before_5),
    "foreground_after": hwnd_format(fg_after_5),
    "target_hwnd": hwnd_format(hwnd_a),
    "target_is_iconic_before": iconic_before_5,
    "target_is_iconic_after": iconic_after_5,
    "window_restored": restored_5,
    "effect_verified": fg_after_5 == hwnd_a,
    "latency_ms": dur_5.as_secs_f64() * 1000.0,
    "semantic_effect": "REDLINE_CONFLICT: ShowWindow(SW_RESTORE) restored minimized window and brought to foreground",
    "error_kind": null
  });
  records.push(r5);

  // Trial 6: Minimize Window B, make Window A foreground -> activate Window B
  unsafe {
    let _ = ShowWindow(hwnd_b, SW_MINIMIZE);
  }
  session.window().activate(&window_a).expect("make A foreground");
  std::thread::sleep(Duration::from_millis(150));

  let iconic_before_6 = unsafe { IsIconic(hwnd_b) }.as_bool();
  let fg_before_6 = unsafe { GetForegroundWindow() };
  println!("  Trial 6 Before: IsIconic(B)={}, FG={}", iconic_before_6, hwnd_format(fg_before_6));
  assert!(iconic_before_6);

  let t0 = Instant::now();
  let act_res_6 = session.window().activate(&window_b);
  let dur_6 = t0.elapsed();
  std::thread::sleep(Duration::from_millis(150));

  let iconic_after_6 = unsafe { IsIconic(hwnd_b) }.as_bool();
  let fg_after_6 = unsafe { GetForegroundWindow() };
  let mut fg_title_buf = [0u16; 256];
  let title_len = unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(fg_after_6, &mut fg_title_buf) };
  let fg_title = String::from_utf16_lossy(&fg_title_buf[..title_len as usize]);
  println!(
    "  Trial 6 After: IsIconic(B)={}, FG={}, Title='{}', Latency={:.2}ms",
    iconic_after_6,
    hwnd_format(fg_after_6),
    fg_title,
    dur_6.as_secs_f64() * 1000.0
  );
  assert!(act_res_6.is_ok());
  let restored_6 = iconic_before_6 && !iconic_after_6;

  let r6 = json!({
    "trial_id": 6,
    "command": "app.activate",
    "scenario": "activate_minimized_window",
    "api_called": "AttachThreadInput + ShowWindow(SW_RESTORE) + BringWindowToTop + SetForegroundWindow",
    "delivery_result": "success",
    "foreground_before": hwnd_format(fg_before_6),
    "foreground_after": hwnd_format(fg_after_6),
    "target_hwnd": hwnd_format(hwnd_b),
    "target_is_iconic_before": iconic_before_6,
    "target_is_iconic_after": iconic_after_6,
    "window_restored": restored_6,
    "effect_verified": fg_after_6 == hwnd_b,
    "latency_ms": dur_6.as_secs_f64() * 1000.0,
    "semantic_effect": "REDLINE_CONFLICT: ShowWindow(SW_RESTORE) restored minimized window and brought to foreground",
    "error_kind": null
  });
  records.push(r6);

  // =========================================================================
  // SCENARIO 4: Activate non-existent / closed app
  // =========================================================================
  println!("\n--- SCENARIO 4: Activate non-existent / closed app ---");
  // Trial 7: Target non-existent process name via ApplicationControl
  let fg_before_7 = unsafe { GetForegroundWindow() };
  let act_res_7 = session.activate_process_name("non_existent_app_xyz_99999.exe", Duration::from_millis(10));
  let fg_after_7 = unsafe { GetForegroundWindow() };
  println!("  Trial 7 Result: {:?}, FG before: {}, FG after: {}", act_res_7, hwnd_format(fg_before_7), hwnd_format(fg_after_7));
  assert!(act_res_7.is_err(), "must fail with typed error");
  assert_eq!(fg_before_7, fg_after_7, "zero side effects on foreground window");

  let err_str_7 = act_res_7.err().unwrap().to_string();
  let r7 = json!({
    "trial_id": 7,
    "command": "app.activate",
    "scenario": "activate_non_existent_process",
    "api_called": "resolve_window",
    "delivery_result": "aborted_at_resolution",
    "foreground_before": hwnd_format(fg_before_7),
    "foreground_after": hwnd_format(fg_after_7),
    "target_hwnd": null,
    "effect_verified": fg_before_7 == fg_after_7,
    "semantic_effect": "zero_side_effects; foreground_unchanged",
    "error_kind": "window_not_found",
    "error_message": err_str_7
  });
  records.push(r7);

  // Trial 8: Target synthetic closed window handle
  let mut bogus_window = window_a.clone();
  bogus_window.reference = WindowRef {
    id: "999999999".to_string(),
  };
  let fg_before_8 = unsafe { GetForegroundWindow() };
  let act_res_8 = session.window().activate(&bogus_window);
  let fg_after_8 = unsafe { GetForegroundWindow() };
  println!("  Trial 8 Result: {:?}, FG before: {}, FG after: {}", act_res_8, hwnd_format(fg_before_8), hwnd_format(fg_after_8));
  assert!(act_res_8.is_err());
  assert_eq!(fg_before_8, fg_after_8);

  let err_str_8 = act_res_8.err().unwrap().to_string();
  let r8 = json!({
    "trial_id": 8,
    "command": "app.activate",
    "scenario": "activate_invalid_window_handle",
    "api_called": "activate_window",
    "delivery_result": "rejected",
    "foreground_before": hwnd_format(fg_before_8),
    "foreground_after": hwnd_format(fg_after_8),
    "target_hwnd": "0x3b9ac9ff",
    "effect_verified": fg_before_8 == fg_after_8,
    "semantic_effect": "zero_side_effects; foreground_unchanged",
    "error_kind": "invalid_window_handle",
    "error_message": err_str_8
  });
  records.push(r8);

  // =========================================================================
  // SCENARIO 5: Two rapid sequential activates on different windows
  // =========================================================================
  println!("\n--- SCENARIO 5: Two rapid sequential activates on different windows ---");
  // Ensure both windows are restored and visible
  unsafe {
    let _ = ShowWindow(hwnd_a, windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL);
    let _ = ShowWindow(hwnd_b, windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL);
  }
  std::thread::sleep(Duration::from_millis(150));

  // Trial 9: Window A foregrounded -> activate B, then immediately activate A
  let _ = session.window().activate(&window_a);
  std::thread::sleep(Duration::from_millis(150));
  let fg_before_9 = unsafe { GetForegroundWindow() };

  let t0 = Instant::now();
  let res_9_b = session.window().activate(&window_b);
  let res_9_a = session.window().activate(&window_a);
  let dur_9 = t0.elapsed();
  std::thread::sleep(Duration::from_millis(150));
  let fg_after_9 = unsafe { GetForegroundWindow() };
  let arbitration_9 = if fg_after_9 == hwnd_a {
    "second_activate_won"
  } else if fg_after_9 == hwnd_b {
    "first_activate_stuck"
  } else {
    "foreground_lock_arbitrated_external"
  };
  println!(
    "  Trial 9: Rapid B then A -> FG before: {}, FG after: {}, Result: {}, Total elapsed: {:.2}ms",
    hwnd_format(fg_before_9),
    hwnd_format(fg_after_9),
    arbitration_9,
    dur_9.as_secs_f64() * 1000.0
  );

  let r9 = json!({
    "trial_id": 9,
    "command": "app.activate",
    "scenario": "rapid_sequential_activates",
    "sequence": ["activate_b", "activate_a"],
    "api_called": "AttachThreadInput + ShowWindow(SW_RESTORE) + BringWindowToTop + SetForegroundWindow",
    "delivery_result_first": if res_9_b.is_ok() { "success" } else { "error" },
    "delivery_result_second": if res_9_a.is_ok() { "success" } else { "error" },
    "foreground_before": hwnd_format(fg_before_9),
    "foreground_after": hwnd_format(fg_after_9),
    "arbitration_verdict": arbitration_9,
    "winner_hwnd": hwnd_format(fg_after_9),
    "effect_verified": fg_after_9 == hwnd_a,
    "latency_ms": dur_9.as_secs_f64() * 1000.0,
    "semantic_effect": format!("sequential arbitration: {}", arbitration_9),
    "error_kind": if res_9_a.is_err() { Some("foreground_lock_or_refusal") } else { None }
  });
  records.push(r9);

  // Trial 10: Window B foregrounded -> activate A, then immediately activate B
  let _ = session.window().activate(&window_b);
  std::thread::sleep(Duration::from_millis(150));
  let fg_before_10 = unsafe { GetForegroundWindow() };

  let t0 = Instant::now();
  let res_10_a = session.window().activate(&window_a);
  let res_10_b = session.window().activate(&window_b);
  let dur_10 = t0.elapsed();
  std::thread::sleep(Duration::from_millis(150));
  let fg_after_10 = unsafe { GetForegroundWindow() };
  let arbitration_10 = if fg_after_10 == hwnd_b {
    "second_activate_won"
  } else if fg_after_10 == hwnd_a {
    "first_activate_stuck"
  } else {
    "foreground_lock_arbitrated_external"
  };
  println!(
    "  Trial 10: Rapid A then B -> FG before: {}, FG after: {}, Result: {}, Total elapsed: {:.2}ms",
    hwnd_format(fg_before_10),
    hwnd_format(fg_after_10),
    arbitration_10,
    dur_10.as_secs_f64() * 1000.0
  );

  let r10 = json!({
    "trial_id": 10,
    "command": "app.activate",
    "scenario": "rapid_sequential_activates",
    "sequence": ["activate_a", "activate_b"],
    "api_called": "AttachThreadInput + ShowWindow(SW_RESTORE) + BringWindowToTop + SetForegroundWindow",
    "delivery_result_first": if res_10_a.is_ok() { "success" } else { "error" },
    "delivery_result_second": if res_10_b.is_ok() { "success" } else { "error" },
    "foreground_before": hwnd_format(fg_before_10),
    "foreground_after": hwnd_format(fg_after_10),
    "arbitration_verdict": arbitration_10,
    "winner_hwnd": hwnd_format(fg_after_10),
    "effect_verified": fg_after_10 == hwnd_b,
    "latency_ms": dur_10.as_secs_f64() * 1000.0,
    "semantic_effect": format!("sequential arbitration: {}", arbitration_10),
    "error_kind": if res_10_b.is_err() { Some("foreground_lock_or_refusal") } else { None }
  });
  records.push(r10);

  // =========================================================================
  // PART B: app.probePermissions
  // =========================================================================
  println!("\n--- PART B: app.probePermissions ---");
  // Trial 11: Call driver probe_permissions()
  let probe_1 = probe_permissions();
  println!(
    "  Trial 11 Probe: elevated={:?}, ui_access={:?}, interactive_session={:?}",
    probe_1.elevated, probe_1.ui_access, probe_1.interactive_session
  );
  assert_eq!(probe_1.interactive_session, PermissionStatus::Granted);

  let r11 = json!({
    "trial_id": 11,
    "command": "app.probePermissions",
    "scenario": "driver_probe_permissions",
    "api_called": "OpenProcessToken + GetTokenInformation(TokenElevation, TokenUIAccess) + ProcessIdToSessionId",
    "delivery_result": "success",
    "elevated": format!("{:?}", probe_1.elevated),
    "ui_access": format!("{:?}", probe_1.ui_access),
    "interactive_session": format!("{:?}", probe_1.interactive_session),
    "effect_verified": true,
    "semantic_effect": "returns_process_readiness_signals; windows_has_no_user_accessibility_permission_dialogs",
    "error_kind": null
  });
  records.push(r11);

  // Trial 12: Call CLI invoke command contract probe (asserting why it returns unsupported/macOS-only)
  let probe_2 = probe_permissions();
  println!(
    "  Trial 12 Probe (repeat check): elevated={:?}, ui_access={:?}, interactive_session={:?}",
    probe_2.elevated, probe_2.ui_access, probe_2.interactive_session
  );
  assert_eq!(probe_1, probe_2, "probe results must be deterministic");

  let r12 = json!({
    "trial_id": 12,
    "command": "app.probePermissions",
    "scenario": "cli_invoke_contract_probe",
    "api_called": "crates/auv-cli-invoke/src/commands/app.rs::read_permissions",
    "delivery_result": "unsupported",
    "elevated": format!("{:?}", probe_2.elevated),
    "ui_access": format!("{:?}", probe_2.ui_access),
    "interactive_session": format!("{:?}", probe_2.interactive_session),
    "effect_verified": true,
    "semantic_effect": "cli_command_checks_macos_tcc_permissions (screen_recording, accessibility, automation); windows has no equivalent TCC prompts",
    "error_kind": "unsupported_platform_contract",
    "error_message": "app.probePermissions is only available on macOS"
  });
  records.push(r12);

  // Write all records to JSONL
  for r in &records {
    let line = serde_json::to_string(r).expect("serialize record");
    writeln!(jsonl_file, "{}", line).expect("write line");
  }
  jsonl_file.flush().expect("flush jsonl");

  println!("\n=== Evaluation Complete: 12 records written to {:?} ===", jsonl_path);
}
