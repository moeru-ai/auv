//! End-to-end integration tests verifying mode-aware scheduling and execution.
//!
//! Validates:
//! 1. Fast mode E2E: Exact key match -> FakeOperationExecutor dispatch -> confirmed: false (CRITICAL INVARIANT).
//! 2. Verified mode E2E: Exact key match -> FakeOperationExecutor dispatch -> confirmed: true.
//! 3. Verified mode gate failure & escalation: Configurable gate failure -> GateFailed.
//! 4. Mode conflict (Verified op + Fast request): Intercepted at scheduler (ModeMismatchEscalateVlm)
//!    AND intercepted at fake executor boundary (ModeConflictError) with zero driver calls.
//! 5. Mode conflict (Fast op + Verified request): Bidirectional interception with zero driver calls.
//! 6. Undeclared mode (requested_mode: None): Rejected fail-closed with REJECT_MODE_UNDECLARED (zero calls).
//! 7. Fast confirmation invariant: ExecutionResult::success always clamps confirmed: false for Fast mode.

use auv_auto_loop::decision_log::DecisionLogger;
use auv_auto_loop::models::{
  CompilationMetadata, ExecutionMode, OPERATION_SCHEMA_VERSION, OperationDef, OperationStepDef, ReasonCode, TargetMetadata,
  VerificationGateDef,
};
use auv_auto_loop::runtime::{ExecutionResult, FakeOperationExecutor, OperationExecutor};
use auv_auto_loop::scheduler::{FastLoopScheduler, OperationCatalog, TaskRequest};
use std::collections::HashMap;

fn build_fast_op() -> OperationDef {
  OperationDef {
    schema_version: OPERATION_SCHEMA_VERSION.to_string(),
    name: "qqmusic.prepare_playback_fast".to_string(),
    description: "Fast playback preparation without verification waiting".to_string(),
    execution_mode: ExecutionMode::Fast,
    compilation_metadata: CompilationMetadata {
      compiler: "auv-compiler".to_string(),
      source_record: "2026-10-04-qqmusic-vlm-record.json".to_string(),
      date: "2026-10-08".to_string(),
      crux_goal: "prepare_playback_fast".to_string(),
    },
    target: TargetMetadata {
      app_name: "QQMusic.exe".to_string(),
      backend: "windows.smtc".to_string(),
    },
    preconditions: vec![],
    parameters: vec![],
    steps: vec![OperationStepDef {
      id: "step_play_dispatch".to_string(),
      name: "Play dispatch".to_string(),
      description: "Trigger play dispatch action".to_string(),
      action: serde_json::json!({ "type": "play", "app_id": "QQMusic.exe" }),
      verification_gate: VerificationGateDef::CustomAssertion {
        expression: "true".to_string(),
        timeout_ms: 50,
        escalate_on_mismatch: "none".to_string(),
      },
      is_unverified: false,
    }],
    tags: vec![],
  }
}

