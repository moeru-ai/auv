use std::cell::RefCell;
use std::num::NonZeroUsize;
use std::sync::{Arc, mpsc};
use std::time::{SystemTime, UNIX_EPOCH};

use futures_channel::oneshot;

use crate::artifact::DetachedArtifact;
use crate::{ArtifactEmission, ArtifactRequest, DispatchErrorReporter, ErrorCode, StoreError, TraceExporter, TraceRecord, TracingStore};

/// Identifies the observational destination that failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchStage {
  Store,
  Export,
  Flush,
}

/// One asynchronous tracing failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DispatchFailure {
  stage: DispatchStage,
  code: ErrorCode,
}
impl DispatchFailure {
  pub fn stage(&self) -> DispatchStage {
    self.stage
  }
  pub fn code(&self) -> &ErrorCode {
    &self.code
  }
}

/// Failures completed before one flush barrier.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{} tracing dispatch failure(s)", .failures.len())]
pub struct FlushError {
  failures: Vec<DispatchFailure>,
}
impl FlushError {
  pub fn failure_count(&self) -> NonZeroUsize {
    NonZeroUsize::new(self.failures.len()).expect("flush errors are non-empty")
  }
  pub fn first(&self) -> &DispatchFailure {
    &self.failures[0]
  }
  pub fn failures(&self) -> &[DispatchFailure] {
    &self.failures
  }
}

/// Reports invalid dispatch construction.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BuildError {
  #[error("a dispatch accepts at most one tracing store")]
  MultipleTracingStores,
  #[error("failed to start the tracing worker")]
  WorkerSpawn,
}

#[derive(Default)]
pub struct DispatchBuilder {
  store: Option<Arc<dyn TracingStore>>,
  duplicate_store: bool,
  exporters: Vec<Arc<dyn TraceExporter>>,
  reporter: Option<Arc<dyn DispatchErrorReporter>>,
}

pub fn configure() -> DispatchBuilder {
  DispatchBuilder::default()
}

impl DispatchBuilder {
  pub fn tracing_store(mut self, store: Arc<dyn TracingStore>) -> Self {
    if self.store.replace(store).is_some() {
      self.duplicate_store = true;
    }
    self
  }
  pub fn exporter(mut self, exporter: Arc<dyn TraceExporter>) -> Self {
    self.exporters.push(exporter);
    self
  }
  pub fn on_error(mut self, reporter: Arc<dyn DispatchErrorReporter>) -> Self {
    self.reporter = Some(reporter);
    self
  }

  pub fn build(self) -> Result<Dispatch, BuildError> {
    if self.duplicate_store {
      return Err(BuildError::MultipleTracingStores);
    }
    if self.store.is_none() && self.exporters.is_empty() {
      return Ok(Dispatch {
        sender: None,
        has_store: false,
      });
    }
    let has_store = self.store.is_some();
    let (sender, receiver) = mpsc::channel();
    let worker = Worker {
      store: self.store,
      exporters: self.exporters,
      reporter: self.reporter.unwrap_or_else(|| Arc::new(DiscardReporter)),
      failures: Vec::new(),
    };
    std::thread::Builder::new().name("auv-tracing".into()).spawn(move || worker.run(receiver)).map_err(|_| BuildError::WorkerSpawn)?;
    Ok(Dispatch {
      sender: Some(sender),
      has_store,
    })
  }
}

/// Cloneable producer-side handle to one ordered tracing pipeline.
#[derive(Clone)]
pub struct Dispatch {
  sender: Option<mpsc::Sender<Work>>,
  has_store: bool,
}
impl Dispatch {
  pub(crate) fn is_enabled(&self) -> bool {
    self.sender.is_some()
  }
  pub(crate) fn can_write_artifacts(&self) -> bool {
    self.has_store
  }

  pub(crate) fn submit(&self, record: TraceRecord) {
    if let Some(sender) = &self.sender {
      let _ = sender.send(Work::Record(record));
    }
  }

  pub(crate) fn submit_artifact(
    &self,
    run_id: crate::RunId,
    span_id: Option<crate::SpanId>,
    artifact: DetachedArtifact,
  ) -> ArtifactEmission {
    let Some(sender) = &self.sender else {
      return ArtifactEmission::disabled();
    };
    let request = ArtifactRequest::new(
      run_id,
      span_id,
      artifact.artifact_id,
      artifact.purpose,
      artifact.content_type,
      artifact.byte_length,
      artifact.sha256,
      artifact.attributes,
    );
    let (receipt, emission) = ArtifactEmission::pending();
    if sender
      .send(Work::Artifact {
        request,
        body: artifact.body,
        receipt,
      })
      .is_err()
    {
      return ArtifactEmission::disabled();
    }
    emission
  }

