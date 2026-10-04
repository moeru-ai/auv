//! Acceptance test suite for Brief PR2 (feat/auto-loop-v0.1: Full Dual-Loop Auto Mode).
//!
//! Validates all 7 acceptance criteria:
//! 1. Clean trajectory (`record.json`) automatically compiles into operation semantically matching manual YAML.
//! 2. Dirty trajectory with ambiguous drops is rejected into manual review queue (`REJECT_AMBIGUOUS_DROPS`).
//! 3. Parameter gate: `Next()` parameterization is rejected; two `SetVolume(40/60)` trajectories generalize to `{{volume}}`.
//! 4. Blast-radius gate: trajectory with deletion action is rejected (`REJECT_BLAST_RADIUS_VIOLATION`).
//! 5. Scheduler: exact key hit with 0 embedding calls; unknown task uses embedding top-3 and strict preconditions intercept false hits.
//! 6. Runtime: fault injection triggers gate catch -> 2 consecutive failures -> auto-isolation -> routes to VLM slow loop.
//! 7. Zero silent errors: every decision is logged with a structured `ReasonCode`.

use auv_auto_loop::compiler::AutoCompiler;
use auv_auto_loop::decision_log::DecisionLogger;
use auv_auto_loop::models::{ReasonCode, TrajectoryRecord, TrajectoryStep, VerificationGateDef};
use auv_auto_loop::runtime::{ExecutionResult, RuntimeEnvironment, RuntimeExecutor};
use auv_auto_loop::scheduler::{FastLoopScheduler, OperationCatalog, TaskRequest};
use std::collections::HashMap;

fn load_clean_record() -> TrajectoryRecord {
  let record_path = "f:/auv/docs/ai/references/driver/2026-10-04-qqmusic-vlm-record.json";
  let content = std::fs::read_to_string(record_path).expect("Failed to read 2026-10-04-qqmusic-vlm-record.json");
  serde_json::from_str(&content).expect("Failed to deserialize clean record.json")
}

#[test]
fn test_1_clean_trajectory_auto_compilation() {
  let logger = DecisionLogger::new();
  let compiler = AutoCompiler::new(&logger);
  let record = load_clean_record();

  let op = compiler.compile(&record, None).expect("Clean trajectory must compile automatically without human intervention");

  // Semantic checks against manual YAML
  assert_eq!(op.schema_version, "auv.operation.v1");
  assert_eq!(op.name, "qqmusic.prepare_playback");
  assert_eq!(op.target.app_name, "QQMusic.exe");
  assert_eq!(op.target.backend, "windows.smtc+coreaudio+wgc");
  assert_eq!(op.steps.len(), 5);

  // Step 1: SMTC session query
  assert!(matches!(op.steps[0].verification_gate, VerificationGateDef::SmtcSessionPresent { .. }));

  // Step 2: Volume 40% with mandatory 0.05 float tolerance
  match &op.steps[1].verification_gate {
    VerificationGateDef::StatusAndVolumeGate {
      expected_volume,
      volume_tolerance,
      ..
    } => {
      assert_eq!(*expected_volume, 0.40);
      assert_eq!(*volume_tolerance, 0.05);
    }
    other => panic!("Step 2 expected StatusAndVolumeGate, got {:?}", other),
  }

  // Step 3: Skip next track gate
  assert!(matches!(
    op.steps[2].verification_gate,
    VerificationGateDef::TitleChangeGate {
      require_title_change: true,
      ..
    }
  ));

  // Step 5: WGC alive gate (forbids pixel hash)
  match &op.steps[4].verification_gate {
    VerificationGateDef::WgcAliveGate {
      min_non_black_ratio,
      ..
    } => {
      assert!(*min_non_black_ratio >= 0.50);
    }
    other => panic!("Step 5 expected WgcAliveGate, got {:?}", other),
  }

  // Verify tags: all steps covered by templates, no unverified tag
  assert!(!op.tags.contains(&"unverified-step".to_string()));

  // Verify decision log reason codes
  let approved_logs = logger.find_by_reason(ReasonCode::CompilationApproved);
  assert_eq!(approved_logs.len(), 1);
  assert_eq!(logger.find_by_reason(ReasonCode::CleaningZeroAmbiguity).len(), 1);
  assert_eq!(logger.find_by_reason(ReasonCode::ParameterGateApproved).len(), 1);
  assert_eq!(logger.find_by_reason(ReasonCode::BlastRadiusApproved).len(), 1);
}

