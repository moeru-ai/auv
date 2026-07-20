#![allow(dead_code)]

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use auv_tracing::{
  ArtifactBody, ArtifactReadError, ArtifactReader, ArtifactUri, ArtifactWriteError, AuthorityId, BoxFuture, CommitError, Dispatch,
  DispatchErrorReporter, DispatchFailure, DispatchTask, ErrorCode, IdempotencyKey, MemoryRunStore, PageLimit, ReadError, RunCommit,
  RunCommitPage, RunCommitRequest, RunFact, RunId, RunMutation, RunRevision, RunSnapshot, RunStore, RunSubscription, StoreArtifactRequest,
  SubscriptionError, TaskSpawnError, TaskSpawner, TelemetryError, TelemetryItem, TelemetryProjector, Timestamp,
};
use futures_channel::oneshot;
use futures_util::StreamExt;

pub const WAIT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectorCall {
  Project,
  Flush,
}

#[derive(Clone, Default)]
pub struct RecordingProjector {
  state: Arc<RecordingProjectorState>,
}

#[derive(Default)]
struct RecordingProjectorState {
  calls: Mutex<Vec<ProjectorCall>>,
  items: Mutex<Vec<TelemetryItem>>,
  project_failures: Mutex<VecDeque<ErrorCode>>,
  flush_failures: Mutex<VecDeque<ErrorCode>>,
}

impl RecordingProjector {
  pub fn new() -> Arc<Self> {
    Arc::new(Self::default())
  }

  pub fn fail_next_project(&self, code: ErrorCode) {
    self.state.project_failures.lock().unwrap().push_back(code);
  }

  pub fn fail_next_flush(&self, code: ErrorCode) {
    self.state.flush_failures.lock().unwrap().push_back(code);
  }

  pub fn calls(&self) -> Vec<ProjectorCall> {
    self.state.calls.lock().unwrap().clone()
  }

  pub fn items(&self) -> Vec<TelemetryItem> {
    self.state.items.lock().unwrap().clone()
  }

  pub fn item_count(&self) -> usize {
    self.state.items.lock().unwrap().len()
  }
}

impl TelemetryProjector for RecordingProjector {
  fn project(&self, item: TelemetryItem) -> BoxFuture<'_, Result<(), TelemetryError>> {
    Box::pin(async move {
      self.state.calls.lock().unwrap().push(ProjectorCall::Project);
      self.state.items.lock().unwrap().push(item);
      match self.state.project_failures.lock().unwrap().pop_front() {
        Some(code) => Err(TelemetryError::new(code)),
        None => Ok(()),
      }
    })
  }

  fn flush(&self) -> BoxFuture<'_, Result<(), TelemetryError>> {
    Box::pin(async move {
      self.state.calls.lock().unwrap().push(ProjectorCall::Flush);
      match self.state.flush_failures.lock().unwrap().pop_front() {
        Some(code) => Err(TelemetryError::new(code)),
        None => Ok(()),
      }
    })
  }
}

#[derive(Clone)]
pub struct BlockingProjector {
  state: Arc<BlockingProjectorState>,
}

struct BlockingProjectorState {
  project_entered: Mutex<bool>,
  project_entered_changed: Condvar,
  project_release: Mutex<Option<oneshot::Sender<()>>>,
  project_receiver: Mutex<Option<oneshot::Receiver<()>>>,
  flush_entered: Mutex<bool>,
  flush_entered_changed: Condvar,
  flush_release: Mutex<Option<oneshot::Sender<()>>>,
  flush_receiver: Mutex<Option<oneshot::Receiver<()>>>,
  calls: Mutex<Vec<ProjectorCall>>,
  items: Mutex<Vec<TelemetryItem>>,
}

impl BlockingProjector {
  pub fn new() -> Arc<Self> {
    let (project_release, project_receiver) = oneshot::channel();
    let (flush_release, flush_receiver) = oneshot::channel();
    Arc::new(Self {
      state: Arc::new(BlockingProjectorState {
        project_entered: Mutex::new(false),
        project_entered_changed: Condvar::new(),
        project_release: Mutex::new(Some(project_release)),
        project_receiver: Mutex::new(Some(project_receiver)),
        flush_entered: Mutex::new(false),
        flush_entered_changed: Condvar::new(),
        flush_release: Mutex::new(Some(flush_release)),
        flush_receiver: Mutex::new(Some(flush_receiver)),
        calls: Mutex::new(Vec::new()),
        items: Mutex::new(Vec::new()),
      }),
    })
  }

