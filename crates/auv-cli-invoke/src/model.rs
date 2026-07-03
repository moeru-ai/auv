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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvokeOutputOptions {
  pub json: bool,
  pub detail: bool,
}

impl Default for InvokeOutputOptions {
  fn default() -> Self {
    Self {
      json: false,
      detail: false,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InvokeReport {
  pub fields: Vec<InvokeReportField>,
  pub sections: Vec<InvokeReportSection>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InvokeReportField {
  pub label: String,
  pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InvokeReportSection {
  pub title: String,
  pub fields: Vec<InvokeReportField>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
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
  pub command_id: String,
  pub command_summary: String,
  pub status: RunStatus,
  pub output_summary: String,
  pub backend: Option<String>,
  pub signals: BTreeMap<String, String>,
  pub notes: Vec<String>,
  pub known_limits: Vec<String>,
  pub verification: Option<String>,
  pub report: Option<InvokeReport>,
  pub artifacts: Vec<ArtifactRecordV1Alpha1>,
  pub artifact_paths: Vec<PathBuf>,
  pub failure_message: Option<String>,
}
