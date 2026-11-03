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
use crate::wgc::{WindowHealth, capture_window_health_cached, capture_window_health_strict};
use crate::window::{list_windows, without_exe_suffix};

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

/// Matches a window's application name against the expected process name.
///
/// Rejects substring false positives (e.g. `NotQQMusic.exe` when expecting `QQMusic.exe`).
pub(crate) fn matches_target_window_app(window: &Window, expected_app_process: &str) -> bool {
  let Some(app_name) = window.app_name.as_deref() else {
    return false;
  };
  let expected_trim = expected_app_process.trim();
  let app_trim = app_name.trim();
  if expected_trim.is_empty() || app_trim.is_empty() {
    return false;
  }
  let expected_stem = without_exe_suffix(expected_trim);
  let app_stem = without_exe_suffix(app_trim);
  app_trim.eq_ignore_ascii_case(expected_trim) || app_stem.eq_ignore_ascii_case(expected_stem)
}

/// Selects the target window for an application process from enumerated windows.
///
/// Enforces:
/// - Exact identity matching (no loose substring authorization)
/// - Ambiguity handling: if multiple candidate windows match, returns an error (fail-closed).
pub(crate) fn select_target_window(windows: &[Window], app_process: &str) -> DriverResult<Window> {
  let matches: Vec<&Window> = windows.iter().filter(|w| matches_target_window_app(w, app_process)).collect();

  match matches.as_slice() {
    [] => Err(DriverError::NotFound {
      target: format!("{app_process} window"),
    }),
    [single] => Ok((*single).clone()),
    multiple => Err(DriverError::InvalidInput {
      message: format!("Ambiguous match: multiple windows (count: {}) matched target '{app_process}'", multiple.len()),
    }),
  }
}

impl WindowsOperationContext {
  /// Resolves the context targeting QQ Music, optionally utilizing a cached CoreAudio endpoint ID.
  pub fn resolve(cached_endpoint_id: Option<&str>) -> DriverResult<Self> {
    Self::resolve_for_target("QQMusic.exe", "QQMusic.exe", cached_endpoint_id)
  }

  /// Resolves context for a given SMTC target app and process executable name.
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
    let window = select_target_window(&windows, app_process)?;
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
}

