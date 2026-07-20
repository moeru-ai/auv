use std::future::Future;
use std::io;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};

use auv_tracing::{
  ArtifactId, ArtifactPurpose, ArtifactWriteError, Attributes, AuthorityId, ByteLength, ContentType, EventId, EventName, EventOccurred,
  EventSchema, IdempotencyKey, JsonPayload, MemoryRunStore, RunCommitRequest, RunId, RunMutation, RunRevision, RunStore, Sha256Digest,
  StoreArtifactRequest, Timestamp,
};
use auv_tracing_conformance::{assert_gap_contract, assert_store_contract};
use futures_io::AsyncRead;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};

#[test]
fn memory_store_satisfies_authority_contract() {
  block_on(assert_store_contract(|| Arc::new(MemoryRunStore::new(AuthorityId::new()))));
}

#[test]
fn memory_store_reports_retention_gaps() {
  let store = Arc::new(MemoryRunStore::with_history_limit(AuthorityId::new(), NonZeroUsize::new(2).expect("history limit is non-zero")));
  block_on(assert_gap_contract(store, |_run_id| async move {}));
}

#[test]
fn retention_preserves_snapshot_and_idempotency_state() {
  block_on(async {
    let store = MemoryRunStore::with_history_limit(AuthorityId::new(), NonZeroUsize::new(2).unwrap());
    let run_id = RunId::new();
    let first_key = IdempotencyKey::new();
    let first = store.commit(event_request(&store, run_id, first_key, "first")).await.unwrap();
    store.commit(event_request(&store, run_id, IdempotencyKey::new(), "second")).await.unwrap();
    store.commit(event_request(&store, run_id, IdempotencyKey::new(), "third")).await.unwrap();

    assert_eq!(store.lookup_commit(run_id, first_key).await.unwrap(), Some(first));
    let snapshot = store.load_snapshot(run_id).await.unwrap().expect("retained run has a snapshot");
    assert_eq!(snapshot.through_revision(), RunRevision::new(3).unwrap());
    assert_eq!(snapshot.events().len(), 3);

    let page = store.commits_after(run_id, RunRevision::new(1).unwrap(), auv_tracing::PageLimit::new(2).unwrap()).await.unwrap();
    assert_eq!(page.commits().iter().map(|commit| commit.revision().get()).collect::<Vec<_>>(), vec![2, 3]);
  });
}

#[test]
fn pending_subscription_is_woken_by_a_later_commit() {
  block_on(async {
    let store = MemoryRunStore::new(AuthorityId::new());
    let run_id = RunId::new();
    let mut subscription = store.subscribe(run_id, RunRevision::new(0).unwrap()).await.unwrap();
    let mut next = Box::pin(subscription.next());
    let wake = Arc::new(CountingWake::default());

    assert!(poll_with(next.as_mut(), &wake).is_pending());
    let committed = store.commit(event_request(&store, run_id, IdempotencyKey::new(), "wake")).await.unwrap();
    assert_eq!(wake.count(), 1);
    assert_eq!(next.await.unwrap().unwrap(), committed);
  });
}

#[test]
fn subscription_repoll_replaces_its_waker_slot() {
  block_on(async {
    let store = MemoryRunStore::new(AuthorityId::new());
    let run_id = RunId::new();
    let mut subscription = store.subscribe(run_id, RunRevision::new(0).unwrap()).await.unwrap();
    let mut next = Box::pin(subscription.next());
    let first = Arc::new(CountingWake::default());
    let replacement = Arc::new(CountingWake::default());

    assert!(poll_with(next.as_mut(), &first).is_pending());
    assert!(poll_with(next.as_mut(), &replacement).is_pending());
    store.commit(event_request(&store, run_id, IdempotencyKey::new(), "replace")).await.unwrap();

    assert_eq!(first.count(), 0, "the replaced executor waker remained registered");
    assert_eq!(replacement.count(), 1);
  });
}

