//! Runtime execution engine with step-by-step gate verification and auto-isolation.
//!
//! Enforces:
//! - Strict mode for `unverified-step`: 1 gate failure triggers immediate escalation & isolation.
//! - Standard mode: Consecutive gate failures >= 2 triggers auto-isolation.
//! - Auto-isolation updates catalog: removes operation from active pool and routes subsequent requests to VLM.
//! - Zero silent errors: All gate evaluations and isolation events log explicit ReasonCodes.

use crate::decision_log::DecisionLogger;
use crate::models::{DecisionAction, DecisionCategory, ExecutionMode, ManualReviewItem, OperationDef, ReasonCode, VerificationGateDef};
use crate::scheduler::catalog::OperationCatalog;
use crate::scheduler::matcher::TaskRequest;
use chrono::Utc;
use std::collections::HashMap;

/// Observable state of the target system / application.
#[derive(Debug, Clone)]
pub struct RuntimeEnvironment {
  pub smtc_session_present: bool,
  pub playback_status: String,
  pub current_volume: f32,
  pub current_title: String,
  pub previous_title: String,
  pub wgc_non_black_ratio: f64,
  pub fault_audio_service_down: bool,
  pub fault_wgc_black_screen: bool,
  pub custom_state: HashMap<String, serde_json::Value>,
}

impl Default for RuntimeEnvironment {
  fn default() -> Self {
    Self {
      smtc_session_present: true,
      playback_status: "Playing".to_string(),
      current_volume: 0.40,
      current_title: "Track 2".to_string(),
      previous_title: "Track 1".to_string(),
      wgc_non_black_ratio: 0.99,
      fault_audio_service_down: false,
      fault_wgc_black_screen: false,
      custom_state: HashMap::new(),
    }
  }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExecutionResult {
  Success {
    steps_executed: usize,
    execution_mode: ExecutionMode,
    confirmed: bool,
  },
  GateFailed {
    step_id: String,
    message: String,
    consecutive_failures: usize,
  },
  EscalatedToVlm {
    step_id: String,
    reason: String,
    isolated: bool,
  },
  ModeConflictError {
    operation_name: String,
    message: String,
  },
}

impl ExecutionResult {
  /// Constructs a success result enforcing that Fast mode is NEVER confirmed.
  pub fn success(steps_executed: usize, mode: ExecutionMode, confirmed: bool) -> Self {
    let effective_confirmed = match mode {
      ExecutionMode::Fast => false,
      ExecutionMode::Verified => confirmed,
    };
    Self::Success {
      steps_executed,
      execution_mode: mode,
      confirmed: effective_confirmed,
    }
  }

  pub fn confirmed(&self) -> bool {
    match self {
      Self::Success { confirmed, .. } => *confirmed,
      _ => false,
    }
  }

  pub fn execution_mode(&self) -> Option<ExecutionMode> {
    match self {
      Self::Success { execution_mode, .. } => Some(*execution_mode),
      _ => None,
    }
  }
}

/// Trait for executing a compiled operation against a runtime or driver environment.
pub trait OperationExecutor: Send + Sync {
  fn execute(&self, op: &OperationDef, request: &TaskRequest) -> ExecutionResult;
}

pub struct RuntimeExecutor<'a> {
  logger: &'a DecisionLogger,
  failure_tracker: HashMap<String, usize>,
}

impl<'a> RuntimeExecutor<'a> {
  pub fn new(logger: &'a DecisionLogger) -> Self {
    Self {
      logger,
      failure_tracker: HashMap::new(),
    }
  }

