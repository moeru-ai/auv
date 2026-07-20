use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::future::Future;
use std::num::NonZeroUsize;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};
use std::task::{Context as TaskContext, Poll, Waker};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::FutureExt;

use crate::{
  AuthorityId, CommitError, DispatchErrorReporter, ErrorCode, EventId, EventOccurred, EventPayload, EventSchema, IdempotencyKey,
  JsonPayload, NonEmptyVec, PageLimit, ReadError, RunCommit, RunCommitRequest, RunFact, RunId, RunMutation, RunRevision, RunStore,
  RunSubscription, SpanEnded, SpanId, SpanLink, SpanName, SpanSpec, SpanStarted, SubscriptionError, TelemetryItem, TelemetryProjector,
  TelemetryRoutePolicy, Timestamp,
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

  /// Returns the stable machine-readable failure code.
  pub fn code(&self) -> &ErrorCode {
    &self.code
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
  AuthorityRead,
  /// Telemetry projection.
  Project,
  /// Projector flush.
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
  projector_routes: Vec<ProjectorRoute>,
  error_reporter: Option<Arc<dyn DispatchErrorReporter>>,
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

  /// Adds one ordered telemetry projector and its producer-attribute policy.
  pub fn project_telemetry(mut self, projector: Arc<dyn TelemetryProjector>, policy: TelemetryRoutePolicy) -> Self {
    self.projector_routes.push(ProjectorRoute { projector, policy });
    self
  }

  /// Selects the non-blocking diagnostic sink for asynchronous failures.
  pub fn on_error(mut self, reporter: Arc<dyn DispatchErrorReporter>) -> Self {
    self.error_reporter = Some(reporter);
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
    let has_async_routes = route.is_some() || !self.projector_routes.is_empty();
    let spawner: Option<Arc<dyn TaskSpawner>> = match (self.task_spawner, has_async_routes) {
      (Some(spawner), _) => Some(spawner),
      (None, true) => Some(Arc::new(ThreadTaskSpawner::new()?)),
      (None, false) => None,
    };

    Ok(Dispatch {
      inner: Arc::new(DispatchInner {
        route,
        spawner,
        projector_routes: self.projector_routes,
        reporter: self.error_reporter.unwrap_or_else(|| Arc::new(DiscardReporter)),
        lanes: Mutex::new(HashMap::new()),
        projection: Mutex::new(ProjectionState::default()),
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
    self.inner.route.is_some() || !self.inner.projector_routes.is_empty()
  }

  /// Captures the current ticket barrier and returns its cancellation-safe waiter.
  ///
  /// Outstanding flushes consume reporting intervals in call order. Callers
  /// must poll or drop each earlier flush before awaiting a later one.
  pub fn flush(&self) -> crate::BoxFuture<'_, Result<(), FlushError>> {
    let has_projectors = !self.inner.projector_routes.is_empty();
    let ordering_id = {
      // Ticket allocation uses this same lock, so the projector marker cannot
      // be overtaken by a post-barrier emission.
      let mut progress = self.inner.progress.lock().unwrap();
      let registration = progress.register_flush(!has_projectors);
      if has_projectors {
        self.inner.projection.lock().unwrap().flushes.push_back(ProjectionFlush {
          ordering_id: registration.0,
          barrier: registration.1,
        });
      }
      registration.0
    };
    if has_projectors {
      self.wake_projection();
    }
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
    if preparation.is_rejected() {
      drop(spec);
      preparation.finish_rejected();
      return false;
    }
    let mutation = (|| {
      let name = SpanName::parse(S::NAME).map_err(|_| encode_code())?;
      let attributes = SpanSpec::attributes(&spec);
      serde_json::to_vec(&attributes).map_err(|_| encode_code())?;
      Ok(RunMutation::StartSpan(SpanStarted::new(span_id, parent_span_id, remote_link, name, started_at?, attributes)))
    })();
    drop(spec);
    let prepared = mutation.is_ok();
    preparation.complete(mutation);
    prepared
  }

  pub(crate) fn submit_span_end(&self, run_id: RunId, span_id: SpanId, ended_at: Result<Timestamp, ErrorCode>) {
    let preparation = self.reserve_ticket(run_id);
    if preparation.is_rejected() {
      preparation.finish_rejected();
      return;
    }
    preparation.complete(ended_at.map(|ended_at| RunMutation::EndSpan(SpanEnded::new(span_id, ended_at))));
  }

  pub(crate) fn submit_event<E: EventPayload>(
    &self,
    run_id: RunId,
    span_id: Option<SpanId>,
    occurred_at: Result<Timestamp, ErrorCode>,
    event: E,
  ) {
    let preparation = self.reserve_ticket(run_id);
    if preparation.is_rejected() {
      drop(event);
      preparation.finish_rejected();
      return;
    }
    let mutation = (|| {
      let schema = EventSchema::for_payload::<E>().map_err(|_| encode_code())?;
      let payload = JsonPayload::encode(&event).map_err(|_| encode_code())?;
      Ok(RunMutation::EmitEvent(EventOccurred::new(EventId::new(), span_id, occurred_at?, schema, payload)))
    })();
    drop(event);
    preparation.complete(mutation);
  }

  fn reserve_ticket(&self, run_id: RunId) -> PreparationGuard {
    let ticket = {
      let mut progress = self.inner.progress.lock().unwrap();
      progress.next_ticket = progress.next_ticket.checked_add(1).expect("dispatch ticket space exhausted");
      if !self.inner.projector_routes.is_empty() {
        self.inner.projection.lock().unwrap().entries.insert(progress.next_ticket, ProjectionEntry::Preparing);
      }
      progress.next_ticket
    };
    // TODO(dispatch-backpressure-v1): admission limits remain deferred because
    // V1 has no owner-approved capacity policy; add one only with explicit
    // backpressure and DispatchErrorReporter semantics.
    let rejected = if self.inner.route.is_some() {
      let mut lanes = self.inner.lanes.lock().unwrap();
      let lane = lanes.entry(run_id).or_default();
      let rejected = lane.indeterminate;
      let state = if rejected {
        LaneEntryState::Failed(DispatchFailure::new(DispatchStage::AuthorityCommit, run_lane_indeterminate_code()))
      } else {
        LaneEntryState::Preparing
      };
      lane.queue.push_back(LaneEntry { ticket, state });
      rejected
    } else {
      false
    };
    PreparationGuard {
      dispatch: self.clone(),
      run_id,
      ticket,
      rejected,
      armed: true,
    }
  }

  fn complete_preparation(&self, ticket: u64, run_id: RunId, mutation: Result<RunMutation, ErrorCode>) {
    if let Some(route) = &self.inner.route {
      let request = mutation.and_then(|mutation| {
        RunCommitRequest::new(route.authority_id, run_id, IdempotencyKey::new(), vec![mutation]).map_err(|_| encode_code())
      });
      let mut lanes = self.inner.lanes.lock().unwrap();
      let lane = lanes.get_mut(&run_id).expect("ticket reservation creates its run lane");
      let entry = lane.queue.iter_mut().find(|entry| entry.ticket == ticket).expect("reserved ticket remains in its run lane");
      entry.state = if lane.indeterminate {
        LaneEntryState::Failed(DispatchFailure::new(DispatchStage::AuthorityCommit, run_lane_indeterminate_code()))
      } else {
        match request {
          Ok(request) => LaneEntryState::Ready(request),
          Err(code) => LaneEntryState::Failed(DispatchFailure::new(DispatchStage::Encode, code)),
        }
      };
      drop(lanes);
      self.wake_lane(run_id);
      return;
    }

    match mutation {
      Ok(mutation) => {
        self.mark_projection_ready(ticket, projection_for_mutation(None, run_id, None, &mutation, &self.inner.projector_routes))
      }
      Err(code) => {
        self.mark_projection_skipped(ticket);
        self.terminalize(ticket, vec![DispatchFailure::new(DispatchStage::Encode, code)]);
      }
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
            let indeterminate = lane.indeterminate;
            let cursor = lane.cursor.take();
            if !indeterminate {
              lanes.remove(&run_id);
            }
            drop(lanes);
            drop(cursor);
            return;
          }
        }
      };

      match action {
        LaneWake::Terminal(ticket, failure) => {
          self.mark_projection_skipped(ticket);
          self.terminalize(ticket, vec![failure]);
        }
        LaneWake::Spawn => {
          let admission = Arc::new(SpawnAdmission::new());
          let task_admission = admission.clone();
          let dispatch = self.clone();
          let spawn_guard = LaneSpawnGuard::new(admission.clone(), self.clone(), run_id);
          let task = Box::pin(async move {
            let _spawn_guard = spawn_guard;
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
          let failure = match spawn {
            Ok(Ok(())) if admission.spawn_succeeded_needs_recovery() => {
              DispatchFailure::new(DispatchStage::AuthorityCommit, task_unwind_code())
            }
            Ok(Ok(())) => return,
            Ok(Err(error)) if admission.spawn_failed_needs_recovery() => DispatchFailure::new(DispatchStage::Spawn, error.code),
            Err(_) if admission.spawn_failed_needs_recovery() => DispatchFailure::new(DispatchStage::Spawn, spawn_panic_code()),
            Ok(Err(_)) | Err(_) => return,
          };
          self.settle_unpolled_lane_task(run_id, failure);
        }
      }
    }
  }

  fn spawner(&self) -> &dyn TaskSpawner {
    self.inner.spawner.as_deref().expect("authority-backed dispatch has a task spawner")
  }

  async fn drain_run(&self, run_id: RunId) {
    let cursor = self.inner.lanes.lock().unwrap().get_mut(&run_id).and_then(|lane| lane.cursor.take());
    let mut guard = LaneDrainGuard::new(self.clone(), run_id, cursor);
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
            lane.cursor = guard.take_cursor();
            guard.disarm();
            return;
          }
          Some(LaneEntryState::Ready(_)) | Some(LaneEntryState::Failed(_)) => {
            let entry = lane.queue.pop_front().expect("front was present");
            match entry.state {
              LaneEntryState::Ready(_) if lane.indeterminate => {
                LaneAction::Terminal(entry.ticket, DispatchFailure::new(DispatchStage::AuthorityCommit, run_lane_indeterminate_code()))
              }
              LaneEntryState::Ready(request) => LaneAction::Commit(entry.ticket, request),
              LaneEntryState::Failed(failure) => LaneAction::Terminal(entry.ticket, failure),
              LaneEntryState::Preparing => unreachable!("matched a terminal lane entry"),
            }
          }
          None => {
            lane.running = false;
            let indeterminate = lane.indeterminate;
            let cursor = guard.take_cursor();
            // TODO(auv-run-contract-v1-task-9): retain an idle cursor only when
            // an accepted detached artifact job owns an unresolved revision.
            if !indeterminate {
              lanes.remove(&run_id);
            }
            guard.disarm();
            drop(lanes);
            drop(cursor);
            return;
          }
        }
      };

      match action {
        LaneAction::Terminal(ticket, failure) => {
          self.mark_projection_skipped(ticket);
          self.terminalize(ticket, vec![failure]);
        }
        LaneAction::Commit(ticket, request) => {
          let establishing_cursor = !self.inner.projector_routes.is_empty() && guard.cursor().is_none();
          if establishing_cursor {
            guard.activate(ticket, DispatchStage::AuthorityRead);
            let route = self.inner.route.as_ref().expect("authority lane requires a route");
            match AuthorityCursor::establish(route, run_id).await {
              Ok(cursor) => guard.set_cursor(cursor),
              Err(failure) => {
                guard.complete(ticket);
                if failure.is_integrity() {
                  self.quarantine_run(run_id);
                }
                self.mark_projection_skipped(ticket);
                self.terminalize(
                  ticket,
                  vec![DispatchFailure::new(
                    DispatchStage::AuthorityRead,
                    failure.into_code(),
                  )],
                );
                continue;
              }
            }
          }

          if let Some(cursor) = guard.cursor_mut() {
            cursor.register(ticket, request.clone());
          }
          if establishing_cursor {
            guard.set_stage(ticket, DispatchStage::AuthorityCommit);
          } else {
            guard.activate(ticket, DispatchStage::AuthorityCommit);
          }
          let route = self.inner.route.as_ref().expect("authority lane requires a route");
          guard.quarantine_on_drop = true;
          let commit = route.store.commit(request.clone()).await;
          guard.quarantine_on_drop = matches!(&commit, Err(CommitError::CommitUnknown(_)));
          let resolved = match commit {
            Ok(commit) if commit_matches_request(&commit, &request) => Some(commit),
            Ok(_) => {
              if let Some(cursor) = guard.cursor_mut() {
                cursor.remove(request.idempotency_key());
              }
              guard.complete(ticket);
              self.quarantine_run(run_id);
              self.mark_projection_skipped(ticket);
              self.terminalize(
                ticket,
                vec![DispatchFailure::new(
                  DispatchStage::AuthorityCommit,
                  commit_response_mismatch_code(),
                )],
              );
              continue;
            }
            Err(CommitError::CommitUnknown(code)) => match route.store.lookup_commit(run_id, request.idempotency_key()).await {
              Ok(Some(commit)) if commit_matches_request(&commit, &request) => {
                guard.quarantine_on_drop = false;
                Some(commit)
              }
              Ok(Some(_)) | Ok(None) | Err(_) => {
                if let Some(cursor) = guard.cursor_mut() {
                  cursor.remove(request.idempotency_key());
                }
                guard.complete(ticket);
                guard.quarantine_on_drop = false;
                self.quarantine_run(run_id);
                self.mark_projection_skipped(ticket);
                self.terminalize(ticket, vec![DispatchFailure::new(DispatchStage::AuthorityCommit, code)]);
                continue;
              }
            },
            Err(error) => {
              if let Some(cursor) = guard.cursor_mut() {
                cursor.remove(request.idempotency_key());
              }
              guard.complete(ticket);
              self.mark_projection_skipped(ticket);
              self.terminalize(
                ticket,
                vec![DispatchFailure::new(
                  DispatchStage::AuthorityCommit,
                  commit_error_code(error),
                )],
              );
              continue;
            }
          };

          let Some(commit) = resolved else {
            unreachable!("successful and looked-up commits resolve to a value")
          };
          if self.inner.projector_routes.is_empty() {
            guard.complete(ticket);
            self.terminalize(ticket, Vec::new());
            continue;
          }

          guard.set_stage(ticket, DispatchStage::AuthorityRead);
          let projection = guard
            .cursor_mut()
            .expect("projected authority lane established its cursor")
            .observe_through(route, commit.revision(), ticket, &self.inner.projector_routes)
            .await;
          guard.complete(ticket);
          match projection {
            Ok(projected) => {
              for (projected_ticket, items) in projected {
                self.mark_projection_ready(projected_ticket, items);
              }
            }
            Err(failure) => {
              for (projected_ticket, items) in failure.projected {
                if projected_ticket != ticket {
                  self.mark_projection_ready(projected_ticket, items);
                }
              }
              // A failed read may leave the subscription behind its recorded
              // revision. Re-establish from the next authority snapshot.
              guard.take_cursor();
              if failure.failure.is_integrity() {
                self.quarantine_run(run_id);
              }
              self.mark_projection_skipped(ticket);
              self.terminalize(
                ticket,
                vec![DispatchFailure::new(
                  DispatchStage::AuthorityRead,
                  failure.failure.into_code(),
                )],
              );
            }
          }
        }
      }
    }
  }

  fn terminalize(&self, ticket: u64, failures: Vec<DispatchFailure>) {
    let (accepted, waker) = {
      let mut progress = self.inner.progress.lock().unwrap();
      let accepted = progress.terminalize(ticket, failures.clone());
      (accepted, progress.take_ready_front_waker())
    };
    if accepted {
      self.report_failures(&failures);
    }
    if let Some(waker) = waker {
      waker.wake();
    }
  }

  fn fail_preparation(&self, run_id: RunId, ticket: u64) {
    if self.inner.route.is_none() {
      self.mark_projection_skipped(ticket);
      self.terminalize(ticket, vec![DispatchFailure::new(DispatchStage::Encode, encode_code())]);
      return;
    }
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

  fn recover_lane_task(&self, run_id: RunId, active_ticket: Option<(u64, DispatchStage)>, quarantine: bool) {
    if let Some(lane) = self.inner.lanes.lock().unwrap().get_mut(&run_id) {
      lane.running = false;
      if quarantine {
        lane.indeterminate = true;
      }
    }
    if let Some((ticket, stage)) = active_ticket {
      self.mark_projection_skipped(ticket);
      self.terminalize(ticket, vec![DispatchFailure::new(stage, task_unwind_code())]);
    }
    self.wake_lane(run_id);
  }

  fn finish_unpolled_lane_task(&self, run_id: RunId) {
    self.settle_unpolled_lane_task(run_id, DispatchFailure::new(DispatchStage::AuthorityCommit, task_unwind_code()));
    self.wake_lane(run_id);
  }

  fn settle_unpolled_lane_task(&self, run_id: RunId, failure: DispatchFailure) {
    let ticket = {
      let mut lanes = self.inner.lanes.lock().unwrap();
      let lane = lanes.get_mut(&run_id).expect("an accepted unpolled task retains its run lane");
      lane.running = false;
      let entry = lane.queue.pop_front().expect("an accepted unpolled task retains its ready ticket");
      debug_assert!(matches!(entry.state, LaneEntryState::Ready(_)));
      entry.ticket
    };
    self.mark_projection_skipped(ticket);
    self.terminalize(ticket, vec![failure]);
  }

  fn quarantine_run(&self, run_id: RunId) {
    // NOTICE: quarantine is scoped to this dispatch's run lane. Replacing the
    // dispatch does not prove ordering safety; callers must choose a new RunId.
    if let Some(lane) = self.inner.lanes.lock().unwrap().get_mut(&run_id) {
      lane.indeterminate = true;
    }
  }

  fn mark_projection_ready(&self, ticket: u64, items: ProjectionBatch) {
    if self.inner.projector_routes.is_empty() {
      self.terminalize(ticket, Vec::new());
      return;
    }
    self.inner.projection.lock().unwrap().ready(ticket, items);
    self.wake_projection();
  }

  fn mark_projection_skipped(&self, ticket: u64) {
    if self.inner.projector_routes.is_empty() {
      return;
    }
    self.inner.projection.lock().unwrap().skip(ticket);
    self.wake_projection();
  }

  fn wake_projection(&self) {
    {
      let mut projection = self.inner.projection.lock().unwrap();
      if projection.draining {
        return;
      }
      projection.draining = true;
    }

    loop {
      let action = {
        let mut projection = self.inner.projection.lock().unwrap();
        if projection.running {
          projection.draining = false;
          return;
        }
        let Some(action) = projection.next_action() else {
          projection.draining = false;
          return;
        };
        projection.running = true;
        action
      };

      self.spawn_projection_action(action);
      let mut projection = self.inner.projection.lock().unwrap();
      if projection.running {
        projection.draining = false;
        return;
      }
    }
  }

  fn spawn_projection_action(&self, action: ProjectionAction) {
    let recovery = action.recovery();
    let admission = Arc::new(SpawnAdmission::new());
    let task_admission = admission.clone();
    let dispatch = self.clone();
    let spawn_guard = ProjectionSpawnGuard::new(admission.clone(), self.clone(), recovery);
    let task = Box::pin(async move {
      let _spawn_guard = spawn_guard;
      let admitted = async move {
        if task_admission.start() {
          dispatch.run_projection_action(action).await;
        }
      };
      let _ = AssertUnwindSafe(admitted).catch_unwind().await;
    });
    let spawn = catch_unwind(AssertUnwindSafe(|| self.spawner().spawn(task)));
    let failure = match spawn {
      Ok(Ok(())) if admission.spawn_succeeded_needs_recovery() => DispatchFailure::new(recovery.stage(), task_unwind_code()),
      Ok(Ok(())) => return,
      Ok(Err(error)) if admission.spawn_failed_needs_recovery() => DispatchFailure::new(DispatchStage::Spawn, error.code),
      Err(_) if admission.spawn_failed_needs_recovery() => DispatchFailure::new(DispatchStage::Spawn, spawn_panic_code()),
      Ok(Err(_)) | Err(_) => return,
    };
    self.settle_projection_action(recovery, vec![failure]);
  }

  async fn run_projection_action(&self, action: ProjectionAction) {
    let recovery = action.recovery();
    let mut guard = ProjectionTaskGuard::new(self.clone(), recovery);
    let mut failures = Vec::new();
    match action {
      ProjectionAction::Project { items, .. } => {
        for item in items {
          debug_assert_eq!(item.route_items.len(), self.inner.projector_routes.len());
          for (route, route_item) in self.inner.projector_routes.iter().zip(item.route_items) {
            if let Err(code) = project_one(route, route_item).await {
              failures.push(DispatchFailure::new(DispatchStage::Project, code));
            }
          }
        }
      }
      ProjectionAction::Flush { .. } => {
        for route in &self.inner.projector_routes {
          if let Err(code) = flush_one(route).await {
            failures.push(DispatchFailure::new(DispatchStage::ProjectorFlush, code));
          }
        }
      }
    }
    guard.complete(failures);
  }

  fn finish_projection_action(&self, recovery: ProjectionRecovery, failures: Vec<DispatchFailure>) {
    self.settle_projection_action(recovery, failures);
    self.wake_projection();
  }

  fn settle_projection_action(&self, recovery: ProjectionRecovery, failures: Vec<DispatchFailure>) {
    {
      let mut projection = self.inner.projection.lock().unwrap();
      debug_assert!(projection.running, "one active projection action owns the lane");
      projection.running = false;
    }
    match recovery {
      ProjectionRecovery::Ticket(ticket) => self.terminalize(ticket, failures),
      ProjectionRecovery::Flush(ordering_id) => self.complete_projector_flush(ordering_id, failures),
    }
  }

  fn complete_projector_flush(&self, ordering_id: u64, failures: Vec<DispatchFailure>) {
    let waker = {
      let mut progress = self.inner.progress.lock().unwrap();
      progress.complete_flush(ordering_id, failures.clone())
    };
    self.report_failures(&failures);
    if let Some(waker) = waker {
      waker.wake();
    }
  }

  fn report_failures(&self, failures: &[DispatchFailure]) {
    for failure in failures {
      let _ = catch_unwind(AssertUnwindSafe(|| self.inner.reporter.report(failure)));
    }
  }
}

