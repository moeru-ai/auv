use std::cell::RefCell;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};
use std::task::{Context as TaskContext, Poll};
use std::time::{Duration, Instant};

use pin_project::{pin_project, pinned_drop};

use crate::{
  Attributes, AuthorityId, Dispatch, ErrorCode, EventPayload, PropagationError, RemoteContext, RunId, SpanId, SpanLink, TextMapWriter,
  Timestamp, dispatcher,
};

thread_local! {
  static CURRENT_CONTEXTS: RefCell<CurrentContexts> = const {
    RefCell::new(CurrentContexts {
      next_token: 0,
      frames: Vec::new(),
    })
  };
}

struct CurrentContexts {
  next_token: u64,
  frames: Vec<ContextFrame>,
}

struct ContextFrame {
  token: u64,
  context: Context,
}

/// One explicitly propagated AUV run and span scope.
#[derive(Clone)]
pub struct Context {
  dispatch: Option<Dispatch>,
  run_id: Option<RunId>,
  span: Option<Arc<SpanState>>,
  remote_authority_id: Option<AuthorityId>,
  remote_span_id: Option<SpanId>,
}

impl Context {
  /// Captures the current dispatch for an independently supplied run ID.
  pub fn root(run_id: RunId) -> Self {
    Self {
      dispatch: dispatcher::current(),
      run_id: Some(run_id),
      span: None,
      remote_authority_id: None,
      remote_span_id: None,
    }
  }

  /// Clones the innermost thread-local scope or returns a disabled context.
  ///
  /// During thread teardown the context stack may already be destroyed. In
  /// that narrow case this returns a disabled context so later TLS destructors
  /// can finish without aborting the process.
  pub fn current() -> Self {
    CURRENT_CONTEXTS
      .try_with(|contexts| contexts.borrow().frames.last().map(|frame| frame.context.clone()))
      .ok()
      .flatten()
      .unwrap_or_else(Self::disabled)
  }

  /// Returns the captured local authority or preserved remote authority.
  pub fn authority_id(&self) -> Option<&AuthorityId> {
    self.dispatch.as_ref().and_then(Dispatch::authority_id).or(self.remote_authority_id.as_ref())
  }

  /// Returns the explicitly supplied run ID, including for disabled roots.
  pub fn run_id(&self) -> Option<&RunId> {
    self.run_id.as_ref()
  }

  /// Returns the current local span ID.
  pub fn span_id(&self) -> Option<&SpanId> {
    self.span.as_ref().map(|span| &span.id)
  }

  /// Reports whether this context has both a run and an active dispatch route.
  pub fn is_enabled(&self) -> bool {
    self.run_id.is_some() && self.dispatch.as_ref().is_some_and(Dispatch::is_enabled)
  }

  /// Reports whether this context can publish artifacts to a local run authority.
  ///
  /// Telemetry-only dispatches and remotely propagated correlation remain
  /// enabled contexts, but neither supplies writable artifact authority.
  pub fn can_publish_artifacts(&self) -> bool {
    self.run_id.is_some() && self.dispatch.as_ref().is_some_and(|dispatch| dispatch.authority_id().is_some())
  }

  pub(crate) fn dispatch(&self) -> Option<&Dispatch> {
    self.dispatch.as_ref()
  }

