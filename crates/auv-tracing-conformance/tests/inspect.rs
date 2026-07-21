#![forbid(unsafe_code)]

use std::io;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};

use auv_inspect_server::router_with_artifact_origin;
use auv_tracing::{ArtifactId, ArtifactWriteError, AuthorityId, EventId, IdempotencyKey, MemoryRunStore, RunId, RunStore};
use auv_tracing_conformance::{artifact_request, assert_gap_contract, assert_store_contract, event_request};
use auv_tracing_inspect::InspectRunStore;
use futures_util::StreamExt;
use futures_util::io::{AsyncRead, Cursor};
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pending_and_published_artifact_identity_conflicts_have_one_stable_http_class() {
  let backing = Arc::new(MemoryRunStore::new(AuthorityId::new()));
  let server = TestAuthority::start(backing).await;
  let remote = server.connect().await;
  let run_id = RunId::new();
  let artifact_id = ArtifactId::new();
  let gate = BodyGate::new();
  let first_store = remote.clone();
  let first_request = artifact_request(remote.authority_id(), run_id, IdempotencyKey::new(), artifact_id, b"abc");
  let first_gate = gate.clone();
  let first = tokio::spawn(async move { first_store.write_artifact(first_request, Box::pin(first_gate.reader(b"abc"))).await });
  gate.wait_until_polled().await;

  let pending_polls = Arc::new(AtomicUsize::new(0));
  let pending = remote
    .write_artifact(
      artifact_request(remote.authority_id(), run_id, IdempotencyKey::new(), artifact_id, b"abc"),
      Box::pin(PollProbe {
        polls: pending_polls.clone(),
      }),
    )
    .await
    .expect_err("a live draft owns the artifact identity");

  assert_eq!(pending, ArtifactWriteError::Rejected("auv.inspect.artifact_identity_conflict".parse().expect("artifact conflict code")));
  assert_eq!(pending_polls.load(Ordering::SeqCst), 0);

  gate.release();
  first.await.expect("pending publication task").expect("competing artifact publication");

  let published_polls = Arc::new(AtomicUsize::new(0));
  let published = remote
    .write_artifact(
      artifact_request(remote.authority_id(), run_id, IdempotencyKey::new(), artifact_id, b"abc"),
      Box::pin(PollProbe {
        polls: published_polls.clone(),
      }),
    )
    .await
    .expect_err("a published artifact keeps its identity");

  assert_eq!(published, pending);
  assert_eq!(published_polls.load(Ordering::SeqCst), 0);
}

#[derive(Clone)]
struct BodyGate {
  entered: Arc<AtomicBool>,
  released: Arc<AtomicBool>,
  waker: Arc<Mutex<Option<Waker>>>,
}

impl BodyGate {
  fn new() -> Self {
    Self {
      entered: Arc::new(AtomicBool::new(false)),
      released: Arc::new(AtomicBool::new(false)),
      waker: Arc::new(Mutex::new(None)),
    }
  }

  fn reader(&self, bytes: &[u8]) -> GatedBody {
    GatedBody {
      gate: self.clone(),
      inner: Cursor::new(bytes.to_vec()),
    }
  }

  async fn wait_until_polled(&self) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
      while !self.entered.load(Ordering::SeqCst) {
        tokio::task::yield_now().await;
      }
    })
    .await
    .expect("pending artifact body must be polled");
  }

  fn release(&self) {
    self.released.store(true, Ordering::SeqCst);
    if let Some(waker) = self.waker.lock().expect("body gate waker lock").take() {
      waker.wake();
    }
  }
}

struct GatedBody {
  gate: BodyGate,
  inner: Cursor<Vec<u8>>,
}

impl AsyncRead for GatedBody {
  fn poll_read(mut self: Pin<&mut Self>, context: &mut Context<'_>, buffer: &mut [u8]) -> Poll<io::Result<usize>> {
    if !self.gate.released.load(Ordering::SeqCst) {
      let mut waker = self.gate.waker.lock().expect("body gate waker lock");
      if !self.gate.released.load(Ordering::SeqCst) {
        *waker = Some(context.waker().clone());
        self.gate.entered.store(true, Ordering::SeqCst);
        return Poll::Pending;
      }
    }
    Pin::new(&mut self.inner).poll_read(context, buffer)
  }
}

struct PollProbe {
  polls: Arc<AtomicUsize>,
}

impl AsyncRead for PollProbe {
  fn poll_read(self: Pin<&mut Self>, _context: &mut Context<'_>, _buffer: &mut [u8]) -> Poll<io::Result<usize>> {
    self.polls.fetch_add(1, Ordering::SeqCst);
    Poll::Ready(Ok(0))
  }
}
