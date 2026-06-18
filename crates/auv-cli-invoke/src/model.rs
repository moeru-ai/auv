use std::collections::BTreeMap;
use std::path::PathBuf;

use auv_tracing_driver::trace::{ArtifactRecordV1Alpha1, SpanId};

#[derive(Clone, Debug, Default)]
pub struct ExecutionTarget {
  pub application_id: Option<String>,
  pub target_label: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct InvokeRequest {
  pub command_id: String,
  pub target: ExecutionTarget,
  pub inputs: BTreeMap<String, String>,
  pub dry_run: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunStatus {
  Completed,
  Failed,
}

impl RunStatus {
  pub fn as_str(&self) -> &'static str {
    match self {
      Self::Completed => "completed",
      Self::Failed => "failed",
    }
  }
}

#[derive(Clone, Debug)]
pub struct InvokeResult {
  pub run_id: String,
  pub producer_span_id: SpanId,
  pub status: RunStatus,
  pub output_summary: String,
  pub signals: BTreeMap<String, String>,
  pub artifacts: Vec<ArtifactRecordV1Alpha1>,
  pub artifact_paths: Vec<PathBuf>,
  pub failure_message: Option<String>,
}
