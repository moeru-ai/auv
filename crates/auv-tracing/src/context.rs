use std::cell::RefCell;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};

use pin_project::{pin_project, pinned_drop};

use crate::{
  Attributes, Dispatch, EventId, EventPayload, EventSchema, JsonPayload, PropagationError, RemoteContext, RunId, SpanId, SpanName,
  TextMapWriter, TraceRecord, dispatcher,
};

thread_local! {
  static CURRENT: RefCell<CurrentContexts> = const {
    RefCell::new(CurrentContexts { next_token: 0, frames: Vec::new() })
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
  remote_span_id: Option<SpanId>,
}

impl Context {
  pub fn root(run_id: RunId) -> Self {
    Self {
      dispatch: dispatcher::current(),
      run_id: Some(run_id),
      span: None,
      remote_span_id: None,
    }
  }
  pub fn current() -> Self {
    CURRENT.try_with(|items| items.borrow().frames.last().map(|frame| frame.context.clone())).ok().flatten().unwrap_or_else(Self::disabled)
  }
  pub fn run_id(&self) -> Option<&RunId> {
    self.run_id.as_ref()
  }
  pub fn span_id(&self) -> Option<&SpanId> {
    self.span.as_ref().map(|span| &span.id)
  }
  pub fn is_enabled(&self) -> bool {
    self.run_id.is_some() && self.dispatch.as_ref().is_some_and(Dispatch::is_enabled)
  }
  pub fn can_publish_artifacts(&self) -> bool {
    self.run_id.is_some() && self.dispatch.as_ref().is_some_and(Dispatch::can_write_artifacts)
  }
  pub(crate) fn dispatch(&self) -> Option<&Dispatch> {
    self.dispatch.as_ref()
  }

  pub fn enter(&self) -> ContextGuard<'_> {
    let token = CURRENT
      .try_with(|items| {
        let mut items = items.borrow_mut();
        items.next_token = items.next_token.checked_add(1).expect("context token space exhausted");
        let token = items.next_token;
        items.frames.push(ContextFrame {
          token,
          context: self.clone(),
        });
        token
      })
      .ok();
    ContextGuard {
      token,
      lifetime: PhantomData,
      thread_bound: PhantomData,
    }
  }
  pub fn in_scope<T>(&self, operation: impl FnOnce() -> T) -> T {
    let _guard = self.enter();
    operation()
  }
  pub fn instrument<F>(&self, future: F) -> WithContext<F> {
    WithContext {
      context: self.clone(),
      future: Some(future),
    }
  }
  pub fn inject(&self, carrier: &mut dyn TextMapWriter) {
    crate::propagation::inject(carrier, self.run_id, self.span_id().copied().or(self.remote_span_id));
  }
  pub fn from_remote(remote: RemoteContext) -> Result<Self, PropagationError> {
    Ok(Self {
      dispatch: dispatcher::current(),
      run_id: Some(remote.run_id),
      span: None,
      remote_span_id: remote.remote_span_id,
    })
  }
  fn disabled() -> Self {
    Self {
      dispatch: None,
      run_id: None,
      span: None,
      remote_span_id: None,
    }
  }
  fn with_span(&self, span: Arc<SpanState>) -> Self {
    Self {
      dispatch: self.dispatch.clone(),
      run_id: self.run_id,
      span: Some(span),
      remote_span_id: None,
    }
  }
}

/// Removes the current scope on its creating thread.
pub struct ContextGuard<'a> {
  token: Option<u64>,
  lifetime: PhantomData<&'a Context>,
  thread_bound: PhantomData<Rc<()>>,
}
impl Drop for ContextGuard<'_> {
  fn drop(&mut self) {
    let Some(token) = self.token else { return };
    let _ = CURRENT.try_with(|items| {
      let mut items = items.borrow_mut();
      if let Some(position) = items.frames.iter().position(|frame| frame.token == token) {
        items.frames.remove(position);
      }
    });
  }
}

