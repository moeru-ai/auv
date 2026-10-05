//! Replay harness for compiled QQ Music operation with hot-path optimizations.
//!
//! Executes the compiled operation ("qqmusic.prepare_playback") repeatedly
//! with strictly ZERO VLM calls and ZERO reasoning tokens.
//!
//! Supports three execution modes:
//! - `baseline`: Unoptimized execution (with correctness fixes applied: no 1.2s auto-retry)
//! - `verified`: Optimized execution with semantic title change confirmation
//! - `fast`: Optimized execution with action dispatch only (eventual consistency)
//!
//! Measures four non-overlapping split latency metrics:
//! - `discovery_ms`: Resource lookup & resolution (SMTC session, window, CoreAudio volume)
//! - `dispatch_ms`: Action command dispatches (play, volume set, skip_next)
//! - `verification_ms`: Semantic verification gates & polling (playback status, title change)
//! - `wgc_ms`: WGC window health verification
#![cfg(target_os = "windows")]

use auv_driver_common::error::{DriverError, DriverResult};
use auv_driver_common::window::Window;
use auv_driver_windows::desktop::ensure_input_desktop;
use auv_driver_windows::media::{AudioVolumeController, MediaPlaybackStatus, ProcessAudioVolume, SmtcMediaManager, SmtcSession};
use auv_driver_windows::wgc::{capture_window_health, capture_window_wgc};
use auv_driver_windows::window::list_windows;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{IsIconic, SW_RESTORE, ShowWindow};

