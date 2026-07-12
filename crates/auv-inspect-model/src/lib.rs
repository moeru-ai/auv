//! Neutral inspect composition contract for AUV frontends.
//!
//! # Provisional core terms
//!
//! `InspectSection`, `InspectDocument`, and `InspectComposer` are provisional
//! core vocabulary (see `docs/TERMS_AND_CONCEPTS.md`). They own composition
//! shape only — not donor artifact schemas.
//!
//! # Semantics
//!
//! - Section order = registration order.
//! - Duplicate section ids fail composer assembly.
//! - After `collect` returns `Some(output)`, `output.id` must equal `section.id()`;
//!   mismatch fails the document with [`InspectError::SectionIdMismatch`].
//! - `collect` returning `None` omits the section from the document.
//! - A single section `Err` aborts the whole document.
//! - Outputs are type-erased via [`InspectSectionOutput`] (no downcast).

use std::fs;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::Arc;

use auv_tracing_driver::store::{CanonicalRun, LocalStore};
use auv_tracing_driver::trace::{ArtifactId, ArtifactRecordV1Alpha1, EventId, RunId, SpanId};
use serde::de::DeserializeOwned;
use serde_json::Value;
use thiserror::Error;

/// Core-neutral role for line-oriented telemetry sample artifacts.
///
/// Owned here with the shared telemetry reader so core and donor crates do not
/// fork the durable role string.
pub const TELEMETRY_SAMPLE_ARTIFACT_ROLE: &str = "telemetry-sample";

/// Shared artifact identity projection used by inspect / run_read donors.
///
/// NOTICE(inspect-composition / S3a): owned here so game crates can parse
/// artifacts without depending on `auv-cli`. Do not fork a same-shape copy.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ArtifactRefLineage {
  pub run_id: RunId,
  pub artifact_id: ArtifactId,
  pub span_id: SpanId,
  pub captured_event_id: Option<EventId>,
  pub role: Option<String>,
  pub path: Option<String>,
  pub summary: Option<String>,
  pub resolved: bool,
}

/// MIME policy for JSON artifact readers (shared; do not copy into donors).
pub fn is_json_mime(mime_type: &str) -> bool {
  mime_type == "application/json" || mime_type.ends_with("+json")
}

