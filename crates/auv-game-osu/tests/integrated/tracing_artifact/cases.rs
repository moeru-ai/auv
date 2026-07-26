use std::sync::Arc;

use auv_game_osu::projection::{
  OSU_PROJECTION_PURPOSE, ProjectionArtifact, ProjectionBounds, ProjectionDerivationMethod, publish_osu_projection,
};
use auv_tracing::{Context, MemoryTracingStore, RunId, TraceRecord, configure, dispatcher};

#[test]
fn public_projection_publisher_records_the_artifact() {
  futures_executor::block_on(async {
    let store = Arc::new(MemoryTracingStore::new());
    let dispatch = configure().tracing_store(store.clone()).build().expect("memory tracing dispatch");
    let context = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
    let projection = ProjectionArtifact {
      source_window_bounds: ProjectionBounds {
        x: 0.0,
        y: 0.0,
        width: 1280.0,
        height: 720.0,
      },
      capture_bounds: None,
      capture_width: None,
      capture_height: None,
      capture_scale_factor: None,
      scale_x: 1.0,
      scale_y: 1.0,
      offset_x: 0.0,
      offset_y: 0.0,
      match_radius_px: 8.0,
      derivation_method: ProjectionDerivationMethod::LayoutRule,
      verification_reference: None,
    };

    let metadata = publish_osu_projection(Some(&context), &projection).await.expect("projection publication").expect("enabled publication");
    dispatch.flush().await.expect("flush tracing");

    assert_eq!(metadata.purpose().as_str(), OSU_PROJECTION_PURPOSE);
    assert!(store.artifact(metadata.uri()).is_some());
    assert!(store.records().iter().any(|record| matches!(record, TraceRecord::Artifact { metadata: stored, .. } if stored == &metadata)));
  });
}
