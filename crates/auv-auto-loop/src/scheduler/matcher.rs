//! Fast-loop scheduler matching engine.
//!
//! Strict priority order:
//! 1. Exact operation key / target fingerprint -> Direct hit without embedding call.
//! 2. Unmatched -> Embedding candidate Top-3 (long-tail fallback).
//! 3. Candidate -> Strict precondition checks (App.ProcessName, window state, etc.).
//!    All preconditions must pass, otherwise execution is intercepted.

use crate::decision_log::DecisionLogger;
use crate::models::{DecisionAction, DecisionCategory, ExecutionMode, OperationDef, PreconditionDef, ReasonCode};
use crate::scheduler::catalog::OperationCatalog;
use std::collections::HashMap;

/// Request to find and schedule an executable operation.
#[derive(Debug, Clone)]
pub struct TaskRequest {
  pub app_name: String,
  pub task_name: String,
  pub instruction: String,
  pub requested_mode: Option<ExecutionMode>,
  pub current_context: HashMap<String, serde_json::Value>,
  pub embedding_vector: Option<Vec<f32>>,
}

/// The outcome of the scheduling process.
#[derive(Debug, Clone)]
pub struct SchedulingOutcome {
  pub selected_operation: Option<OperationDef>,
  pub embedding_called: bool,
  pub reason_code: ReasonCode,
  pub message: String,
}

pub struct FastLoopScheduler<'a> {
  logger: &'a DecisionLogger,
}

impl<'a> FastLoopScheduler<'a> {
  pub fn new(logger: &'a DecisionLogger) -> Self {
    Self { logger }
  }

