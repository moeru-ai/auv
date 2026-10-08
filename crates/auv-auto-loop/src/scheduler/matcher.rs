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
      // Check execution mode compatibility
      if request.requested_mode == Some(ExecutionMode::Fast) && op.execution_mode == ExecutionMode::Verified {
        self.logger.log(
          DecisionCategory::Scheduling,
          DecisionAction::Escalated,
          ReasonCode::ModeMismatchEscalateVlm,
          task_id,
          Some(op.name.clone()),
          format!(
            "Mode mismatch for '{}': requested Fast mode, but operation requires Verified mode. Rejecting silent downgrade and escalating to VLM (zero side-effects).",
            canonical_key
          ),
          serde_json::json!({
            "op_mode": op.execution_mode.as_str(),
            "requested_mode": "fast",
            "zero_side_effects": true
          }),
        );

        return SchedulingOutcome {
          selected_operation: None,
          embedding_called: false,
          reason_code: ReasonCode::ModeMismatchEscalateVlm,
          message: format!(
            "Mode mismatch: operation '{}' requires Verified mode, cannot run in Fast mode (zero side effects)",
            canonical_key
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
    for candidate in candidates {
      if request.requested_mode == Some(ExecutionMode::Fast) && candidate.execution_mode == ExecutionMode::Verified {
        self.logger.log(
          DecisionCategory::Scheduling,
          DecisionAction::Intercepted,
          ReasonCode::ModeMismatchEscalateVlm,
          task_id,
          Some(candidate.name.clone()),
          format!("Candidate '{}' requires Verified mode, but request asked for Fast mode; intercepted.", candidate.name),
          serde_json::json!({ "op": candidate.name, "op_mode": candidate.execution_mode.as_str() }),
        );
        continue;
      }
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
