#![cfg(feature = "memory-store")]

mod support;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::task::{Context as TaskContext, Poll};

use auv_tracing::{Attributes, Context, DispatchStage, EventPayload, RunId, RunMutation, RunStore, SpanSpec, configure, dispatcher};
use serde::{Deserialize, Serialize};
use support::{
  ControlledStore, DropFirstTaskSpawner, FailFirstSpawner, InlineSpawner, PanicFirstSpawner, TestDispatch, WAIT_TIMEOUT, block_on_timeout,
};

struct TestSpan;

impl SpanSpec for TestSpan {
  const NAME: &'static str = "auv.test.operation";

  fn attributes(&self) -> Attributes {
    Attributes::empty()
  }
}

struct InvalidSpan;

impl SpanSpec for InvalidSpan {
  const NAME: &'static str = "invalid";

  fn attributes(&self) -> Attributes {
    Attributes::empty()
  }
}

struct PanickingSpan;

impl SpanSpec for PanickingSpan {
  const NAME: &'static str = "auv.test.panicking_span";

  fn attributes(&self) -> Attributes {
    panic!("span attribute preparation failed");
  }
}

struct DropPanickingSpan {
  panic_on_drop: bool,
}

impl SpanSpec for DropPanickingSpan {
  const NAME: &'static str = "auv.test.drop_panicking_span";

  fn attributes(&self) -> Attributes {
    Attributes::empty()
  }
}

impl Drop for DropPanickingSpan {
  fn drop(&mut self) {
    if self.panic_on_drop {
      self.panic_on_drop = false;
      panic!("span specification destructor failed");
    }
  }
}

#[derive(Deserialize, Serialize)]
struct TestEvent {
  value: u32,
}

impl EventPayload for TestEvent {
  const NAME: &'static str = "auv.test.event";
  const VERSION: u32 = 1;
}

struct BlockingEvent {
  value: u32,
  entered: SyncSender<()>,
  release: Receiver<()>,
}

impl Serialize for BlockingEvent {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: serde::Serializer,
  {
    self.entered.send(()).unwrap();
    self.release.recv_timeout(WAIT_TIMEOUT).expect("timed out waiting to release blocking event serialization");
    TestEvent { value: self.value }.serialize(serializer)
  }
}

impl EventPayload for BlockingEvent {
  const NAME: &'static str = TestEvent::NAME;
  const VERSION: u32 = TestEvent::VERSION;
}

struct PanickingEvent;

impl Serialize for PanickingEvent {
  fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
  where
    S: serde::Serializer,
  {
    panic!("event payload preparation failed");
  }
}

impl EventPayload for PanickingEvent {
  const NAME: &'static str = "auv.test.panicking_event";
  const VERSION: u32 = 1;
}

struct DropPanickingEvent {
  value: u32,
  panic_on_drop: bool,
}

impl Serialize for DropPanickingEvent {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: serde::Serializer,
  {
    TestEvent { value: self.value }.serialize(serializer)
  }
}

impl EventPayload for DropPanickingEvent {
  const NAME: &'static str = "auv.test.drop_panicking_event";
  const VERSION: u32 = 1;
}

impl Drop for DropPanickingEvent {
  fn drop(&mut self) {
    if self.panic_on_drop {
      self.panic_on_drop = false;
      panic!("event payload destructor failed");
    }
  }
}

struct WakeCounter(AtomicUsize);

impl futures_util::task::ArcWake for WakeCounter {
  fn wake_by_ref(counter: &Arc<Self>) {
    counter.0.fetch_add(1, Ordering::SeqCst);
  }
}

fn event_values(requests: &[auv_tracing::RunCommitRequest]) -> Vec<u32> {
  requests
    .iter()
    .map(|request| match &request.mutations()[0] {
      RunMutation::EmitEvent(event) => serde_json::from_str::<TestEvent>(event.payload().get()).unwrap().value,
      mutation => panic!("expected event mutation, received {mutation:?}"),
    })
    .collect()
}