  /// Schedules an operation for a task request.
  pub fn schedule(&self, request: &TaskRequest, catalog: &OperationCatalog) -> SchedulingOutcome {
    let task_id = &request.task_name;

    // Check if task is explicitly isolated
    let canonical_key = format_canonical_key(&request.app_name, &request.task_name);
    if catalog.is_isolated(&canonical_key) {
      self.logger.log(
        DecisionCategory::Scheduling,
        DecisionAction::Escalated,
        ReasonCode::EscalateToVlm,
        task_id,
        Some(canonical_key.clone()),
        format!("Operation '{}' is isolated. Bypassing fast loop and routing to VLM slow loop.", canonical_key),
        serde_json::json!({ "isolated": true }),
      );

      return SchedulingOutcome {
        selected_operation: None,
        embedding_called: false,
        reason_code: ReasonCode::EscalateToVlm,
        message: format!("Operation '{}' is isolated; routed directly to VLM.", canonical_key),
      };
    }

    // Step 1: Exact Operation Key / Target Fingerprint Match
    if let Some(op) = catalog.get_active(&canonical_key) {
      // Check execution mode compatibility (bidirectional)
      if let Some(req_mode) = request.requested_mode
        && req_mode != op.execution_mode
      {
        self.logger.log(
          DecisionCategory::Scheduling,
          DecisionAction::Escalated,
          ReasonCode::ModeMismatchEscalateVlm,
          task_id,
          Some(op.name.clone()),
          format!(
            "Mode mismatch for '{}': requested {:?} mode, but operation requires {:?} mode. Rejecting incompatible execution and escalating to VLM (zero side-effects).",
            canonical_key, req_mode, op.execution_mode
          ),
          serde_json::json!({
            "op_mode": op.execution_mode.as_str(),
            "requested_mode": req_mode.as_str(),
            "zero_side_effects": true
          }),
        );

        return SchedulingOutcome {
          selected_operation: None,
          embedding_called: false,
          reason_code: ReasonCode::ModeMismatchEscalateVlm,
          message: format!(
            "Mode mismatch: operation '{}' requires {:?} mode, cannot satisfy requested {:?} mode (zero side effects)",
            canonical_key, op.execution_mode, req_mode
          ),
        };
      }

      // Evaluate strict preconditions
      let pre_check = evaluate_preconditions(&op.preconditions, &request.current_context);
      if pre_check.passed {
        self.logger.log(
          DecisionCategory::Scheduling,
          DecisionAction::ExactHit,
          ReasonCode::ExactKeyMatch,
          task_id,
          Some(op.name.clone()),
          format!("Exact key match hit for '{}' with 0 embedding calls. Preconditions verified.", canonical_key),
          serde_json::json!({ "embedding_called": false, "key": canonical_key }),
        );

        return SchedulingOutcome {
          selected_operation: Some(op.clone()),
          embedding_called: false,
          reason_code: ReasonCode::ExactKeyMatch,
          message: format!("Exact key match hit for '{}'", canonical_key),
        };
      } else {
        self.logger.log(
          DecisionCategory::Scheduling,
          DecisionAction::Intercepted,
          ReasonCode::PreconditionMismatch,
          task_id,
          Some(op.name.clone()),
          format!("Exact key candidate '{}' failed precondition: {}", op.name, pre_check.failure_reason),
          serde_json::json!({ "failure": pre_check.failure_reason }),
        );
      }
    }

    // Step 2: Unmatched -> Embedding Candidate Top-3
    // Note: This path is only invoked when exact key misses!
    let embedding_called = true;
    let candidates = rank_embedding_candidates(request, catalog);

    if candidates.is_empty() {
      self.logger.log(
        DecisionCategory::Scheduling,
        DecisionAction::Escalated,
        ReasonCode::SchedulerMissEscalateVlm,
        task_id,
        None,
        "No matching operation in catalog; escalating to VLM slow loop.",
        serde_json::json!({ "embedding_called": true, "candidates_count": 0 }),
      );

      return SchedulingOutcome {
        selected_operation: None,
        embedding_called,
        reason_code: ReasonCode::SchedulerMissEscalateVlm,
        message: "No candidates found; escalating to VLM slow loop.".to_string(),
      };
    }

    self.logger.log(
      DecisionCategory::Scheduling,
      DecisionAction::EmbeddingHit,
      ReasonCode::EmbeddingTop3Candidate,
      task_id,
      None,
      format!("Found {} embedding candidate(s) for long-tail query.", candidates.len()),
      serde_json::json!({ "embedding_called": true, "candidate_names": candidates.iter().map(|c| &c.name).collect::<Vec<_>>() }),
    );

    // Step 3: Candidate -> Strict Preconditions Check
    let mut had_mode_mismatch = false;
    let mut had_mode_compatible_candidate = false;
    for candidate in candidates {
      if let Some(req_mode) = request.requested_mode
        && req_mode != candidate.execution_mode
      {
        self.logger.log(
          DecisionCategory::Scheduling,
          DecisionAction::Intercepted,
          ReasonCode::ModeMismatchEscalateVlm,
          task_id,
          Some(candidate.name.clone()),
          format!(
            "Candidate '{}' mode ({:?}) incompatible with requested mode ({:?}); intercepted.",
            candidate.name, candidate.execution_mode, req_mode
          ),
          serde_json::json!({
            "op": candidate.name,
            "op_mode": candidate.execution_mode.as_str(),
            "requested_mode": req_mode.as_str(),
          }),
        );
        had_mode_mismatch = true;
        continue;
      }
      had_mode_compatible_candidate = true;
      let pre_check = evaluate_preconditions(&candidate.preconditions, &request.current_context);
      if pre_check.passed {
        self.logger.log(
          DecisionCategory::Scheduling,
          DecisionAction::Approved,
          ReasonCode::PreconditionPassed,
          task_id,
          Some(candidate.name.clone()),
          format!("Candidate '{}' passed all strict preconditions.", candidate.name),
          serde_json::json!({ "op": candidate.name }),
        );

        return SchedulingOutcome {
          selected_operation: Some(candidate),
          embedding_called,
          reason_code: ReasonCode::PreconditionPassed,
          message: "Candidate passed strict preconditions.".to_string(),
        };
      } else {
        self.logger.log(
          DecisionCategory::Scheduling,
          DecisionAction::Intercepted,
          ReasonCode::PreconditionMismatch,
          task_id,
          Some(candidate.name.clone()),
          format!("Candidate '{}' intercepted by strict precondition: {}", candidate.name, pre_check.failure_reason),
          serde_json::json!({ "failure": pre_check.failure_reason }),
        );
      }
    }

    if had_mode_mismatch && !had_mode_compatible_candidate {
      self.logger.log(
        DecisionCategory::Scheduling,
        DecisionAction::Escalated,
        ReasonCode::ModeMismatchEscalateVlm,
        task_id,
        None,
        "All candidates intercepted due to execution mode mismatch; escalating to VLM.",
        serde_json::json!({ "embedding_called": true }),
      );

      return SchedulingOutcome {
        selected_operation: None,
        embedding_called,
        reason_code: ReasonCode::ModeMismatchEscalateVlm,
        message: "All candidates intercepted due to execution mode mismatch; escalating to VLM.".to_string(),
      };
    }

    // All candidates failed preconditions
    self.logger.log(
      DecisionCategory::Scheduling,
      DecisionAction::Escalated,
      ReasonCode::SchedulerMissEscalateVlm,
      task_id,
      None,
      "All candidates intercepted by preconditions; escalating to VLM.",
      serde_json::json!({ "embedding_called": true }),
    );

    SchedulingOutcome {
      selected_operation: None,
      embedding_called,
      reason_code: ReasonCode::SchedulerMissEscalateVlm,
      message: "All candidates intercepted by strict preconditions; escalated to VLM.".to_string(),
    }
  }

