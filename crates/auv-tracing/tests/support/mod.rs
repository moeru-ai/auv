#![allow(dead_code)]

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex};

use auv_tracing::{
  ArtifactBody, ArtifactReadError, ArtifactReader, ArtifactUri, ArtifactWriteError, AuthorityId, BoxFuture, CommitError, Dispatch,
  DispatchTask, ErrorCode, IdempotencyKey, MemoryRunStore, PageLimit, ReadError, RunCommit, RunCommitPage, RunCommitRequest, RunId,
  RunRevision, RunSnapshot, RunStore, RunSubscription, StoreArtifactRequest, SubscriptionError, TaskSpawnError, TaskSpawner,
};
use futures_channel::oneshot;

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
    let mut entered = self.state.entered.lock().unwrap();
    while !*entered {
      entered = self.state.entered_changed.wait(entered).unwrap();
    }
  }

  pub fn release(&self) {
    if let Some(release) = self.state.release.lock().unwrap().take() {
      let _ = release.send(());
    }
  }
}

#[derive(Clone)]
pub struct ControlledStore {
  inner: Arc<MemoryRunStore>,
  control: Arc<ControlState>,
}

struct ControlState {
  gates: Mutex<HashMap<RunId, VecDeque<CommitGate>>>,
  first_gate: Mutex<Option<CommitGate>>,
  calls: Mutex<Vec<RunCommitRequest>>,
  calls_changed: Condvar,
  committed: Mutex<Vec<(RunCommitRequest, RunCommit)>>,
  committed_changed: Condvar,
}

impl ControlledStore {
  pub fn new() -> Arc<Self> {
    Arc::new(Self {
      inner: Arc::new(MemoryRunStore::new(AuthorityId::new())),
      control: Arc::new(ControlState {
        gates: Mutex::new(HashMap::new()),
        first_gate: Mutex::new(None),
        calls: Mutex::new(Vec::new()),
        calls_changed: Condvar::new(),
        committed: Mutex::new(Vec::new()),
        committed_changed: Condvar::new(),
      }),
    })
  }

  pub fn block_first_commit(&self) -> CommitGate {
    let gate = CommitGate::new();
    *self.control.first_gate.lock().unwrap() = Some(gate.clone());
    gate
  }

  pub fn block_next_commit(&self, run_id: RunId) -> CommitGate {
    let gate = CommitGate::new();
    self.control.gates.lock().unwrap().entry(run_id).or_default().push_back(gate.clone());
    gate
  }

  pub fn commit_call_count(&self, run_id: RunId) -> usize {
    self.control.calls.lock().unwrap().iter().filter(|request| request.run_id() == run_id).count()
  }

  pub fn wait_for_commit_calls(&self, run_id: RunId, expected: usize) {
    let mut calls = self.control.calls.lock().unwrap();
    while calls.iter().filter(|request| request.run_id() == run_id).count() < expected {
      calls = self.control.calls_changed.wait(calls).unwrap();
    }
  }

  pub fn wait_until_committed(&self, run_id: RunId) {
    let mut committed = self.control.committed.lock().unwrap();
    while !committed.iter().any(|(request, _)| request.run_id() == run_id) {
      committed = self.control.committed_changed.wait(committed).unwrap();
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

  fn gate_for(&self, run_id: RunId) -> Option<CommitGate> {
    if let Some(gate) = self.control.first_gate.lock().unwrap().take() {
      return Some(gate);
    }
    let mut gates = self.control.gates.lock().unwrap();
    let queue = gates.get_mut(&run_id)?;
    let gate = queue.pop_front();
    if queue.is_empty() {
      gates.remove(&run_id);
    }
    gate
  }
}

impl RunStore for ControlledStore {
  fn authority_id(&self) -> AuthorityId {
    self.inner.authority_id()
  }

  fn commit(&self, request: RunCommitRequest) -> BoxFuture<'_, Result<RunCommit, CommitError>> {
    self.control.calls.lock().unwrap().push(request.clone());
    self.control.calls_changed.notify_all();
    let gate = self.gate_for(request.run_id()).and_then(|gate| gate.enter());
    Box::pin(async move {
      if let Some(gate) = gate {
        let _ = gate.await;
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
    std::thread::spawn(move || futures_executor::block_on(task));
    Ok(())
  }
}

fn _assert_error_types(_: ArtifactReadError, _: SubscriptionError) {}
