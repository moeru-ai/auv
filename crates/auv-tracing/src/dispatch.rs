use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::future::Future;
use std::num::NonZeroUsize;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::task::{Context as TaskContext, Poll, Waker};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::FutureExt;

use crate::{
  ArtifactEmission, ArtifactMetadata, ArtifactReceiptMessage, ArtifactReceiptSender, ArtifactWriteError, AuthorityId, CommitError,
  DetachedArtifact, DispatchErrorReporter, ErrorCode, EventId, EventOccurred, EventPayload, EventSchema, IdempotencyKey, JsonPayload,
  NonEmptyVec, PageLimit, ReadError, RunCommit, RunCommitRequest, RunFact, RunId, RunMutation, RunRevision, RunStore, RunSubscription,
  SpanEnded, SpanId, SpanLink, SpanName, SpanSpec, SpanStarted, StoreArtifactRequest, SubscriptionError, TelemetryItem, TelemetryProjector,
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

    let authority_ordered_projection = route.is_some();
    Ok(Dispatch {
      inner: Arc::new(DispatchInner {
        route,
        spawner,
        projector_routes: self.projector_routes,
        reporter: self.error_reporter.unwrap_or_else(|| Arc::new(DiscardReporter)),
        lanes: Mutex::new(HashMap::new()),
        projection: Mutex::new(ProjectionState::new(authority_ordered_projection)),
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

  pub(crate) fn submit_artifact(&self, run_id: RunId, span_id: Option<SpanId>, artifact: DetachedArtifact) -> ArtifactEmission {
    let route = self.inner.route.as_ref().expect("artifact admission requires an authority route");
    let request = StoreArtifactRequest::new(
      route.authority_id,
      run_id,
      artifact.idempotency_key,
      artifact.artifact_id,
      span_id,
      artifact.purpose,
      artifact.content_type,
      artifact.expected_byte_length,
      artifact.expected_sha256,
      artifact.attributes,
    );
    let (receipt, emission) = ArtifactEmission::pending(self.clone());
    let settlement = ArtifactSettlement::new(receipt);
    let ticket = self.allocate_ticket();
    {
      let mut lanes = self.inner.lanes.lock().unwrap();
      let lane = lanes.entry(run_id).or_default();
      lane.queue.push_back(LaneEntry {
        ticket,
        state: LaneEntryState::Artifact(Box::new(ArtifactAdmission {
          request,
          body: artifact.body,
          settlement,
        })),
      });
    }
    self.wake_lane(run_id);
    emission
  }

  pub(crate) fn report_unobserved_artifact_failure(&self, failure: &DispatchFailure) {
    self.report_failures(std::slice::from_ref(failure));
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
    authority_id: Option<AuthorityId>,
    run_id: RunId,
    parent_span_id: Option<SpanId>,
    remote_link: Option<SpanLink>,
    span_id: SpanId,
    started_at: Result<Timestamp, ErrorCode>,
    spec: S,
  ) -> bool {
    let preparation = self.reserve_ticket(authority_id, run_id);
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

  pub(crate) fn submit_span_end(
    &self,
    authority_id: Option<AuthorityId>,
    run_id: RunId,
    span_id: SpanId,
    ended_at: Result<Timestamp, ErrorCode>,
  ) {
    let preparation = self.reserve_ticket(authority_id, run_id);
    if preparation.is_rejected() {
      preparation.finish_rejected();
      return;
    }
    preparation.complete(ended_at.map(|ended_at| RunMutation::EndSpan(SpanEnded::new(span_id, ended_at))));
  }

  pub(crate) fn submit_event<E: EventPayload>(
    &self,
    authority_id: Option<AuthorityId>,
    run_id: RunId,
    span_id: Option<SpanId>,
    occurred_at: Result<Timestamp, ErrorCode>,
    event: E,
  ) {
    let preparation = self.reserve_ticket(authority_id, run_id);
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

  fn reserve_ticket(&self, authority_id: Option<AuthorityId>, run_id: RunId) -> PreparationGuard {
    let ticket = self.allocate_ticket();
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
      authority_id,
      run_id,
      ticket,
      rejected,
      armed: true,
    }
  }

  fn allocate_ticket(&self) -> u64 {
    let mut progress = self.inner.progress.lock().unwrap();
    progress.next_ticket = progress.next_ticket.checked_add(1).expect("dispatch ticket space exhausted");
    if !self.inner.projector_routes.is_empty() {
      self.inner.projection.lock().unwrap().prepare(progress.next_ticket);
    }
    progress.next_ticket
  }

  fn complete_preparation(&self, ticket: u64, authority_id: Option<AuthorityId>, run_id: RunId, mutation: Result<RunMutation, ErrorCode>) {
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
        self.mark_projection_ready(ticket, projection_for_mutation(authority_id, run_id, None, &mutation, &self.inner.projector_routes))
      }
      Err(code) => {
        self.mark_projection_skipped(ticket);
        self.terminalize(ticket, vec![DispatchFailure::new(DispatchStage::Encode, code)]);
      }
    }
  }

  fn wake_lane(&self, run_id: RunId) {
    {
      let mut lanes = self.inner.lanes.lock().unwrap();
      let Some(lane) = lanes.get_mut(&run_id) else {
        return;
      };
      // Reentrant wakes only publish lane state; one synchronous owner keeps
      // draining until async work takes over or the front is unprepared.
      if lane.draining {
        return;
      }
      lane.draining = true;
    }

    loop {
      let action = {
        let mut lanes = self.inner.lanes.lock().unwrap();
        let Some(lane) = lanes.get_mut(&run_id) else {
          return;
        };
        if lane.running {
          lane.draining = false;
          return;
        }
        match lane.queue.front().map(|entry| &entry.state) {
          Some(LaneEntryState::Preparing) => {
            lane.draining = false;
            return;
          }
          Some(LaneEntryState::Ready(_) | LaneEntryState::Artifact(_)) => {
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
            lane.draining = false;
            if !indeterminate && lane.owned.is_empty() && !lane.observation.running && lane.observation.targets.is_empty() {
              lanes.remove(&run_id);
            }
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
          match spawn {
            Ok(Ok(())) if admission.spawn_succeeded_needs_recovery() => {
              self.settle_unpolled_lane_task(run_id, DispatchFailure::new(DispatchStage::AuthorityCommit, task_unwind_code()));
            }
            Ok(Err(error)) if admission.spawn_failed_needs_recovery() => {
              self.settle_unpolled_lane_task(run_id, DispatchFailure::new(DispatchStage::Spawn, error.code));
            }
            Err(_) if admission.spawn_failed_needs_recovery() => {
              self.settle_unpolled_lane_task(run_id, DispatchFailure::new(DispatchStage::Spawn, spawn_panic_code()));
            }
            Ok(Ok(())) | Ok(Err(_)) | Err(_) => {}
          }
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
          Some(LaneEntryState::Ready(_) | LaneEntryState::Artifact(_)) if lane.observation.establishing => {
            lane.running = false;
            guard.disarm();
            return;
          }
          Some(LaneEntryState::Preparing) => {
            lane.running = false;
            guard.disarm();
            return;
          }
          Some(LaneEntryState::Ready(_) | LaneEntryState::Artifact(_) | LaneEntryState::Failed(_)) => {
            let entry = lane.queue.pop_front().expect("front was present");
            match entry.state {
              LaneEntryState::Ready(_) if lane.indeterminate => {
                LaneAction::Terminal(entry.ticket, DispatchFailure::new(DispatchStage::AuthorityCommit, run_lane_indeterminate_code()))
              }
              LaneEntryState::Ready(request) => LaneAction::Commit(entry.ticket, request),
              LaneEntryState::Artifact(admission) => LaneAction::StartArtifact(entry.ticket, admission),
              LaneEntryState::Failed(failure) => LaneAction::Terminal(entry.ticket, failure),
              LaneEntryState::Preparing => unreachable!("matched a terminal lane entry"),
            }
          }
          None => {
            lane.running = false;
            let indeterminate = lane.indeterminate;
            let draining = lane.draining;
            if !indeterminate && !draining && lane.owned.is_empty() && !lane.observation.running && lane.observation.targets.is_empty() {
              lanes.remove(&run_id);
            }
            guard.disarm();
            return;
          }
        }
      };

      match action {
        LaneAction::Terminal(ticket, failure) => {
          self.mark_projection_skipped(ticket);
          self.terminalize(ticket, vec![failure]);
        }
        LaneAction::StartArtifact(ticket, admission) => {
          if self.inner.lanes.lock().unwrap().get(&run_id).is_some_and(|lane| lane.indeterminate) {
            self.finish_unstarted_artifact(
              ticket,
              admission,
              DispatchFailure::new(DispatchStage::ArtifactWrite, run_lane_indeterminate_code()),
            );
            continue;
          }

          let mut admission = Some(admission);
          if self.observation_needs_initialization(run_id) {
            guard.activate_artifact(ticket, DispatchStage::AuthorityRead, admission.take().expect("artifact admission is available"));
            match self.initialize_observation(run_id).await {
              Ok(()) => {}
              Err(failure) => {
                let admission = guard.take_artifact(ticket);
                if failure.is_integrity() {
                  self.quarantine_run(run_id);
                }
                self.finish_unstarted_artifact(ticket, admission, DispatchFailure::new(DispatchStage::AuthorityRead, failure.into_code()));
                continue;
              }
            }
          }
          let admission = if guard.has_active(ticket) {
            guard.take_artifact(ticket)
          } else {
            admission.take().expect("artifact admission is available")
          };

          self.register_owned(
            run_id,
            ticket,
            OwnedRequest::Artifact {
              request: admission.request.clone(),
              settlement: admission.settlement.clone(),
            },
          );
          self.spawn_artifact_task(run_id, ticket, admission);
        }
        LaneAction::Commit(ticket, request) => {
          if self.observation_needs_initialization(run_id) {
            guard.activate(ticket, DispatchStage::AuthorityRead);
            match self.initialize_observation(run_id).await {
              Ok(()) => {}
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

          self.register_owned(run_id, ticket, OwnedRequest::Ordinary(request.clone()));
          if guard.has_active(ticket) {
            guard.set_stage(ticket, DispatchStage::AuthorityCommit);
          } else {
            guard.activate(ticket, DispatchStage::AuthorityCommit);
          }
          guard.set_idempotency_key(ticket, request.idempotency_key());
          let route = self.inner.route.as_ref().expect("authority lane requires a route");
          guard.quarantine_on_drop = true;
          let commit = route.store.commit(request.clone()).await;
          guard.quarantine_on_drop = matches!(&commit, Err(CommitError::CommitUnknown(_)));
          let resolved = match commit {
            Ok(result) if commit_matches_request(result.commit(), &request) => Some(result.into_commit()),
            Ok(_) => {
              self.remove_owned(run_id, request.idempotency_key());
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
                if let Some(commit) = self.observed_commit(run_id, request.idempotency_key()) {
                  guard.quarantine_on_drop = false;
                  Some(commit)
                } else {
                  self.remove_owned(run_id, request.idempotency_key());
                  guard.complete(ticket);
                  guard.quarantine_on_drop = false;
                  self.quarantine_run(run_id);
                  self.mark_projection_skipped(ticket);
                  self.terminalize(ticket, vec![DispatchFailure::new(DispatchStage::AuthorityCommit, code)]);
                  continue;
                }
              }
            },
            Err(error) => {
              self.remove_owned(run_id, request.idempotency_key());
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
            self.remove_owned(run_id, request.idempotency_key());
            guard.complete(ticket);
            self.terminalize(ticket, Vec::new());
            continue;
          }

          self.complete_owned_write(run_id, request.idempotency_key(), commit.revision());
          guard.complete(ticket);
        }
      }
    }
  }

  fn observation_needs_initialization(&self, run_id: RunId) -> bool {
    if self.inner.projector_routes.is_empty() {
      return false;
    }
    let lanes = self.inner.lanes.lock().unwrap();
    let lane = lanes.get(&run_id).expect("an authority write retains its run lane");
    !lane.observation.initialized && !lane.observation.running
  }

  async fn initialize_observation(&self, run_id: RunId) -> Result<(), CursorFailure> {
    let route = self.inner.route.as_ref().expect("authority observation requires a route");
    let resume_after = self.inner.lanes.lock().unwrap().get(&run_id).and_then(|lane| lane.observation.resume_after);
    let cursor = match resume_after {
      Some(after) => AuthorityCursor::resume(route, run_id, after).await?,
      None => AuthorityCursor::establish(route, run_id).await?,
    };
    let mut lanes = self.inner.lanes.lock().unwrap();
    let lane = lanes.get_mut(&run_id).expect("cursor initialization retains its run lane");
    debug_assert!(!lane.observation.initialized);
    debug_assert!(lane.observation.cursor.is_none());
    lane.observation.initialized = true;
    lane.observation.resume_after = Some(cursor.through_revision);
    lane.observation.cursor = Some(cursor);
    Ok(())
  }

  fn register_owned(&self, run_id: RunId, ticket: u64, request: OwnedRequest) {
    let idempotency_key = request.idempotency_key();
    let mut lanes = self.inner.lanes.lock().unwrap();
    let lane = lanes.get_mut(&run_id).expect("an accepted write retains its run lane");
    let previous = lane.owned.insert(
      idempotency_key,
      OwnedSubmission {
        ticket,
        request,
        observed_commit: None,
        response_validated: false,
        artifact_response_commit: None,
      },
    );
    debug_assert!(previous.is_none(), "owned idempotency keys are unique within a run");
  }

  fn remove_owned(&self, run_id: RunId, idempotency_key: IdempotencyKey) -> Option<OwnedSubmission> {
    self.inner.lanes.lock().unwrap().get_mut(&run_id).and_then(|lane| lane.owned.remove(&idempotency_key))
  }

  fn observed_commit(&self, run_id: RunId, idempotency_key: IdempotencyKey) -> Option<RunCommit> {
    self.inner.lanes.lock().unwrap().get(&run_id)?.owned.get(&idempotency_key)?.observed_commit.clone()
  }

  fn complete_owned_write(&self, run_id: RunId, idempotency_key: IdempotencyKey, revision: RunRevision) {
    let (enqueue, ready) = {
      let mut lanes = self.inner.lanes.lock().unwrap();
      let Some(lane) = lanes.get_mut(&run_id) else {
        return;
      };
      let Some(owned) = lane.owned.get_mut(&idempotency_key) else {
        return;
      };
      debug_assert!(!owned.response_validated, "an owned write response validates once");
      owned.response_validated = true;
      if let Some(commit) = owned.observed_commit.clone() {
        let ticket = owned.ticket;
        lane.owned.remove(&idempotency_key);
        (false, Some((ticket, commit)))
      } else {
        lane.observation.targets.push_back(ObservationTarget {
          ticket: owned.ticket,
          idempotency_key,
          revision,
        });
        (true, None)
      }
    };
    if let Some((ticket, commit)) = ready {
      self.mark_projection_ready(ticket, projection_for_commit(&commit, &self.inner.projector_routes));
    }
    if enqueue {
      self.wake_observation(run_id);
    }
  }

  fn wake_observation(&self, run_id: RunId) {
    // Revision observation is independent of the ordinary write FIFO so a
    // blocked store response cannot retain the run's sole committed cursor.
    {
      let mut lanes = self.inner.lanes.lock().unwrap();
      let Some(lane) = lanes.get_mut(&run_id) else {
        return;
      };
      if lane.observation.draining {
        return;
      }
      lane.observation.draining = true;
    }

    loop {
      let work = {
        let mut lanes = self.inner.lanes.lock().unwrap();
        let Some(lane) = lanes.get_mut(&run_id) else {
          return;
        };
        if lane.observation.running {
          lane.observation.draining = false;
          return;
        }
        let target = loop {
          let Some(target) = lane.observation.targets.pop_front() else {
            break None;
          };
          if lane.owned.get(&target.idempotency_key).is_some_and(|owned| owned.observed_commit.is_none()) {
            break Some(target);
          }
        };
        let Some(target) = target else {
          lane.observation.draining = false;
          if !lane.indeterminate && !lane.running && lane.queue.is_empty() && lane.owned.is_empty() {
            lanes.remove(&run_id);
          }
          return;
        };
        let cursor = lane.observation.cursor.take();
        if let Some(cursor) = cursor.as_ref() {
          lane.observation.resume_after = Some(cursor.through_revision);
        }
        lane.observation.running = true;
        lane.observation.establishing = cursor.is_none();
        ObservationWork {
          run_id,
          target,
          cursor,
          resume_after: lane.observation.resume_after,
        }
      };

      self.spawn_observation(work);
      let mut lanes = self.inner.lanes.lock().unwrap();
      let Some(lane) = lanes.get_mut(&run_id) else {
        return;
      };
      if lane.observation.running {
        lane.observation.draining = false;
        return;
      }
    }
  }

  fn spawn_observation(&self, work: ObservationWork) {
    let run_id = work.run_id;
    let target = work.target;
    let recovery = Arc::new(Mutex::new(Some(work)));
    let admission = Arc::new(SpawnAdmission::new());
    let task_admission = admission.clone();
    let task_recovery = recovery.clone();
    let dispatch = self.clone();
    let spawn_guard = ObservationSpawnGuard::new(admission.clone(), self.clone(), recovery.clone());
    let task = Box::pin(async move {
      let _spawn_guard = spawn_guard;
      let admitted = async move {
        if task_admission.start() {
          let work = task_recovery.lock().unwrap().take().expect("started observation task owns its work");
          let mut guard = ObservationTaskGuard::new(dispatch.clone(), run_id, target);
          let result = dispatch.observe_target(work).await;
          guard.complete(result);
        }
      };
      let _ = AssertUnwindSafe(admitted).catch_unwind().await;
    });
    let spawn = catch_unwind(AssertUnwindSafe(|| self.spawner().spawn(task)));
    let failure = match spawn {
      Ok(Ok(())) if admission.spawn_succeeded_needs_recovery() => {
        Some(DispatchFailure::new(DispatchStage::AuthorityRead, task_unwind_code()))
      }
      Ok(Ok(())) => None,
      Ok(Err(error)) if admission.spawn_failed_needs_recovery() => Some(DispatchFailure::new(DispatchStage::Spawn, error.code)),
      Err(_) if admission.spawn_failed_needs_recovery() => Some(DispatchFailure::new(DispatchStage::Spawn, spawn_panic_code())),
      Ok(Err(_)) | Err(_) => None,
    };
    if let Some(failure) = failure
      && let Some(work) = recovery.lock().unwrap().take()
    {
      self.fail_observation(work.run_id, work.target, failure, false, None, true);
    }
  }

  async fn observe_target(&self, mut work: ObservationWork) -> ObservationResult {
    let route = self.inner.route.as_ref().expect("authority observation requires a route");
    let mut cursor = match work.cursor.take() {
      Some(cursor) => cursor,
      None => match work.resume_after {
        Some(after) => match AuthorityCursor::resume(route, work.run_id, after).await {
          Ok(cursor) => cursor,
          Err(failure) => {
            return ObservationResult {
              cursor: None,
              resume_after: work.resume_after,
              commits: Vec::new(),
              failure: Some(failure),
            };
          }
        },
        None => match AuthorityCursor::establish(route, work.run_id).await {
          Ok(cursor) => cursor,
          Err(failure) => {
            return ObservationResult {
              cursor: None,
              resume_after: None,
              commits: Vec::new(),
              failure: Some(failure),
            };
          }
        },
      },
    };
    match cursor.observe_through(route, work.target.revision).await {
      Ok(observation) => ObservationResult {
        resume_after: Some(cursor.through_revision),
        cursor: observation.cursor_usable.then_some(cursor),
        commits: observation.commits,
        failure: None,
      },
      Err(failure) => {
        let CursorObservationFailure { failure, commits } = failure;
        let cursor = (!failure.is_integrity()).then_some(cursor);
        ObservationResult {
          resume_after: cursor.as_ref().map(|cursor| cursor.through_revision),
          cursor,
          commits,
          failure: Some(failure),
        }
      }
    }
  }

  fn finish_observation(&self, run_id: RunId, target: ObservationTarget, result: ObservationResult) {
    let ObservationResult {
      cursor,
      resume_after,
      commits,
      failure,
    } = result;
    let skip_target = failure.is_some();
    let observed = self.apply_observed_commits(run_id, &commits, skip_target.then_some(target.idempotency_key));
    let target_observed = observed.target_observed(target.idempotency_key);
    let mut failure = failure;
    let mismatch = observed.mismatch;
    if let Some(mismatch) = mismatch.as_ref() {
      failure = Some(CursorFailure::integrity(mismatch.code.clone()));
    } else if failure.is_none() && !target_observed {
      failure = Some(CursorFailure::integrity(committed_cursor_mismatch_code()));
    }

    if let Some(failure) = failure {
      let integrity = failure.is_integrity();
      let code = failure.into_code();
      let mut settle_target = true;
      if let Some(mismatch) = mismatch {
        let mismatch_failure = DispatchFailure::new(DispatchStage::AuthorityRead, code.clone());
        self.mark_projection_skipped(mismatch.ticket);
        match mismatch.receipt {
          Some(receipt) => {
            self.deliver_artifact_receipt(receipt, Err(ArtifactWriteError::Integrity(code.clone())), Some(mismatch_failure.clone()));
            self.terminalize_unreported(mismatch.ticket, vec![mismatch_failure]);
          }
          None => self.terminalize(mismatch.ticket, vec![mismatch_failure]),
        }
        settle_target = mismatch.ticket != target.ticket;
      }
      self.fail_observation(run_id, target, DispatchFailure::new(DispatchStage::AuthorityRead, code), integrity, cursor, settle_target);
      return;
    }

    {
      let mut lanes = self.inner.lanes.lock().unwrap();
      if let Some(lane) = lanes.get_mut(&run_id) {
        lane.observation.running = false;
        lane.observation.establishing = false;
        lane.observation.initialized = cursor.is_some();
        lane.observation.resume_after = resume_after;
        lane.observation.cursor = cursor;
      }
    }
    self.wake_observation(run_id);
    self.wake_lane(run_id);
  }

  fn apply_observed_commits(&self, run_id: RunId, commits: &[RunCommit], skip: Option<IdempotencyKey>) -> ObservedCommits {
    let mut ready = Vec::new();
    let mut observed_keys = Vec::new();
    let mut mismatch = None;
    {
      let mut lanes = self.inner.lanes.lock().unwrap();
      let lane = lanes.get_mut(&run_id).expect("active observation retains its run lane");
      let mut projection = self.inner.projection.lock().unwrap();
      for commit in commits {
        let key = commit.idempotency_key();
        if skip == Some(key) {
          continue;
        }
        let Some(owned) = lane.owned.get_mut(&key) else {
          continue;
        };
        let mismatch_code = if !owned.request.matches(commit) {
          Some(committed_cursor_mismatch_code())
        } else if matches!(&owned.request, OwnedRequest::Artifact { .. })
          && owned.artifact_response_commit.as_ref().is_some_and(|response| response != commit)
        {
          Some(commit_response_mismatch_code())
        } else {
          None
        };
        if let Some(code) = mismatch_code {
          let ticket = owned.ticket;
          let owned = lane.owned.remove(&key).expect("the mismatching owned submission is present");
          lane.observation.targets.retain(|target| target.idempotency_key != key);
          lane.indeterminate = true;
          let receipt = match owned.request {
            OwnedRequest::Ordinary(_) => None,
            OwnedRequest::Artifact { settlement, .. } => settlement.claim(),
          };
          mismatch = Some(ObservedCommitMismatch {
            ticket,
            receipt,
            code,
          });
          break;
        }
        if owned.observed_commit.is_some() {
          continue;
        }
        owned.observed_commit = Some(commit.clone());
        observed_keys.push(key);
        projection.stage(owned.ticket);
        lane.observation.targets.retain(|target| target.idempotency_key != key);
        if owned.response_validated {
          let owned = lane.owned.remove(&key).expect("the response-validated owned submission is present");
          match owned.request {
            OwnedRequest::Ordinary(_) => ready.push(ObservedReady::Ordinary {
              ticket: owned.ticket,
              commit: commit.clone(),
            }),
            OwnedRequest::Artifact { settlement, .. } => {
              if let Some(receipt) = settlement.claim() {
                ready.push(ObservedReady::Artifact {
                  ticket: owned.ticket,
                  commit: commit.clone(),
                  receipt,
                });
              }
            }
          }
        }
      }
    }
    for item in ready {
      match item {
        ObservedReady::Ordinary { ticket, commit } => {
          self.mark_projection_ready(ticket, projection_for_commit(&commit, &self.inner.projector_routes));
        }
        ObservedReady::Artifact {
          ticket,
          commit,
          receipt,
        } => {
          let metadata = artifact_metadata(&commit).expect("validated observed artifact commit contains metadata").clone();
          self.mark_projection_ready(ticket, projection_for_commit(&commit, &self.inner.projector_routes));
          self.deliver_artifact_receipt(receipt, Ok(metadata), None);
        }
      }
    }
    ObservedCommits {
      observed_keys,
      mismatch,
    }
  }

  fn fail_observation(
    &self,
    run_id: RunId,
    target: ObservationTarget,
    failure: DispatchFailure,
    quarantine: bool,
    cursor: Option<AuthorityCursor>,
    settle_target: bool,
  ) {
    let target_settlement = {
      let mut lanes = self.inner.lanes.lock().unwrap();
      let Some(lane) = lanes.get_mut(&run_id) else {
        return;
      };
      lane.observation.running = false;
      lane.observation.establishing = false;
      lane.observation.initialized = cursor.is_some();
      if quarantine {
        lane.observation.resume_after = None;
      } else if let Some(cursor) = cursor.as_ref() {
        lane.observation.resume_after = Some(cursor.through_revision);
      }
      lane.observation.cursor = cursor;
      if quarantine {
        lane.indeterminate = true;
      }
      if settle_target {
        lane.owned.remove(&target.idempotency_key).and_then(|owned| match owned.request {
          OwnedRequest::Ordinary(_) => Some(ObservationFailureTarget::Ordinary),
          OwnedRequest::Artifact { settlement, .. } => settlement.claim().map(ObservationFailureTarget::Artifact),
        })
      } else {
        None
      }
    };
    match target_settlement {
      Some(ObservationFailureTarget::Ordinary) => {
        self.mark_projection_skipped(target.ticket);
        self.terminalize(target.ticket, vec![failure]);
      }
      Some(ObservationFailureTarget::Artifact(receipt)) => {
        let error = if quarantine {
          ArtifactWriteError::Integrity(failure.code().clone())
        } else {
          ArtifactWriteError::Unavailable(failure.code().clone())
        };
        self.mark_projection_skipped(target.ticket);
        self.deliver_artifact_receipt(receipt, Err(error), Some(failure.clone()));
        self.terminalize_unreported(target.ticket, vec![failure]);
      }
      None => {}
    }
    self.wake_observation(run_id);
    self.wake_lane(run_id);
  }

  fn spawn_artifact_task(&self, run_id: RunId, ticket: u64, admission: Box<ArtifactAdmission>) {
    let admission = *admission;
    let request = admission.request;
    let recovery = Arc::new(Mutex::new(Some(ArtifactTaskToken {
      run_id,
      ticket,
      request: request.clone(),
      settlement: admission.settlement,
      lookup_attempted: Arc::new(AtomicBool::new(false)),
    })));
    let spawn_admission = Arc::new(SpawnAdmission::new());
    let task_admission = spawn_admission.clone();
    let task_recovery = recovery.clone();
    let dispatch = self.clone();
    let spawn_guard = ArtifactSpawnGuard::new(spawn_admission.clone(), self.clone(), recovery.clone());
    let task = Box::pin(async move {
      let _spawn_guard = spawn_guard;
      let admitted = async move {
        if task_admission.start() {
          let token = task_recovery.lock().unwrap().take().expect("started artifact task owns its recovery token");
          let mut guard = ArtifactTaskGuard::new(dispatch.clone(), token);
          let result = dispatch.write_artifact_once(request, admission.body, guard.token().lookup_attempted.clone()).await;
          guard.complete(result);
        }
      };
      let _ = AssertUnwindSafe(admitted).catch_unwind().await;
    });
    let spawn = catch_unwind(AssertUnwindSafe(|| self.spawner().spawn(task)));
    let failure = match spawn {
      Ok(Ok(())) if spawn_admission.spawn_succeeded_needs_recovery() => Some((DispatchStage::ArtifactWrite, task_unwind_code())),
      Ok(Ok(())) => None,
      Ok(Err(error)) if spawn_admission.spawn_failed_needs_recovery() => Some((DispatchStage::Spawn, error.code)),
      Err(_) if spawn_admission.spawn_failed_needs_recovery() => Some((DispatchStage::Spawn, spawn_panic_code())),
      Ok(Err(_)) | Err(_) => None,
    };
    if let Some((stage, code)) = failure
      && let Some(token) = recovery.lock().unwrap().take()
    {
      self.finish_artifact_result(token, ArtifactTaskResult::authority(Err(ArtifactWriteError::Unavailable(code))), stage);
    }
  }

  async fn write_artifact_once(
    &self,
    request: StoreArtifactRequest,
    body: crate::ArtifactBody,
    lookup_attempted: Arc<AtomicBool>,
  ) -> ArtifactTaskResult {
    let route = self.inner.route.as_ref().expect("artifact task requires an authority route");
    let write = catch_unwind(AssertUnwindSafe(|| route.store.write_artifact(request.clone(), body)));
    let result = match write {
      Ok(future) => match AssertUnwindSafe(future).catch_unwind().await {
        Ok(result) => result,
        Err(_) => {
          return ArtifactTaskResult::authority(self.resolve_artifact_publication(&request, task_unwind_code(), &lookup_attempted).await);
        }
      },
      Err(_) => {
        return ArtifactTaskResult::authority(self.resolve_artifact_publication(&request, task_unwind_code(), &lookup_attempted).await);
      }
    };
    match result {
      Ok(result) if artifact_commit_matches_request(result.commit(), &request) => ArtifactTaskResult::authority(Ok(result.into_commit())),
      Ok(_) => ArtifactTaskResult::DirectResponseContradiction,
      Err(ArtifactWriteError::PublicationUnknown(code)) => {
        ArtifactTaskResult::authority(self.resolve_artifact_publication(&request, code, &lookup_attempted).await)
      }
      Err(error) => ArtifactTaskResult::authority(Err(error)),
    }
  }

  async fn resolve_artifact_publication(
    &self,
    request: &StoreArtifactRequest,
    unknown_code: ErrorCode,
    lookup_attempted: &AtomicBool,
  ) -> Result<RunCommit, ArtifactWriteError> {
    if lookup_attempted.swap(true, Ordering::SeqCst) {
      return Err(ArtifactWriteError::PublicationUnknown(unknown_code));
    }
    let route = self.inner.route.as_ref().expect("artifact lookup requires an authority route");
    let lookup = catch_unwind(AssertUnwindSafe(|| route.store.lookup_commit(request.run_id(), request.idempotency_key())));
    let result = match lookup {
      Ok(future) => match AssertUnwindSafe(future).catch_unwind().await {
        Ok(result) => result,
        Err(_) => return Err(ArtifactWriteError::PublicationUnknown(unknown_code)),
      },
      Err(_) => return Err(ArtifactWriteError::PublicationUnknown(unknown_code)),
    };
    match result {
      Ok(Some(commit)) if artifact_commit_matches_request(&commit, request) => Ok(commit),
      Ok(Some(_)) | Ok(None) | Err(_) => Err(ArtifactWriteError::PublicationUnknown(unknown_code)),
    }
  }

  fn finish_artifact_result(&self, token: ArtifactTaskToken, result: ArtifactTaskResult, failure_stage: DispatchStage) {
    let ArtifactTaskToken {
      run_id,
      ticket,
      request,
      settlement,
      ..
    } = token;
    let idempotency_key = request.idempotency_key();
    let (result, direct_contradiction) = match result {
      ArtifactTaskResult::Authority(result) => (result, false),
      ArtifactTaskResult::DirectResponseContradiction => (Err(ArtifactWriteError::Integrity(commit_response_mismatch_code())), true),
    };
    let completion = {
      let mut lanes = self.inner.lanes.lock().unwrap();
      let Some(lane) = lanes.get_mut(&run_id) else {
        return;
      };
      if !lane.owned.contains_key(&idempotency_key) {
        return;
      }
      match result {
        Ok(commit) => {
          if self.inner.projector_routes.is_empty() {
            lane.owned.remove(&idempotency_key);
            settlement.claim().map(|receipt| ArtifactWorkerCompletion::Success {
              receipt,
              metadata: Box::new(artifact_metadata(&commit).expect("validated artifact commit contains metadata").clone()),
              ready_commit: None,
              terminal: true,
            })
          } else {
            let observed_commit = lane.owned.get(&idempotency_key).and_then(|owned| owned.observed_commit.clone());
            if observed_commit.as_ref().is_some_and(|observed| observed != &commit) {
              lane.owned.remove(&idempotency_key);
              lane.indeterminate = true;
              let error = ArtifactWriteError::Integrity(commit_response_mismatch_code());
              let failure = DispatchFailure::new(failure_stage, artifact_error_code(&error));
              settlement.claim().map(|receipt| ArtifactWorkerCompletion::Failure {
                receipt,
                error,
                failure,
              })
            } else {
              let owned = lane.owned.get_mut(&idempotency_key).expect("a claimed artifact settlement retains its owned submission");
              debug_assert!(!owned.response_validated, "an artifact response validates once");
              owned.response_validated = true;
              owned.artifact_response_commit = Some(commit.clone());
              if let Some(ready_commit) = observed_commit {
                lane.owned.remove(&idempotency_key);
                settlement.claim().map(|receipt| ArtifactWorkerCompletion::Success {
                  receipt,
                  metadata: Box::new(artifact_metadata(&ready_commit).expect("observed artifact commit contains metadata").clone()),
                  ready_commit: Some(ready_commit),
                  terminal: false,
                })
              } else {
                lane.observation.targets.push_back(ObservationTarget {
                  ticket,
                  idempotency_key,
                  revision: commit.revision(),
                });
                None
              }
            }
          }
        }
        Err(ArtifactWriteError::PublicationUnknown(_))
          if lane.owned.get(&idempotency_key).and_then(|owned| owned.observed_commit.as_ref()).is_some() =>
        {
          let owned = lane.owned.remove(&idempotency_key).expect("an observed artifact retains its owned submission");
          let commit = owned.observed_commit.expect("the guarded artifact commit is present");
          settlement.claim().map(|receipt| ArtifactWorkerCompletion::Success {
            receipt,
            metadata: Box::new(artifact_metadata(&commit).expect("observed artifact commit contains metadata").clone()),
            ready_commit: Some(commit),
            terminal: false,
          })
        }
        Err(error) => {
          let contradiction =
            direct_contradiction || lane.owned.get(&idempotency_key).and_then(|owned| owned.observed_commit.as_ref()).is_some();
          let error = if contradiction {
            ArtifactWriteError::Integrity(commit_response_mismatch_code())
          } else {
            error
          };
          let failure = DispatchFailure::new(failure_stage, artifact_error_code(&error));
          lane.owned.remove(&idempotency_key);
          if contradiction {
            lane.indeterminate = true;
          }
          settlement.claim().map(|receipt| ArtifactWorkerCompletion::Failure {
            receipt,
            error,
            failure,
          })
        }
      }
    };
    match completion {
      None => {}
      Some(ArtifactWorkerCompletion::Success {
        receipt,
        metadata,
        ready_commit,
        terminal,
      }) => {
        if let Some(commit) = ready_commit {
          self.mark_projection_ready(ticket, projection_for_commit(&commit, &self.inner.projector_routes));
        }
        if terminal {
          self.terminalize_unreported(ticket, Vec::new());
        }
        self.deliver_artifact_receipt(receipt, Ok(*metadata), None);
      }
      Some(ArtifactWorkerCompletion::Failure {
        receipt,
        error,
        failure,
      }) => {
        self.mark_projection_skipped(ticket);
        self.deliver_artifact_receipt(receipt, Err(error), Some(failure.clone()));
        self.terminalize_unreported(ticket, vec![failure]);
      }
    }
    self.wake_observation(run_id);
    self.wake_lane(run_id);
  }

  fn recover_started_artifact(&self, token: ArtifactTaskToken) {
    let recovery = Arc::new(Mutex::new(Some(token)));
    let spawn_admission = Arc::new(SpawnAdmission::new());
    let task_admission = spawn_admission.clone();
    let task_recovery = recovery.clone();
    let dispatch = self.clone();
    let spawn_guard = ArtifactLookupSpawnGuard::new(spawn_admission.clone(), self.clone(), recovery.clone());
    let task = Box::pin(async move {
      let _spawn_guard = spawn_guard;
      if task_admission.start() {
        let token = task_recovery.lock().unwrap().take().expect("started artifact lookup owns its recovery token");
        let mut guard = ArtifactLookupTaskGuard::new(dispatch.clone(), token);
        let result =
          dispatch.resolve_artifact_publication(&guard.token().request, task_unwind_code(), guard.token().lookup_attempted.as_ref()).await;
        guard.complete(result);
      }
    });
    let spawn = catch_unwind(AssertUnwindSafe(|| self.spawner().spawn(task)));
    let needs_recovery = match spawn {
      Ok(Ok(())) => spawn_admission.spawn_succeeded_needs_recovery(),
      Ok(Err(_)) | Err(_) => spawn_admission.spawn_failed_needs_recovery(),
    };
    if needs_recovery && let Some(token) = recovery.lock().unwrap().take() {
      self.finish_artifact_result(
        token,
        ArtifactTaskResult::authority(Err(ArtifactWriteError::PublicationUnknown(task_unwind_code()))),
        DispatchStage::ArtifactWrite,
      );
    }
  }

  fn terminalize(&self, ticket: u64, failures: Vec<DispatchFailure>) {
    self.terminalize_inner(ticket, failures, true);
  }

  fn terminalize_unreported(&self, ticket: u64, failures: Vec<DispatchFailure>) {
    self.terminalize_inner(ticket, failures, false);
  }

  fn terminalize_inner(&self, ticket: u64, failures: Vec<DispatchFailure>, report: bool) {
    let (accepted, waker) = {
      let mut progress = self.inner.progress.lock().unwrap();
      let accepted = progress.terminalize(ticket, failures.clone());
      (accepted, progress.take_ready_front_waker())
    };
    if accepted && report {
      self.report_failures(&failures);
    }
    if let Some(waker) = waker {
      waker.wake();
    }
  }

  fn finish_unstarted_artifact(&self, ticket: u64, admission: Box<ArtifactAdmission>, failure: DispatchFailure) {
    let error = ArtifactWriteError::Unavailable(failure.code().clone());
    self.mark_projection_skipped(ticket);
    if let Some(receipt) = admission.settlement.claim() {
      self.deliver_artifact_receipt(receipt, Err(error), Some(failure.clone()));
    }
    self.terminalize_unreported(ticket, vec![failure]);
  }

  fn deliver_artifact_receipt(
    &self,
    receipt: ArtifactReceiptSender,
    result: Result<ArtifactMetadata, ArtifactWriteError>,
    unobserved_failure: Option<DispatchFailure>,
  ) {
    let message = ArtifactReceiptMessage {
      result,
      unobserved_failure,
    };
    if let Err(message) = receipt.send(message)
      && let Some(failure) = message.unobserved_failure
    {
      self.report_unobserved_artifact_failure(&failure);
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

  fn recover_lane_task(&self, run_id: RunId, active: Option<ActiveLaneWork>, quarantine: bool) {
    let observed = {
      let mut lanes = self.inner.lanes.lock().unwrap();
      let Some(lane) = lanes.get_mut(&run_id) else {
        return;
      };
      lane.running = false;
      let observed = active
        .as_ref()
        .and_then(|active| match active.recovery {
          ActiveLaneRecovery::Ordinary(Some(key)) => lane.owned.remove(&key),
          ActiveLaneRecovery::Ordinary(None) | ActiveLaneRecovery::ArtifactAdmission(_) => None,
        })
        .and_then(|owned| owned.observed_commit);
      if quarantine && observed.is_none() {
        lane.indeterminate = true;
      }
      observed
    };
    if let Some(active) = active {
      let failure = DispatchFailure::new(active.stage, task_unwind_code());
      match active.recovery {
        ActiveLaneRecovery::Ordinary(_) => {
          if let Some(commit) = observed {
            self.mark_projection_ready(active.ticket, projection_for_commit(&commit, &self.inner.projector_routes));
          } else {
            self.mark_projection_skipped(active.ticket);
            self.terminalize(active.ticket, vec![failure]);
          }
        }
        ActiveLaneRecovery::ArtifactAdmission(admission) => {
          self.finish_unstarted_artifact(active.ticket, admission, failure);
        }
      }
    }
    self.wake_lane(run_id);
  }

  fn finish_unpolled_lane_task(&self, run_id: RunId) {
    self.settle_unpolled_lane_task(run_id, DispatchFailure::new(DispatchStage::AuthorityCommit, task_unwind_code()));
    self.wake_lane(run_id);
  }

  fn settle_unpolled_lane_task(&self, run_id: RunId, failure: DispatchFailure) {
    let entry = {
      let mut lanes = self.inner.lanes.lock().unwrap();
      let lane = lanes.get_mut(&run_id).expect("an accepted unpolled task retains its run lane");
      lane.running = false;
      lane.queue.pop_front().expect("an accepted unpolled task retains its ready entry")
    };
    match entry.state {
      LaneEntryState::Artifact(admission) => self.finish_unstarted_artifact(entry.ticket, admission, failure),
      LaneEntryState::Ready(_) => {
        self.mark_projection_skipped(entry.ticket);
        self.terminalize(entry.ticket, vec![failure]);
      }
      LaneEntryState::Preparing | LaneEntryState::Failed(_) => {
        debug_assert!(false, "an unpolled lane task owns a ready entry");
      }
    }
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

struct ObservationSpawnGuard {
  admission: Arc<SpawnAdmission>,
  dispatch: Dispatch,
  recovery: Arc<Mutex<Option<ObservationWork>>>,
}

impl ObservationSpawnGuard {
  fn new(admission: Arc<SpawnAdmission>, dispatch: Dispatch, recovery: Arc<Mutex<Option<ObservationWork>>>) -> Self {
    Self {
      admission,
      dispatch,
      recovery,
    }
  }
}

impl Drop for ObservationSpawnGuard {
  fn drop(&mut self) {
    if self.admission.task_dropped_before_start()
      && let Some(work) = self.recovery.lock().unwrap().take()
    {
      self.dispatch.fail_observation(
        work.run_id,
        work.target,
        DispatchFailure::new(DispatchStage::AuthorityRead, task_unwind_code()),
        false,
        None,
        true,
      );
    }
  }
}

struct ObservationTaskGuard {
  dispatch: Dispatch,
  run_id: RunId,
  target: ObservationTarget,
  armed: bool,
}

impl ObservationTaskGuard {
  fn new(dispatch: Dispatch, run_id: RunId, target: ObservationTarget) -> Self {
    Self {
      dispatch,
      run_id,
      target,
      armed: true,
    }
  }

  fn complete(&mut self, result: ObservationResult) {
    self.armed = false;
    self.dispatch.finish_observation(self.run_id, self.target, result);
  }
}

impl Drop for ObservationTaskGuard {
  fn drop(&mut self) {
    if self.armed {
      self.dispatch.fail_observation(
        self.run_id,
        self.target,
        DispatchFailure::new(DispatchStage::AuthorityRead, task_unwind_code()),
        false,
        None,
        true,
      );
    }
  }
}

struct ArtifactTaskToken {
  run_id: RunId,
  ticket: u64,
  request: StoreArtifactRequest,
  settlement: ArtifactSettlement,
  lookup_attempted: Arc<AtomicBool>,
}

#[derive(Clone)]
struct ArtifactSettlement {
  receipt: Arc<Mutex<Option<ArtifactReceiptSender>>>,
}

impl ArtifactSettlement {
  fn new(receipt: ArtifactReceiptSender) -> Self {
    Self {
      receipt: Arc::new(Mutex::new(Some(receipt))),
    }
  }

  fn claim(&self) -> Option<ArtifactReceiptSender> {
    self.receipt.lock().unwrap().take()
  }
}

enum ArtifactWorkerCompletion {
  Success {
    receipt: ArtifactReceiptSender,
    metadata: Box<ArtifactMetadata>,
    ready_commit: Option<RunCommit>,
    terminal: bool,
  },
  Failure {
    receipt: ArtifactReceiptSender,
    error: ArtifactWriteError,
    failure: DispatchFailure,
  },
}

enum ArtifactTaskResult {
  Authority(Result<RunCommit, ArtifactWriteError>),
  DirectResponseContradiction,
}

impl ArtifactTaskResult {
  fn authority(result: Result<RunCommit, ArtifactWriteError>) -> Self {
    Self::Authority(result)
  }
}

struct ArtifactSpawnGuard {
  admission: Arc<SpawnAdmission>,
  dispatch: Dispatch,
  recovery: Arc<Mutex<Option<ArtifactTaskToken>>>,
}

impl ArtifactSpawnGuard {
  fn new(admission: Arc<SpawnAdmission>, dispatch: Dispatch, recovery: Arc<Mutex<Option<ArtifactTaskToken>>>) -> Self {
    Self {
      admission,
      dispatch,
      recovery,
    }
  }
}

impl Drop for ArtifactSpawnGuard {
  fn drop(&mut self) {
    if self.admission.task_dropped_before_start()
      && let Some(token) = self.recovery.lock().unwrap().take()
    {
      self.dispatch.finish_artifact_result(
        token,
        ArtifactTaskResult::authority(Err(ArtifactWriteError::Unavailable(task_unwind_code()))),
        DispatchStage::ArtifactWrite,
      );
    }
  }
}

struct ArtifactTaskGuard {
  dispatch: Dispatch,
  token: Option<ArtifactTaskToken>,
}

impl ArtifactTaskGuard {
  fn new(dispatch: Dispatch, token: ArtifactTaskToken) -> Self {
    Self {
      dispatch,
      token: Some(token),
    }
  }

  fn token(&self) -> &ArtifactTaskToken {
    self.token.as_ref().expect("artifact task retains its token")
  }

  fn complete(&mut self, result: ArtifactTaskResult) {
    let token = self.token.take().expect("artifact task completes once");
    self.dispatch.finish_artifact_result(token, result, DispatchStage::ArtifactWrite);
  }
}

impl Drop for ArtifactTaskGuard {
  fn drop(&mut self) {
    if let Some(token) = self.token.take() {
      self.dispatch.recover_started_artifact(token);
    }
  }
}

struct ArtifactLookupSpawnGuard {
  admission: Arc<SpawnAdmission>,
  dispatch: Dispatch,
  recovery: Arc<Mutex<Option<ArtifactTaskToken>>>,
}

impl ArtifactLookupSpawnGuard {
  fn new(admission: Arc<SpawnAdmission>, dispatch: Dispatch, recovery: Arc<Mutex<Option<ArtifactTaskToken>>>) -> Self {
    Self {
      admission,
      dispatch,
      recovery,
    }
  }
}

impl Drop for ArtifactLookupSpawnGuard {
  fn drop(&mut self) {
    if self.admission.task_dropped_before_start()
      && let Some(token) = self.recovery.lock().unwrap().take()
    {
      self.dispatch.finish_artifact_result(
        token,
        ArtifactTaskResult::authority(Err(ArtifactWriteError::PublicationUnknown(task_unwind_code()))),
        DispatchStage::ArtifactWrite,
      );
    }
  }
}

struct ArtifactLookupTaskGuard {
  dispatch: Dispatch,
  token: Option<ArtifactTaskToken>,
}

impl ArtifactLookupTaskGuard {
  fn new(dispatch: Dispatch, token: ArtifactTaskToken) -> Self {
    Self {
      dispatch,
      token: Some(token),
    }
  }

  fn token(&self) -> &ArtifactTaskToken {
    self.token.as_ref().expect("artifact lookup task retains its token")
  }

  fn complete(&mut self, result: Result<RunCommit, ArtifactWriteError>) {
    let token = self.token.take().expect("artifact lookup task completes once");
    self.dispatch.finish_artifact_result(token, ArtifactTaskResult::authority(result), DispatchStage::ArtifactWrite);
  }
}

impl Drop for ArtifactLookupTaskGuard {
  fn drop(&mut self) {
    if let Some(token) = self.token.take() {
      self.dispatch.finish_artifact_result(
        token,
        ArtifactTaskResult::authority(Err(ArtifactWriteError::PublicationUnknown(task_unwind_code()))),
        DispatchStage::ArtifactWrite,
      );
    }
  }
}

type ProjectionBatch = Vec<RoutedTelemetryItem>;

struct RoutedTelemetryItem {
  route_items: Vec<TelemetryItem>,
}

struct ProjectionState {
  draining: bool,
  running: bool,
  authority_ordered: bool,
  next_commit_order: u64,
  entries: BTreeMap<u64, ProjectionEntry>,
  flushes: VecDeque<ProjectionFlush>,
}

impl ProjectionState {
  fn new(authority_ordered: bool) -> Self {
    Self {
      draining: false,
      running: false,
      authority_ordered,
      next_commit_order: 0,
      entries: BTreeMap::new(),
      flushes: VecDeque::new(),
    }
  }

  fn prepare(&mut self, ticket: u64) {
    let previous = self.entries.insert(ticket, ProjectionEntry::Preparing);
    debug_assert!(previous.is_none(), "dispatch projection tickets are unique");
  }

  fn stage(&mut self, ticket: u64) {
    debug_assert!(self.authority_ordered, "only authority commits have a staged projection state");
    let Some(entry) = self.entries.get_mut(&ticket) else {
      debug_assert!(false, "a prepared projection ticket remains queued");
      return;
    };
    match entry {
      ProjectionEntry::Preparing => {
        self.next_commit_order = self.next_commit_order.checked_add(1).expect("authority projection order exhausted");
        *entry = ProjectionEntry::Staged {
          order: self.next_commit_order,
        };
      }
      ProjectionEntry::Staged { .. } | ProjectionEntry::Ready { .. } => {
        debug_assert!(false, "an authority projection ticket is staged once");
      }
    }
  }

  fn ready(&mut self, ticket: u64, items: ProjectionBatch) {
    let Some(entry) = self.entries.get_mut(&ticket) else {
      debug_assert!(false, "a prepared projection ticket remains queued");
      return;
    };
    let order = match entry {
      ProjectionEntry::Preparing if !self.authority_ordered => ticket,
      ProjectionEntry::Staged { order } if self.authority_ordered => *order,
      ProjectionEntry::Preparing | ProjectionEntry::Staged { .. } => {
        debug_assert!(false, "authority projection readiness follows cursor staging");
        return;
      }
      ProjectionEntry::Ready { .. } => {
        debug_assert!(false, "a projection ticket becomes ready once");
        return;
      }
    };
    *entry = ProjectionEntry::Ready { order, items };
  }

  fn skip(&mut self, ticket: u64) {
    let Some(entry) = self.entries.get(&ticket) else {
      return;
    };
    if matches!(entry, ProjectionEntry::Preparing | ProjectionEntry::Staged { .. }) {
      self.entries.remove(&ticket);
    }
  }

  fn next_action(&mut self) -> Option<ProjectionAction> {
    let barrier = self.flushes.front().map(|flush| flush.barrier);
    if let Some(barrier) = barrier
      && self.entries.range(..=barrier).next().is_none()
    {
      let flush = self.flushes.pop_front().expect("front flush was present");
      return Some(ProjectionAction::Flush {
        ordering_id: flush.ordering_id,
      });
    }

    let ticket = if self.authority_ordered {
      // A ready post-barrier commit may precede a pre-barrier commit at the
      // authority. Preserve commit order, but never wait for preparing work.
      let candidate = self
        .entries
        .iter()
        .filter_map(|(ticket, entry)| match entry {
          ProjectionEntry::Staged { order } => Some((*order, *ticket, false)),
          ProjectionEntry::Ready { order, .. } => Some((*order, *ticket, true)),
          ProjectionEntry::Preparing => None,
        })
        .min_by_key(|(order, _, _)| *order)?;
      if !candidate.2 {
        return None;
      }
      candidate.1
    } else {
      let eligible = |ticket: &u64| barrier.is_none_or(|barrier| *ticket <= barrier);
      let (&ticket, entry) = self.entries.iter().find(|(ticket, _)| eligible(ticket))?;
      if matches!(entry, ProjectionEntry::Preparing | ProjectionEntry::Staged { .. }) {
        return None;
      }
      ticket
    };
    let Some(ProjectionEntry::Ready { items, .. }) = self.entries.remove(&ticket) else {
      unreachable!("selected projection entry was ready")
    };
    Some(ProjectionAction::Project { ticket, items })
  }
}

enum ProjectionEntry {
  Preparing,
  Staged { order: u64 },
  Ready { order: u64, items: ProjectionBatch },
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
  commits: Vec<RunCommit>,
}

impl CursorObservationFailure {
  fn new(failure: CursorFailure, commits: Vec<RunCommit>) -> Self {
    Self { failure, commits }
  }
}

struct CursorObservation {
  commits: Vec<RunCommit>,
  cursor_usable: bool,
}

struct OwnedSubmission {
  ticket: u64,
  request: OwnedRequest,
  observed_commit: Option<RunCommit>,
  response_validated: bool,
  artifact_response_commit: Option<RunCommit>,
}

enum OwnedRequest {
  Ordinary(RunCommitRequest),
  Artifact {
    request: StoreArtifactRequest,
    settlement: ArtifactSettlement,
  },
}

impl OwnedRequest {
  fn idempotency_key(&self) -> IdempotencyKey {
    match self {
      Self::Ordinary(request) => request.idempotency_key(),
      Self::Artifact { request, .. } => request.idempotency_key(),
    }
  }

  fn matches(&self, commit: &RunCommit) -> bool {
    match self {
      Self::Ordinary(request) => commit_matches_request(commit, request),
      Self::Artifact { request, .. } => artifact_commit_matches_request(commit, request),
    }
  }
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
    })
  }

  async fn resume(route: &AuthorityRoute, run_id: RunId, through_revision: RunRevision) -> Result<Self, CursorFailure> {
    let subscription = route.store.subscribe(run_id, through_revision).await.map_err(cursor_failure_from_read)?;
    Ok(Self {
      authority_id: route.authority_id,
      run_id,
      through_revision,
      subscription,
    })
  }

  async fn observe_through(&mut self, route: &AuthorityRoute, target: RunRevision) -> Result<CursorObservation, CursorObservationFailure> {
    let mut commits = Vec::new();
    let mut resume = false;
    while self.through_revision < target {
      match poll_subscription_once(&mut self.subscription).await {
        Poll::Ready(Some(Ok(commit))) => {
          if let Err(failure) = self.observe_commit(commit, &mut commits) {
            return Err(CursorObservationFailure::new(failure, commits));
          }
        }
        Poll::Ready(Some(Err(SubscriptionError::Gap { .. }))) | Poll::Pending | Poll::Ready(None) => {
          if let Err(failure) = self.recover_through(route, target, &mut commits).await {
            return Err(CursorObservationFailure::new(failure, commits));
          }
          resume = true;
        }
        Poll::Ready(Some(Err(SubscriptionError::Store(error)))) => {
          return Err(CursorObservationFailure::new(cursor_failure_from_read(error), commits));
        }
      }
    }
    if resume {
      self.subscription = match route.store.subscribe(self.run_id, self.through_revision).await {
        Ok(subscription) => subscription,
        Err(_) => {
          return Ok(CursorObservation {
            commits,
            cursor_usable: false,
          });
        }
      };
    }
    Ok(CursorObservation {
      commits,
      cursor_usable: true,
    })
  }

  async fn recover_through(
    &mut self,
    route: &AuthorityRoute,
    target: RunRevision,
    commits: &mut Vec<RunCommit>,
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
        self.observe_commit(commit, commits)?;
      }
      if self.through_revision < target && !page.has_more() {
        return Err(CursorFailure::integrity(committed_cursor_mismatch_code()));
      }
    }
    Ok(())
  }

  fn observe_commit(&mut self, commit: RunCommit, commits: &mut Vec<RunCommit>) -> Result<(), CursorFailure> {
    let expected_revision =
      self.through_revision.get().checked_add(1).ok_or_else(|| CursorFailure::integrity(committed_cursor_mismatch_code()))?;
    if commit.authority_id() != self.authority_id || commit.run_id() != self.run_id || commit.revision().get() != expected_revision {
      return Err(CursorFailure::integrity(committed_cursor_mismatch_code()));
    }
    self.through_revision = commit.revision();
    commits.push(commit);
    Ok(())
  }
}

async fn poll_subscription_once(subscription: &mut RunSubscription) -> Poll<Option<Result<RunCommit, SubscriptionError>>> {
  futures_util::future::poll_fn(|context| Poll::Ready(subscription.as_mut().poll_next(context))).await
}

struct LaneDrainGuard {
  dispatch: Dispatch,
  run_id: RunId,
  active: Option<ActiveLaneWork>,
  quarantine_on_drop: bool,
  armed: bool,
}

struct ActiveLaneWork {
  ticket: u64,
  stage: DispatchStage,
  recovery: ActiveLaneRecovery,
}

enum ActiveLaneRecovery {
  Ordinary(Option<IdempotencyKey>),
  ArtifactAdmission(Box<ArtifactAdmission>),
}

impl LaneDrainGuard {
  fn new(dispatch: Dispatch, run_id: RunId) -> Self {
    Self {
      dispatch,
      run_id,
      active: None,
      quarantine_on_drop: false,
      armed: true,
    }
  }

  fn activate(&mut self, ticket: u64, stage: DispatchStage) {
    debug_assert!(
      self
        .active
        .replace(ActiveLaneWork {
          ticket,
          stage,
          recovery: ActiveLaneRecovery::Ordinary(None),
        })
        .is_none(),
      "lane task owns at most one active ticket"
    );
  }

  fn activate_artifact(&mut self, ticket: u64, stage: DispatchStage, admission: Box<ArtifactAdmission>) {
    debug_assert!(
      self
        .active
        .replace(ActiveLaneWork {
          ticket,
          stage,
          recovery: ActiveLaneRecovery::ArtifactAdmission(admission),
        })
        .is_none(),
      "lane task owns at most one active ticket"
    );
  }

  fn take_artifact(&mut self, ticket: u64) -> Box<ArtifactAdmission> {
    let active = self.active.take().expect("artifact cursor establishment owns its admission");
    debug_assert_eq!(active.ticket, ticket);
    let ActiveLaneRecovery::ArtifactAdmission(admission) = active.recovery else {
      unreachable!("active artifact work retains its admission")
    };
    admission
  }

  fn set_stage(&mut self, ticket: u64, stage: DispatchStage) {
    let active = self.active.as_mut().expect("lane task has active work");
    debug_assert_eq!(active.ticket, ticket);
    active.stage = stage;
  }

  fn set_idempotency_key(&mut self, ticket: u64, idempotency_key: IdempotencyKey) {
    let active = self.active.as_mut().expect("lane task has active work");
    debug_assert_eq!(active.ticket, ticket);
    let ActiveLaneRecovery::Ordinary(key) = &mut active.recovery else {
      unreachable!("only ordinary writes have a commit idempotency key")
    };
    debug_assert!(key.replace(idempotency_key).is_none());
  }

  fn has_active(&self, ticket: u64) -> bool {
    self.active.as_ref().is_some_and(|active| active.ticket == ticket)
  }

  fn complete(&mut self, ticket: u64) {
    debug_assert_eq!(self.active.as_ref().map(|active| active.ticket), Some(ticket), "lane task completes its active ticket");
    self.active = None;
  }

  fn disarm(&mut self) {
    self.armed = false;
  }
}

impl Drop for LaneDrainGuard {
  fn drop(&mut self) {
    if self.armed {
      self.dispatch.recover_lane_task(self.run_id, self.active.take(), self.quarantine_on_drop);
    }
  }
}

struct PreparationGuard {
  dispatch: Dispatch,
  authority_id: Option<AuthorityId>,
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
    self.dispatch.complete_preparation(self.ticket, self.authority_id, self.run_id, mutation);
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
  draining: bool,
  running: bool,
  indeterminate: bool,
  queue: VecDeque<LaneEntry>,
  owned: HashMap<IdempotencyKey, OwnedSubmission>,
  observation: ObservationLane,
}

#[derive(Default)]
struct ObservationLane {
  draining: bool,
  running: bool,
  establishing: bool,
  initialized: bool,
  resume_after: Option<RunRevision>,
  cursor: Option<AuthorityCursor>,
  targets: VecDeque<ObservationTarget>,
}

#[derive(Clone, Copy)]
struct ObservationTarget {
  ticket: u64,
  idempotency_key: IdempotencyKey,
  revision: RunRevision,
}

struct ObservationWork {
  run_id: RunId,
  target: ObservationTarget,
  cursor: Option<AuthorityCursor>,
  resume_after: Option<RunRevision>,
}

struct ObservationResult {
  cursor: Option<AuthorityCursor>,
  resume_after: Option<RunRevision>,
  commits: Vec<RunCommit>,
  failure: Option<CursorFailure>,
}

struct ObservedCommits {
  observed_keys: Vec<IdempotencyKey>,
  mismatch: Option<ObservedCommitMismatch>,
}

impl ObservedCommits {
  fn target_observed(&self, idempotency_key: IdempotencyKey) -> bool {
    self.observed_keys.contains(&idempotency_key)
  }
}

struct ObservedCommitMismatch {
  ticket: u64,
  receipt: Option<ArtifactReceiptSender>,
  code: ErrorCode,
}

enum ObservedReady {
  Ordinary {
    ticket: u64,
    commit: RunCommit,
  },
  Artifact {
    ticket: u64,
    commit: RunCommit,
    receipt: ArtifactReceiptSender,
  },
}

enum ObservationFailureTarget {
  Ordinary,
  Artifact(ArtifactReceiptSender),
}

struct LaneEntry {
  ticket: u64,
  state: LaneEntryState,
}

enum LaneEntryState {
  Preparing,
  Ready(RunCommitRequest),
  Artifact(Box<ArtifactAdmission>),
  Failed(DispatchFailure),
}

struct ArtifactAdmission {
  request: StoreArtifactRequest,
  body: crate::ArtifactBody,
  settlement: ArtifactSettlement,
}

enum LaneWake {
  Spawn,
  Terminal(u64, DispatchFailure),
}

enum LaneAction {
  Commit(u64, RunCommitRequest),
  StartArtifact(u64, Box<ArtifactAdmission>),
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
    .map(|fact| match fact {
      RunFact::SpanStarted(started) => {
        routed_span_start(Some(commit.authority_id()), commit.run_id(), Some(commit.revision()), started, routes)
      }
      RunFact::SpanEnded(ended) => routed_span_end(Some(commit.authority_id()), commit.run_id(), Some(commit.revision()), ended, routes),
      RunFact::EventOccurred(event) => routed_event(Some(commit.authority_id()), commit.run_id(), Some(commit.revision()), event, routes),
      RunFact::ArtifactPublished(published) => routed_artifact(commit, published, routes),
    })
    .collect()
}

fn routed_artifact(commit: &RunCommit, published: &crate::ArtifactPublished, routes: &[ProjectorRoute]) -> RoutedTelemetryItem {
  let metadata = published.metadata();
  RoutedTelemetryItem {
    route_items: routes
      .iter()
      .map(|route| TelemetryItem::Artifact {
        authority_id: commit.authority_id(),
        run_id: commit.run_id(),
        span_id: published.span_id(),
        uri: metadata.uri().clone(),
        purpose: metadata.purpose().clone(),
        content_type: metadata.content_type().clone(),
        byte_length: metadata.byte_length(),
        sha256: metadata.sha256(),
        attributes: route.policy.artifact_attributes(metadata.attributes()),
        revision: commit.revision(),
      })
      .collect(),
  }
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

fn artifact_commit_matches_request(commit: &RunCommit, request: &StoreArtifactRequest) -> bool {
  commit.authority_id() == request.authority_id()
    && commit.run_id() == request.run_id()
    && commit.idempotency_key() == request.idempotency_key()
    && commit.facts().len() == 1
    && commit.facts().first().is_some_and(|fact| match fact {
      RunFact::ArtifactPublished(published) => {
        let metadata = published.metadata();
        published.span_id() == request.span_id()
          && metadata.uri() == &crate::ArtifactUri::from_ids(request.run_id(), request.artifact_id())
          && metadata.purpose() == request.purpose()
          && metadata.content_type() == request.content_type()
          && metadata.byte_length() == request.expected_byte_length()
          && metadata.sha256() == request.expected_sha256()
          && metadata.attributes() == request.attributes()
      }
      RunFact::SpanStarted(_) | RunFact::SpanEnded(_) | RunFact::EventOccurred(_) => false,
    })
}

fn artifact_metadata(commit: &RunCommit) -> Option<&ArtifactMetadata> {
  match commit.facts() {
    [RunFact::ArtifactPublished(published)] => Some(published.metadata()),
    _ => None,
  }
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

fn artifact_error_code(error: &ArtifactWriteError) -> ErrorCode {
  match error {
    ArtifactWriteError::Rejected(code)
    | ArtifactWriteError::Integrity(code)
    | ArtifactWriteError::Unavailable(code)
    | ArtifactWriteError::PublicationUnknown(code) => code.clone(),
    ArtifactWriteError::AuthorityMismatch { .. } => {
      ErrorCode::parse("auv.dispatch.authority_mismatch").expect("static dispatch error code is valid")
    }
    ArtifactWriteError::IdempotencyMismatch => {
      ErrorCode::parse("auv.dispatch.idempotency_mismatch").expect("static dispatch error code is valid")
    }
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