  /// Makes this context current on the calling thread until the guard drops.
  ///
  /// If called after the context TLS has been destroyed, the returned guard is
  /// a no-op. This permits another TLS value to destroy an instrumented future.
  pub fn enter(&self) -> ContextGuard<'_> {
    let token = CURRENT_CONTEXTS
      .try_with(|contexts| {
        let mut contexts = contexts.borrow_mut();
        contexts.next_token = contexts.next_token.checked_add(1).expect("context frame token space exhausted");
        let token = contexts.next_token;
        contexts.frames.push(ContextFrame {
          token,
          context: self.clone(),
        });
        token
      })
      .ok();
    ContextGuard {
      token,
      context: PhantomData,
      thread_bound: PhantomData,
    }
  }

  /// Runs a synchronous closure with this context current.
  pub fn in_scope<T>(&self, f: impl FnOnce() -> T) -> T {
    let _guard = self.enter();
    f()
  }

  /// Wraps a future so this context is current only while it is polled or destroyed.
  pub fn instrument<F>(&self, future: F) -> WithContext<F> {
    WithContext {
      context: self.clone(),
      future: Some(future),
    }
  }

  /// Removes stale AUV fields and injects this context's propagatable values.
  ///
  /// A context without a run removes the fields and writes no replacements.
  pub fn inject(&self, carrier: &mut dyn TextMapWriter) {
    crate::propagation::inject(carrier, self.authority_id().copied(), self.run_id, self.span_id().copied().or(self.remote_span_id));
  }

  /// Binds extracted remote correlation to the current local dispatch.
  ///
  /// Construction fails when local and remote authorities are both present
  /// but do not identify the same run history.
  pub fn from_remote(remote: RemoteContext) -> Result<Self, PropagationError> {
    let dispatch = dispatcher::current();
    let local_authority_id = dispatch.as_ref().and_then(Dispatch::authority_id).copied();
    if let (Some(local), Some(remote_authority)) = (local_authority_id, remote.authority_id)
      && local != remote_authority
    {
      return Err(crate::propagation::authority_mismatch());
    }

    Ok(Self {
      dispatch,
      run_id: Some(remote.run_id),
      span: None,
      remote_authority_id: remote.authority_id,
      remote_span_id: remote.remote_span_id,
    })
  }

  fn disabled() -> Self {
    Self {
      dispatch: None,
      run_id: None,
      span: None,
      remote_authority_id: None,
      remote_span_id: None,
    }
  }

  fn with_span(&self, span: Arc<SpanState>) -> Self {
    Self {
      dispatch: self.dispatch.clone(),
      run_id: self.run_id,
      span: Some(span),
      remote_authority_id: self.remote_authority_id,
      remote_span_id: None,
    }
  }

  fn clear_span(&mut self) {
    self.span = None;
  }
}

/// Removes its own current-context frame when dropped on its creating thread.
///
/// Newer frames remain current when guards are dropped out of entry order.
pub struct ContextGuard<'a> {
  token: Option<u64>,
  context: PhantomData<&'a Context>,
  thread_bound: PhantomData<Rc<()>>,
}

impl Drop for ContextGuard<'_> {
  fn drop(&mut self) {
    let Some(token) = self.token else {
      return;
    };
    // NOTICE: another TLS destructor may run after CURRENT_CONTEXTS itself.
    // Missing TLS is the unavoidable teardown boundary, so restoration is a no-op.
    let _ = CURRENT_CONTEXTS.try_with(|contexts| {
      let mut contexts = contexts.borrow_mut();
      if let Some(position) = contexts.frames.iter().position(|frame| frame.token == token) {
        contexts.frames.remove(position);
      }
    });
  }
}

/// A future polled and destroyed with one explicitly captured context current.
#[pin_project(PinnedDrop)]
pub struct WithContext<F> {
  context: Context,
  #[pin]
  future: Option<F>,
}

impl<F: Future> Future for WithContext<F> {
  type Output = F::Output;

  fn poll(self: Pin<&mut Self>, task_context: &mut TaskContext<'_>) -> Poll<Self::Output> {
    let mut this = self.project();
    let poll = {
      let _guard = this.context.enter();
      this.future.as_mut().as_pin_mut().expect("completed WithContext futures must not be polled again").poll(task_context)
    };
    if poll.is_ready() {
      let release = ReadySpanRelease::with_context(this.context);
      drop_future_in_context(release.context(), this.future.as_mut());
    }
    poll
  }
}

#[pinned_drop]
impl<F> PinnedDrop for WithContext<F> {
  fn drop(self: Pin<&mut Self>) {
    let mut this = self.project();
    drop_future_in_context(this.context, this.future.as_mut());
    this.context.clear_span();
  }
}

/// Declares the stable name and bounded attributes for a typed span.
pub trait SpanSpec {
  /// Stable namespaced span name.
  const NAME: &'static str;

  /// Returns the span's validated attributes.
  fn attributes(&self) -> Attributes;
}

/// A cloneable handle to a started span or a disabled span scope.
#[derive(Clone)]
pub struct Span {
  context: Context,
}

impl Span {
  /// Returns the span identity, or `None` for a disabled span.
  pub fn id(&self) -> Option<&SpanId> {
    self.context.span_id()
  }

