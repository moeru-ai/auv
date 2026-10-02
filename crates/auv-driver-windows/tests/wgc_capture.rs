//! Acceptance tests for Windows.Graphics.Capture (WGC) backend.
#![cfg(target_os = "windows")]

use auv_driver_common::geometry::Rect;
use auv_driver_common::window::{Window, WindowRef};
use auv_driver_common::{CoordinateSpace, Driver};
use auv_driver_windows::WindowsDriver;
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

#[test]
fn test_wgc_pixel_correctness_and_gdi_comparison() {
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