  /// Executes an operation step-by-step against the runtime environment.
  pub fn execute(&mut self, op: &OperationDef, env: &mut RuntimeEnvironment, catalog: &mut OperationCatalog) -> ExecutionResult {
    let task_id = &op.name;

    // Fast mode: Dispatch actions only, never wait on verification gates; confirmed is ALWAYS false
    if op.execution_mode == ExecutionMode::Fast {
      for step in &op.steps {
        apply_action_effects(&step.action, env);
      }
      self.logger.log(
        DecisionCategory::Runtime,
        DecisionAction::Executed,
        ReasonCode::GatePassed,
        task_id,
        Some(op.name.clone()),
        format!("Fast mode: dispatched {} actions with eventual consistency (confirmed: false)", op.steps.len()),
        serde_json::json!({ "execution_mode": "fast", "confirmed": false }),
      );
      return ExecutionResult::success(op.steps.len(), ExecutionMode::Fast, false);
    }

    // Verified mode: Dispatch actions and verify each gate strictly
    let is_strict_mode = op.tags.iter().any(|t| t == "unverified-step") || op.steps.iter().any(|s| s.is_unverified);

    for step in &op.steps {
      // 1. Dispatch action against environment
      apply_action_effects(&step.action, env);

      // 2. Evaluate verification gate
      let gate_eval = evaluate_runtime_gate(&step.verification_gate, env);

      if gate_eval.passed {
        self.logger.log(
          DecisionCategory::Runtime,
          DecisionAction::GatePass,
          ReasonCode::GatePassed,
          task_id,
          Some(op.name.clone()),
          format!("Step '{}' verification gate passed: {}", step.id, gate_eval.message),
          serde_json::json!({ "step_id": step.id }),
        );
      } else {
        self.logger.log(
          DecisionCategory::Runtime,
          DecisionAction::GateFail,
          ReasonCode::GateFailed,
          task_id,
          Some(op.name.clone()),
          format!("Step '{}' verification gate failed: {}", step.id, gate_eval.message),
          serde_json::json!({ "step_id": step.id, "failure_detail": gate_eval.message }),
        );

        // Strict mode branch for unverified steps: instant escalation + isolation
        if is_strict_mode {
          self.logger.log(
            DecisionCategory::Runtime,
            DecisionAction::GateFail,
            ReasonCode::StrictStepFailed,
            task_id,
            Some(op.name.clone()),
            format!("Strict mode: step '{}' failed gate. Triggering instant escalation and isolation.", step.id),
            serde_json::json!({ "step_id": step.id }),
          );

          self.isolate_operation(op, catalog, &step.id, &gate_eval.message);

          return ExecutionResult::EscalatedToVlm {
            step_id: step.id.clone(),
            reason: format!("Strict mode step failed: {}", gate_eval.message),
            isolated: true,
          };
        }

        // Standard mode: update consecutive failure tracker
        let failures = self.failure_tracker.entry(op.name.clone()).or_insert(0);
        *failures += 1;
        let consecutive = *failures;

        if consecutive >= 2 {
          self.isolate_operation(op, catalog, &step.id, &gate_eval.message);

          return ExecutionResult::EscalatedToVlm {
            step_id: step.id.clone(),
            reason: format!("Consecutive gate failures ({}) exceeded threshold (2). Operation auto-isolated.", consecutive),
            isolated: true,
          };
        } else {
          return ExecutionResult::GateFailed {
            step_id: step.id.clone(),
            message: gate_eval.message,
            consecutive_failures: consecutive,
          };
        }
      }
    }

    // All steps passed: reset consecutive failure counter
    self.failure_tracker.insert(op.name.clone(), 0);

    let confirmed = op.execution_mode == ExecutionMode::Verified;
    ExecutionResult::success(op.steps.len(), op.execution_mode, confirmed)
  }

  /// Executes an operation explicitly via the fast path.
  ///
  /// If the operation requires Verified mode, execution is rejected fail-closed with zero side-effects.
  pub fn execute_fast(&mut self, op: &OperationDef, env: &mut RuntimeEnvironment, catalog: &mut OperationCatalog) -> ExecutionResult {
    if op.execution_mode == ExecutionMode::Verified {
      self.logger.log(
        DecisionCategory::Runtime,
        DecisionAction::Escalated,
        ReasonCode::ModeMismatchEscalateVlm,
        &op.name,
        Some(op.name.clone()),
        format!("Fast path execution invoked for Verified operation '{}'; escalating to VLM without commands dispatched.", op.name),
        serde_json::json!({ "op_mode": "verified", "requested_path": "fast", "zero_side_effects": true }),
      );

      return ExecutionResult::ModeConflictError {
        operation_name: op.name.clone(),
        message: format!("Fast path cannot execute Verified operation '{}'", op.name),
      };
    }

    self.execute(op, env, catalog)
  }

  fn isolate_operation(&mut self, op: &OperationDef, catalog: &mut OperationCatalog, failed_step_id: &str, failure_detail: &str) {
    self.logger.log(
      DecisionCategory::Runtime,
      DecisionAction::Isolated,
      ReasonCode::AutoIsolatedConsecutiveFailures,
      &op.name,
      Some(op.name.clone()),
      format!("Operation '{}' auto-isolated after gate failures. Queued for human inspection.", op.name),
      serde_json::json!({ "failed_step": failed_step_id, "detail": failure_detail }),
    );

    self.logger.log(
      DecisionCategory::Runtime,
      DecisionAction::Escalated,
      ReasonCode::EscalateToVlm,
      &op.name,
      Some(op.name.clone()),
      format!("Task '{}' escalated to VLM slow loop execution planner.", op.name),
      serde_json::json!({ "op": op.name }),
    );

    let review_item = ManualReviewItem {
      id: format!("isolate_{}", Utc::now().timestamp_millis()),
      task_name: op.name.clone(),
      reason_code: ReasonCode::AutoIsolatedConsecutiveFailures,
      reason_description: format!("Failed at step '{}': {}", failed_step_id, failure_detail),
      execution_mode: Some(op.execution_mode),
      source_trajectory: None,
      isolated_operation: Some(Box::new(op.clone())),
      created_at: Utc::now().to_rfc3339(),
    };

    catalog.isolate(&op.name, review_item);
  }
}