  /// Schedules an operation for an explicit exact operation name request.
  ///
  /// Bypasses canonical key synthesis and embedding-based fallback:
  /// - Queries `catalog.is_isolated(operation_name)` fail-closed (returns `EscalateToVlm`).
  /// - Queries `catalog.get_active(operation_name)` directly.
  /// - If not found in catalog, fails closed with `SchedulerMissEscalateVlm` (zero embedding calls).
  /// - Enforces mandatory `requested_mode` declaration: if `None`, rejects with `RejectModeUndeclared`.
  /// - Enforces execution mode compatibility (returns `ModeMismatchEscalateVlm`).
  /// - Evaluates strict preconditions (returns `ExactKeyMatch` if passed, `PreconditionMismatch` if failed).
  pub fn schedule_exact(&self, operation_name: &str, request: &TaskRequest, catalog: &OperationCatalog) -> SchedulingOutcome {
    let task_id = &request.task_name;

    // Check if operation is explicitly isolated
    if catalog.is_isolated(operation_name) {
      self.logger.log(
        DecisionCategory::Scheduling,
        DecisionAction::Escalated,
        ReasonCode::EscalateToVlm,
        task_id,
        Some(operation_name.to_string()),
        format!("Operation '{}' is isolated. Bypassing fast loop and routing to VLM slow loop.", operation_name),
        serde_json::json!({ "isolated": true }),
      );

      return SchedulingOutcome {
        selected_operation: None,
        embedding_called: false,
        reason_code: ReasonCode::EscalateToVlm,
        message: format!("Operation '{}' is isolated; routed directly to VLM.", operation_name),
      };
    }

    // Exact name lookup in active catalog
    let Some(op) = catalog.get_active(operation_name) else {
      self.logger.log(
        DecisionCategory::Scheduling,
        DecisionAction::Escalated,
        ReasonCode::SchedulerMissEscalateVlm,
        task_id,
        Some(operation_name.to_string()),
        format!("Operation '{}' not found in active catalog; escalating to VLM slow loop.", operation_name),
        serde_json::json!({ "embedding_called": false, "exact_lookup_miss": true }),
      );

      return SchedulingOutcome {
        selected_operation: None,
        embedding_called: false,
        reason_code: ReasonCode::SchedulerMissEscalateVlm,
        message: format!("Operation '{}' not found in active catalog.", operation_name),
      };
    };

    // Explicit execution requires a declared execution mode (fail-closed)
    let Some(req_mode) = request.requested_mode else {
      self.logger.log(
        DecisionCategory::Scheduling,
        DecisionAction::Intercepted,
        ReasonCode::RejectModeUndeclared,
        task_id,
        Some(op.name.clone()),
        format!("Operation '{}' requested without explicit execution_mode (fail-closed).", operation_name),
        serde_json::json!({ "mode_undeclared": true }),
      );

      return SchedulingOutcome {
        selected_operation: None,
        embedding_called: false,
        reason_code: ReasonCode::RejectModeUndeclared,
        message: format!("Execution mode must be explicitly declared for exact operation '{}' (fail-closed)", operation_name),
      };
    };

    // Check execution mode compatibility
    if req_mode != op.execution_mode {
      self.logger.log(
        DecisionCategory::Scheduling,
        DecisionAction::Escalated,
        ReasonCode::ModeMismatchEscalateVlm,
        task_id,
        Some(op.name.clone()),
        format!(
          "Mode mismatch for '{}': requested {:?} mode, but operation requires {:?} mode. Rejecting incompatible execution and escalating to VLM (zero side-effects).",
          operation_name, req_mode, op.execution_mode
        ),
        serde_json::json!({
          "op_mode": op.execution_mode.as_str(),
          "requested_mode": req_mode.as_str(),
          "zero_side_effects": true
        }),
      );

      return SchedulingOutcome {
        selected_operation: None,
        embedding_called: false,
        reason_code: ReasonCode::ModeMismatchEscalateVlm,
        message: format!(
          "Mode mismatch: operation '{}' requires {:?} mode, cannot satisfy requested {:?} mode (zero side effects)",
          operation_name, op.execution_mode, req_mode
        ),
      };
    }

    // Evaluate strict preconditions
    let pre_check = evaluate_preconditions(&op.preconditions, &request.current_context);
    if pre_check.passed {
      self.logger.log(
        DecisionCategory::Scheduling,
        DecisionAction::ExactHit,
        ReasonCode::ExactKeyMatch,
        task_id,
        Some(op.name.clone()),
        format!("Exact key match hit for '{}' with 0 embedding calls. Preconditions verified.", operation_name),
        serde_json::json!({ "embedding_called": false, "key": operation_name }),
      );

      SchedulingOutcome {
        selected_operation: Some(op.clone()),
        embedding_called: false,
        reason_code: ReasonCode::ExactKeyMatch,
        message: format!("Exact key match hit for '{}'", operation_name),
      }
    } else {
      self.logger.log(
        DecisionCategory::Scheduling,
        DecisionAction::Intercepted,
        ReasonCode::PreconditionMismatch,
        task_id,
        Some(op.name.clone()),
        format!("Exact key candidate '{}' failed precondition: {}", op.name, pre_check.failure_reason),
        serde_json::json!({ "failure": pre_check.failure_reason }),
      );

      SchedulingOutcome {
        selected_operation: None,
        embedding_called: false,
        reason_code: ReasonCode::PreconditionMismatch,
        message: format!("Exact operation '{}' failed precondition: {}", op.name, pre_check.failure_reason),
      }
    }
  }
}

