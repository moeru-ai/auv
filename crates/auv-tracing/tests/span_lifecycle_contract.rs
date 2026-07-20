#![cfg(feature = "memory-store")]

mod support;

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Poll};

use auv_tracing::{Attributes, Context, EventPayload, RunId, SpanId, SpanSpec};
use futures_channel::oneshot;
use futures_util::future;
use serde::{Deserialize, Serialize};
use support::{TestDispatch, block_on_timeout};

struct TestSpan;

impl SpanSpec for TestSpan {
  const NAME: &'static str = "auv.test.lifecycle";

  fn attributes(&self) -> Attributes {
    Attributes::empty()
  }
}

#[derive(Debug, Deserialize, Serialize)]
struct TestEvent {
  value: u32,
}

impl EventPayload for TestEvent {
  const NAME: &'static str = "auv.test.lifecycle_event";
  const VERSION: u32 = 1;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ObservedContext {
  run_id: Option<RunId>,
  span_id: Option<SpanId>,
}

impl ObservedContext {
  fn current() -> Self {
    let context = Context::current();
    Self {
      run_id: context.run_id().copied(),
      span_id: context.span_id().copied(),
    }
  }
}

#[derive(Default)]
struct ProbeLog {
  polls: Vec<ObservedContext>,
  drops: Vec<ObservedContext>,
}

struct ContextProbe {
  ready: bool,
  log: Arc<Mutex<ProbeLog>>,
}

impl ContextProbe {
  fn ready(log: Arc<Mutex<ProbeLog>>) -> Self {
    Self { ready: true, log }
  }

  fn pending(log: Arc<Mutex<ProbeLog>>) -> Self {
    Self { ready: false, log }
  }
}

impl Future for ContextProbe {
  type Output = ();