fn assert_context_is_send_sync<T: Send + Sync>() {}

#[test]
fn context_is_clone_send_and_sync() {
  assert_context_is_send_sync::<Context>();
  let context = Context::root(RunId::new());
  let _clone = context.clone();
}

#[test]
fn root_creation_does_not_install_current_context() {
  let before = Context::current();
  let run_id = RunId::new();
  let root = Context::root(run_id);

  assert!(!root.is_enabled());
  assert_eq!(root.run_id(), Some(&run_id));
  assert_eq!(Context::current().run_id(), before.run_id());
}

#[test]
fn dispatch_without_an_active_route_stays_disabled() {
  let dispatch = configure().build().unwrap();
  let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));

  assert!(!root.is_enabled());
  assert!(root.authority_id().is_none());
}

#[test]
fn disabled_calls_do_not_create_a_run() {
  let fixture = TestDispatch::memory();
  let run_id = RunId::new();
  let root = Context::root(run_id);

  root.in_scope(|| {
    let span = auv_tracing::start_span!(TestSpan);
    assert!(!span.is_enabled());
    assert!(span.id().is_none());
    auv_tracing::emit_event!(TestEvent { value: 1 });
  });

  block_on_timeout(fixture.dispatch.flush()).unwrap();
  assert!(block_on_timeout(fixture.store.load_snapshot(run_id)).unwrap().is_none());
}

#[test]
fn enabled_root_without_emissions_creates_no_run() {
  let fixture = TestDispatch::memory();
  let run_id = RunId::new();
  let root = fixture.context(run_id);

  assert!(root.is_enabled());
  assert_eq!(root.authority_id(), Some(&fixture.store.authority_id()));
  block_on_timeout(fixture.dispatch.flush()).unwrap();
  assert!(block_on_timeout(fixture.store.load_snapshot(run_id)).unwrap().is_none());
}

#[test]
fn scoped_dispatches_nest_and_restore_on_unwind() {
  let outer = TestDispatch::memory();
  let inner = TestDispatch::memory();

  dispatcher::with_default(&outer.dispatch, || {
    assert_eq!(Context::root(RunId::new()).authority_id(), Some(&outer.store.authority_id()));
    let panic = catch_unwind(AssertUnwindSafe(|| {
      dispatcher::with_default(&inner.dispatch, || {
        assert_eq!(Context::root(RunId::new()).authority_id(), Some(&inner.store.authority_id()));
        panic!("exercise unwind restoration");
      });
    }));
    assert!(panic.is_err());
    assert_eq!(Context::root(RunId::new()).authority_id(), Some(&outer.store.authority_id()));
  });

  assert!(!Context::root(RunId::new()).is_enabled());
}

#[test]
fn current_context_nests_and_restores_on_unwind() {
  let outer = Context::root(RunId::new());
  let inner = Context::root(RunId::new());

  outer.in_scope(|| {
    assert_eq!(Context::current().run_id(), outer.run_id());
    let panic = catch_unwind(AssertUnwindSafe(|| {
      inner.in_scope(|| {
        assert_eq!(Context::current().run_id(), inner.run_id());
        panic!("exercise unwind restoration");
      });
    }));
    assert!(panic.is_err());
    assert_eq!(Context::current().run_id(), outer.run_id());
  });

  assert!(Context::current().run_id().is_none());
}

#[test]
fn context_guards_can_drop_out_of_order_without_removing_newer_frames() {
  let outer = Context::root(RunId::new());
  let inner = Context::root(RunId::new());
  let outer_guard = outer.enter();
  let inner_guard = inner.enter();

  drop(outer_guard);
  assert_eq!(Context::current().run_id(), inner.run_id());
  drop(inner_guard);
  assert!(Context::current().run_id().is_none());
}

