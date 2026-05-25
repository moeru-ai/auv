//! Run recording backend (store + recorder façade).
//!
//! `RunRecordingBackend` combines a `LocalStore` (canonical snapshot + artifact
//! file persistence) with one `RunRecorder` (live update delivery). Construct
//! one of these to share runtime recording state across CLI/library callers.

use std::path::Path;
use std::sync::Arc;

use crate::model::AuvResult;
use crate::store::{ArtifactFileSource, CanonicalRun, LocalStore};
use crate::trace::{ArtifactRecordV1Alpha1, RunId, SpanId};

use super::recorder::{NoopRunRecorder, RunRecorder};
use super::update::RunUpdate;

#[derive(Clone)]
pub struct RunRecordingBackend {
  store: LocalStore,
  recorder: Arc<dyn RunRecorder>,
  local_snapshot_write_enabled: bool,
  cleanup_store_on_drop: bool,
}

impl RunRecordingBackend {
  pub fn new(store: LocalStore, recorder: Arc<dyn RunRecorder>) -> Self {
    Self {
      store,
      recorder,
      local_snapshot_write_enabled: true,
      cleanup_store_on_drop: false,
    }
  }

  pub fn local_only(store: LocalStore) -> Self {
    Self {
      store,
      recorder: Arc::new(NoopRunRecorder),
      local_snapshot_write_enabled: true,
      cleanup_store_on_drop: false,
    }
  }

  pub fn with_local_snapshot_write_enabled(mut self, enabled: bool) -> Self {
    self.local_snapshot_write_enabled = enabled;
    self
  }

  pub fn with_temporary_store_cleanup(mut self, cleanup: bool) -> Self {
    self.cleanup_store_on_drop = cleanup;
    self
  }

  pub fn store(&self) -> &LocalStore {
    &self.store
  }

  pub fn recorder(&self) -> Arc<dyn RunRecorder> {
    self.recorder.clone()
  }

  pub fn record(&self, update: RunUpdate) -> AuvResult<()> {
    self.recorder.record(update)
  }

  pub fn requires_successful_delivery(&self) -> bool {
    self.recorder.requires_successful_delivery()
  }

  pub fn read_run(&self, run_id: &str) -> AuvResult<CanonicalRun> {
    self.store.read_run(run_id)
  }

  pub fn write_run_snapshot(&self, snapshot: &CanonicalRun) -> AuvResult<()> {
    if !self.local_snapshot_write_enabled {
      return Ok(());
    }
    self.store.replace_run_snapshot(snapshot)
  }

  pub fn run_dir(&self, run_id: impl AsRef<str>) -> AuvResult<std::path::PathBuf> {
    self.store.run_dir(run_id)
  }

  pub fn stage_artifact(
    &self,
    run_id: &RunId,
    index: usize,
    artifact: crate::model::ProducedArtifact,
    span_id: &SpanId,
    event_id: Option<crate::trace::EventId>,
  ) -> AuvResult<ArtifactRecordV1Alpha1> {
    self
      .store
      .stage_artifact(run_id, index, artifact, span_id, event_id)
  }

  pub fn stage_artifact_file(
    &self,
    run_id: &RunId,
    index: usize,
    span_id: &SpanId,
    event_id: Option<crate::trace::EventId>,
    artifact: ArtifactFileSource,
  ) -> AuvResult<ArtifactRecordV1Alpha1> {
    self
      .store
      .stage_artifact_file(run_id, index, span_id, event_id, artifact)
  }

  pub fn record_artifact_bytes(
    &self,
    run_id: &RunId,
    artifact: &ArtifactRecordV1Alpha1,
    path: &Path,
  ) -> AuvResult<()> {
    self.recorder.record_artifact_bytes(run_id, artifact, path)
  }
}

impl Drop for RunRecordingBackend {
  fn drop(&mut self) {
    if self.cleanup_store_on_drop {
      let _ = std::fs::remove_dir_all(self.store.root());
    }
  }
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;

  use crate::store::{ArtifactFileSource, LocalStore};
  use crate::trace::{RunId, SpanId};

  use super::super::recorder::NoopRunRecorder;
  use super::RunRecordingBackend;

  #[test]
  fn recording_backend_cleans_temporary_store_on_drop() {
    let root = std::env::temp_dir().join(format!(
      "auv-recording-temp-store-cleanup-{}",
      crate::model::now_millis()
    ));
    let source = std::env::temp_dir().join(format!(
      "auv-recording-temp-source-{}.txt",
      crate::model::now_millis()
    ));
    std::fs::write(&source, "artifact body").expect("artifact source should write");
    {
      let store = LocalStore::new(root.clone()).expect("store should initialize");
      let recording = RunRecordingBackend::new(store, Arc::new(NoopRunRecorder))
        .with_local_snapshot_write_enabled(false)
        .with_temporary_store_cleanup(true);
      let artifact = recording
        .stage_artifact_file(
          &RunId::new("run_temp_cleanup"),
          0,
          &SpanId::new("0000000000000001"),
          None,
          ArtifactFileSource {
            role: "text".to_string(),
            source_path: source.clone(),
            preferred_name: "artifact.txt".to_string(),
            summary: None,
          },
        )
        .expect("temporary artifact should stage");
      assert!(
        recording
          .run_dir("run_temp_cleanup")
          .expect("run dir")
          .join(artifact.path)
          .exists()
      );
      assert!(root.exists());
    }

    let _ = std::fs::remove_file(source);
    assert!(!root.exists());
  }
}