#[test]
fn test_2_dirty_trajectory_rejection_into_manual_queue() {
  let logger = DecisionLogger::new();
  let compiler = AutoCompiler::new(&logger);
  let mut dirty_record = load_clean_record();

  // Inject an ambiguous step simulating background state mutation without verifiable feedback
  dirty_record.structured_trajectory.insert(
    2,
    TrajectoryStep {
      step: 99,
      intent: "Simulate unverified_mutation in background with no visible feedback".to_string(),
      action: "simulate_ambiguous_mutation --silent".to_string(),
      perception: "No direct perception signal available".to_string(),
      result: serde_json::json!({ "mutation": true }),
      pre_state: None,
      post_state: None,
    },
  );

  let result = compiler.compile(&dirty_record, None);
  assert!(result.is_err(), "Dirty trajectory must be rejected by cleaning gate");

  let review_item = result.unwrap_err();
  assert_eq!(review_item.reason_code, ReasonCode::RejectAmbiguousDrops);
  assert!(review_item.reason_description.contains("ambiguous drop"));

  // Check structured log
  let rejection_logs = logger.find_by_reason(ReasonCode::RejectAmbiguousDrops);
  assert_eq!(rejection_logs.len(), 1);
}

#[test]
fn test_3_parameter_gate_anti_unification_and_guards() {
  let logger = DecisionLogger::new();
  let compiler = AutoCompiler::new(&logger);

  // Test 3a: Forbidden parameterization on Next()
  let mut rec_next_1 = load_clean_record();
  let mut rec_next_2 = load_clean_record();
  rec_next_1.structured_trajectory[2].action = "cargo run -p auv-driver-windows -- next 1".to_string();
  rec_next_2.structured_trajectory[2].action = "cargo run -p auv-driver-windows -- next 2".to_string();

  let err_next = compiler.compile(&rec_next_1, Some(&[rec_next_2])).expect_err("Next() parameterization must be barred");
  assert_eq!(err_next.reason_code, ReasonCode::RejectForbiddenParameterization);
  assert_eq!(logger.find_by_reason(ReasonCode::RejectForbiddenParameterization).len(), 1);

  // Test 3b: Isomorphic parameterization on SetVolume(40/60)
  let logger_unify = DecisionLogger::new();
  let compiler_unify = AutoCompiler::new(&logger_unify);
  let mut rec_vol_40 = load_clean_record();
  let mut rec_vol_60 = load_clean_record();

  rec_vol_40.structured_trajectory[1].action = "cargo run -p auv-driver-windows --bin qqmusic_p0 -- volume 0.40".to_string();
  rec_vol_60.structured_trajectory[1].action = "cargo run -p auv-driver-windows --bin qqmusic_p0 -- volume 0.60".to_string();

  let op = compiler_unify
    .compile(&rec_vol_40, Some(&[rec_vol_60]))
    .expect("Isomorphic volume trajectories must anti-unify into parameterized operation");

  assert_eq!(op.parameters.len(), 1);
  let vol_param = &op.parameters[0];
  assert_eq!(vol_param.name, "volume");
  assert_eq!(vol_param.param_type, "float");
  assert_eq!(vol_param.min_value, Some(0.0));
  assert_eq!(vol_param.max_value, Some(1.0));

  assert_eq!(logger_unify.find_by_reason(ReasonCode::ParameterGateApproved).len(), 1);
}

#[test]
fn test_4_blast_radius_gate_rejects_destructive_actions() {
  let logger = DecisionLogger::new();
  let compiler = AutoCompiler::new(&logger);
  let mut bad_record = load_clean_record();

  // Inject destructive action: delete user cache
  bad_record.structured_trajectory.push(TrajectoryStep {
    step: 6,
    intent: "Delete user cache directory".to_string(),
    action: "delete C:\\Users\\QQMusic\\cache".to_string(),
    perception: "File system deletion".to_string(),
    result: serde_json::json!({ "deleted": true }),
    pre_state: None,
    post_state: None,
  });

  let result = compiler.compile(&bad_record, None);
  assert!(result.is_err(), "Destructive action must be rejected by blast-radius gate");

  let review_item = result.unwrap_err();
  assert_eq!(review_item.reason_code, ReasonCode::RejectBlastRadiusViolation);
  assert!(review_item.reason_description.contains("blacklisted"));

  assert_eq!(logger.find_by_reason(ReasonCode::RejectBlastRadiusViolation).len(), 1);
}

