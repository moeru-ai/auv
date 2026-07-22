use std::future::Future;

use crate::{Context, Dispatch, FlushError, ReadError, RunId, RunSnapshot, dispatcher};

/// The flush state observed after the direct call completed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordingFlush {
  /// All instrumentation submitted before the barrier was flushed.
  Complete,
  /// Instrumentation flushing failed without changing the direct call value.
  Failed(FlushError),
}

impl RecordingFlush {
  /// Returns the non-authoritative tracing failure, when flushing failed.
  pub fn failure(&self) -> Option<&FlushError> {
    match self {
      Self::Complete => None,
      Self::Failed(error) => Some(error),
    }
  }
}

impl std::fmt::Display for RecordingFlush {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Complete => formatter.write_str("flush completed"),
      Self::Failed(error) => write!(formatter, "flush failed: {error}"),
    }
  }
}

/// A committed authority snapshot and its non-authoritative flush metadata.
#[derive(Clone, Debug, PartialEq)]
pub struct CommittedRecording {
  snapshot: RunSnapshot,
  flush: RecordingFlush,
}

impl CommittedRecording {
  /// Returns the committed authority snapshot read after the flush barrier.
  pub fn snapshot(&self) -> &RunSnapshot {
    &self.snapshot
  }

  /// Returns the flush state paired with the committed snapshot.
  pub fn flush(&self) -> &RecordingFlush {
    &self.flush
  }

  /// Returns the non-authoritative tracing failure, when flushing failed.
  pub fn tracing_failure(&self) -> Option<&FlushError> {
    self.flush.failure()
  }
}

/// Typed recording failure discovered after the direct call completed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PostExecutionRecordingFailure {
  /// The authority rejected or could not serve the post-flush snapshot read.
  #[error("failed to read recorded run snapshot after execution ({flush}): {source}")]
  SnapshotRead {
    #[source]
    source: ReadError,
    flush: RecordingFlush,
  },
  /// The configured authority did not retain a snapshot for the completed call.
  #[error("recorded run snapshot is missing after execution ({flush})")]
  SnapshotMissing { flush: RecordingFlush },
}

impl PostExecutionRecordingFailure {
  /// Returns the flush state observed before the snapshot failure.
  pub fn flush(&self) -> &RecordingFlush {
    match self {
      Self::SnapshotRead { flush, .. } | Self::SnapshotMissing { flush } => flush,
    }
  }

  /// Returns the non-authoritative tracing failure, when flushing also failed.
  pub fn tracing_failure(&self) -> Option<&FlushError> {
    self.flush().failure()
  }
}

/// Post-execution recording state for one direct call.
#[derive(Clone, Debug, PartialEq)]
pub enum RecordingState {
  /// The authority served the committed snapshot for the call.
  Committed(CommittedRecording),
  /// Recording failed after the direct call value was already produced.
  Failed(PostExecutionRecordingFailure),
}

impl RecordingState {
  /// Returns the committed snapshot when the authority served it.
  pub fn snapshot(&self) -> Option<&RunSnapshot> {
    match self {
      Self::Committed(recording) => Some(recording.snapshot()),
      Self::Failed(_) => None,
    }
  }

  /// Returns the typed post-execution recording failure, when present.
  pub fn failure(&self) -> Option<&PostExecutionRecordingFailure> {
    match self {
      Self::Committed(_) => None,
      Self::Failed(failure) => Some(failure),
    }
  }

  /// Returns a non-authoritative flush failure from either recording branch.
  pub fn tracing_failure(&self) -> Option<&FlushError> {
    match self {
      Self::Committed(recording) => recording.tracing_failure(),
      Self::Failed(failure) => failure.tracing_failure(),
    }
  }
}

/// The direct value and post-execution recording state from one opt-in call.
pub struct Recorded<T> {
  run_id: RunId,
  value: T,
  state: RecordingState,
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

  /// Returns the post-execution recording state.
  pub fn state(&self) -> &RecordingState {
    &self.state
  }

  /// Returns the committed authority snapshot when recording committed.
  pub fn snapshot(&self) -> Option<&RunSnapshot> {
    self.state.snapshot()
  }

  /// Returns the typed post-execution recording failure, when present.
  pub fn recording_failure(&self) -> Option<&PostExecutionRecordingFailure> {
    self.state.failure()
  }

  /// Returns a non-authoritative tracing failure, when instrumentation flushing failed.
  pub fn tracing_failure(&self) -> Option<&FlushError> {
    self.state.tracing_failure()
  }

  /// Splits the direct value from post-execution recording state.
  pub fn into_parts(self) -> (RunId, T, RecordingState) {
    (self.run_id, self.value, self.state)
  }
}

/// Failure to establish recording before a caller-supplied future is constructed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RecordingError {
  /// Recording requires one configured run authority.
  #[error("recording requires one configured run store authority")]
  AuthorityUnavailable,
}

impl Dispatch {
  /// Records one caller-supplied future without selecting or dispatching domain work.
  ///
  /// The closure is constructed inside a fresh root context, the returned future
  /// is polled in that context on the caller's async task, and the sole configured
  /// authority is flushed and snapshotted before return. Once the future returns,
  /// recording failures are retained in [`Recorded`] and never replace its value.
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
    let flush = match self.flush().await {
      Ok(()) => RecordingFlush::Complete,
      Err(error) => RecordingFlush::Failed(error),
    };
    let state = match store.load_snapshot(run_id).await {
      Ok(Some(snapshot)) => RecordingState::Committed(CommittedRecording { snapshot, flush }),
      Ok(None) => RecordingState::Failed(PostExecutionRecordingFailure::SnapshotMissing { flush }),
      Err(source) => RecordingState::Failed(PostExecutionRecordingFailure::SnapshotRead { source, flush }),
    };
    Ok(Recorded {
      run_id,
      value,
      state,
    })
  }
}