#[test]
fn out_of_order_context_drop_followed_by_unwind_restores_empty_scope() {
  let outer = Context::root(RunId::new());
  let inner = Context::root(RunId::new());

  let panic = catch_unwind(AssertUnwindSafe(|| {
    let outer_guard = outer.enter();
    let _inner_guard = inner.enter();
    drop(outer_guard);
    panic!("exercise unwind after out-of-order drop");
  }));

  assert!(panic.is_err());
  assert!(Context::current().run_id().is_none());
}

#[test]
fn span_scope_sets_and_restores_the_current_span() {
  let fixture = TestDispatch::memory();
  let root = fixture.root();

  root.in_scope(|| {
    let span = auv_tracing::start_span!(TestSpan);
    let span_id = *span.id().expect("enabled span has an ID");
    assert!(Context::current().span_id().is_none());
    span.in_scope(|| assert_eq!(Context::current().span_id(), Some(&span_id)));
    assert!(Context::current().span_id().is_none());
  });

  block_on_timeout(fixture.dispatch.flush()).unwrap();
}

#[test]
fn authority_commits_follow_submission_order() {
  let store = ControlledStore::new();
  let first = store.block_first_commit();
  let fixture = TestDispatch::with_store(store.clone());
  let root = fixture.root();
  let run_id = *root.run_id().unwrap();

  root.in_scope(|| {
    auv_tracing::emit_event!(TestEvent { value: 1 });
    auv_tracing::emit_event!(TestEvent { value: 2 });
  });

  first.wait_until_entered();
  assert_eq!(store.commit_call_count(run_id), 1, "same-run commits must not overlap");
  first.release();
  block_on_timeout(fixture.dispatch.flush()).unwrap();
  assert_eq!(event_values(&store.committed_requests(run_id)), [1, 2]);
  assert_eq!(store.committed_revisions(run_id), [1, 2]);
}

#[test]
fn same_run_order_is_reserved_before_payload_validation() {
  let store = ControlledStore::new();
  let fixture = TestDispatch::with_store(store.clone());
  let root = fixture.root();
  let run_id = *root.run_id().unwrap();
  let (entered_tx, entered_rx) = sync_channel(1);
  let (release_tx, release_rx) = sync_channel(1);
  let first_root = root.clone();

  let first = std::thread::spawn(move || {
    first_root.in_scope(|| {
      auv_tracing::emit_event!(BlockingEvent {
        value: 1,
        entered: entered_tx,
        release: release_rx,
      });
    });
  });
  entered_rx.recv_timeout(WAIT_TIMEOUT).expect("timed out waiting for blocking event serialization to start");
  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 2 }));
  release_tx.send(()).unwrap();
  first.join().unwrap();

  block_on_timeout(fixture.dispatch.flush()).unwrap();
  assert_eq!(event_values(&store.committed_requests(run_id)), [1, 2]);
}

#[test]
fn blocked_run_does_not_block_an_independent_run() {
  let store = ControlledStore::new();
  let fixture = TestDispatch::with_store(store.clone());
  let run_a = fixture.context(RunId::new());
  let run_b = fixture.context(RunId::new());
  let run_a_id = *run_a.run_id().unwrap();
  let run_b_id = *run_b.run_id().unwrap();
  let blocked = store.block_next_commit(run_a_id);

  run_a.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  blocked.wait_until_entered();
  run_b.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 2 }));

  store.wait_until_committed(run_b_id);
  assert_eq!(event_values(&store.committed_requests(run_b_id)), [2]);
  blocked.release();
  block_on_timeout(fixture.dispatch.flush()).unwrap();
}

#[test]
fn flush_captures_its_barrier_when_called() {
  let store = ControlledStore::new();
  let fixture = TestDispatch::with_store(store.clone());
  let root = fixture.root();
  let run_id = *root.run_id().unwrap();
  let first = store.block_next_commit(run_id);
  let second = store.block_next_commit(run_id);

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  first.wait_until_entered();
  let first_flush = fixture.dispatch.flush();
  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 2 }));
  first.release();
  second.wait_until_entered();

  block_on_timeout(first_flush).unwrap();
  assert_eq!(store.committed_revisions(run_id), [1]);
  second.release();
  block_on_timeout(fixture.dispatch.flush()).unwrap();
}

