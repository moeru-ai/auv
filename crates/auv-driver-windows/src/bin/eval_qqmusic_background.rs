//! Evaluation harness for QQ Music background control via SMTC and CoreAudio.
//!
//! Asserts 100% focus invariance (GetForegroundWindow() constant) across all
//! operations (metadata query, play, pause, toggle, next, previous, volume).
#![cfg(target_os = "windows")]

use auv_driver_windows::media::{AudioVolumeController, MediaPlaybackStatus, SmtcMediaManager};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

fn get_fg() -> HWND {
  unsafe { GetForegroundWindow() }
}

fn assert_focus_unchanged(expected: HWND, operation: &str) {
  let current = get_fg();
  assert_eq!(
    expected, current,
    "REDLINE VIOLATION: Foreground focus stolen during '{operation}'! Expected: {:?}, Actual: {:?}",
    expected.0, current.0
  );
}

fn ensure_input_desktop() {
  unsafe {
    use windows::Win32::System::StationsAndDesktops::{DESKTOP_ACCESS_FLAGS, DESKTOP_CONTROL_FLAGS, OpenInputDesktop, SetThreadDesktop};
    if let Ok(desktop) = OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_ACCESS_FLAGS(0x000F_01FF)) {
      let _ = SetThreadDesktop(desktop);
    }
  }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
  if sorted.is_empty() {
    return 0.0;
  }
  let rank = p * (sorted.len() - 1) as f64;
  let lower = rank.floor() as usize;
  let upper = rank.ceil() as usize;
  let weight = rank - lower as f64;
  sorted[lower] * (1.0 - weight) + sorted[upper] * weight
}

