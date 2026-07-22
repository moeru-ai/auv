//! Product inspect-server read projection.
//!
//! Wraps core enrichment and injects the product [`InspectComposer`] so HTTP
//! inspect text/document routes share the same composition path as product CLI
//! and product MCP. Donor JSON extensions (e.g. quality baseline) remain
//! registered by extension key — not as first-class Minecraft routes.

use std::sync::Arc;

use auv_inspect_model::legacy::InspectComposer;
use auv_inspect_server::legacy::InspectReadProjection;
use auv_runtime::RootInspectReadProjection;

/// Product projection: core enrichment + product composer + named JSON extensions.
#[derive(Clone)]
pub struct ProductInspectReadProjection {
  inner: RootInspectReadProjection,
  canonical_store: Option<Arc<dyn auv_tracing::RunStore>>,
}

impl std::fmt::Debug for ProductInspectReadProjection {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter
      .debug_struct("ProductInspectReadProjection")
      .field("inner", &self.inner)
      .field("has_canonical_store", &self.canonical_store.is_some())
      .finish()
  }
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
      canonical_store: None,
    }
  }

  /// Installs the same canonical authority used by the Inspect server adapter.
  pub fn with_canonical_store(store: Arc<dyn auv_tracing::RunStore>) -> Self {
    Self {
      inner: RootInspectReadProjection::with_composer(crate::inspect::build_product_inspect_composer().expect("product inspect composer")),
      canonical_store: Some(store),
    }
  }

  pub fn composer(&self) -> &Arc<InspectComposer> {
    self.inner.composer()
  }

  pub async fn run_snapshot_json_extension(
    &self,
    extension: &str,
    store: &dyn auv_tracing::RunStore,
    snapshot: &auv_tracing::RunSnapshot,
  ) -> Result<Option<serde_json::Value>, String> {
    match extension {
      "minecraft-quality-baseline-report" => {
        let inspection = auv_game_minecraft::inspect::read_minecraft_quality_spatial_inspection(store, snapshot)
          .await
          .map_err(|error| error.to_string())?;
        serde_json::to_value(inspection.quality_baseline())
          .map(Some)
          .map_err(|error| format!("failed to encode minecraft quality baseline report: {error}"))
      }
      _ => Ok(None),
    }
  }
}

impl auv_inspect_server::legacy::InspectReadProjection for ProductInspectReadProjection {
  fn run_enrichment(
    &self,
    store: &auv_tracing_driver::store::LocalStore,
    run: &auv_tracing_driver::store::CanonicalRun,
  ) -> Result<auv_inspect_server::legacy::InspectRunEnrichment, String> {
    InspectReadProjection::run_enrichment(&self.inner, store, run)
  }

  fn run_json_extension(
    &self,
    extension: &str,
    store: &auv_tracing_driver::store::LocalStore,
    run: &auv_tracing_driver::store::CanonicalRun,
  ) -> Result<Option<serde_json::Value>, String> {
    if extension != "minecraft-quality-baseline-report" {
      return InspectReadProjection::run_json_extension(&self.inner, extension, store, run);
    }

    let Some(canonical_store) = self.canonical_store.as_deref() else {
      return InspectReadProjection::run_json_extension(&self.inner, extension, store, run);
    };
    let run_id = run
      .run
      .run_id
      .as_str()
      .parse::<auv_tracing::RunId>()
      .map_err(|error| format!("inspect extension run ID is not canonical: {error}"))?;
    let Some(snapshot) = futures_executor::block_on(canonical_store.load_snapshot(run_id))
      .map_err(|error| format!("failed to load canonical inspect snapshot for {run_id}: {error}"))?
    else {
      return Ok(None);
    };
    futures_executor::block_on(self.run_snapshot_json_extension(extension, canonical_store, &snapshot))
  }

  fn inspect_document(
    &self,
    store: &auv_tracing_driver::store::LocalStore,
    run: &auv_tracing_driver::store::CanonicalRun,
  ) -> Result<Option<auv_inspect_model::legacy::InspectDocument>, String> {
    InspectReadProjection::inspect_document(&self.inner, store, run)
  }

  fn inspect_text(&self, store: &auv_tracing_driver::store::LocalStore, run_id: &str) -> Result<Option<String>, String> {
    InspectReadProjection::inspect_text(&self.inner, store, run_id)
  }
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;
  use std::fs;
  use std::sync::Arc;

