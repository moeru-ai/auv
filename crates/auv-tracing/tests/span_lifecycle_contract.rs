#![cfg(feature = "memory-store")]

mod support;

use std::cell::RefCell;
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::pin::Pin;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Poll};

use auv_tracing::{Attributes, Context, EventPayload, RunId, SpanId, SpanSpec};
use futures_channel::oneshot;
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

struct ReadyDropPanicFuture {
  expected: ObservedContext,
  drops: Arc<AtomicUsize>,
  panic_on_drop: bool,
}

impl Future for ReadyDropPanicFuture {
  type Output = ();

  fn poll(self: Pin<&mut Self>, _context: &mut TaskContext<'_>) -> Poll<Self::Output> {
    Poll::Ready(())
  }
}

impl Drop for ReadyDropPanicFuture {
  fn drop(&mut self) {
    self.drops.fetch_add(1, Ordering::SeqCst);
    if self.panic_on_drop {
      self.panic_on_drop = false;
      assert_eq!(ObservedContext::current(), self.expected);
      panic!("ready future destructor failed");
    }
  }
}

struct TeardownDropProbe {
  marker: PathBuf,
}

impl Future for TeardownDropProbe {
  type Output = ();

  fn poll(self: Pin<&mut Self>, _context: &mut TaskContext<'_>) -> Poll<Self::Output> {
    Poll::Pending
  }
}

impl Drop for TeardownDropProbe {
  fn drop(&mut self) {
    assert!(Context::current().run_id().is_none(), "destroyed context TLS must fall back to a disabled context");
    std::fs::write(&self.marker, b"dropped").unwrap();
  }
}