#[test]
fn dropped_subscription_removes_its_waker_slot() {
  block_on(async {
    let store = MemoryRunStore::new(AuthorityId::new());
    let run_id = RunId::new();
    let mut subscription = store.subscribe(run_id, RunRevision::new(0).unwrap()).await.unwrap();
    let mut next = Box::pin(subscription.next());
    let wake = Arc::new(CountingWake::default());
    assert!(poll_with(next.as_mut(), &wake).is_pending());
    drop(next);
    drop(subscription);

    store.commit(event_request(&store, run_id, IdempotencyKey::new(), "drop")).await.unwrap();
    assert_eq!(wake.count(), 0, "a dropped subscription left a stale executor waker");
  });
}

#[test]
fn pending_equal_artifact_waits_without_polling_replacement_body() {
  block_on(async {
    let store = MemoryRunStore::new(AuthorityId::new());
    let run_id = RunId::new();
    let bytes = b"reserved artifact".to_vec();
    let request = artifact_request(&store, run_id, IdempotencyKey::new(), ArtifactId::new(), &bytes);
    let (owner_body, gate) = GatedBody::pending(bytes.clone(), false);
    let mut owner = store.write_artifact(request.clone(), Box::pin(owner_body));
    assert!(poll_with(owner.as_mut(), &Arc::new(CountingWake::default())).is_pending());

    let replacement = ProbeBody::new(bytes);
    let replacement_polled = replacement.polled();
    let mut replay = store.write_artifact(request, Box::pin(replacement));
    assert!(poll_with(replay.as_mut(), &Arc::new(CountingWake::default())).is_pending());
    assert!(!replacement_polled.load(Ordering::SeqCst));

    gate.release();
    let committed = owner.await.unwrap();
    assert_eq!(replay.await.unwrap(), committed);
    assert!(!replacement_polled.load(Ordering::SeqCst));
  });
}

#[test]
fn pending_artifact_metadata_conflicts_reject_without_polling() {
  block_on(async {
    let store = MemoryRunStore::new(AuthorityId::new());
    let run_id = RunId::new();
    let key = IdempotencyKey::new();
    let artifact_id = ArtifactId::new();
    let bytes = b"owner bytes".to_vec();
    let request = artifact_request(&store, run_id, key, artifact_id, &bytes);
    let (owner_body, _gate) = GatedBody::pending(bytes.clone(), false);
    let mut owner = store.write_artifact(request, Box::pin(owner_body));
    assert!(poll_with(owner.as_mut(), &Arc::new(CountingWake::default())).is_pending());

    let changed = b"other bytes".to_vec();
    let changed_request = artifact_request(&store, run_id, key, artifact_id, &changed);
    let changed_body = ProbeBody::new(changed);
    let changed_polled = changed_body.polled();
    let mut changed_write = store.write_artifact(changed_request, Box::pin(changed_body));
    assert!(matches!(
      poll_with(changed_write.as_mut(), &Arc::new(CountingWake::default())),
      Poll::Ready(Err(ArtifactWriteError::IdempotencyMismatch))
    ));
    assert!(!changed_polled.load(Ordering::SeqCst));
  });
}

#[test]
fn pending_artifact_uri_conflict_rejects_without_polling() {
  block_on(async {
    let store = MemoryRunStore::new(AuthorityId::new());
    let run_id = RunId::new();
    let artifact_id = ArtifactId::new();
    let bytes = b"owner bytes".to_vec();
    let request = artifact_request(&store, run_id, IdempotencyKey::new(), artifact_id, &bytes);
    let (owner_body, _gate) = GatedBody::pending(bytes.clone(), false);
    let mut owner = store.write_artifact(request, Box::pin(owner_body));
    assert!(poll_with(owner.as_mut(), &Arc::new(CountingWake::default())).is_pending());

    let conflicting = artifact_request(&store, run_id, IdempotencyKey::new(), artifact_id, &bytes);
    let conflicting_body = ProbeBody::new(bytes);
    let conflicting_polled = conflicting_body.polled();
    let mut conflicting_write = store.write_artifact(conflicting, Box::pin(conflicting_body));
    assert!(matches!(
      poll_with(conflicting_write.as_mut(), &Arc::new(CountingWake::default())),
      Poll::Ready(Err(ArtifactWriteError::Rejected(_)))
    ));
    assert!(!conflicting_polled.load(Ordering::SeqCst));
  });
}

