//! Replay harness for compiled QQ Music operation with hot-path optimizations and granular profiling.
//!
//! Executes the compiled operation ("qqmusic.prepare_playback") repeatedly
//! with strictly ZERO VLM calls and ZERO reasoning tokens.
//!
//! Supports three execution modes:
//! - `baseline`: Unoptimized execution (with correctness fixes applied: no 1.2s auto-retry)
//! - `verified`: Optimized execution with semantic title change confirmation
//! - `fast`: Optimized execution with action dispatch only (eventual consistency)
//!
//! Measures ten granular split latency metrics:
//! - `manager_discovery_ms`: SMTC session manager acquisition
//! - `session_discovery_ms`: `GetCurrentSession()` fast-path vs `GetSessions()` scan
//! - `window_discovery_ms`: HWND enumeration & matching
//! - `audio_lookup_ms`: CoreAudio endpoint + ISimpleAudioVolume resolution
//! - `volume_rw_ms`: Volume read/write duration in Step 2
//! - `dispatch_ms`: SMTC command dispatch latency
//! - `verification_ms`: Metadata refresh wait & identity confirmation
//! - `wgc_ms`: WGC window health check
//! - `serialization_ms`: JSONL serialization time
//! - `pacing_ms`: Inter-iteration pacing (strictly isolated from total_duration_ms)
#![cfg(target_os = "windows")]

use auv_driver_common::error::{DriverError, DriverResult};
use auv_driver_common::window::Window;
use auv_driver_windows::desktop::ensure_input_desktop;
use auv_driver_windows::media::{
  AudioLookupStats, AudioLookupStatus, AudioVolumeController, MediaPlaybackStatus, ProcessAudioVolume, SmtcMediaManager, SmtcSession,
};
use auv_driver_windows::playback_guard::{
  DEFAULT_PLAY_POLL_TIMEOUT, DEFAULT_TARGET_VOLUME, DEFAULT_VOLUME_TOLERANCE, Step2Options, execute_step2_real_with_prestate,
};
use auv_driver_windows::track_identity::{TrackChangeVerdict, TrackIdentity, evaluate_track_change};
use auv_driver_windows::wgc::{
  FastWindowVerification, capture_window_health_cached, capture_window_health_strict, capture_window_wgc, check_window_liveness,
  prewarm_wgc, prewarm_wgc_window, reset_d3d_context,
};
use auv_driver_windows::window::list_windows;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{IsIconic, SW_RESTORE, ShowWindow};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ExecutionMode {
  Fast,
  Verified,
}

#[derive(Debug, Serialize, Deserialize)]
struct CompiledOperation {
  schema_version: String,
  name: String,
  description: String,
  execution_mode: ExecutionMode,
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

  // 10-field granular timing metrics
  pub total_duration_ms: f64,
  pub discovery_ms: f64,
  pub manager_discovery_ms: f64,
  pub session_discovery_ms: f64,
  pub window_discovery_ms: f64,
  pub audio_lookup_ms: f64,
  pub volume_rw_ms: f64,
  pub dispatch_ms: f64,
  pub verification_ms: f64,
  pub wgc_ms: f64,
  pub wgc_init_ms: f64,
  pub wgc_capture_ms: f64,
  pub serialization_ms: f64,
  pub pacing_ms: f64,

  // 4 profiling dimensions & metadata
  pub cache_state: String,
  pub window_state: String,
  pub frame_health: String,
  pub identity_level: Option<String>,

  // Idempotency & command counts
  pub skipped_volume_write: bool,
  pub skipped_play_write: bool,
  pub play_dispatched: bool,
  pub volume_set_calls: usize,
  pub play_calls: usize,

  // Audio lookup statistics
  pub audio_lookup_status: Option<String>,
  pub audio_endpoint_count: Option<usize>,

  pub success: bool,
  pub confirmed: bool,
  pub fault_injected: Option<String>,
  pub escalated_to_vlm: bool,
  pub steps: Vec<StepRecord>,
}

struct DiscoveryTimings {
  manager_discovery_ms: f64,
  session_discovery_ms: f64,
  window_discovery_ms: f64,
  audio_lookup_ms: f64,
  audio_lookup_stats: Option<AudioLookupStats>,
}

/// Holds resolved resources for a single operation execution lifecycle.
struct WindowsOperationContext {
  session: SmtcSession,
  #[allow(dead_code)]
  pid: u32,
  window: Window,
  audio: Option<ProcessAudioVolume>,
  endpoint_id: Option<String>,
  timings: DiscoveryTimings,
}

