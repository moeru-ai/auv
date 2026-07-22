use std::future::Future;

use crate::{Context, Dispatch, FlushError, ReadError, RunId, RunSnapshot, dispatcher};

/// The direct value and committed V1 snapshot from one opt-in recorded call.
pub struct Recorded<T> {
  run_id: RunId,
  value: T,
  tracing_failure: Option<FlushError>,
  snapshot: RunSnapshot,
}

impl<T> Recorded<T> {
  /// Returns the run created for this recording scope.
  pub fn run_id(&self) -> RunId {
    self.run_id
  }

  /// Returns the direct value without replacing it with recording state.
  pub fn value(&self) -> &T {
    &self.value
  }

  /// Returns the committed authority snapshot through the flush barrier.
  pub fn snapshot(&self) -> &RunSnapshot {
    &self.snapshot
  }

  /// Returns a non-authoritative tracing failure, when instrumentation failed.
  pub fn tracing_failure(&self) -> Option<&FlushError> {
    self.tracing_failure.as_ref()
  }

  /// Splits the direct value from generic recording metadata.
  pub fn into_parts(self) -> (RunId, T, Option<FlushError>, RunSnapshot) {
    (self.run_id, self.value, self.tracing_failure, self.snapshot)
  }
}

/// Failure to establish or read the sole authority for a recorded call.
#[derive(Debug, thiserror::Error)]
pub enum RecordingError {
  /// Recording requires one configured run authority.
  #[error("recording requires one configured run store authority")]
  AuthorityUnavailable,
  /// The authority rejected or could not serve the post-flush snapshot read.
  #[error("failed to read recorded run {run_id}: {source}")]
  Read {
    run_id: RunId,
    #[source]
    source: ReadError,
  },
  /// The call emitted facts, but its configured authority did not retain them.
  #[error("recorded run {run_id} was not persisted by its V1 authority")]
  MissingSnapshot { run_id: RunId },
}

impl Dispatch {
  /// Records one caller-supplied future without selecting or dispatching domain work.
  ///
  /// The closure is constructed inside a fresh root context, the returned future
  /// is polled in that context on the caller's async task, and the sole configured
  /// authority is flushed and snapshotted before return.
  pub async fn record<T, F, Fut>(&self, call: F) -> Result<Recorded<T>, RecordingError>
  where
    F: FnOnce() -> Fut,
    Fut: Future<Output = T>,
  {
    let store = self.run_store().ok_or(RecordingError::AuthorityUnavailable)?;
    let run_id = RunId::new();
    let root = dispatcher::with_default(self, || Context::root(run_id));
    let future = root.in_scope(call);
    let value = root.instrument(future).await;
    let tracing_failure = self.flush().await.err();
    let snapshot = store
      .load_snapshot(run_id)
      .await
      .map_err(|source| RecordingError::Read { run_id, source })?
      .ok_or(RecordingError::MissingSnapshot { run_id })?;
    Ok(Recorded {
      run_id,
      value,
      tracing_failure,
      snapshot,
    })
  }
}