#[test]
fn cancelled_artifact_owner_releases_waiting_equal_request() {
  block_on(async {
    let store = MemoryRunStore::new(AuthorityId::new());
    let run_id = RunId::new();
    let bytes = b"retry after cancel".to_vec();
    let request = artifact_request(&store, run_id, IdempotencyKey::new(), ArtifactId::new(), &bytes);
    let (owner_body, _gate) = GatedBody::pending(bytes.clone(), false);
    let mut owner = store.write_artifact(request.clone(), Box::pin(owner_body));
    assert!(poll_with(owner.as_mut(), &Arc::new(CountingWake::default())).is_pending());

    let replacement = ProbeBody::new(bytes);
    let replacement_polled = replacement.polled();
    let waiter_wake = Arc::new(CountingWake::default());
    let mut waiter = store.write_artifact(request, Box::pin(replacement));
    assert!(poll_with(waiter.as_mut(), &waiter_wake).is_pending());
    assert!(!replacement_polled.load(Ordering::SeqCst));

    drop(owner);
    assert_eq!(waiter_wake.count(), 1);
    waiter.await.unwrap();
    assert!(replacement_polled.load(Ordering::SeqCst));
  });
}

#[test]
fn failed_artifact_owner_releases_waiting_equal_request() {
  block_on(async {
    let store = MemoryRunStore::new(AuthorityId::new());
    let run_id = RunId::new();
    let bytes = b"retry after failure".to_vec();
    let request = artifact_request(&store, run_id, IdempotencyKey::new(), ArtifactId::new(), &bytes);
    let (owner_body, gate) = GatedBody::pending(bytes.clone(), true);
    let mut owner = store.write_artifact(request.clone(), Box::pin(owner_body));
    assert!(poll_with(owner.as_mut(), &Arc::new(CountingWake::default())).is_pending());

    let replacement = ProbeBody::new(bytes);
    let replacement_polled = replacement.polled();
    let waiter_wake = Arc::new(CountingWake::default());
    let mut waiter = store.write_artifact(request, Box::pin(replacement));
    assert!(poll_with(waiter.as_mut(), &waiter_wake).is_pending());
    assert!(!replacement_polled.load(Ordering::SeqCst));

    gate.release();
    assert!(matches!(owner.await, Err(ArtifactWriteError::Unavailable(_))));
    assert_eq!(waiter_wake.count(), 1);
    waiter.await.unwrap();
    assert!(replacement_polled.load(Ordering::SeqCst));
  });
}

#[test]
fn integrity_failed_artifact_owner_releases_waiting_equal_request() {
  block_on(async {
    let store = MemoryRunStore::new(AuthorityId::new());
    let run_id = RunId::new();
    let bytes = b"expected artifact".to_vec();
    let request = artifact_request(&store, run_id, IdempotencyKey::new(), ArtifactId::new(), &bytes);
    let (owner_body, gate) = GatedBody::pending(b"wrong artifact!!!".to_vec(), false);
    let mut owner = store.write_artifact(request.clone(), Box::pin(owner_body));
    assert!(poll_with(owner.as_mut(), &Arc::new(CountingWake::default())).is_pending());

    let replacement = ProbeBody::new(bytes);
    let replacement_polled = replacement.polled();
    let waiter_wake = Arc::new(CountingWake::default());
    let mut waiter = store.write_artifact(request, Box::pin(replacement));
    assert!(poll_with(waiter.as_mut(), &waiter_wake).is_pending());
    assert!(!replacement_polled.load(Ordering::SeqCst));

    gate.release();
    assert!(matches!(owner.await, Err(ArtifactWriteError::Integrity(_))));
    assert_eq!(waiter_wake.count(), 1);
    waiter.await.unwrap();
    assert!(replacement_polled.load(Ordering::SeqCst));
  });
}