#[pin_project(PinnedDrop)]
pub struct WithContext<F> {
  context: Context,
  #[pin]
  future: Option<F>,
}
impl<F: Future> Future for WithContext<F> {
  type Output = F::Output;
  fn poll(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Self::Output> {
    let mut this = self.project();
    let result = {
      let _guard = this.context.enter();
      this.future.as_mut().as_pin_mut().expect("completed future polled").poll(cx)
    };
    if result.is_ready() {
      let _guard = this.context.enter();
      this.future.set(None);
    }
    result
  }
}
#[pinned_drop]
impl<F> PinnedDrop for WithContext<F> {
  fn drop(self: Pin<&mut Self>) {
    let mut this = self.project();
    let _guard = this.context.enter();
    this.future.set(None);
  }
}

/// Declares a stable span name and bounded attributes.
pub trait SpanSpec {
  const NAME: &'static str;
  fn attributes(&self) -> Attributes;
}

#[derive(Clone)]
pub struct Span {
  context: Context,
}
impl Span {
  pub fn id(&self) -> Option<&SpanId> {
    self.context.span_id()
  }
  pub fn is_enabled(&self) -> bool {
    self.id().is_some() && self.context.is_enabled()
  }
  pub fn context(&self) -> Context {
    self.context.clone()
  }
  pub fn enter(&self) -> ContextGuard<'_> {
    self.context.enter()
  }
  pub fn in_scope<T>(&self, operation: impl FnOnce() -> T) -> T {
    self.context.in_scope(operation)
  }
  pub fn instrument<F>(self, future: F) -> Instrumented<F> {
    Instrumented {
      context: self.context.clone(),
      span: Some(self),
      future: Some(future),
    }
  }
}

struct SpanState {
  id: SpanId,
  close: Option<SpanClose>,
}
struct SpanClose {
  dispatch: Dispatch,
  run_id: RunId,
}
impl Drop for SpanState {
  fn drop(&mut self) {
    if let (Some(close), Ok(ended_at)) = (&self.close, crate::dispatch::timestamp_now()) {
      close.dispatch.submit(TraceRecord::SpanEnded {
        run_id: close.run_id,
        span_id: self.id,
        ended_at,
      });
    }
  }
}

#[pin_project(PinnedDrop)]
pub struct Instrumented<F> {
  context: Context,
  span: Option<Span>,
  #[pin]
  future: Option<F>,
}
impl<F: Future> Future for Instrumented<F> {
  type Output = F::Output;
  fn poll(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Self::Output> {
    let mut this = self.project();
    let result = {
      let _guard = this.context.enter();
      this.future.as_mut().as_pin_mut().expect("completed future polled").poll(cx)
    };
    if result.is_ready() {
      let _guard = this.context.enter();
      this.future.set(None);
      this.span.take();
    }
    result
  }
}
#[pinned_drop]
impl<F> PinnedDrop for Instrumented<F> {
  fn drop(self: Pin<&mut Self>) {
    let mut this = self.project();
    let _guard = this.context.enter();
    this.future.set(None);
    this.span.take();
  }
}

pub fn start_span<S: SpanSpec>(spec: S) -> Span {
  let parent = Context::current();
  let (Some(dispatch), Some(run_id)) = (parent.dispatch.clone().filter(Dispatch::is_enabled), parent.run_id) else {
    return Span { context: parent };
  };
  let span_id = SpanId::new();
  let prepared = (|| {
    let name = SpanName::parse(S::NAME).ok()?;
    let started_at = crate::dispatch::timestamp_now().ok()?;
    let attributes = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| spec.attributes())).ok()?;
    dispatch.submit(TraceRecord::SpanStarted {
      run_id,
      span_id,
      parent_span_id: parent.span_id().copied(),
      remote_span_id: parent.remote_span_id,
      name,
      started_at,
      attributes,
    });
    Some(SpanClose { dispatch, run_id })
  })();
  Span {
    context: parent.with_span(Arc::new(SpanState {
      id: span_id,
      close: prepared,
    })),
  }
}

pub fn emit_event<E: EventPayload>(event: E) {
  let context = Context::current();
  let (Some(dispatch), Some(run_id)) = (context.dispatch.clone().filter(Dispatch::is_enabled), context.run_id) else {
    return;
  };
  let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| JsonPayload::encode(&event)));
  let (Ok(schema), Ok(Ok(payload)), Ok(occurred_at)) = (EventSchema::for_payload::<E>(), payload, crate::dispatch::timestamp_now()) else {
    return;
  };
  dispatch.submit(TraceRecord::Event {
    run_id,
    span_id: context.span_id().copied(),
    event_id: EventId::new(),
    schema,
    occurred_at,
    payload,
  });
}
