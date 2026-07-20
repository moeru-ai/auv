use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use futures_channel::oneshot;

use crate::{
  AuthorityId, CommitError, ErrorCode, EventId, EventOccurred, EventPayload, EventSchema, IdempotencyKey, JsonPayload, NonEmptyVec,
  RunCommitRequest, RunId, RunMutation, RunStore, SpanId, SpanName, SpanSpec, SpanStarted, Timestamp,
};

/// A boxed instrumentation IO task accepted by a dispatch spawner.
pub type DispatchTask = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

/// Schedules instrumentation IO without selecting an application runtime.
pub trait TaskSpawner: Send + Sync {
  /// Schedules one task for polling to completion.
  fn spawn(&self, task: DispatchTask) -> Result<(), TaskSpawnError>;
}

/// Reports that an instrumentation task could not be scheduled.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("instrumentation task spawn failed: {code}")]
pub struct TaskSpawnError {
  code: ErrorCode,
}

impl TaskSpawnError {
  /// Creates a spawn failure with a stable machine-readable code.
  pub fn new(code: ErrorCode) -> Self {
    Self { code }
  }

  /// Returns the stable failure code.
  pub fn code(&self) -> &ErrorCode {
    &self.code
  }
}

/// Runtime-independent thread-pool scheduling for instrumentation IO.
#[derive(Clone)]
pub struct ThreadTaskSpawner {
  pool: futures_executor::ThreadPool,
}

impl ThreadTaskSpawner {
  /// Creates the default instrumentation task pool.
  pub fn new() -> Result<Self, BuildError> {
    futures_executor::ThreadPool::new().map(|pool| Self { pool }).map_err(|_| BuildError::TaskSpawnerInitialization)
  }
}

impl TaskSpawner for ThreadTaskSpawner {
  fn spawn(&self, task: DispatchTask) -> Result<(), TaskSpawnError> {
    self.pool.spawn_ok(task);
    Ok(())
  }
}

/// Identifies the routing stage where an accepted dispatch ticket failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchStage {
  /// Typed value validation or canonical encoding.
  Encode,
  /// Instrumentation task scheduling.
  Spawn,
  /// Authority mutation commit.
  AuthorityCommit,
  /// Authority snapshot or commit-history read.
  // TODO(auv-run-contract-v1-task-8): use this stable stage for projector
  // cursor establishment and gap-recovery reads once those routes are added.
  AuthorityRead,
  /// Telemetry projection.
  // TODO(auv-run-contract-v1-task-8): report projector failures at this stable
  // stage once ordered telemetry projection is an approved active route.
  Project,
  /// Projector flush.
  // TODO(auv-run-contract-v1-task-8): extend ticket barriers with projector
  // flush completion without replacing Task 6 authority barriers.
  ProjectorFlush,
  /// Detached artifact publication.
  // TODO(auv-run-contract-v1-task-9): report artifact publication failures at
  // this stable stage when detached artifact emission is implemented.
  ArtifactWrite,
}

/// One terminal routing failure retained for the next flush interval.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DispatchFailure {
  stage: DispatchStage,
  code: ErrorCode,
}

impl DispatchFailure {
  fn new(stage: DispatchStage, code: ErrorCode) -> Self {
    Self { stage, code }
  }

  /// Returns the stage that terminalized the ticket.
  pub fn stage(&self) -> DispatchStage {
    self.stage
  }

  /// Returns the validated machine-readable failure code.
  pub fn code(&self) -> &ErrorCode {
    &self.code
  }
}

/// Non-empty failures from one completed flush interval.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{} instrumentation dispatch failure(s)", .failures.len())]
pub struct FlushError {
  failures: NonEmptyVec<DispatchFailure>,
}

impl FlushError {
  /// Returns the non-zero number of failures in this interval.
  pub fn failure_count(&self) -> NonZeroUsize {
    NonZeroUsize::new(self.failures.len()).expect("FlushError is non-empty")
  }