#[test]
fn artifact_body_length_boundaries_are_exact() {
  block_on(async {
    let store = MemoryRunStore::new(AuthorityId::new());
    let run_id = RunId::new();
    for bytes in [Vec::new(), b"exact".to_vec()] {
      let artifact_id = ArtifactId::new();
      store
        .write_artifact(
          artifact_request(&store, run_id, IdempotencyKey::new(), artifact_id, &bytes),
          Box::pin(ProbeBody::new(bytes.clone())),
        )
        .await
        .unwrap();
      let mut reader = store.open_artifact(auv_tracing::ArtifactUri::from_ids(run_id, artifact_id)).await.unwrap();
      let mut collected = Vec::new();
      while let Some(chunk) = reader.next().await {
        collected.extend_from_slice(&chunk.unwrap());
      }
      assert_eq!(collected, bytes);
    }

    let exact = b"limit".to_vec();
    let mut too_long = exact.clone();
    too_long.push(b'!');
    let body = ProbeBody::new(too_long);
    let bytes_read = body.bytes_read();
    let request = StoreArtifactRequest::new(
      store.authority_id(),
      run_id,
      IdempotencyKey::new(),
      ArtifactId::new(),
      None,
      artifact_purpose(),
      content_type(),
      ByteLength::new(exact.len() as u64).unwrap(),
      digest(&exact),
      Attributes::empty(),
    );
    assert!(matches!(store.write_artifact(request, Box::pin(body)).await, Err(ArtifactWriteError::Integrity(_))));
    assert_eq!(bytes_read.load(Ordering::SeqCst), exact.len() + 1);
  });
}

fn block_on<F: Future>(future: F) -> F::Output {
  let mut future = Box::pin(future);
  let waker = Waker::from(Arc::new(ThreadWake(thread::current())));
  let mut context = Context::from_waker(&waker);
  loop {
    match future.as_mut().poll(&mut context) {
      Poll::Ready(output) => return output,
      Poll::Pending => thread::park(),
    }
  }
}

struct ThreadWake(Thread);

impl Wake for ThreadWake {
  fn wake(self: Arc<Self>) {
    self.0.unpark();
  }

  fn wake_by_ref(self: &Arc<Self>) {
    self.0.unpark();
  }
}

#[derive(Default)]
struct CountingWake(AtomicUsize);

impl CountingWake {
  fn count(&self) -> usize {
    self.0.load(Ordering::SeqCst)
  }
}

impl Wake for CountingWake {
  fn wake(self: Arc<Self>) {
    self.0.fetch_add(1, Ordering::SeqCst);
  }

  fn wake_by_ref(self: &Arc<Self>) {
    self.0.fetch_add(1, Ordering::SeqCst);
  }
}

fn poll_with<F: Future + ?Sized>(future: std::pin::Pin<&mut F>, wake: &Arc<CountingWake>) -> Poll<F::Output> {
  let waker = Waker::from(Arc::clone(wake));
  future.poll(&mut Context::from_waker(&waker))
}

fn artifact_request(
  store: &MemoryRunStore,
  run_id: RunId,
  key: IdempotencyKey,
  artifact_id: ArtifactId,
  bytes: &[u8],
) -> StoreArtifactRequest {
  StoreArtifactRequest::new(
    store.authority_id(),
    run_id,
    key,
    artifact_id,
    None,
    artifact_purpose(),
    content_type(),
    ByteLength::new(bytes.len() as u64).unwrap(),
    digest(bytes),
    Attributes::empty(),
  )
}

