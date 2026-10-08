//! Production driver execution engine for mode-aware operations on Windows.
//!
//! Enforces:
//! - Fail-closed: `requested_mode: None` is rejected with `REJECT_MODE_UNDECLARED`
//!   before any driver or COM side effects.
//! - Second gate check: mode conflict (`requested_mode != op.execution_mode`)
//!   is rejected with `ModeConflictError` before any driver or COM calls (zero side effects).
//! - Fast mode: fire-and-forget dispatch with eventual consistency; `confirmed` is ALWAYS `false`.
//! - Verified mode: step-by-step gate verification with SMTC, CoreAudio volume guard,
//!   adaptive backoff track identity change verification, and WGC window liveness checks.

use std::time::{Duration, Instant};

use auv_auto_loop::models::{ExecutionMode, OperationDef, OperationStepDef, VerificationGateDef};
use auv_auto_loop::runtime::{ExecutionResult, OperationExecutor};
use auv_auto_loop::scheduler::TaskRequest;
use auv_driver_common::error::{DriverError, DriverResult};
use auv_driver_common::window::Window;

use crate::media::{AudioLookupStats, AudioVolumeController, MediaPlaybackStatus, ProcessAudioVolume, SmtcMediaManager, SmtcSession};
use crate::playback_guard::{
  DEFAULT_PLAY_POLL_TIMEOUT, DEFAULT_TARGET_VOLUME, DEFAULT_VOLUME_TOLERANCE, Step2Options, execute_step2_real_with_prestate,
};
use crate::track_identity::{TrackChangeVerdict, TrackIdentity, evaluate_track_change};
use crate::wgc::{capture_window_health_cached, capture_window_health_strict};
use crate::window::list_windows;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HWND;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{IsIconic, SW_RESTORE, ShowWindow};

/// Granular split discovery timings for operation context acquisition.
#[derive(Debug, Clone)]
pub struct DiscoveryTimings {
  pub manager_discovery_ms: f64,
  pub session_discovery_ms: f64,
  pub window_discovery_ms: f64,
  pub audio_lookup_ms: f64,
  pub audio_lookup_stats: Option<AudioLookupStats>,
}

/// Holds resolved Win32 / COM resources for a single operation execution lifecycle.
pub struct WindowsOperationContext {
  pub session: SmtcSession,
  pub pid: u32,
  pub window: Window,
  pub audio: Option<ProcessAudioVolume>,
  pub endpoint_id: Option<String>,
  pub timings: DiscoveryTimings,
}

impl WindowsOperationContext {
  /// Resolves the context targeting QQ Music, optionally utilizing a cached CoreAudio endpoint ID.
  pub fn resolve(cached_endpoint_id: Option<&str>) -> DriverResult<Self> {
    Self::resolve_for_target("qqmusic", "QQMusic.exe", cached_endpoint_id)
  }