  pub fn wait_until_project_entered(&self) {
    wait_for_flag(&self.state.project_entered, &self.state.project_entered_changed, "project call");
  }

  pub fn wait_until_flush_entered(&self) {
    wait_for_flag(&self.state.flush_entered, &self.state.flush_entered_changed, "projector flush");
  }

  pub fn release_project(&self) {
    if let Some(release) = self.state.project_release.lock().unwrap().take() {
      let _ = release.send(());
    }
  }

  pub fn release_flush(&self) {
    if let Some(release) = self.state.flush_release.lock().unwrap().take() {
      let _ = release.send(());
    }
  }

  pub fn calls(&self) -> Vec<ProjectorCall> {
    self.state.calls.lock().unwrap().clone()
  }

  pub fn item_count(&self) -> usize {
    self.state.items.lock().unwrap().len()
  }
}

impl TelemetryProjector for BlockingProjector {
  fn project(&self, item: TelemetryItem) -> BoxFuture<'_, Result<(), TelemetryError>> {
    let receiver = self.state.project_receiver.lock().unwrap().take();
    Box::pin(async move {
      if let Some(receiver) = receiver {
        *self.state.project_entered.lock().unwrap() = true;
        self.state.project_entered_changed.notify_all();
        let _ = receiver.await;
      }
      self.state.items.lock().unwrap().push(item);
      self.state.calls.lock().unwrap().push(ProjectorCall::Project);
      Ok(())
    })
  }

  fn flush(&self) -> BoxFuture<'_, Result<(), TelemetryError>> {
    let receiver = self.state.flush_receiver.lock().unwrap().take().expect("blocking projector accepts one flush call");
    Box::pin(async move {
      *self.state.flush_entered.lock().unwrap() = true;
      self.state.flush_entered_changed.notify_all();
      let _ = receiver.await;
      self.state.calls.lock().unwrap().push(ProjectorCall::Flush);
      Ok(())
    })
  }
}

fn wait_for_flag(flag: &Mutex<bool>, changed: &Condvar, operation: &str) {
  let deadline = Instant::now() + WAIT_TIMEOUT;
  let mut entered = flag.lock().unwrap();
  while !*entered {
    let remaining = deadline.checked_duration_since(Instant::now()).unwrap_or_else(|| panic!("timed out waiting for {operation}"));
    let (next, timeout) = changed.wait_timeout(entered, remaining).unwrap();
    entered = next;
    assert!(!timeout.timed_out() || *entered, "timed out waiting for {operation}");
  }
}

#[derive(Clone, Default)]
pub struct RecordingReporter {
  failures: Arc<Mutex<Vec<DispatchFailure>>>,
}

impl RecordingReporter {
  pub fn new() -> Arc<Self> {
    Arc::new(Self::default())
  }

  pub fn failures(&self) -> Vec<DispatchFailure> {
    self.failures.lock().unwrap().clone()
  }
}

impl DispatchErrorReporter for RecordingReporter {
  fn report(&self, failure: &DispatchFailure) {
    self.failures.lock().unwrap().push(failure.clone());
  }
}

struct ChannelWake(SyncSender<()>);

impl futures_util::task::ArcWake for ChannelWake {
  fn wake_by_ref(wake: &Arc<Self>) {
    let _ = wake.0.try_send(());
  }
}

pub fn block_on_timeout<F: Future>(future: F) -> F::Output {
  let deadline = Instant::now() + WAIT_TIMEOUT;
  let (sender, receiver) = sync_channel(1);
  let wake = Arc::new(ChannelWake(sender));
  let waker = futures_util::task::waker_ref(&wake);
  let mut context = Context::from_waker(&waker);
  futures_util::pin_mut!(future);

  loop {
    match future.as_mut().poll(&mut context) {
      Poll::Ready(output) => return output,
      Poll::Pending => {
        let remaining = deadline.checked_duration_since(Instant::now()).expect("timed out waiting for test future completion");
        receiver.recv_timeout(remaining).expect("timed out waiting for test future wakeup");
      }
    }
  }
}

pub struct TestDispatch<S = MemoryRunStore> {
  pub dispatch: Dispatch,
  pub store: Arc<S>,
}

impl TestDispatch {
  pub fn memory() -> Self {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = auv_tracing::configure().run_store(store.clone()).build().unwrap();
    Self { dispatch, store }
  }

  pub fn snapshot(&self, run_id: RunId) -> Option<RunSnapshot> {
    block_on_timeout(self.store.load_snapshot(run_id)).unwrap()
  }