fn artifact_purpose() -> ArtifactPurpose {
  ArtifactPurpose::parse("auv.test.memory_artifact").unwrap()
}

fn content_type() -> ContentType {
  ContentType::parse("application/octet-stream").unwrap()
}

fn digest(bytes: &[u8]) -> Sha256Digest {
  Sha256Digest::new(Sha256::digest(bytes).into())
}

struct ProbeBody {
  bytes: Vec<u8>,
  offset: usize,
  polled: Arc<AtomicBool>,
  bytes_read: Arc<AtomicUsize>,
}

impl ProbeBody {
  fn new(bytes: Vec<u8>) -> Self {
    Self {
      bytes,
      offset: 0,
      polled: Arc::new(AtomicBool::new(false)),
      bytes_read: Arc::new(AtomicUsize::new(0)),
    }
  }

  fn polled(&self) -> Arc<AtomicBool> {
    Arc::clone(&self.polled)
  }

  fn bytes_read(&self) -> Arc<AtomicUsize> {
    Arc::clone(&self.bytes_read)
  }
}

impl AsyncRead for ProbeBody {
  fn poll_read(mut self: Pin<&mut Self>, _cx: &mut Context<'_>, buffer: &mut [u8]) -> Poll<io::Result<usize>> {
    self.polled.store(true, Ordering::SeqCst);
    let count = buffer.len().min(self.bytes.len().saturating_sub(self.offset));
    buffer[..count].copy_from_slice(&self.bytes[self.offset..self.offset + count]);
    self.offset += count;
    self.bytes_read.fetch_add(count, Ordering::SeqCst);
    Poll::Ready(Ok(count))
  }
}

struct GatedBody {
  bytes: Vec<u8>,
  offset: usize,
  fail: bool,
  gate: Arc<GateState>,
}

impl GatedBody {
  fn pending(bytes: Vec<u8>, fail: bool) -> (Self, Arc<GateState>) {
    let gate = Arc::new(GateState::default());
    (
      Self {
        bytes,
        offset: 0,
        fail,
        gate: Arc::clone(&gate),
      },
      gate,
    )
  }
}

impl AsyncRead for GatedBody {
  fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buffer: &mut [u8]) -> Poll<io::Result<usize>> {
    if !self.gate.released.load(Ordering::SeqCst) {
      *self.gate.waker.lock().unwrap() = Some(cx.waker().clone());
      return Poll::Pending;
    }
    if self.fail {
      return Poll::Ready(Err(io::Error::new(io::ErrorKind::ConnectionReset, "injected gated failure")));
    }
    let count = buffer.len().min(self.bytes.len().saturating_sub(self.offset));
    buffer[..count].copy_from_slice(&self.bytes[self.offset..self.offset + count]);
    self.offset += count;
    Poll::Ready(Ok(count))
  }
}

#[derive(Default)]
struct GateState {
  released: AtomicBool,
  waker: Mutex<Option<Waker>>,
}

impl GateState {
  fn release(&self) {
    self.released.store(true, Ordering::SeqCst);
    if let Some(waker) = self.waker.lock().unwrap().take() {
      waker.wake();
    }
  }
}

fn event_request(store: &MemoryRunStore, run_id: RunId, key: IdempotencyKey, value: &str) -> RunCommitRequest {
  let schema = EventSchema::new(EventName::parse("auv.test.retention").unwrap(), 1).unwrap();
  let event = EventOccurred::new(
    EventId::new(),
    None,
    Timestamp::new(1, 0).unwrap(),
    schema,
    JsonPayload::encode(&serde_json::json!({ "value": value })).unwrap(),
  );
  RunCommitRequest::new(store.authority_id(), run_id, key, vec![RunMutation::EmitEvent(event)]).unwrap()
}