impl WindowsOperationContext {
  fn resolve(cached_endpoint_id: Option<&str>) -> DriverResult<Self> {
    let t_mgr = Instant::now();
    let manager = SmtcMediaManager::new()?;
    let manager_discovery_ms = t_mgr.elapsed().as_secs_f64() * 1000.0;

    let t_sess = Instant::now();
    let session = manager.find_session("qqmusic")?.ok_or_else(|| DriverError::NotFound {
      target: "QQ Music SMTC session".to_string(),
    })?;
    let session_discovery_ms = t_sess.elapsed().as_secs_f64() * 1000.0;

    let t_win = Instant::now();
    let windows = list_windows()?;
    let window = windows.into_iter().find(|w| w.app_name.as_deref() == Some("QQMusic.exe")).ok_or_else(|| DriverError::NotFound {
      target: "QQMusic.exe window".to_string(),
    })?;
    let window_discovery_ms = t_win.elapsed().as_secs_f64() * 1000.0;

    let pid = window.process_id.unwrap_or(0);
    let t_audio = Instant::now();
    let (audio, audio_lookup_stats) = if pid > 0 {
      match AudioVolumeController::open_process_cached(pid, cached_endpoint_id) {
        Ok((vol, stats)) => (Some(vol), Some(stats)),
        Err(_) => (None, None),
      }
    } else {
      (None, None)
    };
    let audio_lookup_ms = t_audio.elapsed().as_secs_f64() * 1000.0;
    let endpoint_id = audio_lookup_stats.as_ref().and_then(|s| s.endpoint_id.clone());

    Ok(Self {
      session,
      pid,
      window,
      audio,
      endpoint_id,
      timings: DiscoveryTimings {
        manager_discovery_ms,
        session_discovery_ms,
        window_discovery_ms,
        audio_lookup_ms,
        audio_lookup_stats,
      },
    })
  }
}