async fn project_one(route: &ProjectorRoute, item: TelemetryItem) -> Result<(), ErrorCode> {
  let future = catch_unwind(AssertUnwindSafe(|| route.projector.project(item))).map_err(|_| projector_panic_code())?;
  match AssertUnwindSafe(future).catch_unwind().await {
    Ok(Ok(())) => Ok(()),
    Ok(Err(error)) => Err(error.code().clone()),
    Err(_) => Err(projector_panic_code()),
  }
}

async fn flush_one(route: &ProjectorRoute) -> Result<(), ErrorCode> {
  let future = catch_unwind(AssertUnwindSafe(|| route.projector.flush())).map_err(|_| projector_flush_panic_code())?;
  match AssertUnwindSafe(future).catch_unwind().await {
    Ok(Ok(())) => Ok(()),
    Ok(Err(error)) => Err(error.code().clone()),
    Err(_) => Err(projector_flush_panic_code()),
  }
}

struct SpawnAdmission {
  state: Mutex<SpawnAdmissionState>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpawnAdmissionState {
  Admitting,
  Accepted,
  Started,
  DroppedBeforeReturn,
  Recovered,
}

impl SpawnAdmission {
  fn new() -> Self {
    Self {
      state: Mutex::new(SpawnAdmissionState::Admitting),
    }
  }

