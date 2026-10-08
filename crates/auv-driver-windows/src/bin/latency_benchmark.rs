//! Latency benchmark harness for Windows driver critical paths.
//!
//! Measures latency distribution (min, p50, p90, p95, p99, max, mean, stddev) for:
//! 1. `capture_display` (xcap / GDI full screen capture)
//! 2. `capture_window` (PrintWindow window capture)
//! 3. SendInput injection (`click_at`, `press_key`, `scroll_at`)
//! 4. System OCR (`recognize_text_in_capture` on 400x100 ROI)
//!
//! Outputs formatted Markdown tables and exports raw per-call records to JSONL.

use std::path::PathBuf;
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

use auv_driver_common::geometry::{NormalizedRect, Point};
use auv_driver_common::input::{Click, ClickModifiers, KeyPressOptions, MouseButton, Scroll};
use auv_driver_common::vision::TextRecognitionOptions;

use auv_driver_windows::capture::{capture_display, capture_window};
use auv_driver_windows::desktop::ensure_input_desktop;
use auv_driver_windows::input::{click_at, press_key, scroll_at};
use auv_driver_windows::latency::{LatencyRecord, clear_records, save_records_to_jsonl, start_recording, stop_recording, take_records};
use auv_driver_windows::vision::recognize_text_in_capture;
use auv_driver_windows::wgc::{capture_display_wgc, capture_window_wgc};
use auv_driver_windows::window::list_windows;

#[derive(Debug, Clone)]
pub struct LatencyStats {
  pub path: String,
  pub scene: String,
  pub count: usize,
  pub min: f64,
  pub p50: f64,
  pub p90: f64,
  pub p95: f64,
  pub p99: f64,
  pub max: f64,
  pub mean: f64,
  pub stddev: f64,
  pub resolution: Option<(u32, u32)>,
  pub backend: Option<String>,
}

pub fn compute_stats(path: &str, scene: &str, records: &[LatencyRecord]) -> Option<LatencyStats> {
  let mut latencies: Vec<f64> = records.iter().map(|r| r.latency_ms).collect();
  if latencies.is_empty() {
    return None;
  }
  latencies.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
  let count = latencies.len();
  let min = latencies[0];
  let max = latencies[count - 1];
  let p50 = latencies[(count as f64 * 0.50).floor() as usize];
  let p90 = latencies[((count as f64 * 0.90).floor() as usize).min(count - 1)];
  let p95 = latencies[((count as f64 * 0.95).floor() as usize).min(count - 1)];
  let p99 = latencies[((count as f64 * 0.99).floor() as usize).min(count - 1)];
  let sum: f64 = latencies.iter().sum();
  let mean = sum / count as f64;
  let variance: f64 = latencies.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / count as f64;
  let stddev = variance.sqrt();
  let resolution = records.first().and_then(|r| r.resolution);
  let backend = records.first().and_then(|r| r.backend.clone());

  Some(LatencyStats {
    path: path.to_string(),
    scene: scene.to_string(),
    count,
    min,
    p50,
    p90,
    p95,
    p99,
    max,
    mean,
    stddev,
    resolution,
    backend,
  })
}

fn print_stats_table(stats_list: &[LatencyStats]) {
  println!(
    "\n| 路径 / 测量项 | 场景 / 目标 | 分辨率 | 后端 | 样本数 | Min (ms) | P50 (ms) | P90 (ms) | P95 (ms) | P99 (ms) | Max (ms) | Mean (ms) | StdDev |"
  );
  println!("| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |");
  for s in stats_list {
    let res_str = s.resolution.map(|(w, h)| format!("{w}x{h}")).unwrap_or_else(|| "-".to_string());
    let backend_str = s.backend.as_deref().unwrap_or("-");
    println!(
      "| `{}` | {} | {} | `{}` | {} | {:.2} | **{:.2}** | {:.2} | **{:.2}** | {:.2} | {:.2} | {:.2} | {:.2} |",
      s.path, s.scene, res_str, backend_str, s.count, s.min, s.p50, s.p90, s.p95, s.p99, s.max, s.mean, s.stddev
    );
  }
}