// ==============================================================================
// BASELINE EXECUTION (Unoptimized with correctness fixes: no 1.2s auto-retry)
// ==============================================================================
fn execute_replay_baseline(
  iteration: usize,
  fault_injection: Option<&str>,
  allow_restore: bool,
  pacing_ms: f64,
  cache_state: &str,
  wgc_init_ms: f64,
) -> DriverResult<ReplayRecord> {
  let start_time = Instant::now();
  let mut steps = Vec::new();
  let mut overall_success = true;
  let mut escalated = false;

  // Discovery phase
  let t_mgr = Instant::now();
  let manager = SmtcMediaManager::new()?;
  let manager_discovery_ms = t_mgr.elapsed().as_secs_f64() * 1000.0;

  let t_sess = Instant::now();
  let session = manager.find_session("qqmusic")?.ok_or_else(|| DriverError::NotFound {
    target: "QQ Music SMTC session".to_string(),
  })?;
  let session_discovery_ms = t_sess.elapsed().as_secs_f64() * 1000.0;

  let t_win = Instant::now();
  let windows = list_windows()?;
  let qq_win = windows.into_iter().find(|w| w.app_name.as_deref() == Some("QQMusic.exe")).ok_or_else(|| DriverError::NotFound {
    target: "QQMusic.exe window".to_string(),
  })?;
  let window_discovery_ms = t_win.elapsed().as_secs_f64() * 1000.0;

  let pid = qq_win.process_id.unwrap_or(0);
  let t_audio = Instant::now();
  let vol = if pid > 0 {
    AudioVolumeController::get_process_volume(pid).ok()
  } else {
    None
  };
  let audio_lookup_ms = t_audio.elapsed().as_secs_f64() * 1000.0;
  let discovery_ms = manager_discovery_ms + session_discovery_ms + window_discovery_ms + audio_lookup_ms;

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
  let mut vol_calls = 0usize;
  let mut play_calls = 0usize;

  if fault_injection == Some("volume") {
    if pid > 0 {
      let _ = AudioVolumeController::set_process_volume(pid, 0.10);
      vol_calls += 1;
    }
  } else if pid > 0 {
    AudioVolumeController::set_process_volume(pid, target_vol)?;
    vol_calls += 1;
  }

  if fault_injection == Some("pause") {
    let _ = session.pause();
    std::thread::sleep(Duration::from_millis(150));
  } else {
    let current_st = session.playback_status().unwrap_or(MediaPlaybackStatus::Closed);
    if current_st != MediaPlaybackStatus::Playing && current_st != MediaPlaybackStatus::Changing {
      let _ = session.play();
      play_calls += 1;
    }
  }
  let s2_disp_ms = t_s2_disp.elapsed().as_secs_f64() * 1000.0;
  let volume_rw_ms = s2_disp_ms;

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
        play_calls += 1;
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

  let (s4_dur, s4_gate_passed, frame_health, window_state, s4_details) = if is_still_minimized {
    let dur = s4_start.elapsed().as_secs_f64() * 1000.0;
    (
      dur,
      true,
      "skipped_minimized".to_string(),
      "minimized".to_string(),
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
        for chunk in raw.as_chunks::<4>().0 {
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
          if alive {
            "fresh".to_string()
          } else {
            "stale".to_string()
          },
          "active".to_string(),
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
          "error".to_string(),
          "active".to_string(),
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
  let wgc_capture_ms = s4_dur;
  let wgc_ms = wgc_capture_ms;

  Ok(ReplayRecord {
    iteration,
    timestamp: chrono_now_iso(),
    operation: "qqmusic.prepare_playback".to_string(),
    mode: "baseline".to_string(),
    vlm_calls: 0,
    tokens_used: 0,
    total_duration_ms,
    discovery_ms,
    manager_discovery_ms,
    session_discovery_ms,
    window_discovery_ms,
    audio_lookup_ms,
    volume_rw_ms,
    dispatch_ms,
    verification_ms,
    wgc_ms,
    wgc_init_ms,
    wgc_capture_ms,
    serialization_ms: 0.0,
    pacing_ms,
    cache_state: cache_state.to_string(),
    window_state,
    frame_health,
    identity_level: Some("title_only".to_string()),
    skipped_volume_write: false,
    skipped_play_write: false,
    play_dispatched: play_calls > 0,
    volume_set_calls: vol_calls,
    play_calls,
    audio_lookup_status: None,
    audio_endpoint_count: None,
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
#[allow(clippy::too_many_arguments)]
fn execute_replay_optimized(
  iteration: usize,
  fault_injection: Option<&str>,
  allow_restore: bool,
  is_fast_mode: bool,
  fast_window_verification: FastWindowVerification,
  pacing_ms: f64,
  cache_state: &str,
  cached_endpoint_id: Option<&str>,
  wgc_init_ms: f64,
) -> DriverResult<(ReplayRecord, Option<String>)> {
  let start_time = Instant::now();
  let mut steps = Vec::new();
  let mut overall_success = true;
  let mut escalated = false;

  // Single operation context resolution with cached endpoint support
  let ctx = WindowsOperationContext::resolve(cached_endpoint_id)?;
  let discovery_ms =
    ctx.timings.manager_discovery_ms + ctx.timings.session_discovery_ms + ctx.timings.window_discovery_ms + ctx.timings.audio_lookup_ms;

  // --------------------------------------------------------------------------
  // Step 1: Query Playback State (reusing resolved context)
  // --------------------------------------------------------------------------
  let s1_start = Instant::now();
  let meta = ctx.session.track_metadata()?;
  let status = ctx.session.playback_status()?;
  let pre_vol = ctx.audio.as_ref().and_then(|a| a.get_volume().ok()).unwrap_or(0.0);
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
      "album": meta.album_title,
      "status": format!("{:?}", status),
      "volume": pre_vol,
      "audio_lookup_status": ctx.timings.audio_lookup_stats.as_ref().map(|s| s.status),
      "audio_endpoint_count": ctx.timings.audio_lookup_stats.as_ref().map(|s| s.endpoint_count),
    }),
  });

  // --------------------------------------------------------------------------
  // Step 2: Ensure Playing & Volume 40% (Phase 4 Command Counting & Batch Guard)
  // --------------------------------------------------------------------------
  let s2_start = Instant::now();
  let step2_opts = Step2Options {
    target_volume: DEFAULT_TARGET_VOLUME,
    volume_tolerance: DEFAULT_VOLUME_TOLERANCE,
    play_poll_timeout: if fault_injection.is_some() {
      Duration::from_millis(200)
    } else {
      DEFAULT_PLAY_POLL_TIMEOUT
    },
    fire_and_forget_play: is_fast_mode,
  };

  let (s2_result, volume_rw_ms) = if fault_injection == Some("volume") {
    if let Some(ref audio) = ctx.audio {
      let _ = audio.set_volume(0.10);
    }
    let res = execute_step2_real_with_prestate(ctx.audio.as_ref(), &ctx.session, 0.10, status, step2_opts)?;
    (res, s2_start.elapsed().as_secs_f64() * 1000.0)
  } else if fault_injection == Some("pause") {
    let _ = ctx.session.pause();
    std::thread::sleep(Duration::from_millis(150));
    let res = execute_step2_real_with_prestate(ctx.audio.as_ref(), &ctx.session, pre_vol, MediaPlaybackStatus::Paused, step2_opts)?;
    (res, s2_start.elapsed().as_secs_f64() * 1000.0)
  } else {
    let t_rw = Instant::now();
    let res = execute_step2_real_with_prestate(ctx.audio.as_ref(), &ctx.session, pre_vol, status, step2_opts)?;
    let rw_ms = t_rw.elapsed().as_secs_f64() * 1000.0;
    (res, rw_ms)
  };

  let s2_dur = s2_start.elapsed().as_secs_f64() * 1000.0;
  let s2_disp_ms = if s2_result.skipped_volume_write && s2_result.skipped_play_write {
    0.0
  } else {
    volume_rw_ms
  };
  let s2_verif_ms = (s2_dur - s2_disp_ms).max(0.0);

  if !s2_result.gate_passed {
    overall_success = false;
    escalated = true;
  }

  steps.push(StepRecord {
    step_id: "step_2_ensure_playing_and_volume".to_string(),
    duration_ms: s2_dur,
    success: true,
    gate_passed: s2_result.gate_passed,
    escalation: if s2_result.gate_passed {
      None
    } else {
      Some("would escalate to VLM".to_string())
    },
    details: serde_json::json!({
      "final_status": format!("{:?}", s2_result.final_status),
      "final_volume": s2_result.final_volume,
      "skipped_volume_write": s2_result.skipped_volume_write,
      "skipped_play_write": s2_result.skipped_play_write,
      "command_counts": {
        "set_volume": s2_result.command_counts.set_volume_calls,
        "play": s2_result.command_counts.play_calls,
      },
      "gate_passed": s2_result.gate_passed,
    }),
  });

  // --------------------------------------------------------------------------
  // Step 3: Skip Next Track (Phase 2A Track Identity + Phase 2B 2-Stage Wait)
  // --------------------------------------------------------------------------
  let s3_start = Instant::now();
  let prev_meta = ctx.session.track_metadata()?;
  let prev_identity = TrackIdentity::from(&prev_meta);

  // Dispatch action
  let t_s3_disp = Instant::now();
  let _ = ctx.session.skip_next();
  let s3_disp_ms = t_s3_disp.elapsed().as_secs_f64() * 1000.0;

  let (s3_verif_ms, s3_gate_passed, confirmed, identity_level, s3_details) = if is_fast_mode {
    // Fast / action mode: command dispatched successfully, return immediately without confirmation
    (
      0.0,
      true,
      false,
      Some(prev_identity.level().to_string()),
      serde_json::json!({
        "mode": "fast",
        "dispatched": true,
        "confirmed": false,
        "previous_identity": prev_identity,
        "dispatch_ms": s3_disp_ms,
      }),
    )
  } else {
    // Verified mode: Phase 2B Two-stage wait (MediaPropertiesChanged + PlaybackInfoChanged)
    let t_s3_verif = Instant::now();
    let (tx, rx) = std::sync::mpsc::sync_channel::<()>(8);

    let tx_prop = tx.clone();
    let token_prop = ctx
      .session
      .on_media_properties_changed(move || {
        let _ = tx_prop.try_send(());
      })
      .ok();

    let tx_play = tx;
    let token_play = ctx
      .session
      .on_playback_info_changed(move || {
        let _ = tx_play.try_send(());
      })
      .ok();

    let s3_timeout = Duration::from_millis(3000);
    let mut curr_identity = prev_identity.clone();
    let mut change_verdict = TrackChangeVerdict::Unchanged;
    let mut resolved_level = prev_identity.level();
    let mut backoff = Duration::from_millis(10);

    while t_s3_verif.elapsed() < s3_timeout {
      // Sleep until event arrives or backoff interval elapses
      let _ = rx.recv_timeout(backoff);

      if let Ok(curr_meta) = ctx.session.track_metadata() {
        curr_identity = TrackIdentity::from(&curr_meta);
        let (verdict, level) = evaluate_track_change(&prev_identity, &curr_identity, false);
        resolved_level = level;
        change_verdict = verdict;

        if verdict == TrackChangeVerdict::Changed {
          break;
        }
      }

      backoff = (backoff * 2).min(Duration::from_millis(80));
    }

    if let Some(tok) = token_prop {
      let _ = ctx.session.remove_media_properties_changed(tok);
    }
    if let Some(tok) = token_play {
      let _ = ctx.session.remove_playback_info_changed(tok);
    }

    let verif_ms = t_s3_verif.elapsed().as_secs_f64() * 1000.0;
    let is_confirmed = change_verdict == TrackChangeVerdict::Changed;

    (
      verif_ms,
      is_confirmed,
      is_confirmed,
      Some(resolved_level.to_string()),
      serde_json::json!({
        "mode": "verified",
        "previous_identity": prev_identity,
        "current_identity": curr_identity,
        "verdict": change_verdict.to_string(),
        "identity_level": resolved_level.to_string(),
        "confirmed": is_confirmed,
        "dispatch_ms": s3_disp_ms,
        "verification_ms": verif_ms,
      }),
    )
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
    details: s3_details,
  });

  // --------------------------------------------------------------------------
  // Step 4: Verify Window Alive (Phase 5 Lightweight WGC health check)
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

  let (s4_dur, s4_gate_passed, frame_health, window_state, s4_details) = if is_still_minimized {
    let dur = s4_start.elapsed().as_secs_f64() * 1000.0;
    (
      dur,
      true,
      "skipped_minimized".to_string(),
      "minimized".to_string(),
      serde_json::json!({
        "status": "skipped_minimized",
        "reason": "window_minimized (zero-window-mutation redline preserves user state)",
        "alive": true,
        "verification_kind": if is_fast_mode && fast_window_verification == FastWindowVerification::LightweightLiveness {
          "lightweight_liveness"
        } else {
          "wgc_fresh"
        },
      }),
    )
  } else if is_fast_mode && fast_window_verification == FastWindowVerification::LightweightLiveness {
    let alive = check_window_liveness(&ctx.window).unwrap_or(false);
    let dur = s4_start.elapsed().as_secs_f64() * 1000.0;
    (
      dur,
      alive,
      if alive {
        "alive".to_string()
      } else {
        "dead".to_string()
      },
      "active".to_string(),
      serde_json::json!({
        "status": if alive { "liveness_verified" } else { "liveness_failed" },
        "alive": alive,
        "verification_kind": "lightweight_liveness",
      }),
    )
  } else if is_fast_mode {
    match capture_window_health_cached(&ctx.window) {
      Ok(health) => {
        let dur = s4_start.elapsed().as_secs_f64() * 1000.0;
        let fh = if health.is_fresh {
          "fresh".to_string()
        } else {
          "stale".to_string()
        };
        (
          dur,
          health.alive && health.is_fresh,
          fh,
          "active".to_string(),
          serde_json::json!({
            "status": "captured_health",
            "width": health.width,
            "height": health.height,
            "non_black_ratio": health.non_black_ratio,
            "is_fresh": health.is_fresh,
            "alive": health.alive,
            "verification_kind": "wgc_fresh",
          }),
        )
      }
      Err(e) => {
        let dur = s4_start.elapsed().as_secs_f64() * 1000.0;
        (
          dur,
          false,
          "error".to_string(),
          "active".to_string(),
          serde_json::json!({
            "status": "error",
            "error": format!("{e:?}"),
            "alive": false,
            "verification_kind": "wgc_fresh",
          }),
        )
      }
    }
  } else {
    match capture_window_health_strict(&ctx.window) {
      Ok(health) => {
        let dur = s4_start.elapsed().as_secs_f64() * 1000.0;
        let fh = if health.is_fresh {
          "fresh".to_string()
        } else {
          "stale".to_string()
        };
        (
          dur,
          health.alive && health.is_fresh,
          fh,
          "active".to_string(),
          serde_json::json!({
            "status": "captured_health",
            "width": health.width,
            "height": health.height,
            "non_black_ratio": health.non_black_ratio,
            "is_fresh": health.is_fresh,
            "alive": health.alive,
            "verification_kind": "wgc_fresh",
          }),
        )
      }
      Err(e) => {
        let dur = s4_start.elapsed().as_secs_f64() * 1000.0;
        (
          dur,
          false,
          "error".to_string(),
          "active".to_string(),
          serde_json::json!({
            "status": "error",
            "error": format!("{e:?}"),
            "alive": false,
            "verification_kind": "wgc_fresh",
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
  let wgc_capture_ms = s4_dur;
  let wgc_ms = wgc_capture_ms;

  let record = ReplayRecord {
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
    manager_discovery_ms: ctx.timings.manager_discovery_ms,
    session_discovery_ms: ctx.timings.session_discovery_ms,
    window_discovery_ms: ctx.timings.window_discovery_ms,
    audio_lookup_ms: ctx.timings.audio_lookup_ms,
    volume_rw_ms,
    dispatch_ms,
    verification_ms,
    wgc_ms,
    wgc_init_ms,
    wgc_capture_ms,
    serialization_ms: 0.0,
    pacing_ms,
    cache_state: cache_state.to_string(),
    window_state,
    frame_health,
    identity_level,
    skipped_volume_write: s2_result.skipped_volume_write,
    skipped_play_write: s2_result.skipped_play_write,
    play_dispatched: s2_result.play_dispatched,
    volume_set_calls: s2_result.command_counts.set_volume_calls,
    play_calls: s2_result.command_counts.play_calls,
    audio_lookup_status: ctx.timings.audio_lookup_stats.as_ref().map(|s| match s.status {
      AudioLookupStatus::Hit => "hit".to_string(),
      AudioLookupStatus::Miss => "miss".to_string(),
      AudioLookupStatus::Invalidated => "invalidated".to_string(),
    }),
    audio_endpoint_count: ctx.timings.audio_lookup_stats.as_ref().map(|s| s.endpoint_count),
    success: overall_success,
    confirmed,
    fault_injected: fault_injection.map(ToString::to_string),
    escalated_to_vlm: escalated,
    steps,
  };

  Ok((record, ctx.endpoint_id))
}

fn chrono_now_iso() -> String {
  let now = std::time::SystemTime::now();
  let dur = now.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
  let secs = dur.as_secs();
  let millis = dur.subsec_millis();
  format!("{}.{:03}Z", secs, millis)
}

fn linear_percentile(sorted_vals: &[f64], p: f64) -> f64 {
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

fn compute_stats(mut vals: Vec<f64>) -> (f64, f64, f64) {
  if vals.is_empty() {
    return (0.0, 0.0, 0.0);
  }
  vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
  let mean = vals.iter().sum::<f64>() / vals.len() as f64;
  let p50 = linear_percentile(&vals, 0.50);
  let p95 = linear_percentile(&vals, 0.95);
  (p50, p95, mean)
}

fn validate_cli_mode(mode: &str) -> Result<(), String> {
  match mode {
    "baseline" | "verified" | "fast" => Ok(()),
    other => Err(format!("Unsupported --mode '{other}'; expected baseline, verified, or fast")),
  }
}

pub(super) fn main() {
  let args: Vec<String> = env::args().collect();
  let mut replays_count = 20usize;
  let mut mode = "verified".to_string(); // "baseline", "verified", "fast"
  let mut output_file: Option<String> = None;
  let mut user_op_file: Option<String> = None;
  let mut fault_inject: Option<String> = None;
  let mut allow_restore = false;
  let mut pacing_delay_ms = 50.0;
  let mut is_single_cold_mode = false;
  let mut fast_window_verification = FastWindowVerification::WgcFresh;

  let mut i = 1;
  while i < args.len() {
    match args[i].as_str() {
      "--mode" => {
        if i + 1 < args.len() {
          mode = args[i + 1].to_lowercase();
          i += 1;
        } else {
          panic!("Missing value for --mode; expected baseline, verified, or fast");
        }
      }
      "--op" | "--operation" => {
        if i + 1 < args.len() {
          user_op_file = Some(args[i + 1].clone());
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
      "--pacing-ms" => {
        if i + 1 < args.len() {
          pacing_delay_ms = args[i + 1].parse().unwrap_or(50.0);
          i += 1;
        }
      }
      "--cold" => {
        is_single_cold_mode = true;
        replays_count = 1;
      }
      "--fast-window-verification" => {
        if i + 1 < args.len() {
          fast_window_verification = match args[i + 1].to_lowercase().as_str() {
            "liveness" | "lightweight" | "lightweight_liveness" => FastWindowVerification::LightweightLiveness,
            _ => FastWindowVerification::WgcFresh,
          };
          i += 1;
        }
      }
      _ => {}
    }
    i += 1;
  }

  validate_cli_mode(&mode).unwrap_or_else(|message| panic!("{message}"));
  ensure_input_desktop();

  // Prewarm only warm experimental groups. Baseline and --cold runs must
  // measure the uninitialized path without D3D/session/worker state.
  let mut wgc_init_dur = Duration::ZERO;
  if mode == "baseline" || is_single_cold_mode {
    reset_d3d_context();
  } else {
    wgc_init_dur += prewarm_wgc().unwrap_or(Duration::ZERO);
    if let Ok(windows) = list_windows()
      && let Some(w) = windows.into_iter().find(|win| {
        win.app_name.as_deref() == Some("QQMusic.exe")
          || win.app_name.as_deref() == Some("QQMusic")
          || win.title.as_deref().map(|t| t.contains("QQ音乐")).unwrap_or(false)
      })
      && let Ok(win_dur) = prewarm_wgc_window(&w)
    {
      wgc_init_dur += win_dur;
    }
  }
  let wgc_init_ms = wgc_init_dur.as_secs_f64() * 1000.0;

  let final_output = output_file.unwrap_or_else(|| match mode.as_str() {
    "baseline" => "docs/ai/references/driver/2026-10-08-wgc-health-baseline-20x.jsonl".to_string(),
    "fast" => "docs/ai/references/driver/2026-10-08-wgc-health-warm-fast-20x.jsonl".to_string(),
    _ => "docs/ai/references/driver/2026-10-08-wgc-health-warm-verified-20x.jsonl".to_string(),
  });

  let default_op_path = match mode.as_str() {
    "fast" => "docs/ai/references/driver/qqmusic-prepared-playback-fast.json",
    "baseline" | "verified" => "docs/ai/references/driver/qqmusic-prepared-playback.json",
    _ => unreachable!("CLI mode was validated above"),
  };
  let op_file_path = user_op_file.as_deref().unwrap_or(default_op_path);
  let op_json = fs::read_to_string(op_file_path)
    .or_else(|_| fs::read_to_string(Path::new("..").join("..").join(op_file_path)))
    .unwrap_or_else(|e| panic!("Failed to read compiled operation JSON at {op_file_path}: {e}"));
  let compiled_op: CompiledOperation =
    serde_json::from_str(&op_json).unwrap_or_else(|e| panic!("Failed to parse compiled operation JSON at {op_file_path}: {e}"));

  if compiled_op.schema_version != "auv.operation.v2" {
    panic!(
      "Compiled operation schema mismatch at {}: expected 'auv.operation.v2', got '{}' (fail-closed)",
      op_file_path, compiled_op.schema_version
    );
  }

  // Validate execution mode compatibility between CLI and operation declaration
  match (mode.as_str(), compiled_op.execution_mode) {
    ("fast", ExecutionMode::Verified) => {
      panic!(
        "Mode conflict: CLI requested 'fast' mode, but compiled operation '{}' requires 'verified' mode (fail-closed)",
        compiled_op.name
      );
    }
    ("verified", ExecutionMode::Fast) => {
      panic!(
        "Mode conflict: CLI requested 'verified' mode, but compiled operation '{}' declares 'fast' mode (fail-closed)",
        compiled_op.name
      );
    }
    ("baseline", _) => {
      // Baseline is a harness path, not an operation execution mode. It uses
      // the verified fixture to measure the unoptimized implementation.
    }
    _ => unreachable!("CLI mode was validated above"),
  }

  println!("================================================================================");
  println!("QQ Music Hotpath Profiling & Replay Harness (Zero VLM / Zero Token)");
  println!("================================================================================");
  println!("Target Operation : {} ({})", compiled_op.name, compiled_op.schema_version);
  println!("Execution Mode   : {}", mode);
  println!("Compiler Source  : {}", compiled_op.compilation_metadata.compiler);
  println!("Replays Count    : {}", replays_count);
  println!("Pacing Delay     : {:.1}ms (isolated from total_duration_ms)", pacing_delay_ms);
  println!("Cold Mode        : {}", is_single_cold_mode);
  println!("WGC Prewarm      : {:.2}ms", wgc_init_ms);
  println!("Output JSONL     : {}", final_output);
  println!("Fault Injection  : {:?}", fault_inject);
  println!("Fast Window Verif: {:?}", fast_window_verification);
  println!("Allow Restore    : {} (default false, zero-window-mutation redline)", allow_restore);
  println!("VLM Invocations  : 0 (hard constraint)");
  println!("Token Budget     : 0 (hard constraint)");
  println!("--------------------------------------------------------------------------------");

  if let Some(ref fi) = fault_inject {
    println!("[FAULT INJECTION MODE] Injecting fault: {}", fi);
    let record =
      match mode.as_str() {
        "baseline" => execute_replay_baseline(1, Some(fi), allow_restore, 0.0, "cold", wgc_init_ms),
        "fast" => execute_replay_optimized(1, Some(fi), allow_restore, true, fast_window_verification, 0.0, "cold", None, wgc_init_ms)
          .map(|(r, _)| r),
        "verified" => execute_replay_optimized(1, Some(fi), allow_restore, false, fast_window_verification, 0.0, "cold", None, wgc_init_ms)
          .map(|(r, _)| r),
        _ => unreachable!("CLI mode was validated above"),
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
  let mut records = Vec::with_capacity(replays_count);
  let parent_dir = Path::new(&final_output).parent().unwrap_or(Path::new("."));
  let _ = fs::create_dir_all(parent_dir);

  let mut cached_ep_id: Option<String> = None;
  let mut successes = 0usize;

  for iter in 1..=replays_count {
    let cache_state = if iter == 1 { "cold" } else { "warm" };
    let current_pacing = if iter == 1 { 0.0 } else { pacing_delay_ms };

    print!("[Replay {:02}/{:02} ({})] Executing... ", iter, replays_count, cache_state);
    std::io::stdout().flush().unwrap();

    let res = match mode.as_str() {
      "baseline" => execute_replay_baseline(iter, None, allow_restore, current_pacing, cache_state, wgc_init_ms).map(|r| (r, None)),
      "fast" => execute_replay_optimized(
        iter,
        None,
        allow_restore,
        true,
        fast_window_verification,
        current_pacing,
        cache_state,
        cached_ep_id.as_deref(),
        wgc_init_ms,
      ),
      "verified" => execute_replay_optimized(
        iter,
        None,
        allow_restore,
        false,
        fast_window_verification,
        current_pacing,
        cache_state,
        cached_ep_id.as_deref(),
        wgc_init_ms,
      ),
      _ => unreachable!("CLI mode was validated above"),
    };

    let (mut record, next_ep) = match res {
      Ok((r, ep)) => (r, ep),
      Err(e) => {
        println!("FAILED: {e:?}");
        continue;
      }
    };

    if next_ep.is_some() {
      cached_ep_id = next_ep;
    }

    if record.success {
      successes += 1;
    }

    // Measure serialization timing in memory (Phase 6)
    let t_ser = Instant::now();
    let _ = serde_json::to_string(&record);
    record.serialization_ms = t_ser.elapsed().as_secs_f64() * 1000.0;

    println!(
      "OK ({:.1}ms) | Mgr={:.1}ms, Sess={:.1}ms, Win={:.1}ms, Aud={:.1}ms, Disp={:.1}ms, Verif={:.1}ms, WGC={:.1}ms | tokens=0",
      record.total_duration_ms,
      record.manager_discovery_ms,
      record.session_discovery_ms,
      record.window_discovery_ms,
      record.audio_lookup_ms,
      record.dispatch_ms,
      record.verification_ms,
      record.wgc_ms
    );

    records.push(record);

    if iter < replays_count && pacing_delay_ms > 0.0 {
      std::thread::sleep(Duration::from_millis(pacing_delay_ms as u64));
    }
  }

  // Phase 6: Batch write to disk
  let write_start = Instant::now();
  let append_mode = is_single_cold_mode && Path::new(&final_output).exists();
  let mut file = OpenOptions::new()
    .create(true)
    .write(true)
    .append(append_mode)
    .truncate(!append_mode)
    .open(&final_output)
    .expect("Failed to open output jsonl file");

  for r in &records {
    let line = serde_json::to_string(r).unwrap();
    writeln!(file, "{}", line).unwrap();
  }
  file.flush().unwrap();
  let total_batch_write_ms = write_start.elapsed().as_secs_f64() * 1000.0;

  println!("--------------------------------------------------------------------------------");
  println!("Execution Summary (Mode: {}, Total Runs: {}, Successful: {}):", mode, records.len(), successes);
  println!("  Batch File Write : {:.2}ms for {} records", total_batch_write_ms, records.len());
  println!("  VLM Invocations  : 0 (实测 0 调用)");
  println!("  Tokens Used      : 0 (实测 0 token)");
  println!();

  // Print 10-Field Split Benchmark using standard linear interpolation
  let extract = |f: fn(&ReplayRecord) -> f64| records.iter().map(f).collect::<Vec<f64>>();
  let (tot_p50, tot_p95, tot_mean) = compute_stats(extract(|r| r.total_duration_ms));
  let (disc_p50, disc_p95, disc_mean) = compute_stats(extract(|r| r.discovery_ms));
  let (mgr_p50, mgr_p95, mgr_mean) = compute_stats(extract(|r| r.manager_discovery_ms));
  let (sess_p50, sess_p95, sess_mean) = compute_stats(extract(|r| r.session_discovery_ms));
  let (win_p50, win_p95, win_mean) = compute_stats(extract(|r| r.window_discovery_ms));
  let (aud_p50, aud_p95, aud_mean) = compute_stats(extract(|r| r.audio_lookup_ms));
  let (vrw_p50, vrw_p95, vrw_mean) = compute_stats(extract(|r| r.volume_rw_ms));
  let (disp_p50, disp_p95, disp_mean) = compute_stats(extract(|r| r.dispatch_ms));
  let (verif_p50, verif_p95, verif_mean) = compute_stats(extract(|r| r.verification_ms));
  let (wgc_p50, wgc_p95, wgc_mean) = compute_stats(extract(|r| r.wgc_ms));
  let (ser_p50, ser_p95, ser_mean) = compute_stats(extract(|r| r.serialization_ms));
  let (pace_p50, pace_p95, pace_mean) = compute_stats(extract(|r| r.pacing_ms));

  println!("10-Field Granular Split Benchmark (Standard Linear Interpolation):");
  println!("  | Metric                 | P50 Latency | P95 Latency | Mean Latency | Description |");
  println!("  |------------------------|-------------|-------------|--------------|-------------|");
  println!("  | manager_discovery_ms   | {:>9.2}ms | {:>9.2}ms | {:>10.2}ms | SMTC manager acquisition    |", mgr_p50, mgr_p95, mgr_mean);
  println!("  | session_discovery_ms   | {:>9.2}ms | {:>9.2}ms | {:>10.2}ms | GetCurrentSession vs scan   |", sess_p50, sess_p95, sess_mean);
  println!("  | window_discovery_ms    | {:>9.2}ms | {:>9.2}ms | {:>10.2}ms | HWND enumeration & filter   |", win_p50, win_p95, win_mean);
  println!("  | audio_lookup_ms        | {:>9.2}ms | {:>9.2}ms | {:>10.2}ms | CoreAudio endpoint/session  |", aud_p50, aud_p95, aud_mean);
  println!("  | discovery_ms (sum)     | {:>9.2}ms | {:>9.2}ms | {:>10.2}ms | Total discovery phase       |", disc_p50, disc_p95, disc_mean);
  println!("  | volume_rw_ms           | {:>9.2}ms | {:>9.2}ms | {:>10.2}ms | Volume read/write duration  |", vrw_p50, vrw_p95, vrw_mean);
  println!("  | dispatch_ms            | {:>9.2}ms | {:>9.2}ms | {:>10.2}ms | Command call latency        |", disp_p50, disp_p95, disp_mean);
  println!(
    "  | verification_ms        | {:>9.2}ms | {:>9.2}ms | {:>10.2}ms | Identity polling / wait     |",
    verif_p50, verif_p95, verif_mean
  );
  println!("  | wgc_ms                 | {:>9.2}ms | {:>9.2}ms | {:>10.2}ms | WGC health check            |", wgc_p50, wgc_p95, wgc_mean);
  println!("  | serialization_ms       | {:>9.2}ms | {:>9.2}ms | {:>10.2}ms | In-memory JSON serialize    |", ser_p50, ser_p95, ser_mean);
  println!("  | pacing_ms (isolated)   | {:>9.2}ms | {:>9.2}ms | {:>10.2}ms | Inter-iteration sleep       |", pace_p50, pace_p95, pace_mean);
  println!("  |------------------------|-------------|-------------|--------------|-------------|");
  println!("  | total_duration_ms      | {:>9.2}ms | {:>9.2}ms | {:>10.2}ms | End-to-end operation        |", tot_p50, tot_p95, tot_mean);
  println!();

  // Print Command Counting Summary (Phase 4)
  let vol_calls_total: usize = records.iter().map(|r| r.volume_set_calls).sum();
  let play_calls_total: usize = records.iter().map(|r| r.play_calls).sum();
  let skipped_vol_total = records.iter().filter(|r| r.skipped_volume_write).count();
  let skipped_play_total = records.iter().filter(|r| r.skipped_play_write).count();

  println!("Phase 4 Command Counting & Idempotency Proof:");
  println!(
    "  - SetMasterVolume calls : {} total across {} runs (skipped: {}/{})",
    vol_calls_total,
    records.len(),
    skipped_vol_total,
    records.len()
  );
  println!(
    "  - Play calls            : {} total across {} runs (skipped: {}/{})",
    play_calls_total,
    records.len(),
    skipped_play_total,
    records.len()
  );
  println!("================================================================================");
}

#[cfg(test)]
mod tests {
  use super::validate_cli_mode;

  #[test]
  fn cli_mode_validation_accepts_supported_modes() {
    for mode in ["baseline", "verified", "fast"] {
      assert_eq!(validate_cli_mode(mode), Ok(()));
    }
  }

  #[test]
  fn cli_mode_validation_rejects_unknown_modes() {
    let error = validate_cli_mode("fastt").unwrap_err();
    assert!(error.contains("Unsupported --mode 'fastt'"));
  }
}
