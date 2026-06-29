//! Operation summary source seam.
//!
//! Names "how an operation's summary view is obtained" as one owned boundary so
//! a future session API server reads the summary fields through this seam
//! instead of ad-hoc field access in RPC handlers.
//!
//! Per the API-P3 two-source finding, the summary view here covers only the
//! fields that originate on the runtime return value (`InvokeResult`):
//! `status`, `output_summary`, `signals`, and `failure_message`. The
//! `OperationResult`-sourced fields (`operation_id`, `known_limits`) are a
//! deliberately separate source (see the deferral marker below).

use std::collections::BTreeMap;

use crate::{InvokeResult, RunStatus};

/// Read access to an operation's summary view: the `InvokeResult`-sourced half
/// of the API-P3 two-source `GetOperation` projection.
///
/// Kept object-safe so a later backing (an in-memory cache keyed by `run_id`, a
/// persisted projection, or a composed read path) can be served behind the same
/// seam without changing callers.
pub trait OperationSummarySource {
  /// Execution status of the operation (not the verification verdict).
  fn status(&self) -> RunStatus;

  /// Human-facing summary line produced by the command handler.
  fn output_summary(&self) -> &str;

  /// Structured signals emitted by the command handler.
  fn signals(&self) -> &BTreeMap<String, String>;

  /// Failure message when the operation failed, otherwise `None`.
  fn failure_message(&self) -> Option<&str>;
}

// TODO(api-session-server): the `OperationResult`-sourced half of the
// `GetOperation` projection (`operation_id`, `known_limits`) is intentionally
// NOT modeled by this seam yet. Adding it would pull in the persisted-record
// read path and a two-source join, expanding past this skeleton slice and
// re-opening API-P3 open decision 1. Trigger: an owner-named session API server
// slice that joins both sources. See
// docs/ai/references/2026-06-30-auv-api-p4-session-proto-server-seam-design.md
// (summary-source seam, section C).

impl OperationSummarySource for InvokeResult {
  fn status(&self) -> RunStatus {
    self.status.clone()
  }

  fn output_summary(&self) -> &str {
    &self.output_summary
  }

  fn signals(&self) -> &BTreeMap<String, String> {
    &self.signals
  }

  fn failure_message(&self) -> Option<&str> {
    self.failure_message.as_deref()
  }
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;

  use auv_tracing_driver::SpanId;

  use crate::{InvokeResult, OperationSummarySource, RunStatus};

  #[test]
  fn invoke_result_summary_source_exposes_runtime_summary_fields() {
    let mut signals = BTreeMap::new();
    signals.insert("fixture".to_string(), "observed".to_string());
    let result = InvokeResult {
      run_id: "run-summary-completed".to_string(),
      producer_span_id: SpanId::new("0000000000000001"),
      status: RunStatus::Completed,
      output_summary: "fixture observed".to_string(),
      signals,
      artifacts: Vec::new(),
      artifact_paths: Vec::new(),
      failure_message: None,
    };

    assert_eq!(result.status(), RunStatus::Completed);
    assert_eq!(result.output_summary(), "fixture observed");
    assert_eq!(
      result.signals().get("fixture").map(String::as_str),
      Some("observed")
    );
    assert_eq!(result.failure_message(), None);
  }

  #[test]
  fn invoke_result_summary_source_exposes_failure_message() {
    let result = InvokeResult {
      run_id: "run-summary-failed".to_string(),
      producer_span_id: SpanId::new("0000000000000002"),
      status: RunStatus::Failed,
      output_summary: "failed summary".to_string(),
      signals: BTreeMap::new(),
      artifacts: Vec::new(),
      artifact_paths: Vec::new(),
      failure_message: Some("boom".to_string()),
    };

    assert_eq!(result.status(), RunStatus::Failed);
    assert_eq!(result.failure_message(), Some("boom"));
  }
}
