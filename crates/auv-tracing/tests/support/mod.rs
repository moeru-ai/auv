#![allow(dead_code)]

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use auv_tracing::{
  ArtifactBody, ArtifactReadError, ArtifactReader, ArtifactUri, ArtifactWriteError, AuthorityId, BoxFuture, CommitError, Dispatch,
  DispatchTask, ErrorCode, IdempotencyKey, MemoryRunStore, PageLimit, ReadError, RunCommit, RunCommitPage, RunCommitRequest, RunId,
  RunRevision, RunSnapshot, RunStore, RunSubscription, StoreArtifactRequest, SubscriptionError, TaskSpawnError, TaskSpawner,
};
use futures_channel::oneshot;

pub const WAIT_TIMEOUT: Duration = Duration::from_secs(5);

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