  /// Reports whether the span has an active captured route and context.
  ///
  /// This does not report whether start encoding or an asynchronous authority
  /// commit later succeeds.
  pub fn is_enabled(&self) -> bool {
    self.id().is_some() && self.context.is_enabled()
  }

  /// Returns the context carrying this span.
  pub fn context(&self) -> Context {
    self.context.clone()
  }

  /// Makes this span's context current until the guard drops.
  pub fn enter(&self) -> ContextGuard<'_> {
    self.context.enter()
  }

  /// Runs a synchronous closure with this span's context current.
  pub fn in_scope<T>(&self, f: impl FnOnce() -> T) -> T {
    self.context.in_scope(f)
  }

  /// Wraps a future and consumes this handle as the wrapper's span lifetime.
  pub fn instrument<F>(self, future: F) -> Instrumented<F> {
    Instrumented {
      context: self.context(),
      span: Some(self),
      future: Some(future),
    }
  }
}

struct SpanState {
  id: SpanId,
  // A failed start preparation remains an active context handle but is not
  // armed to submit an end for a start fact that can never be committed.
  close: Option<SpanClose>,
}

struct SpanClose {
  dispatch: Dispatch,
  authority_id: Option<AuthorityId>,
  run_id: RunId,
  started_at: Timestamp,
  started_tick: Duration,
  clock: Arc<dyn Clock>,
}

struct ClockSample {
  clock: Arc<dyn Clock>,
  tick: Duration,
  timestamp: Result<Timestamp, ErrorCode>,
}

impl SpanState {
  fn sample_clock(&self) -> Option<ClockSample> {
    let close = self.close.as_ref()?;
    let tick = close.clock.monotonic_now();
    Some(ClockSample {
      clock: close.clock.clone(),
      tick,
      timestamp: crate::dispatch::timestamp_after(close.started_at, close.started_tick, tick),
    })
  }
}

impl Drop for SpanState {
  fn drop(&mut self) {
    let Some(close) = &self.close else {
      return;
    };
    let ended_at = crate::dispatch::timestamp_after(close.started_at, close.started_tick, close.clock.monotonic_now());
    close.dispatch.submit_span_end(close.authority_id, close.run_id, self.id, ended_at);
  }
}

trait Clock: Send + Sync {
  fn wall_now(&self) -> Result<Timestamp, ErrorCode>;
  fn monotonic_now(&self) -> Duration;
}

struct SystemClock;

impl Clock for SystemClock {
  fn wall_now(&self) -> Result<Timestamp, ErrorCode> {
    crate::dispatch::timestamp_now()
  }

  fn monotonic_now(&self) -> Duration {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    ORIGIN.get_or_init(Instant::now).elapsed()
  }
}

fn system_clock() -> Arc<dyn Clock> {
  static CLOCK: OnceLock<Arc<dyn Clock>> = OnceLock::new();
  CLOCK.get_or_init(|| Arc::new(SystemClock)).clone()
}

fn sample_root_clock(clock: Arc<dyn Clock>) -> ClockSample {
  let timestamp = clock.wall_now();
  let tick = clock.monotonic_now();
  ClockSample {
    clock,
    tick,
    timestamp,
  }
}

/// A future polled and destroyed inside one consumed span scope.
#[pin_project(PinnedDrop)]
pub struct Instrumented<F> {
  context: Context,
  span: Option<Span>,
  #[pin]
  future: Option<F>,
}

impl<F: Future> Future for Instrumented<F> {
  type Output = F::Output;

  fn poll(self: Pin<&mut Self>, task_context: &mut TaskContext<'_>) -> Poll<Self::Output> {
    let mut this = self.project();
    let poll = {
      let _guard = this.context.enter();
      this.future.as_mut().as_pin_mut().expect("completed Instrumented futures must not be polled again").poll(task_context)
    };
    if poll.is_ready() {
      let release = ReadySpanRelease::instrumented(this.context, this.span);
      drop_future_in_context(release.context(), this.future.as_mut());
    }
    poll
  }
}

#[pinned_drop]
impl<F> PinnedDrop for Instrumented<F> {
  fn drop(self: Pin<&mut Self>) {
    let mut this = self.project();
    drop_future_in_context(this.context, this.future.as_mut());
    release_instrumented_span(this.context, this.span);
  }
}