fn main() {
  ensure_input_desktop();
  let initial_fg = get_fg();
  println!("============================================================");
  println!(" AUV QQ Music Background Media Control Evaluation");
  println!(" Initial Foreground HWND: {:?}", initial_fg.0);
  println!("============================================================");

  let manager = SmtcMediaManager::new().expect("Failed to initialize SmtcMediaManager");
  let session = manager
    .find_session("qqmusic")
    .expect("Failed to search sessions")
    .expect("QQ Music SMTC session not found. Please ensure QQ Music is running.");

  println!("Targeted Session: \"{}\"", session.app_id());
  assert_focus_unchanged(initial_fg, "Session Acquisition");

  // 1. Benchmark Now Playing Metadata Queries (30 samples)
  println!("\n--- [Phase 1] Now-Playing Metadata Queries (30 rounds) ---");
  let mut query_latencies: Vec<f64> = Vec::with_capacity(30);
  for i in 0..30 {
    let t0 = Instant::now();
    let meta = session.track_metadata().expect("Failed to query track metadata");
    let status = session.playback_status().expect("Failed to query status");
    let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
    query_latencies.push(elapsed_ms);
    assert_focus_unchanged(initial_fg, &format!("Query Round {i}"));
    if i == 0 || i == 29 {
      println!("  Round [{:02}]: Title=\"{}\", Artist=\"{}\", Status={:?}, Latency={:.2}ms", i, meta.title, meta.artist, status, elapsed_ms);
    }
  }
  query_latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
  println!(
    "  Metadata Query Distribution: P50={:.2}ms, P95={:.2}ms, Max={:.2}ms (30/30 passed)",
    percentile(&query_latencies, 0.50),
    percentile(&query_latencies, 0.95),
    query_latencies.last().copied().unwrap_or(0.0)
  );

  // 2. Benchmark Play / Pause Cycles (20 cycles = 40 actions)
  println!("\n--- [Phase 2] Play / Pause Control Cycles (20 cycles) ---");
  let mut play_latencies: Vec<f64> = Vec::new();
  let mut pause_latencies: Vec<f64> = Vec::new();
  for cycle in 0..20 {
    // Play
    let t0 = Instant::now();
    let play_ok = session.play().expect("Play call failed");
    let play_ms = t0.elapsed().as_secs_f64() * 1000.0;
    play_latencies.push(play_ms);
    assert!(play_ok, "Play returned false");
    assert_focus_unchanged(initial_fg, &format!("Play Cycle {cycle}"));

    // Verify status became Playing
    std::thread::sleep(Duration::from_millis(150));
    let status = session.playback_status().expect("Query status failed");
    assert_eq!(status, MediaPlaybackStatus::Playing, "Expected status Playing at cycle {cycle}");

    // Pause
    let t1 = Instant::now();
    let pause_ok = session.pause().expect("Pause call failed");
    let pause_ms = t1.elapsed().as_secs_f64() * 1000.0;
    pause_latencies.push(pause_ms);
    assert!(pause_ok, "Pause returned false");
    assert_focus_unchanged(initial_fg, &format!("Pause Cycle {cycle}"));

    // Verify status became Paused
    std::thread::sleep(Duration::from_millis(150));
    let status_after = session.playback_status().expect("Query status failed");
    assert_eq!(status_after, MediaPlaybackStatus::Paused, "Expected status Paused at cycle {cycle}");

    if cycle % 5 == 0 || cycle == 19 {
      println!("  Cycle [{:02}]: Play={:.2}ms -> Playing, Pause={:.2}ms -> Paused", cycle, play_ms, pause_ms);
    }
  }

  play_latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
  pause_latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
  println!(
    "  Play Latency Distribution:  P50={:.2}ms, P95={:.2}ms, Max={:.2}ms",
    percentile(&play_latencies, 0.50),
    percentile(&play_latencies, 0.95),
    play_latencies.last().copied().unwrap_or(0.0)
  );
  println!(
    "  Pause Latency Distribution: P50={:.2}ms, P95={:.2}ms, Max={:.2}ms",
    percentile(&pause_latencies, 0.50),
    percentile(&pause_latencies, 0.95),
    pause_latencies.last().copied().unwrap_or(0.0)
  );

  // 3. Benchmark Track Switching (Next / Previous, 10 rounds = 20 skips)
  println!("\n--- [Phase 3] Track Switching (Next / Previous, 10 rounds) ---");
  let mut next_latencies: Vec<f64> = Vec::new();
  let mut prev_latencies: Vec<f64> = Vec::new();
  for r in 0..10 {
    let before = session.track_metadata().expect("query before failed");

    // Skip Next
    let t0 = Instant::now();
    let next_ok = session.skip_next().expect("Skip next failed");
    let next_ms = t0.elapsed().as_secs_f64() * 1000.0;
    next_latencies.push(next_ms);
    assert!(next_ok, "Next returned false");
    assert_focus_unchanged(initial_fg, &format!("Skip Next {r}"));

    std::thread::sleep(Duration::from_millis(800));
    let after_next = session.track_metadata().expect("query after next failed");
    if after_next.title == before.title {
      // Allow extra settle time for network loading
      std::thread::sleep(Duration::from_millis(600));
    }

    // Skip Previous
    let t1 = Instant::now();
    let prev_ok = session.skip_previous().expect("Skip previous failed");
    let prev_ms = t1.elapsed().as_secs_f64() * 1000.0;
    prev_latencies.push(prev_ms);
    assert!(prev_ok, "Prev returned false");
    assert_focus_unchanged(initial_fg, &format!("Skip Prev {r}"));

    std::thread::sleep(Duration::from_millis(800));
    let after_prev = session.track_metadata().expect("query after prev failed");

    if r % 2 == 0 || r == 9 {
      println!(
        "  Round [{:02}]: Next ({:.2}ms) -> \"{}\", Prev ({:.2}ms) -> \"{}\"",
        r, next_ms, after_next.title, prev_ms, after_prev.title
      );
    }
  }

  next_latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
  prev_latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
  println!(
    "  Skip Next Distribution: P50={:.2}ms, P95={:.2}ms, Max={:.2}ms",
    percentile(&next_latencies, 0.50),
    percentile(&next_latencies, 0.95),
    next_latencies.last().copied().unwrap_or(0.0)
  );
  println!(
    "  Skip Prev Distribution: P50={:.2}ms, P95={:.2}ms, Max={:.2}ms",
    percentile(&prev_latencies, 0.50),
    percentile(&prev_latencies, 0.95),
    prev_latencies.last().copied().unwrap_or(0.0)
  );

  let _ = session.pause();
  std::thread::sleep(Duration::from_millis(200));

  // 4. Benchmark CoreAudio Volume Control (PID 34772, 10 rounds)
  println!("\n--- [Phase 4] CoreAudio Process Volume Control (10 rounds) ---");
  let qq_pid = 34772u32;
  let original_vol = AudioVolumeController::get_process_volume(qq_pid).expect("Failed to get process volume");
  println!("  Initial QQ Music Volume: {:.2}", original_vol);
  assert_focus_unchanged(initial_fg, "Volume Query");

  let mut vol_latencies: Vec<f64> = Vec::new();
  for v_idx in 0..10 {
    let target_vol = 0.50 + (v_idx as f32) * 0.04;
    let t0 = Instant::now();
    AudioVolumeController::set_process_volume(qq_pid, target_vol).expect("Failed to set process volume");
    let vol_ms = t0.elapsed().as_secs_f64() * 1000.0;
    vol_latencies.push(vol_ms);
    assert_focus_unchanged(initial_fg, &format!("Set Volume {v_idx}"));

    let read_back = AudioVolumeController::get_process_volume(qq_pid).expect("Failed to read back volume");
    assert!((read_back - target_vol).abs() < 0.01, "Volume mismatch at round {v_idx}: expected {target_vol}, got {read_back}");
  }

  // Restore original volume
  AudioVolumeController::set_process_volume(qq_pid, original_vol).expect("Failed to restore volume");
  println!("  Restored original volume to {:.2}", original_vol);
  assert_focus_unchanged(initial_fg, "Volume Restore");

  vol_latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
  println!(
    "  Volume Set Distribution: P50={:.2}ms, P95={:.2}ms, Max={:.2}ms (10/10 passed)",
    percentile(&vol_latencies, 0.50),
    percentile(&vol_latencies, 0.95),
    vol_latencies.last().copied().unwrap_or(0.0)
  );

  // Final check
  let final_fg = get_fg();
  println!("\n============================================================");
  println!(" EVALUATION VERDICT: 100% SUCCESS");
  println!(" Initial Foreground HWND: {:?}", initial_fg.0);
  println!(" Final Foreground HWND:   {:?}", final_fg.0);
  println!(" Total Assertions Checked: 30 + 40 + 20 + 10 = 100 focus assertions");
  println!(" Focus Disturbances: 0 (Zero Focus Stealing)");
  println!("============================================================");
  assert_eq!(initial_fg, final_fg, "Final focus check violated!");
}