  use auv_game_minecraft::training_result_spatial_query::publish_minecraft_training_spatial_query;
  use auv_tracing::dispatcher;
  use auv_tracing::{AuthorityId, Context, MemoryRunStore, RunId, configure};
  use auv_tracing_driver::store::{CanonicalRun, LocalStore};
  use auv_tracing_driver::trace::{
    RUN_API_VERSION, RunId as LegacyRunId, RunRecordV1Alpha1, RunType, SpanId, TraceId, TraceState, TraceStatusCode,
  };
  use serde_json::json;

  use super::*;

  #[tokio::test]
  async fn minecraft_quality_extension_reads_typed_snapshot_artifact() {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
    let spatial_query = serde_json::from_value(json!({
      "schema_version": 1,
      "generated_at_millis": 20,
      "training_result_semantic_manifest_path": ".tmp/mc10-smoke-review/semantic/minecraft-3dgs-training-result-semantic.json",
      "source_training_result_artifact_manifest_path": "result-artifacts.json",
      "source_training_result_manifest_path": "result.json",
      "source_training_job_manifest_path": "job.json",
      "source_training_launch_plan_path": "launch.json",
      "source_training_package_manifest_path": "package.json",
      "source_scene_packet_manifest_path": "scene.json",
      "source_bundle_manifest_paths": ["bundle.json"],
      "source_run_ids": ["run-source"],
      "trainer_backend": "nerfstudio",
      "job_backend": "fixture",
      "normalized_result_dir": "normalized",
      "query_kind": "block_projection",
      "target_block": {"x": 511, "y": 73, "z": 728},
      "target_face": "north",
      "target_semantics": "hit_face_center",
      "selected_backend": "projection_reference",
      "status": "answered",
      "visibility": "visible",
      "screen_point": {"x": 12.0, "y": 34.0},
      "match_radius_px": 8.0,
      "confidence": 1.0,
      "basis_frame_id": "frame-20",
      "comparison_verdict": "match",
      "known_limits": ["fixture"]
    }))
    .expect("typed spatial query");
    let metadata = publish_minecraft_training_spatial_query(Some(&root), &spatial_query)
      .await
      .expect("publish spatial query")
      .expect("enabled publication");
    dispatch.flush().await.expect("flush spatial query");
    let legacy_root = std::env::temp_dir().join(format!("auv-product-inspect-projection-{}", auv_runtime::model::now_millis()));
    let _ = fs::remove_dir_all(&legacy_root);
    let legacy_store = LocalStore::new(legacy_root.clone()).expect("legacy adapter store");
    let legacy_run = legacy_run(run_id);
    let projection = ProductInspectReadProjection::with_canonical_store(store.clone());
    let value = InspectReadProjection::run_json_extension(&projection, "minecraft-quality-baseline-report", &legacy_store, &legacy_run)
      .expect("quality extension")
      .expect("registered extension");
    assert_eq!(value["spatial_query"]["uri"], metadata.uri().to_string());
    assert_eq!(value["spatial_query"]["payload"]["status"], "answered");
    assert_eq!(value["evidence_coverage"], "partial");

    let _ = fs::remove_dir_all(legacy_root);
  }

  #[test]
  fn production_adapter_preserves_unknown_extension_behavior() {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let run_id = RunId::new();
    let legacy_root = std::env::temp_dir().join(format!("auv-product-inspect-projection-{}", auv_runtime::model::now_millis()));
    let _ = fs::remove_dir_all(&legacy_root);
    let legacy_store = LocalStore::new(legacy_root.clone()).expect("legacy adapter store");
    let legacy_run = legacy_run(run_id);
    let projection = ProductInspectReadProjection::with_canonical_store(store);

    let value = InspectReadProjection::run_json_extension(&projection, "not-a-real-extension", &legacy_store, &legacy_run)
      .expect("unknown extension projection");
    assert_eq!(value, None);

    let _ = fs::remove_dir_all(legacy_root);
  }

  fn legacy_run(run_id: RunId) -> CanonicalRun {
    CanonicalRun {
      run: RunRecordV1Alpha1 {
        api_version: RUN_API_VERSION.to_string(),
        run_id: LegacyRunId::new(run_id.to_string()),
        trace_id: TraceId::new("trace_product_projection"),
        run_type: RunType::Command,
        state: TraceState::Ended,
        status_code: TraceStatusCode::Ok,
        started_at_millis: 1,
        finished_at_millis: Some(2),
        root_span_id: SpanId::new("span_product_projection"),
        attributes: BTreeMap::new(),
        summary: None,
        failure: None,
      },
      spans: Vec::new(),
      events: Vec::new(),
      artifacts: Vec::new(),
    }
  }
}
