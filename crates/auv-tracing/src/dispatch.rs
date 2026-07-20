use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::future::Future;
use std::num::NonZeroUsize;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::task::{Context as TaskContext, Poll, Waker};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::FutureExt;

use crate::{
  AuthorityId, CommitError, ErrorCode, EventId, EventOccurred, EventPayload, EventSchema, IdempotencyKey, JsonPayload, NonEmptyVec,
  RunCommitRequest, RunId, RunMutation, RunStore, SpanEnded, SpanId, SpanLink, SpanName, SpanSpec, SpanStarted, Timestamp,
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
}

/// Runtime-independent thread-pool scheduling for instrumentation IO.
#[derive(Clone)]
pub struct ThreadTaskSpawner {
  pool: Arc<futures_executor::ThreadPool>,
}

impl ThreadTaskSpawner {
  fn new() -> Result<Self, BuildError> {
    static DEFAULT_POOL: OnceLock<Option<Arc<futures_executor::ThreadPool>>> = OnceLock::new();
    DEFAULT_POOL
      .get_or_init(|| futures_executor::ThreadPool::builder().pool_size(2).create().ok().map(Arc::new))
      .clone()
      .map(|pool| Self { pool })
      .ok_or(BuildError::TaskSpawnerInitialization)
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
    let spawner: Option<Arc<dyn TaskSpawner>> = match (self.task_spawner, route.is_some()) {
      (Some(spawner), _) => Some(spawner),
      (None, true) => Some(Arc::new(ThreadTaskSpawner::new()?)),
      (None, false) => None,
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
  pub(crate) fn authority_id(&self) -> Option<&AuthorityId> {
    self.inner.route.as_ref().map(|route| &route.authority_id)
  }

  pub(crate) fn is_enabled(&self) -> bool {
    self.inner.route.is_some()
  }

  /// Captures the current ticket barrier and returns its cancellation-safe waiter.
  ///
  /// Outstanding flushes consume reporting intervals in call order. Callers
  /// must poll or drop each earlier flush before awaiting a later one.
  pub fn flush(&self) -> crate::BoxFuture<'_, Result<(), FlushError>> {
    let ordering_id = {
      let mut progress = self.inner.progress.lock().unwrap();
      progress.register_flush()
    };
    Box::pin(FlushFuture {
      dispatch: self.clone(),
      ordering_id: Some(ordering_id),
    })
  }

  pub(crate) fn submit_span_start<S: SpanSpec>(
    &self,
    run_id: RunId,
    parent_span_id: Option<SpanId>,
    remote_link: Option<SpanLink>,
    span_id: SpanId,
    started_at: Result<Timestamp, ErrorCode>,
    spec: S,
  ) -> bool {
    let preparation = self.reserve_ticket(run_id);
    let mutation = (|| {
      let name = SpanName::parse(S::NAME).map_err(|_| encode_code())?;
      let attributes = spec.attributes();
      serde_json::to_vec(&attributes).map_err(|_| encode_code())?;
      Ok(RunMutation::StartSpan(SpanStarted::new(span_id, parent_span_id, remote_link, name, started_at?, attributes)))
    })();
    let prepared = mutation.is_ok();
    preparation.complete(mutation);
    prepared
  }

  pub(crate) fn submit_span_end(&self, run_id: RunId, span_id: SpanId, ended_at: Result<Timestamp, ErrorCode>) {
    let preparation = self.reserve_ticket(run_id);
    preparation.complete(ended_at.map(|ended_at| RunMutation::EndSpan(SpanEnded::new(span_id, ended_at))));
  }

  pub(crate) fn submit_event<E: EventPayload>(&self, run_id: RunId, span_id: Option<SpanId>, event: E) {
    let preparation = self.reserve_ticket(run_id);
    let mutation = (|| {
      let schema = EventSchema::for_payload::<E>().map_err(|_| encode_code())?;
      let payload = JsonPayload::encode(&event).map_err(|_| encode_code())?;
      Ok(RunMutation::EmitEvent(EventOccurred::new(EventId::new(), span_id, timestamp_now()?, schema, payload)))
    })();
    preparation.complete(mutation);
  }

  fn reserve_ticket(&self, run_id: RunId) -> PreparationGuard {
    let mut progress = self.inner.progress.lock().unwrap();
    progress.next_ticket = progress.next_ticket.checked_add(1).expect("dispatch ticket space exhausted");
    let ticket = progress.next_ticket;
    // TODO(auv-run-contract-v1-task-8): define bounded admission/backpressure
    // with DispatchErrorReporter policy before imposing any queue capacity.
    self.inner.lanes.lock().unwrap().entry(run_id).or_default().queue.push_back(LaneEntry {
      ticket,
      state: LaneEntryState::Preparing,
    });
    PreparationGuard {
      dispatch: self.clone(),
      run_id,
      ticket,
      armed: true,
    }
  }

  fn complete_preparation(&self, ticket: u64, run_id: RunId, mutation: Result<RunMutation, ErrorCode>) {
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
          let admission = Arc::new(SpawnAdmission::new());
          let task_admission = admission.clone();
          let dispatch = self.clone();
          let task = Box::pin(async move {
            let admitted = async move {
              if task_admission.start() {
                dispatch.drain_run(run_id).await;
              }
            };
            // The lane guard repairs the active ticket while `admitted` unwinds;
            // this boundary only prevents that panic from terminating a worker.
            let _ = AssertUnwindSafe(admitted).catch_unwind().await;
          });
          let spawn = catch_unwind(AssertUnwindSafe(|| self.spawner().spawn(task)));
          let failure_code = match spawn {
            Ok(Ok(())) => return,
            Ok(Err(error)) if admission.cancel_before_start() => error.code,
            Err(_) if admission.cancel_before_start() => spawn_panic_code(),
            Ok(Err(_)) | Err(_) => return,
          };
          let ticket = {
            let mut lanes = self.inner.lanes.lock().unwrap();
            let lane = lanes.get_mut(&run_id).expect("failed spawn leaves its lane registered");
            lane.running = false;
            let entry = lane.queue.pop_front().expect("spawn was requested for a ready entry");
            debug_assert!(matches!(entry.state, LaneEntryState::Ready(_)));
            entry.ticket
          };
          self.terminalize(ticket, Some(DispatchFailure::new(DispatchStage::Spawn, failure_code)));
        }
      }
    }
  }

  fn spawner(&self) -> &dyn TaskSpawner {
    self.inner.spawner.as_deref().expect("authority-backed dispatch has a task spawner")
  }

  async fn drain_run(&self, run_id: RunId) {
    let mut guard = LaneDrainGuard::new(self.clone(), run_id);
    loop {
      let action = {
        let mut lanes = self.inner.lanes.lock().unwrap();
        let Some(lane) = lanes.get_mut(&run_id) else {
          guard.disarm();
          return;
        };
        match lane.queue.front().map(|entry| &entry.state) {
          Some(LaneEntryState::Preparing) => {
            lane.running = false;
            guard.disarm();
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
            guard.disarm();
            return;
          }
        }
      };

      match action {
        LaneAction::Terminal(ticket, failure) => self.terminalize(ticket, Some(failure)),
        LaneAction::Commit(ticket, request) => {
          guard.activate(ticket);
          let route = self.inner.route.as_ref().expect("authority lane requires a route");
          let failure = route
            .store
            .commit(request)
            .await
            .err()
            .map(|error| DispatchFailure::new(DispatchStage::AuthorityCommit, commit_error_code(error)));
          guard.complete(ticket);
          self.terminalize(ticket, failure);
        }
      }
    }
  }

  fn terminalize(&self, ticket: u64, failure: Option<DispatchFailure>) {
    let waker = {
      let mut progress = self.inner.progress.lock().unwrap();
      progress.terminalize(ticket, failure);
      progress.take_ready_front_waker()
    };
    if let Some(waker) = waker {
      waker.wake();
    }
  }

  fn fail_preparation(&self, run_id: RunId, ticket: u64) {
    let changed = {
      let mut lanes = self.inner.lanes.lock().unwrap();
      let Some(lane) = lanes.get_mut(&run_id) else {
        return;
      };
      let Some(entry) = lane.queue.iter_mut().find(|entry| entry.ticket == ticket) else {
        return;
      };
      if matches!(entry.state, LaneEntryState::Preparing) {
        entry.state = LaneEntryState::Failed(DispatchFailure::new(DispatchStage::Encode, encode_code()));
        true
      } else {
        false
      }
    };
    debug_assert!(changed, "preparation guard owns one Preparing lane entry");
    if changed {
      self.wake_lane(run_id);
    }
  }

  fn recover_lane_task(&self, run_id: RunId, active_ticket: Option<u64>) {
    if let Some(lane) = self.inner.lanes.lock().unwrap().get_mut(&run_id) {
      lane.running = false;
    }
    if let Some(ticket) = active_ticket {
      self.terminalize(ticket, Some(DispatchFailure::new(DispatchStage::AuthorityCommit, task_unwind_code())));
    }
    self.wake_lane(run_id);
  }
}

