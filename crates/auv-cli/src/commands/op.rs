//! Command-line frontend for running compiled mode-aware operations (`auv op run`).
//!
//! Provides:
//! - Exact-key fast loop scheduling with bidirectional mode compatibility enforcement.
//! - Built-in operation catalog registration for pre-compiled operations.
//! - Admitting custom operation definitions from file via `catalog.admit_operation_json`.
//! - Execution dispatch to Windows production driver or fake test executor.
//! - Structured JSON or human-readable execution output reporting.

use std::collections::HashMap;
use std::path::PathBuf;

use auv_auto_loop::decision_log::DecisionLogger;
use auv_auto_loop::models::{ExecutionMode, ReasonCode};
use auv_auto_loop::runtime::{ExecutionResult, OperationExecutor};
use auv_auto_loop::scheduler::{FastLoopScheduler, OperationCatalog, TaskRequest};
use clap::{Args, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};

#[cfg(not(target_os = "windows"))]
use auv_auto_loop::runtime::FakeOperationExecutor;
#[cfg(target_os = "windows")]
use auv_driver_windows::WindowsProductionExecutor;

const BUILTIN_QQMUSIC_PREPARED_PLAYBACK: &str = include_str!("../../../../docs/ai/references/driver/qqmusic-prepared-playback.json");
const BUILTIN_QQMUSIC_PREPARED_PLAYBACK_FAST: &str =
  include_str!("../../../../docs/ai/references/driver/qqmusic-prepared-playback-fast.json");

#[derive(Clone, Debug, Args)]
pub struct OpArgs {
  #[command(subcommand)]
  pub command: OpCommand,
}

#[derive(Clone, Debug, Subcommand)]
pub enum OpCommand {
  /// Run a compiled mode-aware operation
  Run(OpRunArgs),
}

#[derive(Clone, Debug, Args)]
pub struct OpRunArgs {
  /// Operation name (e.g. qqmusic.prepare_playback)
  #[arg(long)]
  pub operation: String,

  /// Execution mode (fast or verified; required)
  #[arg(long, value_enum)]
  pub mode: ExecutionModeArg,

  /// Optional path to custom compiled operation JSON file
  #[arg(long)]
  pub file: Option<PathBuf>,

  /// Output JSON format
  #[arg(long)]
  pub json: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionModeArg {
  Fast,
  Verified,
}

impl ExecutionModeArg {
  pub fn as_str(&self) -> &'static str {
    match self {
      Self::Fast => "fast",
      Self::Verified => "verified",
    }
  }
}

impl From<ExecutionModeArg> for ExecutionMode {
  fn from(arg: ExecutionModeArg) -> Self {
    match arg {
      ExecutionModeArg::Fast => ExecutionMode::Fast,
      ExecutionModeArg::Verified => ExecutionMode::Verified,
    }
  }
}

impl From<ExecutionMode> for ExecutionModeArg {
  fn from(mode: ExecutionMode) -> Self {
    match mode {
      ExecutionMode::Fast => ExecutionModeArg::Fast,
      ExecutionMode::Verified => ExecutionModeArg::Verified,
    }
  }
}