fn print_freshness_breakdown(records: &[LatencyRecord]) {
  let wgc_records: Vec<&LatencyRecord> = records.iter().filter(|r| r.backend.as_deref() == Some("wgc.windows")).collect();
  if wgc_records.is_empty() {
    return;
  }
  println!("\n=== WGC 帧新鲜度拆分 (Frame Freshness Breakdown) ===");
  println!(
    "| 测量路径 | 样本总量 | 新帧数 (fresh=true) | 新帧占比 | 新帧 P50 (ms) | 新帧 P95 (ms) | 复用帧数 (fresh=false) | 复用占比 | 复用 P50 (ms) |"
  );
  println!("| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |");

  use std::collections::BTreeMap;
  let mut groups: BTreeMap<String, Vec<&LatencyRecord>> = BTreeMap::new();
  for r in &wgc_records {
    let key = r.path.clone();
    groups.entry(key).or_default().push(r);
  }

  for (path, list) in groups {
    let total = list.len();
    let fresh: Vec<LatencyRecord> =
      list.iter().filter(|r| r.details.as_deref().map(|d| d.contains("fresh=true")).unwrap_or(true)).map(|&r| r.clone()).collect();
    let reused: Vec<LatencyRecord> =
      list.iter().filter(|r| r.details.as_deref().map(|d| d.contains("fresh=false")).unwrap_or(false)).map(|&r| r.clone()).collect();

    let fresh_stat = compute_stats(&path, "fresh=true", &fresh);
    let reused_stat = compute_stats(&path, "fresh=false", &reused);

    let fresh_count = fresh.len();
    let fresh_ratio = if total > 0 {
      fresh_count as f64 / total as f64 * 100.0
    } else {
      0.0
    };
    let fresh_p50 = fresh_stat.as_ref().map(|s| s.p50).unwrap_or(0.0);
    let fresh_p95 = fresh_stat.as_ref().map(|s| s.p95).unwrap_or(0.0);

    let reused_count = reused.len();
    let reused_ratio = if total > 0 {
      reused_count as f64 / total as f64 * 100.0
    } else {
      0.0
    };
    let reused_p50 = reused_stat.as_ref().map(|s| s.p50).unwrap_or(0.0);

    println!(
      "| `{}` | {} | {} | {:.1}% | **{:.2}** | **{:.2}** | {} | {:.1}% | {:.2} |",
      path, total, fresh_count, fresh_ratio, fresh_p50, fresh_p95, reused_count, reused_ratio, reused_p50
    );
  }
}

fn bench_display_capture(samples: usize, scene: &str) -> (Option<LatencyStats>, Vec<LatencyRecord>) {
  println!("\n[1/5] Benchmarking `capture_display` (Scene: {scene}, Samples: {samples})...");
  // Warmup
  for _ in 0..5 {
    let _ = capture_display(None);
    sleep(Duration::from_millis(10));
  }

  clear_records();
  start_recording();
  for i in 0..samples {
    if (i + 1) % 50 == 0 || i + 1 == samples {
      print!("\r  Progress: {}/{}", i + 1, samples);
      use std::io::Write;
      let _ = std::io::stdout().flush();
    }
    if let Err(e) = capture_display(None) {
      eprintln!("\n  capture_display error on #{}: {:?}", i + 1, e);
    }
    sleep(Duration::from_millis(10));
  }
  println!();
  stop_recording();

  let records = take_records();
  let stats = compute_stats("capture_display", scene, &records);
  (stats, records)
}

fn bench_display_capture_wgc(samples: usize, scene: &str) -> (Option<LatencyStats>, Vec<LatencyRecord>) {
  println!("\n[*] Benchmarking `capture_display_wgc` (Scene: {scene}, Samples: {samples})...");
  // Warmup
  for _ in 0..5 {
    let _ = capture_display_wgc(None);
    sleep(Duration::from_millis(10));
  }

  clear_records();
  start_recording();
  for i in 0..samples {
    if (i + 1) % 50 == 0 || i + 1 == samples {
      print!("\r  Progress: {}/{}", i + 1, samples);
      use std::io::Write;
      let _ = std::io::stdout().flush();
    }
    if let Err(e) = capture_display_wgc(None) {
      eprintln!("\n  capture_display_wgc error on #{}: {:?}", i + 1, e);
    }
    sleep(Duration::from_millis(10));
  }
  println!();
  stop_recording();

  let records = take_records();
  let stats = compute_stats("capture_display", scene, &records);
  (stats, records)
}

