//! Temporary neutral adapters for readers of the retired tracing-driver records.
//!
//! TODO(run-contract-tasks-17-23): Remove this module after Tasks 17-23 migrate
//! the remaining producers and text composers to canonical `auv-tracing` data.
//! No new caller should depend on these types.

use std::fs;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::Arc;

use auv_tracing_driver::store::{CanonicalRun, LocalStore};
use auv_tracing_driver::trace::{ArtifactId, ArtifactRecordV1Alpha1, EventId, RunId, SpanId};
use serde::de::DeserializeOwned;
use serde_json::Value;
use thiserror::Error;

/// Shared role used by legacy line-oriented telemetry readers.
pub const TELEMETRY_SAMPLE_ARTIFACT_ROLE: &str = "telemetry-sample";

/// Artifact reference projected from a legacy tracing-driver record.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ArtifactRefView {
  pub run_id: RunId,
  pub artifact_id: ArtifactId,
  pub span_id: SpanId,
  pub captured_event_id: Option<EventId>,
  pub role: Option<String>,
  pub path: Option<String>,
  pub summary: Option<String>,
  pub resolved: bool,
}

/// Reports whether a legacy artifact MIME type carries JSON.
pub fn is_json_mime(mime_type: &str) -> bool {
  mime_type == "application/json" || mime_type.ends_with("+json")
}

/// Projects a legacy artifact row for existing read-side consumers.
pub fn artifact_record_view(run_id: RunId, artifact: &ArtifactRecordV1Alpha1) -> ArtifactRefView {
  ArtifactRefView {
    run_id,
    artifact_id: artifact.artifact_id.clone(),
    span_id: artifact.span_id.clone(),
    captured_event_id: artifact.event_id.clone(),
    role: Some(artifact.role.clone()),
    path: Some(artifact.path.clone()),
    summary: artifact.summary.clone(),
    resolved: true,
  }
}

/// Opens one legacy artifact beneath its scoped local store path.
pub fn open_artifact_file(
  store: &LocalStore,
  run_id: &str,
  artifact: &ArtifactRecordV1Alpha1,
  artifact_role: &str,
) -> Result<(fs::File, PathBuf), String> {
  let (_, artifact_path) = store.artifact_file_scoped(run_id, artifact.artifact_id.as_str(), Some(artifact.span_id.as_str()))?;
  let file = fs::File::open(&artifact_path).map_err(|error| {
    format!("failed to open {artifact_role} artifact {} for run {run_id} from {}: {error}", artifact.artifact_id, artifact_path.display())
  })?;
  Ok((file, artifact_path))
}

/// Parses a legacy JSON artifact into its caller-owned type.
pub fn read_artifact_json<T: DeserializeOwned>(
  store: &LocalStore,
  run_id: &str,
  artifact: &ArtifactRecordV1Alpha1,
  artifact_role: &str,
) -> Result<T, String> {
  let (file, artifact_path) = open_artifact_file(store, run_id, artifact, artifact_role)?;
  serde_json::from_reader(BufReader::new(file)).map_err(|error| {
    format!("failed to parse {artifact_role} artifact {} for run {run_id} from {}: {error}", artifact.artifact_id, artifact_path.display())
  })
}

/// Summarizes one legacy line-oriented telemetry artifact.
pub fn read_telemetry_artifact_summary(
  store: &LocalStore,
  run_id: &str,
  artifact: &ArtifactRecordV1Alpha1,
  artifact_role: &str,
) -> Result<(PathBuf, usize, u64), String> {
  use std::io::BufRead;

  let (_, artifact_path) = store.artifact_file_scoped(run_id, artifact.artifact_id.as_str(), Some(artifact.span_id.as_str()))?;
  let metadata = fs::metadata(&artifact_path).map_err(|error| {
    format!("failed to stat {artifact_role} artifact {} for run {run_id} from {}: {error}", artifact.artifact_id, artifact_path.display())
  })?;
  let (file, _) = open_artifact_file(store, run_id, artifact, artifact_role)?;
  let line_count = BufReader::new(file).lines().try_fold(0usize, |count, line| {
    let line = line.map_err(|error| {
      format!("failed to read {artifact_role} artifact {} for run {run_id} from {}: {error}", artifact.artifact_id, artifact_path.display())
    })?;
    Ok::<_, String>(count + usize::from(!line.trim().is_empty()))
  })?;
  Ok((artifact_path, line_count, metadata.len()))
}

/// Type-erased output of one legacy text inspect section.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct InspectSectionOutput {
  pub id: &'static str,
  pub text: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub json: Option<Value>,
}

/// Ordered legacy text inspect document.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct InspectDocument {
  pub sections: Vec<InspectSectionOutput>,
}

impl InspectDocument {
  pub fn render_text(&self) -> String {
    let mut output = String::new();
    for section in &self.sections {
      output.push_str(&section.text);
      if !section.text.ends_with('\n') {
        output.push('\n');
      }
    }
    output
  }
}

/// Assembly errors for the temporary legacy composer.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum InspectError {
  #[error("{0}")]
  Message(String),
  #[error("duplicate inspect section id: {0}")]
  DuplicateSectionId(&'static str),
  #[error("inspect section id mismatch: registered={registered}, output={output}")]
  SectionIdMismatch {
    registered: &'static str,
    output: &'static str,
  },
}

impl From<String> for InspectError {
  fn from(value: String) -> Self {
    Self::Message(value)
  }
}

impl From<&str> for InspectError {
  fn from(value: &str) -> Self {
    Self::Message(value.to_string())
  }
}

/// One section collected from a legacy local-store run.
pub trait InspectSection: Send + Sync {
  fn id(&self) -> &'static str;

  fn collect(&self, store: &LocalStore, run: &CanonicalRun) -> Result<Option<InspectSectionOutput>, InspectError>;
}

/// Ordered composer retained for legacy CLI and MCP text output only.
#[derive(Clone)]
pub struct InspectComposer {
  sections: Arc<Vec<Arc<dyn InspectSection>>>,
}

impl std::fmt::Debug for InspectComposer {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter
      .debug_struct("InspectComposer")
      .field("section_count", &self.sections.len())
      .field("section_ids", &self.sections.iter().map(|section| section.id()).collect::<Vec<_>>())
      .finish()
  }
}

impl InspectComposer {
  pub fn try_new(sections: Vec<Arc<dyn InspectSection>>) -> Result<Self, InspectError> {
    let mut seen = std::collections::BTreeSet::new();
    for section in &sections {
      let id = section.id();
      if !seen.insert(id) {
        return Err(InspectError::DuplicateSectionId(id));
      }
    }
    Ok(Self {
      sections: Arc::new(sections),
    })
  }

  pub fn sections(&self) -> &[Arc<dyn InspectSection>] {
    &self.sections
  }

  pub fn collect_document(&self, store: &LocalStore, run: &CanonicalRun) -> Result<InspectDocument, InspectError> {
    let mut sections = Vec::new();
    for section in self.sections.iter() {
      if let Some(output) = section.collect(store, run)? {
        let registered = section.id();
        if output.id != registered {
          return Err(InspectError::SectionIdMismatch {
            registered,
            output: output.id,
          });
        }
        sections.push(output);
      }
    }
    Ok(InspectDocument { sections })
  }

  pub fn inspect_text(&self, store: &LocalStore, run_id: &str) -> Result<String, InspectError> {
    let run = store.read_run(run_id).map_err(InspectError::Message)?;
    Ok(self.collect_document(store, &run)?.render_text())
  }
}
