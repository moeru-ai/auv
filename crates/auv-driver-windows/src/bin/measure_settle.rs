//! Settle timing measurement harness for Windows background wheel input.
//!
//! Measures frame-reflect latency from single wheel delivery (`PostMessageW`
//! via `WindowTargetedWheel`, `BackgroundOnly`) to the first WGC frame reflecting
//! viewport pixel motion, across two scrollable surfaces:
//! 1. QQ Music (owner anchor, playlist/lyrics view)
//! 2. Windows Explorer (Downloads folder file list)
//!
//! Produces P50/P95/mean triplets for cold vs warm runs using standard linear
//! interpolation, zero outlier filtering, and exports raw records to JSONL.

#[cfg(target_os = "windows")]
mod windows_impl {
  use std::env;
  use std::fs::OpenOptions;
  use std::io::Write;
  use std::path::PathBuf;
  use std::thread::sleep;
  use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

  use auv_driver_common::Driver;
  use auv_driver_common::geometry::WindowPoint;
  use auv_driver_common::input::{InputPolicy, Scroll, ScrollDeliveryStrategy, ScrollOptions, WindowInput};
  use auv_driver_common::window::Window;
  use auv_driver_windows::WindowsDriver;
  use auv_driver_windows::desktop::ensure_input_desktop;
  use auv_driver_windows::wgc::{FrameStream, FrameUpdate};
  use auv_driver_windows::window::list_windows;
  use image::RgbaImage;
  use serde::{Deserialize, Serialize};
  use windows::Win32::Foundation::HWND;

  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct SettleMeasurementRecord {
    pub timestamp: String,
    pub surface: String,
    // NOTICE: no window title field. The QQ Music title is the playing track
    // and artist, which is the user's own music and must not reach evidence.
    pub window_id: String,
    pub iteration: usize,
    pub cold: bool,
    pub delta_y: f64,
    pub delivery_to_reflect_ms: f64,
    pub motion_settle_ms: f64,
    pub frames_observed: usize,
    pub stalled: bool,
  }

  #[derive(Debug, Clone)]
  pub struct LatencyTriplet {
    pub p50: f64,
    pub p95: f64,
    pub mean: f64,
    pub count: usize,
  }

  fn chrono_now_iso() -> String {
    let now = SystemTime::now();
    let dur = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = dur.as_secs();
    let millis = dur.subsec_millis();
    format!("{secs}.{millis:03}Z")
  }

  /// Standard linear interpolation percentile (Type 7 / standard continuous percentile).
  pub fn linear_percentile(sorted_vals: &[f64], p: f64) -> f64 {
    let n = sorted_vals.len();
    if n == 0 {
      return 0.0;
    }
    if n == 1 {
      return sorted_vals[0];
    }
    let rank = p * (n as f64 - 1.0);
    let low = rank.floor() as usize;
    let high = (low + 1).min(n - 1);
    let weight = rank - low as f64;
    sorted_vals[low] * (1.0 - weight) + sorted_vals[high] * weight
  }

  pub fn compute_triplet(latencies: &[f64]) -> Option<LatencyTriplet> {
    if latencies.is_empty() {
      return None;
    }
    let mut sorted = latencies.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let count = sorted.len();
    let p50 = linear_percentile(&sorted, 0.50);
    let p95 = linear_percentile(&sorted, 0.95);
    let sum: f64 = sorted.iter().sum();
    let mean = sum / count as f64;
    Some(LatencyTriplet {
      p50,
      p95,
      mean,
      count,
    })
  }