fn drop_future_in_context<F>(context: &Context, mut future: Pin<&mut Option<F>>) {
  let _guard = context.enter();
  future.set(None);
}

fn release_instrumented_span(context: &mut Context, span: &mut Option<Span>) {
  context.clear_span();
  span.take();
}

struct ReadySpanRelease<'a> {
  context: &'a mut Context,
  span: Option<&'a mut Option<Span>>,
}

impl<'a> ReadySpanRelease<'a> {
  fn with_context(context: &'a mut Context) -> Self {
    Self {
      context,
      span: None,
    }
  }

  fn instrumented(context: &'a mut Context, span: &'a mut Option<Span>) -> Self {
    Self {
      context,
      span: Some(span),
    }
  }

  fn context(&self) -> &Context {
    self.context
  }
}

impl Drop for ReadySpanRelease<'_> {
  fn drop(&mut self) {
    if let Some(span) = self.span.take() {
      release_instrumented_span(self.context, span);
    } else {
      self.context.clear_span();
    }
  }
}

/// Starts a typed span under the current context.
pub fn start_span(spec: impl SpanSpec) -> Span {
  start_span_with_clock(spec, system_clock())
}

fn start_span_with_clock(spec: impl SpanSpec, clock: Arc<dyn Clock>) -> Span {
  let parent = Context::current();
  let Some(dispatch) = parent.dispatch.clone().filter(Dispatch::is_enabled) else {
    return Span { context: parent };
  };
  let Some(run_id) = parent.run_id else {
    return Span { context: parent };
  };

  let span_id = SpanId::new();
  let sample = parent.span.as_ref().and_then(|span| span.sample_clock()).unwrap_or_else(|| sample_root_clock(clock));
  let close_started_at = sample.timestamp.as_ref().ok().copied();
  let remote_link = parent.remote_span_id.map(SpanLink::new);
  let authority_id = parent.authority_id().copied();
  let prepared = dispatch.submit_span_start(&parent, remote_link, span_id, sample.timestamp, spec);
  let close = close_started_at.filter(|_| prepared).map(|started_at| SpanClose {
    dispatch,
    authority_id,
    run_id,
    started_at,
    started_tick: sample.tick,
    clock: sample.clock,
  });
  let span = Arc::new(SpanState { id: span_id, close });
  Span {
    context: parent.with_span(span),
  }
}

/// Emits a typed point event under the current context.
pub fn emit_event(event: impl EventPayload) {
  let context = Context::current();
  let Some(dispatch) = context.dispatch.clone().filter(Dispatch::is_enabled) else {
    return;
  };
  let Some(run_id) = context.run_id else {
    return;
  };

  let occurred_at =
    context.span.as_ref().and_then(|span| span.sample_clock()).map(|sample| sample.timestamp).unwrap_or_else(crate::dispatch::timestamp_now);
  dispatch.submit_event(context.authority_id().copied(), run_id, context.span_id().copied(), occurred_at, event);
}

#[cfg(all(test, feature = "memory-store"))]
mod tests {
  use std::sync::Mutex;
  use std::time::Duration;

  use super::*;
  use crate::{DispatchStage, ErrorCode, MemoryRunStore, RunStore, Timestamp, configure};

  struct TestSpan;

  impl SpanSpec for TestSpan {
    const NAME: &'static str = "auv.test.clock";

    fn attributes(&self) -> Attributes {
      Attributes::empty()
    }
  }

  #[derive(serde::Serialize)]
  struct TestEvent {
    value: u32,
  }

  impl EventPayload for TestEvent {
    const NAME: &'static str = "auv.test.clock_event";
    const VERSION: u32 = 1;
  }

  struct ManualClock {
    wall: Mutex<Timestamp>,
    tick: Mutex<Duration>,
  }

  impl ManualClock {
    fn new(wall: Timestamp, tick: Duration) -> Arc<Self> {
      Arc::new(Self {
        wall: Mutex::new(wall),
        tick: Mutex::new(tick),
      })
    }

    fn set(&self, wall: Timestamp, tick: Duration) {
      *self.wall.lock().unwrap() = wall;
      *self.tick.lock().unwrap() = tick;
    }
  }