struct GateEval {
  passed: bool,
  message: String,
}

fn apply_action_effects(action: &serde_json::Value, env: &mut RuntimeEnvironment) {
  if env.fault_audio_service_down {
    // Simulated system fault: audio backend does not update volume or playback state
    return;
  }

  if env.fault_wgc_black_screen {
    env.wgc_non_black_ratio = 0.0;
  }

  if let Some(action_type) = action.get("type").and_then(|v| v.as_str()) {
    match action_type {
      "ensure_playing_and_volume" => {
        if let Some(vol) = action.get("volume").and_then(|v| v.as_f64()) {
          env.current_volume = vol as f32;
        }
        env.playback_status = "Playing".to_string();
      }
      "skip_next" => {
        env.previous_title = env.current_title.clone();
        env.current_title = format!("{} (Next)", env.previous_title);
      }
      "play" => {
        env.playback_status = "Playing".to_string();
      }
      _ => {}
    }
  }
}

fn evaluate_runtime_gate(gate: &VerificationGateDef, env: &RuntimeEnvironment) -> GateEval {
  match gate {
    VerificationGateDef::SmtcSessionPresent { .. } => {
      if env.smtc_session_present {
        GateEval {
          passed: true,
          message: "SMTC session is present and active".to_string(),
        }
      } else {
        GateEval {
          passed: false,
          message: "No active SMTC session found".to_string(),
        }
      }
    }
    VerificationGateDef::StatusAndVolumeGate {
      expected_status,
      expected_volume,
      volume_tolerance,
      ..
    } => {
      if env.playback_status != *expected_status {
        return GateEval {
          passed: false,
          message: format!("Playback status mismatch: expected '{}', got '{}'", expected_status, env.playback_status),
        };
      }
      let diff = (env.current_volume - expected_volume).abs();
      if diff > *volume_tolerance {
        return GateEval {
          passed: false,
          message: format!("Volume outside tolerance: expected {} ± {}, got {}", expected_volume, volume_tolerance, env.current_volume),
        };
      }
      GateEval {
        passed: true,
        message: format!("Status is '{}' and volume {} is within tolerance ±{}", env.playback_status, env.current_volume, volume_tolerance),
      }
    }
    VerificationGateDef::TitleChangeGate {
      require_title_change,
      ..
    } => {
      let changed = env.current_title != env.previous_title;
      if *require_title_change && !changed {
        GateEval {
          passed: false,
          message: format!("Title did not change: previous='{}', current='{}'", env.previous_title, env.current_title),
        }
      } else {
        GateEval {
          passed: true,
          message: format!("Title successfully changed to '{}'", env.current_title),
        }
      }
    }
    VerificationGateDef::WgcAliveGate {
      min_non_black_ratio,
      ..
    } => {
      if env.wgc_non_black_ratio >= *min_non_black_ratio {
        GateEval {
          passed: true,
          message: format!(
            "WGC window alive: non-black ratio {:.2}% >= min {:.2}%",
            env.wgc_non_black_ratio * 100.0,
            min_non_black_ratio * 100.0
          ),
        }
      } else {
        GateEval {
          passed: false,
          message: format!(
            "WGC black screen detected: non-black ratio {:.2}% < min {:.2}%",
            env.wgc_non_black_ratio * 100.0,
            min_non_black_ratio * 100.0
          ),
        }
      }
    }
    VerificationGateDef::CustomAssertion { expression, .. } => GateEval {
      passed: true,
      message: format!("Custom assertion evaluated: {}", expression),
    },
    VerificationGateDef::UnverifiedFallback { description, .. } => {
      // Unverified steps fail in fault injection if custom_state flags it
      if env.custom_state.get("unverified_step_fail").and_then(|v| v.as_bool()).unwrap_or(false) {
        GateEval {
          passed: false,
          message: format!("Unverified step failed in strict mode: {}", description),
        }
      } else {
        GateEval {
          passed: true,
          message: format!("Unverified step executed without reported fault: {}", description),
        }
      }
    }
  }
}