  fn start(&self) -> bool {
    let mut state = self.state.lock().unwrap();
    match *state {
      SpawnAdmissionState::Admitting | SpawnAdmissionState::Accepted => {
        *state = SpawnAdmissionState::Started;
        true
      }
      SpawnAdmissionState::Started | SpawnAdmissionState::DroppedBeforeReturn | SpawnAdmissionState::Recovered => false,
    }
  }

  fn task_dropped_before_start(&self) -> bool {
    let mut state = self.state.lock().unwrap();
    match *state {
      SpawnAdmissionState::Admitting => {
        *state = SpawnAdmissionState::DroppedBeforeReturn;
        false
      }
      SpawnAdmissionState::Accepted => {
        *state = SpawnAdmissionState::Recovered;
        true
      }
      SpawnAdmissionState::Started | SpawnAdmissionState::DroppedBeforeReturn | SpawnAdmissionState::Recovered => false,
    }
  }

  fn spawn_succeeded_needs_recovery(&self) -> bool {
    let mut state = self.state.lock().unwrap();
    match *state {
      SpawnAdmissionState::Admitting => {
        *state = SpawnAdmissionState::Accepted;
        false
      }
      SpawnAdmissionState::DroppedBeforeReturn => {
        *state = SpawnAdmissionState::Recovered;
        true
      }
      SpawnAdmissionState::Accepted | SpawnAdmissionState::Started | SpawnAdmissionState::Recovered => false,
    }
  }