pub fn format_canonical_key(app_name: &str, task_name: &str) -> String {
  let clean_app = app_name.to_lowercase().replace(".exe", "").replace(' ', "_");
  let clean_task = task_name.to_lowercase().replace(' ', "_");
  format!("{}.{}", clean_app, clean_task)
}

struct PreconditionResult {
  passed: bool,
  failure_reason: String,
}

fn evaluate_preconditions(preconditions: &[PreconditionDef], context: &HashMap<String, serde_json::Value>) -> PreconditionResult {
  for pre in preconditions {
    match context.get(&pre.key) {
      Some(val) => {
        if val != &pre.expected_value {
          return PreconditionResult {
            passed: false,
            failure_reason: format!("key '{}': expected {:?}, got {:?}", pre.key, pre.expected_value, val),
          };
        }
      }
      None => {
        return PreconditionResult {
          passed: false,
          failure_reason: format!("missing required context key '{}'", pre.key),
        };
      }
    }
  }
  PreconditionResult {
    passed: true,
    failure_reason: String::new(),
  }
}

fn rank_embedding_candidates(request: &TaskRequest, catalog: &OperationCatalog) -> Vec<OperationDef> {
  let mut scored: Vec<(f32, OperationDef)> = Vec::new();

  for (op_name, op) in catalog.active_operations() {
    let score = compute_similarity(request, op_name, op);
    if score > 0.3 {
      scored.push((score, op.clone()));
    }
  }

  scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
  scored.into_iter().take(3).map(|(_, op)| op).collect()
}

