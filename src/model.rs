use std::collections::BTreeMap;
use std::fmt::{self, Display, Formatter};
use std::path::PathBuf;
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub type AuvResult<T> = Result<T, String>;

static RUN_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub struct CommandSpec {
  pub id: &'static str,
  pub summary: &'static str,
  pub driver_id: &'static str,
  pub operation: &'static str,
}

#[derive(Clone, Debug, Default)]
pub struct ExecutionTarget {
  pub application_id: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct InvokeRequest {
  pub command_id: String,
  pub target: ExecutionTarget,
  pub inputs: BTreeMap<String, String>,
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
  pub status: RunStatus,
  pub output_summary: String,
  pub artifact_paths: Vec<PathBuf>,
  pub failure_message: Option<String>,
}

#[derive(Clone, Debug)]
pub struct EventRecord {
  pub at_millis: u128,
  pub kind: String,
  pub message: String,
}

#[derive(Clone, Debug)]
pub struct ArtifactRecord {
  pub id: String,
  pub kind: String,
  pub path: PathBuf,
  pub note: Option<String>,
}

#[derive(Clone, Debug)]
pub struct RunRecord {
  pub run_id: String,
  pub command_id: String,
  pub driver_id: String,
  pub operation: String,
  pub target_application_id: Option<String>,
  pub runtime_version: String,
  pub started_at_millis: u128,
  pub finished_at_millis: Option<u128>,
  pub status: RunStatus,
  pub inputs: BTreeMap<String, String>,
  pub output_summary: String,
  pub events: Vec<EventRecord>,
  pub artifacts: Vec<ArtifactRecord>,
}

impl EventRecord {
  pub fn render_log_line(&self) -> String {
    format!("{} {} {}", self.at_millis, self.kind, self.message)
  }
}

impl ArtifactRecord {
  pub fn render_manifest_line(&self) -> String {
    let note = self.note.clone().unwrap_or_else(|| "n/a".to_string());
    format!(
      "{} kind={} path={} note={}",
      self.id,
      self.kind,
      self.path.display(),
      note
    )
  }
}

impl RunRecord {
  pub fn render_meta(&self) -> String {
    let target = self
      .target_application_id
      .clone()
      .unwrap_or_else(|| "n/a".to_string());
    let finished = self
      .finished_at_millis
      .map(|value| value.to_string())
      .unwrap_or_else(|| "n/a".to_string());

    [
      format!("runId: {}", self.run_id),
      format!("status: {}", self.status.as_str()),
      format!("command: {}", self.command_id),
      format!("driver: {}", self.driver_id),
      format!("operation: {}", self.operation),
      format!("targetApplicationId: {target}"),
      format!("runtimeVersion: {}", self.runtime_version),
      format!("startedAtMillis: {}", self.started_at_millis),
      format!("finishedAtMillis: {finished}"),
    ]
    .join("\n")
      + "\n"
  }

  pub fn render_inputs(&self) -> String {
    if self.inputs.is_empty() {
      return "none\n".to_string();
    }

    let mut lines = Vec::new();
    for (key, value) in &self.inputs {
      lines.push(format!("{key}={value}"));
    }
    lines.join("\n") + "\n"
  }

  pub fn render_events(&self) -> String {
    if self.events.is_empty() {
      return "none\n".to_string();
    }

    self
      .events
      .iter()
      .map(EventRecord::render_log_line)
      .collect::<Vec<_>>()
      .join("\n")
      + "\n"
  }

  pub fn render_artifacts(&self) -> String {
    if self.artifacts.is_empty() {
      return "none\n".to_string();
    }

    self
      .artifacts
      .iter()
      .map(ArtifactRecord::render_manifest_line)
      .collect::<Vec<_>>()
      .join("\n")
      + "\n"
  }

  pub fn render_inspection(&self) -> String {
    let target = self
      .target_application_id
      .clone()
      .unwrap_or_else(|| "n/a".to_string());
    let finished = self
      .finished_at_millis
      .map(|value| value.to_string())
      .unwrap_or_else(|| "n/a".to_string());
    let sections = vec![
      format!("Run {}", self.run_id),
      format!("Status: {}", self.status.as_str()),
      format!("Command: {}", self.command_id),
      format!("Driver: {}", self.driver_id),
      format!("Operation: {}", self.operation),
      format!("Target: {target}"),
      format!("Runtime Version: {}", self.runtime_version),
      format!("Started At (ms): {}", self.started_at_millis),
      format!("Finished At (ms): {finished}"),
      String::new(),
      "Inputs".to_string(),
      render_block(&self.render_inputs()),
      "Output".to_string(),
      render_block(&format!("{}\n", self.output_summary)),
      "Artifacts".to_string(),
      render_block(&self.render_artifacts()),
      "Events".to_string(),
      render_block(&self.render_events()),
    ];

    sections.join("\n")
  }
}

impl Display for RunRecord {
  fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
    formatter.write_str(&self.render_inspection())
  }
}

#[derive(Clone, Debug)]
pub struct DriverDescriptor {
  pub id: &'static str,
  pub summary: &'static str,
  pub capabilities: &'static [&'static str],
  pub donor_boundary: &'static str,
}

#[derive(Clone, Debug)]
pub struct DriverCall {
  pub operation: String,
  pub target: ExecutionTarget,
  pub inputs: BTreeMap<String, String>,
  pub working_directory: PathBuf,
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
  pub notes: Vec<String>,
  pub artifacts: Vec<ProducedArtifact>,
}

pub fn now_millis() -> u128 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap_or_default()
    .as_millis()
}

pub fn new_run_id() -> String {
  let sequence = RUN_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
  format!("run_{}_{}_{}", now_millis(), process::id(), sequence)
}

fn render_block(raw: &str) -> String {
  raw
    .lines()
    .map(|line| format!("  {line}"))
    .collect::<Vec<_>>()
    .join("\n")
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
