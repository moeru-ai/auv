//! Acceptance tests for Windows.Graphics.Capture (WGC) backend.
#![cfg(target_os = "windows")]

use auv_driver_common::geometry::Rect;
use auv_driver_common::window::{Window, WindowRef};
use auv_driver_common::{CoordinateSpace, Driver};
use auv_driver_windows::{
  WindowsDriver, capture_window_health_cached, capture_window_health_strict, check_window_liveness, prewarm_wgc_window,
};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateSolidBrush, DeleteObject, HBRUSH};
use windows::Win32::UI::WindowsAndMessaging::{
  CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, RegisterClassW, SW_SHOW, SWP_FRAMECHANGED,
  SWP_NOMOVE, SWP_NOZORDER, SetForegroundWindow, SetWindowPos, ShowWindow, UnregisterClassW, WINDOW_EX_STYLE, WNDCLASSW,
  WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};
use windows::core::w;

struct TestWindow {
  hwnd: HWND,
  class_name: windows::core::PCWSTR,
  brush: HBRUSH,
}

impl Drop for TestWindow {
  fn drop(&mut self) {
    unsafe {
      let _ = DestroyWindow(self.hwnd);
      let _ = UnregisterClassW(self.class_name, None);
      let _ = DeleteObject(self.brush);
    }
  }
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
  unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn pump_messages(duration: Duration) {
  let deadline = Instant::now() + duration;
  loop {
    let mut msg = MSG::default();
    unsafe {
      while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
        DispatchMessageW(&msg);
      }
    }
    if Instant::now() >= deadline {
      break;
    }
    std::thread::sleep(Duration::from_millis(5));
  }
}

fn create_test_window(
  title: &'static str,
  class_name: windows::core::PCWSTR,
  x: i32,
  y: i32,
  width: i32,
  height: i32,
  color_bgr: u32,
) -> (TestWindow, Window) {
  auv_driver_windows::desktop::ensure_input_desktop();
  let brush = unsafe { CreateSolidBrush(COLORREF(color_bgr)) };

  let class = WNDCLASSW {
    lpfnWndProc: Some(window_proc),
    lpszClassName: class_name,
    hbrBackground: brush,
    ..Default::default()
  };

  let hwnd = unsafe {
    let _ = RegisterClassW(&class);
    let title_utf16: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    CreateWindowExW(
      WINDOW_EX_STYLE::default(),
      class.lpszClassName,
      windows::core::PCWSTR(title_utf16.as_ptr()),
      WS_OVERLAPPEDWINDOW | WS_VISIBLE,
      x,
      y,
      width,
      height,
      None,
      None,
      None,
      None,
    )
    .expect("failed to create test window")
  };

  unsafe {
    let _ = ShowWindow(hwnd, SW_SHOW);
    let _ = SetForegroundWindow(hwnd);
  }

  pump_messages(Duration::from_millis(300));

  let driver_window = Window {
    reference: WindowRef {
      id: (hwnd.0 as isize).to_string(),
    },
    title: Some(title.to_string()),
    app_name: Some("test_runner.exe".to_string()),
    app_bundle_id: None,
    process_id: Some(std::process::id()),
    frame: Rect::new(x as f64, y as f64, width as f64, height as f64),
    coordinate_space: CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
  };

  (
    TestWindow {
      hwnd,
      class_name,
      brush,
    },
    driver_window,
  )
}

