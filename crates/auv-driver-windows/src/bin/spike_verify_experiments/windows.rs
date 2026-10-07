#![cfg(target_os = "windows")]

use auv_driver_common::error::{DriverError, DriverResult};
use auv_driver_windows::desktop::ensure_input_desktop;
use auv_driver_windows::media::{MediaPlaybackStatus, SmtcMediaManager, SmtcSession};
use auv_driver_windows::track_identity::{TrackChangeVerdict, TrackIdentity, evaluate_track_change};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SmteEventType {
  Properties,
  PlaybackInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SingleProbeRun {
  pub run_index: usize,
  pub dimension: String,
  pub group: String,
  pub timestamp: String,
  pub initial_status: String,
  pub previous_title: String,
  pub previous_artist: String,
  pub new_title: String,
  pub new_artist: String,
  pub dispatch_ms: f64,
  pub playback_info_event_ms: Option<f64>,
  pub media_properties_event_ms: Option<f64>,
  pub verification_ms: f64,
  pub total_step3_ms: f64,
  pub is_fast_path: bool, // < 150ms
  pub is_slow_path: bool, // 800ms .. 2000ms
  pub is_timeout: bool,   // >= 2800ms
  pub verdict: String,
  pub notes: String,
}

fn chrono_now_iso() -> String {
  use std::time::SystemTime;
  let now = SystemTime::now();
  let dur = now.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
  format!("{}.{:03}Z", dur.as_secs(), dur.subsec_millis())
}

fn get_session() -> DriverResult<SmtcSession> {
  let manager = SmtcMediaManager::new()?;
  manager.find_session("qqmusic")?.ok_or_else(|| DriverError::NotFound {
    target: "QQ Music SMTC session".to_string(),
  })
}

/// Ensures that QQ Music is actively in `Playing` status.
fn ensure_playing(session: &SmtcSession, timeout: Duration) -> DriverResult<MediaPlaybackStatus> {
  let mut st = session.playback_status().unwrap_or(MediaPlaybackStatus::Closed);
  if st == MediaPlaybackStatus::Playing {
    return Ok(st);
  }

  let _ = session.play();
  let start = Instant::now();
  while start.elapsed() < timeout {
    std::thread::sleep(Duration::from_millis(50));
    if let Ok(curr) = session.playback_status() {
      st = curr;
      if st == MediaPlaybackStatus::Playing {
        return Ok(st);
      }
    }
  }
  Ok(st)
}

/// Performs a single single-variable skip probe, accurately capturing event timing.
fn execute_single_skip_probe(
  session: &SmtcSession,
  run_index: usize,
  dimension: &str,
  group: &str,
  register_listeners_before_dispatch: bool,
) -> DriverResult<SingleProbeRun> {
  let initial_status = session.playback_status().unwrap_or(MediaPlaybackStatus::Closed);
  let prev_meta = session.track_metadata()?;
  let prev_identity = TrackIdentity::from(&prev_meta);

  let (tx, rx) = channel::<(SmteEventType, Instant)>();

  let (token_prop, token_play, t_disp, dispatch_ms) = if register_listeners_before_dispatch {
    let tx_prop = tx.clone();
    let tok_p = session
      .on_media_properties_changed(move || {
        let _ = tx_prop.send((SmteEventType::Properties, Instant::now()));
      })
      .ok();

    let tx_play = tx;
    let tok_l = session
      .on_playback_info_changed(move || {
        let _ = tx_play.send((SmteEventType::PlaybackInfo, Instant::now()));
      })
      .ok();

    let t_d = Instant::now();
    let _ = session.skip_next();
    let d_ms = t_d.elapsed().as_secs_f64() * 1000.0;
    (tok_p, tok_l, t_d, d_ms)
  } else {
    let t_d = Instant::now();
    let _ = session.skip_next();
    let d_ms = t_d.elapsed().as_secs_f64() * 1000.0;

    let tx_prop = tx.clone();
    let tok_p = session
      .on_media_properties_changed(move || {
        let _ = tx_prop.send((SmteEventType::Properties, Instant::now()));
      })
      .ok();

    let tx_play = tx;
    let tok_l = session
      .on_playback_info_changed(move || {
        let _ = tx_play.send((SmteEventType::PlaybackInfo, Instant::now()));
      })
      .ok();
    (tok_p, tok_l, t_d, d_ms)
  };

  let t_verif_start = Instant::now();
  let timeout = Duration::from_millis(3200);

  let mut playback_info_event_ms: Option<f64> = None;
  let mut media_properties_event_ms: Option<f64> = None;

  let mut curr_identity = prev_identity.clone();
  let mut change_verdict = TrackChangeVerdict::Unchanged;
  let mut backoff = Duration::from_millis(10);

  while t_verif_start.elapsed() < timeout {
    // Check if any events arrived in channel
    while let Ok((evt_type, evt_time)) = rx.try_recv() {
      let delta_ms = evt_time.duration_since(t_disp).as_secs_f64() * 1000.0;
      match evt_type {
        SmteEventType::PlaybackInfo => {
          if playback_info_event_ms.is_none() {
            playback_info_event_ms = Some(delta_ms);
          }
        }
        SmteEventType::Properties => {
          if media_properties_event_ms.is_none() {
            media_properties_event_ms = Some(delta_ms);
          }
        }
      }
    }

    if let Ok(curr_meta) = session.track_metadata() {
      curr_identity = TrackIdentity::from(&curr_meta);
      let (verdict, _) = evaluate_track_change(&prev_identity, &curr_identity, false);
      change_verdict = verdict;
      if verdict == TrackChangeVerdict::Changed {
        break;
      }
    }

    // Wait for event or backoff interval
    if let Ok((evt_type, evt_time)) = rx.recv_timeout(backoff) {
      let delta_ms = evt_time.duration_since(t_disp).as_secs_f64() * 1000.0;
      match evt_type {
        SmteEventType::PlaybackInfo => {
          if playback_info_event_ms.is_none() {
            playback_info_event_ms = Some(delta_ms);
          }
        }
        SmteEventType::Properties => {
          if media_properties_event_ms.is_none() {
            media_properties_event_ms = Some(delta_ms);
          }
        }
      }
    }

    backoff = (backoff * 2).min(Duration::from_millis(80));
  }

  let verification_ms = t_verif_start.elapsed().as_secs_f64() * 1000.0;
  let total_step3_ms = dispatch_ms + verification_ms;

  if let Some(tok) = token_prop {
    let _ = session.remove_media_properties_changed(tok);
  }
  if let Some(tok) = token_play {
    let _ = session.remove_playback_info_changed(tok);
  }

  let is_fast = verification_ms < 150.0;
  let is_slow = (800.0..2800.0).contains(&verification_ms);
  let is_timeout = verification_ms >= 2800.0;

  Ok(SingleProbeRun {
    run_index,
    dimension: dimension.to_string(),
    group: group.to_string(),
    timestamp: chrono_now_iso(),
    initial_status: format!("{:?}", initial_status),
    previous_title: prev_identity.title.clone(),
    previous_artist: prev_identity.artist.clone(),
    new_title: curr_identity.title.clone(),
    new_artist: curr_identity.artist.clone(),
    dispatch_ms,
    playback_info_event_ms,
    media_properties_event_ms,
    verification_ms,
    total_step3_ms,
    is_fast_path: is_fast,
    is_slow_path: is_slow,
    is_timeout,
    verdict: change_verdict.to_string(),
    notes: format!(
      "play_evt={:?}, prop_evt={:?}",
      playback_info_event_ms.map(|v| format!("{:.1}ms", v)),
      media_properties_event_ms.map(|v| format!("{:.1}ms", v))
    ),
  })
}

fn restart_qqmusic_process() -> DriverResult<SmtcSession> {
  // 1. Kill any existing QQMusic processes
  let _ = Command::new("powershell")
    .args([
      "-Command",
      "Stop-Process -Name QQMusic -Force -ErrorAction SilentlyContinue",
    ])
    .output();
  let _ = Command::new("schtasks").args(["/end", "/tn", "RunQQMusic"]).output();

  std::thread::sleep(Duration::from_millis(1500));

  // 2. Launch via scheduled task in interactive desktop
  let _ = Command::new("schtasks").args(["/run", "/tn", "RunQQMusic"]).output();

  // 3. Poll for SMTC session up to 12 seconds
  let start = Instant::now();
  while start.elapsed() < Duration::from_millis(12000) {
    std::thread::sleep(Duration::from_millis(400));
    if let Ok(s) = get_session() {
      // Wait for session to have valid title
      if let Ok(meta) = s.track_metadata()
        && !meta.title.is_empty()
      {
        return Ok(s);
      }
    }
  }

  Err(DriverError::Backend {
    message: "Timeout waiting for restarted QQ Music SMTC session".to_string(),
  })
}

fn append_jsonl(path: &Path, records: &[SingleProbeRun]) {
  if let Some(p) = path.parent() {
    let _ = fs::create_dir_all(p);
  }
  let mut file = OpenOptions::new().create(true).append(true).open(path).expect("Failed to open output jsonl");

  for r in records {
    let s = serde_json::to_string(r).unwrap();
    writeln!(file, "{}", s).unwrap();
  }
}

fn print_group_summary(group_name: &str, runs: &[SingleProbeRun]) {
  let n = runs.len();
  if n == 0 {
    println!("{}: 0 runs", group_name);
    return;
  }
  let fast_count = runs.iter().filter(|r| r.is_fast_path).count();
  let slow_count = runs.iter().filter(|r| r.is_slow_path).count();
  let timeout_count = runs.iter().filter(|r| r.is_timeout).count();

  let mut verif_times: Vec<f64> = runs.iter().map(|r| r.verification_ms).collect();
  verif_times.sort_by(|a, b| a.partial_cmp(b).unwrap());

  let p50 = if n % 2 == 1 {
    verif_times[n / 2]
  } else {
    (verif_times[n / 2 - 1] + verif_times[n / 2]) / 2.0
  };
  let mean: f64 = verif_times.iter().sum::<f64>() / n as f64;
  let min = verif_times.first().copied().unwrap_or(0.0);
  let max = verif_times.last().copied().unwrap_or(0.0);

  println!(
    "[{:<25}] N={:<2} | Fast: {:>2} ({:>5.1}%) | Slow: {:>2} ({:>5.1}%) | Timeout: {:>2} | P50={:>6.1}ms | Mean={:>6.1}ms | Range={:.1}..{:.1}ms",
    group_name,
    n,
    fast_count,
    (fast_count as f64 / n as f64) * 100.0,
    slow_count,
    (slow_count as f64 / n as f64) * 100.0,
    timeout_count,
    p50,
    mean,
    min,
    max,
  );
}

pub(super) fn main() -> Result<(), Box<dyn std::error::Error>> {
  ensure_input_desktop();
  let args: Vec<String> = env::args().collect();

  let dimension = if args.len() > 1 {
    args[1].to_lowercase()
  } else {
    "all".to_string()
  };

  let out_path = Path::new("scratch/spike_verify_experiments.jsonl");

  println!("================================================================================");
  println!("QQ Music Verified Path Spike: Single-Variable Experiments");
  println!("Target Dimension: {}", dimension);
  println!("Output File     : {:?}", out_path);
  println!("================================================================================");

  let session = get_session().expect("QQ Music session not found. Please start QQ Music.");
  println!("Connected to SMTC: AppId='{}', Title='{}'", session.app_id(), session.track_metadata()?.title);

  let mut all_records: Vec<SingleProbeRun> = Vec::new();

  // --------------------------------------------------------------------------
  // Dimension 1: Skip Interval Dimension
  // --------------------------------------------------------------------------
  if dimension == "all" || dimension == "interval" {
    println!("\n>>> Running Dimension 1: Skip Interval Dimension");

    let intervals: [(&str, Duration, usize); 4] = [
      ("Group A (<200ms, 50ms pacing)", Duration::from_millis(50), 12),
      ("Group B (1s interval)", Duration::from_secs(1), 12),
      ("Group C (5s interval)", Duration::from_secs(5), 12),
      ("Group D (30s interval)", Duration::from_secs(30), 10),
    ];

    for (group_name, interval_dur, run_count) in intervals {
      println!("\n--- Testing {} (N={}) ---", group_name, run_count);
      let mut group_runs = Vec::new();

      for i in 1..=run_count {
        // Ensure playback before each skip
        let _ = ensure_playing(&session, Duration::from_millis(2000));

        let res = execute_single_skip_probe(&session, i, "skip_interval", group_name, true)?;
        println!(
          "  Run {:02}/{:02}: verif={:>6.1}ms (fast={}, slow={}) | prev='{}' -> new='{}' | {}",
          i, run_count, res.verification_ms, res.is_fast_path, res.is_slow_path, res.previous_title, res.new_title, res.notes
        );
        group_runs.push(res);

        if i < run_count && !interval_dur.is_zero() {
          std::thread::sleep(interval_dur);
        }
      }

      print_group_summary(group_name, &group_runs);
      append_jsonl(out_path, &group_runs);
      all_records.extend(group_runs);
    }
  }

  // --------------------------------------------------------------------------
  // Dimension 2: Lifecycle / Cache Dimension
  // --------------------------------------------------------------------------
  if dimension == "all" || dimension == "lifecycle" {
    println!("\n>>> Running Dimension 2: QQ Music Lifecycle / Cache Dimension");

    // Group 2A: Fresh process launch (10 runs, killing and restarting process each time)
    println!("\n--- Testing Group 2A: Fresh Process Launch (1st skip after launch, N=10) ---");
    let mut fresh_runs = Vec::new();
    for i in 1..=10 {
      print!("  [Fresh {:02}/10] Restarting QQ Music... ", i);
      std::io::stdout().flush().unwrap();
      let fresh_sess = restart_qqmusic_process()?;
      let _ = ensure_playing(&fresh_sess, Duration::from_millis(2500));
      // Short settling delay (500ms)
      std::thread::sleep(Duration::from_millis(500));

      let res = execute_single_skip_probe(&fresh_sess, i, "lifecycle", "Fresh Process (1st skip)", true)?;
      println!(
        "verif={:>6.1}ms (fast={}, slow={}) | '{}' -> '{}' | {}",
        res.verification_ms, res.is_fast_path, res.is_slow_path, res.previous_title, res.new_title, res.notes
      );
      fresh_runs.push(res);
    }
    print_group_summary("Fresh Process (1st skip)", &fresh_runs);
    append_jsonl(out_path, &fresh_runs);
    all_records.extend(fresh_runs);

    // Group 2B: After N consecutive skips on same warm process
    println!("\n--- Testing Group 2B: Consecutive Warm Skips after Warmup (N=10) ---");
    let warm_sess = get_session()?;
    let _ = ensure_playing(&warm_sess, Duration::from_millis(2000));
    // 3 warmup skips
    for _ in 0..3 {
      let _ = warm_sess.skip_next();
      std::thread::sleep(Duration::from_millis(800));
    }

    let mut warm_runs = Vec::new();
    for i in 1..=10 {
      let _ = ensure_playing(&warm_sess, Duration::from_millis(2000));
      std::thread::sleep(Duration::from_millis(800)); // 800ms between warm skips
      let res = execute_single_skip_probe(&warm_sess, i, "lifecycle", "Consecutive Skips (post-warmup)", true)?;
      println!(
        "  Run {:02}/10: verif={:>6.1}ms (fast={}, slow={}) | '{}' -> '{}' | {}",
        i, res.verification_ms, res.is_fast_path, res.is_slow_path, res.previous_title, res.new_title, res.notes
      );
      warm_runs.push(res);
    }
    print_group_summary("Consecutive Skips (post-warmup)", &warm_runs);
    append_jsonl(out_path, &warm_runs);
    all_records.extend(warm_runs);
  }

  // --------------------------------------------------------------------------
  // Dimension 3: Playback Position Dimension
  // --------------------------------------------------------------------------
  // --------------------------------------------------------------------------
  // Dimension 3: Playback Position Dimension
  // --------------------------------------------------------------------------
  if dimension == "all" || dimension == "position" || dimension == "middle" || dimension == "deep" {
    println!("\n>>> Running Dimension 3: Playback Position Dimension");

    let mut position_groups: Vec<(&str, u64, usize)> = Vec::new();
    if dimension == "all" || dimension == "position" {
      position_groups.push(("Track Start (play 2s then skip)", 2, 10));
      position_groups.push(("Track Middle (play 30s then skip)", 30, 10));
      position_groups.push(("Track Deep (play 60s then skip)", 60, 10));
    } else if dimension == "middle" {
      position_groups.push(("Track Middle (play 30s then skip)", 30, 10));
    } else if dimension == "deep" {
      position_groups.push(("Track Deep (play 60s then skip)", 60, 10));
    }

    for (group_name, play_secs, run_count) in position_groups {
      println!("\n--- Testing {} (N={}) ---", group_name, run_count);
      let mut group_runs = Vec::new();

      for i in 1..=run_count {
        let active_sess = get_session().unwrap_or_else(|_| session.clone());
        let _ = ensure_playing(&active_sess, Duration::from_millis(2000));
        print!("  Run {:02}/{:02}: Letting track play for {}s... ", i, run_count, play_secs);
        let _ = std::io::stdout().flush();
        std::thread::sleep(Duration::from_secs(play_secs));

        let sess_for_probe = get_session().unwrap_or_else(|_| active_sess.clone());
        match execute_single_skip_probe(&sess_for_probe, i, "playback_position", group_name, true) {
          Ok(res) => {
            println!(
              "verif={:>6.1}ms (fast={}, slow={}) | '{}' -> '{}' | {}",
              res.verification_ms, res.is_fast_path, res.is_slow_path, res.previous_title, res.new_title, res.notes
            );
            append_jsonl(out_path, std::slice::from_ref(&res));
            group_runs.push(res);
          }
          Err(e) => {
            eprintln!("Error on run {:02}: {:?}", i, e);
          }
        }
      }

      print_group_summary(group_name, &group_runs);
      all_records.extend(group_runs);
    }
  }

  println!("\n================================================================================");
  println!("Spike Experiments Completed!");
  println!("Total runs captured: {}", all_records.len());
  println!("Output saved to: {:?}", out_path);
  println!("================================================================================");

  Ok(())
}