#[test]
fn test_5_scheduler_exact_key_hit_vs_embedding_precondition_interception() {
  let logger = DecisionLogger::new();
  let compiler = AutoCompiler::new(&logger);
  let record = load_clean_record();
  let op = compiler.compile(&record, None).unwrap();

  let mut catalog = OperationCatalog::new();
  catalog.register_active(op);

  let scheduler = FastLoopScheduler::new(&logger);

  // 5a: Exact key match (app_name + task_name) -> 0 embedding calls
  let mut valid_context = HashMap::new();
  valid_context.insert("App.ProcessName".to_string(), serde_json::json!("QQMusic.exe"));

  let req_exact = TaskRequest {
    app_name: "QQMusic.exe".to_string(),
    task_name: "prepare_playback".to_string(),
    instruction: "把音乐调好：音量40%，切到下一首，确保在播".to_string(),
    current_context: valid_context,
    embedding_vector: None,
  };

  let outcome_exact = scheduler.schedule(&req_exact, &catalog);
  assert!(outcome_exact.selected_operation.is_some());
  assert!(!outcome_exact.embedding_called);
  assert_eq!(outcome_exact.reason_code, ReasonCode::ExactKeyMatch);

  let exact_logs = logger.find_by_reason(ReasonCode::ExactKeyMatch);
  assert_eq!(exact_logs.len(), 1);

  // 5b: Unknown task -> Falls back to embedding top-3 -> Strict precondition intercepts false hit
  let mut mismatch_context = HashMap::new();
  mismatch_context.insert(
    "App.ProcessName".to_string(),
    serde_json::json!("Notepad.exe"), // Mismatch!
  );

  let req_unknown = TaskRequest {
    app_name: "QQMusic.exe".to_string(),
    task_name: "long_tail_random_task".to_string(),
    instruction: "把音乐调好：音量40%".to_string(),
    current_context: mismatch_context,
    embedding_vector: Some(vec![0.5, 0.5, 0.5, 0.5]),
  };

  let outcome_unknown = scheduler.schedule(&req_unknown, &catalog);
  assert!(outcome_unknown.selected_operation.is_none());
  assert!(outcome_unknown.embedding_called);
  assert_eq!(outcome_unknown.reason_code, ReasonCode::SchedulerMissEscalateVlm);

  let embedding_logs = logger.find_by_reason(ReasonCode::EmbeddingTop3Candidate);
  assert_eq!(embedding_logs.len(), 1);

  let mismatch_logs = logger.find_by_reason(ReasonCode::PreconditionMismatch);
  assert_eq!(mismatch_logs.len(), 1);
}

#[test]
fn test_6_runtime_fault_injection_auto_isolation_and_vlm_routing() {
  let logger = DecisionLogger::new();
  let compiler = AutoCompiler::new(&logger);
  let record = load_clean_record();
  let op = compiler.compile(&record, None).unwrap();

  let mut catalog = OperationCatalog::new();
  catalog.register_active(op.clone());

  let mut executor = RuntimeExecutor::new(&logger);

  // Run 1: Clean execution -> Success
  let mut clean_env = RuntimeEnvironment::default();
  let res_1 = executor.execute(&op, &mut clean_env, &mut catalog);
  assert!(matches!(res_1, ExecutionResult::Success { .. }));
  assert!(!catalog.is_isolated(&op.name));

  // Run 2: Fault injection 1 -> Gate fails (1st failure)
  let mut fault_env_1 = RuntimeEnvironment {
    fault_audio_service_down: true,
    playback_status: "Paused".to_string(),
    current_volume: 0.99,
    ..Default::default()
  };

  let res_2 = executor.execute(&op, &mut fault_env_1, &mut catalog);
  match res_2 {
    ExecutionResult::GateFailed {
      consecutive_failures,
      ..
    } => {
      assert_eq!(consecutive_failures, 1);
    }
    other => panic!("Expected GateFailed with consecutive=1, got {:?}", other),
  }
  assert!(!catalog.is_isolated(&op.name), "Must not isolate on 1st failure");

  // Run 3: Fault injection 2 -> Gate fails (2nd failure) -> Triggers auto-isolation!
  let mut fault_env_2 = RuntimeEnvironment {
    fault_audio_service_down: true,
    playback_status: "Paused".to_string(),
    current_volume: 0.99,
    ..Default::default()
  };

  let res_3 = executor.execute(&op, &mut fault_env_2, &mut catalog);
  match res_3 {
    ExecutionResult::EscalatedToVlm { isolated, .. } => {
      assert!(isolated, "Must be auto-isolated on consecutive failures >= 2");
    }
    other => panic!("Expected EscalatedToVlm with isolated=true, got {:?}", other),
  }

  // Operation is now isolated in catalog
  assert!(catalog.is_isolated(&op.name));
  assert_eq!(catalog.manual_review_queue().len(), 1);
  assert_eq!(catalog.manual_review_queue()[0].reason_code, ReasonCode::AutoIsolatedConsecutiveFailures);

  // Run 4: Scheduler test for isolated operation -> Routes directly to VLM slow loop
  let scheduler = FastLoopScheduler::new(&logger);
  let mut context = HashMap::new();
  context.insert("App.ProcessName".to_string(), serde_json::json!("QQMusic.exe"));

  let req = TaskRequest {
    app_name: "QQMusic.exe".to_string(),
    task_name: "prepare_playback".to_string(),
    instruction: "把音乐调好：音量40%，切到下一首，确保在播".to_string(),
    current_context: context,
    embedding_vector: None,
  };

  let outcome = scheduler.schedule(&req, &catalog);
  assert!(outcome.selected_operation.is_none());
  assert_eq!(outcome.reason_code, ReasonCode::EscalateToVlm);

  assert_eq!(logger.find_by_reason(ReasonCode::AutoIsolatedConsecutiveFailures).len(), 1);
}