  pub fn span_end_count(&self, run_id: RunId) -> usize {
    self.snapshot(run_id).map(|snapshot| snapshot.spans().values().filter(|span| span.ended().is_some()).count()).unwrap_or_default()
  }
}

impl<S> TestDispatch<S>
where
  S: RunStore + 'static,
{
  pub fn with_store(store: Arc<S>) -> Self {
    let dispatch = auv_tracing::configure().run_store(store.clone()).build().unwrap();
    Self { dispatch, store }
  }

  pub fn with_store_and_spawner(store: Arc<S>, spawner: Arc<dyn TaskSpawner>) -> Self {
    let dispatch = auv_tracing::configure().run_store(store.clone()).task_spawner(spawner).build().unwrap();
    Self { dispatch, store }
  }

  pub fn context(&self, run_id: RunId) -> auv_tracing::Context {
    auv_tracing::dispatcher::with_default(&self.dispatch, || auv_tracing::Context::root(run_id))
  }

  pub fn root(&self) -> auv_tracing::Context {
    self.context(RunId::new())
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthorityCall {
  LoadSnapshot,
  Subscribe,
  Commit,
  CommitsAfter,
}

#[derive(Clone, Copy)]
enum FirstSubscription {
  Normal,
  Gap,
  Pending,
}

#[derive(Clone)]
pub struct CursorStore {
  inner: Arc<MemoryRunStore>,
  first_subscription: FirstSubscription,
  fail_snapshot: bool,
  fail_page: Arc<AtomicBool>,
  subscribe_calls: Arc<AtomicUsize>,
  commits_after_calls: Arc<AtomicUsize>,
  calls: Arc<Mutex<Vec<AuthorityCall>>>,
}

impl CursorStore {
  pub fn normal() -> Arc<Self> {
    Self::new(FirstSubscription::Normal, false, false)
  }

  pub fn gap_once() -> Arc<Self> {
    Self::new(FirstSubscription::Gap, false, false)
  }

  pub fn pending_once() -> Arc<Self> {
    Self::new(FirstSubscription::Pending, false, false)
  }

  pub fn snapshot_failure() -> Arc<Self> {
    Self::new(FirstSubscription::Normal, true, false)
  }

  pub fn page_failure() -> Arc<Self> {
    Self::new(FirstSubscription::Pending, false, true)
  }

  fn new(first_subscription: FirstSubscription, fail_snapshot: bool, fail_page: bool) -> Arc<Self> {
    Arc::new(Self {
      inner: Arc::new(MemoryRunStore::new(AuthorityId::new())),
      first_subscription,
      fail_snapshot,
      fail_page: Arc::new(AtomicBool::new(fail_page)),
      subscribe_calls: Arc::new(AtomicUsize::new(0)),
      commits_after_calls: Arc::new(AtomicUsize::new(0)),
      calls: Arc::new(Mutex::new(Vec::new())),
    })
  }

  pub fn calls(&self) -> Vec<AuthorityCall> {
    self.calls.lock().unwrap().clone()
  }

  pub fn subscribe_call_count(&self) -> usize {
    self.subscribe_calls.load(Ordering::SeqCst)
  }

  pub fn commits_after_call_count(&self) -> usize {
    self.commits_after_calls.load(Ordering::SeqCst)
  }
}

impl RunStore for CursorStore {
  fn authority_id(&self) -> AuthorityId {
    self.inner.authority_id()
  }

  fn commit(&self, request: RunCommitRequest) -> BoxFuture<'_, Result<RunCommit, CommitError>> {
    self.calls.lock().unwrap().push(AuthorityCall::Commit);
    self.inner.commit(request)
  }

  fn write_artifact(&self, request: StoreArtifactRequest, body: ArtifactBody) -> BoxFuture<'_, Result<RunCommit, ArtifactWriteError>> {
    self.inner.write_artifact(request, body)
  }

  fn lookup_commit(&self, run_id: RunId, key: IdempotencyKey) -> BoxFuture<'_, Result<Option<RunCommit>, ReadError>> {
    self.inner.lookup_commit(run_id, key)
  }

  fn load_snapshot(&self, run_id: RunId) -> BoxFuture<'_, Result<Option<RunSnapshot>, ReadError>> {
    self.calls.lock().unwrap().push(AuthorityCall::LoadSnapshot);
    if self.fail_snapshot {
      return Box::pin(async { Err(ReadError::Unavailable(ErrorCode::parse("auv.test.snapshot_failed").unwrap())) });
    }
    self.inner.load_snapshot(run_id)
  }

  fn commits_after(&self, run_id: RunId, after: RunRevision, limit: PageLimit) -> BoxFuture<'_, Result<RunCommitPage, ReadError>> {
    self.calls.lock().unwrap().push(AuthorityCall::CommitsAfter);
    self.commits_after_calls.fetch_add(1, Ordering::SeqCst);
    if self.fail_page.swap(false, Ordering::SeqCst) {
      return Box::pin(async { Err(ReadError::Unavailable(ErrorCode::parse("auv.test.page_failed").unwrap())) });
    }
    self.inner.commits_after(run_id, after, limit)
  }

  fn subscribe(&self, run_id: RunId, after: RunRevision) -> BoxFuture<'_, Result<RunSubscription, ReadError>> {
    self.calls.lock().unwrap().push(AuthorityCall::Subscribe);
    let call = self.subscribe_calls.fetch_add(1, Ordering::SeqCst);
    Box::pin(async move {
      if call == 0 {
        match self.first_subscription {
          FirstSubscription::Normal => {}
          FirstSubscription::Gap => {
            let earliest_available = RunRevision::new(after.get() + 1).unwrap();
            let stream = futures_util::stream::once(async move {
              Err(SubscriptionError::Gap {
                requested_after: after,
                earliest_available,
              })
            })
            .chain(futures_util::stream::pending());
            return Ok(Box::pin(stream) as RunSubscription);
          }
          FirstSubscription::Pending => return Ok(Box::pin(futures_util::stream::pending()) as RunSubscription),
        }
      }
      self.inner.subscribe(run_id, after).await
    })
  }

  fn open_artifact(&self, uri: ArtifactUri) -> BoxFuture<'_, Result<ArtifactReader, ReadError>> {
    self.inner.open_artifact(uri)
  }
}

