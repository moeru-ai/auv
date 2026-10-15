//! Loopback receiver benchmark (Round-2 Gate: 5s, 1000 edges @ 4ms, receiver P99 <= 2.0ms)
//!
//! Unlike dispatch-side smoke tests (which measure QPC right after thread wait),
//! this benchmark creates an actual Win32 receiver thread with a real message pump
//! and low-level keyboard hook (WH_KEYBOARD_LL) to measure the exact arrival time
//! of SendInput events received through the OS message queue.
//!
//! This verifies that the OS timer, thread scheduler, and input message queue do not
//! degrade under sustained high-frequency input (1000 edges @ 4ms over ~5 seconds).

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::Duration;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
#[cfg(target_os = "windows")]
use windows::Win32::System::StationsAndDesktops::{DESKTOP_ACCESS_FLAGS, DESKTOP_CONTROL_FLAGS, OpenInputDesktop, SetThreadDesktop};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
  CallNextHookEx, DispatchMessageW, GetMessageW, HHOOK, KBDLLHOOKSTRUCT, MSG, PostThreadMessageW, SetWindowsHookExW, TranslateMessage,
  UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_QUIT,
};

use crate::injector::{PrecisionClock, Win32InputActuator, percentile_linear};
use crate::synthesizer::KeyCode;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoopbackBenchmarkReport {
  pub planned_edges: usize,
  pub received_edges: usize,
  pub count_reconciled: bool,
  pub duration_seconds: f64,
  pub interval_ms: f64,
  pub raw_p50_ms: f64,
  pub raw_p95_ms: f64,
  pub raw_p99_ms: f64,
  pub raw_max_ms: f64,
  pub median_ms: f64,
  pub comp_p50_ms: f64,
  pub comp_p95_ms: f64,
  pub comp_p99_ms: f64,
  pub comp_max_ms: f64,
  pub gate_2_0ms_passed: bool,
}

// Global receiver state for the Windows low-level hook
static SHARED_RECORDS: RwLock<Option<Arc<Mutex<Vec<f64>>>>> = RwLock::new(None);
static GLOBAL_CLOCK: RwLock<Option<Arc<PrecisionClock>>> = RwLock::new(None);

#[cfg(target_os = "windows")]
unsafe extern "system" fn low_level_keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
  if code >= 0 {
    let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
    // 0x5A is VK_Z (K1)
    if kb.vkCode == 0x5A
      && let Ok(clock_guard) = GLOBAL_CLOCK.read()
      && let Some(ref clock) = *clock_guard
    {
      let now = clock.elapsed_ms();
      if let Ok(sink_guard) = SHARED_RECORDS.read()
        && let Some(ref sink) = *sink_guard
        && let Ok(mut records) = sink.lock()
      {
        records.push(now);
      }
    }
  }
  unsafe { CallNextHookEx(HHOOK::default(), code, wparam, lparam) }
}

