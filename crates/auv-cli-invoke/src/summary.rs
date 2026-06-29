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

use std::collections::{BTreeMap, HashMap};

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
// (summary-source seam, section C). API-P7 owns the join in the session API
// module, not this invoke-domain crate.

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

/// Owned snapshot of the `InvokeResult`-sourced summary fields, captured so the
/// summary view outlives the transient `InvokeResult`.
///
/// API-P3 open decision 1: `output_summary` / `signals` / `failure_message`
/// live only on the runtime return value and are not persisted in
/// `OperationResult`. To answer a later `GetOperation` read, the server seam
/// must keep this state addressable after invoke returns. This snapshot is that
/// retained projection — distinct from `InvokeResult` itself, which also owns
/// non-summary fields (`producer_span_id`, `artifacts`, `artifact_paths`) that
/// the summary view must not depend on.
#[derive(Clone, Debug, PartialEq)]
pub struct OperationSummary {
  run_id: String,
  status: RunStatus,
  output_summary: String,
  signals: BTreeMap<String, String>,
  failure_message: Option<String>,
}

impl OperationSummary {
  /// Capture the summary view from an invoke result, cloning only the
  /// `InvokeResult`-sourced summary fields.
  pub fn capture(result: &InvokeResult) -> Self {
    Self {
      run_id: result.run_id.clone(),
      status: result.status.clone(),
      output_summary: result.output_summary.clone(),
      signals: result.signals.clone(),
      failure_message: result.failure_message.clone(),
    }
  }

  /// Run id this summary was captured for.
  pub fn run_id(&self) -> &str {
    &self.run_id
  }
}

impl OperationSummarySource for OperationSummary {
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

/// In-memory operation summary cache keyed by `run_id`.
///
/// One concrete backing for the API-P4 summary-source seam (responsibility C,
/// "an in-memory summary cache keyed by `run_id`"). The session API server
/// records a summary here right after invoke so a later `GetOperation` can read
/// the `InvokeResult`-sourced fields that the persisted `OperationResult` does
/// not carry (API-P3 open decision 1).
///
/// This cache deliberately owns no persistence and no eviction policy: it is a
/// process-local projection store. Durable projection and eviction are a later
/// owner-named decision (see API-P4 responsibility C, which lists in-memory
/// cache, persisted projection, and composed read path as alternatives).
#[derive(Debug, Default)]
pub struct OperationSummaryCache {
  entries: HashMap<String, OperationSummary>,
}

impl OperationSummaryCache {
  /// Create an empty cache.
  pub fn new() -> Self {
    Self::default()
  }

  /// Record (insert or replace) a captured summary, keyed by its `run_id`.
  pub fn record(&mut self, summary: OperationSummary) {
    self.entries.insert(summary.run_id.clone(), summary);
  }

  /// Capture and record the summary for an invoke result in one step.
  pub fn record_result(&mut self, result: &InvokeResult) {
    self.record(OperationSummary::capture(result));
  }

  /// Read the cached summary for a run, if present.
  pub fn get(&self, run_id: &str) -> Option<&OperationSummary> {
    self.entries.get(run_id)
  }

  /// Number of cached summaries.
  pub fn len(&self) -> usize {
    self.entries.len()
  }

  /// Whether the cache holds no summaries.
  pub fn is_empty(&self) -> bool {
    self.entries.is_empty()
  }
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;

  use auv_tracing_driver::SpanId;

  use crate::summary::{OperationSummary, OperationSummaryCache};
  use crate::{InvokeResult, OperationSummarySource, RunStatus};

  fn completed_result(run_id: &str) -> InvokeResult {
    let mut signals = BTreeMap::new();
    signals.insert("fixture".to_string(), "observed".to_string());
    InvokeResult {
      run_id: run_id.to_string(),
      producer_span_id: SpanId::new("0000000000000001"),
      status: RunStatus::Completed,
      output_summary: "fixture observed".to_string(),
      signals,
      artifacts: Vec::new(),
      artifact_paths: Vec::new(),
      failure_message: None,
    }
  }

  #[test]
  fn invoke_result_summary_source_exposes_runtime_summary_fields() {
    let result = completed_result("run-summary-completed");

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

  #[test]
  fn operation_summary_captures_invoke_result_summary_fields() {
    let result = completed_result("run-capture");
    let summary = OperationSummary::capture(&result);

    assert_eq!(summary.run_id(), "run-capture");
    assert_eq!(summary.status(), RunStatus::Completed);
    assert_eq!(summary.output_summary(), "fixture observed");
    assert_eq!(
      summary.signals().get("fixture").map(String::as_str),
      Some("observed")
    );
    assert_eq!(summary.failure_message(), None);
  }

  #[test]
  fn summary_cache_records_and_reads_back_by_run_id() {
    let mut cache = OperationSummaryCache::new();
    assert!(cache.is_empty());

    cache.record_result(&completed_result("run-cached"));

    assert_eq!(cache.len(), 1);
    let cached = cache.get("run-cached").expect("summary should be cached");
    assert_eq!(cached.status(), RunStatus::Completed);
    assert_eq!(cached.output_summary(), "fixture observed");
  }

  #[test]
  fn summary_cache_returns_none_for_unknown_run() {
    let cache = OperationSummaryCache::new();
    assert!(cache.get("missing").is_none());
  }

  #[test]
  fn summary_cache_record_replaces_existing_run_entry() {
    let mut cache = OperationSummaryCache::new();
    cache.record_result(&completed_result("run-dup"));

    let mut updated = completed_result("run-dup");
    updated.output_summary = "second observation".to_string();
    cache.record_result(&updated);

    assert_eq!(cache.len(), 1);
    assert_eq!(
      cache
        .get("run-dup")
        .map(OperationSummarySource::output_summary),
      Some("second observation")
    );
  }
}