struct SpawnAdmission(AtomicU8);

impl SpawnAdmission {
  const ADMITTING: u8 = 0;
  const STARTED: u8 = 1;
  const CANCELED: u8 = 2;

  fn new() -> Self {
    Self(AtomicU8::new(Self::ADMITTING))
  }

  fn start(&self) -> bool {
    self.0.compare_exchange(Self::ADMITTING, Self::STARTED, Ordering::AcqRel, Ordering::Acquire).is_ok()
  }

  fn cancel_before_start(&self) -> bool {
    self.0.compare_exchange(Self::ADMITTING, Self::CANCELED, Ordering::AcqRel, Ordering::Acquire).is_ok()
  }
}

struct LaneDrainGuard {
  dispatch: Dispatch,
  run_id: RunId,
  active_ticket: Option<u64>,
  armed: bool,
}

impl LaneDrainGuard {
  fn new(dispatch: Dispatch, run_id: RunId) -> Self {
    Self {
      dispatch,
      run_id,
      active_ticket: None,
      armed: true,
    }
  }

  fn activate(&mut self, ticket: u64) {
    debug_assert!(self.active_ticket.replace(ticket).is_none(), "lane task owns at most one active ticket");
  }

  fn complete(&mut self, ticket: u64) {
    debug_assert_eq!(self.active_ticket, Some(ticket), "lane task completes its active ticket");
    self.active_ticket = None;
  }