fn classify_step(step: &OperationStepDef) -> Result<StepKind, String> {
  let action_type = match step.action.get("type") {
    Some(val) => match val.as_str() {
      Some(s) => s,
      None => return Err(format!("action.type in step '{}' is not a string", step.id)),
    },
    None => {
      return Err(format!(
        "step '{}' is missing required 'action.type'; state-changing actions cannot be inferred from step ID or name",
        step.id
      ));
    }
  };

  let step_kind = match action_type {
    "smtc_query" => StepKind::SmtcQuery,
    "ensure_playing_and_volume" | "volume" | "set_volume" => StepKind::EnsurePlayingAndVolume,
    "skip_next" | "next" => StepKind::SkipNext,
    "play" => StepKind::Play,
    "wgc_capture" | "capture" => StepKind::WgcCapture,
    unknown => return Err(format!("unrecognized action.type '{unknown}' in step '{}'", step.id)),
  };

  // Preflight action and verification gate compatibility:
  // Prevent incompatible action/gate pairings from claiming false confirmation or bypassing verification.
  match (&step_kind, &step.verification_gate) {
    (StepKind::Play, VerificationGateDef::StatusAndVolumeGate { .. }) => {
      return Err(format!(
        "step '{}' has action.type 'play' which is incompatible with StatusAndVolumeGate (requires volume verification; use 'ensure_playing_and_volume')",
        step.id
      ));
    }
    (StepKind::Play, VerificationGateDef::TitleChangeGate { .. }) => {
      return Err(format!("step '{}' has action.type 'play' which is incompatible with TitleChangeGate", step.id));
    }
    (StepKind::Play, VerificationGateDef::WgcAliveGate { .. }) => {
      return Err(format!("step '{}' has action.type 'play' which is incompatible with WgcAliveGate", step.id));
    }
    (StepKind::EnsurePlayingAndVolume, VerificationGateDef::TitleChangeGate { .. }) => {
      return Err(format!("step '{}' has action.type '{action_type}' which is incompatible with TitleChangeGate", step.id));
    }
    (StepKind::EnsurePlayingAndVolume, VerificationGateDef::WgcAliveGate { .. }) => {
      return Err(format!("step '{}' has action.type '{action_type}' which is incompatible with WgcAliveGate", step.id));
    }
    (StepKind::SkipNext, VerificationGateDef::StatusAndVolumeGate { .. }) => {
      return Err(format!("step '{}' has action.type '{action_type}' which is incompatible with StatusAndVolumeGate", step.id));
    }
    (StepKind::SkipNext, VerificationGateDef::WgcAliveGate { .. }) => {
      return Err(format!("step '{}' has action.type '{action_type}' which is incompatible with WgcAliveGate", step.id));
    }
    (StepKind::WgcCapture, VerificationGateDef::StatusAndVolumeGate { .. }) => {
      return Err(format!("step '{}' has action.type '{action_type}' which is incompatible with StatusAndVolumeGate", step.id));
    }
    (StepKind::WgcCapture, VerificationGateDef::TitleChangeGate { .. }) => {
      return Err(format!("step '{}' has action.type '{action_type}' which is incompatible with TitleChangeGate", step.id));
    }
    (StepKind::SmtcQuery, VerificationGateDef::StatusAndVolumeGate { .. }) => {
      return Err(format!("step '{}' has action.type 'smtc_query' which is incompatible with StatusAndVolumeGate", step.id));
    }
    (StepKind::SmtcQuery, VerificationGateDef::TitleChangeGate { .. }) => {
      return Err(format!("step '{}' has action.type 'smtc_query' which is incompatible with TitleChangeGate", step.id));
    }
    (StepKind::SmtcQuery, VerificationGateDef::WgcAliveGate { .. }) => {
      return Err(format!("step '{}' has action.type 'smtc_query' which is incompatible with WgcAliveGate", step.id));
    }
    _ => {}
  }

  // Validate gate values before any earlier step can dispatch an action. These
  // branches execute fixed capabilities; they cannot satisfy arbitrary statuses.
  match &step.verification_gate {
    VerificationGateDef::StatusAndVolumeGate {
      expected_status,
      expected_volume,
      volume_tolerance,
      timeout_ms,
      ..
    } => {
      if !expected_status.eq_ignore_ascii_case("Playing") {
        return Err(format!("step '{}' has unsupported expected_status '{expected_status}'; only Playing is verified", step.id));
      }
      if !expected_volume.is_finite() || !(0.0..=1.0).contains(expected_volume) {
        return Err(format!("step '{}' expected_volume must be finite and in 0..=1", step.id));
      }
      if !volume_tolerance.is_finite() || !(0.0..=1.0).contains(volume_tolerance) {
        return Err(format!("step '{}' volume_tolerance must be finite and in 0..=1", step.id));
      }
      if *timeout_ms == 0 {
        return Err(format!("step '{}' gate timeout_ms must be greater than zero", step.id));
      }
    }
    VerificationGateDef::WgcAliveGate {
      min_non_black_ratio,
      timeout_ms,
      ..
    } => {
      if !min_non_black_ratio.is_finite() || !(0.0..=1.0).contains(min_non_black_ratio) {
        return Err(format!("step '{}' min_non_black_ratio must be a finite fraction in 0..=1", step.id));
      }
      if *timeout_ms == 0 {
        return Err(format!("step '{}' gate timeout_ms must be greater than zero", step.id));
      }
    }
    _ => {}
  }

  Ok(step_kind)
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

fn execute_wgc_capture_step(
  step: &OperationStepDef,
  mode: ExecutionMode,
  is_still_minimized: bool,
  health_check: impl FnOnce() -> Result<WindowHealth, String>,
) -> Result<bool, ExecutionResult> {
  if is_still_minimized {
    if mode == ExecutionMode::Verified {
      return Err(ExecutionResult::GateFailed {
        step_id: step.id.clone(),
        message: "Target window is minimized; WGC capture verification cannot verify an iconic window".to_string(),
        consecutive_failures: 1,
      });
    }
    // In Fast mode, skipping a minimized window is not verified and does not count as an executed step.
    return Ok(false);
  }

  let started = Instant::now();
  let health_result = health_check();
  if mode == ExecutionMode::Verified
    && let VerificationGateDef::WgcAliveGate { timeout_ms, .. } = &step.verification_gate
    && started.elapsed() >= Duration::from_millis(*timeout_ms)
  {
    // NOTICE: the native capture has its own bounded wait and is synchronous.
    // This deadline rejects late confirmation; it cannot cancel an in-flight OS
    // call. Cancellation requires a separate driver-lifecycle slice.
    return Err(ExecutionResult::GateFailed {
      step_id: step.id.clone(),
      message: format!("WGC verification exceeded gate timeout_ms={timeout_ms}"),
      consecutive_failures: 1,
    });
  }

  match health_result {
    Ok(health) => {
      if mode == ExecutionMode::Fast {
        if health.alive {
          Ok(true)
        } else {
          Err(ExecutionResult::GateFailed {
            step_id: step.id.clone(),
            message: format!("Fast window health check failed: alive={}, non_black_ratio={}", health.alive, health.non_black_ratio),
            consecutive_failures: 1,
          })
        }
      } else {
        if !health.is_fresh {
          return Err(ExecutionResult::GateFailed {
            step_id: step.id.clone(),
            message: format!(
              "Strict window health check failed: frame is stale (is_fresh=false), alive={}, non_black_ratio={}",
              health.alive, health.non_black_ratio
            ),
            consecutive_failures: 1,
          });
        }
        if let VerificationGateDef::WgcAliveGate {
          min_non_black_ratio,
          ..
        } = &step.verification_gate
        {
          // Gates use fractions; WindowHealth always uses percentages, even
          // below 1%. The generic alive flag uses a fixed 50% policy and cannot
          // override the declared threshold.
          let gate_threshold_pct = min_non_black_ratio * 100.0;
          let health_ratio_pct = health.non_black_ratio;
          if !min_non_black_ratio.is_finite()
            || !(0.0..=1.0).contains(min_non_black_ratio)
            || !health_ratio_pct.is_finite()
            || !(0.0..=100.0).contains(&health_ratio_pct)
            || health.width == 0
            || health.height == 0
            || health_ratio_pct < gate_threshold_pct
          {
            return Err(ExecutionResult::GateFailed {
              step_id: step.id.clone(),
              message: format!(
                "Strict window health check failed: non_black_ratio {:.2}% is invalid or below gate threshold {:.2}%",
                health_ratio_pct, gate_threshold_pct
              ),
              consecutive_failures: 1,
            });
          }
        } else if !health.alive {
          return Err(ExecutionResult::GateFailed {
            step_id: step.id.clone(),
            message: format!("Strict window health check failed: window is not alive, non_black_ratio={}", health.non_black_ratio),
            consecutive_failures: 1,
          });
        }
        Ok(true)
      }
    }
    Err(e) => Err(ExecutionResult::GateFailed {
      step_id: step.id.clone(),
      message: format!("Window health check error: {e}"),
      consecutive_failures: 1,
    }),
  }
}

fn calculate_operation_confirmed(
  mode: ExecutionMode,
  steps: &[OperationStepDef],
  classified_kinds: &[StepKind],
  skip_next_confirmed: bool,
) -> bool {
  let has_unverified_steps = steps.iter().any(|s| {
    s.is_unverified
      || matches!(&s.verification_gate, VerificationGateDef::UnverifiedFallback { .. })
      || s.verification_gate.requires_verified_mode().is_none()
  });

  if mode == ExecutionMode::Fast || has_unverified_steps || steps.is_empty() {
    false
  } else if classified_kinds.iter().any(|kind| *kind == StepKind::SkipNext) {
    skip_next_confirmed
  } else {
    true
  }
}

/// Applies the declared title gate to the broader media identity verdict.
fn evaluate_step_track_change(gate: &VerificationGateDef, previous: &TrackIdentity, current: &TrackIdentity) -> TrackChangeVerdict {
  if matches!(
    gate,
    VerificationGateDef::TitleChangeGate {
      require_title_change: true,
      ..
    }
  ) && (previous.title.is_empty() || current.title.is_empty() || previous.title == current.title)
  {
    return TrackChangeVerdict::Unchanged;
  }
  evaluate_track_change(previous, current, false).0
}

/// Validates that the operation and request target a supported Windows application and backend.
///
/// Fails closed before any COM or Windows driver context resolution if:
/// - Operation target application is not `QQMusic.exe` (case-insensitive)
/// - Request application is empty or does not match operation target application (case-insensitive)
/// - Target backend is not `windows.smtc+coreaudio+wgc` (exact match)
/// - Any step declares an `app_id` that is not a string or does not match operation target application
fn validate_supported_target(op: &OperationDef, request: &TaskRequest) -> Result<(), (String, String)> {
  if !op.target.app_name.eq_ignore_ascii_case("QQMusic.exe") {
    return Err((
      "target_validation".to_string(),
      format!(
        "Unsupported operation target app '{}'; only 'QQMusic.exe' is currently supported by WindowsProductionExecutor",
        op.target.app_name
      ),
    ));
  }

  if request.app_name.is_empty() || !request.app_name.eq_ignore_ascii_case(&op.target.app_name) {
    return Err((
      "target_validation".to_string(),
      format!("Request app '{}' does not match operation target app '{}'", request.app_name, op.target.app_name),
    ));
  }

  if op.target.backend != "windows.smtc+coreaudio+wgc" {
    return Err((
      "target_validation".to_string(),
      format!("Unsupported operation target backend '{}'; only 'windows.smtc+coreaudio+wgc' is supported", op.target.backend),
    ));
  }

  for step in &op.steps {
    if let Some(val) = step.action.get("app_id") {
      match val.as_str() {
        Some(step_app) => {
          if !step_app.eq_ignore_ascii_case(&op.target.app_name) {
            return Err((
              step.id.clone(),
              format!(
                "step '{}' specifies app_id '{}' which does not match operation target app '{}'",
                step.id, step_app, op.target.app_name
              ),
            ));
          }
        }
        None => {
          return Err((step.id.clone(), format!("step '{}' has invalid 'app_id' field: expected string, found {val}", step.id)));
        }
      }
    }
  }

  Ok(())
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

    // Pre-flight check: validate that ALL steps are recognized and can be executed.
    // Must fail-closed BEFORE acquiring driver context or dispatching any side-effects.
    let mut classified_steps = Vec::with_capacity(op.steps.len());
    for step in &op.steps {
      match classify_step(step) {
        Ok(kind) => classified_steps.push((step, kind)),
        Err(err) => {
          return ExecutionResult::GateFailed {
            step_id: step.id.clone(),
            message: format!("Unrecognized or unsupported operation step: {err}"),
            consecutive_failures: 1,
          };
        }
      }
    }

    // Target validation: verify supported app, backend, and step app_id BEFORE COM/driver context resolution
    if let Err((step_id, message)) = validate_supported_target(op, request) {
      return ExecutionResult::GateFailed {
        step_id,
        message,
        consecutive_failures: 1,
      };
    }

    // Acquire operation context (first driver call occurs ONLY after mode gate and pre-flight step checks pass)
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

    for (step, step_kind) in &classified_steps {
      match *step_kind {
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
          let (target_volume, volume_tolerance, play_poll_timeout) = match &step.verification_gate {
            VerificationGateDef::StatusAndVolumeGate {
              expected_volume,
              volume_tolerance,
              timeout_ms,
              ..
            } => (*expected_volume, *volume_tolerance, Duration::from_millis(*timeout_ms)),
            _ => {
              let vol = step.action.get("volume").and_then(|v| v.as_f64()).map(|v| v as f32).unwrap_or(DEFAULT_TARGET_VOLUME);
              (vol, DEFAULT_VOLUME_TOLERANCE, DEFAULT_PLAY_POLL_TIMEOUT)
            }
          };
          let step2_opts = Step2Options {
            target_volume,
            volume_tolerance,
            play_poll_timeout,
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

            let s3_timeout = match &step.verification_gate {
              VerificationGateDef::TitleChangeGate { timeout_ms, .. } => Duration::from_millis(*timeout_ms),
              _ => Duration::from_millis(3000),
            };
            let mut change_verdict = TrackChangeVerdict::Unchanged;
            let mut backoff = Duration::from_millis(10);

            while t_s3_verif.elapsed() < s3_timeout {
              let remaining = s3_timeout.saturating_sub(t_s3_verif.elapsed());
              let _ = rx.recv_timeout(backoff.min(remaining));

              if let Ok(curr_meta) = ctx.session.track_metadata() {
                let curr_identity = TrackIdentity::from(&curr_meta);
                let verdict = evaluate_step_track_change(&step.verification_gate, &prev_identity, &curr_identity);
                change_verdict = verdict;

                if verdict == TrackChangeVerdict::Changed && t_s3_verif.elapsed() < s3_timeout {
                  break;
                }
                if t_s3_verif.elapsed() >= s3_timeout {
                  change_verdict = TrackChangeVerdict::Unchanged;
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
          let executed = match execute_wgc_capture_step(step, op.execution_mode, is_still_minimized, || {
            if op.execution_mode == ExecutionMode::Fast {
              capture_window_health_cached(&ctx.window).map_err(|e| e.to_string())
            } else {
              capture_window_health_strict(&ctx.window).map_err(|e| e.to_string())
            }
          }) {
            Ok(ex) => ex,
            Err(failed) => return failed,
          };
          if executed {
            steps_executed += 1;
          }
        }
      }
    }

    let kinds: Vec<StepKind> = classified_steps.iter().map(|(_, k)| *k).collect();
    let effective_confirmed = calculate_operation_confirmed(op.execution_mode, &op.steps, &kinds, skip_next_confirmed);

    ExecutionResult::success(steps_executed, op.execution_mode, effective_confirmed)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use auv_auto_loop::models::{CompilationMetadata, OPERATION_SCHEMA_VERSION, TargetMetadata};
  use std::collections::HashMap;

  fn wgc_step(min_non_black_ratio: f64) -> OperationStepDef {
    OperationStepDef {
      id: "wgc_regression".to_string(),
      name: "Check window pixels".to_string(),
      description: "Check the declared non-black threshold".to_string(),
      action: serde_json::json!({ "type": "wgc_capture" }),
      verification_gate: VerificationGateDef::WgcAliveGate {
        min_non_black_ratio,
        timeout_ms: 1000,
        escalate_on_mismatch: "none".to_string(),
      },
      is_unverified: false,
    }
  }

  // https://github.com/moeru-ai/auv/pull/423
  // ROOT CAUSE:
  //
  // If a frame met a lower threshold, verification failed because it used the fixed 50% alive flag.
  // Before the fix, a fresh 40% frame failed a 30% gate.
  // The fix uses the declared threshold and the producer's percentage units.
  #[test]
  fn pr423_wgc_uses_declared_threshold_instead_of_fixed_alive_flag() {
    let result = execute_wgc_capture_step(&wgc_step(0.30), ExecutionMode::Verified, false, || {
      Ok(WindowHealth {
        width: 800,
        height: 600,
        alive: false,
        is_fresh: true,
        non_black_ratio: 40.0,
      })
    });
    assert_eq!(result, Ok(true));
  }

  // https://github.com/moeru-ai/auv/pull/423
  #[test]
  fn pr423_wgc_percentage_below_one_is_not_treated_as_fraction() {
    let result = execute_wgc_capture_step(&wgc_step(0.80), ExecutionMode::Verified, false, || {
      Ok(WindowHealth {
        width: 800,
        height: 600,
        alive: true,
        is_fresh: true,
        non_black_ratio: 0.95,
      })
    });
    assert!(matches!(result, Err(ExecutionResult::GateFailed { .. })));
  }

  // https://github.com/moeru-ai/auv/pull/423
  #[test]
  fn pr423_preflight_rejects_invalid_wgc_thresholds() {
    for ratio in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.1, 1.01, 80.0] {
      let step = wgc_step(ratio);
      let error = classify_step(&step).expect_err("invalid gate must fail before driver calls");
      assert!(error.contains("min_non_black_ratio"), "{error}");
    }
  }

  // https://github.com/moeru-ai/auv/pull/423
  #[test]
  fn pr423_preflight_rejects_invalid_volume_gate_values() {
    for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.1, 1.1] {
      let mut step = wgc_step(0.5);
      step.action = serde_json::json!({ "type": "ensure_playing_and_volume" });
      step.verification_gate = VerificationGateDef::StatusAndVolumeGate {
        expected_status: "Playing".to_string(),
        expected_volume: invalid,
        volume_tolerance: 0.05,
        timeout_ms: 1000,
        escalate_on_mismatch: "none".to_string(),
      };
      let error = classify_step(&step).expect_err("invalid target must fail before driver calls");
      assert!(error.contains("expected_volume"), "{error}");
      if let VerificationGateDef::StatusAndVolumeGate {
        expected_volume,
        volume_tolerance,
        ..
      } = &mut step.verification_gate
      {
        *expected_volume = 0.4;
        *volume_tolerance = invalid;
      }
      let error = classify_step(&step).expect_err("invalid tolerance must fail before driver calls");
      assert!(error.contains("volume_tolerance"), "{error}");
    }
  }

  // https://github.com/moeru-ai/auv/pull/423
  #[test]
  fn pr423_preflight_rejects_invalid_trailing_gate_before_context_resolution() {
    let mut operation = dummy_operation("invalid.trailing.gate", ExecutionMode::Verified);
    operation.steps.push(wgc_step(80.0));
    let result = WindowsProductionExecutor::new().execute(&operation, &dummy_request(Some(ExecutionMode::Verified)));
    match result {
      ExecutionResult::GateFailed {
        step_id, message, ..
      } => {
        assert_eq!(step_id, "wgc_regression");
        assert!(message.contains("min_non_black_ratio"), "{message}");
      }
      other => panic!("invalid gate must fail preflight: {other:?}"),
    }
  }

  // https://github.com/moeru-ai/auv/pull/423
  #[test]
  fn pr423_wgc_rejects_invalid_health_ratios_and_zero_timeout() {
    for ratio in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 101.0] {
      let result = execute_wgc_capture_step(&wgc_step(0.5), ExecutionMode::Verified, false, || {
        Ok(WindowHealth {
          width: 800,
          height: 600,
          alive: true,
          is_fresh: true,
          non_black_ratio: ratio,
        })
      });
      assert!(matches!(result, Err(ExecutionResult::GateFailed { .. })), "invalid ratio {ratio} must not be confirmed");
    }
    let mut step = wgc_step(0.5);
    if let VerificationGateDef::WgcAliveGate { timeout_ms, .. } = &mut step.verification_gate {
      *timeout_ms = 0;
    }
    let error = classify_step(&step).expect_err("zero deadline must fail preflight");
    assert!(error.contains("timeout_ms"), "{error}");
  }

  // https://github.com/moeru-ai/auv/pull/423
  #[test]
  fn pr423_preflight_rejects_unsupported_playback_status() {
    let mut step = wgc_step(0.5);
    step.action = serde_json::json!({ "type": "ensure_playing_and_volume" });
    step.verification_gate = VerificationGateDef::StatusAndVolumeGate {
      expected_status: "Paused".to_string(),
      expected_volume: 0.4,
      volume_tolerance: 0.05,
      timeout_ms: 2000,
      escalate_on_mismatch: "none".to_string(),
    };
    let error = classify_step(&step).expect_err("executor only verifies Playing");
    assert!(error.contains("expected_status"), "{error}");
  }

  // https://github.com/moeru-ai/auv/pull/423
  #[test]
  fn pr423_title_gate_rejects_artist_only_identity_change() {
    let previous = TrackIdentity {
      title: "same song".to_string(),
      artist: "first artist".to_string(),
      ..Default::default()
    };
    let current = TrackIdentity {
      artist: "second artist".to_string(),
      ..previous.clone()
    };
    let gate = VerificationGateDef::TitleChangeGate {
      require_title_change: true,
      timeout_ms: 1000,
      escalate_on_mismatch: "none".to_string(),
    };
    let verdict = evaluate_step_track_change(&gate, &previous, &current);
    assert_ne!(verdict, TrackChangeVerdict::Changed, "artist changes cannot satisfy a required title change");
    let changed_title = TrackIdentity {
      title: "next song".to_string(),
      ..current
    };
    assert_eq!(evaluate_step_track_change(&gate, &previous, &changed_title), TrackChangeVerdict::Changed);
  }

  // https://github.com/moeru-ai/auv/pull/423
  #[test]
  fn pr423_wgc_rejects_late_confirmation() {
    let mut step = wgc_step(0.5);
    if let VerificationGateDef::WgcAliveGate { timeout_ms, .. } = &mut step.verification_gate {
      *timeout_ms = 1;
    }
    let result = execute_wgc_capture_step(&step, ExecutionMode::Verified, false, || {
      std::thread::sleep(Duration::from_millis(5));
      Ok(WindowHealth {
        width: 800,
        height: 600,
        alive: true,
        is_fresh: true,
        non_black_ratio: 95.0,
      })
    });
    match result {
      Err(ExecutionResult::GateFailed { message, .. }) => assert!(message.contains("timeout_ms=1"), "{message}"),
      other => panic!("late sample must fail its gate: {other:?}"),
    }
  }

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
  #[test]
  fn test_classify_step_rejects_unknown_action_type() {
    let step = OperationStepDef {
      id: "step_custom_unknown".to_string(),
      name: "Play something".to_string(),
      description: "Unknown action".to_string(),
      action: serde_json::json!({ "type": "some_unrecognized_action" }),
      verification_gate: VerificationGateDef::CustomAssertion {
        expression: "true".to_string(),
        timeout_ms: 100,
        escalate_on_mismatch: "none".to_string(),
      },
      is_unverified: false,
    };

    let err = classify_step(&step).expect_err("unknown action.type must be rejected");
    assert!(err.contains("unrecognized action.type 'some_unrecognized_action'"), "{err}");
    assert!(err.contains("step_custom_unknown"), "{err}");
  }

  #[test]
  fn test_classify_step_rejects_substring_authorization_without_typed_gate() {
    // ID/Name containing 'play' or 'next' with no recognized action type and custom assertion
    // MUST NOT authorize real SMTC operations.
    let play_step = OperationStepDef {
      id: "play_audio_step".to_string(),
      name: "Play track".to_string(),
      description: "Step with play in name".to_string(),
      action: serde_json::json!({}),
      verification_gate: VerificationGateDef::CustomAssertion {
        expression: "true".to_string(),
        timeout_ms: 100,
        escalate_on_mismatch: "none".to_string(),
      },
      is_unverified: false,
    };
    let err = classify_step(&play_step).expect_err("substring 'play' must not authorize execution");
    assert!(err.contains("missing required 'action.type'"), "{err}");

    let next_step = OperationStepDef {
      id: "skip_to_next".to_string(),
      name: "Next Track".to_string(),
      description: "Step with next in name".to_string(),
      action: serde_json::json!({}),
      verification_gate: VerificationGateDef::CustomAssertion {
        expression: "true".to_string(),
        timeout_ms: 100,
        escalate_on_mismatch: "none".to_string(),
      },
      is_unverified: false,
    };
    let err = classify_step(&next_step).expect_err("substring 'next' must not authorize execution");
    assert!(err.contains("missing required 'action.type'"), "{err}");
  }

  #[test]
  fn test_classify_step_rejects_missing_action_type_with_status_and_volume_gate() {
    // A step named play... with missing action.type and StatusAndVolumeGate
    // MUST NOT authorize real SMTC operations or false confirmation.
    let step = OperationStepDef {
      id: "play_music_untyped".to_string(),
      name: "Play music step".to_string(),
      description: "Step with play in name and no action.type".to_string(),
      action: serde_json::json!({ "app_id": "QQMusic.exe" }),
      verification_gate: VerificationGateDef::StatusAndVolumeGate {
        expected_status: "Playing".to_string(),
        expected_volume: 0.40,
        volume_tolerance: 0.05,
        timeout_ms: 2000,
        escalate_on_mismatch: "none".to_string(),
      },
      is_unverified: false,
    };
    let err = classify_step(&step).expect_err("missing action.type must be rejected");
    assert!(err.contains("missing required 'action.type'"), "{err}");

    // In execution, preflight rejection yields GateFailed with steps_executed=0 and confirmed=false
    let executor = WindowsProductionExecutor::default();
    let req = dummy_request(Some(ExecutionMode::Verified));
    let mut op = dummy_operation("custom.no_action_type_op", ExecutionMode::Verified);
    op.steps = vec![step];
    let res = executor.execute(&op, &req);
    match res {
      ExecutionResult::GateFailed {
        step_id, message, ..
      } => {
        assert_eq!(step_id, "play_music_untyped");
        assert!(message.contains("missing required 'action.type'"));
      }
      other => panic!("expected GateFailed, got {other:?}"),
    }
  }

  #[test]
  fn test_classify_step_rejects_explicit_play_with_status_and_volume_gate() {
    // An explicit action.type=play combined with StatusAndVolumeGate MUST be rejected
    // because Play arm does not verify volume and cannot bypass the gate.
    let step = OperationStepDef {
      id: "step_play_with_volume_gate".to_string(),
      name: "Play action with volume gate".to_string(),
      description: "Play action incompatible with volume gate".to_string(),
      action: serde_json::json!({ "type": "play", "app_id": "QQMusic.exe" }),
      verification_gate: VerificationGateDef::StatusAndVolumeGate {
        expected_status: "Playing".to_string(),
        expected_volume: 0.40,
        volume_tolerance: 0.05,
        timeout_ms: 2000,
        escalate_on_mismatch: "none".to_string(),
      },
      is_unverified: false,
    };
    let err = classify_step(&step).expect_err("explicit play with volume gate must be rejected");
    assert!(err.contains("is incompatible with StatusAndVolumeGate"), "{err}");

    // In execution, preflight rejection yields GateFailed with steps_executed=0 and confirmed=false
    let executor = WindowsProductionExecutor::default();
    let req = dummy_request(Some(ExecutionMode::Verified));
    let mut op = dummy_operation("custom.incompatible_gate_op", ExecutionMode::Verified);
    op.steps = vec![step];
    let res = executor.execute(&op, &req);
    match res {
      ExecutionResult::GateFailed {
        step_id, message, ..
      } => {
        assert_eq!(step_id, "step_play_with_volume_gate");
        assert!(message.contains("is incompatible with StatusAndVolumeGate"));
      }
      other => panic!("expected GateFailed, got {other:?}"),
    }
  }

  // TODO(repo-split): this test reads the QQ Music operations from
  // `supported/apps/auv-qqmusic`, so `crates/` still reaches into `supported/`
  // in tests. Move the test next to the operations when `supported/` splits
  // out of this repository.
  #[test]
  fn test_classify_step_qqmusic_operations_all_recognized() {
    // All steps in the QQ Music verified operation must be recognized
    let raw_verified: OperationDef =
      serde_json::from_str(include_str!("../../../supported/apps/auv-qqmusic/operations/qqmusic-prepared-playback.json"))
        .expect("qqmusic verified op json must parse");
    for step in &raw_verified.steps {
      classify_step(step).unwrap_or_else(|e| panic!("step {} in verified op must be recognized: {e}", step.id));
    }

    // All steps in the QQ Music fast operation must be recognized
    let raw_fast: OperationDef =
      serde_json::from_str(include_str!("../../../supported/apps/auv-qqmusic/operations/qqmusic-prepared-playback-fast.json"))
        .expect("qqmusic fast op json must parse");
    for step in &raw_fast.steps {
      classify_step(step).unwrap_or_else(|e| panic!("step {} in fast op must be recognized: {e}", step.id));
    }

    // Both operations must pass target validation
    let req_ver = dummy_request(Some(ExecutionMode::Verified));
    validate_supported_target(&raw_verified, &req_ver).expect("qqmusic verified operation must pass target validation");

    let req_fast = dummy_request(Some(ExecutionMode::Fast));
    validate_supported_target(&raw_fast, &req_fast).expect("qqmusic fast operation must pass target validation");
  }

  #[test]
  fn test_preflight_rejects_single_unknown_step_without_success_or_confirmation() {
    let executor = WindowsProductionExecutor::default();
    let req = dummy_request(Some(ExecutionMode::Verified));

    let mut op = dummy_operation("custom.unknown_step_op", ExecutionMode::Verified);
    op.steps = vec![OperationStepDef {
      id: "unrecognized_step_1".to_string(),
      name: "Do mystery thing".to_string(),
      description: "Mystery".to_string(),
      action: serde_json::json!({ "type": "mystery_action" }),
      verification_gate: VerificationGateDef::CustomAssertion {
        expression: "true".to_string(),
        timeout_ms: 100,
        escalate_on_mismatch: "none".to_string(),
      },
      is_unverified: false,
    }];

    let res = executor.execute(&op, &req);
    match &res {
      ExecutionResult::GateFailed {
        step_id,
        message,
        consecutive_failures,
      } => {
        assert_eq!(step_id, "unrecognized_step_1");
        assert_eq!(*consecutive_failures, 1);
        assert!(message.contains("Unrecognized or unsupported operation step"), "{message}");
        assert!(!message.contains("Bearer"), "no sensitive data leak: {message}");
      }
      other => panic!("expected GateFailed for unknown step, got {other:?}"),
    }
    assert!(!res.confirmed(), "unknown step must NEVER produce confirmed: true");
    assert!(!matches!(res, ExecutionResult::Success { .. }), "unknown step must NEVER produce Success");
  }

  #[test]
  fn test_preflight_rejects_unknown_step_after_valid_step_with_zero_side_effects() {
    let executor = WindowsProductionExecutor::default();
    let req = dummy_request(Some(ExecutionMode::Verified));

    let mut op = dummy_operation("custom.multi_step_op", ExecutionMode::Verified);
    op.steps = vec![
      // Step 1: Valid recognized action
      OperationStepDef {
        id: "step_valid_1".to_string(),
        name: "Query state".to_string(),
        description: "SMTC query step".to_string(),
        action: serde_json::json!({ "type": "smtc_query" }),
        verification_gate: VerificationGateDef::SmtcSessionPresent {
          timeout_ms: 1000,
          escalate_on_mismatch: "escalate_to_vlm".to_string(),
        },
        is_unverified: false,
      },
      // Step 2: Unrecognized action
      OperationStepDef {
        id: "step_invalid_2".to_string(),
        name: "Unknown action".to_string(),
        description: "Should fail in preflight".to_string(),
        action: serde_json::json!({ "type": "format_c_drive" }),
        verification_gate: VerificationGateDef::CustomAssertion {
          expression: "true".to_string(),
          timeout_ms: 100,
          escalate_on_mismatch: "none".to_string(),
        },
        is_unverified: false,
      },
    ];

    let res = executor.execute(&op, &req);
    match &res {
      ExecutionResult::GateFailed {
        step_id,
        message,
        consecutive_failures,
      } => {
        assert_eq!(step_id, "step_invalid_2");
        assert_eq!(*consecutive_failures, 1);
        assert!(message.contains("format_c_drive"), "{message}");
      }
      other => panic!("expected GateFailed at preflight for invalid step 2, got {other:?}"),
    }
    // Crucial: steps_executed is 0 because step 1 was never executed!
    assert!(!matches!(res, ExecutionResult::Success { .. }), "preflight failure must NEVER produce Success");
    assert!(!res.confirmed());
  }

  #[test]
  fn test_calculate_operation_confirmed_hermetic_invariants() {
    let make_step = |is_unverified: bool, gate: VerificationGateDef| OperationStepDef {
      id: "test_step".to_string(),
      name: "Test step".to_string(),
      description: "Test".to_string(),
      action: serde_json::json!({ "type": "smtc_query" }),
      verification_gate: gate,
      is_unverified,
    };

    let strong_gate = VerificationGateDef::SmtcSessionPresent {
      timeout_ms: 1000,
      escalate_on_mismatch: "none".to_string(),
    };
    let unverified_fallback_gate = VerificationGateDef::UnverifiedFallback {
      description: "fallback".to_string(),
      timeout_ms: 100,
      escalate_on_mismatch: "none".to_string(),
    };
    let custom_assertion_gate = VerificationGateDef::CustomAssertion {
      expression: "true".to_string(),
      timeout_ms: 100,
      escalate_on_mismatch: "none".to_string(),
    };

    // 1. Fast mode is NEVER confirmed, even with strong gates and skip_next_confirmed: true
    let confirmed_fast =
      calculate_operation_confirmed(ExecutionMode::Fast, &[make_step(false, strong_gate.clone())], &[StepKind::SmtcQuery], true);
    assert!(!confirmed_fast, "Fast mode must NEVER be confirmed");

    // 2. Empty steps is NEVER confirmed
    let confirmed_empty = calculate_operation_confirmed(ExecutionMode::Verified, &[], &[], true);
    assert!(!confirmed_empty, "Empty steps must NEVER be confirmed");

    // 3. Step marked is_unverified: true in Verified mode is NEVER confirmed
    let confirmed_unverified_flag =
      calculate_operation_confirmed(ExecutionMode::Verified, &[make_step(true, strong_gate.clone())], &[StepKind::SmtcQuery], true);
    assert!(!confirmed_unverified_flag, "is_unverified step must NEVER be confirmed");

    // 4. Step with UnverifiedFallback in Verified mode is NEVER confirmed
    let confirmed_fallback =
      calculate_operation_confirmed(ExecutionMode::Verified, &[make_step(false, unverified_fallback_gate)], &[StepKind::SmtcQuery], true);
    assert!(!confirmed_fallback, "UnverifiedFallback step must NEVER be confirmed");

    // 5. Step with CustomAssertion in Verified mode is NEVER confirmed (requires_verified_mode is None)
    let confirmed_custom =
      calculate_operation_confirmed(ExecutionMode::Verified, &[make_step(false, custom_assertion_gate)], &[StepKind::SmtcQuery], true);
    assert!(!confirmed_custom, "CustomAssertion step must NEVER be confirmed in Verified mode");

    // 6. SkipNext present with skip_next_confirmed: false yields confirmed: false
    let confirmed_skip_false = calculate_operation_confirmed(
      ExecutionMode::Verified,
      &[make_step(
        false,
        VerificationGateDef::TitleChangeGate {
          require_title_change: true,
          timeout_ms: 1000,
          escalate_on_mismatch: "none".to_string(),
        },
      )],
      &[StepKind::SkipNext],
      false,
    );
    assert!(!confirmed_skip_false, "SkipNext with unconfirmed title change must NOT be confirmed");

    // 7. SkipNext present with skip_next_confirmed: true yields confirmed: true
    let confirmed_skip_true = calculate_operation_confirmed(
      ExecutionMode::Verified,
      &[make_step(
        false,
        VerificationGateDef::TitleChangeGate {
          require_title_change: true,
          timeout_ms: 1000,
          escalate_on_mismatch: "none".to_string(),
        },
      )],
      &[StepKind::SkipNext],
      true,
    );
    assert!(confirmed_skip_true, "SkipNext with confirmed title change must be confirmed");

    // 8. Fully verified steps without SkipNext yields confirmed: true
    let confirmed_strong =
      calculate_operation_confirmed(ExecutionMode::Verified, &[make_step(false, strong_gate)], &[StepKind::SmtcQuery], false);
    assert!(confirmed_strong, "Fully verified steps in Verified mode must be confirmed");
  }

  #[test]
  fn test_wgc_capture_minimized_in_verified_mode_fails_gate_without_confirmation() {
    let step = OperationStepDef {
      id: "step_wgc".to_string(),
      name: "Capture window".to_string(),
      description: "Verify window liveness via WGC".to_string(),
      action: serde_json::json!({ "type": "wgc_capture" }),
      verification_gate: VerificationGateDef::WgcAliveGate {
        min_non_black_ratio: 0.5,
        timeout_ms: 1000,
        escalate_on_mismatch: "none".to_string(),
      },
      is_unverified: false,
    };

    // 1. Minimized window in Verified mode must FAIL the gate and never increment steps
    let res = execute_wgc_capture_step(&step, ExecutionMode::Verified, true, || {
      panic!("health check closure must NOT be invoked when target is minimized");
    });
    match res {
      Err(ExecutionResult::GateFailed {
        step_id,
        message,
        consecutive_failures,
      }) => {
        assert_eq!(step_id, "step_wgc");
        assert_eq!(consecutive_failures, 1);
        assert!(message.contains("minimized"), "message must explain window is minimized: {message}");
      }
      other => panic!("expected GateFailed, got {other:?}"),
    }

    // 2. Minimized window in Fast mode skips capture without counting as executed
    let fast_res = execute_wgc_capture_step(&step, ExecutionMode::Fast, true, || {
      panic!("health check closure must NOT be invoked when target is minimized");
    });
    assert_eq!(fast_res, Ok(false), "Fast mode on minimized target must return Ok(false) [not executed]");

    // 3. Unminimized window with alive=true, is_fresh=true in Verified mode succeeds and counts as executed
    let ok_res = execute_wgc_capture_step(&step, ExecutionMode::Verified, false, || {
      Ok(WindowHealth {
        width: 800,
        height: 600,
        alive: true,
        is_fresh: true,
        non_black_ratio: 95.0,
      })
    });
    assert_eq!(ok_res, Ok(true));

    // 4. Unminimized window with is_fresh=false in Verified mode fails gate
    let stale_res = execute_wgc_capture_step(&step, ExecutionMode::Verified, false, || {
      Ok(WindowHealth {
        width: 800,
        height: 600,
        alive: true,
        is_fresh: false,
        non_black_ratio: 95.0,
      })
    });
    assert!(matches!(stale_res, Err(ExecutionResult::GateFailed { .. })));
  }

  #[test]
  fn test_wgc_capture_threshold_satisfied_in_verified_mode() {
    // Gate requires 60% non-black ratio (0.60 fraction)
    let step = OperationStepDef {
      id: "step_thresh_ok".to_string(),
      name: "Assert alive".to_string(),
      description: "Assert 60% non-black".to_string(),
      action: serde_json::json!({ "type": "wgc_capture" }),
      verification_gate: VerificationGateDef::WgcAliveGate {
        min_non_black_ratio: 0.60,
        timeout_ms: 1000,
        escalate_on_mismatch: "none".to_string(),
      },
      is_unverified: false,
    };

    // WindowHealth ratios are always percentages.
    let res1 = execute_wgc_capture_step(&step, ExecutionMode::Verified, false, || {
      Ok(WindowHealth {
        width: 1024,
        height: 768,
        alive: true,
        is_fresh: true,
        non_black_ratio: 75.0,
      })
    });
    assert_eq!(res1, Ok(true));

    let res2 = execute_wgc_capture_step(&step, ExecutionMode::Verified, false, || {
      Ok(WindowHealth {
        width: 1024,
        height: 768,
        alive: true,
        is_fresh: true,
        non_black_ratio: 0.75,
      })
    });
    assert!(matches!(res2, Err(ExecutionResult::GateFailed { .. })), "0.75% is below 60%");
  }

  #[test]
  fn test_wgc_capture_threshold_not_satisfied_in_verified_mode_even_when_alive() {
    // Gate requires 80% non-black ratio (0.80 fraction)
    let step = OperationStepDef {
      id: "step_thresh_fail".to_string(),
      name: "Assert high alive".to_string(),
      description: "Assert 80% non-black".to_string(),
      action: serde_json::json!({ "type": "wgc_capture" }),
      verification_gate: VerificationGateDef::WgcAliveGate {
        min_non_black_ratio: 0.80,
        timeout_ms: 1000,
        escalate_on_mismatch: "none".to_string(),
      },
      is_unverified: false,
    };

    // Health has alive: true (since 60% >= 50%), is_fresh: true, but non_black_ratio is 60.0% < 80%
    let res = execute_wgc_capture_step(&step, ExecutionMode::Verified, false, || {
      Ok(WindowHealth {
        width: 1024,
        height: 768,
        alive: true,
        is_fresh: true,
        non_black_ratio: 60.0,
      })
    });
    match res {
      Err(ExecutionResult::GateFailed {
        step_id,
        message,
        consecutive_failures,
      }) => {
        assert_eq!(step_id, "step_thresh_fail");
        assert_eq!(consecutive_failures, 1);
        assert!(message.contains("below gate threshold"), "message must explicitly state ratio is below threshold: {message}");
      }
      other => panic!("expected GateFailed, got {other:?}"),
    }
  }

  #[test]
  fn test_target_validation_rejects_unsupported_op_target_app() {
    let executor = WindowsProductionExecutor::default();
    let mut op = dummy_operation("other.prepare", ExecutionMode::Verified);
    op.target.app_name = "Other.exe".to_string();

    let mut req = dummy_request(Some(ExecutionMode::Verified));
    req.app_name = "Other.exe".to_string();

    let res = executor.execute(&op, &req);
    match res {
      ExecutionResult::GateFailed {
        step_id,
        message,
        consecutive_failures,
      } => {
        assert_eq!(step_id, "target_validation");
        assert_eq!(consecutive_failures, 1);
        assert!(message.contains("Unsupported operation target app 'Other.exe'"), "{message}");
      }
      other => panic!("expected GateFailed for unsupported target app, got {other:?}"),
    }
  }

  #[test]
  fn test_target_validation_rejects_request_target_mismatch() {
    let executor = WindowsProductionExecutor::default();
    let op = dummy_operation("qqmusic.prepare", ExecutionMode::Verified);

    let mut req = dummy_request(Some(ExecutionMode::Verified));
    req.app_name = "Notepad.exe".to_string();

    let res = executor.execute(&op, &req);
    match res {
      ExecutionResult::GateFailed {
        step_id,
        message,
        consecutive_failures,
      } => {
        assert_eq!(step_id, "target_validation");
        assert_eq!(consecutive_failures, 1);
        assert!(message.contains("Request app 'Notepad.exe' does not match operation target app 'QQMusic.exe'"), "{message}");
      }
      other => panic!("expected GateFailed for request/target mismatch, got {other:?}"),
    }
  }

  #[test]
  fn test_target_validation_rejects_empty_request_app() {
    let executor = WindowsProductionExecutor::default();
    let op = dummy_operation("qqmusic.prepare", ExecutionMode::Verified);

    let mut req = dummy_request(Some(ExecutionMode::Verified));
    req.app_name = "".to_string();

    let res = executor.execute(&op, &req);
    match res {
      ExecutionResult::GateFailed {
        step_id,
        message,
        consecutive_failures,
      } => {
        assert_eq!(step_id, "target_validation");
        assert_eq!(consecutive_failures, 1);
        assert!(message.contains("Request app '' does not match operation target app 'QQMusic.exe'"), "{message}");
      }
      other => panic!("expected GateFailed for empty request app, got {other:?}"),
    }
  }

  #[test]
  fn test_target_validation_rejects_unsupported_backend() {
    let executor = WindowsProductionExecutor::default();
    let mut op = dummy_operation("qqmusic.prepare", ExecutionMode::Verified);
    op.target.backend = "linux.pipewire".to_string();

    let req = dummy_request(Some(ExecutionMode::Verified));

    let res = executor.execute(&op, &req);
    match res {
      ExecutionResult::GateFailed {
        step_id,
        message,
        consecutive_failures,
      } => {
        assert_eq!(step_id, "target_validation");
        assert_eq!(consecutive_failures, 1);
        assert!(message.contains("Unsupported operation target backend 'linux.pipewire'"), "{message}");
      }
      other => panic!("expected GateFailed for unsupported backend, got {other:?}"),
    }
  }

  #[test]
  fn test_target_validation_rejects_step_app_id_mismatch() {
    let executor = WindowsProductionExecutor::default();
    let mut op = dummy_operation("qqmusic.prepare", ExecutionMode::Verified);
    op.steps[0].action = serde_json::json!({
      "type": "smtc_query",
      "app_id": "Other.exe"
    });

    let req = dummy_request(Some(ExecutionMode::Verified));

    let res = executor.execute(&op, &req);
    match res {
      ExecutionResult::GateFailed {
        step_id,
        message,
        consecutive_failures,
      } => {
        assert_eq!(step_id, "step_smtc_query");
        assert_eq!(consecutive_failures, 1);
        assert!(
          message.contains("step 'step_smtc_query' specifies app_id 'Other.exe' which does not match operation target app 'QQMusic.exe'"),
          "{message}"
        );
      }
      other => panic!("expected GateFailed for step app_id mismatch, got {other:?}"),
    }
  }

  #[test]
  fn test_target_validation_rejects_step_app_id_non_string_types() {
    let invalid_values = vec![
      serde_json::json!(12345),
      serde_json::json!(true),
      serde_json::json!(null),
      serde_json::json!(["QQMusic.exe"]),
      serde_json::json!({ "name": "QQMusic.exe" }),
    ];

    let executor = WindowsProductionExecutor::default();
    let req = dummy_request(Some(ExecutionMode::Verified));

    for val in invalid_values {
      let mut op = dummy_operation("qqmusic.prepare", ExecutionMode::Verified);
      op.steps[0].action = serde_json::json!({
        "type": "smtc_query",
        "app_id": val
      });

      let res = executor.execute(&op, &req);
      match res {
        ExecutionResult::GateFailed {
          step_id,
          message,
          consecutive_failures,
        } => {
          assert_eq!(step_id, "step_smtc_query");
          assert_eq!(consecutive_failures, 1);
          assert!(
            message.contains("step 'step_smtc_query' has invalid 'app_id' field: expected string"),
            "val: {:?}, message: {}",
            val,
            message
          );
        }
        other => panic!("expected GateFailed for non-string app_id {:?}, got {:?}", val, other),
      }
    }
  }

  #[test]
  fn test_target_validation_accepts_valid_qqmusic_targets() {
    let mut op = dummy_operation("qqmusic.prepare", ExecutionMode::Verified);
    op.target.app_name = "qqmusic.exe".to_string(); // case-insensitive check
    op.target.backend = "windows.smtc+coreaudio+wgc".to_string();
    op.steps[0].action = serde_json::json!({
      "type": "smtc_query",
      "app_id": "QQMusic.exe"
    });

    let mut req = dummy_request(Some(ExecutionMode::Verified));
    req.app_name = "QQMUSIC.EXE".to_string();

    assert!(validate_supported_target(&op, &req).is_ok());
  }

  fn test_window(id: &str, app: Option<&str>, is_visible: bool, is_main: bool) -> Window {
    Window {
      reference: auv_driver_common::window::WindowRef { id: id.to_string() },
      title: Some("Test Window".to_string()),
      app_name: app.map(str::to_string),
      application_id: None,
      process_id: Some(1234),
      frame: auv_driver_common::geometry::Rect::new(0.0, 0.0, 800.0, 600.0),
      coordinate_space: auv_driver_common::geometry::CoordinateSpace::Screen,
      is_main,
      is_visible,
    }
  }

  #[test]
  fn test_select_target_window_resolution_and_ambiguity() {
    // 1. Positive exact / case-insensitive / stem matches
    let win_exact = test_window("1", Some("QQMusic.exe"), true, true);
    assert_eq!(select_target_window(&[win_exact], "QQMusic.exe").unwrap().reference.id, "1");

    let win_lower = test_window("2", Some("qqmusic.exe"), true, true);
    assert_eq!(select_target_window(&[win_lower], "QQMusic.exe").unwrap().reference.id, "2");

    let win_stem = test_window("3", Some("QQMusic"), true, true);
    assert_eq!(select_target_window(&[win_stem], "QQMusic.exe").unwrap().reference.id, "3");

    // 2. Strict negative matches: substring false positives are NOT authorized
    let win_not = test_window("4", Some("NotQQMusic.exe"), true, true);
    assert!(matches!(select_target_window(&[win_not], "QQMusic.exe").unwrap_err(), DriverError::NotFound { .. }));

    let win_helper = test_window("5", Some("QQMusic_Helper.exe"), true, true);
    assert!(matches!(select_target_window(&[win_helper], "QQMusic.exe").unwrap_err(), DriverError::NotFound { .. }));

    let win_other = test_window("6", Some("Spotify.exe"), true, true);
    assert!(matches!(select_target_window(&[win_other], "QQMusic.exe").unwrap_err(), DriverError::NotFound { .. }));

    // 3. Target missing (empty list)
    assert!(matches!(select_target_window(&[], "QQMusic.exe").unwrap_err(), DriverError::NotFound { .. }));

    // 4. Ambiguity: multiple windows for target rejected
    let win_a = test_window("a", Some("QQMusic.exe"), true, false);
    let win_b = test_window("b", Some("QQMusic.exe"), true, false);
    let ambig_err = select_target_window(&[win_a, win_b], "QQMusic.exe").unwrap_err();
    match ambig_err {
      DriverError::InvalidInput { message } => {
        assert!(message.contains("Ambiguous match"), "{message}");
        assert!(message.contains("count: 2"), "{message}");
      }
      other => panic!("expected InvalidInput for ambiguous windows, got {other:?}"),
    }
  }

  #[cfg(target_os = "windows")]
  #[test]
  #[ignore = "live Windows execution against running QQMusic: run explicitly with -- --ignored --nocapture"]
  fn live_windows_production_executor_qqmusic_verification() {
    let executor = WindowsProductionExecutor::default();

    // 1. Target validation gate rejects unsupported/mismatched targets before COM calls
    let mut bad_req_op = dummy_operation("qqmusic.prepare", ExecutionMode::Verified);
    bad_req_op.target.app_name = "QQMusic.exe".to_string();
    bad_req_op.target.backend = "windows.smtc+coreaudio+wgc".to_string();
    let mut bad_req = dummy_request(Some(ExecutionMode::Verified));
    bad_req.app_name = "Spotify.exe".to_string();
    let res = executor.execute(&bad_req_op, &bad_req);
    assert!(matches!(res, ExecutionResult::GateFailed { ref step_id, .. } if step_id == "target_validation"));

    // 2. Resolve live WindowsOperationContext
    let ctx = WindowsOperationContext::resolve(None).expect("live WindowsOperationContext::resolve should succeed with QQMusic running");
    assert!(ctx.window.app_name.as_deref().unwrap_or("").eq_ignore_ascii_case("QQMusic.exe"));
    assert!(ctx.audio.is_some(), "CoreAudio session should be resolved for QQMusic PID");
    println!(
      "Live discovery verified: Window HWND={:?}, PID={:?}, App={:?}, Title={:?}, AudioResolved={}",
      ctx.window.reference.id,
      ctx.window.process_id,
      ctx.window.app_name,
      ctx.window.title,
      ctx.audio.is_some(),
    );

    // 3. Load actual operations and execute live
    let op_json = include_str!("../../../supported/apps/auv-qqmusic/operations/qqmusic-prepared-playback.json");
    let op: OperationDef = serde_json::from_str(op_json).expect("parse verified operation");
    let mut req = dummy_request(Some(ExecutionMode::Verified));
    req.app_name = "QQMusic.exe".to_string();

    let res = executor.execute(&op, &req);
    println!("live verified execute result: {:?}", res);
    match res {
      ExecutionResult::Success {
        steps_executed,
        execution_mode,
        confirmed,
      } => {
        assert_eq!(execution_mode, ExecutionMode::Verified);
        assert_eq!(steps_executed, 4);
        assert!(confirmed, "verified execution with title change and WGC must be confirmed");
      }
      ExecutionResult::GateFailed {
        step_id, message, ..
      } => {
        panic!("live execution failed at gate '{step_id}': {message}");
      }
      other => panic!("unexpected execution result: {other:?}"),
    }

    // 4. Load actual Fast operation and execute live
    let fast_op_json = include_str!("../../../supported/apps/auv-qqmusic/operations/qqmusic-prepared-playback-fast.json");
    let fast_op: OperationDef = serde_json::from_str(fast_op_json).expect("parse fast operation");
    let mut fast_req = dummy_request(Some(ExecutionMode::Fast));
    fast_req.app_name = "QQMusic.exe".to_string();

    let fast_res = executor.execute(&fast_op, &fast_req);
    println!("live fast execute result: {:?}", fast_res);
    match fast_res {
      ExecutionResult::Success {
        steps_executed,
        execution_mode,
        confirmed,
      } => {
        assert_eq!(execution_mode, ExecutionMode::Fast);
        assert_eq!(steps_executed, 4);
        assert!(!confirmed, "fast mode must never report confirmed: true");
      }
      ExecutionResult::GateFailed {
        step_id, message, ..
      } => {
        panic!("live fast execution failed at gate '{step_id}': {message}");
      }
      other => panic!("unexpected execution result: {other:?}"),
    }
  }
}
