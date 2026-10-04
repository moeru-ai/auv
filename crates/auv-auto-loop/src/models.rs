//! Core domain models for AUV auto-loop, compiler, scheduler, and runtime.

use serde::{Deserialize, Serialize};

/// Reason codes for all automatic decisions across compilation, scheduling, and runtime.
/// Enforces the invariant: ZERO silent errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReasonCode {
  // --- Compilation Gates ---
  /// Cleaning gate passed: zero ambiguous drops detected.
  CleaningZeroAmbiguity,
  /// Cleaning gate failed: trajectory contains state mutations not on slicing path or ambiguous steps.
  RejectAmbiguousDrops,
  /// Parameter gate passed: parameterization derived from isomorphic trajectories or typed whitelist.
  ParameterGateApproved,
  /// Parameter gate failed: illegal parameterization proposed (e.g. Next() or unvalidated parameter).
  RejectForbiddenParameterization,
  /// Blast-radius gate passed: all actions belong to human-maintained whitelist.
  BlastRadiusApproved,
  /// Blast-radius gate failed: operation contains non-whitelisted/destructive actions (delete, send, sys-config).
  RejectBlastRadiusViolation,
  /// Auto compilation gate fully passed: all 3 sub-gates satisfied, approved for activation.
  CompilationApproved,

  // --- Scheduler ---
  /// Exact operation key / target fingerprint matched directly (zero embedding call).
  ExactKeyMatch,
  /// Embedding fallback selected top-3 candidates for long-tail task.
  EmbeddingTop3Candidate,
  /// Strict preconditions satisfied for matched candidate.
  PreconditionPassed,
  /// Strict preconditions failed: intercepted false candidate execution.
  PreconditionMismatch,
  /// Scheduler miss: task not in catalog or candidates failed preconditions, escalate to VLM.
  SchedulerMissEscalateVlm,

  // --- Runtime ---
  /// Verification gate passed successfully.
  GatePassed,
  /// Verification gate failed: observable state does not match expectation.
  GateFailed,
  /// Step marked unverified-step failed in strict mode: instant escalation & isolation.
  StrictStepFailed,
  /// Operation gate failed consecutively >= 2 times: automatically isolated.
  AutoIsolatedConsecutiveFailures,
  /// Escalated to slow loop VLM execution planner.
  EscalateToVlm,
}

impl ReasonCode {
  pub fn as_str(&self) -> &'static str {
    match self {
      Self::CleaningZeroAmbiguity => "CLEANING_ZERO_AMBIGUITY",
      Self::RejectAmbiguousDrops => "REJECT_AMBIGUOUS_DROPS",
      Self::ParameterGateApproved => "PARAMETER_GATE_APPROVED",
      Self::RejectForbiddenParameterization => "REJECT_FORBIDDEN_PARAMETERIZATION",
      Self::BlastRadiusApproved => "BLAST_RADIUS_APPROVED",
      Self::RejectBlastRadiusViolation => "REJECT_BLAST_RADIUS_VIOLATION",
      Self::CompilationApproved => "COMPILATION_APPROVED",
      Self::ExactKeyMatch => "EXACT_KEY_MATCH",
      Self::EmbeddingTop3Candidate => "EMBEDDING_TOP3_CANDIDATE",
      Self::PreconditionPassed => "PRECONDITION_PASSED",
      Self::PreconditionMismatch => "PRECONDITION_MISMATCH",
      Self::SchedulerMissEscalateVlm => "SCHEDULER_MISS_ESCALATE_VLM",
      Self::GatePassed => "GATE_PASSED",
      Self::GateFailed => "GATE_FAILED",
      Self::StrictStepFailed => "STRICT_STEP_FAILED",
      Self::AutoIsolatedConsecutiveFailures => "AUTO_ISOLATED_CONSECUTIVE_FAILURES",
      Self::EscalateToVlm => "ESCALATE_TO_VLM",
    }
  }
}