  fn disarm(&mut self) {
    self.armed = false;
  }
}

impl Drop for LaneDrainGuard {
  fn drop(&mut self) {
    if self.armed {
      self.dispatch.recover_lane_task(self.run_id, self.active_ticket.take());
    }
  }
}

struct PreparationGuard {
  dispatch: Dispatch,
  run_id: RunId,
  ticket: u64,
  armed: bool,
}

impl PreparationGuard {
  fn complete(mut self, mutation: Result<RunMutation, ErrorCode>) {
    self.dispatch.complete_preparation(self.ticket, self.run_id, mutation);
    self.armed = false;
    self.dispatch.wake_lane(self.run_id);
  }
}

impl Drop for PreparationGuard {
  fn drop(&mut self) {
    if self.armed {
      self.dispatch.fail_preparation(self.run_id, self.ticket);
    }
  }
}

struct FlushFuture {
  dispatch: Dispatch,
  ordering_id: Option<u64>,
}

impl Future for FlushFuture {
  type Output = Result<(), FlushError>;

  fn poll(mut self: Pin<&mut Self>, context: &mut TaskContext<'_>) -> Poll<Self::Output> {
    let ordering_id = self.ordering_id.expect("completed flush futures must not be polled again");
    let (result, waker) = {
      let mut progress = self.dispatch.inner.progress.lock().unwrap();
      progress.poll_flush(ordering_id, context)
    };
    if result.is_ready() {
      self.ordering_id = None;
    }
    if let Some(waker) = waker {
      waker.wake();
    }
    result
  }
}

impl Drop for FlushFuture {
  fn drop(&mut self) {
    let Some(ordering_id) = self.ordering_id.take() else {
      return;
    };
    let waker = self.dispatch.inner.progress.lock().unwrap().cancel_flush(ordering_id);
    if let Some(waker) = waker {
      waker.wake();
    }
  }
}

struct DispatchInner {
  route: Option<AuthorityRoute>,
  spawner: Option<Arc<dyn TaskSpawner>>,
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
  next_flush_id: u64,
  terminal_prefix: u64,
  success_ranges: BTreeMap<u64, u64>,
  out_of_order_failures: BTreeMap<u64, DispatchFailure>,
  failures: BTreeMap<u64, DispatchFailure>,
  reported_through: u64,
  flushes: VecDeque<FlushRegistration>,
}

