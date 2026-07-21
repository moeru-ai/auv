#![forbid(unsafe_code)]

use std::num::NonZeroUsize;
use std::sync::Arc;

use auv_inspect_server::router_with_artifact_origin;
use auv_tracing::{ArtifactId, AuthorityId, EventId, IdempotencyKey, MemoryRunStore, RunId, RunStore};
use auv_tracing_conformance::{artifact_request, assert_gap_contract, assert_store_contract, event_request};
use auv_tracing_inspect::InspectRunStore;
use futures_util::StreamExt;
use futures_util::io::Cursor;
use tokio::net::TcpListener;

struct TestAuthority {
  base_url: String,
  task: tokio::task::JoinHandle<()>,
}

impl TestAuthority {
  async fn start(store: Arc<dyn RunStore>) -> Self {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind Inspect conformance listener");
    let address = listener.local_addr().expect("Inspect conformance listener address");
    let base_url = format!("http://{address}/");
    let app =
      router_with_artifact_origin(store, base_url.parse().expect("Inspect conformance base URL")).expect("Inspect conformance router");
    let task = tokio::spawn(async move {
      axum::serve(listener, app).await.expect("Inspect conformance server");
    });
    Self { base_url, task }
  }

  async fn connect(&self) -> InspectRunStore {
    InspectRunStore::connect(self.base_url.parse().expect("Inspect conformance base URL")).await.expect("connect Inspect run store")
  }
}

impl Drop for TestAuthority {
  fn drop(&mut self) {
    self.task.abort();
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn inspect_store_satisfies_authority_contract_over_http() {
  let backing = Arc::new(MemoryRunStore::new(AuthorityId::new()));
  let server = TestAuthority::start(backing.clone()).await;
  let first = Arc::new(server.connect().await);
  let second = server.connect().await;

  assert_eq!(first.authority_id(), backing.authority_id());
  assert_eq!(second.authority_id(), backing.authority_id());

  let remote: Arc<dyn RunStore> = first;
  assert_store_contract(|| remote.clone()).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn inspect_store_round_trips_binary_only_through_artifact_endpoints() {
  const MARKER: &[u8] = b"AUV_BINARY_BODY";

  let backing = Arc::new(MemoryRunStore::new(AuthorityId::new()));
  let server = TestAuthority::start(backing).await;
  let remote = server.connect().await;
  let run_id = RunId::new();
  let artifact_id = ArtifactId::new();
  let mut bytes = vec![0, 0xff, 0x80];
  bytes.extend_from_slice(MARKER);
  let request = artifact_request(remote.authority_id(), run_id, IdempotencyKey::new(), artifact_id, &bytes);

  let published = remote.write_artifact(request, Box::pin(Cursor::new(bytes.clone()))).await.expect("publish binary artifact");
  let commit_json = serde_json::to_vec(published.commit()).expect("artifact commit JSON");
  assert!(std::str::from_utf8(&commit_json).is_ok());
  assert!(!commit_json.windows(MARKER.len()).any(|window| window == MARKER));

  let mut reader =
    remote.open_artifact(auv_tracing::ArtifactUri::from_ids(run_id, artifact_id)).await.expect("open binary artifact over Inspect");
  let mut received = Vec::new();
  while let Some(chunk) = reader.next().await {
    received.extend_from_slice(&chunk.expect("read binary artifact chunk"));
  }
  assert_eq!(received, bytes);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn inspect_store_reports_retention_gaps_over_http_and_sse() {
  let backing = Arc::new(MemoryRunStore::with_history_limit(AuthorityId::new(), NonZeroUsize::new(3).expect("history limit is non-zero")));
  let server = TestAuthority::start(backing.clone()).await;
  let remote: Arc<dyn RunStore> = Arc::new(server.connect().await);

  assert_gap_contract(remote, move |run_id| async move {
    let request = event_request(backing.authority_id(), run_id, EventId::new(), IdempotencyKey::new(), "retention hook");
    backing.commit(request).await.expect("advance retained Inspect history");
  })
  .await;
}
