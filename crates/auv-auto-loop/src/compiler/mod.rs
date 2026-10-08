//! Automatic compilation pipeline for AUV trajectories.
//!
//! Enforces:
//! - Deterministic cleaning (forward diff + backward slicing, zero LLM-as-judge).
//! - Parameter lifting via anti-unification.
//! - Template-based gate derivation.
//! - Three mandatory compilation gates (Cleaning, Parameter, Blast-Radius).
//! - Zero silent errors (every pass/fail logs a structured ReasonCode).

pub mod anti_unify;
pub mod cleaner;
pub mod compile_gate;
pub mod gate_derive;

use crate::decision_log::DecisionLogger;
use crate::models::{
  CompilationMetadata, DecisionAction, DecisionCategory, ExecutionMode, ManualReviewItem, OPERATION_SCHEMA_VERSION, OperationDef,
  PreconditionDef, ReasonCode, TargetMetadata, TrajectoryRecord,
};
use chrono::Utc;

pub struct AutoCompiler<'a> {
  logger: &'a DecisionLogger,
}

impl<'a> AutoCompiler<'a> {
  pub fn new(logger: &'a DecisionLogger) -> Self {
    Self { logger }
  }

  /// Compiles a trajectory record into an executable operation specification,
  /// automatically deriving the execution mode from the step verification gates.
  pub fn compile(
    &self,
    primary_record: &TrajectoryRecord,
    isomorphic_records: Option<&[TrajectoryRecord]>,
  ) -> Result<OperationDef, ManualReviewItem> {
    self.compile_with_mode(primary_record, isomorphic_records, None)
  }