fn build_verified_op() -> OperationDef {
  OperationDef {
    schema_version: OPERATION_SCHEMA_VERSION.to_string(),
    name: "qqmusic.prepare_playback".to_string(),
    description: "Verified playback preparation with SMTC gate".to_string(),
    execution_mode: ExecutionMode::Verified,
    compilation_metadata: CompilationMetadata {
      compiler: "auv-compiler".to_string(),
      source_record: "2026-10-04-qqmusic-vlm-record.json".to_string(),
      date: "2026-10-08".to_string(),
      crux_goal: "prepare_playback".to_string(),
    },
    target: TargetMetadata {
      app_name: "QQMusic.exe".to_string(),
      backend: "windows.smtc+coreaudio+wgc".to_string(),
    },
    preconditions: vec![],
    parameters: vec![],
    steps: vec![OperationStepDef {
      id: "step_smtc_session".to_string(),
      name: "Query SMTC session".to_string(),
      description: "Verify SMTC playback session is present".to_string(),
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

#[test]
fn test_1_fast_mode_e2e() {
  let logger = DecisionLogger::new();
  let scheduler = FastLoopScheduler::new(&logger);
  let fake = FakeOperationExecutor::new();
  let mut catalog = OperationCatalog::new();

  let fast_op = build_fast_op();
  catalog.register_active(fast_op).expect("Fast op must register successfully");

  let req = TaskRequest {
    app_name: "QQMusic.exe".to_string(),
    task_name: "prepare_playback_fast".to_string(),
    instruction: "prepare playback fast".to_string(),
    requested_mode: Some(ExecutionMode::Fast),
    current_context: HashMap::new(),
    embedding_vector: None,
  };

  // 1. Scheduler matching: exact key match without embedding call
  let outcome = scheduler.schedule(&req, &catalog);
  assert!(outcome.selected_operation.is_some(), "Scheduler must select exact matching Fast op");
  assert!(!outcome.embedding_called, "Exact key match must NOT invoke embedding fallback");
  assert_eq!(outcome.reason_code, ReasonCode::ExactKeyMatch);

  let selected_op = outcome.selected_operation.unwrap();
  assert_eq!(selected_op.execution_mode, ExecutionMode::Fast);

  // 2. Execution via FakeOperationExecutor
  let exec_result = fake.execute(&selected_op, &req);

  // Assertions:
  // - ExecutionResult::Success
  // - execution_mode == ExecutionMode::Fast
  // - confirmed == false (CRITICAL INVARIANT: Fast mode NEVER confirmed: true)
  // - fake.calls_count() == 1
  match &exec_result {
    ExecutionResult::Success {
      steps_executed,
      execution_mode,
      confirmed,
    } => {
      assert_eq!(*steps_executed, 1);
      assert_eq!(*execution_mode, ExecutionMode::Fast);
      assert!(!*confirmed, "CRITICAL INVARIANT: Fast mode MUST NEVER report confirmed: true");
    }
    other => panic!("Expected ExecutionResult::Success, got {:?}", other),
  }

  assert_eq!(exec_result.execution_mode(), Some(ExecutionMode::Fast));
  assert!(!exec_result.confirmed(), "exec_result.confirmed() must be false for Fast mode");
  assert_eq!(fake.calls_count(), 1, "Fake driver must record exactly 1 invocation");
}

#[test]
fn test_2_verified_mode_e2e() {
  let logger = DecisionLogger::new();
  let scheduler = FastLoopScheduler::new(&logger);
  let fake = FakeOperationExecutor::new();
  let mut catalog = OperationCatalog::new();

  let verified_op = build_verified_op();
  catalog.register_active(verified_op).expect("Verified op must register successfully");

  let req = TaskRequest {
    app_name: "QQMusic.exe".to_string(),
    task_name: "prepare_playback".to_string(),
    instruction: "prepare playback with verification".to_string(),
    requested_mode: Some(ExecutionMode::Verified),
    current_context: HashMap::new(),
    embedding_vector: None,
  };

  // 1. Scheduler matching: exact key match
  let outcome = scheduler.schedule(&req, &catalog);
  assert!(outcome.selected_operation.is_some(), "Scheduler must select exact matching Verified op");
  assert!(!outcome.embedding_called, "Exact key match must NOT invoke embedding fallback");
  assert_eq!(outcome.reason_code, ReasonCode::ExactKeyMatch);

  let selected_op = outcome.selected_operation.unwrap();
  assert_eq!(selected_op.execution_mode, ExecutionMode::Verified);

  // 2. Execution via FakeOperationExecutor
  let exec_result = fake.execute(&selected_op, &req);

  // Assertions:
  // - ExecutionResult::Success
  // - execution_mode == ExecutionMode::Verified
  // - confirmed == true
  // - fake.calls_count() == 1
  match &exec_result {
    ExecutionResult::Success {
      steps_executed,
      execution_mode,
      confirmed,
    } => {
      assert_eq!(*steps_executed, 1);
      assert_eq!(*execution_mode, ExecutionMode::Verified);
      assert!(*confirmed, "Verified mode with passing gate MUST report confirmed: true");
    }
    other => panic!("Expected ExecutionResult::Success, got {:?}", other),
  }

  assert_eq!(exec_result.execution_mode(), Some(ExecutionMode::Verified));
  assert!(exec_result.confirmed(), "exec_result.confirmed() must be true for Verified mode");
  assert_eq!(fake.calls_count(), 1, "Fake driver must record exactly 1 invocation");
}

#[test]
fn test_3_verified_mode_gate_failure_and_escalation() {
  let fake = FakeOperationExecutor::new();
  let verified_op = build_verified_op();

  // Configure gate to fail
  fake.set_verified_should_pass(false);

  let req = TaskRequest {
    app_name: "QQMusic.exe".to_string(),
    task_name: "prepare_playback".to_string(),
    instruction: "prepare playback with verification".to_string(),
    requested_mode: Some(ExecutionMode::Verified),
    current_context: HashMap::new(),
    embedding_vector: None,
  };

  let exec_result = fake.execute(&verified_op, &req);

  // Assert: returns GateFailed
  match &exec_result {
    ExecutionResult::GateFailed {
      step_id,
      message,
      consecutive_failures,
    } => {
      assert_eq!(step_id, "step_smtc_session");
      assert!(message.contains("timed out"));
      assert_eq!(*consecutive_failures, 1);
    }
    other => panic!("Expected ExecutionResult::GateFailed, got {:?}", other),
  }

  assert!(!exec_result.confirmed(), "Gate failure must not be confirmed");
  assert_eq!(fake.calls_count(), 1, "Driver invocation occurred prior to gate evaluation");
}

#[test]
fn test_4_mode_conflict_verified_op_fast_request() {
  let logger = DecisionLogger::new();
  let scheduler = FastLoopScheduler::new(&logger);
  let fake = FakeOperationExecutor::new();
  let mut catalog = OperationCatalog::new();

  let verified_op = build_verified_op();
  catalog.register_active(verified_op.clone()).expect("Verified op must register");

  let fast_req = TaskRequest {
    app_name: "QQMusic.exe".to_string(),
    task_name: "prepare_playback".to_string(),
    instruction: "prepare playback fast".to_string(),
    requested_mode: Some(ExecutionMode::Fast),
    current_context: HashMap::new(),
    embedding_vector: None,
  };

  // 1. Gate 1: Scheduler interception
  let outcome = scheduler.schedule(&fast_req, &catalog);
  assert!(outcome.selected_operation.is_none(), "Scheduler MUST NOT select Verified op when Fast mode is requested");
  assert_eq!(outcome.reason_code, ReasonCode::ModeMismatchEscalateVlm);
  assert_eq!(fake.calls_count(), 0, "Zero driver calls via scheduler interception");

  // 2. Gate 2: Driver boundary defense in FakeOperationExecutor
  let direct_result = fake.execute(&verified_op, &fast_req);
  match &direct_result {
    ExecutionResult::ModeConflictError {
      operation_name,
      message,
    } => {
      assert_eq!(operation_name, &verified_op.name);
      assert!(message.contains("Driver boundary mode conflict"));
      assert!(message.contains("fail-closed, zero side effects"));
    }
    other => panic!("Expected ModeConflictError at driver boundary, got {:?}", other),
  }

  assert_eq!(fake.calls_count(), 0, "CRITICAL DEFENSE: Fake executor driver calls count MUST remain exactly 0 on mode conflict rejection");
}

#[test]
fn test_5_mode_conflict_fast_op_verified_request() {
  let logger = DecisionLogger::new();
  let scheduler = FastLoopScheduler::new(&logger);
  let fake = FakeOperationExecutor::new();
  let mut catalog = OperationCatalog::new();

  let fast_op = build_fast_op();
  catalog.register_active(fast_op.clone()).expect("Fast op must register");

  let verified_req = TaskRequest {
    app_name: "QQMusic.exe".to_string(),
    task_name: "prepare_playback_fast".to_string(),
    instruction: "prepare playback verified".to_string(),
    requested_mode: Some(ExecutionMode::Verified),
    current_context: HashMap::new(),
    embedding_vector: None,
  };

  // 1. Gate 1: Scheduler interception
  let outcome = scheduler.schedule(&verified_req, &catalog);
  assert!(outcome.selected_operation.is_none(), "Scheduler MUST NOT select Fast op when Verified mode is requested");
  assert_eq!(outcome.reason_code, ReasonCode::ModeMismatchEscalateVlm);
  assert_eq!(fake.calls_count(), 0, "Zero driver calls via scheduler interception");

  // 2. Gate 2: Driver boundary defense in FakeOperationExecutor
  let direct_result = fake.execute(&fast_op, &verified_req);
  match &direct_result {
    ExecutionResult::ModeConflictError {
      operation_name,
      message,
    } => {
      assert_eq!(operation_name, &fast_op.name);
      assert!(message.contains("Driver boundary mode conflict"));
    }
    other => panic!("Expected ModeConflictError at driver boundary, got {:?}", other),
  }

  assert_eq!(fake.calls_count(), 0, "CRITICAL DEFENSE: Driver calls count MUST remain 0 on bidirectional mode conflict");
}

#[test]
fn test_6_undeclared_mode_rejected_fail_closed() {
  let fake = FakeOperationExecutor::new();
  let fast_op = build_fast_op();
  let verified_op = build_verified_op();

  let undeclared_req = TaskRequest {
    app_name: "QQMusic.exe".to_string(),
    task_name: "prepare_playback_fast".to_string(),
    instruction: "play without declared mode".to_string(),
    requested_mode: None,
    current_context: HashMap::new(),
    embedding_vector: None,
  };

  // Calling fake.execute with requested_mode: None directly returns ModeConflictError with REJECT_MODE_UNDECLARED
  let res_fast = fake.execute(&fast_op, &undeclared_req);
  match &res_fast {
    ExecutionResult::ModeConflictError {
      operation_name,
      message,
    } => {
      assert_eq!(operation_name, &fast_op.name);
      assert!(message.contains("REJECT_MODE_UNDECLARED"), "Undeclared mode error message must contain REJECT_MODE_UNDECLARED");
    }
    other => panic!("Expected ModeConflictError, got {:?}", other),
  }
  assert_eq!(fake.calls_count(), 0, "Driver calls must be 0 for undeclared mode on Fast op");

  let res_verified = fake.execute(&verified_op, &undeclared_req);
  match &res_verified {
    ExecutionResult::ModeConflictError {
      operation_name,
      message,
    } => {
      assert_eq!(operation_name, &verified_op.name);
      assert!(message.contains("REJECT_MODE_UNDECLARED"));
    }
    other => panic!("Expected ModeConflictError, got {:?}", other),
  }
  assert_eq!(fake.calls_count(), 0, "Driver calls must be 0 for undeclared mode on Verified op");
}

#[test]
fn test_7_fast_confirmation_invariant_across_all_pathways() {
  // Test ExecutionResult::success with any boolean always clamps confirmed: false for Fast mode
  let fast_with_true = ExecutionResult::success(5, ExecutionMode::Fast, true);
  assert!(!fast_with_true.confirmed(), "ExecutionResult::success with confirmed: true MUST be clamped to false for Fast mode");
  assert_eq!(fast_with_true.execution_mode(), Some(ExecutionMode::Fast));
  match fast_with_true {
    ExecutionResult::Success {
      confirmed,
      execution_mode,
      steps_executed,
    } => {
      assert!(!confirmed);
      assert_eq!(execution_mode, ExecutionMode::Fast);
      assert_eq!(steps_executed, 5);
    }
    _ => panic!("Expected Success variant"),
  }

  let fast_with_false = ExecutionResult::success(3, ExecutionMode::Fast, false);
  assert!(!fast_with_false.confirmed());
  assert_eq!(fast_with_false.execution_mode(), Some(ExecutionMode::Fast));

  // Contrast with Verified mode: explicitly preserves confirmation boolean
  let verified_with_true = ExecutionResult::success(2, ExecutionMode::Verified, true);
  assert!(verified_with_true.confirmed(), "Verified mode preserves true confirmed status");
  assert_eq!(verified_with_true.execution_mode(), Some(ExecutionMode::Verified));

  let verified_with_false = ExecutionResult::success(2, ExecutionMode::Verified, false);
  assert!(!verified_with_false.confirmed(), "Verified mode preserves false confirmed status");
  assert_eq!(verified_with_false.execution_mode(), Some(ExecutionMode::Verified));

  // Non-success variants always return confirmed() == false and execution_mode() == None
  let gate_failed = ExecutionResult::GateFailed {
    step_id: "s1".to_string(),
    message: "failed".to_string(),
    consecutive_failures: 1,
  };
  assert!(!gate_failed.confirmed());
  assert_eq!(gate_failed.execution_mode(), None);

  let escalated = ExecutionResult::EscalatedToVlm {
    step_id: "s1".to_string(),
    reason: "escalated".to_string(),
    isolated: false,
  };
  assert!(!escalated.confirmed());
  assert_eq!(escalated.execution_mode(), None);

  let mode_conflict = ExecutionResult::ModeConflictError {
    operation_name: "op".to_string(),
    message: "conflict".to_string(),
  };
  assert!(!mode_conflict.confirmed());
  assert_eq!(mode_conflict.execution_mode(), None);
}