  impl Clock for ManualClock {
    fn wall_now(&self) -> Result<Timestamp, ErrorCode> {
      Ok(*self.wall.lock().unwrap())
    }

    fn monotonic_now(&self) -> Duration {
      *self.tick.lock().unwrap()
    }
  }

  fn timestamp(seconds: i64) -> Timestamp {
    Timestamp::new(seconds, 0).unwrap()
  }

  #[test]
  fn span_end_uses_start_wall_time_plus_monotonic_elapsed() {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().unwrap();
    let run_id = RunId::new();
    let clock = ManualClock::new(timestamp(100), Duration::from_secs(10));
    let span = dispatcher::with_default(&dispatch, || {
      let root = Context::root(run_id);
      root.in_scope(|| start_span_with_clock(TestSpan, clock.clone()))
    });
    let span_id = *span.id().unwrap();

    clock.set(timestamp(50), Duration::from_secs(14));
    assert_eq!(clock.wall_now().unwrap(), timestamp(50));
    drop(span);
    futures_executor::block_on(dispatch.flush()).unwrap();

    let snapshot = futures_executor::block_on(store.load_snapshot(run_id)).unwrap().unwrap();
    let stored = snapshot.spans().get(&span_id).unwrap();
    assert_eq!(stored.started().started_at(), timestamp(100));
    assert_eq!(stored.ended().unwrap().ended_at(), timestamp(104));
  }

  #[test]
  fn span_end_timestamp_overflow_is_one_encode_failure() {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().unwrap();
    let run_id = RunId::new();
    let clock = ManualClock::new(timestamp(9_007_199_254_740_991), Duration::ZERO);
    let span = dispatcher::with_default(&dispatch, || {
      let root = Context::root(run_id);
      root.in_scope(|| start_span_with_clock(TestSpan, clock.clone()))
    });
    let span_id = *span.id().unwrap();

    clock.set(timestamp(1), Duration::from_secs(1));
    drop(span);
    let error = futures_executor::block_on(dispatch.flush()).unwrap_err();

    assert_eq!(error.failure_count().get(), 1);
    assert_eq!(error.first().stage(), DispatchStage::Encode);
    let snapshot = futures_executor::block_on(store.load_snapshot(run_id)).unwrap().unwrap();
    assert!(snapshot.spans().get(&span_id).unwrap().ended().is_none());
  }

  // ROOT CAUSE:
  //
  // If wall time jumped forward, events and children sampled SystemTime while
  // their parent end stayed on its start-wall-plus-monotonic mapping.
  //
  // Before the fix, the authority rejected the parent end as earlier than its
  // facts. The fix derives all in-span timestamps from one affine clock domain.
  #[test]
  fn active_span_clock_orders_events_children_and_ends_across_forward_wall_jump() {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().unwrap();
    let run_id = RunId::new();
    let clock = ManualClock::new(timestamp(100), Duration::from_secs(10));
    let parent = dispatcher::with_default(&dispatch, || {
      let root = Context::root(run_id);
      root.in_scope(|| start_span_with_clock(TestSpan, clock.clone()))
    });
    let parent_id = *parent.id().unwrap();

    clock.set(timestamp(200), Duration::from_secs(12));
    let child = parent.in_scope(|| {
      emit_event(TestEvent { value: 1 });
      start_span(TestSpan)
    });
    let child_id = *child.id().unwrap();
    clock.set(timestamp(250), Duration::from_secs(14));
    drop(child);
    clock.set(timestamp(300), Duration::from_secs(15));
    drop(parent);
    futures_executor::block_on(dispatch.flush()).unwrap();

    let snapshot = futures_executor::block_on(store.load_snapshot(run_id)).unwrap().unwrap();
    let parent = snapshot.spans().get(&parent_id).unwrap();
    let child = snapshot.spans().get(&child_id).unwrap();
    assert_eq!(parent.started().started_at(), timestamp(100));
    assert_eq!(snapshot.events()[0].occurred_at(), timestamp(102));
    assert_eq!(child.started().started_at(), timestamp(102));
    assert_eq!(child.ended().unwrap().ended_at(), timestamp(104));
    assert_eq!(parent.ended().unwrap().ended_at(), timestamp(105));
  }
}