#[test]
fn dropped_flush_future_does_not_block_a_later_flush() {
  let store = ControlledStore::new();
  let fixture = TestDispatch::with_store(store.clone());
  let root = fixture.root();
  let blocked = store.block_next_commit(*root.run_id().unwrap());

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  blocked.wait_until_entered();
  drop(fixture.dispatch.flush());
  let later_flush = fixture.dispatch.flush();
  blocked.release();

  block_on_timeout(later_flush).unwrap();
}

#[test]
fn dropped_flush_does_not_consume_its_later_failure() {
  let store = ControlledStore::new();
  let fixture = TestDispatch::with_store(store.clone());
  let root = fixture.root();
  let run_id = *root.run_id().unwrap();
  let blocked = store.fail_next_commit(run_id, auv_tracing::ErrorCode::parse("auv.test.commit_failure").unwrap());

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  blocked.wait_until_entered();
  let first_flush = fixture.dispatch.flush();
  drop(first_flush);
  let later_flush = fixture.dispatch.flush();
  blocked.release();

  let error = block_on_timeout(later_flush).unwrap_err();
  assert_eq!(error.failure_count().get(), 1);
  assert_eq!(error.first().stage(), DispatchStage::AuthorityCommit);
  assert_eq!(error.first().code().as_str(), "auv.test.commit_failure");
}

#[test]
fn overlapping_flushes_complete_in_call_order_not_poll_order() {
  let fixture = TestDispatch::memory();
  let root = fixture.root();
  root.in_scope(|| {
    let _span = auv_tracing::start_span!(InvalidSpan);
  });
  let mut first_flush = fixture.dispatch.flush();
  let mut second_flush = fixture.dispatch.flush();
  let waker = futures_util::task::noop_waker();
  let mut task_context = TaskContext::from_waker(&waker);

  assert!(second_flush.as_mut().poll(&mut task_context).is_pending());
  let Poll::Ready(Err(first_error)) = first_flush.as_mut().poll(&mut task_context) else {
    panic!("the first flush must consume the ready failure interval");
  };
  assert_eq!(first_error.first().stage(), DispatchStage::Encode);
  assert!(matches!(second_flush.as_mut().poll(&mut task_context), Poll::Ready(Ok(()))));
}

#[test]
fn dropping_ready_front_flush_wakes_successor_and_preserves_interval() {
  let fixture = TestDispatch::memory();
  let root = fixture.root();
  root.in_scope(|| {
    let _span = auv_tracing::start_span!(InvalidSpan);
  });
  let first_flush = fixture.dispatch.flush();
  let mut second_flush = fixture.dispatch.flush();
  let counter = Arc::new(WakeCounter(AtomicUsize::new(0)));
  let waker = futures_util::task::waker_ref(&counter);
  let mut task_context = TaskContext::from_waker(&waker);

  assert!(second_flush.as_mut().poll(&mut task_context).is_pending());
  drop(first_flush);
  assert_eq!(counter.0.load(Ordering::SeqCst), 1);
  let Poll::Ready(Err(error)) = second_flush.as_mut().poll(&mut task_context) else {
    panic!("the successor must inherit the canceled front flush interval");
  };
  assert_eq!(error.first().stage(), DispatchStage::Encode);
}

#[test]
fn spawn_failure_releases_the_run_lane_and_failed_flush_interval_advances() {
  let store = ControlledStore::new();
  let fixture = TestDispatch::with_store_and_spawner(store.clone(), FailFirstSpawner::new());
  let root = fixture.root();
  let run_id = *root.run_id().unwrap();

  root.in_scope(|| {
    auv_tracing::emit_event!(TestEvent { value: 1 });
    auv_tracing::emit_event!(TestEvent { value: 2 });
  });

  let error = block_on_timeout(fixture.dispatch.flush()).unwrap_err();
  assert_eq!(error.failure_count().get(), 1);
  assert_eq!(error.first().stage(), DispatchStage::Spawn);
  assert_eq!(event_values(&store.committed_requests(run_id)), [2]);
  block_on_timeout(fixture.dispatch.flush()).unwrap();
}