  /// Returns the first failure in ticket order.
  pub fn first(&self) -> &DispatchFailure {
    &self.failures.as_slice()[0]
  }
}

/// Reports invalid dispatch configuration or default worker initialization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BuildError {
  /// More than one run authority was supplied to one builder.
  #[error("a dispatch accepts at most one run store authority")]
  MultipleRunStores,
  /// The default thread task spawner could not be initialized.
  #[error("the default instrumentation task spawner could not be initialized")]
  TaskSpawnerInitialization,
}

/// Builds one producer-side instrumentation dispatch.
#[derive(Default)]
pub struct DispatchBuilder {
  run_store: Option<Arc<dyn RunStore>>,
  duplicate_run_store: bool,
  task_spawner: Option<Arc<dyn TaskSpawner>>,
}

/// Starts configuring an opt-in instrumentation dispatch.
pub fn configure() -> DispatchBuilder {
  DispatchBuilder::default()
}

impl DispatchBuilder {
  /// Selects the sole durable run authority.
  pub fn run_store(mut self, store: Arc<dyn RunStore>) -> Self {
    if self.run_store.replace(store).is_some() {
      self.duplicate_run_store = true;
    }
    self
  }

  /// Overrides scheduling for instrumentation IO only.
  pub fn task_spawner(mut self, spawner: Arc<dyn TaskSpawner>) -> Self {
    self.task_spawner = Some(spawner);
    self
  }

  /// Builds a dispatch without installing it as a scoped or global default.
  pub fn build(self) -> Result<Dispatch, BuildError> {
    if self.duplicate_run_store {
      return Err(BuildError::MultipleRunStores);
    }

    let route = self.run_store.map(|store| AuthorityRoute {
      authority_id: store.authority_id(),
      store,
    });
    let spawner = match self.task_spawner {
      Some(spawner) => spawner,
      None => Arc::new(ThreadTaskSpawner::new()?),
    };

    Ok(Dispatch {
      inner: Arc::new(DispatchInner {
        route,
        spawner,
        lanes: Mutex::new(HashMap::new()),
        progress: Mutex::new(Progress::default()),
      }),
    })
  }
}

/// A configured producer-side router for AUV instrumentation IO.
#[derive(Clone)]
pub struct Dispatch {
  inner: Arc<DispatchInner>,
}

impl Dispatch {
  /// Returns the captured stable authority identity, when configured.
  pub fn authority_id(&self) -> Option<&AuthorityId> {
    self.inner.route.as_ref().map(|route| &route.authority_id)
  }

  /// Reports whether Task 6 has an active instrumentation route.
  pub fn is_enabled(&self) -> bool {
    self.inner.route.is_some()
  }

