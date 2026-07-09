use auv_tracing_driver::store::{CanonicalRun, LocalStore};
use auv_view::memory::{ViewParserInspect, ViewParserListSummary};

use crate::InspectResult;

pub trait InspectReadProjection: Send + Sync + 'static {
  fn run_enrichment(&self, store: &LocalStore, run: &CanonicalRun) -> InspectResult<InspectRunEnrichment>;
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
  pub view_parser: ViewParserInspect,
  pub view_parser_summary: ViewParserListSummary,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct CommandBoundaryClaim {
  pub span_id: auv_tracing_driver::trace::SpanId,
  pub kind: String,
  pub message: String,
}
