use auv_tracing_driver::store::{CanonicalRun, LocalStore};
use auv_view::memory::{ViewParserInspect, ViewParserListSummary};

use crate::InspectResult;

/// Wire shape for one collected inspect section (HTTP / projection boundary).
///
/// Mirrors `auv_inspect_model::InspectSectionOutput` without forcing the server
/// crate to depend on the composition model crate.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct InspectSectionWire {
  pub id: String,
  pub text: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub json: Option<serde_json::Value>,
}

/// Wire shape for an ordered inspect document.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct InspectDocumentWire {
  pub sections: Vec<InspectSectionWire>,
}

impl InspectDocumentWire {
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

pub trait InspectReadProjection: Send + Sync + 'static {
  fn run_enrichment(&self, store: &LocalStore, run: &CanonicalRun) -> InspectResult<InspectRunEnrichment>;

  /// Named JSON extension lookup for an already-loaded run.
  ///
  /// Returns `Ok(None)` when the extension key is unsupported for this
  /// projection (HTTP maps that to 404). Real load/encode failures remain `Err`.
  fn run_json_extension(&self, extension: &str, store: &LocalStore, run: &CanonicalRun) -> InspectResult<Option<serde_json::Value>> {
    let _ = (store, run, extension);
    Ok(None)
  }

  /// Composer-backed structured inspect document (S5).
  ///
  /// Default: unsupported. Core / product projections override by collecting
  /// from an injected [`auv_inspect_model::InspectComposer`].
  fn inspect_document(&self, store: &LocalStore, run: &CanonicalRun) -> InspectResult<Option<InspectDocumentWire>> {
    let _ = (store, run);
    Ok(None)
  }

  /// Composer-backed inspect text (S5). Default derives from [`Self::inspect_document`].
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
  pub view_parser: ViewParserInspect,
  pub view_parser_summary: ViewParserListSummary,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct CommandBoundaryClaim {
  pub span_id: auv_tracing_driver::trace::SpanId,
  pub kind: String,
  pub message: String,
}
