use std::sync::Arc;

use auv_game_minecraft::artifact::{MINECRAFT_PROJECTION_PURPOSE, publish_minecraft_projection};
use auv_game_minecraft::{MinecraftProjectionArtifact, ProjectionViewportBounds, ProjectionVisibility};
use auv_tracing::{Context, MemoryTracingStore, RunId, TraceRecord, configure, dispatcher};

#[test]
fn public_projection_publisher_records_the_artifact() {
  futures_executor::block_on(async {
    let store = Arc::new(MemoryTracingStore::new());
    let dispatch = configure().tracing_store(store.clone()).build().expect("memory tracing dispatch");
    let context = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
    let projection = MinecraftProjectionArtifact {
      spatial_frame_id: "frame-1".to_string(),
      world_tick: 1,
      monotonic_timestamp_ms: 2,
      screenshot_artifact_ref: None,
      mc_capture_skew_ms: None,
      viewport_bounds: ProjectionViewportBounds {
        x: 0.0,
        y: 0.0,
        width: 1280.0,
        height: 720.0,
      },
      projected_point: None,
      visibility: ProjectionVisibility::OutsideWindow,
      raycast_block_id: None,
      screen_state: None,
      resource_pack_ids: vec![],
      mismatch_refusal_reason: None,
      verification_reference: None,
    };

    let metadata =
      publish_minecraft_projection(Some(&context), &projection).await.expect("projection publication").expect("enabled publication");
    dispatch.flush().await.expect("flush tracing");

    assert_eq!(metadata.purpose().as_str(), MINECRAFT_PROJECTION_PURPOSE);
    assert!(store.artifact(metadata.uri()).is_some());
    assert!(store.records().iter().any(|record| matches!(record, TraceRecord::Artifact { metadata: stored, .. } if stored == &metadata)));
  });
}
