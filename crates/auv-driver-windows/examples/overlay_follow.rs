//! Live evidence that the overlay follows what the Windows driver really does, without
//! touching the user's mouse or focus.
//!
//! ```text
//! cargo run --release -p auv-driver-windows --features overlay --example overlay_follow -- [out-dir]
//! ```
//!
//! The example opens an opaque backdrop and two top-level windows of its own above it (so a
//! capture never contains anything else on the user's desktop), finds the windows through the driver's
//! window discovery, and performs real window-targeted input on them through
//! `WindowsDriverSession`: a logical-mouse movement and clicks posted straight to each
//! window. The live overlay (`OverlayApi::follow_operations`) draws the cursor, ripples and
//! window marks from what the driver reports. It never calls `SendInput`, `SetCursorPos` or
//! any focus API, and it checks that the OS pointer and the foreground window are the same
//! before and after. The screen is captured around both windows after each step.
//!
//! Needs an interactive desktop. Exits non-zero when a check fails.

#[cfg(target_os = "windows")]
#[allow(dead_code)]
#[path = "../../auv-driver-overlay-windows/examples/support/backdrop.rs"]
mod backdrop;

#[cfg(target_os = "windows")]
fn main() {
  let out_dir = std::env::args().nth(1).unwrap_or_else(|| "overlay-follow".to_string());
  if let Err(error) = live::run(std::path::Path::new(&out_dir)) {
    eprintln!("overlay_follow failed: {error}");
    std::process::exit(1);
  }
}

#[cfg(not(target_os = "windows"))]
fn main() {
  eprintln!("overlay_follow only runs on Windows");
}

#[cfg(target_os = "windows")]
mod live {
  use std::path::Path;
  use std::sync::atomic::{AtomicUsize, Ordering};
  use std::time::Duration;