  fn pixels_differ(img1: &RgbaImage, img2: &RgbaImage, min_changed: usize) -> bool {
    if img1.dimensions() != img2.dimensions() {
      return true;
    }
    let raw1 = img1.as_raw();
    let raw2 = img2.as_raw();
    let mut changed = 0;
    for (chunk1, chunk2) in raw1.as_chunks::<4>().0.iter().zip(raw2.as_chunks::<4>().0.iter()) {
      let dr = (chunk1[0] as i32 - chunk2[0] as i32).abs();
      let dg = (chunk1[1] as i32 - chunk2[1] as i32).abs();
      let db = (chunk1[2] as i32 - chunk2[2] as i32).abs();
      if dr > 15 || dg > 15 || db > 15 {
        changed += 1;
        if changed >= min_changed {
          return true;
        }
      }
    }
    false
  }

  fn resolve_window_handle(window: &Window) -> Result<HWND, String> {
    let raw = window.reference.id.parse::<usize>().map_err(|e| format!("invalid window reference id {}: {e}", window.reference.id))?;
    Ok(HWND(raw as *mut _))
  }

  struct TargetSurface {
    name: &'static str,
    app_pattern: &'static str,
    title_pattern: Option<&'static str>,
    scroll_point: WindowPoint,
  }

  pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    ensure_input_desktop();

    let mut runs_per_surface: usize = 55;
    // NOTICE: the default output is the temp dir so a rerun cannot overwrite the
    // committed evidence in docs/ai/references/driver/; pass --output to choose.
    let mut output_path = env::temp_dir().join("auv-windows-settle-timing.jsonl");

    let args: Vec<String> = env::args().collect();
    let mut i = 1;
    while i < args.len() {
      match args[i].as_str() {
        "--runs" if i + 1 < args.len() => {
          runs_per_surface = args[i + 1].parse().unwrap_or(55);
          i += 2;
        }
        "--output" if i + 1 < args.len() => {
          output_path = PathBuf::from(&args[i + 1]);
          i += 2;
        }
        _ => i += 1,
      }
    }

    println!("================================================================================");
    println!("AUV Windows Scroll Settle Timing Benchmark (Brief 2)");
    println!("Target surfaces: QQ Music (anchor) & Windows Explorer");
    println!("Measurements per surface: {runs_per_surface} (1 cold, {} warm)", runs_per_surface.saturating_sub(1));
    println!("Percentile method: Standard Linear Interpolation (zero outlier filtering)");
    println!("Output evidence: {}", output_path.display());
    println!("================================================================================");

    let session = WindowsDriver::new().open_local()?;
    let all_windows = list_windows()?;

    // TODO(windows-settle-receiver): both surfaces are the user's own windows, so
    // a run scrolls their playlist and Downloads folder. Replace them with a
    // harness-owned scroll receiver before the next run; deferred because this
    // change only stops the harness recording titles and overwriting evidence.
    let targets = [
      TargetSurface {
        name: "qqmusic",
        app_pattern: "QQMusic.exe",
        title_pattern: None,
        scroll_point: WindowPoint::new(750.0, 350.0),
      },
      TargetSurface {
        name: "explorer",
        app_pattern: "explorer.exe",
        title_pattern: Some("下载"),
        scroll_point: WindowPoint::new(600.0, 400.0),
      },
    ];

    let mut all_records: Vec<SettleMeasurementRecord> = Vec::new();

