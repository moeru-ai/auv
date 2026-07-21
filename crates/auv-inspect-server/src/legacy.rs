//! Temporary text-composer projection over retired tracing-driver records.
//!
//! TODO(run-contract-tasks-17-23): Remove this module after Tasks 17-23 move
//! the remaining CLI and producer readers to canonical snapshots. The V1
//! router does not install or invoke this projection.

use auv_inspect_model::legacy::InspectDocument;
use auv_tracing_driver::store::{CanonicalRun, LocalStore};
use auv_view::memory::{ViewParserInspect, ViewParserListSummary};

use crate::InspectResult;

/// Read-side enrichment retained only for legacy CLI/text composition.
pub trait InspectReadProjection: Send + Sync + 'static {
  fn run_enrichment(&self, store: &LocalStore, run: &CanonicalRun) -> InspectResult<InspectRunEnrichment>;

  fn run_json_extension(&self, extension: &str, store: &LocalStore, run: &CanonicalRun) -> InspectResult<Option<serde_json::Value>> {
    let _ = (store, run, extension);
    Ok(None)
  }

  fn inspect_document(&self, store: &LocalStore, run: &CanonicalRun) -> InspectResult<Option<InspectDocument>> {
    let _ = (store, run);
    Ok(None)
  }

  fn inspect_text(&self, store: &LocalStore, run_id: &str) -> InspectResult<Option<String>> {
    let run = store.read_run(run_id)?;
    Ok(self.inspect_document(store, &run)?.map(|document| document.render_text()))
  }
}

#[derive(Clone, Debug, Default)]
pub struct DefaultInspectReadProjection;

impl InspectReadProjection for DefaultInspectReadProjection {
  fn run_enrichment(&self, _store: &LocalStore, _run: &CanonicalRun) -> InspectResult<InspectRunEnrichment> {
    Ok(InspectRunEnrichment::default())
  }
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct InspectRunEnrichment {
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub command_boundary_claims: Vec<CommandBoundaryClaim>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub verifications: Vec<serde_json::Value>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub observation_snapshots: Vec<serde_json::Value>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub detector_recognition_lineage: Vec<serde_json::Value>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub input_action_results: Vec<serde_json::Value>,
  pub view_parser: ViewParserInspect,
  pub view_parser_summary: ViewParserListSummary,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct CommandBoundaryClaim {
  pub span_id: auv_tracing_driver::trace::SpanId,
  pub kind: String,
  pub message: String,
}