#[derive(Debug, Serialize, Deserialize)]
struct CompiledOperation {
  schema_version: String,
  name: String,
  description: String,
  compilation_metadata: CompilationMetadata,
  target: TargetMetadata,
  steps: Vec<OperationStepDef>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CompilationMetadata {
  compiler: String,
  source_record: String,
  date: String,
  crux_goal: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct TargetMetadata {
  app_name: String,
  backend: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct OperationStepDef {
  id: String,
  name: String,
  description: String,
  action: serde_json::Value,
  verification_gate: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct StepRecord {
  step_id: String,
  duration_ms: f64,
  success: bool,
  gate_passed: bool,
  escalation: Option<String>,
  details: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ReplayRecord {
  iteration: usize,
  timestamp: String,
  operation: String,
  mode: String,
  vlm_calls: usize,
  tokens_used: usize,
  total_duration_ms: f64,
  discovery_ms: f64,
  dispatch_ms: f64,
  verification_ms: f64,
  wgc_ms: f64,
  success: bool,
  confirmed: bool,
  fault_injected: Option<String>,
  escalated_to_vlm: bool,
  steps: Vec<StepRecord>,
}

/// Holds resolved resources for a single operation execution lifecycle.
struct WindowsOperationContext {
  session: SmtcSession,
  #[allow(dead_code)]
  pid: u32,
  window: Window,
  audio: Option<ProcessAudioVolume>,
}

impl WindowsOperationContext {
  fn resolve() -> DriverResult<Self> {
    let manager = SmtcMediaManager::new()?;
    let session = manager.find_session("qqmusic")?.ok_or_else(|| DriverError::NotFound {
      target: "QQ Music SMTC session".to_string(),
    })?;

    let windows = list_windows()?;
    let window = windows.into_iter().find(|w| w.app_name.as_deref() == Some("QQMusic.exe")).ok_or_else(|| DriverError::NotFound {
      target: "QQMusic.exe window".to_string(),
    })?;

    let pid = window.process_id.unwrap_or(0);
    let audio = if pid > 0 {
      AudioVolumeController::open_process(pid).ok()
    } else {
      None
    };

    Ok(Self {
      session,
      pid,
      window,
      audio,
    })
  }
}

fn get_qq_session_and_window_baseline() -> DriverResult<(SmtcSession, u32, Window)> {
  let manager = SmtcMediaManager::new()?;
  let session = manager.find_session("qqmusic")?.ok_or_else(|| DriverError::NotFound {
    target: "QQ Music SMTC session".to_string(),
  })?;

  let windows = list_windows()?;
  let qq_win = windows.into_iter().find(|w| w.app_name.as_deref() == Some("QQMusic.exe")).ok_or_else(|| DriverError::NotFound {
    target: "QQMusic.exe window".to_string(),
  })?;

  let pid = qq_win.process_id.unwrap_or(0);

  Ok((session, pid, qq_win))
}

// ==============================================================================
// BASELINE EXECUTION (Unoptimized with correctness fixes: no 1.2s auto-retry)
// ==============================================================================
fn execute_replay_baseline(iteration: usize, fault_injection: Option<&str>, allow_restore: bool) -> DriverResult<ReplayRecord> {
  let start_time = Instant::now();
  let mut steps = Vec::new();
  let mut overall_success = true;
  let mut escalated = false;

  // Discovery phase
  let t_disc = Instant::now();
  let (session, pid, qq_win) = get_qq_session_and_window_baseline()?;
  let vol = if pid > 0 {
    AudioVolumeController::get_process_volume(pid).ok()
  } else {
    None
  };
  let discovery_ms = t_disc.elapsed().as_secs_f64() * 1000.0;

  // --------------------------------------------------------------------------
  // Step 1: Query Playback State
  // --------------------------------------------------------------------------
  let s1_start = Instant::now();
  let meta = session.track_metadata()?;
  let status = session.playback_status()?;
  let s1_verif_ms = s1_start.elapsed().as_secs_f64() * 1000.0;

  let s1_gate_passed = !meta.title.is_empty();
  if !s1_gate_passed {
    overall_success = false;
    escalated = true;
  }
  steps.push(StepRecord {
    step_id: "step_1_query_state".to_string(),
    duration_ms: discovery_ms + s1_verif_ms,
    success: true,
    gate_passed: s1_gate_passed,
    escalation: if s1_gate_passed {
      None
    } else {
      Some("would escalate to VLM".to_string())
    },
    details: serde_json::json!({
      "title": meta.title,
      "artist": meta.artist,
      "status": format!("{:?}", status),
      "volume": vol,
    }),
  });

  // --------------------------------------------------------------------------
  // Step 2: Ensure Playing & Volume 40% (Unoptimized: always writes)
  // --------------------------------------------------------------------------
  let s2_start = Instant::now();
  let target_vol = 0.40f32;

  let t_s2_disp = Instant::now();
  if fault_injection == Some("volume") {
    if pid > 0 {
      let _ = AudioVolumeController::set_process_volume(pid, 0.10);
    }
  } else if pid > 0 {
    AudioVolumeController::set_process_volume(pid, target_vol)?;
  }

  if fault_injection == Some("pause") {
    let _ = session.pause();
    std::thread::sleep(Duration::from_millis(150));
  } else {
    let current_st = session.playback_status().unwrap_or(MediaPlaybackStatus::Closed);
    if current_st != MediaPlaybackStatus::Playing && current_st != MediaPlaybackStatus::Changing {
      let _ = session.play();
    }
  }
  let s2_disp_ms = t_s2_disp.elapsed().as_secs_f64() * 1000.0;

  // Polling with fixed 80ms sleep
  let t_s2_verif = Instant::now();
  let s2_timeout = if fault_injection.is_some() {
    Duration::from_millis(200)
  } else {
    Duration::from_millis(2000)
  };
  let mut s2_status = session.playback_status()?;
  let s2_poll_start = Instant::now();
  while s2_poll_start.elapsed() < s2_timeout {
    if s2_status == MediaPlaybackStatus::Playing {
      break;
    }
    std::thread::sleep(Duration::from_millis(80));
    if let Ok(st) = session.playback_status() {
      s2_status = st;
      if s2_status == MediaPlaybackStatus::Paused && fault_injection.is_none() {
        let _ = session.play();
      }
    }
  }
  let s2_vol = if pid > 0 {
    AudioVolumeController::get_process_volume(pid).unwrap_or(0.0)
  } else {
    0.0
  };
  let s2_verif_ms = t_s2_verif.elapsed().as_secs_f64() * 1000.0;
  let s2_dur = s2_start.elapsed().as_secs_f64() * 1000.0;

  let vol_ok = (s2_vol - target_vol).abs() <= 0.05;
  let status_ok = s2_status == MediaPlaybackStatus::Playing;
  let s2_gate_passed = vol_ok && status_ok;

  if !s2_gate_passed {
    overall_success = false;
    escalated = true;
  }

  steps.push(StepRecord {
    step_id: "step_2_ensure_playing_and_volume".to_string(),
    duration_ms: s2_dur,
    success: true,
    gate_passed: s2_gate_passed,
    escalation: if s2_gate_passed {
      None
    } else {
      Some("would escalate to VLM".to_string())
    },
    details: serde_json::json!({
      "status": format!("{:?}", s2_status),
      "volume": s2_vol,
      "target_volume": target_vol,
      "vol_check": vol_ok,
      "status_check": status_ok,
    }),
  });

  // --------------------------------------------------------------------------
  // Step 3: Skip Next Track (Unoptimized: fixed 80ms sleep, NO 1.2s auto-retry)
  // --------------------------------------------------------------------------
  let s3_start = Instant::now();
  let prev_meta = session.track_metadata()?;
  let t_s3_disp = Instant::now();
  let _ = session.skip_next();
  let s3_disp_ms = t_s3_disp.elapsed().as_secs_f64() * 1000.0;

  let t_s3_verif = Instant::now();
  let s3_timeout = Duration::from_millis(3000);
  let mut new_title = prev_meta.title.clone();
  let s3_poll_start = Instant::now();
  while s3_poll_start.elapsed() < s3_timeout {
    std::thread::sleep(Duration::from_millis(80));
    if let Ok(curr) = session.track_metadata()
      && curr.title != prev_meta.title
      && !curr.title.is_empty()
    {
      new_title = curr.title;
      break;
    }
  }
  let s3_verif_ms = t_s3_verif.elapsed().as_secs_f64() * 1000.0;
  let s3_dur = s3_start.elapsed().as_secs_f64() * 1000.0;
  let s3_gate_passed = new_title != prev_meta.title;

  if !s3_gate_passed {
    overall_success = false;
    escalated = true;
  }

  steps.push(StepRecord {
    step_id: "step_3_skip_next_track".to_string(),
    duration_ms: s3_dur,
    success: true,
    gate_passed: s3_gate_passed,
    escalation: if s3_gate_passed {
      None
    } else {
      Some("would escalate to VLM".to_string())
    },
    details: serde_json::json!({
      "previous_title": prev_meta.title,
      "new_title": new_title,
      "title_changed": s3_gate_passed,
      "dispatch_ms": s3_disp_ms,
      "verification_ms": s3_verif_ms,
    }),
  });

  // --------------------------------------------------------------------------
  // Step 4: Verify Window Alive (Unoptimized full WGC capture)
  // --------------------------------------------------------------------------
  let s4_start = Instant::now();
  let hwnd_opt = qq_win.reference.id.parse::<isize>().ok().map(|h| HWND(h as _));
  let is_minimized = hwnd_opt.map(|hwnd| unsafe { IsIconic(hwnd).as_bool() }).unwrap_or(false);

  if is_minimized
    && allow_restore
    && let Some(hwnd) = hwnd_opt
  {
    unsafe {
      let _ = ShowWindow(hwnd, SW_RESTORE);
      std::thread::sleep(Duration::from_millis(150));
    }
  }

  let is_still_minimized = hwnd_opt.map(|hwnd| unsafe { IsIconic(hwnd).as_bool() }).unwrap_or(false);

  let (s4_dur, s4_gate_passed, s4_details) = if is_still_minimized {
    let dur = s4_start.elapsed().as_secs_f64() * 1000.0;
    (
      dur,
      true,
      serde_json::json!({
        "status": "skipped_minimized",
        "reason": "window_minimized (zero-window-mutation redline preserves user state)",
        "alive": true,
      }),
    )
  } else {
    match capture_window_wgc(&qq_win) {
      Ok(cap) => {
        let total = (cap.image.width() * cap.image.height()) as usize;
        let raw = cap.image.as_raw();
        let mut non_black = 0usize;
        for chunk in raw.chunks_exact(4) {
          if chunk[0] > 10 || chunk[1] > 10 || chunk[2] > 10 {
            non_black += 1;
          }
        }
        let ratio = if total > 0 {
          non_black as f64 / total as f64 * 100.0
        } else {
          0.0
        };
        let alive = ratio >= 50.0;
        let dur = s4_start.elapsed().as_secs_f64() * 1000.0;
        (
          dur,
          alive,
          serde_json::json!({
            "status": "captured_full",
            "width": cap.image.width(),
            "height": cap.image.height(),
            "non_black_ratio": ratio,
            "alive": alive,
          }),
        )
      }
      Err(e) => {
        let dur = s4_start.elapsed().as_secs_f64() * 1000.0;
        (
          dur,
          false,
          serde_json::json!({
            "status": "error",
            "error": format!("{e:?}"),
            "alive": false,
          }),
        )
      }
    }
  };

  if !s4_gate_passed {
    overall_success = false;
    escalated = true;
  }

  steps.push(StepRecord {
    step_id: "step_4_verify_window_alive".to_string(),
    duration_ms: s4_dur,
    success: true,
    gate_passed: s4_gate_passed,
    escalation: if s4_gate_passed {
      None
    } else {
      Some("would escalate to VLM".to_string())
    },
    details: s4_details,
  });

  let total_duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;
  let dispatch_ms = s2_disp_ms + s3_disp_ms;
  let verification_ms = s1_verif_ms + s2_verif_ms + s3_verif_ms;
  let wgc_ms = s4_dur;

  Ok(ReplayRecord {
    iteration,
    timestamp: chrono_now_iso(),
    operation: "qqmusic.prepare_playback".to_string(),
    mode: "baseline".to_string(),
    vlm_calls: 0,
    tokens_used: 0,
    total_duration_ms,
    discovery_ms,
    dispatch_ms,
    verification_ms,
    wgc_ms,
    success: overall_success,
    confirmed: s3_gate_passed,
    fault_injected: fault_injection.map(ToString::to_string),
    escalated_to_vlm: escalated,
    steps,
  })
}

// ==============================================================================
// OPTIMIZED EXECUTION (Context reuse + Idempotency + Event/Adaptive polling + Lightweight WGC)
// ==============================================================================
fn execute_replay_optimized(
  iteration: usize,
  fault_injection: Option<&str>,
  allow_restore: bool,
  is_fast_mode: bool,
) -> DriverResult<ReplayRecord> {
  let start_time = Instant::now();
  let mut steps = Vec::new();
  let mut overall_success = true;
  let mut escalated = false;

  // Single operation context resolution
  let t_disc = Instant::now();
  let ctx = WindowsOperationContext::resolve()?;
  let discovery_ms = t_disc.elapsed().as_secs_f64() * 1000.0;

  // --------------------------------------------------------------------------
  // Step 1: Query Playback State (reusing resolved context)
  // --------------------------------------------------------------------------
  let s1_start = Instant::now();
  let meta = ctx.session.track_metadata()?;
  let status = ctx.session.playback_status()?;
  let vol = ctx.audio.as_ref().and_then(|a| a.get_volume().ok());
  let s1_verif_ms = s1_start.elapsed().as_secs_f64() * 1000.0;

  let s1_gate_passed = !meta.title.is_empty();
  if !s1_gate_passed {
    overall_success = false;
    escalated = true;
  }
  steps.push(StepRecord {
    step_id: "step_1_query_state".to_string(),
    duration_ms: s1_verif_ms,
    success: true,
    gate_passed: s1_gate_passed,
    escalation: if s1_gate_passed {
      None
    } else {
      Some("would escalate to VLM".to_string())
    },
    details: serde_json::json!({
      "title": meta.title,
      "artist": meta.artist,
      "status": format!("{:?}", status),
      "volume": vol,
    }),
  });

  // --------------------------------------------------------------------------
  // Step 2: Ensure Playing & Volume 40% (Idempotency Guard)
  // --------------------------------------------------------------------------
  let s2_start = Instant::now();
  let target_vol = 0.40f32;
  let mut skipped_volume_write = false;
  let mut skipped_play_write = false;

  let t_s2_disp = Instant::now();
  if fault_injection == Some("volume") {
    if let Some(ref audio) = ctx.audio {
      let _ = audio.set_volume(0.10);
    }
  } else {
    let curr_vol = ctx.audio.as_ref().and_then(|a| a.get_volume().ok()).unwrap_or(0.0);
    if (curr_vol - target_vol).abs() <= 0.05 {
      skipped_volume_write = true;
    } else if let Some(ref audio) = ctx.audio {
      audio.set_volume(target_vol)?;
    }
  }

  if fault_injection == Some("pause") {
    let _ = ctx.session.pause();
    std::thread::sleep(Duration::from_millis(150));
  } else {
    let current_st = ctx.session.playback_status().unwrap_or(MediaPlaybackStatus::Closed);
    if current_st == MediaPlaybackStatus::Playing {
      skipped_play_write = true;
    } else if current_st != MediaPlaybackStatus::Changing {
      let _ = ctx.session.play();
    }
  }
  let s2_disp_ms = t_s2_disp.elapsed().as_secs_f64() * 1000.0;

  // Verification Gate 2: Adaptive backoff polling
  let t_s2_verif = Instant::now();
  let mut s2_status = ctx.session.playback_status()?;
  if !skipped_play_write || fault_injection.is_some() {
    let s2_timeout = if fault_injection.is_some() {
      Duration::from_millis(200)
    } else {
      Duration::from_millis(2000)
    };
    let s2_poll_start = Instant::now();
    let mut backoff = Duration::from_millis(10);
    while s2_poll_start.elapsed() < s2_timeout {
      if s2_status == MediaPlaybackStatus::Playing {
        break;
      }
      std::thread::sleep(backoff);
      backoff = (backoff * 2).min(Duration::from_millis(80));
      if let Ok(st) = ctx.session.playback_status() {
        s2_status = st;
        if s2_status == MediaPlaybackStatus::Paused && fault_injection.is_none() {
          let _ = ctx.session.play();
        }
      }
    }
  }
  let s2_vol = ctx.audio.as_ref().and_then(|a| a.get_volume().ok()).unwrap_or(0.0);
  let s2_verif_ms = t_s2_verif.elapsed().as_secs_f64() * 1000.0;
  let s2_dur = s2_start.elapsed().as_secs_f64() * 1000.0;

  let vol_ok = (s2_vol - target_vol).abs() <= 0.05;
  let status_ok = s2_status == MediaPlaybackStatus::Playing;
  let s2_gate_passed = vol_ok && status_ok;

  if !s2_gate_passed {
    overall_success = false;
    escalated = true;
  }

  steps.push(StepRecord {
    step_id: "step_2_ensure_playing_and_volume".to_string(),
    duration_ms: s2_dur,
    success: true,
    gate_passed: s2_gate_passed,
    escalation: if s2_gate_passed {
      None
    } else {
      Some("would escalate to VLM".to_string())
    },
    details: serde_json::json!({
      "status": format!("{:?}", s2_status),
      "volume": s2_vol,
      "target_volume": target_vol,
      "skipped_volume_write": skipped_volume_write,
      "skipped_play_write": skipped_play_write,
      "vol_check": vol_ok,
      "status_check": status_ok,
    }),
  });

  // --------------------------------------------------------------------------
  // Step 3: Skip Next Track (Action/Verification separation)
  // --------------------------------------------------------------------------
  let s3_start = Instant::now();
  let prev_meta = ctx.session.track_metadata()?;

  // Dispatch action
  let t_s3_disp = Instant::now();
  let _ = ctx.session.skip_next();
  let s3_disp_ms = t_s3_disp.elapsed().as_secs_f64() * 1000.0;

  let (s3_verif_ms, s3_gate_passed, confirmed, new_title) = if is_fast_mode {
    // Fast / action mode: command dispatched successfully, return immediately
    (0.0, true, false, prev_meta.title.clone())
  } else {
    // Verified mode: event listener + adaptive backoff polling
    let t_s3_verif = Instant::now();
    let (tx, rx) = std::sync::mpsc::sync_channel::<()>(4);
    let token = ctx
      .session
      .on_media_properties_changed(move || {
        let _ = tx.try_send(());
      })
      .ok();

    let s3_timeout = Duration::from_millis(3000);
    let mut updated_title = prev_meta.title.clone();
    let mut backoff = Duration::from_millis(10);

    while t_s3_verif.elapsed() < s3_timeout {
      // Sleep until event arrives or backoff interval elapses
      let _ = rx.recv_timeout(backoff);

      if let Ok(curr) = ctx.session.track_metadata()
        && curr.title != prev_meta.title
        && !curr.title.is_empty()
      {
        updated_title = curr.title;
        break;
      }

      backoff = (backoff * 2).min(Duration::from_millis(80));
    }

    if let Some(tok) = token {
      let _ = ctx.session.remove_media_properties_changed(tok);
    }

    let verif_ms = t_s3_verif.elapsed().as_secs_f64() * 1000.0;
    let title_changed = updated_title != prev_meta.title;
    (verif_ms, title_changed, title_changed, updated_title)
  };

  let s3_dur = s3_start.elapsed().as_secs_f64() * 1000.0;
  if !s3_gate_passed {
    overall_success = false;
    escalated = true;
  }

  steps.push(StepRecord {
    step_id: "step_3_skip_next_track".to_string(),
    duration_ms: s3_dur,
    success: true,
    gate_passed: s3_gate_passed,
    escalation: if s3_gate_passed {
      None
    } else {
      Some("would escalate to VLM".to_string())
    },
    details: serde_json::json!({
      "previous_title": prev_meta.title,
      "new_title": new_title,
      "confirmed": confirmed,
      "fast_mode": is_fast_mode,
      "dispatch_ms": s3_disp_ms,
      "verification_ms": s3_verif_ms,
    }),
  });

  // --------------------------------------------------------------------------
  // Step 4: Verify Window Alive (Lightweight WGC health check)
  // --------------------------------------------------------------------------
  let s4_start = Instant::now();
  let hwnd_opt = ctx.window.reference.id.parse::<isize>().ok().map(|h| HWND(h as _));
  let is_minimized = hwnd_opt.map(|hwnd| unsafe { IsIconic(hwnd).as_bool() }).unwrap_or(false);

  if is_minimized
    && allow_restore
    && let Some(hwnd) = hwnd_opt
  {
    unsafe {
      let _ = ShowWindow(hwnd, SW_RESTORE);
      std::thread::sleep(Duration::from_millis(150));
    }
  }

  let is_still_minimized = hwnd_opt.map(|hwnd| unsafe { IsIconic(hwnd).as_bool() }).unwrap_or(false);

  let (s4_dur, s4_gate_passed, s4_details) = if is_still_minimized {
    let dur = s4_start.elapsed().as_secs_f64() * 1000.0;
    (
      dur,
      true,
      serde_json::json!({
        "status": "skipped_minimized",
        "reason": "window_minimized (zero-window-mutation redline preserves user state)",
        "alive": true,
      }),
    )
  } else {
    match capture_window_health(&ctx.window) {
      Ok(health) => {
        let dur = s4_start.elapsed().as_secs_f64() * 1000.0;
        (
          dur,
          health.alive,
          serde_json::json!({
            "status": "captured_health",
            "width": health.width,
            "height": health.height,
            "non_black_ratio": health.non_black_ratio,
            "is_fresh": health.is_fresh,
            "alive": health.alive,
          }),
        )
      }
      Err(e) => {
        let dur = s4_start.elapsed().as_secs_f64() * 1000.0;
        (
          dur,
          false,
          serde_json::json!({
            "status": "error",
            "error": format!("{e:?}"),
            "alive": false,
          }),
        )
      }
    }
  };

  if !s4_gate_passed {
    overall_success = false;
    escalated = true;
  }

  steps.push(StepRecord {
    step_id: "step_4_verify_window_alive".to_string(),
    duration_ms: s4_dur,
    success: true,
    gate_passed: s4_gate_passed,
    escalation: if s4_gate_passed {
      None
    } else {
      Some("would escalate to VLM".to_string())
    },
    details: s4_details,
  });

  let total_duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;
  let dispatch_ms = s2_disp_ms + s3_disp_ms;
  let verification_ms = s1_verif_ms + s2_verif_ms + s3_verif_ms;
  let wgc_ms = s4_dur;

  Ok(ReplayRecord {
    iteration,
    timestamp: chrono_now_iso(),
    operation: "qqmusic.prepare_playback".to_string(),
    mode: if is_fast_mode {
      "fast".to_string()
    } else {
      "verified".to_string()
    },
    vlm_calls: 0,
    tokens_used: 0,
    total_duration_ms,
    discovery_ms,
    dispatch_ms,
    verification_ms,
    wgc_ms,
    success: overall_success,
    confirmed,
    fault_injected: fault_injection.map(ToString::to_string),
    escalated_to_vlm: escalated,
    steps,
  })
}

fn chrono_now_iso() -> String {
  let now = std::time::SystemTime::now();
  let dur = now.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
  let secs = dur.as_secs();
  let millis = dur.subsec_millis();
  format!("{}.{:03}Z", secs, millis)
}

fn percentile(mut vals: Vec<f64>, p: f64) -> f64 {
  if vals.is_empty() {
    return 0.0;
  }
  vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
  let idx = ((vals.len() as f64 - 1.0) * p).round() as usize;
  vals[idx.min(vals.len() - 1)]
}

fn main() {
  ensure_input_desktop();

  let args: Vec<String> = env::args().collect();
  let mut replays_count = 20usize;
  let mut mode = "verified".to_string(); // "baseline", "verified", "fast"
  let mut output_file: Option<String> = None;
  let mut fault_inject: Option<String> = None;
  let mut allow_restore = false;

  let mut i = 1;
  while i < args.len() {
    match args[i].as_str() {
      "--mode" => {
        if i + 1 < args.len() {
          mode = args[i + 1].to_lowercase();
          i += 1;
        }
      }
      "--replays" => {
        if i + 1 < args.len() {
          replays_count = args[i + 1].parse().unwrap_or(20);
          i += 1;
        }
      }
      "--output" => {
        if i + 1 < args.len() {
          output_file = Some(args[i + 1].clone());
          i += 1;
        }
      }
      "--fault-inject" => {
        if i + 1 < args.len() {
          fault_inject = Some(args[i + 1].clone());
          i += 1;
        }
      }
      "--allow-restore" => {
        allow_restore = true;
      }
      _ => {}
    }
    i += 1;
  }

  let final_output = output_file.unwrap_or_else(|| match mode.as_str() {
    "baseline" => "docs/ai/references/driver/2026-10-04-windows-hotpath-baseline-20x.jsonl".to_string(),
    "fast" => "docs/ai/references/driver/2026-10-04-windows-hotpath-optimized-fast-20x.jsonl".to_string(),
    _ => "docs/ai/references/driver/2026-10-04-windows-hotpath-optimized-verified-20x.jsonl".to_string(),
  });

  let op_file_path = "docs/ai/references/driver/qqmusic-prepared-playback.json";
  let op_json = fs::read_to_string(op_file_path)
    .or_else(|_| fs::read_to_string(Path::new("..").join("..").join(op_file_path)))
    .unwrap_or_else(|e| panic!("Failed to read compiled operation JSON at {op_file_path}: {e}"));
  let compiled_op: CompiledOperation =
    serde_json::from_str(&op_json).unwrap_or_else(|e| panic!("Failed to parse compiled operation JSON: {e}"));

  println!("================================================================================");
  println!("QQ Music Hotpath Optimization Replay Harness (Zero VLM / Zero Token)");
  println!("================================================================================");
  println!("Target Operation : {} ({})", compiled_op.name, compiled_op.schema_version);
  println!("Execution Mode   : {}", mode);
  println!("Compiler Source  : {}", compiled_op.compilation_metadata.compiler);
  println!("Replays Count    : {}", replays_count);
  println!("Output JSONL     : {}", final_output);
  println!("Fault Injection  : {:?}", fault_inject);
  println!("Allow Restore    : {} (default false, zero-window-mutation redline)", allow_restore);
  println!("VLM Invocations  : 0 (hard constraint)");
  println!("Token Budget     : 0 (hard constraint)");
  println!("--------------------------------------------------------------------------------");

  if let Some(ref fi) = fault_inject {
    println!("[FAULT INJECTION MODE] Injecting fault: {}", fi);
    let record = match mode.as_str() {
      "baseline" => execute_replay_baseline(1, Some(fi), allow_restore),
      "fast" => execute_replay_optimized(1, Some(fi), allow_restore, true),
      _ => execute_replay_optimized(1, Some(fi), allow_restore, false),
    }
    .expect("Failed to execute fault injection replay");

    println!("Iteration 1: success={}, escalated_to_vlm={}", record.success, record.escalated_to_vlm);
    for step in &record.steps {
      println!(
        "  Step {:<30} | gate_passed={:<5} | dur={:>6.2}ms | escalation={:?}",
        step.step_id, step.gate_passed, step.duration_ms, step.escalation
      );
    }
    println!("\nVerification Gate Trigger Summary:");
    println!("  Gate caught mismatch: {}", !record.success);
    println!("  Escalation marked   : {}", record.escalated_to_vlm);
    println!("  VLM Calls           : 0 (only marked 'would escalate to VLM', no real call)");
    return;
  }

  // Normal execution
  let mut records = Vec::new();
  let parent_dir = Path::new(&final_output).parent().unwrap_or(Path::new("."));
  let _ = fs::create_dir_all(parent_dir);

  let mut jsonl_file =
    OpenOptions::new().create(true).write(true).truncate(true).open(&final_output).expect("Failed to open output jsonl file");

  let mut discovery_durs = Vec::new();
  let mut dispatch_durs = Vec::new();
  let mut verif_durs = Vec::new();
  let mut wgc_durs = Vec::new();
  let mut total_durs = Vec::new();
  let mut s1_durs = Vec::new();
  let mut s2_durs = Vec::new();
  let mut s3_durs = Vec::new();
  let mut s4_durs = Vec::new();
  let mut successes = 0usize;

  for iter in 1..=replays_count {
    print!("[Replay {:02}/{:02}] Executing... ", iter, replays_count);
    std::io::stdout().flush().unwrap();

    let res = match mode.as_str() {
      "baseline" => execute_replay_baseline(iter, None, allow_restore),
      "fast" => execute_replay_optimized(iter, None, allow_restore, true),
      _ => execute_replay_optimized(iter, None, allow_restore, false),
    };

    let record = match res {
      Ok(r) => r,
      Err(e) => {
        println!("FAILED: {e:?}");
        continue;
      }
    };

    if record.success {
      successes += 1;
    }

    discovery_durs.push(record.discovery_ms);
    dispatch_durs.push(record.dispatch_ms);
    verif_durs.push(record.verification_ms);
    wgc_durs.push(record.wgc_ms);
    total_durs.push(record.total_duration_ms);

    s1_durs.push(record.steps[0].duration_ms);
    s2_durs.push(record.steps[1].duration_ms);
    s3_durs.push(record.steps[2].duration_ms);
    s4_durs.push(record.steps[3].duration_ms);

    let json_line = serde_json::to_string(&record).unwrap();
    writeln!(jsonl_file, "{}", json_line).unwrap();

    println!(
      "OK ({:.1}ms) | Disc={:.1}ms, Disp={:.1}ms, Verif={:.1}ms, WGC={:.1}ms | tokens=0",
      record.total_duration_ms, record.discovery_ms, record.dispatch_ms, record.verification_ms, record.wgc_ms
    );

    records.push(record);
    std::thread::sleep(Duration::from_millis(50));
  }

  println!("--------------------------------------------------------------------------------");
  println!("Execution Summary (Mode: {}, N={}):", mode, records.len());
  println!("  Total Runs      : {}", replays_count);
  println!("  Successful Runs : {} ({:.1}%)", successes, (successes as f64 / replays_count as f64) * 100.0);
  println!("  VLM Invocations : 0 (实测 0 调用)");
  println!("  Tokens Used     : 0 (实测 0 token)");
  println!();
  println!("4-Way Split Metrics Benchmark:");
  println!("  | Metric            | P50 Latency | P95 Latency | Mean Latency | Description |");
  println!("  |-------------------|-------------|-------------|--------------|-------------|");
  println!(
    "  | discovery_ms      | {:>9.2}ms | {:>9.2}ms | {:>10.2}ms | SMTC & window & Audio resolution |",
    percentile(discovery_durs.clone(), 0.50),
    percentile(discovery_durs.clone(), 0.95),
    discovery_durs.iter().sum::<f64>() / discovery_durs.len() as f64
  );
  println!(
    "  | dispatch_ms       | {:>9.2}ms | {:>9.2}ms | {:>10.2}ms | WinRT/CoreAudio command calls    |",
    percentile(dispatch_durs.clone(), 0.50),
    percentile(dispatch_durs.clone(), 0.95),
    dispatch_durs.iter().sum::<f64>() / dispatch_durs.len() as f64
  );
  println!(
    "  | verification_ms   | {:>9.2}ms | {:>9.2}ms | {:>10.2}ms | Semantic check & polling latency |",
    percentile(verif_durs.clone(), 0.50),
    percentile(verif_durs.clone(), 0.95),
    verif_durs.iter().sum::<f64>() / verif_durs.len() as f64
  );
  println!(
    "  | wgc_ms            | {:>9.2}ms | {:>9.2}ms | {:>10.2}ms | WGC window alive check           |",
    percentile(wgc_durs.clone(), 0.50),
    percentile(wgc_durs.clone(), 0.95),
    wgc_durs.iter().sum::<f64>() / wgc_durs.len() as f64
  );
  println!("  |-------------------|-------------|-------------|--------------|-------------|");
  println!(
    "  | total_duration_ms | {:>9.2}ms | {:>9.2}ms | {:>10.2}ms | End-to-end operation latency     |",
    percentile(total_durs.clone(), 0.50),
    percentile(total_durs.clone(), 0.95),
    total_durs.iter().sum::<f64>() / total_durs.len() as f64
  );
  println!();
  println!("Per-Step Breakdown:");
  println!(
    "  - Step 1 (Query State): P50={:.2}ms, P95={:.2}ms, Mean={:.2}ms",
    percentile(s1_durs.clone(), 0.50),
    percentile(s1_durs.clone(), 0.95),
    s1_durs.iter().sum::<f64>() / s1_durs.len() as f64
  );
  println!(
    "  - Step 2 (Play & Vol):  P50={:.2}ms, P95={:.2}ms, Mean={:.2}ms",
    percentile(s2_durs.clone(), 0.50),
    percentile(s2_durs.clone(), 0.95),
    s2_durs.iter().sum::<f64>() / s2_durs.len() as f64
  );
  println!(
    "  - Step 3 (Skip Next):   P50={:.2}ms, P95={:.2}ms, Mean={:.2}ms",
    percentile(s3_durs.clone(), 0.50),
    percentile(s3_durs.clone(), 0.95),
    s3_durs.iter().sum::<f64>() / s3_durs.len() as f64
  );
  println!(
    "  - Step 4 (WGC Alive):   P50={:.2}ms, P95={:.2}ms, Mean={:.2}ms",
    percentile(s4_durs.clone(), 0.50),
    percentile(s4_durs.clone(), 0.95),
    s4_durs.iter().sum::<f64>() / s4_durs.len() as f64
  );
  println!("================================================================================");
}