fn find_or_spawn_target_window() -> Option<auv_driver_common::window::Window> {
  // Try launching notepad via shell command
  let _ = Command::new("cmd").args(["/c", "start", "notepad.exe"]).spawn();
  sleep(Duration::from_millis(800));

  for _ in 0..10 {
    let all = list_windows().unwrap_or_default();
    if let Some(w) = all.iter().find(|w| {
      w.title.as_deref().map(|t| t.to_lowercase().contains("notepad") || t.contains("记事本") || t.contains("无标题")).unwrap_or(false)
        || w.app_name.as_deref().map(|a| a.to_lowercase().contains("notepad")).unwrap_or(false)
    }) {
      return Some(w.clone());
    }
    sleep(Duration::from_millis(200));
  }

  let all = list_windows().unwrap_or_default();
  all.into_iter().find(|w| {
    if let Some(app) = w.app_name.as_deref() {
      let app_lower = app.to_lowercase();
      app_lower != "explorer.exe"
        && !app_lower.contains("overlay")
        && app_lower != "cmd.exe"
        && w.frame.size.width > 200.0
        && w.frame.size.height > 200.0
    } else {
      false
    }
  })
}

fn bench_window_capture(window: &auv_driver_common::window::Window, samples: usize) -> (Option<LatencyStats>, Vec<LatencyRecord>) {
  let target_desc =
    format!("{} ({}) [printwindow]", window.app_name.as_deref().unwrap_or("unknown"), window.title.as_deref().unwrap_or("untitled"));
  println!("\n[2/5] Benchmarking `capture_window` (Target: {target_desc}, Samples: {samples})...");

  // Warmup
  for _ in 0..5 {
    let _ = capture_window(window);
    sleep(Duration::from_millis(10));
  }

  clear_records();
  start_recording();
  for i in 0..samples {
    if (i + 1) % 50 == 0 || i + 1 == samples {
      print!("\r  Progress: {}/{}", i + 1, samples);
      use std::io::Write;
      let _ = std::io::stdout().flush();
    }
    let _ = capture_window(window);
    sleep(Duration::from_millis(10));
  }
  println!();
  stop_recording();

  let records = take_records();
  let stats = compute_stats("capture_window", &target_desc, &records);
  (stats, records)
}

fn bench_window_capture_wgc(window: &auv_driver_common::window::Window, samples: usize) -> (Option<LatencyStats>, Vec<LatencyRecord>) {
  let target_desc = format!("{} ({}) [wgc]", window.app_name.as_deref().unwrap_or("unknown"), window.title.as_deref().unwrap_or("untitled"));
  println!("\n[*] Benchmarking `capture_window_wgc` (Target: {target_desc}, Samples: {samples})...");

  // Warmup
  for _ in 0..5 {
    let _ = capture_window_wgc(window);
    sleep(Duration::from_millis(10));
  }

  clear_records();
  start_recording();
  for i in 0..samples {
    if (i + 1) % 50 == 0 || i + 1 == samples {
      print!("\r  Progress: {}/{}", i + 1, samples);
      use std::io::Write;
      let _ = std::io::stdout().flush();
    }
    let _ = capture_window_wgc(window);
    sleep(Duration::from_millis(10));
  }
  println!();
  stop_recording();

  let records = take_records();
  let stats = compute_stats("capture_window", &target_desc, &records);
  (stats, records)
}

