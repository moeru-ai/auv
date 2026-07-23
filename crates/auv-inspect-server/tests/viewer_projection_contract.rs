use std::str::FromStr;

use auv_inspect_model::InspectDocument;
use auv_tracing::{
  ArtifactId, ArtifactPurpose, Attributes, AuthorityId, ByteLength, ContentType, EventId, EventName, EventOccurred, EventSchema,
  IdempotencyKey, JsonPayload, MemoryRunStore, RunCommitRequest, RunId, RunMutation, RunStore, Sha256Digest, SpanId, SpanName, SpanStarted,
  StoreArtifactRequest, Timestamp,
};
use futures_util::StreamExt;
use futures_util::io::Cursor;

const AUTHORITY: &str = "019f8b1e-4b2d-7a00-8f00-0000000000aa";
const RUN: &str = "019f8b1e-4b2d-7a00-8f00-000000000001";
const SPAN: &str = "019f8b1e-4b2d-7a00-8f00-000000000011";

struct TestAuthority {
  store: MemoryRunStore,
  run_id: RunId,
  next_key: u64,
}

impl TestAuthority {
  fn new() -> Self {
    Self {
      store: MemoryRunStore::new(AUTHORITY.parse().unwrap()),
      run_id: RUN.parse().unwrap(),
      next_key: 1,
    }
  }

  async fn commit(&mut self, mutation: RunMutation) {
    let key = format!("019f8b1e-4b2d-7a00-8f00-{:012x}", self.next_key);
    self.next_key += 1;
    self
      .store
      .commit(RunCommitRequest::new(self.store.authority_id(), self.run_id, key.parse().unwrap(), vec![mutation]).unwrap())
      .await
      .unwrap();
  }

  async fn commit_span_start(&mut self) {
    self
      .commit(RunMutation::StartSpan(SpanStarted::new(
        SPAN.parse().unwrap(),
        None,
        None,
        SpanName::parse("auv.test.root").unwrap(),
        Timestamp::new(1, 0).unwrap(),
        Attributes::empty(),
      )))
      .await;
  }

  async fn commit_event(&mut self) {
    self
      .commit(RunMutation::EmitEvent(EventOccurred::new(
        EventId::from_str("019f8b1e-4b2d-7a00-8f00-000000000021").unwrap(),
        Some(SpanId::from_str(SPAN).unwrap()),
        Timestamp::new(2, 0).unwrap(),
        EventSchema::new(EventName::parse("auv.test.event").unwrap(), 1).unwrap(),
        JsonPayload::from_str(r#"{"message":"kept"}"#).unwrap(),
      )))
      .await;
  }

  async fn snapshot(&self) -> auv_tracing::RunSnapshot {
    self.store.load_snapshot(self.run_id).await.unwrap().unwrap()
  }

  async fn publish_artifact(&mut self) {
    let key = format!("019f8b1e-4b2d-7a00-8f00-{:012x}", self.next_key);
    self.next_key += 1;
    self
      .store
      .write_artifact(
        StoreArtifactRequest::new(
          self.store.authority_id(),
          self.run_id,
          key.parse().unwrap(),
          ArtifactId::from_str("019f8b1e-4b2d-7a00-8f00-000000000041").unwrap(),
          Some(SpanId::from_str(SPAN).unwrap()),
          ArtifactPurpose::parse("auv.test.output").unwrap(),
          ContentType::parse("text/plain").unwrap(),
          ByteLength::new(5).unwrap(),
          Sha256Digest::from_str("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824").unwrap(),
          Attributes::empty(),
        ),
        Box::pin(Cursor::new(b"hello".to_vec())),
      )
      .await
      .unwrap();
  }

  async fn subscribe(&self, after: auv_tracing::RunRevision) -> Vec<auv_tracing::RunCommit> {
    let mut subscription = self.store.subscribe(self.run_id, after).await.unwrap();
    vec![tokio::time::timeout(std::time::Duration::from_secs(1), subscription.next()).await.unwrap().unwrap().unwrap()]
  }
}

#[tokio::test]
async fn snapshot_then_subscription_does_not_drop_intervening_commit() {
  let mut authority = TestAuthority::new();
  authority.commit_span_start().await;
  let initial = authority.snapshot().await;
  authority.commit_event().await;
  let updates = authority.subscribe(initial.through_revision()).await;
  assert_eq!(updates.first().unwrap().revision().get(), initial.through_revision().get() + 1);
}

#[tokio::test]
async fn inspect_document_is_a_snapshot_only_projection_without_inferred_outcomes() {
  let mut authority = TestAuthority::new();
  authority.commit_span_start().await;
  authority.commit_event().await;
  authority.publish_artifact().await;
  let snapshot = authority.snapshot().await;

  let document = InspectDocument::from(&snapshot);
  let value = serde_json::to_value(&document).unwrap();

  assert_eq!(value["authority_id"], AUTHORITY);
  assert_eq!(value["run_id"], RUN);
  assert_eq!(value["through_revision"], 3);
  assert_eq!(value["spans"][0]["span_id"], SPAN);
  assert_eq!(value["spans"][0]["ended_at"], serde_json::Value::Null);
  assert_eq!(value["events"][0]["schema"]["name"], "auv.test.event");
  assert_eq!(value["events"][0]["payload"], serde_json::json!({"message":"kept"}));
  assert_eq!(value["artifacts"][0]["uri"], format!("auv://runs/{RUN}/artifacts/019f8b1e-4b2d-7a00-8f00-000000000041"));
  assert_eq!(value["artifacts"][0]["purpose"], "auv.test.output");
  assert_eq!(value["artifacts"][0]["content_type"], "text/plain");
  assert_eq!(value["artifacts"][0]["byte_length"], 5);

  let encoded = serde_json::to_string(&document).unwrap();
  for forbidden in [
    "filesystem_path",
    "preferred_filename",
    "role",
    "summary",
    "status",
    "result",
    "verification",
    "trace_id",
  ] {
    assert!(!encoded.contains(&format!(r#""{forbidden}""#)), "document exposed {forbidden}: {encoded}");
  }
}

#[test]
fn viewer_uses_snapshot_then_revision_sse_without_websocket_or_status_inference() {
  let source = include_str!("../viewer/src/viewer.ts");
  let vite = include_str!("../viewer/vite.config.ts");

  assert!(source.contains("/snapshot"));
  assert!(source.contains("/commits/stream?after_revision="));
  assert!(source.contains("new EventSource"));
  assert!(source.contains("addEventListener(\"gap\""));
  assert!(source.contains("validateSnapshot"));
  assert!(source.contains("MAX_RECOVERY_ATTEMPTS"));
  assert!(source.contains("Object.keys(value).length !== 1"));
  assert!(!source.contains("new WebSocket"));
  assert!(!source.contains("status_code"));
  assert!(!source.contains("running"));
  assert!(!source.contains("success"));
  assert!(source.contains("addEventListener(\"open\""));
  assert!(vite.contains(r#""/v1""#));
  assert!(!vite.contains(r#""/runs""#));
  assert!(!vite.contains(r#""/write""#));
}

#[test]
fn ids_used_by_projection_contract_are_valid() {
  AuthorityId::from_str(AUTHORITY).unwrap();
  RunId::from_str(RUN).unwrap();
  SpanId::from_str(SPAN).unwrap();
  IdempotencyKey::from_str("019f8b1e-4b2d-7a00-8f00-000000000001").unwrap();
}