// Live smoke tests against real Win32/D3D11 capture device.
// The D3D11 device and WGC capture session are process-wide resources,
// so live acceptance tests must serialize to prevent cross-test cache invalidation.
static WGC_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn test_wgc_pixel_correctness_and_gdi_comparison() {
  let _lock = WGC_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
  let class_name = w!("AuvWgcPixelCorrectnessTest");
  // Blue=0xFF, Green=0x80, Red=0x40 -> in Win32 COLORREF (0x00BBGGRR): 0x00FF8040
  let (_test_win, driver_win) = create_test_window("AUV WGC Correctness Test", class_name, 200, 200, 480, 360, 0x00FF_8040);

  let session = WindowsDriver::default().open_local().expect("failed to open driver session");

  // 1. Capture via WGC
  let wgc_cap = session.window().capture_wgc(&driver_win).expect("WGC capture failed");
  assert_eq!(wgc_cap.backend, "wgc.windows");
  assert!(wgc_cap.image.width() >= 400);
  assert!(wgc_cap.image.height() >= 300);

  // 2. Capture via GDI PrintWindow
  let gdi_cap = session.window().capture(&driver_win).expect("GDI PrintWindow capture failed");
  assert_eq!(gdi_cap.backend, "printwindow.windows");

  // 3. Compare inner client pixels to expected color (R=64, G=128, B=255)
  // Sample a 100x100 region around the center of the client area
  let center_x = wgc_cap.image.width() / 2;
  let center_y = wgc_cap.image.height() / 2;
  let mut matching_pixels = 0;
  let mut total_sampled = 0;

  for dy in 0..100 {
    for dx in 0..100 {
      let px = center_x - 50 + dx;
      let py = center_y - 50 + dy;
      let pixel = wgc_cap.image.get_pixel(px, py);
      // Expected R=0x40(64), G=0x80(128), B=0xFF(255)
      let dr = (pixel[0] as i32 - 64).abs();
      let dg = (pixel[1] as i32 - 128).abs();
      let db = (pixel[2] as i32 - 255).abs();
      if dr <= 2 && dg <= 2 && db <= 2 {
        matching_pixels += 1;
      }
      total_sampled += 1;
    }
  }

  let match_ratio = matching_pixels as f64 / total_sampled as f64;
  println!("WGC pixel match ratio against target color: {:.2}% ({}/{})", match_ratio * 100.0, matching_pixels, total_sampled);
  assert!(match_ratio >= 0.99, "Expected >=99% matching pixels in client region, got {:.2}%", match_ratio * 100.0);

  // 4. Compare WGC vs GDI PrintWindow on the same center region
  let mut diff_pixels = 0;
  for dy in 0..100 {
    for dx in 0..100 {
      let px = center_x - 50 + dx;
      let py = center_y - 50 + dy;
      if px < gdi_cap.image.width() && py < gdi_cap.image.height() {
        let p_wgc = wgc_cap.image.get_pixel(px, py);
        let p_gdi = gdi_cap.image.get_pixel(px, py);
        let dr = (p_wgc[0] as i32 - p_gdi[0] as i32).abs();
        let dg = (p_wgc[1] as i32 - p_gdi[1] as i32).abs();
        let db = (p_wgc[2] as i32 - p_gdi[2] as i32).abs();
        if dr > 2 || dg > 2 || db > 2 {
          diff_pixels += 1;
        }
      }
    }
  }
  let diff_ratio = diff_pixels as f64 / total_sampled as f64;
  println!("WGC vs GDI diff ratio: {:.2}% ({}/{})", diff_ratio * 100.0, diff_pixels, total_sampled);
  assert!(diff_ratio <= 0.01, "Expected <=1% diff between WGC and GDI on client region, got {:.2}%", diff_ratio * 100.0);
}

#[test]
fn test_wgc_occlusion_isolation() {
  let _lock = WGC_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
  let target_class = w!("AuvWgcOcclusionTarget");
  let occluder_class = w!("AuvWgcOccluder");

  // Target window: Blue/Green (BGR: 0x00FF_FF00 -> Cyan: B=255, G=255, R=0)
  let (_target_win, target_driver_win) = create_test_window("AUV Occlusion Target", target_class, 250, 250, 400, 300, 0x00FF_FF00);

  // Occluder window: Pure Red (BGR: 0x0000_00FF -> R=255, G=0, B=0), placed directly on top of Target
  let (_occluder_win, _occluder_driver_win) = create_test_window("AUV Occluder", occluder_class, 250, 250, 400, 300, 0x0000_00FF);

  pump_messages(Duration::from_millis(300));

  let session = WindowsDriver::default().open_local().expect("failed to open driver session");

  // Capture Target Window (which is completely occluded underneath the occluder window)
  let cap = session.window().capture_wgc(&target_driver_win).expect("WGC capture of occluded window failed");
  assert_eq!(cap.backend, "wgc.windows");

  // Sample center pixel: must be Cyan (R~0, G~255, B~255), definitely NOT Red (R~255, G~0, B~0)
  let cx = cap.image.width() / 2;
  let cy = cap.image.height() / 2;
  let pixel = cap.image.get_pixel(cx, cy);

  println!("Occluded target captured center pixel: RGBA({}, {}, {}, {})", pixel[0], pixel[1], pixel[2], pixel[3]);
  assert!(pixel[0] < 50, "Target window should not have Red > 50, got {}", pixel[0]);
  assert!(pixel[1] > 200, "Target window should have Green > 200, got {}", pixel[1]);
  assert!(pixel[2] > 200, "Target window should have Blue > 200, got {}", pixel[2]);
}

#[test]
fn test_wgc_resize_robustness() {
  let _lock = WGC_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
  let resize_class = w!("AuvWgcResizeTest");
  let (test_win, driver_win) = create_test_window("AUV WGC Resize Test", resize_class, 150, 150, 400, 300, 0x00AA_BBCC);

  let session = WindowsDriver::default().open_local().expect("failed to open driver session");

  // 1. Initial capture
  let cap1 = session.window().capture_wgc(&driver_win).expect("initial WGC capture failed");
  let init_w = cap1.image.width();
  let init_h = cap1.image.height();
  assert!(init_w >= 350 && init_h >= 250);

  // 2. Resize window to larger size (550x420)
  unsafe {
    let _ = SetWindowPos(test_win.hwnd, None, 0, 0, 550, 420, SWP_NOMOVE | SWP_NOZORDER | SWP_FRAMECHANGED);
  }
  pump_messages(Duration::from_millis(300));

  // 3. Second capture: should succeed with new size without panic or crash
  let cap2 = session.window().capture_wgc(&driver_win).expect("post-resize WGC capture failed");
  let resized_w = cap2.image.width();
  let resized_h = cap2.image.height();

  println!("Window initial capture: {}x{}, after resize: {}x{}", init_w, init_h, resized_w, resized_h);
  assert!(resized_w > init_w, "Expected resized width {} > initial width {}", resized_w, init_w);
  assert!(resized_h > init_h, "Expected resized height {} > initial height {}", resized_h, init_h);
}