thread_local! {
  static TEARDOWN_WRAPPER: RefCell<Option<auv_tracing::WithContext<TeardownDropProbe>>> = const { RefCell::new(None) };
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

struct InterleavedEventFuture {
  expected_run_id: RunId,
  value: u32,
  returned_pending: bool,
}

impl Future for InterleavedEventFuture {
  type Output = ();

  fn poll(mut self: Pin<&mut Self>, task_context: &mut TaskContext<'_>) -> Poll<Self::Output> {
    assert_eq!(Context::current().run_id(), Some(&self.expected_run_id));
    if !self.returned_pending {
      self.returned_pending = true;
      task_context.waker().wake_by_ref();
      return Poll::Pending;
    }
    auv_tracing::emit_event!(TestEvent { value: self.value });
    Poll::Ready(())
  }
}

struct WakeCounter(AtomicUsize);

impl futures_util::task::ArcWake for WakeCounter {
  fn wake_by_ref(counter: &Arc<Self>) {
    counter.0.fetch_add(1, Ordering::SeqCst);
  }
}

fn assert_probe_context(log: &Arc<Mutex<ProbeLog>>, expected: ObservedContext) {
  let log = log.lock().unwrap();
  assert_eq!(log.polls, [expected]);
  assert_eq!(log.drops, [expected]);
}

// ROOT CAUSE:
//
// If another TLS value retained a wrapper after CURRENT_CONTEXTS teardown,
// pinned drop called LocalKey::with and aborted inside a TLS destructor.
//
// Before the fix, the child process aborted with AccessError. The fix falls
// back to disabled context while still destroying the inner future.
#[test]
fn tls_teardown_drops_inner_future_without_aborting() {
  const MARKER_ENV: &str = "AUV_TRACING_TLS_TEARDOWN_MARKER";
  if let Some(marker) = std::env::var_os(MARKER_ENV) {
    TEARDOWN_WRAPPER.with(|wrapper| {
      let root = Context::root(RunId::new());
      std::mem::forget(root.enter());
      *wrapper.borrow_mut() = Some(root.instrument(TeardownDropProbe {
        marker: marker.into(),
      }));
    });
    return;
  }

  let marker = std::env::temp_dir().join(format!("auv-tracing-tls-teardown-{}.marker", std::process::id()));
  let _ = std::fs::remove_file(&marker);
  let output = Command::new(std::env::current_exe().unwrap())
    .arg("--exact")
    .arg("tls_teardown_drops_inner_future_without_aborting")
    .arg("--nocapture")
    .env(MARKER_ENV, &marker)
    .output()
    .unwrap();

  assert!(
    output.status.success(),
    "TLS teardown child failed with {:?}\nstdout:\n{}\nstderr:\n{}",
    output.status.code(),
    String::from_utf8_lossy(&output.stdout),
    String::from_utf8_lossy(&output.stderr),
  );
  assert_eq!(std::fs::read(&marker).unwrap(), b"dropped");
  std::fs::remove_file(marker).unwrap();
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

// ROOT CAUSE:
//
// If a completed WithContext remained allocated, its captured Context kept the
// local SpanState alive even though the inner future and all external handles
// were gone.
//
// Before the fix, no end committed until wrapper drop. The fix releases the
// wrapper's local span reference immediately after Ready.
#[test]
fn completed_context_instrument_releases_span_before_wrapper_drop() {
  let fixture = TestDispatch::memory();
  let root = fixture.root();
  let run_id = *root.run_id().unwrap();
  let span = root.in_scope(|| auv_tracing::start_span!(TestSpan));
  let span_id = *span.id().unwrap();
  let context = span.context();
  let log = Arc::new(Mutex::new(ProbeLog::default()));
  let mut future = Box::pin(context.instrument(ContextProbe::ready(log.clone())));
  drop(context);
  drop(span);
  let waker = futures_util::task::noop_waker();
  let mut task_context = TaskContext::from_waker(&waker);

  assert!(matches!(future.as_mut().poll(&mut task_context), Poll::Ready(())));
  block_on_timeout(fixture.dispatch.flush()).unwrap();

  assert_probe_context(
    &log,
    ObservedContext {
      run_id: Some(run_id),
      span_id: Some(span_id),
    },
  );
  assert_eq!(fixture.span_end_count(run_id), 1);
  drop(future);
  block_on_timeout(fixture.dispatch.flush()).unwrap();
  assert_eq!(fixture.span_end_count(run_id), 1);
}

// ROOT CAUSE:
//
// If a Ready future's destructor panicked, WithContext skipped clearing its
// captured local span and retained the SpanState after poll unwound.
//
// Before the fix, SpanEnded waited for wrapper destruction. The fix releases
// the captured span on both normal and unwinding Ready cleanup.
#[test]
fn ready_drop_panic_releases_with_context_span_before_wrapper_drop() {
  let fixture = TestDispatch::memory();
  let root = fixture.root();
  let run_id = *root.run_id().unwrap();
  let span = root.in_scope(|| auv_tracing::start_span!(TestSpan));
  let span_id = *span.id().unwrap();
  let context = span.context();
  let expected = ObservedContext {
    run_id: Some(run_id),
    span_id: Some(span_id),
  };
  let drops = Arc::new(AtomicUsize::new(0));
  let mut future = Box::pin(context.instrument(ReadyDropPanicFuture {
    expected,
    drops: drops.clone(),
    panic_on_drop: true,
  }));
  drop(context);
  drop(span);
  let waker = futures_util::task::noop_waker();
  let mut task_context = TaskContext::from_waker(&waker);

  let panic = catch_unwind(AssertUnwindSafe(|| {
    let _ = future.as_mut().poll(&mut task_context);
  }));
  assert!(panic.is_err());
  assert!(Context::current().run_id().is_none());
  block_on_timeout(fixture.dispatch.flush()).unwrap();
  assert_eq!(fixture.span_end_count(run_id), 1);
  assert_eq!(drops.load(Ordering::SeqCst), 1);

  drop(future);
  block_on_timeout(fixture.dispatch.flush()).unwrap();
  assert_eq!(fixture.span_end_count(run_id), 1);
  assert_eq!(drops.load(Ordering::SeqCst), 1);
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

// ROOT CAUSE:
//
// If a Ready future's destructor panicked, Instrumented skipped releasing both
// its captured Context and consumed Span handle after poll unwound.
//
// Before the fix, SpanEnded waited for wrapper destruction. The fix releases
// both owning references on normal and unwinding Ready cleanup.
#[test]
fn ready_drop_panic_releases_instrumented_span_before_wrapper_drop() {
  let fixture = TestDispatch::memory();
  let root = fixture.root();
  let run_id = *root.run_id().unwrap();
  let span = root.in_scope(|| auv_tracing::start_span!(TestSpan));
  let span_id = *span.id().unwrap();
  let expected = ObservedContext {
    run_id: Some(run_id),
    span_id: Some(span_id),
  };
  let drops = Arc::new(AtomicUsize::new(0));
  let mut future = Box::pin(span.instrument(ReadyDropPanicFuture {
    expected,
    drops: drops.clone(),
    panic_on_drop: true,
  }));
  let waker = futures_util::task::noop_waker();
  let mut task_context = TaskContext::from_waker(&waker);

  let panic = catch_unwind(AssertUnwindSafe(|| {
    let _ = future.as_mut().poll(&mut task_context);
  }));
  assert!(panic.is_err());
  assert!(Context::current().run_id().is_none());
  block_on_timeout(fixture.dispatch.flush()).unwrap();
  assert_eq!(fixture.span_end_count(run_id), 1);
  assert_eq!(drops.load(Ordering::SeqCst), 1);

  drop(future);
  block_on_timeout(fixture.dispatch.flush()).unwrap();
  assert_eq!(fixture.span_end_count(run_id), 1);
  assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[test]
fn concurrent_instrumented_futures_keep_distinct_run_events() {
  let fixture = TestDispatch::memory();
  let first = fixture.root();
  let second = fixture.root();
  let first_run_id = *first.run_id().unwrap();
  let second_run_id = *second.run_id().unwrap();
  let mut first_future = Box::pin(first.instrument(InterleavedEventFuture {
    expected_run_id: first_run_id,
    value: 1,
    returned_pending: false,
  }));
  let mut second_future = Box::pin(second.instrument(InterleavedEventFuture {
    expected_run_id: second_run_id,
    value: 2,
    returned_pending: false,
  }));
  let wake_counter = Arc::new(WakeCounter(AtomicUsize::new(0)));
  let waker = futures_util::task::waker_ref(&wake_counter);
  let mut task_context = TaskContext::from_waker(&waker);

  assert!(first_future.as_mut().poll(&mut task_context).is_pending());
  assert!(Context::current().run_id().is_none());
  assert!(second_future.as_mut().poll(&mut task_context).is_pending());
  assert!(Context::current().run_id().is_none());
  assert_eq!(wake_counter.0.load(Ordering::SeqCst), 2);
  assert!(first_future.as_mut().poll(&mut task_context).is_ready());
  assert!(Context::current().run_id().is_none());
  assert!(second_future.as_mut().poll(&mut task_context).is_ready());
  assert!(Context::current().run_id().is_none());
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