/// Runs the 5-second round-2 loopback receiver benchmark (1000 edges @ 4.0ms).
/// Measures arrival at the receiving message pump via WH_KEYBOARD_LL and asserts compensated P99 <= 2.0ms.
pub fn run_loopback_receiver_bench(num_edges: usize, interval_ms: f64) -> Result<LoopbackBenchmarkReport, String> {
  #[cfg(not(target_os = "windows"))]
  {
    let _ = (num_edges, interval_ms);
    return Err("Loopback receiver bench is only supported on Windows".to_string());
  }

  #[cfg(target_os = "windows")]
  {
    let received_timestamps = Arc::new(Mutex::new(Vec::with_capacity(num_edges + 10)));
    let clock = Arc::new(PrecisionClock::new());

    {
      let mut lock_records = SHARED_RECORDS.write().unwrap();
      *lock_records = Some(received_timestamps.clone());
      let mut lock_clock = GLOBAL_CLOCK.write().unwrap();
      *lock_clock = Some(clock.clone());
    }

    let ready_flag = Arc::new(AtomicBool::new(false));
    let receiver_tid = Arc::new(AtomicU32::new(0));

    let ready_clone = ready_flag.clone();
    let tid_clone = receiver_tid.clone();

    let attach_desktop = || unsafe {
      if let Ok(desktop) = OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_ACCESS_FLAGS(0x000F_01FF)) {
        let _ = SetThreadDesktop(desktop);
      }
    };

    // Attach calling thread to interactive input desktop
    attach_desktop();

    // Spawn receiver thread running an actual Win32 message pump with WH_KEYBOARD_LL hook
    let receiver_thread = thread::spawn(move || {
      // Attach receiver thread to interactive input desktop
      attach_desktop();

      let tid = unsafe { windows::Win32::System::Threading::GetCurrentThreadId() };
      tid_clone.store(tid, Ordering::Release);

      let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(low_level_keyboard_proc), None, 0) }
        .map_err(|e| format!("failed to install low level keyboard hook: {e}"))?;

      ready_clone.store(true, Ordering::Release);

      let mut msg = MSG::default();
      unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
          let _ = TranslateMessage(&msg);
          DispatchMessageW(&msg);
        }
        let _ = UnhookWindowsHookEx(hook);
      }

      Ok::<(), String>(())
    });

    // Wait until receiver thread is active and has installed hook
    let start_wait = std::time::Instant::now();
    while !ready_flag.load(Ordering::Acquire) {
      if start_wait.elapsed() > Duration::from_secs(3) {
        return Err("timed out waiting for receiver thread to initialize".to_string());
      }
      thread::sleep(Duration::from_millis(10));
    }

    // Settling delay for hook registration
    thread::sleep(Duration::from_millis(50));

    // Clear any startup events
    {
      let mut lock = received_timestamps.lock().map_err(|_| "poisoned lock")?;
      lock.clear();
    }

    // Planned schedule: dispatch num_edges at interval_ms
    let start_time_ms = clock.elapsed_ms() + 20.0;
    let mut planned_times = Vec::with_capacity(num_edges);

    for i in 0..num_edges {
      let t_plan = start_time_ms + (i as f64) * interval_ms;
      planned_times.push(t_plan);

      clock.sleep_until_ms(t_plan);

      // Alternate down / up on each edge (each constitutes one edge)
      let is_down = (i % 2) == 0;
      Win32InputActuator::send_key(KeyCode::K1, is_down);
    }

    // Drain period: wait for OS message queue to settle
    thread::sleep(Duration::from_millis(300));

    // Stop receiver thread via WM_QUIT to its thread ID
    let tid = receiver_tid.load(Ordering::Acquire);
    if tid != 0 {
      unsafe {
        let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
      }
    }
    let _ = receiver_thread.join();

    // Reset statics
    {
      let mut lock_records = SHARED_RECORDS.write().unwrap();
      *lock_records = None;
      let mut lock_clock = GLOBAL_CLOCK.write().unwrap();
      *lock_clock = None;
    }

    // Collect timestamps
    let timestamps = {
      let lock = received_timestamps.lock().map_err(|_| "poisoned lock")?;
      lock.clone()
    };

    let received_edges = timestamps.len();
    let count_reconciled = received_edges == num_edges;
    let duration_seconds = (num_edges as f64) * interval_ms / 1000.0;

    let eval_count = num_edges.min(received_edges);
    let mut raw_deviations: Vec<f64> = (0..eval_count).map(|i| timestamps[i] - planned_times[i]).collect();

    if raw_deviations.is_empty() {
      return Err(format!("No edges received in loopback receiver window (expected {num_edges})"));
    }

    raw_deviations.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let raw_p50 = percentile_linear(&raw_deviations, 0.50);
    let raw_p95 = percentile_linear(&raw_deviations, 0.95);
    let raw_p99 = percentile_linear(&raw_deviations, 0.99);
    let raw_max = *raw_deviations.last().unwrap_or(&0.0);

    let median = raw_p50;
    let mut comp_deviations: Vec<f64> = raw_deviations.iter().map(|x| (x - median).abs()).collect();
    comp_deviations.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let comp_p50 = percentile_linear(&comp_deviations, 0.50);
    let comp_p95 = percentile_linear(&comp_deviations, 0.95);
    let comp_p99 = percentile_linear(&comp_deviations, 0.99);
    let comp_max = *comp_deviations.last().unwrap_or(&0.0);

    let gate_passed = count_reconciled && comp_p99 <= 2.0;

    Ok(LoopbackBenchmarkReport {
      planned_edges: num_edges,
      received_edges,
      count_reconciled,
      duration_seconds,
      interval_ms,
      raw_p50_ms: raw_p50,
      raw_p95_ms: raw_p95,
      raw_p99_ms: raw_p99,
      raw_max_ms: raw_max,
      median_ms: median,
      comp_p50_ms: comp_p50,
      comp_p95_ms: comp_p95,
      comp_p99_ms: comp_p99,
      comp_max_ms: comp_max,
      gate_2_0ms_passed: gate_passed,
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[cfg(not(target_os = "windows"))]
  #[test]
  fn test_loopback_receiver_reports_unsupported_platform() {
    assert_eq!(run_loopback_receiver_bench(50, 4.0).unwrap_err(), "Loopback receiver bench is only supported on Windows");
  }

  #[cfg(target_os = "windows")]
  #[test]
  #[ignore = "requires an interactive, low-load Windows runner with a keyboard hook"]
  fn test_loopback_receiver_smoke_50_edges() {
    let report = run_loopback_receiver_bench(50, 4.0).expect("loopback bench");
    assert!(report.count_reconciled, "50 edges must reconcile in receiver window, got {}/{}", report.received_edges, report.planned_edges);
    assert!(report.comp_p99_ms <= 2.0, "receiver compensated P99 must be <= 2.0ms, got {}ms", report.comp_p99_ms);
  }

  #[cfg(target_os = "windows")]
  #[test]
  #[ignore = "5-second live loopback benchmark; requires an interactive Windows desktop"]
  fn test_loopback_receiver_bench_1000_edges() {
    let report = run_loopback_receiver_bench(1000, 4.0).expect("loopback bench 1000 edges");
    assert!(report.count_reconciled, "1000 edges must reconcile in receiver window, got {}/{}", report.received_edges, report.planned_edges);
    assert!(report.comp_p99_ms <= 2.0, "receiver compensated P99 must be <= 2.0ms, got {}ms", report.comp_p99_ms);
  }
}