#[derive(Clone, Copy)]
pub enum UnknownLookup {
  Committed,
  None,
  ReadFailure,
  Mismatch,
}

#[derive(Clone)]
pub struct CommitUnknownStore {
  inner: Arc<MemoryRunStore>,
  mode: UnknownLookup,
  first: Arc<AtomicBool>,
  commit_calls: Arc<AtomicUsize>,
  lookup_calls: Arc<AtomicUsize>,
  first_request: Arc<Mutex<Option<RunCommitRequest>>>,
}

impl CommitUnknownStore {
  pub fn new(mode: UnknownLookup) -> Arc<Self> {
    Arc::new(Self {
      inner: Arc::new(MemoryRunStore::new(AuthorityId::new())),
      mode,
      first: Arc::new(AtomicBool::new(true)),
      commit_calls: Arc::new(AtomicUsize::new(0)),
      lookup_calls: Arc::new(AtomicUsize::new(0)),
      first_request: Arc::new(Mutex::new(None)),
    })
  }

  pub fn commit_calls(&self) -> usize {
    self.commit_calls.load(Ordering::SeqCst)
  }

  pub fn lookup_calls(&self) -> usize {
    self.lookup_calls.load(Ordering::SeqCst)
  }
}

impl RunStore for CommitUnknownStore {
  fn authority_id(&self) -> AuthorityId {
    self.inner.authority_id()
  }