  fn spawn_failed_needs_recovery(&self) -> bool {
    let mut state = self.state.lock().unwrap();
    match *state {
      SpawnAdmissionState::Admitting | SpawnAdmissionState::DroppedBeforeReturn => {
        *state = SpawnAdmissionState::Recovered;
        true
      }
      SpawnAdmissionState::Accepted | SpawnAdmissionState::Started | SpawnAdmissionState::Recovered => false,
    }
  }
}

struct LaneSpawnGuard {
  admission: Arc<SpawnAdmission>,
  dispatch: Dispatch,
  run_id: RunId,
}

impl LaneSpawnGuard {
  fn new(admission: Arc<SpawnAdmission>, dispatch: Dispatch, run_id: RunId) -> Self {
    Self {
      admission,
      dispatch,
      run_id,
    }
  }
}

impl Drop for LaneSpawnGuard {
  fn drop(&mut self) {
    if self.admission.task_dropped_before_start() {
      self.dispatch.finish_unpolled_lane_task(self.run_id);
    }
  }
}

type ProjectionBatch = Vec<RoutedTelemetryItem>;

struct RoutedTelemetryItem {
  route_items: Vec<TelemetryItem>,
}

#[derive(Default)]
struct ProjectionState {
  draining: bool,
  running: bool,
  consumed_through: u64,
  entries: BTreeMap<u64, ProjectionEntry>,
  flushes: VecDeque<ProjectionFlush>,
}

impl ProjectionState {
  fn ready(&mut self, ticket: u64, items: ProjectionBatch) {
    let Some(entry) = self.entries.get_mut(&ticket) else {
      debug_assert!(false, "a prepared projection ticket remains queued");
      return;
    };
    debug_assert!(matches!(entry, ProjectionEntry::Preparing));
    *entry = ProjectionEntry::Ready(items);
  }

