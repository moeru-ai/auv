//! Fake test-only operation executor faithfully modeling Fast and Verified semantics.
//!
//! Enforces:
//! - Fail-closed: `requested_mode: None` is rejected with `REJECT_MODE_UNDECLARED` before any driver side effects.
//! - Mode conflict: `requested_mode != op.execution_mode` is rejected with `ModeConflictError` before any driver side effects (zero calls recorded).
//! - Fast mode: dispatch immediate return, `confirmed` is ALWAYS `false`.
//! - Verified mode: evaluates gates; configurable gate success vs timeout / escalation.

use crate::models::{ExecutionMode, OperationDef};
use crate::runtime::executor::{ExecutionResult, OperationExecutor};
use crate::scheduler::matcher::TaskRequest;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Test-only fake driver executor that records call counts and models gate behavior.
#[derive(Debug, Default)]
pub struct FakeOperationExecutor {
  pub driver_calls_count: AtomicUsize,
  pub verified_should_pass_gate: AtomicBool,
}

impl FakeOperationExecutor {
  pub fn new() -> Self {
    Self {
      driver_calls_count: AtomicUsize::new(0),
      verified_should_pass_gate: AtomicBool::new(true),
    }
  }

  /// Returns total number of actual driver invocations (must be 0 on mode conflict rejection).
  pub fn calls_count(&self) -> usize {
    self.driver_calls_count.load(Ordering::SeqCst)
  }

  /// Sets whether Verified mode verification gates should pass or fail.
  pub fn set_verified_should_pass(&self, pass: bool) {
    self.verified_should_pass_gate.store(pass, Ordering::SeqCst);
  }

  /// Resets driver calls counter to 0.
  pub fn reset_calls(&self) {
    self.driver_calls_count.store(0, Ordering::SeqCst);
  }
}