  fn poll(self: Pin<&mut Self>, _context: &mut TaskContext<'_>) -> Poll<Self::Output> {
    self.log.lock().unwrap().polls.push(ObservedContext::current());
    if self.ready {
      Poll::Ready(())
    } else {
      Poll::Pending
    }
  }
}

impl Drop for ContextProbe {
  fn drop(&mut self) {
    self.log.lock().unwrap().drops.push(ObservedContext::current());
  }
}

fn assert_probe_context(log: &Arc<Mutex<ProbeLog>>, expected: ObservedContext) {
  let log = log.lock().unwrap();
  assert_eq!(log.polls, [expected]);
  assert_eq!(log.drops, [expected]);
}

#[test]
fn last_span_or_derived_context_reference_ends_exactly_once() {
  let fixture = TestDispatch::memory();
  let root = fixture.root();
  let run_id = *root.run_id().unwrap();
  let span = root.in_scope(|| auv_tracing::start_span!(TestSpan));
  let child_context = span.context();

  drop(span);
  block_on_timeout(fixture.dispatch.flush()).unwrap();
  assert_eq!(fixture.span_end_count(run_id), 0);

  drop(child_context);
  block_on_timeout(fixture.dispatch.flush()).unwrap();
  assert_eq!(fixture.span_end_count(run_id), 1);
}

#[test]
fn dropping_enter_guard_does_not_end_span() {
  let fixture = TestDispatch::memory();
  let root = fixture.root();
  let run_id = *root.run_id().unwrap();
  let span = root.in_scope(|| auv_tracing::start_span!(TestSpan));

  {
    let _guard = span.enter();
  }
  block_on_timeout(fixture.dispatch.flush()).unwrap();
  assert_eq!(fixture.span_end_count(run_id), 0);

  drop(span);
  block_on_timeout(fixture.dispatch.flush()).unwrap();
  assert_eq!(fixture.span_end_count(run_id), 1);
}

#[test]
fn context_instrument_drops_ready_future_in_captured_context() {
  let root = Context::root(RunId::new());
  let expected = ObservedContext {
    run_id: root.run_id().copied(),
    span_id: None,
  };
  let log = Arc::new(Mutex::new(ProbeLog::default()));

  block_on_timeout(root.instrument(ContextProbe::ready(log.clone())));

  assert_probe_context(&log, expected);
  assert!(Context::current().run_id().is_none());
}

#[test]
fn context_instrument_drops_cancelled_future_in_captured_context() {
  let root = Context::root(RunId::new());
  let expected = ObservedContext {
    run_id: root.run_id().copied(),
    span_id: None,
  };
  let log = Arc::new(Mutex::new(ProbeLog::default()));
  let mut future = Box::pin(root.instrument(ContextProbe::pending(log.clone())));
  let waker = futures_util::task::noop_waker();
  let mut task_context = TaskContext::from_waker(&waker);

  assert!(future.as_mut().poll(&mut task_context).is_pending());
  drop(future);

  assert_probe_context(&log, expected);
  assert!(Context::current().run_id().is_none());
}

#[test]
fn span_instrument_drops_ready_future_in_captured_context() {
  let fixture = TestDispatch::memory();
  let root = fixture.root();
  let run_id = *root.run_id().unwrap();
  let span = root.in_scope(|| auv_tracing::start_span!(TestSpan));
  let span_id = *span.id().unwrap();
  let log = Arc::new(Mutex::new(ProbeLog::default()));

  block_on_timeout(span.instrument(ContextProbe::ready(log.clone())));
  block_on_timeout(fixture.dispatch.flush()).unwrap();

  assert_probe_context(
    &log,
    ObservedContext {
      run_id: Some(run_id),
      span_id: Some(span_id),
    },
  );
  assert_eq!(fixture.span_end_count(run_id), 1);
}

#[test]
fn span_instrument_drops_cancelled_future_in_captured_context() {
  let fixture = TestDispatch::memory();
  let root = fixture.root();
  let run_id = *root.run_id().unwrap();
  let span = root.in_scope(|| auv_tracing::start_span!(TestSpan));
  let span_id = *span.id().unwrap();
  let log = Arc::new(Mutex::new(ProbeLog::default()));
  let mut future = Box::pin(span.instrument(ContextProbe::pending(log.clone())));
  let waker = futures_util::task::noop_waker();
  let mut task_context = TaskContext::from_waker(&waker);

  assert!(future.as_mut().poll(&mut task_context).is_pending());
  drop(future);
  block_on_timeout(fixture.dispatch.flush()).unwrap();

  assert_probe_context(
    &log,
    ObservedContext {
      run_id: Some(run_id),
      span_id: Some(span_id),
    },
  );
  assert_eq!(fixture.span_end_count(run_id), 1);
}

#[test]
fn concurrent_instrumented_futures_keep_distinct_run_events() {
  let fixture = TestDispatch::memory();
  let first = fixture.root();
  let second = fixture.root();
  let first_run_id = *first.run_id().unwrap();
  let second_run_id = *second.run_id().unwrap();

  block_on_timeout(future::join(
    first.instrument(async { auv_tracing::emit_event!(TestEvent { value: 1 }) }),
    second.instrument(async { auv_tracing::emit_event!(TestEvent { value: 2 }) }),
  ));
  block_on_timeout(fixture.dispatch.flush()).unwrap();

  let first_snapshot = fixture.snapshot(first_run_id).unwrap();
  let second_snapshot = fixture.snapshot(second_run_id).unwrap();
  assert_eq!(first_snapshot.events().len(), 1);
  assert_eq!(second_snapshot.events().len(), 1);
  assert_eq!(serde_json::from_str::<TestEvent>(first_snapshot.events()[0].payload().get()).unwrap().value, 1);
  assert_eq!(serde_json::from_str::<TestEvent>(second_snapshot.events()[0].payload().get()).unwrap().value, 2);
}

#[test]
fn spawned_tasks_require_explicit_context_propagation() {
  let root = Context::root(RunId::new());
  let expected_run_id = *root.run_id().unwrap();
  let pool = futures_executor::ThreadPool::new().unwrap();
  let (plain_sender, plain) = oneshot::channel();
  let (propagated_sender, propagated) = oneshot::channel();

  root.in_scope(|| {
    pool.spawn_ok(async move {
      let _ = plain_sender.send(Context::current().run_id().copied());
    });
    pool.spawn_ok(root.instrument(async move {
      let _ = propagated_sender.send(Context::current().run_id().copied());
    }));
  });

  assert_eq!(block_on_timeout(plain).unwrap(), None);
  assert_eq!(block_on_timeout(propagated).unwrap(), Some(expected_run_id));
}