#[test]
fn spawner_panic_before_task_start_becomes_spawn_failure_and_releases_lane() {
  let store = ControlledStore::new();
  let fixture = TestDispatch::with_store_and_spawner(store.clone(), PanicFirstSpawner::new());
  let root = fixture.root();
  let run_id = *root.run_id().unwrap();

  let emission = catch_unwind(AssertUnwindSafe(|| {
    root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  }));
  assert!(emission.is_ok(), "instrumentation must contain a pre-start spawner panic");
  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 2 }));

  let error = block_on_timeout(fixture.dispatch.flush()).unwrap_err();
  assert_eq!(error.failure_count().get(), 1);
  assert_eq!(error.first().stage(), DispatchStage::Spawn);
  assert_eq!(error.first().code().as_str(), "auv.dispatch.spawn_panic");
  assert_eq!(event_values(&store.committed_requests(run_id)), [2]);
}

#[test]
fn panicking_store_future_quarantines_run_and_independent_run_continues() {
  let store = ControlledStore::new();
  let fixture = TestDispatch::with_store(store.clone());
  let root = fixture.root();
  let run_id = *root.run_id().unwrap();
  let independent_run_id = RunId::new();
  let independent = fixture.context(independent_run_id);
  let blocked = store.panic_next_commit(run_id);

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  blocked.wait_until_entered();
  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 2 }));
  blocked.release();
  independent.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 3 }));

  store.wait_until_committed(independent_run_id);
  let error = block_on_timeout(fixture.dispatch.flush()).unwrap_err();
  assert_eq!(error.failure_count().get(), 2);
  assert_eq!(error.first().stage(), DispatchStage::AuthorityCommit);
  assert_eq!(error.first().code().as_str(), "auv.dispatch.task_unwind");
  assert_eq!(store.commit_call_count(run_id), 1);
  assert!(event_values(&store.committed_requests(run_id)).is_empty());
  assert_eq!(event_values(&store.committed_requests(independent_run_id)), [3]);
}

#[test]
fn default_pool_survives_two_panicking_lane_tasks() {
  let store = ControlledStore::new();
  let fixture = TestDispatch::with_store(store.clone());
  let first_run_id = RunId::new();
  let second_run_id = RunId::new();
  let third_run_id = RunId::new();
  let (first_start, first_panic) = store.block_worker_before_panicking_next_commit(first_run_id);
  let (second_start, second_panic) = store.block_worker_before_panicking_next_commit(second_run_id);
  let first = fixture.context(first_run_id);
  let second = fixture.context(second_run_id);
  let third = fixture.context(third_run_id);

  first.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  second.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 2 }));
  first_start.wait_until_entered();
  second_start.wait_until_entered();
  first_start.release();
  first_panic.wait_until_entered();
  second_start.release();
  second_panic.wait_until_entered();

  // Both default workers are blocked inside task polling, so this accepted
  // task remains queued until the panicking tasks leave those boundaries.
  third.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 3 }));
  first_panic.release();
  second_panic.release();

  store.wait_until_committed(third_run_id);
  let error = block_on_timeout(fixture.dispatch.flush()).unwrap_err();
  assert_eq!(error.failure_count().get(), 2);
  assert_eq!(error.first().stage(), DispatchStage::AuthorityCommit);
  assert_eq!(error.first().code().as_str(), "auv.dispatch.task_unwind");
  assert_eq!(event_values(&store.committed_requests(third_run_id)), [3]);
}