  /// Captures the current ticket barrier and returns its cancellation-safe waiter.
  pub fn flush(&self) -> crate::BoxFuture<'_, Result<(), FlushError>> {
    let (sender, receiver) = oneshot::channel();
    {
      let mut progress = self.inner.progress.lock().unwrap();
      let barrier = progress.next_ticket;
      progress.flushes.push_back(FlushWaiter { barrier, sender });
      progress.complete_flushes();
    }
    let keep_alive = self.clone();
    Box::pin(async move {
      let result = receiver.await.expect("dispatch remains alive until its flush resolves");
      drop(keep_alive);
      result
    })
  }

  pub(crate) fn submit_span<S: SpanSpec>(&self, run_id: RunId, parent_span_id: Option<SpanId>, span_id: SpanId, spec: S) {
    let ticket = self.reserve_ticket(run_id);
    let mutation = (|| {
      let name = SpanName::parse(S::NAME).map_err(|_| encode_code())?;
      let attributes = spec.attributes();
      serde_json::to_vec(&attributes).map_err(|_| encode_code())?;
      Ok(RunMutation::StartSpan(SpanStarted::new(span_id, parent_span_id, None, name, now(), attributes)))
    })();
    self.submit_mutation(ticket, run_id, mutation);
  }

  pub(crate) fn submit_event<E: EventPayload>(&self, run_id: RunId, span_id: Option<SpanId>, event: E) {
    let ticket = self.reserve_ticket(run_id);
    let mutation = (|| {
      let schema = EventSchema::for_payload::<E>().map_err(|_| encode_code())?;
      let payload = JsonPayload::encode(&event).map_err(|_| encode_code())?;
      Ok(RunMutation::EmitEvent(EventOccurred::new(EventId::new(), span_id, now(), schema, payload)))
    })();
    self.submit_mutation(ticket, run_id, mutation);
  }

  fn reserve_ticket(&self, run_id: RunId) -> u64 {
    let mut progress = self.inner.progress.lock().unwrap();
    progress.next_ticket = progress.next_ticket.checked_add(1).expect("dispatch ticket space exhausted");
    let ticket = progress.next_ticket;
    self.inner.lanes.lock().unwrap().entry(run_id).or_default().queue.push_back(LaneEntry {
      ticket,
      state: LaneEntryState::Preparing,
    });
    ticket
  }

  fn submit_mutation(&self, ticket: u64, run_id: RunId, mutation: Result<RunMutation, ErrorCode>) {
    let request = mutation.and_then(|mutation| {
      let route = self.inner.route.as_ref().expect("enabled dispatch has an authority route");
      RunCommitRequest::new(route.authority_id, run_id, IdempotencyKey::new(), vec![mutation]).map_err(|_| encode_code())
    });
    {
      let mut lanes = self.inner.lanes.lock().unwrap();
      let lane = lanes.get_mut(&run_id).expect("ticket reservation creates its run lane");
      let entry = lane.queue.iter_mut().find(|entry| entry.ticket == ticket).expect("reserved ticket remains in its run lane");
      entry.state = match request {
        Ok(request) => LaneEntryState::Ready(request),
        Err(code) => LaneEntryState::Failed(DispatchFailure::new(DispatchStage::Encode, code)),
      };
    }
    self.wake_lane(run_id);
  }

  fn wake_lane(&self, run_id: RunId) {
    loop {
      let action = {
        let mut lanes = self.inner.lanes.lock().unwrap();
        let Some(lane) = lanes.get_mut(&run_id) else {
          return;
        };
        if lane.running {
          return;
        }
        match lane.queue.front().map(|entry| &entry.state) {
          Some(LaneEntryState::Preparing) => return,
          Some(LaneEntryState::Ready(_)) => {
            lane.running = true;
            LaneWake::Spawn
          }
          Some(LaneEntryState::Failed(_)) => {
            let entry = lane.queue.pop_front().expect("front was present");
            let LaneEntryState::Failed(failure) = entry.state else {
              unreachable!("matched failed lane entry")
            };
            LaneWake::Terminal(entry.ticket, failure)
          }
          None => {
            lanes.remove(&run_id);
            return;
          }
        }
      };

      match action {
        LaneWake::Terminal(ticket, failure) => self.terminalize(ticket, Some(failure)),
        LaneWake::Spawn => {
          let dispatch = self.clone();
          match self.inner.spawner.spawn(Box::pin(async move { dispatch.drain_run(run_id).await })) {
            Ok(()) => return,
            Err(error) => {
              let ticket = {
                let mut lanes = self.inner.lanes.lock().unwrap();
                let lane = lanes.get_mut(&run_id).expect("failed spawn leaves its lane registered");
                lane.running = false;
                let entry = lane.queue.pop_front().expect("spawn was requested for a ready entry");
                debug_assert!(matches!(entry.state, LaneEntryState::Ready(_)));
                entry.ticket
              };
              self.terminalize(ticket, Some(DispatchFailure::new(DispatchStage::Spawn, error.code().clone())));
            }
          }
        }
      }
    }
  }

  async fn drain_run(&self, run_id: RunId) {
    loop {
      let action = {
        let mut lanes = self.inner.lanes.lock().unwrap();
        let Some(lane) = lanes.get_mut(&run_id) else {
          return;
        };
        match lane.queue.front().map(|entry| &entry.state) {
          Some(LaneEntryState::Preparing) => {
            lane.running = false;
            return;
          }
          Some(LaneEntryState::Ready(_)) | Some(LaneEntryState::Failed(_)) => {
            let entry = lane.queue.pop_front().expect("front was present");
            match entry.state {
              LaneEntryState::Ready(request) => LaneAction::Commit(entry.ticket, request),
              LaneEntryState::Failed(failure) => LaneAction::Terminal(entry.ticket, failure),
              LaneEntryState::Preparing => unreachable!("matched a terminal lane entry"),
            }
          }
          None => {
            lanes.remove(&run_id);
            return;
          }
        }
      };

      match action {
        LaneAction::Terminal(ticket, failure) => self.terminalize(ticket, Some(failure)),
        LaneAction::Commit(ticket, request) => {
          let route = self.inner.route.as_ref().expect("authority lane requires a route");
          let failure = route
            .store
            .commit(request)
            .await
            .err()
            .map(|error| DispatchFailure::new(DispatchStage::AuthorityCommit, commit_error_code(error)));
          self.terminalize(ticket, failure);
        }
      }
    }
  }

  fn terminalize(&self, ticket: u64, failure: Option<DispatchFailure>) {
    let mut progress = self.inner.progress.lock().unwrap();
    progress.terminalize(ticket, failure);
    progress.complete_flushes();
  }
}

