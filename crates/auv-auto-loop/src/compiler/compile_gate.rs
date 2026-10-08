//! The three automatic compilation gates (Core Guardrails).
//!
//! Sub-gates:
//! 1. Cleaning Gate: Zero ambiguous drops permitted.
//! 2. Parameter Gate: Only isomorphic anti-unification or typed whitelist allowed.
//!    Zero LLM semantic judgments permitted; Next() and atomic actions barred.
//! 3. Blast-Radius Gate: Non-whitelisted or destructive actions barred.
//!
//! Rule: ALL 3 sub-gates must pass. Any failure routes to the manual review queue.
//! NO degraded deployment permitted.

use crate::compiler::anti_unify::ParameterLiftingOutcome;
use crate::compiler::cleaner::CleaningOutcome;
use crate::models::{ExecutionMode, OperationStepDef, ReasonCode, VerificationGateDef};

/// Whitelist of safe, allowed domain action types.
const ACTION_WHITELIST: &[&str] = &[
  "smtc_query",
  "volume",
  "set_volume",
  "ensure_playing_and_volume",
  "next",
  "skip_next",
  "play",
  "pause",
  "query",
  "wgc_capture",
  "capture",
  "navigate",
  "inspect",
];

/// Explicit blacklist of destructive or high-risk actions.
const ACTION_BLACKLIST: &[&str] = &[
  "delete",
  "remove",
  "rm",
  "drop",
  "format",
  "send_message",
  "post",
  "system_config",
  "registry",
  "kill_process",
  "shutdown",
];

/// Result of evaluating compilation gates.
#[derive(Debug, Clone)]
pub struct CompilationGateEvaluation {
  pub passed: bool,
  pub reason_code: ReasonCode,
  pub message: String,
}

/// Evaluates Sub-gate 1: Cleaning Gate.
pub fn evaluate_cleaning_gate(outcome: &CleaningOutcome) -> CompilationGateEvaluation {
  if outcome.has_ambiguous_drops {
    CompilationGateEvaluation {
      passed: false,
      reason_code: ReasonCode::RejectAmbiguousDrops,
      message: {
        let reasons: Vec<String> =
          outcome.dropped_steps.iter().filter(|s| s.is_ambiguous).map(|s| format!("step {}: {}", s.step, s.reason)).collect();
        format!("Cleaning gate rejected trajectory: {} ambiguous drop(s) detected [{}]", reasons.len(), reasons.join("; "))
      },
    }
  } else {
    CompilationGateEvaluation {
      passed: true,
      reason_code: ReasonCode::CleaningZeroAmbiguity,
      message: format!("Cleaning gate approved: 0 ambiguous drops (dropped {} safe redundant steps)", outcome.dropped_steps.len()),
    }
  }
}

/// Evaluates Sub-gate 2: Parameter Gate.
pub fn evaluate_parameter_gate(lifting: &ParameterLiftingOutcome) -> CompilationGateEvaluation {
  if let Some(ref forbidden) = lifting.forbidden_parameter_detected {
    CompilationGateEvaluation {
      passed: false,
      reason_code: ReasonCode::RejectForbiddenParameterization,
      message: format!("Parameter gate rejected: {}", forbidden),
    }
  } else {
    CompilationGateEvaluation {
      passed: true,
      reason_code: ReasonCode::ParameterGateApproved,
      message: format!("Parameter gate approved: {} parameter(s) derived via isomorphic anti-unification", lifting.parameters.len()),
    }
  }
}

/// Evaluates Sub-gate 3: Blast-Radius Gate.
pub fn evaluate_blast_radius_gate(steps: &[OperationStepDef]) -> CompilationGateEvaluation {
  for step in steps {
    let action_val = &step.action;
    let action_str = action_val.to_string().to_lowercase();
    let name_str = step.name.to_lowercase();

    // 1. Check blacklist
    for blacklisted in ACTION_BLACKLIST {
      if action_str.contains(blacklisted) || name_str.contains(blacklisted) {
        return CompilationGateEvaluation {
          passed: false,
          reason_code: ReasonCode::RejectBlastRadiusViolation,
          message: format!("Blast-radius gate rejected step '{}': contains blacklisted destructive pattern '{}'", step.id, blacklisted),
        };
      }
    }

    // 2. Check whitelist match
    let matches_whitelist = ACTION_WHITELIST.iter().any(|allowed| action_str.contains(allowed) || name_str.contains(allowed));

    if !matches_whitelist {
      return CompilationGateEvaluation {
        passed: false,
        reason_code: ReasonCode::RejectBlastRadiusViolation,
        message: format!("Blast-radius gate rejected step '{}': action not in safe whitelist", step.id),
      };
    }
  }

  CompilationGateEvaluation {
    passed: true,
    reason_code: ReasonCode::BlastRadiusApproved,
    message: format!("Blast-radius gate approved: all {} steps match safe action whitelist", steps.len()),
  }
}

/// Evaluates execution mode compatibility against operation steps and tags.
///
/// Enforces:
/// - Fast mode requires pure action dispatch with zero effect verification gates.
/// - `TitleChangeGate(require_title_change=true)` requires Verified mode.
/// - `unverified-step` cannot be combined with Fast mode (only Verified is permitted).
/// - Destructive / high-risk actions constructively barred from Fast mode.
pub fn evaluate_execution_mode_gate(mode: ExecutionMode, steps: &[OperationStepDef], tags: &[String]) -> CompilationGateEvaluation {
  match mode {
    ExecutionMode::Verified => CompilationGateEvaluation {
      passed: true,
      reason_code: ReasonCode::CompilationApproved,
      message: "Verified mode approved: effect verification gates permitted".to_string(),
    },
    ExecutionMode::Fast => {
      // 1. unverified-step + Fast is strictly rejected
      if tags.iter().any(|t| t == "unverified-step") || steps.iter().any(|s| s.is_unverified) {
        return CompilationGateEvaluation {
          passed: false,
          reason_code: ReasonCode::RejectModeConflict,
          message: "unverified-step cannot be combined with Fast mode; only Verified mode is permitted".to_string(),
        };
      }

      // 2. High-risk actions barred from Fast mode
      for step in steps {
        let action_str = step.action.to_string().to_lowercase();
        let name_str = step.name.to_lowercase();
        for blacklisted in ACTION_BLACKLIST {
          if action_str.contains(blacklisted) || name_str.contains(blacklisted) {
            return CompilationGateEvaluation {
              passed: false,
              reason_code: ReasonCode::RejectModeConflict,
              message: format!(
                "High-risk action '{}' in step '{}' constructively barred from Fast mode; only Verified mode is permitted",
                blacklisted, step.id
              ),
            };
          }
        }
      }

      // 3. Effect verification gates require Verified mode
      for step in steps {
        if let VerificationGateDef::TitleChangeGate {
          require_title_change: true,
          ..
        } = &step.verification_gate
        {
          return CompilationGateEvaluation {
            passed: false,
            reason_code: ReasonCode::RejectModeConflict,
            message: format!("Step '{}' requires TitleChangeGate verification; cannot be executed in Fast mode", step.id),
          };
        }
      }

      CompilationGateEvaluation {
        passed: true,
        reason_code: ReasonCode::CompilationApproved,
        message: "Fast mode approved: action dispatch only with zero effect verification conflicts".to_string(),
      }
    }
  }
}