impl Progress {
  fn register_flush(&mut self) -> u64 {
    self.next_flush_id = self.next_flush_id.checked_add(1).expect("flush ordering ID space exhausted");
    let ordering_id = self.next_flush_id;
    self.flushes.push_back(FlushRegistration {
      ordering_id,
      barrier: self.next_ticket,
      waker: None,
    });
    ordering_id
  }

  fn terminalize(&mut self, ticket: u64, failure: Option<DispatchFailure>) {
    if ticket <= self.terminal_prefix
      || self.out_of_order_failures.contains_key(&ticket)
      || self.success_ranges.range(..=ticket).next_back().is_some_and(|(_, end)| *end >= ticket)
    {
      debug_assert!(false, "a dispatch ticket terminalized more than once");
      return;
    }

    if let Some(failure) = failure {
      self.out_of_order_failures.insert(ticket, failure);
    } else {
      self.insert_success(ticket);
    }

    while let Some(next) = self.terminal_prefix.checked_add(1) {
      if let Some(failure) = self.out_of_order_failures.remove(&next) {
        self.terminal_prefix = next;
        self.failures.insert(next, failure);
      } else if let Some(end) = self.success_ranges.remove(&next) {
        self.terminal_prefix = end;
      } else {
        break;
      }
    }
  }

  fn insert_success(&mut self, ticket: u64) {
    let mut start = ticket;
    if let Some((&left_start, &left_end)) = self.success_ranges.range(..ticket).next_back()
      && left_end.checked_add(1) == Some(ticket)
    {
      start = left_start;
      self.success_ranges.remove(&left_start);
    }

    let mut end = ticket;
    if let Some(right_start) = ticket.checked_add(1)
      && let Some(right_end) = self.success_ranges.remove(&right_start)
    {
      end = right_end;
    }
    self.success_ranges.insert(start, end);
  }

  fn poll_flush(&mut self, ordering_id: u64, context: &mut TaskContext<'_>) -> (Poll<Result<(), FlushError>>, Option<Waker>) {
    let position = self.flushes.iter().position(|flush| flush.ordering_id == ordering_id).expect("live flush future remains registered");
    if position != 0 || self.flushes[0].barrier > self.terminal_prefix {
      self.flushes[position].waker = Some(context.waker().clone());
      return (Poll::Pending, None);
    }

    let flush = self.flushes.pop_front().expect("front was present");
    let failures = if flush.barrier > self.reported_through {
      self.failures.range((self.reported_through + 1)..=flush.barrier).map(|(_, failure)| failure.clone()).collect::<Vec<_>>()
    } else {
      Vec::new()
    };
    self.failures.retain(|ticket, _| *ticket > flush.barrier);
    self.reported_through = flush.barrier;
    let waker = self.take_front_waker();
    (Poll::Ready(NonEmptyVec::new(failures).map(|failures| Err(FlushError { failures })).unwrap_or(Ok(()))), waker)
  }

  fn cancel_flush(&mut self, ordering_id: u64) -> Option<Waker> {
    let position = self.flushes.iter().position(|flush| flush.ordering_id == ordering_id)?;
    self.flushes.remove(position);
    if position == 0 {
      self.take_front_waker()
    } else {
      None
    }
  }

  fn take_ready_front_waker(&mut self) -> Option<Waker> {
    if self.flushes.front().is_some_and(|flush| flush.barrier <= self.terminal_prefix) {
      self.take_front_waker()
    } else {
      None
    }
  }

  fn take_front_waker(&mut self) -> Option<Waker> {
    self.flushes.front_mut().and_then(|flush| flush.waker.take())
  }
}

struct FlushRegistration {
  ordering_id: u64,
  barrier: u64,
  waker: Option<Waker>,
}

pub(crate) fn timestamp_now() -> Result<Timestamp, ErrorCode> {
  timestamp_from_system_time(SystemTime::now())
}