  fn skip(&mut self, ticket: u64) {
    let Some(entry) = self.entries.get_mut(&ticket) else {
      return;
    };
    if matches!(entry, ProjectionEntry::Preparing) {
      *entry = ProjectionEntry::Skip;
    }
  }

  fn next_action(&mut self) -> Option<ProjectionAction> {
    loop {
      if self.flushes.front().is_some_and(|flush| flush.barrier <= self.consumed_through) {
        let flush = self.flushes.pop_front().expect("front flush was present");
        return Some(ProjectionAction::Flush {
          ordering_id: flush.ordering_id,
        });
      }

      let next = self.consumed_through.checked_add(1)?;
      match self.entries.get(&next) {
        Some(ProjectionEntry::Preparing) | None => return None,
        Some(ProjectionEntry::Skip) => {
          self.entries.remove(&next);
          self.consumed_through = next;
        }
        Some(ProjectionEntry::Ready(_)) => {
          let Some(ProjectionEntry::Ready(items)) = self.entries.remove(&next) else {
            unreachable!("projection entry was matched as ready")
          };
          self.consumed_through = next;
          return Some(ProjectionAction::Project {
            ticket: next,
            items,
          });
        }
      }
    }
  }
}

enum ProjectionEntry {
  Preparing,
  Ready(ProjectionBatch),
  Skip,
}

struct ProjectionFlush {
  ordering_id: u64,
  barrier: u64,
}

enum ProjectionAction {
  Project { ticket: u64, items: ProjectionBatch },
  Flush { ordering_id: u64 },
}

impl ProjectionAction {
  fn recovery(&self) -> ProjectionRecovery {
    match self {
      Self::Project { ticket, .. } => ProjectionRecovery::Ticket(*ticket),
      Self::Flush { ordering_id } => ProjectionRecovery::Flush(*ordering_id),
    }
  }
}

#[derive(Clone, Copy)]
enum ProjectionRecovery {
  Ticket(u64),
  Flush(u64),
}

struct ProjectionSpawnGuard {
  admission: Arc<SpawnAdmission>,
  dispatch: Dispatch,
  recovery: ProjectionRecovery,
}

impl ProjectionSpawnGuard {
  fn new(admission: Arc<SpawnAdmission>, dispatch: Dispatch, recovery: ProjectionRecovery) -> Self {
    Self {
      admission,
      dispatch,
      recovery,
    }
  }
}

impl Drop for ProjectionSpawnGuard {
  fn drop(&mut self) {
    if self.admission.task_dropped_before_start() {
      self.dispatch.finish_projection_action(
        self.recovery,
        vec![DispatchFailure::new(
          self.recovery.stage(),
          task_unwind_code(),
        )],
      );
    }
  }
}

impl ProjectionRecovery {
  fn stage(self) -> DispatchStage {
    match self {
      Self::Ticket(_) => DispatchStage::Project,
      Self::Flush(_) => DispatchStage::ProjectorFlush,
    }
  }
}

struct ProjectionTaskGuard {
  dispatch: Dispatch,
  recovery: ProjectionRecovery,
  armed: bool,
}

impl ProjectionTaskGuard {
  fn new(dispatch: Dispatch, recovery: ProjectionRecovery) -> Self {
    Self {
      dispatch,
      recovery,
      armed: true,
    }
  }

  fn complete(&mut self, failures: Vec<DispatchFailure>) {
    self.armed = false;
    self.dispatch.finish_projection_action(self.recovery, failures);
  }
}

impl Drop for ProjectionTaskGuard {
  fn drop(&mut self) {
    if !self.armed {
      return;
    }
    self.dispatch.finish_projection_action(
      self.recovery,
      vec![DispatchFailure::new(
        self.recovery.stage(),
        task_unwind_code(),
      )],
    );
  }
}

struct AuthorityCursor {
  authority_id: AuthorityId,
  run_id: RunId,
  through_revision: RunRevision,
  subscription: RunSubscription,
  owned: HashMap<IdempotencyKey, OwnedSubmission>,
}

enum CursorFailure {
  Read(ErrorCode),
  Integrity(ErrorCode),
}

impl CursorFailure {
  fn read(code: ErrorCode) -> Self {
    Self::Read(code)
  }

  fn integrity(code: ErrorCode) -> Self {
    Self::Integrity(code)
  }

  fn is_integrity(&self) -> bool {
    matches!(self, Self::Integrity(_))
  }

  fn into_code(self) -> ErrorCode {
    match self {
      Self::Read(code) | Self::Integrity(code) => code,
    }
  }
}

struct CursorObservationFailure {
  failure: CursorFailure,
  projected: Vec<(u64, ProjectionBatch)>,
}

impl CursorObservationFailure {
  fn new(failure: CursorFailure, projected: Vec<(u64, ProjectionBatch)>) -> Self {
    Self { failure, projected }
  }
}

struct OwnedSubmission {
  ticket: u64,
  request: RunCommitRequest,
}

impl AuthorityCursor {
  async fn establish(route: &AuthorityRoute, run_id: RunId) -> Result<Self, CursorFailure> {
    let snapshot = route.store.load_snapshot(run_id).await.map_err(cursor_failure_from_read)?;
    let through_revision = match snapshot {
      Some(snapshot) => {
        if snapshot.authority_id() != route.authority_id || snapshot.run_id() != run_id {
          return Err(CursorFailure::integrity(committed_cursor_mismatch_code()));
        }
        snapshot.through_revision()
      }
      None => zero_revision(),
    };
    let subscription = route.store.subscribe(run_id, through_revision).await.map_err(cursor_failure_from_read)?;
    Ok(Self {
      authority_id: route.authority_id,
      run_id,
      through_revision,
      subscription,
      owned: HashMap::new(),
    })
  }

  fn register(&mut self, ticket: u64, request: RunCommitRequest) {
    let previous = self.owned.insert(request.idempotency_key(), OwnedSubmission { ticket, request });
    debug_assert!(previous.is_none(), "dispatch idempotency keys are unique within a run cursor");
  }

  fn remove(&mut self, key: IdempotencyKey) {
    self.owned.remove(&key);
  }

