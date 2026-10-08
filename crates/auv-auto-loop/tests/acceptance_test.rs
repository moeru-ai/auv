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
use auv_auto_loop::models::{
  CompilationMetadata, ExecutionMode, OPERATION_SCHEMA_VERSION, OperationDef, OperationStepDef, ReasonCode, TargetMetadata,
  TrajectoryRecord, TrajectoryStep, VerificationGateDef,
};
use auv_auto_loop::runtime::{ExecutionResult, RuntimeEnvironment, RuntimeExecutor};
use auv_auto_loop::scheduler::{FastLoopScheduler, OperationCatalog, TaskRequest};
use std::collections::HashMap;

fn load_clean_record() -> TrajectoryRecord {
  let record_path =
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/ai/references/driver/2026-10-04-qqmusic-vlm-record.json");
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
  assert_eq!(op.schema_version, OPERATION_SCHEMA_VERSION);
  assert_eq!(op.execution_mode, ExecutionMode::Verified);
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

  // Inject a realistic background state mutation with pre_state and post_state diff.
  // Note: Neither action nor intent contains "ambiguous"; caught purely via state-diff on non-goal keys.
  dirty_record.structured_trajectory.insert(
    2,
    TrajectoryStep {
      step: 99,
      intent: "Update internal audio buffer prefetch configuration".to_string(),
      action: "configure_audio_engine --prefetch-buffer 512".to_string(),
      perception: "Internal driver registry updated with no visible audio change".to_string(),
      result: serde_json::json!({ "success": true }),
      pre_state: Some(serde_json::json!({ "prefetch_buffer_size": 256 })),
      post_state: Some(serde_json::json!({ "prefetch_buffer_size": 512 })),
    },
  );

  let result = compiler.compile(&dirty_record, None);
  assert!(result.is_err(), "Dirty trajectory must be rejected by cleaning gate");

  let review_item = result.unwrap_err();
  assert_eq!(review_item.reason_code, ReasonCode::RejectAmbiguousDrops);
  assert!(review_item.reason_description.contains("ambiguous"));
  assert!(review_item.reason_description.contains("prefetch_buffer_size"));

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
  catalog.register_active(op).unwrap();

  let scheduler = FastLoopScheduler::new(&logger);

  // 5a: Exact key match (app_name + task_name) -> 0 embedding calls
  let mut valid_context = HashMap::new();
  valid_context.insert("App.ProcessName".to_string(), serde_json::json!("QQMusic.exe"));

  let req_exact = TaskRequest {
    app_name: "QQMusic.exe".to_string(),
    task_name: "prepare_playback".to_string(),
    instruction: "把音乐调好：音量40%，切到下一首，确保在播".to_string(),
    requested_mode: None,
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
    requested_mode: None,
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
  catalog.register_active(op.clone()).unwrap();

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
    requested_mode: None,
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
  catalog.register_active(op.clone()).unwrap();

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
  catalog.register_active(op.clone()).unwrap();

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

#[test]
fn test_9_isolation_persistence_across_restarts() {
  let logger = DecisionLogger::new();
  let compiler = AutoCompiler::new(&logger);
  let record = load_clean_record();
  let op = compiler.compile(&record, None).unwrap();

  let temp_dir = tempfile::tempdir().expect("Failed to create tempdir");
  let persistence_path = temp_dir.path().join("catalog_isolation.jsonl");

  // Step 1: Initialize catalog with persistence path
  let mut catalog = OperationCatalog::with_persistence(&persistence_path).expect("Failed to create catalog with persistence");
  assert_eq!(catalog.persistence_path(), Some(persistence_path.as_path()));
  assert!(!catalog.is_isolated(&op.name));

  let activated = catalog.register_active(op.clone()).expect("register_active must succeed");
  assert!(activated);
  assert!(catalog.get_active(&op.name).is_some());

  // Step 2: Trigger consecutive failures to cause auto-isolation
  let mut executor = RuntimeExecutor::new(&logger);
  let mut fault_env = RuntimeEnvironment {
    fault_audio_service_down: true,
    playback_status: "Paused".to_string(),
    current_volume: 0.99,
    ..Default::default()
  };

  // Run 1: 1st failure
  let res_1 = executor.execute(&op, &mut fault_env, &mut catalog);
  assert!(matches!(
    res_1,
    ExecutionResult::GateFailed {
      consecutive_failures: 1,
      ..
    }
  ));
  assert!(!catalog.is_isolated(&op.name));

  // Run 2: 2nd failure -> auto-isolation
  let res_2 = executor.execute(&op, &mut fault_env, &mut catalog);
  assert!(matches!(res_2, ExecutionResult::EscalatedToVlm { isolated: true, .. }));
  assert!(catalog.is_isolated(&op.name));
  assert!(catalog.get_active(&op.name).is_none());

  // Step 3: Verify the record was persisted to disk immediately
  assert!(persistence_path.exists());
  let content = std::fs::read_to_string(&persistence_path).expect("Failed to read persistence file");
  assert!(!content.trim().is_empty());
  assert!(content.contains(&op.name));
  assert!(content.contains("AUTO_ISOLATED_CONSECUTIVE_FAILURES"));

  // Step 4: Simulate system restart with a brand new catalog instance loading from the same path
  drop(catalog);
  let mut restarted_catalog = OperationCatalog::with_persistence(&persistence_path).expect("Failed to reload catalog with persistence");

  // Verify isolation state is preserved across restart
  assert!(restarted_catalog.is_isolated(&op.name), "Operation must remain isolated on restart");
  let isolation_rec = restarted_catalog.get_isolated_record(&op.name).expect("Persisted isolation record must be present");
  assert_eq!(isolation_rec.operation_name, op.name);
  assert_eq!(isolation_rec.reason_code, ReasonCode::AutoIsolatedConsecutiveFailures);
  assert_eq!(isolation_rec.execution_mode, Some(ExecutionMode::Verified));
  assert!(!isolation_rec.isolated_at.is_empty());

  // Step 5: Prevent bad operation revival!
  // Attempting to register the bad operation into active pool must be rejected.
  let revived = restarted_catalog.register_active(op.clone()).expect("revival check must succeed");
  assert!(!revived, "Registering an isolated operation into active pool must be rejected");
  assert!(restarted_catalog.get_active(&op.name).is_none(), "Bad operation must NOT be in active pool");
  assert!(restarted_catalog.is_isolated(&op.name), "Bad operation must remain isolated");

  // Step 6: Scheduler checks must immediately escalate to VLM without executing or requiring 2 failures
  let scheduler = FastLoopScheduler::new(&logger);
  let mut context = HashMap::new();
  context.insert("App.ProcessName".to_string(), serde_json::json!("QQMusic.exe"));

  let req = TaskRequest {
    app_name: "QQMusic.exe".to_string(),
    task_name: "prepare_playback".to_string(),
    instruction: "把音乐调好：音量40%，切到下一首，确保在播".to_string(),
    requested_mode: None,
    current_context: context,
    embedding_vector: None,
  };

  let outcome = scheduler.schedule(&req, &restarted_catalog);
  assert!(outcome.selected_operation.is_none(), "Isolated operation must not be selected");
  assert_eq!(outcome.reason_code, ReasonCode::EscalateToVlm, "Must escalate to VLM immediately without execution");
  assert!(!outcome.embedding_called, "Exact isolated key must not call embeddings");
}

#[test]
fn test_10_missing_execution_mode_rejected_fail_closed() {
  let mut catalog = OperationCatalog::new();

  // Construct JSON omitting the required execution_mode field
  let raw_json_no_mode = serde_json::json!({
    "schema_version": "auv.operation.v2",
    "name": "qqmusic.test_op_no_mode",
    "description": "Test operation without execution_mode",
    "compilation_metadata": {
      "compiler": "test",
      "source_record": "test",
      "date": "2026-10-08",
      "crux_goal": "test"
    },
    "target": {
      "app_name": "QQMusic.exe",
      "backend": "windows.smtc"
    },
    "steps": []
  })
  .to_string();

  // Serde deserialization must fail directly
  assert!(serde_json::from_str::<OperationDef>(&raw_json_no_mode).is_err(), "Missing execution_mode must fail serde deserialization");

  // Admission via catalog must be rejected fail-closed with REJECT_MODE_UNDECLARED
  let admission_res = catalog.admit_operation_json(&raw_json_no_mode, "catalog_entry_10");
  assert!(admission_res.is_err());
  let failure = admission_res.unwrap_err();
  assert_eq!(failure.reason_code, ReasonCode::RejectModeUndeclared);
  assert_eq!(failure.operation_key, "qqmusic.test_op_no_mode");
  assert_eq!(failure.location, "catalog_entry_10");
  assert!(failure.message.contains("missing required field 'execution_mode'"));

  // Must not be added to active operations
  assert!(catalog.get_active("qqmusic.test_op_no_mode").is_none());
}

#[test]
fn test_11_mode_mismatch_rejected_with_zero_side_effects() {
  let logger = DecisionLogger::new();
  let compiler = AutoCompiler::new(&logger);
  let record = load_clean_record();
  let op = compiler.compile(&record, None).unwrap();
  assert_eq!(op.execution_mode, ExecutionMode::Verified);

  let mut catalog = OperationCatalog::new();
  catalog.register_active(op.clone()).unwrap();

  let scheduler = FastLoopScheduler::new(&logger);
  let mut context = HashMap::new();
  context.insert("App.ProcessName".to_string(), serde_json::json!("QQMusic.exe"));

  // Request Fast execution mode for a Verified operation
  let req = TaskRequest {
    app_name: "QQMusic.exe".to_string(),
    task_name: "prepare_playback".to_string(),
    instruction: "把音乐调好：音量40%".to_string(),
    requested_mode: Some(ExecutionMode::Fast),
    current_context: context,
    embedding_vector: None,
  };

  let outcome = scheduler.schedule(&req, &catalog);

  // Must NOT select operation; must escalate with MODE_MISMATCH_ESCALATE_VLM
  assert!(outcome.selected_operation.is_none(), "Verified operation must never be scheduled for Fast mode");
  assert_eq!(outcome.reason_code, ReasonCode::ModeMismatchEscalateVlm);
  assert!(outcome.message.contains("Mode mismatch"));

  // Check structured decision log
  let mismatch_logs = logger.find_by_reason(ReasonCode::ModeMismatchEscalateVlm);
  assert_eq!(mismatch_logs.len(), 1);
  assert_eq!(mismatch_logs[0].details.get("zero_side_effects").and_then(|v| v.as_bool()), Some(true));

  // RuntimeExecutor fast-path execution on Verified op must also fail with ModeConflictError
  let mut executor = RuntimeExecutor::new(&logger);
  let mut env = RuntimeEnvironment::default();
  let initial_volume = env.current_volume;
  let initial_title = env.current_title.clone();

  let exec_res = executor.execute_fast(&op, &mut env, &mut catalog);
  assert!(matches!(exec_res, ExecutionResult::ModeConflictError { .. }));

  // Assert ZERO side-effects: environment unchanged, no commands executed
  assert_eq!(env.current_volume, initial_volume);
  assert_eq!(env.current_title, initial_title);
}

#[test]
fn test_12_fast_mode_never_reports_confirmed_true() {
  let logger = DecisionLogger::new();
  let mut catalog = OperationCatalog::new();
  let mut executor = RuntimeExecutor::new(&logger);

  // Construct a valid Fast-mode operation with pure action dispatch steps
  let fast_op = OperationDef {
    schema_version: OPERATION_SCHEMA_VERSION.to_string(),
    name: "qqmusic.fast_action".to_string(),
    description: "Fast action dispatch without effect gates".to_string(),
    execution_mode: ExecutionMode::Fast,
    compilation_metadata: CompilationMetadata {
      compiler: "test".to_string(),
      source_record: "test".to_string(),
      date: "2026-10-08".to_string(),
      crux_goal: "fast dispatch".to_string(),
    },
    target: TargetMetadata {
      app_name: "QQMusic.exe".to_string(),
      backend: "windows.smtc".to_string(),
    },
    preconditions: vec![],
    parameters: vec![],
    steps: vec![OperationStepDef {
      id: "step_1_dispatch".to_string(),
      name: "Play dispatch".to_string(),
      description: "Trigger play dispatch".to_string(),
      action: serde_json::json!({ "type": "play", "app_id": "QQMusic.exe" }),
      verification_gate: VerificationGateDef::CustomAssertion {
        expression: "true".to_string(),
        timeout_ms: 100,
        escalate_on_mismatch: "none".to_string(),
      },
      is_unverified: false,
    }],
    tags: vec![],
  };

  catalog.register_active(fast_op.clone()).unwrap();

  let mut env = RuntimeEnvironment::default();
  let res = executor.execute(&fast_op, &mut env, &mut catalog);

  // Assert execution result confirmed is strictly false
  match res {
    ExecutionResult::Success {
      execution_mode,
      confirmed,
      ..
    } => {
      assert_eq!(execution_mode, ExecutionMode::Fast);
      assert!(!confirmed, "REDLINE: Fast mode result must NEVER report confirmed: true");
    }
    other => panic!("Expected Success, got {:?}", other),
  }
  assert!(!res.confirmed(), "res.confirmed() must be false for Fast mode");

  // Even if someone explicitly passes confirmed: true to ExecutionResult::success,
  // constructor must clamp/force confirmed to false in Fast mode!
  let forced = ExecutionResult::success(1, ExecutionMode::Fast, true);
  assert!(!forced.confirmed(), "ExecutionResult constructor must force confirmed: false for Fast mode");
}

#[test]
fn test_13_high_risk_actions_barred_from_fast_mode() {
  let logger = DecisionLogger::new();
  let compiler = AutoCompiler::new(&logger);
  let mut bad_record = load_clean_record();

  // Inject destructive deletion action
  bad_record.structured_trajectory.push(TrajectoryStep {
    step: 6,
    intent: "Delete user cache directory".to_string(),
    action: "delete C:\\Users\\QQMusic\\cache".to_string(),
    perception: "File system deletion".to_string(),
    result: serde_json::json!({ "deleted": true }),
    pre_state: None,
    post_state: None,
  });

  // Attempt to compile in Fast mode -> Barred!
  let res_fast = compiler.compile_with_mode(&bad_record, None, Some(ExecutionMode::Fast));
  assert!(res_fast.is_err(), "High-risk actions must be barred from Fast mode");
  let review_item = res_fast.unwrap_err();
  assert!(review_item.reason_code == ReasonCode::RejectBlastRadiusViolation || review_item.reason_code == ReasonCode::RejectModeConflict);
}

#[test]
fn test_14_unverified_step_with_fast_mode_rejected_by_compilation_gate() {
  let logger = DecisionLogger::new();
  let compiler = AutoCompiler::new(&logger);
  let mut custom_record = load_clean_record();

  // Custom uncovered step -> produces unverified-step
  custom_record.structured_trajectory.push(TrajectoryStep {
    step: 6,
    intent: "Inspect unknown layout".to_string(),
    action: "custom_unknown_action --opaque".to_string(),
    perception: "Opaque data".to_string(),
    result: serde_json::json!({ "ok": true }),
    pre_state: None,
    post_state: None,
  });

  // Attempt to compile unverified-step in Fast mode -> Compilation gate rejects!
  let res = compiler.compile_with_mode(&custom_record, None, Some(ExecutionMode::Fast));
  assert!(res.is_err(), "unverified-step + Fast mode combination must be rejected by compilation gate");
  let review = res.unwrap_err();
  assert_eq!(review.reason_code, ReasonCode::RejectModeConflict);
  assert!(review.reason_description.contains("unverified-step cannot be combined with Fast mode"));
}

#[test]
fn test_15_legacy_schema_v1_rejected_fail_closed() {
  let mut catalog = OperationCatalog::new();

  let v1_json = serde_json::json!({
    "schema_version": "auv.operation.v1",
    "name": "qqmusic.legacy_v1_op",
    "description": "Legacy operation from v1 schema",
    "execution_mode": "verified",
    "compilation_metadata": {
      "compiler": "legacy",
      "source_record": "legacy",
      "date": "2026-10-04",
      "crux_goal": "legacy"
    },
    "target": {
      "app_name": "QQMusic.exe",
      "backend": "windows.smtc"
    },
    "steps": []
  })
  .to_string();

  // Admission via catalog must fail fail-closed with REJECT_SCHEMA_VERSION_MISMATCH
  let res = catalog.admit_operation_json(&v1_json, "catalog_legacy_entry");
  assert!(res.is_err(), "Legacy schema_version must be rejected fail-closed");
  let failure = res.unwrap_err();
  assert_eq!(failure.reason_code, ReasonCode::RejectSchemaVersionMismatch);
  assert!(failure.message.contains("schema_version 'auv.operation.v1' does not match current 'auv.operation.v2'"));

  // register_active with v1 OperationDef must also be rejected
  let mut v1_op: OperationDef = serde_json::from_str(&v1_json).unwrap();
  v1_op.schema_version = "auv.operation.v1".to_string();
  let reg_res = catalog.register_active(v1_op);
  assert!(reg_res.is_err(), "register_active must reject v1 schema");
  assert_eq!(reg_res.unwrap_err().reason_code, ReasonCode::RejectSchemaVersionMismatch);

  // Must not have admitted any active operation
  assert!(catalog.get_active("qqmusic.legacy_v1_op").is_none());
}

#[test]
fn test_16_confirm_volume_not_blacklisted_and_safe_fast_compilation() {
  let step_confirm_volume = OperationStepDef {
    id: "step_1".to_string(),
    name: "confirm_volume".to_string(),
    description: "Confirm and set volume".to_string(),
    action: serde_json::json!({ "type": "set_volume", "value": 0.40 }),
    verification_gate: VerificationGateDef::CustomAssertion {
      expression: "true".to_string(),
      timeout_ms: 100,
      escalate_on_mismatch: "none".to_string(),
    },
    is_unverified: false,
  };

  // 1. Blast-radius gate must NOT reject "confirm_volume" as "rm"
  let blast_eval = auv_auto_loop::compiler::compile_gate::evaluate_blast_radius_gate(std::slice::from_ref(&step_confirm_volume));
  assert!(blast_eval.passed, "confirm_volume must NOT be falsely rejected by blast-radius gate: {:?}", blast_eval);

  // 2. Execution-mode gate must NOT bar "confirm_volume" from Fast mode as "rm"
  let fast_eval = auv_auto_loop::compiler::compile_gate::evaluate_execution_mode_gate(ExecutionMode::Fast, &[step_confirm_volume], &[]);
  assert!(fast_eval.passed, "confirm_volume must NOT be barred from Fast mode: {:?}", fast_eval);

  // 3. Genuine "rm -rf" action MUST be rejected by blast-radius gate
  let rm_eval = auv_auto_loop::compiler::compile_gate::evaluate_blast_radius_gate(&[OperationStepDef {
    id: "step_bad".to_string(),
    name: "remove cache".to_string(),
    description: "Destructive rm".to_string(),
    action: serde_json::json!({ "command": "rm -rf /cache" }),
    verification_gate: VerificationGateDef::CustomAssertion {
      expression: "true".to_string(),
      timeout_ms: 100,
      escalate_on_mismatch: "none".to_string(),
    },
    is_unverified: false,
  }]);
  assert!(!rm_eval.passed, "rm -rf MUST be rejected by blast-radius gate");
  assert_eq!(rm_eval.reason_code, ReasonCode::RejectBlastRadiusViolation);
}

#[test]
fn test_17_execution_mode_gate_aligned_with_derive_rejects_status_and_volume_gate_for_fast_mode() {
  let steps = vec![OperationStepDef {
    id: "step_status_volume".to_string(),
    name: "Volume and status check".to_string(),
    description: "Check volume and status".to_string(),
    action: serde_json::json!({ "type": "set_volume", "value": 0.40 }),
    verification_gate: VerificationGateDef::StatusAndVolumeGate {
      expected_status: "Playing".to_string(),
      expected_volume: 0.40,
      volume_tolerance: 0.05,
      timeout_ms: 1000,
      escalate_on_mismatch: "vlm".to_string(),
    },
    is_unverified: false,
  }];

  // derive_mode_from_steps derives Verified
  let derived = auv_auto_loop::compiler::gate_derive::derive_mode_from_steps(&steps);
  assert_eq!(derived, ExecutionMode::Verified);

  // evaluate_execution_mode_gate MUST also reject Fast mode for StatusAndVolumeGate
  let gate_eval = auv_auto_loop::compiler::compile_gate::evaluate_execution_mode_gate(ExecutionMode::Fast, &steps, &[]);
  assert!(!gate_eval.passed, "StatusAndVolumeGate must be rejected for Fast mode");
  assert_eq!(gate_eval.reason_code, ReasonCode::RejectModeConflict);
  assert!(gate_eval.message.contains("StatusAndVolumeGate"));
}

#[test]
fn test_18_register_active_validates_execution_mode_compatibility_fail_closed() {
  let mut catalog = OperationCatalog::new();
  let bad_op = OperationDef {
    schema_version: OPERATION_SCHEMA_VERSION.to_string(),
    name: "qqmusic.bad_fast_op".to_string(),
    description: "Fast op with status and volume gate".to_string(),
    execution_mode: ExecutionMode::Fast,
    compilation_metadata: CompilationMetadata {
      compiler: "test".to_string(),
      source_record: "test".to_string(),
      date: "2026-10-08".to_string(),
      crux_goal: "test".to_string(),
    },
    target: TargetMetadata {
      app_name: "QQMusic.exe".to_string(),
      backend: "windows.smtc".to_string(),
    },
    preconditions: vec![],
    parameters: vec![],
    steps: vec![OperationStepDef {
      id: "step_bad".to_string(),
      name: "Set volume".to_string(),
      description: "Set volume".to_string(),
      action: serde_json::json!({ "type": "set_volume", "value": 0.40 }),
      verification_gate: VerificationGateDef::StatusAndVolumeGate {
        expected_status: "Playing".to_string(),
        expected_volume: 0.40,
        volume_tolerance: 0.05,
        timeout_ms: 1000,
        escalate_on_mismatch: "vlm".to_string(),
      },
      is_unverified: false,
    }],
    tags: vec![],
  };

  let res = catalog.register_active(bad_op);
  assert!(res.is_err(), "register_active must reject Fast operation containing StatusAndVolumeGate");
  let failure = res.unwrap_err();
  assert_eq!(failure.reason_code, ReasonCode::RejectModeConflict);
  assert!(failure.message.contains("execution mode incompatible with gates"));
  assert!(catalog.get_active("qqmusic.bad_fast_op").is_none());
}

#[test]
fn test_19_scheduler_bidirectional_mode_check_rejects_fast_op_when_verified_requested() {
  let logger = DecisionLogger::new();
  let scheduler = FastLoopScheduler::new(&logger);
  let mut catalog = OperationCatalog::new();

  let fast_op = OperationDef {
    schema_version: OPERATION_SCHEMA_VERSION.to_string(),
    name: "qqmusic.fast_action".to_string(),
    description: "Fast action".to_string(),
    execution_mode: ExecutionMode::Fast,
    compilation_metadata: CompilationMetadata {
      compiler: "test".to_string(),
      source_record: "test".to_string(),
      date: "2026-10-08".to_string(),
      crux_goal: "test".to_string(),
    },
    target: TargetMetadata {
      app_name: "QQMusic.exe".to_string(),
      backend: "windows.smtc".to_string(),
    },
    preconditions: vec![],
    parameters: vec![],
    steps: vec![OperationStepDef {
      id: "step_1".to_string(),
      name: "Play dispatch".to_string(),
      description: "Trigger play dispatch".to_string(),
      action: serde_json::json!({ "type": "play", "app_id": "QQMusic.exe" }),
      verification_gate: VerificationGateDef::CustomAssertion {
        expression: "true".to_string(),
        timeout_ms: 100,
        escalate_on_mismatch: "none".to_string(),
      },
      is_unverified: false,
    }],
    tags: vec![],
  };

  catalog.register_active(fast_op).unwrap();

  // Caller requests Verified mode for a Fast operation
  let req = TaskRequest {
    app_name: "QQMusic.exe".to_string(),
    task_name: "fast_action".to_string(),
    instruction: "play music with verified confirmation".to_string(),
    requested_mode: Some(ExecutionMode::Verified),
    current_context: HashMap::new(),
    embedding_vector: None,
  };

  let outcome = scheduler.schedule(&req, &catalog);
  assert!(outcome.selected_operation.is_none(), "Verified request must NOT select a Fast operation (silent downgrade barred)");
  assert_eq!(outcome.reason_code, ReasonCode::ModeMismatchEscalateVlm);
  assert!(outcome.message.contains("Mode mismatch"));
}

#[test]
fn test_20_scheduler_candidate_mode_mismatch_preserves_typed_reason_code() {
  let logger = DecisionLogger::new();
  let scheduler = FastLoopScheduler::new(&logger);
  let mut catalog = OperationCatalog::new();

  // Register a Verified operation
  let verified_op = OperationDef {
    schema_version: OPERATION_SCHEMA_VERSION.to_string(),
    name: "qqmusic.verified_skip".to_string(),
    description: "Verified skip next track".to_string(),
    execution_mode: ExecutionMode::Verified,
    compilation_metadata: CompilationMetadata {
      compiler: "test".to_string(),
      source_record: "test".to_string(),
      date: "2026-10-08".to_string(),
      crux_goal: "test".to_string(),
    },
    target: TargetMetadata {
      app_name: "QQMusic.exe".to_string(),
      backend: "windows.smtc".to_string(),
    },
    preconditions: vec![],
    parameters: vec![],
    steps: vec![OperationStepDef {
      id: "step_1".to_string(),
      name: "Skip next".to_string(),
      description: "Skip next track".to_string(),
      action: serde_json::json!({ "type": "skip_next" }),
      verification_gate: VerificationGateDef::TitleChangeGate {
        require_title_change: true,
        timeout_ms: 1000,
        escalate_on_mismatch: "vlm".to_string(),
      },
      is_unverified: false,
    }],
    tags: vec![],
  };
  catalog.register_active(verified_op).unwrap();

  // Task query does NOT match exact key ("long_tail_skip" != "verified_skip"),
  // but embedding fallback returns "verified_skip" as candidate
  let req = TaskRequest {
    app_name: "QQMusic.exe".to_string(),
    task_name: "long_tail_skip".to_string(),
    instruction: "skip next track fast".to_string(),
    requested_mode: Some(ExecutionMode::Fast),
    current_context: HashMap::new(),
    embedding_vector: Some(vec![0.5, 0.5, 0.5, 0.5]),
  };

  let outcome = scheduler.schedule(&req, &catalog);
  assert!(outcome.selected_operation.is_none());
  assert!(outcome.embedding_called, "Long tail query must invoke embedding fallback");
  assert_eq!(
    outcome.reason_code,
    ReasonCode::ModeMismatchEscalateVlm,
    "Outcome must preserve typed ReasonCode::ModeMismatchEscalateVlm rather than generic scheduler miss"
  );
}