  /// Resolves context for a given SMTC app substring and process executable name.
  pub fn resolve_for_target(smtc_filter: &str, app_process: &str, cached_endpoint_id: Option<&str>) -> DriverResult<Self> {
    let t_mgr = Instant::now();
    let manager = SmtcMediaManager::new()?;
    let manager_discovery_ms = t_mgr.elapsed().as_secs_f64() * 1000.0;

    let t_sess = Instant::now();
    let session = manager.find_session(smtc_filter)?.ok_or_else(|| DriverError::NotFound {
      target: format!("{smtc_filter} SMTC session"),
    })?;
    let session_discovery_ms = t_sess.elapsed().as_secs_f64() * 1000.0;

    let t_win = Instant::now();
    let windows = list_windows()?;
    let window = windows
      .into_iter()
      .find(|w| {
        w.app_name
          .as_deref()
          .map(|n| n.eq_ignore_ascii_case(app_process) || n.to_ascii_lowercase().contains(&smtc_filter.to_ascii_lowercase()))
          .unwrap_or(false)
      })
      .ok_or_else(|| DriverError::NotFound {
        target: format!("{app_process} window"),
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

/// Production executor for mode-aware Windows operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WindowsProductionExecutor {
  pub allow_restore: bool,
}

impl WindowsProductionExecutor {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn with_allow_restore(allow_restore: bool) -> Self {
    Self { allow_restore }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StepKind {
  SmtcQuery,
  EnsurePlayingAndVolume,
  SkipNext,
  Play,
  WgcCapture,
  Other,
}

fn classify_step(step: &OperationStepDef) -> StepKind {
  if let Some(action_type) = step.action.get("type").and_then(|v| v.as_str()) {
    match action_type {
      "smtc_query" => return StepKind::SmtcQuery,
      "ensure_playing_and_volume" | "volume" => return StepKind::EnsurePlayingAndVolume,
      "skip_next" | "next" => return StepKind::SkipNext,
      "play" => return StepKind::Play,
      "wgc_capture" | "capture" => return StepKind::WgcCapture,
      _ => {}
    }
  }

  match &step.verification_gate {
    VerificationGateDef::SmtcSessionPresent { .. } => StepKind::SmtcQuery,
    VerificationGateDef::StatusAndVolumeGate { .. } => {
      let id_lower = step.id.to_lowercase();
      let name_lower = step.name.to_lowercase();
      if id_lower.contains("volume") || name_lower.contains("volume") {
        StepKind::EnsurePlayingAndVolume
      } else if id_lower.contains("play") || name_lower.contains("play") {
        StepKind::Play
      } else {
        StepKind::EnsurePlayingAndVolume
      }
    }
    VerificationGateDef::TitleChangeGate { .. } => StepKind::SkipNext,
    VerificationGateDef::WgcAliveGate { .. } => StepKind::WgcCapture,
    _ => {
      let id_lower = step.id.to_lowercase();
      let name_lower = step.name.to_lowercase();
      if id_lower.contains("query") || name_lower.contains("query") {
        StepKind::SmtcQuery
      } else if id_lower.contains("volume") || name_lower.contains("volume") {
        StepKind::EnsurePlayingAndVolume
      } else if id_lower.contains("skip") || id_lower.contains("next") || name_lower.contains("skip") || name_lower.contains("next") {
        StepKind::SkipNext
      } else if id_lower.contains("play") || name_lower.contains("play") {
        StepKind::Play
      } else if id_lower.contains("wgc") || id_lower.contains("capture") || id_lower.contains("alive") || name_lower.contains("capture") {
        StepKind::WgcCapture
      } else {
        StepKind::Other
      }
    }
  }
}

#[cfg(target_os = "windows")]
fn check_minimized_and_restore(window: &Window, allow_restore: bool) -> bool {
  let hwnd_opt = window.reference.id.parse::<isize>().ok().map(|h| HWND(h as _));
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
  hwnd_opt.map(|hwnd| unsafe { IsIconic(hwnd).as_bool() }).unwrap_or(false)
}

#[cfg(not(target_os = "windows"))]
fn check_minimized_and_restore(_window: &Window, _allow_restore: bool) -> bool {
  false
}

impl OperationExecutor for WindowsProductionExecutor {
  fn execute(&self, op: &OperationDef, request: &TaskRequest) -> ExecutionResult {
    // Second gate check at driver boundary:
    // Check request.requested_mode BEFORE any driver/COM calls
    let req_mode = match request.requested_mode {
      Some(mode) => mode,
      None => {
        return ExecutionResult::ModeConflictError {
          operation_name: op.name.clone(),
          message: "REJECT_MODE_UNDECLARED: driver execution requires explicit execution_mode (fail-closed)".to_string(),
        };
      }
    };

    if req_mode != op.execution_mode {
      return ExecutionResult::ModeConflictError {
        operation_name: op.name.clone(),
        message: format!(
          "Driver boundary mode conflict: requested {:?}, but operation declares {:?} (fail-closed, zero side effects)",
          request.requested_mode, op.execution_mode
        ),
      };
    }

    // Acquire operation context (first driver call occurs ONLY after mode gate passes)
    let ctx = match WindowsOperationContext::resolve(None) {
      Ok(c) => c,
      Err(err) => {
        let step_id = op.steps.first().map(|s| s.id.clone()).unwrap_or_else(|| "context_resolution".to_string());
        return ExecutionResult::GateFailed {
          step_id,
          message: format!("Failed to resolve Windows operation context: {err}"),
          consecutive_failures: 1,
        };
      }
    };

    if op.steps.is_empty() {
      return ExecutionResult::success(0, op.execution_mode, false);
    }

    let mut pre_vol: f32 = ctx.audio.as_ref().and_then(|a| a.get_volume().ok()).unwrap_or(0.0);
    let mut status: MediaPlaybackStatus = ctx.session.playback_status().unwrap_or(MediaPlaybackStatus::Closed);
    let mut skip_next_confirmed = false;
    let mut steps_executed = 0;

    for step in &op.steps {
      match classify_step(step) {
        StepKind::SmtcQuery => {
          let meta = match ctx.session.track_metadata() {
            Ok(m) => m,
            Err(e) => {
              return ExecutionResult::GateFailed {
                step_id: step.id.clone(),
                message: format!("Failed to query track metadata: {e}"),
                consecutive_failures: 1,
              };
            }
          };
          if let Ok(st) = ctx.session.playback_status() {
            status = st;
          }
          if let Some(vol) = ctx.audio.as_ref().and_then(|a| a.get_volume().ok()) {
            pre_vol = vol;
          }
          if op.execution_mode == ExecutionMode::Verified && meta.title.trim().is_empty() {
            return ExecutionResult::GateFailed {
              step_id: step.id.clone(),
              message: "Verified mode SMTC query gate failed: track title is empty".to_string(),
              consecutive_failures: 1,
            };
          }
          steps_executed += 1;
        }
        StepKind::EnsurePlayingAndVolume => {
          let (target_volume, volume_tolerance) = match &step.verification_gate {
            VerificationGateDef::StatusAndVolumeGate {
              expected_volume,
              volume_tolerance,
              ..
            } => (*expected_volume, *volume_tolerance),
            _ => {
              let vol = step.action.get("volume").and_then(|v| v.as_f64()).map(|v| v as f32).unwrap_or(DEFAULT_TARGET_VOLUME);
              (vol, DEFAULT_VOLUME_TOLERANCE)
            }
          };
          let step2_opts = Step2Options {
            target_volume,
            volume_tolerance,
            play_poll_timeout: DEFAULT_PLAY_POLL_TIMEOUT,
            fire_and_forget_play: op.execution_mode == ExecutionMode::Fast,
          };
          let s2_res = match execute_step2_real_with_prestate(ctx.audio.as_ref(), &ctx.session, pre_vol, status, step2_opts) {
            Ok(r) => r,
            Err(e) => {
              return ExecutionResult::GateFailed {
                step_id: step.id.clone(),
                message: format!("Failed executing ensure_playing_and_volume: {e}"),
                consecutive_failures: 1,
              };
            }
          };
          pre_vol = s2_res.final_volume;
          status = s2_res.final_status;
          if op.execution_mode == ExecutionMode::Verified && !s2_res.gate_passed {
            return ExecutionResult::GateFailed {
              step_id: step.id.clone(),
              message: format!(
                "Step 2 ensure_playing_and_volume gate failed: final_status={:?}, final_volume={}",
                s2_res.final_status, s2_res.final_volume
              ),
              consecutive_failures: 1,
            };
          }
          steps_executed += 1;
        }
        StepKind::SkipNext => {
          if op.execution_mode == ExecutionMode::Fast {
            if let Err(e) = ctx.session.skip_next() {
              return ExecutionResult::GateFailed {
                step_id: step.id.clone(),
                message: format!("Failed to dispatch skip_next in fast mode: {e}"),
                consecutive_failures: 1,
              };
            }
            skip_next_confirmed = false;
            steps_executed += 1;
          } else {
            let prev_meta = match ctx.session.track_metadata() {
              Ok(m) => m,
              Err(e) => {
                return ExecutionResult::GateFailed {
                  step_id: step.id.clone(),
                  message: format!("Failed to query track metadata before skip_next: {e}"),
                  consecutive_failures: 1,
                };
              }
            };
            let prev_identity = TrackIdentity::from(&prev_meta);

            if let Err(e) = ctx.session.skip_next() {
              return ExecutionResult::GateFailed {
                step_id: step.id.clone(),
                message: format!("Failed to dispatch skip_next: {e}"),
                consecutive_failures: 1,
              };
            }

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
            let mut change_verdict = TrackChangeVerdict::Unchanged;
            let mut backoff = Duration::from_millis(10);

            while t_s3_verif.elapsed() < s3_timeout {
              let _ = rx.recv_timeout(backoff);

              if let Ok(curr_meta) = ctx.session.track_metadata() {
                let curr_identity = TrackIdentity::from(&curr_meta);
                let (verdict, _level) = evaluate_track_change(&prev_identity, &curr_identity, false);
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

            if change_verdict == TrackChangeVerdict::Changed {
              skip_next_confirmed = true;
              steps_executed += 1;
            } else {
              return ExecutionResult::GateFailed {
                step_id: step.id.clone(),
                message: format!("Track identity change verification failed after skip_next: verdict={change_verdict:?}"),
                consecutive_failures: 1,
              };
            }
          }
        }
        StepKind::Play => {
          if status != MediaPlaybackStatus::Playing {
            if let Err(e) = ctx.session.play() {
              return ExecutionResult::GateFailed {
                step_id: step.id.clone(),
                message: format!("Failed to dispatch play command: {e}"),
                consecutive_failures: 1,
              };
            }
            if op.execution_mode == ExecutionMode::Verified {
              let t_play = Instant::now();
              let mut is_playing = false;
              while t_play.elapsed() < Duration::from_millis(2000) {
                if let Ok(st) = ctx.session.playback_status() {
                  status = st;
                  if status == MediaPlaybackStatus::Playing {
                    is_playing = true;
                    break;
                  }
                }
                std::thread::sleep(Duration::from_millis(50));
              }
              if !is_playing {
                return ExecutionResult::GateFailed {
                  step_id: step.id.clone(),
                  message: format!("Playback status did not transition to Playing within timeout: status={:?}", status),
                  consecutive_failures: 1,
                };
              }
            }
          }
          steps_executed += 1;
        }
        StepKind::WgcCapture => {
          let is_still_minimized = check_minimized_and_restore(&ctx.window, self.allow_restore);

          if is_still_minimized {
            // skipped_minimized, zero window mutation redline preserves user state
            steps_executed += 1;
          } else if op.execution_mode == ExecutionMode::Fast {
            match capture_window_health_cached(&ctx.window) {
              Ok(health) => {
                if health.alive {
                  steps_executed += 1;
                } else {
                  return ExecutionResult::GateFailed {
                    step_id: step.id.clone(),
                    message: format!("Fast window health check failed: alive={}, non_black_ratio={}", health.alive, health.non_black_ratio),
                    consecutive_failures: 1,
                  };
                }
              }
              Err(e) => {
                return ExecutionResult::GateFailed {
                  step_id: step.id.clone(),
                  message: format!("Fast window health check error: {e}"),
                  consecutive_failures: 1,
                };
              }
            }
          } else {
            match capture_window_health_strict(&ctx.window) {
              Ok(health) => {
                if health.alive && health.is_fresh {
                  steps_executed += 1;
                } else {
                  return ExecutionResult::GateFailed {
                    step_id: step.id.clone(),
                    message: format!(
                      "Strict window health check failed: alive={}, is_fresh={}, non_black_ratio={}",
                      health.alive, health.is_fresh, health.non_black_ratio
                    ),
                    consecutive_failures: 1,
                  };
                }
              }
              Err(e) => {
                return ExecutionResult::GateFailed {
                  step_id: step.id.clone(),
                  message: format!("Strict window health check error: {e}"),
                  consecutive_failures: 1,
                };
              }
            }
          }
        }
        StepKind::Other => {
          steps_executed += 1;
        }
      }
    }

    let effective_confirmed = if op.execution_mode == ExecutionMode::Fast {
      false
    } else if op.steps.iter().any(|s| matches!(classify_step(s), StepKind::SkipNext)) {
      skip_next_confirmed
    } else {
      true
    };

    ExecutionResult::success(steps_executed, op.execution_mode, effective_confirmed)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use auv_auto_loop::models::{CompilationMetadata, OPERATION_SCHEMA_VERSION, TargetMetadata};
  use std::collections::HashMap;

  fn dummy_operation(name: &str, mode: ExecutionMode) -> OperationDef {
    OperationDef {
      schema_version: OPERATION_SCHEMA_VERSION.to_string(),
      name: name.to_string(),
      description: "Test operation definition".to_string(),
      execution_mode: mode,
      compilation_metadata: CompilationMetadata {
        compiler: "test-compiler".to_string(),
        source_record: "test-record.json".to_string(),
        date: "2026-10-08".to_string(),
        crux_goal: "test_goal".to_string(),
      },
      target: TargetMetadata {
        app_name: "QQMusic.exe".to_string(),
        backend: "windows.smtc+coreaudio+wgc".to_string(),
      },
      preconditions: vec![],
      parameters: vec![],
      steps: vec![OperationStepDef {
        id: "step_smtc_query".to_string(),
        name: "Query state".to_string(),
        description: "SMTC query step".to_string(),
        action: serde_json::json!({ "type": "smtc_query" }),
        verification_gate: VerificationGateDef::SmtcSessionPresent {
          timeout_ms: 1000,
          escalate_on_mismatch: "escalate_to_vlm".to_string(),
        },
        is_unverified: false,
      }],
      tags: vec![],
    }
  }

  fn dummy_request(requested_mode: Option<ExecutionMode>) -> TaskRequest {
    TaskRequest {
      app_name: "QQMusic.exe".to_string(),
      task_name: "test_task".to_string(),
      instruction: "test instruction".to_string(),
      requested_mode,
      current_context: HashMap::new(),
      embedding_vector: None,
    }
  }

  #[test]
  fn test_mode_conflict_gate_zero_side_effects() {
    let executor = WindowsProductionExecutor::default();

    // 1. Verified operation with Fast request -> Rejected before driver calls
    let verified_op = dummy_operation("qqmusic.prepare_playback", ExecutionMode::Verified);
    let fast_req = dummy_request(Some(ExecutionMode::Fast));

    let res1 = executor.execute(&verified_op, &fast_req);
    match res1 {
      ExecutionResult::ModeConflictError {
        operation_name,
        message,
      } => {
        assert_eq!(operation_name, "qqmusic.prepare_playback");
        assert!(message.contains("Driver boundary mode conflict"));
        assert!(message.contains("fail-closed, zero side effects"));
      }
      other => panic!("Expected ModeConflictError for Verified op + Fast request, got {:?}", other),
    }

    // 2. Fast operation with Verified request -> Rejected before driver calls
    let fast_op = dummy_operation("qqmusic.prepare_playback_fast", ExecutionMode::Fast);
    let verified_req = dummy_request(Some(ExecutionMode::Verified));

    let res2 = executor.execute(&fast_op, &verified_req);
    match res2 {
      ExecutionResult::ModeConflictError {
        operation_name,
        message,
      } => {
        assert_eq!(operation_name, "qqmusic.prepare_playback_fast");
        assert!(message.contains("Driver boundary mode conflict"));
        assert!(message.contains("fail-closed, zero side effects"));
      }
      other => panic!("Expected ModeConflictError for Fast op + Verified request, got {:?}", other),
    }
  }

  #[test]
  fn test_missing_mode_rejection_fail_closed() {
    let executor = WindowsProductionExecutor::default();

    let undeclared_req = dummy_request(None);
    let verified_op = dummy_operation("qqmusic.prepare_playback", ExecutionMode::Verified);
    let fast_op = dummy_operation("qqmusic.prepare_playback_fast", ExecutionMode::Fast);

    // Verified op with None requested_mode
    let res_ver = executor.execute(&verified_op, &undeclared_req);
    match res_ver {
      ExecutionResult::ModeConflictError {
        operation_name,
        message,
      } => {
        assert_eq!(operation_name, "qqmusic.prepare_playback");
        assert!(message.contains("REJECT_MODE_UNDECLARED"));
        assert!(message.contains("driver execution requires explicit execution_mode (fail-closed)"));
      }
      other => panic!("Expected ModeConflictError for undeclared mode on Verified op, got {:?}", other),
    }

    // Fast op with None requested_mode
    let res_fast = executor.execute(&fast_op, &undeclared_req);
    match res_fast {
      ExecutionResult::ModeConflictError {
        operation_name,
        message,
      } => {
        assert_eq!(operation_name, "qqmusic.prepare_playback_fast");
        assert!(message.contains("REJECT_MODE_UNDECLARED"));
        assert!(message.contains("driver execution requires explicit execution_mode (fail-closed)"));
      }
      other => panic!("Expected ModeConflictError for undeclared mode on Fast op, got {:?}", other),
    }
  }

  #[test]
  fn test_fast_mode_confirmed_false_invariant() {
    // 1. Result constructor invariant: Even if confirmed: true is supplied, Fast mode MUST clamp to false
    let result = ExecutionResult::success(4, ExecutionMode::Fast, true);
    assert_eq!(result.execution_mode(), Some(ExecutionMode::Fast));
    assert!(!result.confirmed(), "Fast mode must NEVER be confirmed (constructor invariant)");

    // 2. Verified mode constructor preserves confirmation
    let verified_result = ExecutionResult::success(4, ExecutionMode::Verified, true);
    assert_eq!(verified_result.execution_mode(), Some(ExecutionMode::Verified));
    assert!(verified_result.confirmed(), "Verified mode preserves confirmed: true");

    // 3. Executor default properties
    let executor = WindowsProductionExecutor::default();
    assert!(!executor.allow_restore);

    let executor_restore = WindowsProductionExecutor::with_allow_restore(true);
    assert!(executor_restore.allow_restore);
  }
}