    for target in &targets {
      println!("\n>>> Resolving surface: {}...", target.name);
      let matched_win = all_windows.iter().find(|w| {
        let app_match = w.app_name.as_deref().map(|p| p.eq_ignore_ascii_case(target.app_pattern)).unwrap_or(false);
        if !app_match {
          return false;
        }
        if let Some(tp) = target.title_pattern {
          w.title.as_deref().map(|t| t.contains(tp)).unwrap_or(false)
        } else {
          // If no title pattern, skip Program Manager or untitled taskbars
          w.title.as_deref().map(|t| !t.is_empty()).unwrap_or(false) && w.frame.size.width > 200.0 && w.frame.size.height > 200.0
        }
      });

      let window = match matched_win {
        Some(w) => w.clone(),
        None => {
          eprintln!("ERROR: Could not find target window for surface {}", target.name);
          continue;
        }
      };

      println!(
        "Found window: REF {} [{}], Frame: {}x{} at {},{}",
        window.reference.id,
        window.app_name.as_deref().unwrap_or("?"),
        window.frame.size.width,
        window.frame.size.height,
        window.frame.origin.x,
        window.frame.origin.y
      );

      let hwnd = resolve_window_handle(&window)?;
      println!("Opening WGC FrameStream for HWND {:?}...", hwnd);
      let stream = FrameStream::open(hwnd)?;

      // Drain initial frames to obtain a steady baseline
      println!("Obtaining steady pre-scroll baseline frame...");
      let mut baseline_image = loop {
        match stream.read(Duration::from_millis(1000)) {
          Ok(FrameUpdate::Fresh { image, .. }) => break image,
          Ok(FrameUpdate::Unchanged { .. }) => sleep(Duration::from_millis(10)),
          Err(e) => return Err(format!("failed to read baseline frame: {e:?}").into()),
        }
      };
      // Let it settle for 300ms
      sleep(Duration::from_millis(300));
      if let Ok(FrameUpdate::Fresh { image, .. }) = stream.read(Duration::from_millis(100)) {
        baseline_image = image;
      }

      println!(
        "Baseline captured: {}x{}. Starting {} measurement iterations...",
        baseline_image.width(),
        baseline_image.height(),
        runs_per_surface
      );

      let mut surface_records: Vec<SettleMeasurementRecord> = Vec::new();

      for iter in 0..runs_per_surface {
        let is_cold = iter == 0;
        // Alternate scroll direction to keep viewport comfortably within list bounds
        let delta_y = if iter % 2 == 0 { 120.0 } else { -120.0 };

        let scroll_options = ScrollOptions {
          policy: InputPolicy::BackgroundOnly,
          delivery_strategy: ScrollDeliveryStrategy::default(),
          settle: Duration::ZERO,
        };

        // Timestamp t0 right before delivery dispatch
        let t_delivery = Instant::now();
        let scroll_result = session.window().scroll(&window, target.scroll_point, Scroll::new(0.0, delta_y), scroll_options);
        if let Err(e) = scroll_result {
          eprintln!("Delivery error on iter {iter}: {e:?}");
        }

        // Tight loop capturing WGC frames to record first reflection and motion settle
        let timeout = Duration::from_millis(1000);
        let mut first_reflect_ms = None;
        let mut last_motion_ms = None;
        let mut frames_observed = 0;
        let mut prev_frame = baseline_image.clone();
        let mut newest_image = baseline_image.clone();
        let mut consecutive_still_frames = 0;

        let poll_start = Instant::now();
        while poll_start.elapsed() < timeout {
          match stream.read(Duration::ZERO) {
            Ok(FrameUpdate::Fresh { image, .. }) => {
              frames_observed += 1;
              let elapsed = t_delivery.elapsed().as_secs_f64() * 1000.0;
              if first_reflect_ms.is_none() {
                if pixels_differ(&baseline_image, &image, 50) {
                  first_reflect_ms = Some(elapsed);
                  last_motion_ms = Some(elapsed);
                  prev_frame = image.clone();
                  newest_image = image;
                }
              } else if pixels_differ(&prev_frame, &image, 50) {
                last_motion_ms = Some(elapsed);
                prev_frame = image.clone();
                newest_image = image;
                consecutive_still_frames = 0;
              } else {
                consecutive_still_frames += 1;
                if consecutive_still_frames >= 3 {
                  break;
                }
              }
            }
            Ok(FrameUpdate::Unchanged { .. }) => {
              if let Some(last_t) = last_motion_ms {
                let current_elapsed = t_delivery.elapsed().as_secs_f64() * 1000.0;
                if current_elapsed - last_t >= 100.0 {
                  break;
                }
              }
              std::thread::yield_now();
            }
            Err(e) => {
              eprintln!("Stream error on iter {iter}: {e:?}");
              break;
            }
          }
        }

        let stalled = first_reflect_ms.is_none();
        let delivery_to_reflect_ms = first_reflect_ms.unwrap_or(timeout.as_secs_f64() * 1000.0);
        let motion_settle_ms = last_motion_ms.unwrap_or(delivery_to_reflect_ms);

        let record = SettleMeasurementRecord {
          timestamp: chrono_now_iso(),
          surface: target.name.to_string(),
          window_id: window.reference.id.clone(),
          iteration: iter,
          cold: is_cold,
          delta_y,
          delivery_to_reflect_ms,
          motion_settle_ms,
          frames_observed,
          stalled,
        };

        if iter % 10 == 0 || iter == runs_per_surface - 1 {
          println!(
            "  [{}] iter {:02}/{} (cold: {:<5}) -> reflect: {:6.2} ms, settle: {:6.2} ms, frames: {}, stalled: {}",
            target.name, iter, runs_per_surface, is_cold, delivery_to_reflect_ms, motion_settle_ms, frames_observed, stalled
          );
        }

        surface_records.push(record.clone());
        all_records.push(record);

        // Update baseline to the settled image
        baseline_image = newest_image;

        // Inter-iteration pause to let compositor & app completely settle
        sleep(Duration::from_millis(200));
        // Drain any late post-settle frame
        if let Ok(FrameUpdate::Fresh { image, .. }) = stream.read(Duration::from_millis(50)) {
          baseline_image = image;
        }
      }

      // Compute stats for this surface
      let cold_records: Vec<&SettleMeasurementRecord> = surface_records.iter().filter(|r| r.cold).collect();
      let warm_records: Vec<&SettleMeasurementRecord> = surface_records.iter().filter(|r| !r.cold).collect();

      let warm_reflect_latencies: Vec<f64> = warm_records.iter().map(|r| r.delivery_to_reflect_ms).collect();
      let warm_settle_latencies: Vec<f64> = warm_records.iter().map(|r| r.motion_settle_ms).collect();

      let warm_reflect_triplet = compute_triplet(&warm_reflect_latencies).unwrap_or(LatencyTriplet {
        p50: 0.0,
        p95: 0.0,
        mean: 0.0,
        count: 0,
      });
      let warm_settle_triplet = compute_triplet(&warm_settle_latencies).unwrap_or(LatencyTriplet {
        p50: 0.0,
        p95: 0.0,
        mean: 0.0,
        count: 0,
      });

      let cold_reflect = cold_records.first().map(|r| r.delivery_to_reflect_ms).unwrap_or(0.0);
      let cold_settle = cold_records.first().map(|r| r.motion_settle_ms).unwrap_or(0.0);

      println!("\n--- Surface Summary: {} ---", target.name);
      println!("  Cold (n={}): reflect = {:.2} ms, settle = {:.2} ms", cold_records.len(), cold_reflect, cold_settle);
      println!(
        "  Warm Reflect (n={}): P50 = {:.2} ms, P95 = {:.2} ms, Mean = {:.2} ms",
        warm_reflect_triplet.count, warm_reflect_triplet.p50, warm_reflect_triplet.p95, warm_reflect_triplet.mean
      );
      println!(
        "  Warm Settle  (n={}): P50 = {:.2} ms, P95 = {:.2} ms, Mean = {:.2} ms",
        warm_settle_triplet.count, warm_settle_triplet.p50, warm_settle_triplet.p95, warm_settle_triplet.mean
      );
    }

    // Write all raw records to output JSONL
    if let Some(parent) = output_path.parent() {
      std::fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).write(true).truncate(true).open(&output_path)?;
    for record in &all_records {
      let line = serde_json::to_string(record)?;
      writeln!(file, "{line}")?;
    }
    println!("\nExported {} measurement records to {}", all_records.len(), output_path.display());

    Ok(())
  }
}

#[cfg(target_os = "windows")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
  windows_impl::run()
}

#[cfg(not(target_os = "windows"))]
fn main() {
  eprintln!("measure-settle is Windows only");
}