/// Build an [`ArtifactRefLineage`] from a recorded artifact row.
pub fn artifact_record_lineage(run_id: RunId, artifact: &ArtifactRecordV1Alpha1) -> ArtifactRefLineage {
  ArtifactRefLineage {
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

/// Open a scoped artifact file for read-side donors (no `auv-cli` dependency).
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

/// Parse a JSON artifact into `T` for ordinary donor readers.
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

/// Summarize a line-oriented telemetry artifact (path, non-empty line count, bytes).
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

/// Type-erased output of one inspect section.
#[derive(Clone, Debug, PartialEq)]
pub struct InspectSectionOutput {
  pub id: &'static str,
  pub text: String,
  pub json: Option<Value>,
}

/// Ordered collection of collected section outputs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InspectDocument {
  pub sections: Vec<InspectSectionOutput>,
}

impl InspectDocument {
  pub fn render_text(&self) -> String {
    let mut out = String::new();
    for section in &self.sections {
      out.push_str(&section.text);
      if !section.text.ends_with('\n') {
        out.push('\n');
      }
    }
    out
  }
}

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

/// One composable inspect section.
pub trait InspectSection: Send + Sync {
  fn id(&self) -> &'static str;

  fn collect(&self, store: &LocalStore, run: &CanonicalRun) -> Result<Option<InspectSectionOutput>, InspectError>;
}

/// Explicit composer shared by CLI / MCP / inspect-server projections.
#[derive(Clone)]
pub struct InspectComposer {
  sections: Arc<Vec<Arc<dyn InspectSection>>>,
}

impl std::fmt::Debug for InspectComposer {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("InspectComposer")
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
    let run = store.read_run(run_id).map_err(|error| InspectError::Message(error))?;
    Ok(self.collect_document(store, &run)?.render_text())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use auv_tracing_driver::trace::{
    RUN_API_VERSION, RunId, RunRecordV1Alpha1, RunType, SPAN_API_VERSION, SpanId, SpanRecordV1Alpha1, TraceId, TraceState, TraceStatusCode,
  };

  struct FixedSection {
    id: &'static str,
    output_id: Option<&'static str>,
    text: &'static str,
  }

  impl InspectSection for FixedSection {
    fn id(&self) -> &'static str {
      self.id
    }

    fn collect(&self, _store: &LocalStore, _run: &CanonicalRun) -> Result<Option<InspectSectionOutput>, InspectError> {
      Ok(Some(InspectSectionOutput {
        id: self.output_id.unwrap_or(self.id),
        text: format!("{}\n", self.text),
        json: None,
      }))
    }
  }

  fn fixed(id: &'static str, text: &'static str) -> FixedSection {
    FixedSection {
      id,
      output_id: None,
      text,
    }
  }

  fn write_minimal_run(store: &LocalStore, run_id: &RunId) {
    let span_id = SpanId::new("span_root");
    store
      .write_run_snapshot(&CanonicalRun {
        run: RunRecordV1Alpha1 {
          api_version: RUN_API_VERSION.to_string(),
          run_id: run_id.clone(),
          trace_id: TraceId::new("trace"),
          run_type: RunType::Command,
          state: TraceState::Ended,
          status_code: TraceStatusCode::Ok,
          started_at_millis: 1,
          finished_at_millis: Some(2),
          root_span_id: span_id.clone(),
          attributes: Default::default(),
          summary: None,
          failure: None,
        },
        spans: vec![SpanRecordV1Alpha1 {
          api_version: SPAN_API_VERSION.to_string(),
          span_id,
          parent_span_id: None,
          name: "root".to_string(),
          state: TraceState::Ended,
          status_code: TraceStatusCode::Ok,
          started_at_millis: 1,
          finished_at_millis: Some(2),
          attributes: Default::default(),
          summary: None,
          failure: None,
        }],
        events: Vec::new(),
        artifacts: Vec::new(),
      })
      .expect("snapshot");
  }

  #[test]
  fn composer_rejects_duplicate_ids() {
    let result = InspectComposer::try_new(vec![Arc::new(fixed("a", "one")), Arc::new(fixed("a", "two"))]);
    assert!(matches!(result, Err(InspectError::DuplicateSectionId("a"))));
  }

  #[test]
  fn composer_rejects_collect_output_id_mismatch() {
    let root = std::env::temp_dir().join(format!(
      "auv-inspect-model-mismatch-{}",
      std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("clock").as_nanos()
    ));
    let store = LocalStore::new(root.clone()).expect("store");
    let run_id = RunId::new("run_mismatch");
    write_minimal_run(&store, &run_id);

    let composer = InspectComposer::try_new(vec![Arc::new(FixedSection {
      id: "a",
      output_id: Some("b"),
      text: "WRONG",
    })])
    .expect("composer");
    let err = composer.inspect_text(&store, run_id.as_str()).expect_err("id mismatch");
    assert_eq!(
      err,
      InspectError::SectionIdMismatch {
        registered: "a",
        output: "b",
      }
    );
    let _ = std::fs::remove_dir_all(root);
  }

  #[test]
  fn composer_renders_in_registration_order() {
    let root = std::env::temp_dir()
      .join(format!("auv-inspect-model-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("clock").as_nanos()));
    let store = LocalStore::new(root.clone()).expect("store");
    let run_id = RunId::new("run_composer");
    write_minimal_run(&store, &run_id);

    let composer = InspectComposer::try_new(vec![
      Arc::new(fixed("first", "FIRST")),
      Arc::new(fixed("second", "SECOND")),
    ])
    .expect("composer");
    let text = composer.inspect_text(&store, run_id.as_str()).expect("text");
    assert_eq!(text, "FIRST\nSECOND\n");
    let _ = std::fs::remove_dir_all(root);
  }
}