/// A structured decision log entry documenting every automated choice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionLog {
  pub timestamp: String,
  pub category: DecisionCategory,
  pub decision: DecisionAction,
  pub reason_code: ReasonCode,
  pub operation_id: Option<String>,
  pub task_name: String,
  pub message: String,
  #[serde(default)]
  pub details: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionCategory {
  Compilation,
  Scheduling,
  Runtime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionAction {
  Approved,
  Rejected,
  ExactHit,
  EmbeddingHit,
  Intercepted,
  Executed,
  GatePass,
  GateFail,
  Isolated,
  Escalated,
}

// ==============================================================================
// VLM Trajectory Models (Slow Loop input)
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryRecord {
  pub metadata: TrajectoryMetadata,
  pub structured_trajectory: Vec<TrajectoryStep>,
  #[serde(default)]
  pub raw_transcript_steps_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryMetadata {
  pub task: String,
  pub instruction: String,
  #[serde(default)]
  pub subagent_conversation_id: Option<String>,
  #[serde(default)]
  pub agent_role: Option<String>,
  #[serde(default)]
  pub model: Option<String>,
  pub date: String,
  #[serde(default)]
  pub tokens: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryStep {
  pub step: usize,
  pub intent: String,
  pub action: String,
  pub perception: String,
  pub result: serde_json::Value,
  #[serde(default)]
  pub pre_state: Option<serde_json::Value>,
  #[serde(default)]
  pub post_state: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DroppedStep {
  pub step: usize,
  pub intent: String,
  pub reason: String,
  pub is_ambiguous: bool,
}

// ==============================================================================
// Operation Specification Models (Compiled Artifact)
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OperationDef {
  pub schema_version: String,
  pub name: String,
  pub description: String,
  pub compilation_metadata: CompilationMetadata,
  pub target: TargetMetadata,
  #[serde(default)]
  pub preconditions: Vec<PreconditionDef>,
  #[serde(default)]
  pub parameters: Vec<ParameterDef>,
  pub steps: Vec<OperationStepDef>,
  #[serde(default)]
  pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompilationMetadata {
  pub compiler: String,
  pub source_record: String,
  pub date: String,
  pub crux_goal: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TargetMetadata {
  pub app_name: String,
  pub backend: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PreconditionDef {
  pub key: String,
  pub expected_value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ParameterDef {
  pub name: String,
  pub param_type: String, // e.g. "float", "string", "int"
  #[serde(default)]
  pub default_value: Option<serde_json::Value>,
  #[serde(default)]
  pub min_value: Option<f64>,
  #[serde(default)]
  pub max_value: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OperationStepDef {
  pub id: String,
  pub name: String,
  pub description: String,
  pub action: serde_json::Value,
  pub verification_gate: VerificationGateDef,
  #[serde(default)]
  pub is_unverified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum VerificationGateDef {
  #[serde(rename = "smtc_session_present")]
  SmtcSessionPresent {
    timeout_ms: u64,
    escalate_on_mismatch: String,
  },
  #[serde(rename = "status_and_volume_gate")]
  StatusAndVolumeGate {
    expected_status: String,
    expected_volume: f32,
    volume_tolerance: f32,
    timeout_ms: u64,
    escalate_on_mismatch: String,
  },
  #[serde(rename = "title_change_gate")]
  TitleChangeGate {
    require_title_change: bool,
    timeout_ms: u64,
    escalate_on_mismatch: String,
  },
  #[serde(rename = "wgc_alive_gate")]
  WgcAliveGate {
    min_non_black_ratio: f64,
    timeout_ms: u64,
    escalate_on_mismatch: String,
  },
  #[serde(rename = "custom_assertion")]
  CustomAssertion {
    expression: String,
    timeout_ms: u64,
    escalate_on_mismatch: String,
  },
  #[serde(rename = "unverified_fallback")]
  UnverifiedFallback {
    description: String,
    timeout_ms: u64,
    escalate_on_mismatch: String,
  },
}

// ==============================================================================
// Manual Review Queue Items
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualReviewItem {
  pub id: String,
  pub task_name: String,
  pub reason_code: ReasonCode,
  pub reason_description: String,
  pub source_trajectory: Option<Box<TrajectoryRecord>>,
  pub isolated_operation: Option<Box<OperationDef>>,
  pub created_at: String,
}