  async fn observe_through(
    &mut self,
    route: &AuthorityRoute,
    target: RunRevision,
    current_ticket: u64,
    projector_routes: &[ProjectorRoute],
  ) -> Result<Vec<(u64, ProjectionBatch)>, CursorObservationFailure> {
    let mut projected = Vec::new();
    let mut resume = false;
    while self.through_revision < target {
      match poll_subscription_once(&mut self.subscription).await {
        Poll::Ready(Some(Ok(commit))) => {
          if let Err(failure) = self.observe_commit(commit, projector_routes, &mut projected) {
            return Err(CursorObservationFailure::new(failure, projected));
          }
        }
        Poll::Ready(Some(Err(SubscriptionError::Gap { .. }))) | Poll::Pending | Poll::Ready(None) => {
          if let Err(failure) = self.recover_through(route, target, projector_routes, &mut projected).await {
            return Err(CursorObservationFailure::new(failure, projected));
          }
          resume = true;
        }
        Poll::Ready(Some(Err(SubscriptionError::Store(error)))) => {
          return Err(CursorObservationFailure::new(cursor_failure_from_read(error), projected));
        }
      }
    }
    if resume {
      self.subscription = match route.store.subscribe(self.run_id, self.through_revision).await {
        Ok(subscription) => subscription,
        Err(error) => return Err(CursorObservationFailure::new(cursor_failure_from_read(error), projected)),
      };
    }
    if !projected.iter().any(|(ticket, _)| *ticket == current_ticket) {
      return Err(CursorObservationFailure::new(CursorFailure::integrity(committed_cursor_mismatch_code()), projected));
    }
    Ok(projected)
  }

  async fn recover_through(
    &mut self,
    route: &AuthorityRoute,
    target: RunRevision,
    projector_routes: &[ProjectorRoute],
    projected: &mut Vec<(u64, ProjectionBatch)>,
  ) -> Result<(), CursorFailure> {
    let limit = PageLimit::new(1024).expect("the cursor recovery page limit is valid");
    while self.through_revision < target {
      let page = match route.store.commits_after(self.run_id, self.through_revision, limit).await {
        Ok(page) => page,
        Err(
          error @ ReadError::HistoryGap {
            earliest_available, ..
          },
        ) if earliest_available <= target => {
          let Some(predecessor) = earliest_available.get().checked_sub(1) else {
            return Err(CursorFailure::read(read_error_code(error)));
          };
          self.through_revision = RunRevision::new(predecessor).expect("a retained revision predecessor is representable");
          continue;
        }
        Err(error) => return Err(cursor_failure_from_read(error)),
      };
      if page.commits().is_empty() {
        return Err(CursorFailure::integrity(committed_cursor_mismatch_code()));
      }
      for commit in page.commits().iter().cloned() {
        self.observe_commit(commit, projector_routes, projected)?;
      }
      if self.through_revision < target && !page.has_more() {
        return Err(CursorFailure::integrity(committed_cursor_mismatch_code()));
      }
    }
    Ok(())
  }

  fn observe_commit(
    &mut self,
    commit: RunCommit,
    projector_routes: &[ProjectorRoute],
    projected: &mut Vec<(u64, ProjectionBatch)>,
  ) -> Result<(), CursorFailure> {
    let expected_revision =
      self.through_revision.get().checked_add(1).ok_or_else(|| CursorFailure::integrity(committed_cursor_mismatch_code()))?;
    if commit.authority_id() != self.authority_id || commit.run_id() != self.run_id || commit.revision().get() != expected_revision {
      return Err(CursorFailure::integrity(committed_cursor_mismatch_code()));
    }
    self.through_revision = commit.revision();
    if let Some(owned) = self.owned.remove(&commit.idempotency_key()) {
      if !commit_matches_request(&commit, &owned.request) {
        return Err(CursorFailure::integrity(commit_response_mismatch_code()));
      }
      projected.push((owned.ticket, projection_for_commit(&commit, projector_routes)));
    }
    Ok(())
  }
}

async fn poll_subscription_once(subscription: &mut RunSubscription) -> Poll<Option<Result<RunCommit, SubscriptionError>>> {
  futures_util::future::poll_fn(|context| Poll::Ready(subscription.as_mut().poll_next(context))).await
}

struct LaneDrainGuard {
  dispatch: Dispatch,
  run_id: RunId,
  active_ticket: Option<(u64, DispatchStage)>,
  cursor: Option<AuthorityCursor>,
  quarantine_on_drop: bool,
  armed: bool,
}

impl LaneDrainGuard {
  fn new(dispatch: Dispatch, run_id: RunId, cursor: Option<AuthorityCursor>) -> Self {
    Self {
      dispatch,
      run_id,
      active_ticket: None,
      cursor,
      quarantine_on_drop: false,
      armed: true,
    }
  }

  fn activate(&mut self, ticket: u64, stage: DispatchStage) {
    debug_assert!(self.active_ticket.replace((ticket, stage)).is_none(), "lane task owns at most one active ticket");
  }

  fn set_stage(&mut self, ticket: u64, stage: DispatchStage) {
    debug_assert_eq!(self.active_ticket.map(|active| active.0), Some(ticket));
    self.active_ticket = Some((ticket, stage));
  }

  fn complete(&mut self, ticket: u64) {
    debug_assert_eq!(self.active_ticket.map(|active| active.0), Some(ticket), "lane task completes its active ticket");
    self.active_ticket = None;
  }

  fn cursor(&self) -> Option<&AuthorityCursor> {
    self.cursor.as_ref()
  }

  fn cursor_mut(&mut self) -> Option<&mut AuthorityCursor> {
    self.cursor.as_mut()
  }

  fn set_cursor(&mut self, cursor: AuthorityCursor) {
    debug_assert!(self.cursor.replace(cursor).is_none());
  }

  fn take_cursor(&mut self) -> Option<AuthorityCursor> {
    self.cursor.take()
  }

  fn disarm(&mut self) {
    self.armed = false;
  }
}

impl Drop for LaneDrainGuard {
  fn drop(&mut self) {
    if self.armed {
      self.dispatch.recover_lane_task(self.run_id, self.active_ticket.take(), self.quarantine_on_drop);
    }
  }
}

struct PreparationGuard {
  dispatch: Dispatch,
  run_id: RunId,
  ticket: u64,
  rejected: bool,
  armed: bool,
}

impl PreparationGuard {
  fn is_rejected(&self) -> bool {
    self.rejected
  }

  fn complete(mut self, mutation: Result<RunMutation, ErrorCode>) {
    self.dispatch.complete_preparation(self.ticket, self.run_id, mutation);
    self.armed = false;
  }