struct DispatchInner {
  route: Option<AuthorityRoute>,
  spawner: Arc<dyn TaskSpawner>,
  lanes: Mutex<HashMap<RunId, RunLane>>,
  progress: Mutex<Progress>,
}

struct AuthorityRoute {
  authority_id: AuthorityId,
  store: Arc<dyn RunStore>,
}

#[derive(Default)]
struct RunLane {
  running: bool,
  queue: VecDeque<LaneEntry>,
}

struct LaneEntry {
  ticket: u64,
  state: LaneEntryState,
}

enum LaneEntryState {
  Preparing,
  Ready(RunCommitRequest),
  Failed(DispatchFailure),
}

enum LaneWake {
  Spawn,
  Terminal(u64, DispatchFailure),
}

enum LaneAction {
  Commit(u64, RunCommitRequest),
  Terminal(u64, DispatchFailure),
}

#[derive(Default)]
struct Progress {
  next_ticket: u64,
  terminal_prefix: u64,
  out_of_order: BTreeMap<u64, Option<DispatchFailure>>,
  failures: BTreeMap<u64, DispatchFailure>,
  reported_through: u64,
  flushes: VecDeque<FlushWaiter>,
}

impl Progress {
  fn terminalize(&mut self, ticket: u64, failure: Option<DispatchFailure>) {
    let replaced = self.out_of_order.insert(ticket, failure);
    debug_assert!(replaced.is_none(), "a dispatch ticket terminalized more than once");
    while let Some(failure) = self.out_of_order.remove(&(self.terminal_prefix + 1)) {
      self.terminal_prefix += 1;
      if let Some(failure) = failure {
        self.failures.insert(self.terminal_prefix, failure);
      }
    }
  }

  fn complete_flushes(&mut self) {
    while self.flushes.front().is_some_and(|flush| flush.barrier <= self.terminal_prefix) {
      let flush = self.flushes.pop_front().expect("front was present");
      let failures = if flush.barrier > self.reported_through {
        self.failures.range((self.reported_through + 1)..=flush.barrier).map(|(_, failure)| failure.clone()).collect::<Vec<_>>()
      } else {
        Vec::new()
      };
      self.failures.retain(|ticket, _| *ticket > flush.barrier);
      self.reported_through = flush.barrier;
      let result = NonEmptyVec::new(failures).map(|failures| Err(FlushError { failures })).unwrap_or(Ok(()));
      let _ = flush.sender.send(result);
    }
  }
}

