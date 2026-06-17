//! Run recording backend (store + recorder façade).
//!
//! `RunRecordingBackend` combines a `LocalStore` (canonical snapshot + artifact
//! file persistence) with one `RunRecorder` (live update delivery). Construct
//! one of these to share runtime recording state across CLI/library callers.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::artifact::{ArtifactFileSource, ArtifactRef, ProducedArtifact};
use crate::error::AuvResult;
use crate::recorded_operation::{RecordedOperationOutput, RecordedOperationServices};
use crate::run_builder::{RecordingRun, RunFinish, RunSpec, SpanRef};
use crate::store::{CanonicalRun, LocalStore};
use crate::time::now_millis;
use crate::trace::{
  ArtifactRecordV1Alpha1, EventRecordV1Alpha1, RunId, RunRecordV1Alpha1, SpanId,
  SpanRecordV1Alpha1, TraceFailure, TraceState, TraceStatusCode, new_event_id, new_run_id,
  new_span_id, new_trace_id,
};

use super::recorder::{NoopRunRecorder, RunRecorder};
use super::update::RunUpdate;

#[derive(Clone)]
pub struct RunRecordingBackend {
  store: LocalStore,
  recorder: Arc<dyn RunRecorder>,
  local_snapshot_write_enabled: bool,
  cleanup_store_on_drop: bool,
  cleanup_guard: Arc<()>,
}

#[derive(Clone)]
pub struct RecordingHandle {
  recording: RunRecordingBackend,
}

#[derive(Debug, Default)]
pub struct RecordedArtifacts {
  pub records: Vec<ArtifactRecordV1Alpha1>,
  pub paths: Vec<PathBuf>,
}

#[derive(Debug)]
pub struct ArtifactRecordingFailure {
  pub recorded: RecordedArtifacts,
  pub message: String,
}

impl RunRecordingBackend {
  pub fn new(store: LocalStore, recorder: Arc<dyn RunRecorder>) -> Self {
    Self {
      store,
      recorder,
      local_snapshot_write_enabled: true,
      cleanup_store_on_drop: false,
      cleanup_guard: Arc::new(()),
    }
  }