  /// Compiles a trajectory record with an explicit or automatically derived execution mode.
  /// If target_mode is specified, the compilation gate validates that the trajectory's
  /// actions and gates are compatible with that mode (e.g., unverified-step cannot be Fast).
  pub fn compile_with_mode(
    &self,
    primary_record: &TrajectoryRecord,
    isomorphic_records: Option<&[TrajectoryRecord]>,
    target_mode: Option<ExecutionMode>,
  ) -> Result<OperationDef, ManualReviewItem> {
    let task_name = &primary_record.metadata.task;

    // Step 1: Trajectory Cleaning (State diff + backward slicing)
    let cleaning = cleaner::clean_trajectory(primary_record);
    let cleaning_eval = compile_gate::evaluate_cleaning_gate(&cleaning);

    if !cleaning_eval.passed {
      self.logger.log(
        DecisionCategory::Compilation,
        DecisionAction::Rejected,
        cleaning_eval.reason_code,
        task_name,
        None,
        &cleaning_eval.message,
        serde_json::json!({ "dropped_steps": cleaning.dropped_steps }),
      );

      return Err(ManualReviewItem {
        id: format!("review_{}", Utc::now().timestamp_millis()),
        task_name: task_name.clone(),
        reason_code: cleaning_eval.reason_code,
        reason_description: cleaning_eval.message,
        execution_mode: target_mode,
        source_trajectory: Some(Box::new(primary_record.clone())),
        isolated_operation: None,
        created_at: Utc::now().to_rfc3339(),
      });
    }

    self.logger.log(
      DecisionCategory::Compilation,
      DecisionAction::Approved,
      cleaning_eval.reason_code,
      task_name,
      None,
      &cleaning_eval.message,
      serde_json::json!({ "kept_count": cleaning.kept_steps.len() }),
    );

    // Step 2: Parameter Lifting & Anti-Unification
    let records_to_unify = match isomorphic_records {
      Some(others) if !others.is_empty() => {
        let mut list = vec![primary_record.clone()];
        list.extend_from_slice(others);
        list
      }
      _ => vec![primary_record.clone()],
    };

    let param_lifting = anti_unify::anti_unify_trajectories(&records_to_unify);
    let param_eval = compile_gate::evaluate_parameter_gate(&param_lifting);

    if !param_eval.passed {
      self.logger.log(
        DecisionCategory::Compilation,
        DecisionAction::Rejected,
        param_eval.reason_code,
        task_name,
        None,
        &param_eval.message,
        serde_json::json!({ "forbidden": param_lifting.forbidden_parameter_detected }),
      );

      return Err(ManualReviewItem {
        id: format!("review_{}", Utc::now().timestamp_millis()),
        task_name: task_name.clone(),
        reason_code: param_eval.reason_code,
        reason_description: param_eval.message,
        execution_mode: target_mode,
        source_trajectory: Some(Box::new(primary_record.clone())),
        isolated_operation: None,
        created_at: Utc::now().to_rfc3339(),
      });
    }

    self.logger.log(
      DecisionCategory::Compilation,
      DecisionAction::Approved,
      param_eval.reason_code,
      task_name,
      None,
      &param_eval.message,
      serde_json::json!({ "parameters": param_lifting.parameters }),
    );

    // Step 3: Template-based Gate Derivation
    let app_id = "QQMusic.exe";
    let mut operation_steps = Vec::new();
    let mut has_unverified = false;

    for step in &param_lifting.parameterized_steps {
      let derived_step = gate_derive::derive_step_gate(step, app_id);
      if derived_step.is_unverified {
        has_unverified = true;
      }
      operation_steps.push(derived_step);
    }

    // Step 4: Blast-Radius Gate
    let blast_eval = compile_gate::evaluate_blast_radius_gate(&operation_steps);
    if !blast_eval.passed {
      self.logger.log(
        DecisionCategory::Compilation,
        DecisionAction::Rejected,
        blast_eval.reason_code,
        task_name,
        None,
        &blast_eval.message,
        serde_json::json!({ "steps_count": operation_steps.len() }),
      );

      return Err(ManualReviewItem {
        id: format!("review_{}", Utc::now().timestamp_millis()),
        task_name: task_name.clone(),
        reason_code: blast_eval.reason_code,
        reason_description: blast_eval.message,
        execution_mode: target_mode,
        source_trajectory: Some(Box::new(primary_record.clone())),
        isolated_operation: None,
        created_at: Utc::now().to_rfc3339(),
      });
    }

    self.logger.log(
      DecisionCategory::Compilation,
      DecisionAction::Approved,
      blast_eval.reason_code,
      task_name,
      None,
      &blast_eval.message,
      serde_json::json!({ "approved_steps": operation_steps.len() }),
    );

    // Step 5: Mode Derivation & Execution Mode Gate
    let mut tags = Vec::new();
    if has_unverified {
      tags.push("unverified-step".to_string());
    }

    let mode = target_mode.unwrap_or_else(|| gate_derive::derive_mode_from_steps(&operation_steps));
    let mode_eval = compile_gate::evaluate_execution_mode_gate(mode, &operation_steps, &tags);
    if !mode_eval.passed {
      self.logger.log(
        DecisionCategory::Compilation,
        DecisionAction::Rejected,
        mode_eval.reason_code,
        task_name,
        None,
        &mode_eval.message,
        serde_json::json!({ "mode": mode.as_str(), "steps_count": operation_steps.len() }),
      );

      return Err(ManualReviewItem {
        id: format!("review_{}", Utc::now().timestamp_millis()),
        task_name: task_name.clone(),
        reason_code: mode_eval.reason_code,
        reason_description: mode_eval.message,
        execution_mode: Some(mode),
        source_trajectory: Some(Box::new(primary_record.clone())),
        isolated_operation: None,
        created_at: Utc::now().to_rfc3339(),
      });
    }

    // Step 6: Final Operation Assembly
    let operation_name = if task_name.contains("播放") || task_name.contains("playback") {
      "qqmusic.prepare_playback".to_string()
    } else {
      format!("qqmusic.{}", task_name.to_lowercase().replace(' ', "_"))
    };

    let op_def = OperationDef {
      schema_version: OPERATION_SCHEMA_VERSION.to_string(),
      name: operation_name.clone(),
      description: primary_record.metadata.instruction.clone(),
      execution_mode: mode,
      compilation_metadata: CompilationMetadata {
        compiler: "auto-loop-v0.1-compiler".to_string(),
        source_record: "vlm_trajectory_record".to_string(),
        date: Utc::now().to_rfc3339(),
        crux_goal: "repeated execution approaches zero reasoning-token cost".to_string(),
      },
      target: TargetMetadata {
        app_name: app_id.to_string(),
        backend: "windows.smtc+coreaudio+wgc".to_string(),
      },
      preconditions: vec![PreconditionDef {
        key: "App.ProcessName".to_string(),
        expected_value: serde_json::json!("QQMusic.exe"),
      }],
      parameters: param_lifting.parameters,
      steps: operation_steps,
      tags,
    };

    self.logger.log(
      DecisionCategory::Compilation,
      DecisionAction::Approved,
      ReasonCode::CompilationApproved,
      task_name,
      Some(op_def.name.clone()),
      "All compilation gates passed. Operation successfully compiled for fast-loop deployment.",
      serde_json::json!({ "step_count": op_def.steps.len(), "execution_mode": op_def.execution_mode.as_str(), "tags": op_def.tags }),
    );

    Ok(op_def)
  }
}
