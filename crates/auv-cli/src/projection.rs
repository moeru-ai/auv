//! Product inspect-server read projection.
//!
//! Wraps core enrichment and injects the product [`InspectComposer`] so HTTP
//! inspect text/document routes share the same composition path as product CLI
//! and product MCP. Donor JSON extensions (e.g. quality baseline) remain
//! registered by extension key — not as first-class Minecraft routes.

use std::sync::Arc;

use auv_inspect_model::InspectComposer;
use auv_inspect_server::InspectReadProjection;
use auv_runtime::RootInspectReadProjection;

/// Product projection: core enrichment + product composer + named JSON extensions.
#[derive(Clone, Debug)]
pub struct ProductInspectReadProjection {
  inner: RootInspectReadProjection,
}

impl Default for ProductInspectReadProjection {
  fn default() -> Self {
    Self::with_composer(crate::inspect::build_product_inspect_composer().expect("product inspect composer"))
  }
}

impl ProductInspectReadProjection {
  pub fn with_composer(composer: Arc<InspectComposer>) -> Self {
    Self {
      inner: RootInspectReadProjection::with_composer(composer),
    }
  }

  pub fn composer(&self) -> &Arc<InspectComposer> {
    self.inner.composer()
  }
}

impl auv_inspect_server::InspectReadProjection for ProductInspectReadProjection {
  fn run_enrichment(
    &self,
    store: &auv_tracing_driver::store::LocalStore,
    run: &auv_tracing_driver::store::CanonicalRun,
  ) -> Result<auv_inspect_server::InspectRunEnrichment, String> {
    InspectReadProjection::run_enrichment(&self.inner, store, run)
  }

  fn run_json_extension(
    &self,
    extension: &str,
    store: &auv_tracing_driver::store::LocalStore,
    run: &auv_tracing_driver::store::CanonicalRun,
  ) -> Result<Option<serde_json::Value>, String> {
    match extension {
      // Donor-specific payload registered by key; served via generic
      // `/runs/{id}/extensions/{extension}` — not a Minecraft-first route.
      "minecraft-quality-baseline-report" => {
        let report = auv_game_minecraft::run_read::quality_baseline_report_with_verdicts_for_run(store, run.run.run_id.as_str())?;
        serde_json::to_value(report).map(Some).map_err(|error| format!("failed to encode minecraft quality baseline report: {error}"))
      }
      other => InspectReadProjection::run_json_extension(&self.inner, other, store, run),
    }
  }

  fn inspect_document(
    &self,
    store: &auv_tracing_driver::store::LocalStore,
    run: &auv_tracing_driver::store::CanonicalRun,
  ) -> Result<Option<auv_inspect_model::InspectDocument>, String> {
    InspectReadProjection::inspect_document(&self.inner, store, run)
  }

  fn inspect_text(&self, store: &auv_tracing_driver::store::LocalStore, run_id: &str) -> Result<Option<String>, String> {
    InspectReadProjection::inspect_text(&self.inner, store, run_id)
  }
}
