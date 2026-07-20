use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;

use crate::{Attributes, AuthorityId, Dispatch, EventPayload, RunId, SpanId, dispatcher};

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
}

impl Context {
  /// Captures the current dispatch for an independently supplied run ID.
  pub fn root(run_id: RunId) -> Self {
    Self {
      dispatch: dispatcher::current(),
      run_id: Some(run_id),
      span: None,
    }
  }

  /// Clones the innermost thread-local scope or returns a disabled context.
  pub fn current() -> Self {
    CURRENT_CONTEXTS.with(|contexts| contexts.borrow().frames.last().map(|frame| frame.context.clone())).unwrap_or_else(Self::disabled)
  }

  /// Returns the configured authority captured by this context.
  pub fn authority_id(&self) -> Option<&AuthorityId> {
    self.dispatch.as_ref().and_then(Dispatch::authority_id)
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

  /// Makes this context current on the calling thread until the guard drops.
  pub fn enter(&self) -> ContextGuard<'_> {
    let token = CURRENT_CONTEXTS.with(|contexts| {
      let mut contexts = contexts.borrow_mut();
      contexts.next_token = contexts.next_token.checked_add(1).expect("context frame token space exhausted");
      let token = contexts.next_token;
      contexts.frames.push(ContextFrame {
        token,
        context: self.clone(),
      });
      token
    });
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

  fn disabled() -> Self {
    Self {
      dispatch: None,
      run_id: None,
      span: None,
    }
  }

  fn with_span(&self, span: Arc<SpanState>) -> Self {
    Self {
      dispatch: self.dispatch.clone(),
      run_id: self.run_id,
      span: Some(span),
    }
  }
}

/// Removes its own current-context frame when dropped on its creating thread.
///
/// Newer frames remain current when guards are dropped out of entry order.
pub struct ContextGuard<'a> {
  token: u64,
  context: PhantomData<&'a Context>,
  thread_bound: PhantomData<Rc<()>>,
}

impl Drop for ContextGuard<'_> {
  fn drop(&mut self) {
    CURRENT_CONTEXTS.with(|contexts| {
      let mut contexts = contexts.borrow_mut();
      if let Some(position) = contexts.frames.iter().position(|frame| frame.token == self.token) {
        contexts.frames.remove(position);
      }
    });
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
}

struct SpanState {
  id: SpanId,
}

// TODO(auv-run-contract-v1-task-7): implement SpanState drop-time SpanEnded
// submission when the last Span/context reference is released; Task 6 does not
// add a terminal field or async context wrappers.

/// Starts a typed span under the current context.
pub fn start_span(spec: impl SpanSpec) -> Span {
  let parent = Context::current();
  let Some(dispatch) = parent.dispatch.clone().filter(Dispatch::is_enabled) else {
    return Span { context: parent };
  };
  let Some(run_id) = parent.run_id else {
    return Span { context: parent };
  };

  let span = Arc::new(SpanState { id: SpanId::new() });
  dispatch.submit_span(run_id, parent.span_id().copied(), span.id, spec);
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

  dispatch.submit_event(run_id, context.span_id().copied(), event);
}

// TODO(auv-run-contract-v1-task-7): add pinned Context/Span future wrappers and
// cross-process propagation without using thread-local guards across await.
