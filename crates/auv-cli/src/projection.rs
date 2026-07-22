//! Product inspect-server read projection.
//!
//! Wraps core enrichment and injects the product [`InspectComposer`] so HTTP
//! inspect text/document routes share the same composition path as product CLI
//! and product MCP. Donor JSON extensions (e.g. quality baseline) remain
//! registered by extension key — not as first-class Minecraft routes.

use std::sync::Arc;

use auv_game_minecraft::MinecraftArtifactReadError;
use auv_inspect_model::legacy::InspectComposer;
use auv_inspect_server::legacy::InspectReadProjection;
use auv_inspect_server::{InspectRunExtensionError, InspectRunExtensionErrorCategory};
use auv_runtime::RootInspectReadProjection;
use auv_tracing::{ArtifactReadError, ErrorCode, ReadError};

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

impl auv_inspect_server::InspectRunExtension for ProductInspectReadProjection {
  fn project_json<'a>(
    &'a self,
    extension: &'a str,
    store: &'a Arc<dyn auv_tracing::RunStore>,
    snapshot: &'a auv_tracing::RunSnapshot,
  ) -> auv_tracing::BoxFuture<'a, Result<Option<serde_json::Value>, InspectRunExtensionError>> {
    Box::pin(async move {
      match extension {
        "minecraft-quality-baseline-report" => {
          let inspection = auv_game_minecraft::inspect::read_minecraft_quality_spatial_inspection(store.as_ref(), snapshot)
            .await
            .map_err(minecraft_artifact_extension_error)?;
          serde_json::to_value(inspection.quality_baseline()).map(Some).map_err(|_| {
            InspectRunExtensionError::new(
              InspectRunExtensionErrorCategory::Integrity,
              ErrorCode::parse("auv.inspect.minecraft_quality_baseline_encode_failed").expect("static extension error code"),
            )
          })
        }
        _ => Ok(None),
      }
    })
  }
}

fn minecraft_artifact_extension_error(error: MinecraftArtifactReadError) -> InspectRunExtensionError {
  let category = match &error {
    MinecraftArtifactReadError::InvalidExpectedPurpose { .. }
    | MinecraftArtifactReadError::InvalidExpectedContentType { .. }
    | MinecraftArtifactReadError::SnapshotAuthorityMismatch { .. }
    | MinecraftArtifactReadError::WrongOwner { .. }
    | MinecraftArtifactReadError::DanglingUri { .. }
    | MinecraftArtifactReadError::WrongPurpose { .. }
    | MinecraftArtifactReadError::WrongContentType { .. }
    | MinecraftArtifactReadError::PayloadTooLarge { .. }
    | MinecraftArtifactReadError::LengthOutOfRange { .. }
    | MinecraftArtifactReadError::LengthMismatch { .. }
    | MinecraftArtifactReadError::DigestMismatch { .. }
    | MinecraftArtifactReadError::MalformedJson { .. }
    | MinecraftArtifactReadError::InvalidPayload { .. } => InspectRunExtensionErrorCategory::Integrity,
    MinecraftArtifactReadError::Allocation { .. } => InspectRunExtensionErrorCategory::Unavailable,
    MinecraftArtifactReadError::Open { source, .. } => match source {
      ReadError::Forbidden => InspectRunExtensionErrorCategory::Forbidden,
      ReadError::Unavailable(_) => InspectRunExtensionErrorCategory::Unavailable,
      ReadError::NotFound
      | ReadError::InvalidReference(_)
      | ReadError::HistoryGap { .. }
      | ReadError::CursorAhead { .. }
      | ReadError::Integrity(_) => InspectRunExtensionErrorCategory::Integrity,
    },
    MinecraftArtifactReadError::Stream { source, .. } => match source {
      ArtifactReadError::Unavailable(_) => InspectRunExtensionErrorCategory::Unavailable,
      ArtifactReadError::Integrity(_) => InspectRunExtensionErrorCategory::Integrity,
    },
  };
  InspectRunExtensionError::new(category, error.code())
}

impl auv_inspect_server::legacy::InspectReadProjection for ProductInspectReadProjection {
  fn run_enrichment(
    &self,
    store: &auv_tracing_driver::store::LocalStore,
    run: &auv_tracing_driver::store::CanonicalRun,
  ) -> Result<auv_inspect_server::legacy::InspectRunEnrichment, String> {
    InspectReadProjection::run_enrichment(&self.inner, store, run)
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
  use auv_tracing::{ArtifactUri, RunId};
  use auv_tracing::{AuthorityId, Context, MemoryRunStore, configure};
  use axum::body::{Body, to_bytes};
  use axum::http::{Request, StatusCode};
  use serde_json::json;
  use tower::ServiceExt;

  use super::*;

  #[test]
  fn minecraft_artifact_errors_map_to_safe_extension_categories_and_codes() {
    let uri = ArtifactUri::from_ids(RunId::new(), auv_tracing::ArtifactId::new());
    let invalid_payload = minecraft_artifact_extension_error(MinecraftArtifactReadError::InvalidPayload {
      uri: uri.clone(),
      message: "unsafe payload detail".to_string(),
    });
    assert_eq!(invalid_payload.category(), InspectRunExtensionErrorCategory::Integrity);
    assert_eq!(invalid_payload.code().as_str(), "auv.minecraft.artifact.invalid_payload");

    let unavailable = minecraft_artifact_extension_error(MinecraftArtifactReadError::Stream {
      uri: uri.clone(),
      source: ArtifactReadError::Unavailable(ErrorCode::parse("auv.internal.backend_detail").unwrap()),
    });
    assert_eq!(unavailable.category(), InspectRunExtensionErrorCategory::Unavailable);
    assert_eq!(unavailable.code().as_str(), "auv.minecraft.artifact.stream_failed");

    let forbidden = minecraft_artifact_extension_error(MinecraftArtifactReadError::Open {
      uri,
      source: ReadError::Forbidden,
    });
    assert_eq!(forbidden.category(), InspectRunExtensionErrorCategory::Forbidden);
    assert_eq!(forbidden.code().as_str(), "auv.minecraft.artifact.open_failed");
  }

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
    let app = auv_inspect_server::router_with_extension(store, Arc::new(ProductInspectReadProjection::default()));
    let response = app
      .clone()
      .oneshot(
        Request::builder()
          .uri(format!("/v1/runs/{run_id}/extensions/minecraft-quality-baseline-report"))
          .body(Body::empty())
          .expect("quality extension request"),
      )
      .await
      .expect("quality extension response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.expect("quality extension body");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("quality extension JSON");
    assert_eq!(value["spatial_query"]["uri"], metadata.uri().to_string());
    assert_eq!(value["spatial_query"]["payload"]["status"], "answered");
    assert_eq!(value["evidence_coverage"], "partial");

    let unknown = app
      .oneshot(
        Request::builder()
          .uri(format!("/v1/runs/{run_id}/extensions/not-a-real-extension"))
          .body(Body::empty())
          .expect("unknown extension request"),
      )
      .await
      .expect("unknown extension response");
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
  }
}