struct FlushWaiter {
  barrier: u64,
  sender: oneshot::Sender<Result<(), FlushError>>,
}

fn now() -> Timestamp {
  let now = SystemTime::now().duration_since(UNIX_EPOCH).expect("system time is after the Unix epoch");
  Timestamp::new(now.as_secs() as i64, now.subsec_nanos()).expect("system time is representable by the run contract")
}

fn encode_code() -> ErrorCode {
  ErrorCode::parse("auv.dispatch.encode").expect("static dispatch error code is valid")
}

fn commit_error_code(error: CommitError) -> ErrorCode {
  match error {
    CommitError::Rejected(code) | CommitError::Unavailable(code) | CommitError::CommitUnknown(code) => code,
    CommitError::AuthorityMismatch { .. } => {
      ErrorCode::parse("auv.dispatch.authority_mismatch").expect("static dispatch error code is valid")
    }
    CommitError::IdempotencyMismatch => ErrorCode::parse("auv.dispatch.idempotency_mismatch").expect("static dispatch error code is valid"),
  }
}

/// Selects scoped and process-global dispatch defaults.
pub mod dispatcher {
  use super::{Dispatch, RefCell};
  use std::marker::PhantomData;
  use std::rc::Rc;

  thread_local! {
    static SCOPED_DISPATCHES: RefCell<Vec<Dispatch>> = const { RefCell::new(Vec::new()) };
  }

  static GLOBAL_DISPATCH: super::OnceLock<Dispatch> = super::OnceLock::new();

  /// Reports that the one-time process-global dispatch was already installed.
  #[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
  #[error("the process-global AUV dispatch is already installed")]
  pub struct SetGlobalDefaultError;

  /// Installs the process-global dispatch exactly once.
  pub fn set_global_default(dispatch: Dispatch) -> Result<(), SetGlobalDefaultError> {
    GLOBAL_DISPATCH.set(dispatch).map_err(|_| SetGlobalDefaultError)
  }

  /// Runs a synchronous closure with a thread-local dispatch override.
  pub fn with_default<T>(dispatch: &Dispatch, f: impl FnOnce() -> T) -> T {
    let depth = SCOPED_DISPATCHES.with(|dispatches| {
      let mut dispatches = dispatches.borrow_mut();
      let depth = dispatches.len();
      dispatches.push(dispatch.clone());
      depth
    });
    let _guard = ScopedDispatchGuard {
      depth,
      thread_bound: PhantomData,
    };
    f()
  }

  pub(crate) fn current() -> Option<Dispatch> {
    SCOPED_DISPATCHES.with(|dispatches| dispatches.borrow().last().cloned()).or_else(|| GLOBAL_DISPATCH.get().cloned())
  }

  struct ScopedDispatchGuard {
    depth: usize,
    thread_bound: PhantomData<Rc<()>>,
  }

  impl Drop for ScopedDispatchGuard {
    fn drop(&mut self) {
      SCOPED_DISPATCHES.with(|dispatches| {
        let mut dispatches = dispatches.borrow_mut();
        debug_assert_eq!(dispatches.len(), self.depth + 1, "dispatch scopes must drop in nesting order");
        dispatches.truncate(self.depth);
      });
    }
  }
}

// TODO(auv-run-contract-v1-task-7): add non-blocking DispatchErrorReporter
// callbacks without exposing retry policy through the producer API.
// TODO(auv-run-contract-v1-task-8): add projector routes and authority reads to
// the existing ticket barrier; a route-less dispatch remains disabled in Task 6.
// TODO(auv-run-contract-v1-task-9): add detached artifact admission and writes
// without moving byte transfer into the per-run authority fact lane.