fn compute_similarity(request: &TaskRequest, _op_name: &str, op: &OperationDef) -> f32 {
  // If embedding vectors provided, compute cosine similarity
  if let Some(ref req_vec) = request.embedding_vector {
    // Simulated operation embedding derived from description hash or tokens
    let op_vec = generate_simulated_embedding(&op.description);
    return cosine_similarity(req_vec, &op_vec);
  }

  // Fallback lexical token overlap
  let req_tokens: Vec<&str> = request.instruction.split_whitespace().collect();
  let op_tokens: Vec<&str> = op.description.split_whitespace().collect();
  let mut matches = 0;
  for t in &req_tokens {
    if op_tokens.contains(t) {
      matches += 1;
    }
  }
  if req_tokens.is_empty() {
    0.0
  } else {
    matches as f32 / req_tokens.len() as f32
  }
}

pub fn generate_simulated_embedding(text: &str) -> Vec<f32> {
  // Generate deterministic 4-dimensional embedding vector
  let mut vec = vec![0.0f32; 4];
  for (i, b) in text.bytes().enumerate() {
    vec[i % 4] += (b as f32) / 255.0;
  }
  let norm: f32 = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
  if norm > 0.0 {
    for v in &mut vec {
      *v /= norm;
    }
  }
  vec
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
  if a.len() != b.len() || a.is_empty() {
    return 0.0;
  }
  let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
  let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
  let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
  if norm_a == 0.0 || norm_b == 0.0 {
    0.0
  } else {
    dot / (norm_a * norm_b)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::models::{
    CompilationMetadata, ManualReviewItem, OPERATION_SCHEMA_VERSION, OperationStepDef, TargetMetadata, VerificationGateDef,
  };

  fn test_op(name: &str, mode: ExecutionMode) -> OperationDef {
    OperationDef {
      schema_version: OPERATION_SCHEMA_VERSION.to_string(),
      name: name.to_string(),
      description: "Test operation".to_string(),
      execution_mode: mode,
      compilation_metadata: CompilationMetadata {
        compiler: "test".to_string(),
        source_record: "none".to_string(),
        date: "2026-11-01".to_string(),
        crux_goal: "test".to_string(),
      },
      target: TargetMetadata {
        app_name: "Example.exe".to_string(),
        backend: "test".to_string(),
      },
      preconditions: vec![],
      parameters: vec![],
      steps: vec![OperationStepDef {
        id: "step_1".to_string(),
        name: "Noop".to_string(),
        description: "Noop".to_string(),
        action: serde_json::json!({ "type": "noop" }),
        verification_gate: VerificationGateDef::CustomAssertion {
          expression: "true".to_string(),
          timeout_ms: 100,
          escalate_on_mismatch: "none".to_string(),
        },
        is_unverified: false,
      }],
      tags: vec![],
    }
  }

  #[test]
  fn test_schedule_exact_success_exact_hit() {
    let logger = DecisionLogger::new();
    let scheduler = FastLoopScheduler::new(&logger);
    let mut catalog = OperationCatalog::new();
    let op = test_op("custom.prepare", ExecutionMode::Verified);
    catalog.register_active(op).unwrap();

    let req = TaskRequest {
      app_name: "Example.exe".to_string(),
      task_name: "prepare".to_string(),
      instruction: "Run custom.prepare".to_string(),
      requested_mode: Some(ExecutionMode::Verified),
      current_context: HashMap::new(),
      embedding_vector: None,
    };

    let outcome = scheduler.schedule_exact("custom.prepare", &req, &catalog);
    assert!(outcome.selected_operation.is_some());
    assert_eq!(outcome.selected_operation.unwrap().name, "custom.prepare");
    assert!(!outcome.embedding_called);
    assert_eq!(outcome.reason_code, ReasonCode::ExactKeyMatch);
  }

  #[test]
  fn test_schedule_exact_isolated_operation_escalates_to_vlm() {
    let logger = DecisionLogger::new();
    let scheduler = FastLoopScheduler::new(&logger);
    let mut catalog = OperationCatalog::new();
    let op = test_op("custom.prepare", ExecutionMode::Verified);
    catalog.register_active(op).unwrap();

    // Manually isolate it
    catalog.isolate(
      "custom.prepare",
      ManualReviewItem {
        id: "item_1".to_string(),
        task_name: "custom.prepare".to_string(),
        reason_code: ReasonCode::AutoIsolatedConsecutiveFailures,
        reason_description: "Failed twice".to_string(),
        execution_mode: Some(ExecutionMode::Verified),
        source_trajectory: None,
        isolated_operation: None,
        created_at: "2026-11-01".to_string(),
      },
    );

    let req = TaskRequest {
      app_name: "Example.exe".to_string(),
      task_name: "prepare".to_string(),
      instruction: "Run custom.prepare".to_string(),
      requested_mode: Some(ExecutionMode::Verified),
      current_context: HashMap::new(),
      embedding_vector: None,
    };

    let outcome = scheduler.schedule_exact("custom.prepare", &req, &catalog);
    assert!(outcome.selected_operation.is_none());
    assert!(!outcome.embedding_called);
    assert_eq!(outcome.reason_code, ReasonCode::EscalateToVlm);
  }

  #[test]
  fn test_schedule_exact_missing_requested_mode_fails_closed() {
    let logger = DecisionLogger::new();
    let scheduler = FastLoopScheduler::new(&logger);
    let mut catalog = OperationCatalog::new();
    let op = test_op("custom.prepare", ExecutionMode::Verified);
    catalog.register_active(op).unwrap();

    let req = TaskRequest {
      app_name: "Example.exe".to_string(),
      task_name: "prepare".to_string(),
      instruction: "Run custom.prepare".to_string(),
      requested_mode: None, // Missing!
      current_context: HashMap::new(),
      embedding_vector: None,
    };

    let outcome = scheduler.schedule_exact("custom.prepare", &req, &catalog);
    assert!(outcome.selected_operation.is_none());
    assert!(!outcome.embedding_called);
    assert_eq!(outcome.reason_code, ReasonCode::RejectModeUndeclared);
  }

  #[test]
  fn test_schedule_exact_not_found_fails_closed_zero_embedding_fallback() {
    let logger = DecisionLogger::new();
    let scheduler = FastLoopScheduler::new(&logger);
    let mut catalog = OperationCatalog::new();
    // Catalog has "other.op", but caller requests "custom.prepare"
    let op = test_op("other.op", ExecutionMode::Verified);
    catalog.register_active(op).unwrap();

    let req = TaskRequest {
      app_name: "Example.exe".to_string(),
      task_name: "prepare".to_string(),
      instruction: "Run custom.prepare".to_string(),
      requested_mode: Some(ExecutionMode::Verified),
      current_context: HashMap::new(),
      embedding_vector: None,
    };

    let outcome = scheduler.schedule_exact("custom.prepare", &req, &catalog);
    assert!(outcome.selected_operation.is_none());
    assert!(!outcome.embedding_called, "Exact lookup must NEVER fall back to embedding");
    assert_eq!(outcome.reason_code, ReasonCode::SchedulerMissEscalateVlm);
  }
}
