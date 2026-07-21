use auv_tracing::{DispatchTask, TaskSpawnError, TaskSpawner};

/// Schedules instrumentation IO on the currently entered Tokio runtime.
#[derive(Clone)]
pub struct TokioTaskSpawner {
  handle: tokio::runtime::Handle,
}

impl TokioTaskSpawner {
  /// Captures the current Tokio runtime handle without creating a runtime.
  pub fn current() -> Result<Self, NoCurrentRuntime> {
    tokio::runtime::Handle::try_current().map(|handle| Self { handle }).map_err(|_| NoCurrentRuntime)
  }
}

impl TaskSpawner for TokioTaskSpawner {
  fn spawn(&self, task: DispatchTask) -> Result<(), TaskSpawnError> {
    self.handle.spawn(task);
    Ok(())
  }
}

/// Reports that no Tokio runtime is entered on the calling thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("no current Tokio runtime is available")]
pub struct NoCurrentRuntime;