  pub fn local_only(store: LocalStore) -> Self {
    Self {
      store,
      recorder: Arc::new(NoopRunRecorder),
      local_snapshot_write_enabled: true,
      cleanup_store_on_drop: false,
      cleanup_guard: Arc::new(()),
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

  pub fn handle(&self) -> RecordingHandle {
    RecordingHandle {
      recording: self.clone(),
    }
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
    artifact: crate::artifact::ProducedArtifact,
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

  pub fn record_produced_artifacts(
    &self,
    run: &mut RecordingRun,
    span: &SpanRef,
    artifacts: impl IntoIterator<Item = ProducedArtifact>,
  ) -> Result<RecordedArtifacts, ArtifactRecordingFailure> {
    let mut recorded = RecordedArtifacts::default();

    for artifact in artifacts {
      let event_id = new_event_id();
      match self.stage_artifact(
        run.id(),
        run.artifact_count(),
        artifact,
        span.id(),
        Some(event_id.clone()),
      ) {
        Ok(stored_artifact) => {
          let staged_path = match self.run_dir(run.id()) {
            Ok(run_dir) => run_dir.join(&stored_artifact.path),
            Err(error) => {
              record_event_with_id(
                run,
                span.id(),
                event_id,
                "artifact.failed",
                Some(format!("artifact path resolution failed: {error}")),
                Vec::new(),
              );
              return Err(ArtifactRecordingFailure {
                recorded,
                message: error,
              });
            }
          };
          record_event_with_id(
            run,
            span.id(),
            event_id,
            "artifact.captured",
            Some(render_artifact_event(&stored_artifact)),
            vec![stored_artifact.artifact_id.clone()],
          );
          run.record_artifact(stored_artifact.clone());
          recorded.paths.push(staged_path.clone());
          recorded.records.push(stored_artifact.clone());
          if let Err(error) = self.record_artifact_bytes(run.id(), &stored_artifact, &staged_path) {
            record_event_with_id(
              run,
              span.id(),
              new_event_id(),
              "artifact.failed",
              Some(format!("artifact upload failed: {error}")),
              Vec::new(),
            );
            return Err(ArtifactRecordingFailure {
              recorded,
              message: error,
            });
          }
        }
        Err(error) => {
          record_event_with_id(
            run,
            span.id(),
            event_id,
            "artifact.failed",
            Some(format!("artifact staging failed: {error}")),
            Vec::new(),
          );
          return Err(ArtifactRecordingFailure {
            recorded,
            message: error,
          });
        }
      }
    }

    Ok(recorded)
  }
}

impl Drop for RunRecordingBackend {
  fn drop(&mut self) {
    if !self.cleanup_store_on_drop || Arc::strong_count(&self.cleanup_guard) != 1 {
      return;
    }
    let _ = std::fs::remove_dir_all(self.store.root());
  }
}

impl RecordingHandle {
  pub fn new(recording: RunRecordingBackend) -> Self {
    Self { recording }
  }

  pub fn recording_backend(&self) -> &RunRecordingBackend {
    &self.recording
  }

  pub fn run_dir(&self, run_id: impl AsRef<str>) -> AuvResult<PathBuf> {
    self.recording.run_dir(run_id)
  }

  pub fn read_run(&self, run_id: &str) -> AuvResult<CanonicalRun> {
    self.recording.read_run(run_id)
  }

  pub fn start_run(&self, spec: RunSpec) -> AuvResult<RecordingRun> {
    let run_id = new_run_id();
    let root_span_id = new_span_id();
    tracing::info!(
      target: "auv.tracing_driver",
      {
        auv.run_id = %run_id,
        auv.root_span_id = %root_span_id,
        auv.run_type = %spec.run_type.as_str(),
      },
      "AUV run started"
    );
    let started = now_millis();
    let mut run_attributes = spec.attributes.clone();
    run_attributes.insert(
      crate::trace::RUN_ATTR_DEVICE_ID.to_string(),
      serde_json::Value::String(spec.device_id.as_str().to_string()),
    );
    run_attributes.insert(
      crate::trace::RUN_ATTR_SESSION_ID.to_string(),
      serde_json::Value::String(spec.session_id.as_str().to_string()),
    );
    let run = RunRecordV1Alpha1 {
      api_version: crate::trace::RUN_API_VERSION.to_string(),
      run_id: run_id.clone(),
      trace_id: new_trace_id(),
      run_type: spec.run_type,
      state: TraceState::Running,
      status_code: TraceStatusCode::Unset,
      started_at_millis: started,
      finished_at_millis: None,
      root_span_id: root_span_id.clone(),
      attributes: run_attributes,
      summary: None,
      failure: None,
    };
    let root_span = SpanRecordV1Alpha1 {
      api_version: crate::trace::SPAN_API_VERSION.to_string(),
      span_id: root_span_id,
      parent_span_id: None,
      name: spec.root_span_name,
      state: TraceState::Running,
      status_code: TraceStatusCode::Unset,
      started_at_millis: started,
      finished_at_millis: None,
      attributes: spec.attributes,
      summary: None,
      failure: None,
    };
    let run = crate::run_builder::RecordingRun::new(run, root_span, self.recording.recorder());
    if self.recording.requires_successful_delivery() && !run.recording_errors().is_empty() {
      return Err(format!(
        "run recording delivery failed: {}",
        run.recording_errors().join("; ")
      ));
    }
    Ok(run)
  }

  pub fn finish_run(&self, run: RecordingRun, finish: RunFinish) -> AuvResult<RunId> {
    let failure = finish.failure.map(|message| TraceFailure { message });
    let recorded = run.finish(finish.status_code, finish.summary, failure);
    let run_id = recorded.snapshot.run.run_id.clone();
    tracing::info!(
      target: "auv.tracing_driver",
      {
        auv.run_id = %run_id,
        auv.status = %recorded.snapshot.run.status_code.as_str(),
      },
      "AUV run finished"
    );
    let mut recording_errors = recorded.recording_errors;
    self.recording.write_run_snapshot(&recorded.snapshot)?;
    if let Err(error) = self.recording.record(RunUpdate::RunFinished {
      run_id: run_id.clone(),
      run: recorded.snapshot.run,
    }) && self.recording.requires_successful_delivery()
    {
      recording_errors.push(error);
    }
    if !recording_errors.is_empty() {
      return Err(format!(
        "run recording delivery failed: {}",
        recording_errors.join("; ")
      ));
    }
    Ok(run_id)
  }

  pub fn run_recorded_operation<T, E, F>(
    &self,
    spec: RunSpec,
    operation_label: impl Into<String>,
    operation: F,
  ) -> AuvResult<RecordedOperationOutput<T>>
  where
    E: std::fmt::Display,
    F: FnOnce(&mut crate::recorded_operation::RecordedOperationContext<'_>) -> Result<T, E>,
  {
    let services = RecordedOperationServices {
      recording: self.recording_backend(),
      start_run: &|spec| self.start_run(spec),
      finish_run: &|run, finish| self.finish_run(run, finish),
      run_dir: &|run_id| self.run_dir(run_id),
    };
    crate::recorded_operation::run_recorded_operation(&services, spec, operation_label, operation)
  }

  pub fn stage_artifact_file(
    &self,
    run: &mut RecordingRun,
    span: &SpanRef,
    role: impl Into<String>,
    source_path: &Path,
    preferred_name: impl Into<String>,
    summary: Option<String>,
  ) -> AuvResult<PathBuf> {
    let event_id = new_event_id();
    let artifact = self.recording.stage_artifact_file(
      run.id(),
      run.artifact_count(),
      span.id(),
      Some(event_id.clone()),
      ArtifactFileSource {
        role: role.into(),
        source_path: source_path.to_path_buf(),
        preferred_name: preferred_name.into(),
        summary,
      },
    )?;
    tracing::info!(
      target: "auv.tracing_driver",
      {
        auv.run_id = %run.id(),
        auv.span_id = %span.id(),
        auv.artifact_id = %artifact.artifact_id,
        auv.artifact_role = %artifact.role,
      },
      "AUV artifact staged"
    );
    record_event_with_id(
      run,
      span.id(),
      event_id,
      "artifact.captured",
      Some(render_artifact_event(&artifact)),
      vec![artifact.artifact_id.clone()],
    );
    let staged_path = self.recording.run_dir(run.id())?.join(&artifact.path);
    run.record_artifact(artifact.clone());
    self
      .recording
      .record_artifact_bytes(run.id(), &artifact, &staged_path)?;
    Ok(staged_path)
  }

  pub fn stage_artifact_file_with_ref(
    &self,
    run: &mut RecordingRun,
    span: &SpanRef,
    role: impl Into<String>,
    source_path: &Path,
    preferred_name: impl Into<String>,
    summary: Option<String>,
  ) -> AuvResult<(PathBuf, ArtifactRef)> {
    let event_id = new_event_id();
    let artifact = self.recording.stage_artifact_file(
      run.id(),
      run.artifact_count(),
      span.id(),
      Some(event_id.clone()),
      ArtifactFileSource {
        role: role.into(),
        source_path: source_path.to_path_buf(),
        preferred_name: preferred_name.into(),
        summary,
      },
    )?;
    tracing::info!(
      target: "auv.tracing_driver",
      {
        auv.run_id = %run.id(),
        auv.span_id = %span.id(),
        auv.artifact_id = %artifact.artifact_id,
        auv.artifact_role = %artifact.role,
      },
      "AUV artifact staged"
    );
    record_event_with_id(
      run,
      span.id(),
      event_id.clone(),
      "artifact.captured",
      Some(render_artifact_event(&artifact)),
      vec![artifact.artifact_id.clone()],
    );
    let staged_path = self.recording.run_dir(run.id())?.join(&artifact.path);
    let artifact_ref = ArtifactRef {
      run_id: run.id().clone(),
      artifact_id: artifact.artifact_id.clone(),
      span_id: span.id().clone(),
      captured_event_id: Some(event_id),
    };
    run.record_artifact(artifact.clone());
    self
      .recording
      .record_artifact_bytes(run.id(), &artifact, &staged_path)?;
    Ok((staged_path, artifact_ref))
  }
}

fn record_event_with_id(
  run: &mut RecordingRun,
  span_id: &SpanId,
  event_id: crate::trace::EventId,
  name: &str,
  message: Option<String>,
  artifact_ids: Vec<crate::trace::ArtifactId>,
) {
  run.record_event(EventRecordV1Alpha1 {
    api_version: crate::trace::EVENT_API_VERSION.to_string(),
    event_id,
    span_id: span_id.clone(),
    name: name.to_string(),
    timestamp_millis: now_millis(),
    attributes: Default::default(),
    message,
    artifact_ids,
  });
}

fn render_artifact_event(artifact: &ArtifactRecordV1Alpha1) -> String {
  let note = artifact
    .summary
    .clone()
    .unwrap_or_else(|| "n/a".to_string());
  format!(
    "{} kind={} path={} note={}",
    artifact.artifact_id, artifact.role, artifact.path, note
  )
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;

  use crate::artifact::{ArtifactFileSource, ProducedArtifact};
  use crate::run_builder::{RunFinish, RunSpec};
  use crate::store::LocalStore;
  use crate::trace::{RunId, RunType, SpanId, TraceStatusCode};

  use super::super::recorder::NoopRunRecorder;
  use super::RunRecordingBackend;

  #[test]
  fn record_produced_artifacts_records_paths_events_and_snapshot_artifacts() {
    let root = std::env::temp_dir().join(format!(
      "auv-recording-produced-artifacts-{}",
      crate::time::now_millis()
    ));
    let source = std::env::temp_dir().join(format!(
      "auv-recording-produced-source-{}.txt",
      crate::time::now_millis()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::write(&source, "artifact body").expect("artifact source should write");

    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let recording = RunRecordingBackend::local_only(store);
    let handle = recording.handle();
    let mut run = handle
      .start_run(RunSpec::new(RunType::Command, "test.command"))
      .expect("run should start");
    let span = run.root_span();

    let recorded = recording
      .record_produced_artifacts(
        &mut run,
        &span,
        [ProducedArtifact {
          kind: "text".to_string(),
          source_path: source.clone(),
          preferred_name: "artifact.txt".to_string(),
          note: Some("test artifact".to_string()),
        }],
      )
      .expect("artifact should record");

    assert_eq!(recorded.records.len(), 1);
    assert_eq!(recorded.paths.len(), 1);
    assert!(recorded.paths[0].exists());
    let snapshot = run.snapshot_preview();
    assert_eq!(snapshot.artifacts.len(), 1);
    assert_eq!(snapshot.events.len(), 1);
    assert_eq!(snapshot.events[0].name, "artifact.captured");
    assert_eq!(
      snapshot.events[0].artifact_ids,
      vec![snapshot.artifacts[0].artifact_id.clone()]
    );

    handle
      .finish_run(
        run,
        RunFinish {
          status_code: TraceStatusCode::Ok,
          summary: Some("done".to_string()),
          failure: None,
        },
      )
      .expect("run should finish");

    let _ = std::fs::remove_file(source);
    let _ = std::fs::remove_dir_all(root);
  }

  #[test]
  fn recording_backend_cleans_temporary_store_on_drop() {
    let root = std::env::temp_dir().join(format!(
      "auv-recording-temp-store-cleanup-{}",
      crate::time::now_millis()
    ));
    let source = std::env::temp_dir().join(format!(
      "auv-recording-temp-source-{}.txt",
      crate::time::now_millis()
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