  fn commit(&self, request: RunCommitRequest) -> BoxFuture<'_, Result<RunCommit, CommitError>> {
    self.commit_calls.fetch_add(1, Ordering::SeqCst);
    let unknown = self.first.swap(false, Ordering::SeqCst);
    if unknown {
      *self.first_request.lock().unwrap() = Some(request.clone());
    }
    Box::pin(async move {
      if !unknown {
        return self.inner.commit(request).await;
      }
      if matches!(self.mode, UnknownLookup::Committed) {
        self.inner.commit(request).await?;
      }
      Err(CommitError::CommitUnknown(ErrorCode::parse("auv.test.commit_unknown").unwrap()))
    })
  }

  fn write_artifact(&self, request: StoreArtifactRequest, body: ArtifactBody) -> BoxFuture<'_, Result<RunCommit, ArtifactWriteError>> {
    self.inner.write_artifact(request, body)
  }

  fn lookup_commit(&self, run_id: RunId, key: IdempotencyKey) -> BoxFuture<'_, Result<Option<RunCommit>, ReadError>> {
    self.lookup_calls.fetch_add(1, Ordering::SeqCst);
    Box::pin(async move {
      match self.mode {
        UnknownLookup::Committed => self.inner.lookup_commit(run_id, key).await,
        UnknownLookup::None => Ok(None),
        UnknownLookup::ReadFailure => Err(ReadError::Unavailable(ErrorCode::parse("auv.test.lookup_failed").unwrap())),
        UnknownLookup::Mismatch => {
          let request = self.first_request.lock().unwrap().clone().expect("unknown commit saved its request");
          let facts = request
            .mutations()
            .iter()
            .cloned()
            .map(|mutation| match mutation {
              RunMutation::StartSpan(value) => RunFact::SpanStarted(value),
              RunMutation::EndSpan(value) => RunFact::SpanEnded(value),
              RunMutation::EmitEvent(value) => RunFact::EventOccurred(auv_tracing::EventOccurred::new(
                value.event_id(),
                value.span_id(),
                value.occurred_at(),
                value.schema().clone(),
                auv_tracing::JsonPayload::encode(&serde_json::json!({ "mismatch": true })).unwrap(),
              )),
            })
            .collect();
          let mismatch = RunCommit::new(
            request.authority_id(),
            request.run_id(),
            RunRevision::new(1).unwrap(),
            request.idempotency_key(),
            Timestamp::new(1, 0).unwrap(),
            facts,
          )
          .unwrap();
          Ok(Some(mismatch))
        }
      }
    })
  }

  fn load_snapshot(&self, run_id: RunId) -> BoxFuture<'_, Result<Option<RunSnapshot>, ReadError>> {
    self.inner.load_snapshot(run_id)
  }

  fn commits_after(&self, run_id: RunId, after: RunRevision, limit: PageLimit) -> BoxFuture<'_, Result<RunCommitPage, ReadError>> {
    self.inner.commits_after(run_id, after, limit)
  }

  fn subscribe(&self, run_id: RunId, after: RunRevision) -> BoxFuture<'_, Result<RunSubscription, ReadError>> {
    self.inner.subscribe(run_id, after)
  }

  fn open_artifact(&self, uri: ArtifactUri) -> BoxFuture<'_, Result<ArtifactReader, ReadError>> {
    self.inner.open_artifact(uri)
  }
}

#[derive(Clone)]
pub struct CommitGate {
  state: Arc<GateState>,
}

struct GateState {
  entered: Mutex<bool>,
  entered_changed: Condvar,
  release: Mutex<Option<oneshot::Sender<()>>>,
  receiver: Mutex<Option<oneshot::Receiver<()>>>,
}

impl CommitGate {
  fn new() -> Self {
    let (release, receiver) = oneshot::channel();
    Self {
      state: Arc::new(GateState {
        entered: Mutex::new(false),
        entered_changed: Condvar::new(),
        release: Mutex::new(Some(release)),
        receiver: Mutex::new(Some(receiver)),
      }),
    }
  }

  fn enter(&self) -> Option<oneshot::Receiver<()>> {
    let receiver = self.state.receiver.lock().unwrap().take()?;
    *self.state.entered.lock().unwrap() = true;
    self.state.entered_changed.notify_all();
    Some(receiver)
  }

  pub fn wait_until_entered(&self) {
    let deadline = Instant::now() + WAIT_TIMEOUT;
    let mut entered = self.state.entered.lock().unwrap();
    while !*entered {
      let remaining = deadline.checked_duration_since(Instant::now()).expect("timed out waiting for controlled commit to start");
      let (next, timeout) = self.state.entered_changed.wait_timeout(entered, remaining).unwrap();
      entered = next;
      assert!(!timeout.timed_out() || *entered, "timed out waiting for controlled commit to start");
    }
  }

  pub fn release(&self) {
    if let Some(release) = self.state.release.lock().unwrap().take() {
      let _ = release.send(());
    }
  }
}

#[derive(Clone)]
pub struct WorkerPanicGate {
  state: Arc<WorkerPanicGateState>,
}

struct WorkerPanicGateState {
  status: Mutex<WorkerPanicGateStatus>,
  changed: Condvar,
}

struct WorkerPanicGateStatus {
  entered: bool,
  released: bool,
}

impl WorkerPanicGate {
  fn new() -> Self {
    Self {
      state: Arc::new(WorkerPanicGateState {
        status: Mutex::new(WorkerPanicGateStatus {
          entered: false,
          released: false,
        }),
        changed: Condvar::new(),
      }),
    }
  }

  fn enter_and_wait(&self) {
    let deadline = Instant::now() + WAIT_TIMEOUT;
    let mut status = self.state.status.lock().unwrap();
    status.entered = true;
    self.state.changed.notify_all();
    while !status.released {
      let remaining = deadline.checked_duration_since(Instant::now()).expect("timed out holding a default worker at its panic boundary");
      let (next, timeout) = self.state.changed.wait_timeout(status, remaining).unwrap();
      status = next;
      assert!(!timeout.timed_out() || status.released, "timed out holding a default worker at its panic boundary");
    }
  }