#[test]
fn test_7_zero_silent_errors_invariant() {
  let logger = DecisionLogger::new();
  let compiler = AutoCompiler::new(&logger);
  let record = load_clean_record();

  let op = compiler.compile(&record, None).unwrap();
  let mut catalog = OperationCatalog::new();
  catalog.register_active(op.clone());

  let mut executor = RuntimeExecutor::new(&logger);
  let mut env = RuntimeEnvironment::default();
  let _ = executor.execute(&op, &mut env, &mut catalog);

  let entries = logger.entries();
  assert!(!entries.is_empty(), "Must have logged multiple decisions");

  for entry in &entries {
    assert!(!entry.timestamp.is_empty(), "Timestamp must be present");
    assert!(!entry.task_name.is_empty(), "Task name must be present");
    assert!(!entry.message.is_empty(), "Message must be present");
    // Verify reason code string representation is non-empty and screaming snake case
    assert!(!entry.reason_code.as_str().is_empty());
    assert!(entry.reason_code.as_str().chars().all(|c| c.is_ascii_uppercase() || c == '_'));
  }
}

#[test]
fn test_8_strict_mode_unverified_step_instant_isolation() {
  let logger = DecisionLogger::new();
  let compiler = AutoCompiler::new(&logger);
  let mut custom_record = load_clean_record();

  // Inject a custom action that does not match known templates (SMTC, CoreAudio, WGC)
  custom_record.structured_trajectory.push(TrajectoryStep {
    step: 6,
    intent: "Inspect custom player layout metrics".to_string(),
    action: "inspect_layout --deep".to_string(),
    perception: "Layout tree bounding boxes".to_string(),
    result: serde_json::json!({ "layout_valid": true }),
    pre_state: None,
    post_state: None,
  });

  let op = compiler.compile(&custom_record, None).unwrap();
  assert!(op.tags.contains(&"unverified-step".to_string()));
  assert!(op.steps.iter().any(|s| s.is_unverified));

  let mut catalog = OperationCatalog::new();
  catalog.register_active(op.clone());

  let mut executor = RuntimeExecutor::new(&logger);

  // In strict mode: 1st failure triggers instant isolation without a 2nd chance
  let mut fault_env = RuntimeEnvironment::default();
  fault_env.custom_state.insert("unverified_step_fail".to_string(), serde_json::json!(true));

  let res = executor.execute(&op, &mut fault_env, &mut catalog);
  match res {
    ExecutionResult::EscalatedToVlm { isolated, .. } => {
      assert!(isolated, "Strict mode must instantly isolate on 1st failure");
    }
    other => panic!("Expected instant EscalatedToVlm, got {:?}", other),
  }

  assert!(catalog.is_isolated(&op.name));
  assert_eq!(logger.find_by_reason(ReasonCode::StrictStepFailed).len(), 1);
  assert_eq!(logger.find_by_reason(ReasonCode::AutoIsolatedConsecutiveFailures).len(), 1);
}