// Keeps lifecycle duration monotonic while retaining the exact wall timestamp
// used by the start fact. Every representability failure stays on the encode route.
pub(crate) fn timestamp_after(started_at: Timestamp, started_tick: Duration, ended_tick: Duration) -> Result<Timestamp, ErrorCode> {
  let elapsed = ended_tick.checked_sub(started_tick).ok_or_else(encode_code)?;
  let nanoseconds = u64::from(started_at.nanoseconds()) + u64::from(elapsed.subsec_nanos());
  let carry_seconds = nanoseconds / 1_000_000_000;
  let unix_seconds = i128::from(started_at.unix_seconds()) + i128::from(elapsed.as_secs()) + i128::from(carry_seconds);
  let unix_seconds = i64::try_from(unix_seconds).map_err(|_| encode_code())?;
  Timestamp::new(unix_seconds, (nanoseconds % 1_000_000_000) as u32).map_err(|_| encode_code())
}

fn timestamp_from_system_time(value: SystemTime) -> Result<Timestamp, ErrorCode> {
  let duration = value.duration_since(UNIX_EPOCH).map_err(|_| encode_code())?;
  timestamp_from_unix_duration(duration)
}

fn timestamp_from_unix_duration(value: Duration) -> Result<Timestamp, ErrorCode> {
  let seconds = i64::try_from(value.as_secs()).map_err(|_| encode_code())?;
  Timestamp::new(seconds, value.subsec_nanos()).map_err(|_| encode_code())
}

fn encode_code() -> ErrorCode {
  ErrorCode::parse("auv.dispatch.encode").expect("static dispatch error code is valid")
}

fn spawn_panic_code() -> ErrorCode {
  ErrorCode::parse("auv.dispatch.spawn_panic").expect("static dispatch error code is valid")
}

fn task_unwind_code() -> ErrorCode {
  ErrorCode::parse("auv.dispatch.task_unwind").expect("static dispatch error code is valid")
}

fn commit_error_code(error: CommitError) -> ErrorCode {
  match error {
    CommitError::Rejected(code) | CommitError::Unavailable(code) => code,
    CommitError::CommitUnknown(code) => {
      // TODO(auv-run-contract-v1-task-8): resolve unknown authority outcomes
      // through lookup/quarantine before projector routing is enabled.
      code
    }
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

// TODO(auv-run-contract-v1-task-8): add non-blocking DispatchErrorReporter
// callbacks without exposing retry policy through the producer API.
// TODO(auv-run-contract-v1-task-8): add projector routes and authority reads to
// the existing ticket barrier; a route-less dispatch remains disabled in Task 6.
// TODO(auv-run-contract-v1-task-9): add detached artifact admission and writes
// without moving byte transfer into the per-run authority fact lane.

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn route_less_dispatch_does_not_allocate_a_default_worker() {
    let dispatch = configure().build().unwrap();
    assert!(dispatch.inner.spawner.is_none());
  }

  #[test]
  fn authority_defaults_share_one_small_worker_pool() {
    let first = ThreadTaskSpawner::new().unwrap();
    let second = ThreadTaskSpawner::new().unwrap();
    assert!(Arc::ptr_eq(&first.pool, &second.pool));
  }

  #[test]
  fn clock_values_before_unix_epoch_are_encode_failures() {
    let before_epoch = UNIX_EPOCH.checked_sub(std::time::Duration::from_secs(1)).unwrap();
    assert_eq!(timestamp_from_system_time(before_epoch).unwrap_err().as_str(), "auv.dispatch.encode");
  }

  #[test]
  fn clock_seconds_that_do_not_fit_i64_are_encode_failures() {
    let overflow = std::time::Duration::from_secs(i64::MAX as u64 + 1);
    assert_eq!(timestamp_from_unix_duration(overflow).unwrap_err().as_str(), "auv.dispatch.encode");
  }

  #[test]
  fn out_of_order_successes_coalesce_around_preserved_failures() {
    let mut progress = Progress::default();
    for ticket in 2..=5_000 {
      progress.terminalize(ticket, None);
    }
    progress.terminalize(5_001, Some(DispatchFailure::new(DispatchStage::Spawn, spawn_panic_code())));
    for ticket in 5_002..=10_000 {
      progress.terminalize(ticket, None);
    }

    assert_eq!(progress.success_ranges.len(), 2);
    assert_eq!(progress.out_of_order_failures.len(), 1);
    progress.terminalize(1, None);
    assert_eq!(progress.terminal_prefix, 10_000);
    assert!(progress.success_ranges.is_empty());
    assert!(progress.out_of_order_failures.is_empty());
    assert_eq!(progress.failures.get(&5_001).unwrap().stage(), DispatchStage::Spawn);
  }
}