  pub fn wait_until_entered(&self) {
    let deadline = Instant::now() + WAIT_TIMEOUT;
    let mut status = self.state.status.lock().unwrap();
    while !status.entered {
      let remaining =
        deadline.checked_duration_since(Instant::now()).expect("timed out waiting for a default worker to reach its panic boundary");
      let (next, timeout) = self.state.changed.wait_timeout(status, remaining).unwrap();
      status = next;
      assert!(!timeout.timed_out() || status.entered, "timed out waiting for a default worker to reach its panic boundary");
    }
  }

  pub fn release(&self) {
    let mut status = self.state.status.lock().unwrap();
    status.released = true;
    self.state.changed.notify_all();
  }
}

#[derive(Clone)]
pub struct ControlledStore {
  inner: Arc<MemoryRunStore>,
  control: Arc<ControlState>,
}

struct ControlState {
  controls: Mutex<HashMap<RunId, VecDeque<CommitControl>>>,
  first_control: Mutex<Option<CommitControl>>,
  calls: Mutex<Vec<RunCommitRequest>>,
  calls_changed: Condvar,
  committed: Mutex<Vec<(RunCommitRequest, RunCommit)>>,
  committed_changed: Condvar,
}

struct CommitControl {
  gate: CommitGate,
  outcome: CommitOutcome,
}

enum CommitOutcome {
  Succeed,
  Fail(ErrorCode),
  Panic(Option<WorkerPanicGate>),
}

impl ControlledStore {
  pub fn new() -> Arc<Self> {
    Arc::new(Self {
      inner: Arc::new(MemoryRunStore::new(AuthorityId::new())),
      control: Arc::new(ControlState {
        controls: Mutex::new(HashMap::new()),
        first_control: Mutex::new(None),
        calls: Mutex::new(Vec::new()),
        calls_changed: Condvar::new(),
        committed: Mutex::new(Vec::new()),
        committed_changed: Condvar::new(),
      }),
    })
  }

  pub fn block_first_commit(&self) -> CommitGate {
    let gate = CommitGate::new();
    *self.control.first_control.lock().unwrap() = Some(CommitControl {
      gate: gate.clone(),
      outcome: CommitOutcome::Succeed,
    });
    gate
  }

  pub fn block_next_commit(&self, run_id: RunId) -> CommitGate {
    let gate = CommitGate::new();
    self.control.controls.lock().unwrap().entry(run_id).or_default().push_back(CommitControl {
      gate: gate.clone(),
      outcome: CommitOutcome::Succeed,
    });
    gate
  }

  pub fn fail_next_commit(&self, run_id: RunId, code: ErrorCode) -> CommitGate {
    let gate = CommitGate::new();
    self.control.controls.lock().unwrap().entry(run_id).or_default().push_back(CommitControl {
      gate: gate.clone(),
      outcome: CommitOutcome::Fail(code),
    });
    gate
  }

  pub fn panic_next_commit(&self, run_id: RunId) -> CommitGate {
    let gate = CommitGate::new();
    self.control.controls.lock().unwrap().entry(run_id).or_default().push_back(CommitControl {
      gate: gate.clone(),
      outcome: CommitOutcome::Panic(None),
    });
    gate
  }

  pub fn block_worker_before_panicking_next_commit(&self, run_id: RunId) -> (CommitGate, WorkerPanicGate) {
    let gate = CommitGate::new();
    let worker_gate = WorkerPanicGate::new();
    self.control.controls.lock().unwrap().entry(run_id).or_default().push_back(CommitControl {
      gate: gate.clone(),
      outcome: CommitOutcome::Panic(Some(worker_gate.clone())),
    });
    (gate, worker_gate)
  }

  pub fn commit_call_count(&self, run_id: RunId) -> usize {
    self.control.calls.lock().unwrap().iter().filter(|request| request.run_id() == run_id).count()
  }

  pub fn wait_for_commit_calls(&self, run_id: RunId, expected: usize) {
    let deadline = Instant::now() + WAIT_TIMEOUT;
    let mut calls = self.control.calls.lock().unwrap();
    while calls.iter().filter(|request| request.run_id() == run_id).count() < expected {
      let remaining = deadline.checked_duration_since(Instant::now()).expect("timed out waiting for controlled commit calls");
      let (next, timeout) = self.control.calls_changed.wait_timeout(calls, remaining).unwrap();
      calls = next;
      let count = calls.iter().filter(|request| request.run_id() == run_id).count();
      assert!(!timeout.timed_out() || count >= expected, "timed out waiting for {expected} commit calls for run {run_id}; observed {count}");
    }
  }