impl From<OpRunArgs> for OpArgs {
  fn from(args: OpRunArgs) -> Self {
    Self {
      command: OpCommand::Run(args),
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpRunOutput {
  pub status: String,
  pub execution_mode: String,
  pub confirmed: bool,
  pub steps_executed: usize,
  pub reason_code: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub step_id: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub message: Option<String>,
}

/// Root CLI entrypoint for `auv op`.
pub async fn run(args: impl Into<OpArgs>) -> Result<i32, String> {
  let args = args.into();
  match args.command {
    OpCommand::Run(run_args) => run_op(run_args),
  }
}

/// Executes a compiled operation with platform-appropriate executor.
pub fn run_op(args: OpRunArgs) -> Result<i32, String> {
  #[cfg(target_os = "windows")]
  {
    let executor = WindowsProductionExecutor::default();
    run_with_executor(args, &executor)
  }
  #[cfg(not(target_os = "windows"))]
  {
    let executor = FakeOperationExecutor::new();
    run_with_executor(args, &executor)
  }
}

/// Executes an operation against a given `OperationExecutor`.
pub fn run_with_executor(args: OpRunArgs, executor: &impl OperationExecutor) -> Result<i32, String> {
  let (exit_code, output) = run_with_executor_inner(&args, executor)?;
  if args.json {
    println!("{}", serde_json::to_string_pretty(&output).unwrap_or_default());
  } else if exit_code == 0 {
    println!("Operation '{}' succeeded:", args.operation);
    println!("  Status:         {}", output.status.to_uppercase());
    println!("  Execution Mode: {}", output.execution_mode);
    println!("  Confirmed:      {}", output.confirmed);
    println!("  Steps Executed: {}", output.steps_executed);
    println!("  Reason Code:    {}", output.reason_code);
  } else {
    eprintln!("Operation '{}' failed:", args.operation);
    eprintln!("  Status:         {}", output.status.to_uppercase());
    eprintln!("  Execution Mode: {}", output.execution_mode);
    eprintln!("  Confirmed:      {}", output.confirmed);
    if let Some(ref step) = output.step_id {
      eprintln!("  Step:           {}", step);
    }
    if let Some(ref msg) = output.message {
      eprintln!("  Message:        {}", msg);
    }
    eprintln!("  Reason Code:    {}", output.reason_code);
  }
  Ok(exit_code)
}

/// Inner execution logic returning exit code and structured output.
pub fn run_with_executor_inner(args: &OpRunArgs, executor: &impl OperationExecutor) -> Result<(i32, OpRunOutput), String> {
  let logger = DecisionLogger::new();
  let mut catalog = OperationCatalog::new();

  // 1. Register built-in operations
  load_builtin_operations(&mut catalog)?;

  // 2. If args.file is provided, read and call catalog.admit_operation_json
  if let Some(ref file_path) = args.file {
    let content =
      std::fs::read_to_string(file_path).map_err(|e| format!("Failed to read operation file '{}': {}", file_path.display(), e))?;
    catalog
      .admit_operation_json(&content, &file_path.display().to_string())
      .map_err(|e| format!("Failed to admit operation from '{}': [{}]: {}", file_path.display(), e.reason_code.as_str(), e.message))?;
  }

  // 3. Build TaskRequest with target app and context
  let (app_name, task_name) = parse_operation_target(&args.operation, &catalog);
  let mut current_context = HashMap::new();
  current_context.insert("App.ProcessName".to_string(), serde_json::json!(app_name));

  let req = TaskRequest {
    app_name,
    task_name,
    instruction: format!("Run operation {}", args.operation),
    requested_mode: Some(args.mode.into()),
    current_context,
    embedding_vector: None,
  };

  // 4. Fast loop scheduler routing
  let scheduler = FastLoopScheduler::new(&logger);
  let outcome = scheduler.schedule(&req, &catalog);

  let Some(op) = outcome.selected_operation else {
    let out = OpRunOutput {
      status: "failed".to_string(),
      execution_mode: args.mode.as_str().to_string(),
      confirmed: false,
      steps_executed: 0,
      reason_code: outcome.reason_code.as_str().to_string(),
      step_id: None,
      message: Some(outcome.message),
    };
    return Ok((1, out));
  };

  // 5. Dispatch to executor
  let exec_result = executor.execute(&op, &req);

  // 6. Format result
  match exec_result {
    ExecutionResult::Success {
      steps_executed,
      execution_mode,
      confirmed,
    } => {
      let reason_code = match execution_mode {
        ExecutionMode::Fast => ReasonCode::ExactKeyMatch,
        ExecutionMode::Verified => ReasonCode::GatePassed,
      };
      let out = OpRunOutput {
        status: "success".to_string(),
        execution_mode: execution_mode.as_str().to_string(),
        confirmed,
        steps_executed,
        reason_code: reason_code.as_str().to_string(),
        step_id: None,
        message: None,
      };
      Ok((0, out))
    }
    ExecutionResult::GateFailed {
      step_id,
      message,
      consecutive_failures: _,
    } => {
      let out = OpRunOutput {
        status: "failed".to_string(),
        execution_mode: args.mode.as_str().to_string(),
        confirmed: false,
        steps_executed: 0,
        reason_code: ReasonCode::GateFailed.as_str().to_string(),
        step_id: Some(step_id),
        message: Some(message),
      };
      Ok((1, out))
    }
    ExecutionResult::EscalatedToVlm {
      step_id,
      reason,
      isolated,
    } => {
      let out = OpRunOutput {
        status: "escalated".to_string(),
        execution_mode: args.mode.as_str().to_string(),
        confirmed: false,
        steps_executed: 0,
        reason_code: ReasonCode::EscalateToVlm.as_str().to_string(),
        step_id: Some(step_id),
        message: Some(format!("{} (isolated: {})", reason, isolated)),
      };
      Ok((1, out))
    }
    ExecutionResult::ModeConflictError {
      operation_name: _,
      message,
    } => {
      let out = OpRunOutput {
        status: "failed".to_string(),
        execution_mode: args.mode.as_str().to_string(),
        confirmed: false,
        steps_executed: 0,
        reason_code: ReasonCode::ModeMismatchEscalateVlm.as_str().to_string(),
        step_id: None,
        message: Some(message),
      };
      Ok((1, out))
    }
  }
}

/// Registers the built-in operations into the catalog fail-closed.
pub fn load_builtin_operations(catalog: &mut OperationCatalog) -> Result<(), String> {
  // 1. qqmusic.prepare_playback (verified)
  catalog
    .admit_operation_json(BUILTIN_QQMUSIC_PREPARED_PLAYBACK, "builtin:qqmusic.prepare_playback")
    .map_err(|e| format!("Failed to admit builtin qqmusic.prepare_playback: {}", e.message))?;

  // 2. qqmusic.prepare_playback_fast (fast)
  catalog
    .admit_operation_json(BUILTIN_QQMUSIC_PREPARED_PLAYBACK_FAST, "builtin:qqmusic.prepare_playback_fast")
    .map_err(|e| format!("Failed to admit builtin qqmusic.prepare_playback_fast: {}", e.message))?;

  Ok(())
}

/// Parses an operation name like `qqmusic.prepare_playback` into `(app_name, task_name)`.
pub fn parse_operation_target(operation: &str, catalog: &OperationCatalog) -> (String, String) {
  if let Some(op) = catalog.get_active(operation) {
    let task = if let Some((_, task_part)) = operation.split_once('.') {
      task_part.to_string()
    } else {
      operation.to_string()
    };
    return (op.target.app_name.clone(), task);
  }

  if let Some((app_part, task_part)) = operation.split_once('.') {
    let app_name = if app_part.eq_ignore_ascii_case("qqmusic") {
      "QQMusic.exe".to_string()
    } else if app_part.ends_with(".exe") {
      app_part.to_string()
    } else {
      format!("{}.exe", app_part)
    };
    (app_name, task_part.to_string())
  } else {
    ("QQMusic.exe".to_string(), operation.to_string())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use auv_auto_loop::runtime::FakeOperationExecutor;
  use clap::Parser;

  #[derive(Parser, Debug)]
  struct TestCli {
    #[command(subcommand)]
    command: TestRootCommand,
  }

  #[derive(Subcommand, Debug)]
  enum TestRootCommand {
    Op(OpArgs),
  }

  #[test]
  fn test_op_cli_parsing_fast_mode() {
    let parsed = TestCli::try_parse_from([
      "auv",
      "op",
      "run",
      "--operation",
      "qqmusic.prepare_playback_fast",
      "--mode",
      "fast",
    ])
    .expect("parsing valid op run command must succeed");

    match parsed.command {
      TestRootCommand::Op(op_args) => match op_args.command {
        OpCommand::Run(run_args) => {
          assert_eq!(run_args.operation, "qqmusic.prepare_playback_fast");
          assert_eq!(run_args.mode, ExecutionModeArg::Fast);
          assert_eq!(run_args.file, None);
          assert!(!run_args.json);
        }
      },
    }
  }

  #[test]
  fn test_op_cli_parsing_verified_mode_with_json_and_file() {
    let parsed = TestCli::try_parse_from([
      "auv",
      "op",
      "run",
      "--operation",
      "qqmusic.prepare_playback",
      "--mode",
      "verified",
      "--file",
      "custom_op.json",
      "--json",
    ])
    .expect("parsing verified mode with json must succeed");

    match parsed.command {
      TestRootCommand::Op(op_args) => match op_args.command {
        OpCommand::Run(run_args) => {
          assert_eq!(run_args.operation, "qqmusic.prepare_playback");
          assert_eq!(run_args.mode, ExecutionModeArg::Verified);
          assert_eq!(run_args.file, Some(PathBuf::from("custom_op.json")));
          assert!(run_args.json);
        }
      },
    }
  }

  #[test]
  fn test_op_cli_parsing_missing_mode_fails_closed() {
    let err = TestCli::try_parse_from([
      "auv",
      "op",
      "run",
      "--operation",
      "qqmusic.prepare_playback",
    ])
    .expect_err("missing --mode must fail-closed at CLI parse boundary");

    assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
  }

  #[test]
  fn test_builtin_operations_catalog_admission() {
    let mut catalog = OperationCatalog::new();
    load_builtin_operations(&mut catalog).expect("built-in operations must admit without error");

    let verified_op = catalog.get_active("qqmusic.prepare_playback").expect("qqmusic.prepare_playback must be present");
    assert_eq!(verified_op.execution_mode, ExecutionMode::Verified);
    assert_eq!(verified_op.target.app_name, "QQMusic.exe");

    let fast_op = catalog.get_active("qqmusic.prepare_playback_fast").expect("qqmusic.prepare_playback_fast must be present");
    assert_eq!(fast_op.execution_mode, ExecutionMode::Fast);
    assert_eq!(fast_op.target.app_name, "QQMusic.exe");
  }

  #[test]
  fn test_run_fast_mode_dispatch_success_unconfirmed() {
    let fake_executor = FakeOperationExecutor::new();
    let args = OpRunArgs {
      operation: "qqmusic.prepare_playback_fast".to_string(),
      mode: ExecutionModeArg::Fast,
      file: None,
      json: true,
    };

    let (exit_code, output) = run_with_executor_inner(&args, &fake_executor).expect("execution must succeed");
    assert_eq!(exit_code, 0);
    assert_eq!(output.status, "success");
    assert_eq!(output.execution_mode, "fast");
    assert_eq!(output.reason_code, "EXACT_KEY_MATCH");
    assert!(!output.confirmed, "Fast mode must NEVER be confirmed (invariant)");
    assert_eq!(fake_executor.calls_count(), 1);
  }

  #[test]
  fn test_run_verified_mode_dispatch_success_confirmed() {
    let fake_executor = FakeOperationExecutor::new();
    let args = OpRunArgs {
      operation: "qqmusic.prepare_playback".to_string(),
      mode: ExecutionModeArg::Verified,
      file: None,
      json: true,
    };

    let (exit_code, output) = run_with_executor_inner(&args, &fake_executor).expect("execution must succeed");
    assert_eq!(exit_code, 0);
    assert_eq!(output.status, "success");
    assert_eq!(output.execution_mode, "verified");
    assert_eq!(output.reason_code, "GATE_PASSED");
    assert!(output.confirmed, "Verified mode must be confirmed on gate pass");
    assert_eq!(fake_executor.calls_count(), 1);
  }

  #[test]
  fn test_run_mode_mismatch_intercepted_zero_driver_calls() {
    let fake_executor = FakeOperationExecutor::new();
    // qqmusic.prepare_playback requires Verified, but Fast requested
    let args = OpRunArgs {
      operation: "qqmusic.prepare_playback".to_string(),
      mode: ExecutionModeArg::Fast,
      file: None,
      json: true,
    };

    let (exit_code, output) = run_with_executor_inner(&args, &fake_executor).expect("scheduler interception must complete");
    assert_eq!(exit_code, 1);
    assert_eq!(output.status, "failed");
    assert_eq!(output.reason_code, "MODE_MISMATCH_ESCALATE_VLM");
    assert_eq!(fake_executor.calls_count(), 0, "Zero driver calls on mode conflict");
  }

  #[test]
  fn test_run_verified_gate_failure() {
    let fake_executor = FakeOperationExecutor::new();
    fake_executor.set_verified_should_pass(false);

    let args = OpRunArgs {
      operation: "qqmusic.prepare_playback".to_string(),
      mode: ExecutionModeArg::Verified,
      file: None,
      json: false,
    };

    let (exit_code, output) = run_with_executor_inner(&args, &fake_executor).expect("gate failure handled gracefully");
    assert_eq!(exit_code, 1);
    assert_eq!(output.status, "failed");
    assert_eq!(output.reason_code, "GATE_FAILED");
    assert!(!output.confirmed);
    assert_eq!(fake_executor.calls_count(), 1);
  }
}
