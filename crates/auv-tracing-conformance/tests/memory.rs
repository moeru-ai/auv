use std::future::Future;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};

use auv_tracing::{
  AuthorityId, EventId, EventName, EventOccurred, EventSchema, IdempotencyKey, JsonPayload, MemoryRunStore, RunCommitRequest, RunId,
  RunMutation, RunRevision, RunStore, Timestamp,
};
use auv_tracing_conformance::{assert_gap_contract, assert_store_contract};

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