#[test]
fn inline_commit_panic_quarantines_run_and_terminalizes_once() {
  let store = ControlledStore::new();
  let root_id = RunId::new();
  let independent_run_id = RunId::new();
  let panic_control = store.panic_next_commit(root_id);
  panic_control.release();
  let fixture = TestDispatch::with_store_and_spawner(store.clone(), InlineSpawner::new());
  let root = fixture.context(root_id);
  let independent = fixture.context(independent_run_id);

  let emission = catch_unwind(AssertUnwindSafe(|| {
    root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  }));
  assert!(emission.is_ok(), "a started task owns recovery from its propagated panic");
  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 2 }));
  independent.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 3 }));

  let error = block_on_timeout(fixture.dispatch.flush()).unwrap_err();
  assert_eq!(error.failure_count().get(), 2);
  assert_eq!(error.first().code().as_str(), "auv.dispatch.task_unwind");
  assert_eq!(store.commit_call_count(root_id), 1);
  assert!(event_values(&store.committed_requests(root_id)).is_empty());
  assert_eq!(event_values(&store.committed_requests(independent_run_id)), [3]);
}

#[test]
fn dropped_in_flight_commit_quarantines_run_and_allows_independent_work() {
  let store = ControlledStore::new();
  let root_id = RunId::new();
  let independent_run_id = RunId::new();
  let blocked = store.block_next_commit(root_id);
  let fixture = TestDispatch::with_store_and_spawner(store.clone(), DropFirstTaskSpawner::new());
  let root = fixture.context(root_id);
  let independent = fixture.context(independent_run_id);

  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  blocked.wait_until_entered();
  blocked.release();
  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 2 }));
  independent.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 3 }));

  assert_eq!(store.commit_call_count(root_id), 1, "the same run must remain quarantined after cancellation");
  let error = block_on_timeout(fixture.dispatch.flush()).unwrap_err();
  assert_eq!(error.failure_count().get(), 2);
  assert_eq!(error.first().stage(), DispatchStage::AuthorityCommit);
  assert_eq!(error.first().code().as_str(), "auv.dispatch.task_unwind");
  assert!(event_values(&store.committed_requests(root_id)).is_empty());
  assert_eq!(event_values(&store.committed_requests(independent_run_id)), [3]);
}

#[test]
fn validation_failure_terminalizes_its_ticket_without_blocking_later_work() {
  let store = ControlledStore::new();
  let fixture = TestDispatch::with_store(store.clone());
  let root = fixture.root();
  let run_id = *root.run_id().unwrap();

  root.in_scope(|| {
    let span = auv_tracing::start_span!(InvalidSpan);
    assert!(span.is_enabled());
    auv_tracing::emit_event!(TestEvent { value: 7 });
  });

  let error = block_on_timeout(fixture.dispatch.flush()).unwrap_err();
  assert_eq!(error.failure_count().get(), 1);
  assert_eq!(error.first().stage(), DispatchStage::Encode);
  assert_eq!(event_values(&store.committed_requests(run_id)), [7]);
}

#[test]
fn panicking_span_preparation_releases_the_lane_and_reports_encode() {
  let store = ControlledStore::new();
  let fixture = TestDispatch::with_store_and_spawner(store.clone(), InlineSpawner::new());
  let root = fixture.root();
  let run_id = *root.run_id().unwrap();

  let panic = catch_unwind(AssertUnwindSafe(|| {
    root.in_scope(|| {
      let _span = auv_tracing::start_span!(PanickingSpan);
    });
  }));
  assert!(panic.is_err());
  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 8 }));

  assert_eq!(store.commit_call_count(run_id), 1, "later same-run work must reach the authority");
  let error = block_on_timeout(fixture.dispatch.flush()).unwrap_err();
  assert_eq!(error.failure_count().get(), 1);
  assert_eq!(error.first().stage(), DispatchStage::Encode);
  assert_eq!(event_values(&store.committed_requests(run_id)), [8]);
}

