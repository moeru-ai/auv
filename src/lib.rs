// File: src/lib.rs
pub mod api;
pub mod app;
pub mod candidate_promotion;
pub mod contract;
pub mod inspect;
pub mod mcp;
pub mod model;
pub mod run_read;
pub mod runtime;
pub mod scene_state_read;
pub mod scroll_scan;
pub mod session;
pub mod stability;
pub mod view_parser_read;

use std::path::PathBuf;
use std::sync::Arc;

use auv_inspect_model::{InspectComposer, InspectDocument};
use auv_inspect_server::{InspectDocumentWire, InspectSectionWire};
use auv_tracing_driver::store::LocalStore;
use model::AuvResult;
use runtime::Runtime;

/// Convert a model document into the inspect-server wire shape.
pub fn inspect_document_to_wire(document: InspectDocument) -> InspectDocumentWire {
  InspectDocumentWire {
    sections: document
      .sections
      .into_iter()
      .map(|section| InspectSectionWire {
        id: section.id.to_string(),
        text: section.text,
        json: section.json,
      })
      .collect(),
  }
}

/// Core inspect-server read projection (no donor JSON extensions).
///
/// Holds the injected core [`InspectComposer`] so HTTP inspect text/document
/// routes share the same composition path as core MCP defaults.
#[derive(Clone, Debug)]
pub struct RootInspectReadProjection {
  composer: Arc<InspectComposer>,
}

impl Default for RootInspectReadProjection {
  fn default() -> Self {
    Self::with_composer(crate::inspect::build_core_inspect_composer().expect("core inspect composer"))
  }
}

impl RootInspectReadProjection {
  pub fn with_composer(composer: Arc<InspectComposer>) -> Self {
    Self { composer }
  }

  pub fn composer(&self) -> &Arc<InspectComposer> {
    &self.composer
  }
}

impl auv_inspect_server::InspectReadProjection for RootInspectReadProjection {
  fn run_enrichment(
    &self,
    store: &auv_tracing_driver::store::LocalStore,
    run: &auv_tracing_driver::store::CanonicalRun,
  ) -> Result<auv_inspect_server::InspectRunEnrichment, String> {
    let view_parser = view_parser_read::build_view_parser_inspect(store, run)?;
    let view_parser_summary = auv_view::memory::summarize_view_parser_inspect(&view_parser);
    Ok(auv_inspect_server::InspectRunEnrichment {
      command_boundary_claims: extract_command_boundary_claims_for_inspect(run),
      verifications: run_read::extract_verifications(store, run)?
        .into_iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to encode verification inspect values: {error}"))?,
      observation_snapshots: run_read::extract_observation_snapshots(store, run)?
        .into_iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to encode observation snapshot inspect values: {error}"))?,
      detector_recognition_lineage: run_read::extract_detector_recognition_lineage(store, run)?
        .into_iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to encode detector recognition lineage inspect values: {error}"))?,
      view_parser,
      view_parser_summary,
    })
  }

  fn run_json_extension(
    &self,
    _extension: &str,
    _store: &auv_tracing_driver::store::LocalStore,
    _run: &auv_tracing_driver::store::CanonicalRun,
  ) -> Result<Option<serde_json::Value>, String> {
    Ok(None)
  }

  fn inspect_document(
    &self,
    store: &auv_tracing_driver::store::LocalStore,
    run: &auv_tracing_driver::store::CanonicalRun,
  ) -> Result<Option<InspectDocumentWire>, String> {
    let document = self.composer.collect_document(store, run).map_err(|error| error.to_string())?;
    Ok(Some(inspect_document_to_wire(document)))
  }

  fn inspect_text(&self, store: &auv_tracing_driver::store::LocalStore, run_id: &str) -> Result<Option<String>, String> {
    self.composer.inspect_text(store, run_id).map(Some).map_err(|error| error.to_string())
  }
}

fn extract_command_boundary_claims_for_inspect(
  run: &auv_tracing_driver::store::CanonicalRun,
) -> Vec<auv_inspect_server::CommandBoundaryClaim> {
  run
    .events
    .iter()
    .filter_map(|event| match event.name.as_str() {
      "command.verification" => Some(auv_inspect_server::CommandBoundaryClaim {
        span_id: event.span_id.clone(),
        kind: "verification".to_string(),
        message: event.message.clone().unwrap_or_default(),
      }),
      "command.known_limit" => Some(auv_inspect_server::CommandBoundaryClaim {
        span_id: event.span_id.clone(),
        kind: "known_limit".to_string(),
        message: event.message.clone().unwrap_or_default(),
      }),
      _ => None,
    })
    .collect()
}

pub fn build_default_runtime(project_root: PathBuf) -> AuvResult<Runtime> {
  let store_root = default_project_store_root(project_root.clone());
  build_runtime_with_store_root(project_root, store_root)
}

pub fn build_runtime_with_store_root(project_root: PathBuf, store_root: PathBuf) -> AuvResult<Runtime> {
  let store = LocalStore::new(store_root)?;
  Ok(Runtime::new(project_root, store))
}

pub fn default_project_store_root(project_root: PathBuf) -> PathBuf {
  project_root.join(".auv")
}

pub fn build_default_store(project_root: PathBuf) -> AuvResult<LocalStore> {
  LocalStore::new(default_project_store_root(project_root))
}
