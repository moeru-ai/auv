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
    // TODO(run-contract-task-22): Delete this legacy projection implementation
    // when the remaining server adapter accepts RunStore/RunSnapshot. The
    // canonical Minecraft extension is run_snapshot_json_extension above.
    InspectReadProjection::run_json_extension(&self.inner, extension, store, run)
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
  use std::sync::Arc;

  use auv_game_minecraft::training_result_spatial_query::publish_minecraft_training_spatial_query;
  use auv_tracing::dispatcher;
  use auv_tracing::{AuthorityId, Context, MemoryRunStore, RunId, RunStore, configure};
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
    let snapshot = store.load_snapshot(run_id).await.expect("load snapshot").expect("snapshot");

    let value = ProductInspectReadProjection::default()
      .run_snapshot_json_extension("minecraft-quality-baseline-report", store.as_ref(), &snapshot)
      .await
      .expect("quality extension")
      .expect("registered extension");
    assert_eq!(value["spatial_query"]["uri"], metadata.uri().to_string());
    assert_eq!(value["spatial_query"]["payload"]["status"], "answered");
    assert_eq!(value["evidence_coverage"], "partial");
  }
}