#[test]
fn test_wgc_display_capture() {
  let _lock = WGC_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
  let session = WindowsDriver::default().open_local().expect("failed to open driver session");
  let display_cap = session.display().capture_wgc(None).expect("WGC display capture failed");

  assert_eq!(display_cap.capture.backend, "wgc.windows");
  assert!(display_cap.capture.image.width() > 0);
  assert!(display_cap.capture.image.height() > 0);
  assert!(display_cap.capture.scale_factor > 0.0);
  println!(
    "WGC Display capture success: {}x{}, scale: {:.2}",
    display_cap.capture.image.width(),
    display_cap.capture.image.height(),
    display_cap.capture.scale_factor
  );
}

#[test]
fn test_wgc_health_cached_and_liveness() {
  let _lock = WGC_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
  let class_name = w!("AuvWgcHealthTest");
  let (_test_win, driver_win) = create_test_window("AUV WGC Health Test", class_name, 200, 200, 400, 300, 0x0011_2233);

  // 1. Liveness check on live window
  let is_alive = check_window_liveness(&driver_win).expect("check_window_liveness should succeed");
  assert!(is_alive, "live window must pass check_window_liveness");

  // 2. Prewarm starts background worker
  let prewarm_dur = prewarm_wgc_window(&driver_win).expect("prewarm_wgc_window should succeed");
  println!("prewarm_wgc_window took {:?}", prewarm_dur);

  // 3. Cached health check should hit fresh cache in <15ms
  let t_cached = Instant::now();
  let cached_health = capture_window_health_cached(&driver_win).expect("capture_window_health_cached should succeed");
  let cached_dur = t_cached.elapsed();
  println!("capture_window_health_cached took {:?}", cached_dur);
  assert!(cached_health.is_fresh, "cached health must be fresh");
  assert!(cached_health.alive, "test window must be alive");
  assert!(cached_dur < Duration::from_millis(15), "cached check should be fast");

  // A retained HWND must never reuse cached health after its live process ID
  // no longer matches the process that produced the Window snapshot.
  let mut stale_pid_window = driver_win.clone();
  stale_pid_window.process_id = Some(std::process::id().wrapping_add(1));
  assert!(
    capture_window_health_cached(&stale_pid_window).is_err(),
    "health lookup must reject a stale expected PID before consulting cache"
  );

  // 4. Strict health check should succeed with fresh sample
  let strict_health = capture_window_health_strict(&driver_win).expect("capture_window_health_strict should succeed");
  assert!(strict_health.is_fresh, "strict health must be fresh");
  assert!(strict_health.alive, "test window must be alive");

  // Requests for distinct HWNDs are explicitly serialized while a single
  // health-session/cache target is active; neither request may evict the other.
  let second_class = w!("AuvWgcHealthSecondTest");
  let (_second_test_win, second_driver_win) =
    create_test_window("AUV WGC Health Second Test", second_class, 650, 220, 400, 300, 0x0033_2211);
  let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
  let first_barrier = std::sync::Arc::clone(&barrier);
  let first_window = driver_win.clone();
  let first = std::thread::spawn(move || {
    auv_driver_windows::desktop::ensure_input_desktop();
    first_barrier.wait();
    capture_window_health_strict(&first_window)
  });
  let second_barrier = std::sync::Arc::clone(&barrier);
  let second = std::thread::spawn(move || {
    auv_driver_windows::desktop::ensure_input_desktop();
    second_barrier.wait();
    capture_window_health_strict(&second_driver_win)
  });
  barrier.wait();
  assert!(first.join().expect("first health request panicked").expect("first target health failed").is_fresh);
  assert!(second.join().expect("second health request panicked").expect("second target health failed").is_fresh);
}

#[test]
fn test_wgc_worker_concurrent_with_capture_wgc_no_deadlock() {
  let _lock = WGC_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
  let class_name = w!("AuvWgcDeadlockTest");
  let (_test_win, driver_win) = create_test_window("AUV WGC Deadlock Test", class_name, 220, 220, 400, 300, 0x0044_5566);

  // Start health worker
  let _ = prewarm_wgc_window(&driver_win).expect("prewarm must succeed");

  let session = WindowsDriver::default().open_local().expect("failed to open driver session");

  // Run concurrent formal WGC captures while background worker is actively sampling
  for i in 0..5 {
    let t0 = Instant::now();
    let cap = session.window().capture_wgc(&driver_win).expect("formal WGC capture during worker execution failed");
    assert_eq!(cap.backend, "wgc.windows");
    assert!(cap.image.width() > 0);
    println!("Concurrent capture {} succeeded in {:?}", i, t0.elapsed());

    // Concurrently read cached health
    let health = capture_window_health_cached(&driver_win).expect("cached health during capture failed");
    assert!(health.alive);
  }
}