impl OperationExecutor for FakeOperationExecutor {
  fn execute(&self, op: &OperationDef, request: &TaskRequest) -> ExecutionResult {
    // 1. Fail-closed: production entrance requires explicit execution_mode
    let Some(req_mode) = request.requested_mode else {
      return ExecutionResult::ModeConflictError {
        operation_name: op.name.clone(),
        message: "REJECT_MODE_UNDECLARED: execution requires explicit execution_mode (fail-closed)".to_string(),
      };
    };

    // 2. Second gate: mode conflict check at driver boundary BEFORE any driver invocation
    if req_mode != op.execution_mode {
      return ExecutionResult::ModeConflictError {
        operation_name: op.name.clone(),
        message: format!(
          "Driver boundary mode conflict: requested {:?}, but operation '{}' declares {:?} (fail-closed, zero side effects)",
          req_mode, op.name, op.execution_mode
        ),
      };
    }

    // Driver side-effect occurs ONLY after mode checks pass
    self.driver_calls_count.fetch_add(1, Ordering::SeqCst);

    match op.execution_mode {
      ExecutionMode::Fast => {
        // Fast mode: immediate return, confirmed is ALWAYS false
        ExecutionResult::success(op.steps.len(), ExecutionMode::Fast, false)
      }
      ExecutionMode::Verified => {
        if self.verified_should_pass_gate.load(Ordering::SeqCst) {
          ExecutionResult::success(op.steps.len(), ExecutionMode::Verified, true)
        } else {
          ExecutionResult::GateFailed {
            step_id: op.steps.first().map(|s| s.id.clone()).unwrap_or_else(|| "step_1".to_string()),
            message: "Verification gate timed out waiting for effect confirmation".to_string(),
            consecutive_failures: 1,
          }
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::models::{CompilationMetadata, OperationStepDef, TargetMetadata, VerificationGateDef};
  use std::collections::HashMap;

  fn dummy_step() -> OperationStepDef {
    OperationStepDef {
      id: "step_1".to_string(),
      name: "Dummy Step".to_string(),
      description: "Dummy".to_string(),
      action: serde_json::json!({ "type": "play" }),
      verification_gate: VerificationGateDef::CustomAssertion {
        expression: "true".to_string(),
        timeout_ms: 100,
        escalate_on_mismatch: "none".to_string(),
      },
      is_unverified: false,
    }
  }

  fn dummy_op(mode: ExecutionMode) -> OperationDef {
    OperationDef {
      schema_version: crate::models::OPERATION_SCHEMA_VERSION.to_string(),
      name: format!("test.op_{}", mode.as_str()),
      description: "Test operation".to_string(),
      execution_mode: mode,
      compilation_metadata: CompilationMetadata {
        compiler: "test".to_string(),
        source_record: "test".to_string(),
        date: "2026-10-08".to_string(),
        crux_goal: "test".to_string(),
      },
      target: TargetMetadata {
        app_name: "test.exe".to_string(),
        backend: "test".to_string(),
      },
      preconditions: vec![],
      parameters: vec![],
      steps: vec![dummy_step()],
      tags: vec![],
    }
  }

  #[test]
  fn test_fake_executor_fast_mode_never_confirmed() {
    let fake = FakeOperationExecutor::new();
    let op = dummy_op(ExecutionMode::Fast);
    let req = TaskRequest {
      app_name: "test".to_string(),
      task_name: "op_fast".to_string(),
      instruction: "run fast".to_string(),
      requested_mode: Some(ExecutionMode::Fast),
      current_context: HashMap::new(),
      embedding_vector: None,
    };

    let res = fake.execute(&op, &req);
    assert_eq!(fake.calls_count(), 1);
    assert!(matches!(
      res,
      ExecutionResult::Success {
        execution_mode: ExecutionMode::Fast,
        confirmed: false,
        ..
      }
    ));
    assert!(!res.confirmed(), "Fast mode must NEVER report confirmed: true");
  }

  #[test]
  fn test_fake_executor_verified_mode_confirmed_or_gate_failed() {
    let fake = FakeOperationExecutor::new();
    let op = dummy_op(ExecutionMode::Verified);
    let req = TaskRequest {
      app_name: "test".to_string(),
      task_name: "op_verified".to_string(),
      instruction: "run verified".to_string(),
      requested_mode: Some(ExecutionMode::Verified),
      current_context: HashMap::new(),
      embedding_vector: None,
    };

    // Passing gate
    let res = fake.execute(&op, &req);
    assert_eq!(fake.calls_count(), 1);
    assert!(matches!(
      res,
      ExecutionResult::Success {
        execution_mode: ExecutionMode::Verified,
        confirmed: true,
        ..
      }
    ));
    assert!(res.confirmed());

    // Failing gate
    fake.set_verified_should_pass(false);
    let res_fail = fake.execute(&op, &req);
    assert_eq!(fake.calls_count(), 2);
    assert!(matches!(res_fail, ExecutionResult::GateFailed { .. }));
  }

  #[test]
  fn test_fake_executor_mode_conflict_rejects_with_zero_driver_calls() {
    let fake = FakeOperationExecutor::new();
    let op = dummy_op(ExecutionMode::Verified);
    let req = TaskRequest {
      app_name: "test".to_string(),
      task_name: "op_verified".to_string(),
      instruction: "run fast".to_string(),
      requested_mode: Some(ExecutionMode::Fast),
      current_context: HashMap::new(),
      embedding_vector: None,
    };

    let res = fake.execute(&op, &req);
    assert_eq!(fake.calls_count(), 0, "Driver calls must be EXACTLY 0 on mode conflict rejection");
    assert!(matches!(res, ExecutionResult::ModeConflictError { .. }));
  }

  #[test]
  fn test_fake_executor_missing_requested_mode_rejected_with_zero_calls() {
    let fake = FakeOperationExecutor::new();
    let op = dummy_op(ExecutionMode::Fast);
    let req = TaskRequest {
      app_name: "test".to_string(),
      task_name: "op_fast".to_string(),
      instruction: "run undeclared".to_string(),
      requested_mode: None,
      current_context: HashMap::new(),
      embedding_vector: None,
    };

    let res = fake.execute(&op, &req);
    assert_eq!(fake.calls_count(), 0, "Driver calls must be 0 when mode is undeclared");
    match res {
      ExecutionResult::ModeConflictError { message, .. } => {
        assert!(message.contains("REJECT_MODE_UNDECLARED"));
      }
      other => panic!("Expected ModeConflictError, got {:?}", other),
    }
  }
}