#[test]
fn panicking_event_preparation_releases_the_lane_and_reports_encode() {
  let store = ControlledStore::new();
  let fixture = TestDispatch::with_store_and_spawner(store.clone(), InlineSpawner::new());
  let root = fixture.root();
  let run_id = *root.run_id().unwrap();

  let panic = catch_unwind(AssertUnwindSafe(|| {
    root.in_scope(|| auv_tracing::emit_event!(PanickingEvent));
  }));
  assert!(panic.is_err());
  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 9 }));

  assert_eq!(store.commit_call_count(run_id), 1, "later same-run work must reach the authority");
  let error = block_on_timeout(fixture.dispatch.flush()).unwrap_err();
  assert_eq!(error.failure_count().get(), 1);
  assert_eq!(error.first().stage(), DispatchStage::Encode);
  assert_eq!(event_values(&store.committed_requests(run_id)), [9]);
}

// ROOT CAUSE:
//
// If a SpanSpec destructor panicked after its preparation ticket completed,
// the start could commit before start_span constructed the owning SpanState.
//
// Before the fix, the store retained an open span with no owner. The fix drops
// the specification while preparation recovery can still reject the start.
#[test]
fn panicking_span_spec_destructor_rejects_start_and_preserves_lane() {
  let store = ControlledStore::new();
  let fixture = TestDispatch::with_store_and_spawner(store.clone(), InlineSpawner::new());
  let root = fixture.root();
  let run_id = *root.run_id().unwrap();

  let panic = catch_unwind(AssertUnwindSafe(|| {
    root.in_scope(|| {
      let _span = auv_tracing::start_span!(DropPanickingSpan {
        panic_on_drop: true,
      });
    });
  }));
  assert!(panic.is_err());
  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 10 }));

  assert_eq!(store.commit_call_count(run_id), 1, "the rejected start must not reach the authority");
  let error = block_on_timeout(fixture.dispatch.flush()).unwrap_err();
  assert_eq!(error.failure_count().get(), 1);
  assert_eq!(error.first().stage(), DispatchStage::Encode);
  assert_eq!(event_values(&store.committed_requests(run_id)), [10]);
  let snapshot = block_on_timeout(store.load_snapshot(run_id)).unwrap().unwrap();
  assert!(snapshot.spans().is_empty());
  assert_eq!(snapshot.events().len(), 1);
}

// ROOT CAUSE:
//
// If an EventPayload destructor panicked after its preparation ticket
// completed, emit_event unwound even though the event had already committed.
//
// Before the fix, callers observed a panic and a stored event. The fix drops
// the payload while preparation recovery can still reject that event.
#[test]
fn panicking_event_payload_destructor_rejects_event_and_preserves_lane() {
  let store = ControlledStore::new();
  let fixture = TestDispatch::with_store_and_spawner(store.clone(), InlineSpawner::new());
  let root = fixture.root();
  let run_id = *root.run_id().unwrap();

  let panic = catch_unwind(AssertUnwindSafe(|| {
    root.in_scope(|| {
      auv_tracing::emit_event!(DropPanickingEvent {
        value: 11,
        panic_on_drop: true,
      });
    });
  }));
  assert!(panic.is_err());
  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 12 }));

  assert_eq!(store.commit_call_count(run_id), 1, "the rejected event must not reach the authority");
  let error = block_on_timeout(fixture.dispatch.flush()).unwrap_err();
  assert_eq!(error.failure_count().get(), 1);
  assert_eq!(error.first().stage(), DispatchStage::Encode);
  assert_eq!(event_values(&store.committed_requests(run_id)), [12]);
  let snapshot = block_on_timeout(store.load_snapshot(run_id)).unwrap().unwrap();
  assert!(snapshot.spans().is_empty());
  assert_eq!(snapshot.events().len(), 1);
}

#[test]
fn scoped_dispatch_does_not_install_itself_globally() {
  let fixture = TestDispatch::memory();
  dispatcher::with_default(&fixture.dispatch, || assert!(Context::root(RunId::new()).is_enabled()));
  assert!(!Context::root(RunId::new()).is_enabled());
}

fn _accepts_arc_store(_: Arc<dyn RunStore>) {}