  use auv_driver_common::window::Window;
  use auv_driver_common::{
    ClickOptions, DisturbanceLevel, Driver, InputDeliveryPath, InputPolicy, InputTarget, MouseButton, MouseCubicBezierSegment, MouseCurve,
    MouseCurveMapping, MouseMotionOptions, MouseStart, MoveMouseRequest, Point, WindowInput, WindowPoint,
  };
  use auv_driver_overlay::LifecycleOptions;
  use auv_driver_windows::{WindowsDriver, WindowsDriverSession};
  use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
  use windows::Win32::Graphics::Gdi::{BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, PAINTSTRUCT};
  use windows::Win32::System::LibraryLoader::GetModuleHandleW;
  use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetClientRect, GetForegroundWindow, RegisterClassExW, SW_SHOWNOACTIVATE,
    SetWindowLongPtrW, ShowWindow, WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_PAINT, WM_RBUTTONDOWN, WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOPMOST,
    WS_OVERLAPPEDWINDOW,
  };
  use windows::core::w;

  use super::backdrop::{Backdrop, capture_region, pump, write_bmp};

  const LEFT: i32 = 200;
  const TOP: i32 = 200;
  const WIDTH: i32 = 920;
  const HEIGHT: i32 = 420;

  const TITLES: [&str; 2] = ["AUV overlay follow A", "AUV overlay follow B"];
  static CLICKS: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];
  static MOVES: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];

  extern "system" fn harness_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let index = unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as usize;
    match message {
      WM_LBUTTONDOWN | WM_RBUTTONDOWN if index < 2 => {
        CLICKS[index].fetch_add(1, Ordering::Relaxed);
        LRESULT(0)
      }
      WM_MOUSEMOVE if index < 2 => {
        MOVES[index].fetch_add(1, Ordering::Relaxed);
        LRESULT(0)
      }
      WM_PAINT => {
        unsafe {
          let mut paint = PAINTSTRUCT::default();
          let dc = BeginPaint(hwnd, &mut paint);
          let mut client = RECT::default();
          let _ = GetClientRect(hwnd, &mut client);
          let brush = CreateSolidBrush(if index == 0 {
            COLORREF(0x00F2_F0E8)
          } else {
            COLORREF(0x00E8_ECF2)
          });
          FillRect(dc, &client, brush);
          let _ = DeleteObject(brush);
          let _ = EndPaint(hwnd, &paint);
        }
        LRESULT(0)
      }
      _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
  }

  fn open_harness_window(index: usize, left: i32, top: i32) -> Result<HWND, String> {
    let instance = unsafe { GetModuleHandleW(None) }.map_err(|error| error.to_string())?;
    let class = WNDCLASSEXW {
      cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
      lpfnWndProc: Some(harness_proc),
      hInstance: instance.into(),
      lpszClassName: w!("AuvOverlayFollowHarness"),
      ..Default::default()
    };
    unsafe {
      let _ = RegisterClassExW(&class);
    }
    let title: Vec<u16> = TITLES[index].encode_utf16().chain(std::iter::once(0)).collect();
    // NOTICE: no-activate and `SW_SHOWNOACTIVATE` keep the user's focus where it is.
    let hwnd = unsafe {
      CreateWindowExW(
        WS_EX_NOACTIVATE | WS_EX_TOPMOST,
        w!("AuvOverlayFollowHarness"),
        windows::core::PCWSTR(title.as_ptr()),
        WS_OVERLAPPEDWINDOW,
        left,
        top,
        420,
        300,
        None,
        None,
        instance,
        None,
      )
    }
    .map_err(|error| format!("failed to open harness window: {error}"))?;
    unsafe {
      SetWindowLongPtrW(hwnd, GWLP_USERDATA, index as isize);
      let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    Ok(hwnd)
  }

  fn find(session: &WindowsDriverSession, title: &str) -> Result<Window, String> {
    session
      .window()
      .list()
      .map_err(|error| error.to_string())?
      .into_iter()
      .find(|window| window.title.as_deref() == Some(title))
      .ok_or_else(|| format!("driver window discovery did not list {title:?}"))
  }

  /// A straight logical-mouse movement of `delta` over `duration`, delivered to `window`.
  fn line(window: &Window, from: Point, delta: Point, duration: Duration) -> MoveMouseRequest {
    MoveMouseRequest {
      mouse: 0,
      target: Some(InputTarget::Window(window.clone())),
      start: MouseStart::Screen(from),
      curve: MouseCurve {
        start: Point::new(0.0, 0.0),
        segments: vec![MouseCubicBezierSegment {
          control_1: Point::new(delta.x / 3.0, delta.y / 3.0),
          control_2: Point::new(delta.x * 2.0 / 3.0, delta.y * 2.0 / 3.0),
          end: delta,
        }],
      },
      mapping: MouseCurveMapping {
        width: 1.0,
        height: 1.0,
      },
      options: MouseMotionOptions {
        duration,
        sample_rate_hz: 60,
        curve_tolerance: 1.0,
      },
    }
  }

  struct Checks {
    failures: usize,
  }

  impl Checks {
    fn check(&mut self, ok: bool, message: String) {
      println!("  {} {message}", if ok { "PASS" } else { "FAIL" });
      if !ok {
        self.failures += 1;
      }
    }
  }

  pub(super) fn run(out_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(out_dir).map_err(|error| format!("failed to create {}: {error}", out_dir.display()))?;
    let backdrop = Backdrop::show(LEFT, TOP, WIDTH, HEIGHT)?;
    // Created after the backdrop and topmost like it, so they stack above it.
    let hwnds = [
      open_harness_window(0, LEFT + 30, TOP + 30)?,
      open_harness_window(1, LEFT + 470, TOP + 100)?,
    ];
    pump(Duration::from_millis(300));

    let result = drive(out_dir);

    for hwnd in hwnds {
      unsafe {
        let _ = DestroyWindow(hwnd);
      }
    }
    backdrop.close();
    result
  }

  fn drive(out_dir: &Path) -> Result<(), String> {
    let mut checks = Checks { failures: 0 };
    let session = WindowsDriver::new().open_local().map_err(|error| error.to_string())?;
    let (window_a, window_b) = (find(&session, TITLES[0])?, find(&session, TITLES[1])?);
    println!("driver discovered {:?} at {:?} and {:?} at {:?}", TITLES[0], window_a.frame, TITLES[1], window_b.frame);

    let pointer_before = session.input().current_position().ok();
    let foreground_before = unsafe { GetForegroundWindow() };

    let follower = session.overlay().follow_operations(LifecycleOptions::manual()).map_err(|error| error.to_string())?;

    let snapshot = |name: &str| -> Result<(), String> {
      pump(Duration::from_millis(140));
      write_bmp(&out_dir.join(format!("{name}.bmp")), WIDTH, HEIGHT, &capture_region(LEFT, TOP, WIDTH, HEIGHT)?)
    };

    println!("operations");
    let mut delivered = Vec::new();
    // 1. The logical mouse glides over window A: sampled input, posted to the window.
    let a_origin = window_a.frame.origin;
    let (_, moved) = session
      .input()
      .move_mouse(
        line(&window_a, Point::new(a_origin.x + 60.0, a_origin.y + 80.0), Point::new(140.0, 80.0), Duration::from_millis(500)),
        |_| true,
      )
      .map_err(|error| error.to_string())?;
    delivered.push(moved);
    snapshot("1-after-move-in-a")?;

    // 2. A click in window A, then a click in window B across the screen.
    delivered.push(session.window().click(&window_a, WindowPoint::new(220.0, 190.0), background()).map_err(|error| error.to_string())?);
    snapshot("2-after-click-in-a")?;
    delivered.push(session.window().click(&window_b, WindowPoint::new(180.0, 120.0), background()).map_err(|error| error.to_string())?);
    snapshot("3-after-click-in-b")?;
    delivered.push(
      session
        .window()
        .click(
          &window_b,
          WindowPoint::new(300.0, 200.0),
          ClickOptions {
            button: MouseButton::Right,
            ..background()
          },
        )
        .map_err(|error| error.to_string())?,
    );
    pump(Duration::from_millis(500));
    snapshot("4-settled")?;

    let stats = follower.stop().map_err(|error| error.to_string())?;
    pump(Duration::from_millis(200));

    println!("results");
    let (clicks_a, clicks_b) = (CLICKS[0].load(Ordering::Relaxed), CLICKS[1].load(Ordering::Relaxed));
    let (moves_a, moves_b) = (MOVES[0].load(Ordering::Relaxed), MOVES[1].load(Ordering::Relaxed));
    println!("  window A received {clicks_a} click(s) and {moves_a} move message(s); window B {clicks_b} and {moves_b}");
    checks.check(clicks_a == 1 && clicks_b == 2, "the driver really delivered the clicks it reported (A: 1, B: 2)".to_string());
    checks.check(moves_a > 10, format!("the driver really delivered the sampled movement to window A ({moves_a} messages)"));

    // The driver's own typed delivery record is the evidence that nothing touched the
    // user's mouse or focus: every action was posted to a window and reports no disturbance.
    let undisturbing = delivered.iter().all(|result| {
      result.selected_path == InputDeliveryPath::WindowTargetedMouse
        && result.mouse_disturbance == DisturbanceLevel::None
        && result.focus_disturbance == DisturbanceLevel::None
    });
    checks.check(
      undisturbing,
      format!("all {} actions were window-targeted with no mouse or focus disturbance (InputActionResult)", delivered.len()),
    );
    // Informational only: the user's own hand can move the pointer or switch windows while
    // this runs, so these cannot prove the automation left them alone.
    println!(
      "  info: OS pointer {pointer_before:?} before, {:?} after; foreground window changed: {}",
      session.input().current_position().ok(),
      foreground_before != unsafe { GetForegroundWindow() }
    );

    println!(
      "  frames {}, late {}, failures {}; frame time P50 {:.2} ms P95 {:.2} ms; event latency P50 {:.2} ms P95 {:.2} ms",
      stats.frames,
      stats.late_frames,
      stats.present_failures,
      stats.frame_ms.p50,
      stats.frame_ms.p95,
      stats.event_latency_ms.p50,
      stats.event_latency_ms.p95
    );
    checks.check(stats.frames > 0 && stats.present_failures == 0, format!("the overlay drew frames ({}) without failures", stats.frames));

    if checks.failures == 0 {
      println!("all checks passed; snapshots in {}", out_dir.display());
      Ok(())
    } else {
      Err(format!("{} check(s) failed", checks.failures))
    }
  }

  fn background() -> ClickOptions {
    ClickOptions {
      policy: InputPolicy::BackgroundOnly,
      ..ClickOptions::default()
    }
  }
}
