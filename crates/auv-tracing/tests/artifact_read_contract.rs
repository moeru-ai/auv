use std::sync::Arc;

use auv_tracing::{
  ArtifactPurpose, Attributes, AuthorityId, ByteLength, ContentType, Context, MemoryRunStore, NewArtifact, ReadArtifactError, RunId,
  RunStore, configure, dispatcher, read_artifact_bytes, read_json_artifact,
};
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq)]
struct JsonFixture {
  value: u64,
}

#[test]
fn reads_only_committed_bytes_matching_the_requested_contract() {
  futures_executor::block_on(async {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
    let purpose = ArtifactPurpose::parse("auv.test.read").unwrap();
    let content_type = ContentType::parse("application/json").unwrap();
    let artifact = NewArtifact::from_bytes(purpose.clone(), content_type.clone(), Attributes::empty(), br#"{"value":42}"#.to_vec()).unwrap();
    let metadata = root.in_scope(|| auv_tracing::emit_artifact!(artifact)).await.expect("artifact write").expect("recording enabled");
    dispatch.flush().await.expect("flush");
    let snapshot = store.load_snapshot(run_id).await.expect("snapshot read").expect("run snapshot");

    let bytes = read_artifact_bytes(store.as_ref(), &snapshot, metadata.uri(), &purpose, &content_type, ByteLength::new(1024).unwrap())
      .await
      .expect("validated bytes");

    assert_eq!(bytes, br#"{"value":42}"#);
  });
}

#[test]
fn rejects_metadata_larger_than_the_consumer_bound_before_opening_the_body() {
  futures_executor::block_on(async {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
    let purpose = ArtifactPurpose::parse("auv.test.read").unwrap();
    let content_type = ContentType::parse("application/octet-stream").unwrap();
    let artifact = NewArtifact::from_bytes(purpose.clone(), content_type.clone(), Attributes::empty(), vec![0; 16]).unwrap();
    let metadata = root.in_scope(|| auv_tracing::emit_artifact!(artifact)).await.expect("artifact write").expect("recording enabled");
    dispatch.flush().await.expect("flush");
    let snapshot = store.load_snapshot(run_id).await.expect("snapshot read").expect("run snapshot");

    let error = read_artifact_bytes(store.as_ref(), &snapshot, metadata.uri(), &purpose, &content_type, ByteLength::new(8).unwrap())
      .await
      .expect_err("consumer bound");

    assert!(matches!(
      error,
      ReadArtifactError::PayloadTooLarge {
        limit,
        actual: 16,
        ..
      } if limit.get() == 8
    ));
  });
}

#[test]
fn reads_typed_json_after_applying_the_canonical_artifact_contract() {
  futures_executor::block_on(async {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
    let purpose = ArtifactPurpose::parse("auv.test.read_json").unwrap();
    let artifact =
      NewArtifact::from_json(purpose.clone(), Attributes::empty(), ByteLength::new(1024).unwrap(), &serde_json::json!({ "value": 42 }))
        .unwrap();
    let metadata = root.in_scope(|| auv_tracing::emit_artifact!(artifact)).await.expect("artifact write").expect("recording enabled");
    dispatch.flush().await.expect("flush");
    let snapshot = store.load_snapshot(run_id).await.expect("snapshot read").expect("run snapshot");

    let value = read_json_artifact::<JsonFixture>(store.as_ref(), &snapshot, metadata.uri(), &purpose, ByteLength::new(1024).unwrap())
      .await
      .expect("typed JSON");

    assert_eq!(value, JsonFixture { value: 42 });
  });
}