  fn finish_rejected(mut self) {
    debug_assert!(self.rejected);
    self.armed = false;
    self.dispatch.wake_lane(self.run_id);
  }
}

impl Drop for PreparationGuard {
  fn drop(&mut self) {
    if self.armed {
      if self.rejected {
        self.dispatch.wake_lane(self.run_id);
      } else {
        self.dispatch.fail_preparation(self.run_id, self.ticket);
      }
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
  projector_routes: Vec<ProjectorRoute>,
  reporter: Arc<dyn DispatchErrorReporter>,
  lanes: Mutex<HashMap<RunId, RunLane>>,
  projection: Mutex<ProjectionState>,
  progress: Mutex<Progress>,
}

struct AuthorityRoute {
  authority_id: AuthorityId,
  store: Arc<dyn RunStore>,
}

struct ProjectorRoute {
  projector: Arc<dyn TelemetryProjector>,
  policy: TelemetryRoutePolicy,
}

struct DiscardReporter;

impl DispatchErrorReporter for DiscardReporter {
  fn report(&self, _failure: &DispatchFailure) {}
}

#[derive(Default)]
struct RunLane {
  running: bool,
  indeterminate: bool,
  queue: VecDeque<LaneEntry>,
  cursor: Option<AuthorityCursor>,
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
  out_of_order_failures: BTreeMap<u64, Vec<DispatchFailure>>,
  failures: BTreeMap<u64, Vec<DispatchFailure>>,
  reported_through: u64,
  flushes: VecDeque<FlushRegistration>,
  carried_flush_failures: VecDeque<CarriedFlushFailure>,
}

impl Progress {
  fn register_flush(&mut self, operation_complete: bool) -> (u64, u64) {
    self.next_flush_id = self.next_flush_id.checked_add(1).expect("flush ordering ID space exhausted");
    let ordering_id = self.next_flush_id;
    let barrier = self.next_ticket;
    self.flushes.push_back(FlushRegistration {
      ordering_id,
      barrier,
      operation_complete,
      canceled: false,
      failures: Vec::new(),
      waker: None,
    });
    (ordering_id, barrier)
  }

  fn terminalize(&mut self, ticket: u64, failures: Vec<DispatchFailure>) -> bool {
    if ticket <= self.terminal_prefix
      || self.out_of_order_failures.contains_key(&ticket)
      || self.success_ranges.range(..=ticket).next_back().is_some_and(|(_, end)| *end >= ticket)
    {
      debug_assert!(false, "a dispatch ticket terminalized more than once");
      return false;
    }

    if failures.is_empty() {
      self.insert_success(ticket);
    } else {
      self.out_of_order_failures.insert(ticket, failures);
    }

    while let Some(next) = self.terminal_prefix.checked_add(1) {
      if let Some(failures) = self.out_of_order_failures.remove(&next) {
        self.terminal_prefix = next;
        self.failures.insert(next, failures);
      } else if let Some(end) = self.success_ranges.remove(&next) {
        self.terminal_prefix = end;
      } else {
        break;
      }
    }
    true
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
    if position != 0 || !self.flushes[0].operation_complete || self.flushes[0].barrier > self.terminal_prefix {
      self.flushes[position].waker = Some(context.waker().clone());
      return (Poll::Pending, None);
    }

    let flush = self.flushes.pop_front().expect("front was present");
    debug_assert!(!flush.canceled, "a canceled completed front flush is drained eagerly");
    let mut failures = Vec::new();
    let mut cursor = self.reported_through;
    while self.carried_flush_failures.front().is_some_and(|carried| carried.barrier <= flush.barrier) {
      let carried = self.carried_flush_failures.pop_front().expect("front carried failure was present");
      self.extend_ticket_failures(cursor, carried.barrier, &mut failures);
      cursor = cursor.max(carried.barrier);
      failures.extend(carried.failures);
    }
    self.extend_ticket_failures(cursor, flush.barrier, &mut failures);
    failures.extend(flush.failures);
    self.failures.retain(|ticket, _| *ticket > flush.barrier);
    self.reported_through = flush.barrier;
    let waker = self.drain_canceled_front().or_else(|| self.take_front_waker());
    (Poll::Ready(NonEmptyVec::new(failures).map(|failures| Err(FlushError { failures })).unwrap_or(Ok(()))), waker)
  }

  fn cancel_flush(&mut self, ordering_id: u64) -> Option<Waker> {
    let flush = self.flushes.iter_mut().find(|flush| flush.ordering_id == ordering_id)?;
    flush.canceled = true;
    flush.waker = None;
    self.drain_canceled_front().or_else(|| self.take_ready_front_waker())
  }

  fn complete_flush(&mut self, ordering_id: u64, failures: Vec<DispatchFailure>) -> Option<Waker> {
    let flush = self.flushes.iter_mut().find(|flush| flush.ordering_id == ordering_id).expect("projector flush remains registered");
    debug_assert!(!flush.operation_complete);
    flush.operation_complete = true;
    flush.failures = failures;
    self.drain_canceled_front().or_else(|| self.take_ready_front_waker())
  }

  fn drain_canceled_front(&mut self) -> Option<Waker> {
    let mut drained = false;
    while self.flushes.front().is_some_and(|flush| flush.canceled && flush.operation_complete) {
      let flush = self.flushes.pop_front().expect("front canceled flush was present");
      if !flush.failures.is_empty() {
        self.carried_flush_failures.push_back(CarriedFlushFailure {
          barrier: flush.barrier,
          failures: flush.failures,
        });
      }
      drained = true;
    }
    drained.then(|| self.take_front_waker()).flatten()
  }

  fn extend_ticket_failures(&self, after: u64, through: u64, output: &mut Vec<DispatchFailure>) {
    if through <= after {
      return;
    }
    output.extend(self.failures.range((after + 1)..=through).flat_map(|(_, failures)| failures.iter().cloned()));
  }

  fn take_ready_front_waker(&mut self) -> Option<Waker> {
    if self.flushes.front().is_some_and(|flush| !flush.canceled && flush.operation_complete && flush.barrier <= self.terminal_prefix) {
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
  operation_complete: bool,
  canceled: bool,
  failures: Vec<DispatchFailure>,
  waker: Option<Waker>,
}

struct CarriedFlushFailure {
  barrier: u64,
  failures: Vec<DispatchFailure>,
}

fn projection_for_mutation(
  authority_id: Option<AuthorityId>,
  run_id: RunId,
  revision: Option<RunRevision>,
  mutation: &RunMutation,
  routes: &[ProjectorRoute],
) -> ProjectionBatch {
  let item = match mutation {
    RunMutation::StartSpan(started) => routed_span_start(authority_id, run_id, revision, started, routes),
    RunMutation::EndSpan(ended) => routed_span_end(authority_id, run_id, revision, ended, routes),
    RunMutation::EmitEvent(event) => routed_event(authority_id, run_id, revision, event, routes),
  };
  vec![item]
}

fn projection_for_commit(commit: &RunCommit, routes: &[ProjectorRoute]) -> ProjectionBatch {
  commit
    .facts()
    .iter()
    .filter_map(|fact| match fact {
      RunFact::SpanStarted(started) => {
        Some(routed_span_start(Some(commit.authority_id()), commit.run_id(), Some(commit.revision()), started, routes))
      }
      RunFact::SpanEnded(ended) => {
        Some(routed_span_end(Some(commit.authority_id()), commit.run_id(), Some(commit.revision()), ended, routes))
      }
      RunFact::EventOccurred(event) => {
        Some(routed_event(Some(commit.authority_id()), commit.run_id(), Some(commit.revision()), event, routes))
      }
      // TODO(auv-run-contract-v1-task-9): emit artifact telemetry only after
      // detached artifact admission owns and registers publication keys.
      RunFact::ArtifactPublished(_) => None,
    })
    .collect()
}

fn routed_span_start(
  authority_id: Option<AuthorityId>,
  run_id: RunId,
  revision: Option<RunRevision>,
  started: &SpanStarted,
  routes: &[ProjectorRoute],
) -> RoutedTelemetryItem {
  RoutedTelemetryItem {
    route_items: routes
      .iter()
      .map(|route| TelemetryItem::SpanStart {
        authority_id,
        run_id,
        span_id: started.span_id(),
        parent_span_id: started.parent_span_id(),
        remote_span_id: started.remote_link().map(SpanLink::span_id),
        name: started.name().clone(),
        started_at: started.started_at(),
        start_revision: revision,
        attributes: route.policy.span_attributes(started.attributes()),
      })
      .collect(),
  }
}

fn routed_span_end(
  authority_id: Option<AuthorityId>,
  run_id: RunId,
  revision: Option<RunRevision>,
  ended: &SpanEnded,
  routes: &[ProjectorRoute],
) -> RoutedTelemetryItem {
  RoutedTelemetryItem {
    route_items: routes
      .iter()
      .map(|_| TelemetryItem::SpanEnd {
        authority_id,
        run_id,
        span_id: ended.span_id(),
        ended_at: ended.ended_at(),
        end_revision: revision,
      })
      .collect(),
  }
}

fn routed_event(
  authority_id: Option<AuthorityId>,
  run_id: RunId,
  revision: Option<RunRevision>,
  event: &EventOccurred,
  routes: &[ProjectorRoute],
) -> RoutedTelemetryItem {
  RoutedTelemetryItem {
    route_items: routes
      .iter()
      .map(|_| TelemetryItem::Event {
        authority_id,
        run_id,
        span_id: event.span_id(),
        event_id: event.event_id(),
        schema: event.schema().clone(),
        occurred_at: event.occurred_at(),
        revision,
      })
      .collect(),
  }
}

fn commit_matches_request(commit: &RunCommit, request: &RunCommitRequest) -> bool {
  commit.authority_id() == request.authority_id()
    && commit.run_id() == request.run_id()
    && commit.idempotency_key() == request.idempotency_key()
    && commit.facts().len() == request.mutations().len()
    && commit.facts().iter().zip(request.mutations()).all(|(fact, mutation)| match (fact, mutation) {
      (RunFact::SpanStarted(fact), RunMutation::StartSpan(mutation)) => fact == mutation,
      (RunFact::SpanEnded(fact), RunMutation::EndSpan(mutation)) => fact == mutation,
      (RunFact::EventOccurred(fact), RunMutation::EmitEvent(mutation)) => fact == mutation,
      (RunFact::ArtifactPublished(_), _) | (_, _) => false,
    })
}

fn zero_revision() -> RunRevision {
  RunRevision::new(0).expect("revision zero is the pre-history cursor")
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

fn projector_panic_code() -> ErrorCode {
  ErrorCode::parse("auv.dispatch.projector_panic").expect("static dispatch error code is valid")
}

fn projector_flush_panic_code() -> ErrorCode {
  ErrorCode::parse("auv.dispatch.projector_flush_panic").expect("static dispatch error code is valid")
}

fn run_lane_indeterminate_code() -> ErrorCode {
  ErrorCode::parse("auv.dispatch.run_lane_indeterminate").expect("static dispatch error code is valid")
}

fn commit_response_mismatch_code() -> ErrorCode {
  ErrorCode::parse("auv.dispatch.commit_response_mismatch").expect("static dispatch error code is valid")
}

fn committed_cursor_mismatch_code() -> ErrorCode {
  ErrorCode::parse("auv.dispatch.committed_cursor_mismatch").expect("static dispatch error code is valid")
}

fn read_error_code(error: ReadError) -> ErrorCode {
  match error {
    ReadError::InvalidReference(code) | ReadError::Unavailable(code) | ReadError::Integrity(code) => code,
    ReadError::NotFound => ErrorCode::parse("auv.dispatch.authority_not_found").expect("static dispatch error code is valid"),
    ReadError::Forbidden => ErrorCode::parse("auv.dispatch.authority_forbidden").expect("static dispatch error code is valid"),
    ReadError::HistoryGap { .. } => ErrorCode::parse("auv.dispatch.authority_history_gap").expect("static dispatch error code is valid"),
    ReadError::CursorAhead { .. } => ErrorCode::parse("auv.dispatch.authority_cursor_ahead").expect("static dispatch error code is valid"),
  }
}

fn cursor_failure_from_read(error: ReadError) -> CursorFailure {
  let integrity =
    matches!(&error, ReadError::NotFound | ReadError::InvalidReference(_) | ReadError::Integrity(_) | ReadError::CursorAhead { .. });
  let code = read_error_code(error);
  if integrity {
    CursorFailure::integrity(code)
  } else {
    CursorFailure::read(code)
  }
}

fn commit_error_code(error: CommitError) -> ErrorCode {
  match error {
    CommitError::Rejected(code) | CommitError::Unavailable(code) => code,
    CommitError::CommitUnknown(code) => code,
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
  fn spawn_admission_assigns_one_prestart_recovery_owner_for_each_order() {
    let admission = SpawnAdmission::new();
    assert!(!admission.task_dropped_before_start());
    assert!(admission.spawn_succeeded_needs_recovery());
    assert!(!admission.spawn_succeeded_needs_recovery());

    let admission = SpawnAdmission::new();
    assert!(!admission.spawn_succeeded_needs_recovery());
    assert!(admission.task_dropped_before_start());
    assert!(!admission.task_dropped_before_start());

    let admission = SpawnAdmission::new();
    assert!(!admission.task_dropped_before_start());
    assert!(admission.spawn_failed_needs_recovery());
    assert!(!admission.spawn_failed_needs_recovery());

    let admission = SpawnAdmission::new();
    assert!(admission.spawn_failed_needs_recovery());
    assert!(!admission.task_dropped_before_start());
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
      progress.terminalize(ticket, Vec::new());
    }
    progress.terminalize(
      5_001,
      vec![DispatchFailure::new(
        DispatchStage::Spawn,
        spawn_panic_code(),
      )],
    );
    for ticket in 5_002..=10_000 {
      progress.terminalize(ticket, Vec::new());
    }

    assert_eq!(progress.success_ranges.len(), 2);
    assert_eq!(progress.out_of_order_failures.len(), 1);
    progress.terminalize(1, Vec::new());
    assert_eq!(progress.terminal_prefix, 10_000);
    assert!(progress.success_ranges.is_empty());
    assert!(progress.out_of_order_failures.is_empty());
    assert_eq!(progress.failures.get(&5_001).unwrap()[0].stage(), DispatchStage::Spawn);
  }
}