fn bench_input_paths(samples: usize) -> (Vec<LatencyStats>, Vec<LatencyRecord>) {
  println!("\n[3/5] Benchmarking SendInput injection paths (click_at, press_key, scroll_at; Samples: {samples} each)...");
  let mut all_stats = Vec::new();
  let mut all_records = Vec::new();

  // 1. click_at
  println!("  Testing `click_at` (MouseButton::Left)...");
  let test_point = Point::new(600.0, 400.0);
  for _ in 0..5 {
    let _ = click_at(test_point, MouseButton::Left, Click::Single, ClickModifiers::default());
    sleep(Duration::from_millis(5));
  }
  clear_records();
  start_recording();
  for i in 0..samples {
    if let Err(e) = click_at(test_point, MouseButton::Left, Click::Single, ClickModifiers::default()) {
      eprintln!("\n  click_at error on #{}: {:?}", i + 1, e);
    }
    sleep(Duration::from_millis(5));
    if (i + 1) % 100 == 0 || i + 1 == samples {
      print!("\r    click_at progress: {}/{}", i + 1, samples);
      use std::io::Write;
      let _ = std::io::stdout().flush();
    }
  }
  println!();
  stop_recording();
  let click_recs = take_records();
  if let Some(s) = compute_stats("click_at", "SendInput Left Click", &click_recs) {
    all_stats.push(s);
  }
  all_records.extend(click_recs);

  // 2. press_key
  println!("  Testing `press_key` (\"Return\")...");
  let key_opt = KeyPressOptions {
    key: "Return".to_string(),
    settle: Duration::ZERO,
  };
  for _ in 0..5 {
    let _ = press_key(key_opt.clone());
    sleep(Duration::from_millis(5));
  }
  clear_records();
  start_recording();
  for i in 0..samples {
    if let Err(e) = press_key(key_opt.clone()) {
      eprintln!("\n  press_key error on #{}: {:?}", i + 1, e);
    }
    sleep(Duration::from_millis(5));
    if (i + 1) % 100 == 0 || i + 1 == samples {
      print!("\r    press_key progress: {}/{}", i + 1, samples);
      use std::io::Write;
      let _ = std::io::stdout().flush();
    }
  }
  println!();
  stop_recording();
  let key_recs = take_records();
  if let Some(s) = compute_stats("press_key", "SendInput Return Key", &key_recs) {
    all_stats.push(s);
  }
  all_records.extend(key_recs);

  // 3. scroll_at
  // One wheel notch (100 logical pixels) keeps the historical benchmark load.
  println!("  Testing `scroll_at` (delta_y = 100.0)...");
  let scroll_val = Scroll::new(0.0, 100.0);
  for _ in 0..5 {
    let _ = scroll_at(test_point, scroll_val, Duration::ZERO);
    sleep(Duration::from_millis(5));
  }
  clear_records();
  start_recording();
  for i in 0..samples {
    if let Err(e) = scroll_at(test_point, scroll_val, Duration::ZERO) {
      eprintln!("\n  scroll_at error on #{}: {:?}", i + 1, e);
    }
    sleep(Duration::from_millis(5));
    if (i + 1) % 100 == 0 || i + 1 == samples {
      print!("\r    scroll_at progress: {}/{}", i + 1, samples);
      use std::io::Write;
      let _ = std::io::stdout().flush();
    }
  }
  println!();
  stop_recording();
  let scroll_recs = take_records();
  if let Some(s) = compute_stats("scroll_at", "SendInput Mouse Wheel", &scroll_recs) {
    all_stats.push(s);
  }
  all_records.extend(scroll_recs);

  (all_stats, all_records)
}

fn bench_ocr_path(samples: usize) -> (Option<LatencyStats>, Vec<LatencyRecord>) {
  println!("\n[4/5] Benchmarking OCR (`recognize_text_in_capture` on 400x100 ROI; Samples: {samples})...");
  let initial = match capture_display(None) {
    Ok(d) => d,
    Err(e) => {
      eprintln!("  Failed to take initial capture for OCR bench: {e}");
      return (None, Vec::new());
    }
  };

  let img_w = initial.capture.image.width() as f64;
  let img_h = initial.capture.image.height() as f64;
  let roi = NormalizedRect::new(0.05, 0.05, 400.0 / img_w, 100.0 / img_h);
  let options = TextRecognitionOptions::default();

  // Warmup
  for _ in 0..5 {
    let _ = recognize_text_in_capture(&initial.capture, roi, &options);
    sleep(Duration::from_millis(10));
  }

  clear_records();
  start_recording();
  for i in 0..samples {
    if (i + 1) % 50 == 0 || i + 1 == samples {
      print!("\r  Progress: {}/{}", i + 1, samples);
      use std::io::Write;
      let _ = std::io::stdout().flush();
    }
    let _ = recognize_text_in_capture(&initial.capture, roi, &options);
    sleep(Duration::from_millis(5));
  }
  println!();
  stop_recording();

  let records = take_records();
  let stats = compute_stats("recognize_text_in_capture", "ROI 400x100", &records);
  (stats, records)
}