  pub fn wait_until_committed(&self, run_id: RunId) {
    let deadline = Instant::now() + WAIT_TIMEOUT;
    let mut committed = self.control.committed.lock().unwrap();
    while !committed.iter().any(|(request, _)| request.run_id() == run_id) {
      let remaining = deadline.checked_duration_since(Instant::now()).expect("timed out waiting for controlled commit completion");
      let (next, timeout) = self.control.committed_changed.wait_timeout(committed, remaining).unwrap();
      committed = next;
      let found = committed.iter().any(|(request, _)| request.run_id() == run_id);
      assert!(!timeout.timed_out() || found, "timed out waiting for a committed request for run {run_id}");
    }
  }

  pub fn wait_for_committed_count(&self, run_id: RunId, expected: usize) {
    let deadline = Instant::now() + WAIT_TIMEOUT;
    let mut committed = self.control.committed.lock().unwrap();
    while committed.iter().filter(|(request, _)| request.run_id() == run_id).count() < expected {
      let remaining = deadline.checked_duration_since(Instant::now()).expect("timed out waiting for controlled commit completions");
      let (next, timeout) = self.control.committed_changed.wait_timeout(committed, remaining).unwrap();
      committed = next;
      let count = committed.iter().filter(|(request, _)| request.run_id() == run_id).count();
      assert!(!timeout.timed_out() || count >= expected, "timed out waiting for {expected} committed requests; observed {count}");
    }
  }

  pub fn committed_requests(&self, run_id: RunId) -> Vec<RunCommitRequest> {
    self
      .control
      .committed
      .lock()
      .unwrap()
      .iter()
      .filter(|(request, _)| request.run_id() == run_id)
      .map(|(request, _)| request.clone())
      .collect()
  }

  pub fn committed_revisions(&self, run_id: RunId) -> Vec<u64> {
    self
      .control
      .committed
      .lock()
      .unwrap()
      .iter()
      .filter(|(request, _)| request.run_id() == run_id)
      .map(|(_, commit)| commit.revision().get())
      .collect()
  }

  fn control_for(&self, run_id: RunId) -> Option<CommitControl> {
    if let Some(control) = self.control.first_control.lock().unwrap().take() {
      return Some(control);
    }
    let mut controls = self.control.controls.lock().unwrap();
    let queue = controls.get_mut(&run_id)?;
    let control = queue.pop_front();
    if queue.is_empty() {
      controls.remove(&run_id);
    }
    control
  }
}

impl RunStore for ControlledStore {
  fn authority_id(&self) -> AuthorityId {
    self.inner.authority_id()
  }

  fn commit(&self, request: RunCommitRequest) -> BoxFuture<'_, Result<RunCommit, CommitError>> {
    self.control.calls.lock().unwrap().push(request.clone());
    self.control.calls_changed.notify_all();
    let control = self.control_for(request.run_id());
    let gate = control.as_ref().and_then(|control| control.gate.enter());
    let outcome = control.map(|control| control.outcome).unwrap_or(CommitOutcome::Succeed);
    Box::pin(async move {
      if let Some(gate) = gate {
        let _ = gate.await;
      }
      match outcome {
        CommitOutcome::Succeed => {}
        CommitOutcome::Fail(code) => return Err(CommitError::Rejected(code)),
        CommitOutcome::Panic(worker_gate) => {
          if let Some(worker_gate) = worker_gate {
            worker_gate.enter_and_wait();
          }
          panic!("controlled store commit future panicked");
        }
      }
      let commit = self.inner.commit(request.clone()).await?;
      self.control.committed.lock().unwrap().push((request, commit.clone()));
      self.control.committed_changed.notify_all();
      Ok(commit)
    })
  }

  fn write_artifact(&self, request: StoreArtifactRequest, body: ArtifactBody) -> BoxFuture<'_, Result<RunCommit, ArtifactWriteError>> {
    self.inner.write_artifact(request, body)
  }

  fn lookup_commit(&self, run_id: RunId, key: IdempotencyKey) -> BoxFuture<'_, Result<Option<RunCommit>, ReadError>> {
    self.inner.lookup_commit(run_id, key)
  }

  fn load_snapshot(&self, run_id: RunId) -> BoxFuture<'_, Result<Option<RunSnapshot>, ReadError>> {
    self.inner.load_snapshot(run_id)
  }

  fn commits_after(&self, run_id: RunId, after: RunRevision, limit: PageLimit) -> BoxFuture<'_, Result<RunCommitPage, ReadError>> {
    self.inner.commits_after(run_id, after, limit)
  }

  fn subscribe(&self, run_id: RunId, after: RunRevision) -> BoxFuture<'_, Result<RunSubscription, ReadError>> {
    self.inner.subscribe(run_id, after)
  }

  fn open_artifact(&self, uri: ArtifactUri) -> BoxFuture<'_, Result<ArtifactReader, ReadError>> {
    self.inner.open_artifact(uri)
  }
}

