use std::sync::Arc;

use auv_tracing::{
  ArtifactPurpose, Attributes, AuthorityId, ByteLength, ContentType, Context, MemoryRunStore, NewArtifact, RunId, RunStore, Sha256Digest,
  configure, dispatcher,
};
use futures_util::io::Cursor as AsyncCursor;
use sha2::{Digest, Sha256};

#[test]
fn minecraft_sections_are_non_empty_without_minecraft_artifacts() {
  futures_executor::block_on(async {
    let (store, snapshot) = snapshot_without_minecraft_artifacts().await;
    let mut sections = crate::inspect::inspect_sections_primary(store.as_ref(), &snapshot).await.expect("primary sections");
    sections.extend(crate::inspect::inspect_sections_quality_spatial(store.as_ref(), &snapshot).await.expect("quality/spatial sections"));
    assert_eq!(
      sections.iter().map(crate::inspect::MinecraftInspectSection::id).collect::<Vec<_>>(),
      ["minecraft_primary", "minecraft_quality_spatial"]
    );
    assert!(sections.iter().all(|section| !section.text().is_empty()));
    let text = sections.into_iter().map(crate::inspect::MinecraftInspectSection::into_text).collect::<String>();

    assert!(text.contains("\nMC-1 Telemetry Samples:\n- none\n"));
    assert!(text.contains("\nMC-17 Quality Baseline Report:\n"));
    assert!(!text.contains("MC-19 Query Wired Live Action"));
  });
}

async fn snapshot_without_minecraft_artifacts() -> (Arc<MemoryRunStore>, auv_tracing::RunSnapshot) {
  let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
  let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
  let run_id = RunId::new();
  let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
  let body = b"{}".to_vec();
  let artifact = NewArtifact::new(
    ArtifactPurpose::parse("auv.test.minecraft.unrelated").expect("unrelated purpose"),
    ContentType::parse("application/json").expect("JSON content type"),
    ByteLength::new(body.len() as u64).expect("body length"),
    Sha256Digest::new(Sha256::digest(&body).into()),
    Attributes::empty(),
    AsyncCursor::new(body),
  );
  root.in_scope(|| auv_tracing::emit_artifact!(artifact)).await.expect("publish unrelated artifact").expect("enabled publication");
  dispatch.flush().await.expect("flush unrelated artifact");
  let snapshot = store.load_snapshot(run_id).await.expect("load snapshot").expect("snapshot");
  (store, snapshot)
}
