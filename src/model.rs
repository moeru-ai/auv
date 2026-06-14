// File: src/model.rs
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::trace::{ArtifactRecordV1Alpha1, SpanId};

pub type AuvResult<T> = Result<T, String>;

static RUN_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

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

#[derive(Clone, Debug)]
pub struct DriverDescriptor {
  pub id: &'static str,
  pub summary: &'static str,
  pub capabilities: &'static [&'static str],
  pub donor_boundary: &'static str,
}

/// Control-plane metadata the runtime injects into every `DriverCall` so drivers can
/// build evidence `ArtifactRef`s and emit `OperationResult`s tied to the active run/span
/// without smuggling identifiers through the user-facing `inputs` map.
///
/// `device_id` / `session_id` identify the automation target and namespace. Today
/// they default to `"local"` / `"default"` since AUV runs only on the local
/// macOS host with one implicit session — drivers can ignore them. The fields
/// exist so future RPC/JS-SDK frontends can route to remote/VM devices and
/// scope state per session without changing the driver contract again.
#[derive(Clone, Debug)]
pub struct DriverRunContext {
  pub run_id: String,
  pub span_id: String,
  pub device_id: String,
  pub session_id: String,
}

impl Default for DriverRunContext {
  fn default() -> Self {
    Self {
      run_id: String::new(),
      span_id: String::new(),
      device_id: "local".to_string(),
      session_id: "default".to_string(),
    }
  }
}

#[derive(Clone, Debug)]
pub struct DriverCall {
  pub operation: String,
  pub target: ExecutionTarget,
  pub inputs: BTreeMap<String, String>,
  pub working_directory: PathBuf,
  pub run_context: DriverRunContext,
}

#[derive(Clone, Debug)]
pub struct ProducedArtifact {
  pub kind: String,
  pub source_path: PathBuf,
  pub preferred_name: String,
  pub note: Option<String>,
}

#[derive(Clone, Debug)]
pub struct DriverResponse {
  pub summary: String,
  pub backend: Option<String>,
  pub signals: BTreeMap<String, String>,
  pub notes: Vec<String>,
  pub artifacts: Vec<ProducedArtifact>,
}

pub fn now_millis() -> u64 {
  u64::try_from(
    SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .unwrap_or_default()
      .as_millis(),
  )
  .unwrap_or(u64::MAX)
}

pub fn new_run_id() -> String {
  let sequence = RUN_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
  format!("run_{}_{}_{}", now_millis(), process::id(), sequence)
}

#[cfg(test)]
mod tests {
  use super::new_run_id;

  #[test]
  fn new_run_id_is_unique_within_process() {
    let first = new_run_id();
    let second = new_run_id();

    assert_ne!(first, second);
  }
}