pub struct FailFirstSpawner {
  failed: Mutex<bool>,
}

impl FailFirstSpawner {
  pub fn new() -> Arc<Self> {
    Arc::new(Self {
      failed: Mutex::new(false),
    })
  }
}

impl TaskSpawner for FailFirstSpawner {
  fn spawn(&self, task: DispatchTask) -> Result<(), TaskSpawnError> {
    let mut failed = self.failed.lock().unwrap();
    if !*failed {
      *failed = true;
      return Err(TaskSpawnError::new(ErrorCode::parse("auv.dispatch.spawn").unwrap()));
    }
    std::thread::spawn(move || block_on_timeout(task));
    Ok(())
  }
}

pub struct InlineSpawner;

impl InlineSpawner {
  pub fn new() -> Arc<Self> {
    Arc::new(Self)
  }
}

impl TaskSpawner for InlineSpawner {
  fn spawn(&self, task: DispatchTask) -> Result<(), TaskSpawnError> {
    block_on_timeout(task);
    Ok(())
  }
}

#[derive(Default)]
pub struct ManualTaskSpawner {
  tasks: Mutex<VecDeque<DispatchTask>>,
}

impl ManualTaskSpawner {
  pub fn new() -> Arc<Self> {
    Arc::new(Self::default())
  }

  pub fn run_all(&self) {
    loop {
      let task = { self.tasks.lock().unwrap().pop_front() };
      let Some(task) = task else {
        return;
      };
      block_on_timeout(task);
    }
  }
}

impl TaskSpawner for ManualTaskSpawner {
  fn spawn(&self, task: DispatchTask) -> Result<(), TaskSpawnError> {
    self.tasks.lock().unwrap().push_back(task);
    Ok(())
  }
}

pub struct PanicFirstSpawner {
  first: AtomicBool,
}

impl PanicFirstSpawner {
  pub fn new() -> Arc<Self> {
    Arc::new(Self {
      first: AtomicBool::new(true),
    })
  }
}

impl TaskSpawner for PanicFirstSpawner {
  fn spawn(&self, task: DispatchTask) -> Result<(), TaskSpawnError> {
    if self.first.swap(false, Ordering::SeqCst) {
      panic!("test spawner panicked before polling the task");
    }
    block_on_timeout(task);
    Ok(())
  }
}

pub struct DropFirstTaskSpawner {
  first: AtomicBool,
}

pub struct DiscardFirstTaskSpawner {
  first: AtomicBool,
}

impl DiscardFirstTaskSpawner {
  pub fn new() -> Arc<Self> {
    Arc::new(Self {
      first: AtomicBool::new(true),
    })
  }
}

impl TaskSpawner for DiscardFirstTaskSpawner {
  fn spawn(&self, task: DispatchTask) -> Result<(), TaskSpawnError> {
    if self.first.swap(false, Ordering::SeqCst) {
      drop(task);
      return Ok(());
    }
    std::thread::spawn(move || block_on_timeout(task));
    Ok(())
  }
}

impl DropFirstTaskSpawner {
  pub fn new() -> Arc<Self> {
    Arc::new(Self {
      first: AtomicBool::new(true),
    })
  }
}

impl TaskSpawner for DropFirstTaskSpawner {
  fn spawn(&self, mut task: DispatchTask) -> Result<(), TaskSpawnError> {
    if self.first.swap(false, Ordering::SeqCst) {
      let waker = futures_util::task::noop_waker();
      let mut context = Context::from_waker(&waker);
      assert!(matches!(task.as_mut().poll(&mut context), Poll::Pending));
      drop(task);
      return Ok(());
    }
    block_on_timeout(task);
    Ok(())
  }
}

fn _assert_error_types(_: ArtifactReadError, _: SubscriptionError) {}