fn main() {
  ensure_input_desktop();
  if !cfg!(feature = "latency-telemetry") {
    eprintln!("NOTICE: compiled without --features latency-telemetry; runtime latency records will not be populated.");
  }
  let args: Vec<String> = std::env::args().collect();
  let mut samples = 300;
  let mut scene_filter: Option<String> = None;
  let mut output_path = PathBuf::from("docs/ai/references/driver/2026-10-03-windows-driver-latency-baseline.jsonl");

  let mut i = 1;
  while i < args.len() {
    match args[i].as_str() {
      "--samples" => {
        if i + 1 < args.len() {
          samples = args[i + 1].parse().unwrap_or(300);
          i += 1;
        }
      }
      "--scene" => {
        if i + 1 < args.len() {
          scene_filter = Some(args[i + 1].clone());
          i += 1;
        }
      }
      "--output" => {
        if i + 1 < args.len() {
          output_path = PathBuf::from(&args[i + 1]);
          i += 1;
        }
      }
      _ => {}
    }
    i += 1;
  }

  println!("============================================================");
  println!(" AUV Windows Driver Latency Baseline Benchmark");
  println!(" Samples per cell: {samples}");
  println!(" Output JSONL: {:?}", output_path);
  println!("============================================================");

  let mut all_stats = Vec::new();
  let mut all_records = Vec::new();

  let run_all = scene_filter.is_none();
  let scene = scene_filter.as_deref().unwrap_or("all");

  if run_all || scene == "idle" || scene == "wgc" || scene == "compare" {
    let (s, r) = bench_display_capture(samples, "Idle Desktop 1440p [gdi]");
    if let Some(stat) = s {
      all_stats.push(stat);
    }
    all_records.extend(r);

    let (s_wgc, r_wgc) = bench_display_capture_wgc(samples, "Idle Desktop 1440p [wgc]");
    if let Some(stat) = s_wgc {
      all_stats.push(stat);
    }
    all_records.extend(r_wgc);
  }

  if run_all || scene == "active" {
    let (s, r) = bench_display_capture(samples, "Active Desktop Load 1440p");
    if let Some(stat) = s {
      all_stats.push(stat);
    }
    all_records.extend(r);
  }

  if run_all || scene == "window" || scene == "wgc" || scene == "compare" {
    if let Some(target_window) = find_or_spawn_target_window() {
      let (s, r) = bench_window_capture(&target_window, samples);
      if let Some(stat) = s {
        all_stats.push(stat);
      }
      all_records.extend(r);

      let (s_wgc, r_wgc) = bench_window_capture_wgc(&target_window, samples);
      if let Some(stat) = s_wgc {
        all_stats.push(stat);
      }
      all_records.extend(r_wgc);
    } else {
      eprintln!("  Failed to locate any application window");
    }
  }

  if run_all || scene == "input" {
    let (stats, r) = bench_input_paths(samples);
    all_stats.extend(stats);
    all_records.extend(r);
  }

  if run_all || scene == "ocr" {
    let (s, r) = bench_ocr_path(samples);
    if let Some(stat) = s {
      all_stats.push(stat);
    }
    all_records.extend(r);
  }

  if scene == "mhw" || scene == "load" {
    let (s, r) = bench_display_capture(samples, "GPU Load 1440p [gdi]");
    if let Some(stat) = s {
      all_stats.push(stat);
    }
    all_records.extend(r);

    let (s_wgc, r_wgc) = bench_display_capture_wgc(samples, "GPU Load 1440p [wgc]");
    if let Some(stat) = s_wgc {
      all_stats.push(stat);
    }
    all_records.extend(r_wgc);

    if let Some(target_window) = find_or_spawn_target_window() {
      let (s_wgc_win, r_wgc_win) = bench_window_capture_wgc(&target_window, samples);
      if let Some(stat) = s_wgc_win {
        all_stats.push(stat);
      }
      all_records.extend(r_wgc_win);
    }
  }

  print_stats_table(&all_stats);
  print_freshness_breakdown(&all_records);

  if let Err(e) = save_records_to_jsonl(&all_records, &output_path) {
    eprintln!("\nFailed to save records to {:?}: {}", output_path, e);
  } else {
    println!("\nExported {} latency records to {:?}", all_records.len(), output_path);
  }
}