  pub async fn flush(&self) -> Result<(), FlushError> {
    let Some(sender) = &self.sender else {
      return Ok(());
    };
    let (reply, receiver) = oneshot::channel();
    if sender.send(Work::Flush(reply)).is_err() {
      return Err(FlushError {
        failures: vec![failure(DispatchStage::Flush, "auv.tracing.worker_closed")],
      });
    }
    receiver.await.unwrap_or_else(|_| {
      Err(FlushError {
        failures: vec![failure(DispatchStage::Flush, "auv.tracing.worker_closed")],
      })
    })
  }
}

enum Work {
  Record(TraceRecord),
  Artifact {
    request: ArtifactRequest,
    body: crate::ArtifactBody,
    receipt: oneshot::Sender<Result<crate::ArtifactMetadata, StoreError>>,
  },
  Flush(oneshot::Sender<Result<(), FlushError>>),
}

struct Worker {
  store: Option<Arc<dyn TracingStore>>,
  exporters: Vec<Arc<dyn TraceExporter>>,
  reporter: Arc<dyn DispatchErrorReporter>,
  failures: Vec<DispatchFailure>,
}
impl Worker {
  fn run(mut self, receiver: mpsc::Receiver<Work>) {
    while let Ok(work) = receiver.recv() {
      match work {
        Work::Record(record) => self.record(record),
        Work::Artifact {
          request,
          body,
          receipt,
        } => self.artifact(request, body, receipt),
        Work::Flush(reply) => {
          let _ = reply.send(self.flush());
        }
      }
    }
  }

  fn record(&mut self, record: TraceRecord) {
    if let Some(store) = &self.store
      && let Err(error) = futures_executor::block_on(store.write(record.clone()))
    {
      self.retain(DispatchStage::Store, error.code().clone());
    }
    for exporter in self.exporters.clone() {
      if let Err(error) = futures_executor::block_on(exporter.export(record.clone())) {
        self.retain(DispatchStage::Export, error.code().clone());
      }
    }
  }

  fn artifact(
    &mut self,
    request: ArtifactRequest,
    body: crate::ArtifactBody,
    receipt: oneshot::Sender<Result<crate::ArtifactMetadata, StoreError>>,
  ) {
    let Some(store) = &self.store else {
      let error = crate::store::store_error("auv.tracing.artifacts_require_store");
      self.retain(DispatchStage::Store, error.code().clone());
      let _ = receipt.send(Err(error));
      return;
    };
    match futures_executor::block_on(store.write_artifact(request.clone(), body)) {
      Ok(metadata) => {
        self.record(TraceRecord::Artifact {
          run_id: request.run_id(),
          span_id: request.span_id(),
          metadata: metadata.clone(),
        });
        let _ = receipt.send(Ok(metadata));
      }
      Err(error) => {
        self.retain(DispatchStage::Store, error.code().clone());
        let _ = receipt.send(Err(error));
      }
    }
  }

  fn flush(&mut self) -> Result<(), FlushError> {
    if let Some(store) = &self.store
      && let Err(error) = futures_executor::block_on(store.flush())
    {
      self.retain(DispatchStage::Flush, error.code().clone());
    }
    for exporter in self.exporters.clone() {
      if let Err(error) = futures_executor::block_on(exporter.flush()) {
        self.retain(DispatchStage::Flush, error.code().clone());
      }
    }
    if self.failures.is_empty() {
      Ok(())
    } else {
      Err(FlushError {
        failures: std::mem::take(&mut self.failures),
      })
    }
  }

  fn retain(&mut self, stage: DispatchStage, code: ErrorCode) {
    let item = DispatchFailure { stage, code };
    self.reporter.report(&item);
    self.failures.push(item);
  }
}

fn failure(stage: DispatchStage, code: &'static str) -> DispatchFailure {
  DispatchFailure {
    stage,
    code: ErrorCode::parse(code).expect("static dispatch code"),
  }
}

struct DiscardReporter;
impl DispatchErrorReporter for DiscardReporter {
  fn report(&self, _: &DispatchFailure) {}
}

pub(crate) fn timestamp_now() -> Result<crate::Timestamp, ErrorCode> {
  let duration = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| ErrorCode::parse("auv.tracing.clock_before_epoch").unwrap())?;
  crate::Timestamp::new(duration.as_secs() as i64, duration.subsec_nanos())
    .map_err(|_| ErrorCode::parse("auv.tracing.clock_out_of_range").unwrap())
}

pub mod dispatcher {
  use super::*;
  thread_local! { static DEFAULTS: RefCell<Vec<Dispatch>> = const { RefCell::new(Vec::new()) }; }
  pub fn current() -> Option<Dispatch> {
    DEFAULTS.try_with(|items| items.borrow().last().cloned()).ok().flatten()
  }
  pub fn with_default<T>(dispatch: &Dispatch, operation: impl FnOnce() -> T) -> T {
    DEFAULTS.with(|items| items.borrow_mut().push(dispatch.clone()));
    struct Guard;
    impl Drop for Guard {
      fn drop(&mut self) {
        let _ = DEFAULTS.try_with(|items| items.borrow_mut().pop());
      }
    }
    let _guard = Guard;
    operation()
  }
}
