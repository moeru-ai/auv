//! Recorded typed-operation helper.
//!
//! This is the narrow bridge between typed Rust driver APIs and AUV's existing
//! run/span/event/artifact recording model. It deliberately does not depend on
//! `DriverCall`/`DriverResponse`; callers provide ordinary Rust code and the
//! root runtime owns recording + persistence.

use std::fmt::Display;
use std::path::{Path, PathBuf};

use crate::artifact::{ArtifactFileSource, ArtifactRef};
use crate::error::AuvResult;
use crate::recording::RunRecordingBackend;
use crate::run_builder::{Attributes, RecordingRun, RunFinish, RunSpec, SpanFinish, SpanRef};
use crate::time::now_millis;
use crate::trace::{
  EVENT_API_VERSION, EventId, EventRecordV1Alpha1, RunId, SPAN_API_VERSION, SpanRecordV1Alpha1,
  TraceState, TraceStatusCode, new_event_id, new_span_id,
};

pub struct RecordedOperationContext<'a> {
  recording: &'a RunRecordingBackend,
  run: &'a mut RecordingRun,
  root: SpanRef,
  current: SpanRef,
}

impl<'a> RecordedOperationContext<'a> {
  pub fn recording(&self) -> &RunRecordingBackend {
    self.recording
  }

  pub fn run(&self) -> &RecordingRun {
    self.run
  }

  pub fn run_mut(&mut self) -> &mut RecordingRun {
    self.run
  }

  pub fn run_id(&self) -> &RunId {
    self.run.id()
  }

  pub fn root_span(&self) -> &SpanRef {
    &self.root
  }

  pub fn current_span(&self) -> &SpanRef {
    &self.current
  }

  pub fn record_event(&mut self, name: impl Into<String>, message: Option<String>) -> EventId {
    record_operation_event(self.run, &self.current, name.into(), message)
  }

  pub fn stage_artifact_file(
    &mut self,
    role: impl Into<String>,
    source_path: &Path,
    preferred_name: impl Into<String>,
    summary: Option<String>,
  ) -> AuvResult<PathBuf> {
    let current = self.current.clone();
    self.stage_artifact_file_in_span(&current, role, source_path, preferred_name, summary)
  }

  pub fn stage_artifact_file_in_span(
    &mut self,
    span: &SpanRef,
    role: impl Into<String>,
    source_path: &Path,
    preferred_name: impl Into<String>,
    summary: Option<String>,
  ) -> AuvResult<PathBuf> {
    let event_id = new_event_id();
    let artifact = self.recording().stage_artifact_file(
      self.run.id(),
      self.run.artifact_count(),
      span.id(),
      Some(event_id.clone()),
      ArtifactFileSource {
        role: role.into(),
        source_path: source_path.to_path_buf(),
        preferred_name: preferred_name.into(),
        summary,
      },
    )?;
    record_event_with_id(
      self.run,
      span.id(),
      event_id,
      "artifact.captured",
      Some(render_artifact_event(&artifact)),
      vec![artifact.artifact_id.clone()],
    );
    let staged_path = self
      .recording()
      .run_dir(self.run.id())?
      .join(&artifact.path);
    self.run.record_artifact(artifact.clone());
    self
      .recording()
      .record_artifact_bytes(self.run.id(), &artifact, &staged_path)?;
    Ok(staged_path)
  }

  pub fn stage_artifact_file_with_ref(
    &mut self,
    role: impl Into<String>,
    source_path: &Path,
    preferred_name: impl Into<String>,
    summary: Option<String>,
  ) -> AuvResult<(PathBuf, ArtifactRef)> {
    let current = self.current.clone();
    self.stage_artifact_file_with_ref_in_span(&current, role, source_path, preferred_name, summary)
  }

  pub fn stage_artifact_file_with_ref_in_span(
    &mut self,
    span: &SpanRef,
    role: impl Into<String>,
    source_path: &Path,
    preferred_name: impl Into<String>,
    summary: Option<String>,
  ) -> AuvResult<(PathBuf, ArtifactRef)> {
    let event_id = new_event_id();
    let artifact = self.recording().stage_artifact_file(
      self.run.id(),
      self.run.artifact_count(),
      span.id(),
      Some(event_id.clone()),
      ArtifactFileSource {
        role: role.into(),
        source_path: source_path.to_path_buf(),
        preferred_name: preferred_name.into(),
        summary,
      },
    )?;
    record_event_with_id(
      self.run,
      span.id(),
      event_id.clone(),
      "artifact.captured",
      Some(render_artifact_event(&artifact)),
      vec![artifact.artifact_id.clone()],
    );
    let staged_path = self
      .recording()
      .run_dir(self.run.id())?
      .join(&artifact.path);
    let artifact_ref = ArtifactRef {
      run_id: self.run.id().clone(),
      artifact_id: artifact.artifact_id.clone(),
      span_id: span.id().clone(),
      captured_event_id: Some(event_id),
    };
    self.run.record_artifact(artifact.clone());
    self
      .recording()
      .record_artifact_bytes(self.run.id(), &artifact, &staged_path)?;
    Ok((staged_path, artifact_ref))
  }

  pub fn stage_artifact_bytes_with_ref(
    &mut self,
    role: impl Into<String>,
    bytes: impl AsRef<[u8]>,
    preferred_name: impl Into<String>,
    summary: Option<String>,
  ) -> AuvResult<(PathBuf, ArtifactRef)> {
    let current = self.current.clone();
    self.stage_artifact_bytes_with_ref_in_span(&current, role, bytes, preferred_name, summary)
  }

  pub fn stage_artifact_bytes_with_ref_in_span(
    &mut self,
    span: &SpanRef,
    role: impl Into<String>,
    bytes: impl AsRef<[u8]>,
    preferred_name: impl Into<String>,
    summary: Option<String>,
  ) -> AuvResult<(PathBuf, ArtifactRef)> {
    let event_id = new_event_id();
    let artifact = self.recording().stage_artifact_bytes(
      self.run.id(),
      self.run.artifact_count(),
      span.id(),
      Some(event_id.clone()),
      crate::artifact::ArtifactBytesSource {
        role: role.into(),
        bytes: bytes.as_ref().to_vec(),
        preferred_name: preferred_name.into(),
        summary,
      },
    )?;
    record_event_with_id(
      self.run,
      span.id(),
      event_id.clone(),
      "artifact.captured",
      Some(render_artifact_event(&artifact)),
      vec![artifact.artifact_id.clone()],
    );
    let staged_path = self
      .recording()
      .run_dir(self.run.id())?
      .join(&artifact.path);
    let artifact_ref = ArtifactRef {
      run_id: self.run.id().clone(),
      artifact_id: artifact.artifact_id.clone(),
      span_id: span.id().clone(),
      captured_event_id: Some(event_id),
    };
    self.run.record_artifact(artifact.clone());
    self
      .recording()
      .record_artifact_bytes(self.run.id(), &artifact, &staged_path)?;
    Ok((staged_path, artifact_ref))
  }

  pub fn in_span<T, E, F>(&mut self, name: impl Into<String>, operation: F) -> Result<T, E>
  where
    E: Display + From<String>,
    F: FnOnce(&mut Self) -> Result<T, E>,
  {
    self.in_span_with_attributes(name, Attributes::new(), operation)
  }

  pub fn in_span_with_attributes<T, E, F>(
    &mut self,
    name: impl Into<String>,
    attributes: Attributes,
    operation: F,
  ) -> Result<T, E>
  where
    E: Display + From<String>,
    F: FnOnce(&mut Self) -> Result<T, E>,
  {
    let span_name = name.into();
    let span = self
      .run
      .start_span(&self.current, operation_span_record(&span_name, attributes))
      .map_err(E::from)?;
    let previous = self.current.clone();
    self.current = span.clone();
    let result = operation(self);
    self.current = previous;

    match result {
      Ok(value) => {
        self
          .run
          .finish_span(
            &span,
            SpanFinish {
              status_code: TraceStatusCode::Ok,
              summary: Some(format!("{span_name} completed")),
              failure: None,
            },
          )
          .map_err(E::from)?;
        Ok(value)
      }
      Err(error) => {
        let message = error.to_string();
        if let Err(finish_error) = self.run.finish_span(
          &span,
          SpanFinish {
            status_code: TraceStatusCode::Error,
            summary: Some(format!("{span_name} failed")),
            failure: Some(message.clone()),
          },
        ) {
          return Err(E::from(format!(
            "{message}; additionally failed to finish failed span {span_name}: {finish_error}"
          )));
        }
        Err(error)
      }
    }
  }
}

#[derive(Debug)]
pub struct RecordedOperationOutput<T> {
  pub value: T,
  pub run_id: RunId,
  pub run_dir: PathBuf,
}

pub struct RecordedOperationServices<'a> {
  pub recording: &'a RunRecordingBackend,
  pub start_run: &'a dyn Fn(RunSpec) -> AuvResult<RecordingRun>,
  pub finish_run: &'a dyn Fn(RecordingRun, RunFinish) -> AuvResult<RunId>,
  pub run_dir: &'a dyn Fn(&str) -> AuvResult<PathBuf>,
}

pub fn run_recorded_operation<T, E, F>(
  services: &RecordedOperationServices<'_>,
  spec: RunSpec,
  operation_label: impl Into<String>,
  operation: F,
) -> AuvResult<RecordedOperationOutput<T>>
where
  E: Display,
  F: FnOnce(&mut RecordedOperationContext<'_>) -> Result<T, E>,
{
  let operation_label = operation_label.into();
  let success_summary = format!("{operation_label} completed");
  let failure_summary = format!("{operation_label} failed");

  let mut run = (services.start_run)(spec)?;
  let root = run.root_span();
  let run_id = run.id().clone();
  let run_dir = (services.run_dir)(run_id.as_str())?;
  let operation_span = tracing::info_span!(
    target: "auv.tracing_driver",
    "auv.recorded_operation",
    auv.run_id = %run_id,
    auv.root_span_id = %root.id(),
    auv.operation_label = %operation_label,
  );
  let _operation_span_guard = operation_span.enter();

  record_operation_event(
    &mut run,
    &root,
    "operation.started".to_string(),
    Some(format!("recorded operation {operation_label} started")),
  );

  let result = {
    let mut context = RecordedOperationContext {
      recording: services.recording,
      run: &mut run,
      root: root.clone(),
      current: root.clone(),
    };
    operation(&mut context).map_err(|error| error.to_string())
  };

  match result {
    Ok(value) => {
      record_operation_event(
        &mut run,
        &root,
        "operation.completed".to_string(),
        Some(success_summary.clone()),
      );
      (services.finish_run)(
        run,
        RunFinish {
          status_code: TraceStatusCode::Ok,
          summary: Some(success_summary),
          failure: None,
        },
      )?;
      Ok(RecordedOperationOutput {
        value,
        run_id,
        run_dir,
      })
    }
    Err(error) => {
      record_operation_event(
        &mut run,
        &root,
        "operation.failed".to_string(),
        Some(format!("{failure_summary}: {error}")),
      );
      let finish_result = (services.finish_run)(
        run,
        RunFinish {
          status_code: TraceStatusCode::Error,
          summary: Some(failure_summary),
          failure: Some(error.clone()),
        },
      );
      if let Err(finish_error) = finish_result {
        return Err(format!(
          "{error}; additionally failed to persist failed run {run_id}: {finish_error}"
        ));
      }
      Err(format!("{error}; recorded run: {}", run_dir.display()))
    }
  }
}

fn record_operation_event(
  run: &mut RecordingRun,
  span: &SpanRef,
  name: String,
  message: Option<String>,
) -> EventId {
  run.record_event(EventRecordV1Alpha1 {
    api_version: EVENT_API_VERSION.to_string(),
    event_id: new_event_id(),
    span_id: span.id().clone(),
    name,
    timestamp_millis: now_millis(),
    attributes: Default::default(),
    message,
    artifact_ids: Vec::new(),
  })
}

fn operation_span_record(name: &str, attributes: Attributes) -> SpanRecordV1Alpha1 {
  SpanRecordV1Alpha1 {
    api_version: SPAN_API_VERSION.to_string(),
    span_id: new_span_id(),
    parent_span_id: None,
    name: name.to_string(),
    state: TraceState::Running,
    status_code: TraceStatusCode::Unset,
    started_at_millis: now_millis(),
    finished_at_millis: None,
    attributes,
    summary: None,
    failure: None,
  }
}

fn record_event_with_id(
  run: &mut RecordingRun,
  span_id: &crate::trace::SpanId,
  event_id: EventId,
  name: &str,
  message: Option<String>,
  artifact_ids: Vec<crate::trace::ArtifactId>,
) {
  run.record_event(EventRecordV1Alpha1 {
    api_version: EVENT_API_VERSION.to_string(),
    event_id,
    span_id: span_id.clone(),
    name: name.to_string(),
    timestamp_millis: now_millis(),
    attributes: Default::default(),
    message,
    artifact_ids,
  });
}

fn render_artifact_event(artifact: &crate::trace::ArtifactRecordV1Alpha1) -> String {
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
  use std::env;
  use std::fs;
  use std::path::PathBuf;

  use crate::recording::RunRecordingBackend;
  use crate::run_builder::RunSpec;
  use crate::store::LocalStore;
  use crate::trace::{RunType, TraceStatusCode};

  #[test]
  fn run_recorded_operation_persists_successful_run_with_root_artifacts() {
    let project_root = temp_dir("recorded-operation-project");
    let store_root = temp_dir("recorded-operation-store");
    let source_path = project_root.join("sample.txt");
    fs::create_dir_all(&project_root).expect("project root should exist");
    fs::write(&source_path, "hello recorded operation").expect("sample file should write");

    let recording = test_recording(store_root.clone());

    let output = recording
      .run_recorded_operation(
        RunSpec::new(RunType::Execute, "auv.example.typed"),
        "Typed operation sample",
        |context| {
          context.in_span("typed.step", |context| {
            context.record_event(
              "typed.step.event",
              Some("typed operation closure executed".to_string()),
            );
            context.stage_artifact_file(
              "debug",
              &source_path,
              "sample.txt",
              Some("first sample artifact".to_string()),
            )?;
            Ok::<_, String>(())
          })?;
          context.stage_artifact_file(
            "debug",
            &source_path,
            "sample-2.txt",
            Some("second sample artifact".to_string()),
          )?;
          Ok::<_, String>("ok".to_string())
        },
      )
      .expect("recorded operation should succeed");

    let run = recording
      .recording_backend()
      .read_run(output.run_id.as_str())
      .expect("run should persist");
    assert_eq!(output.value, "ok");
    assert_eq!(run.run.status_code, TraceStatusCode::Ok);
    assert_eq!(
      run.run.summary.as_deref(),
      Some("Typed operation sample completed")
    );
    assert_eq!(run.artifacts.len(), 2);
    assert_eq!(run.artifacts[0].artifact_id.as_str(), "artifact_0001");
    assert_eq!(run.artifacts[1].artifact_id.as_str(), "artifact_0002");
    let child_span = run
      .spans
      .iter()
      .find(|span| span.name == "typed.step")
      .expect("child span should be recorded");
    assert_eq!(child_span.status_code, TraceStatusCode::Ok);
    assert_eq!(run.artifacts[0].span_id, child_span.span_id);
    assert_eq!(run.artifacts[1].span_id, run.run.root_span_id);
    assert!(
      run
        .events
        .iter()
        .any(|event| event.name == "operation.started")
    );
    assert!(
      run
        .events
        .iter()
        .any(|event| event.name == "typed.step.event" && event.span_id == child_span.span_id)
    );
    assert!(
      run
        .events
        .iter()
        .any(|event| event.name == "artifact.captured" && event.span_id == child_span.span_id)
    );
    assert!(output.run_dir.join(&run.artifacts[0].path).exists());

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn run_recorded_operation_persists_failed_run_and_returns_run_path() {
    let project_root = temp_dir("recorded-operation-fail-project");
    let store_root = temp_dir("recorded-operation-fail-store");
    fs::create_dir_all(&project_root).expect("project root should exist");

    let recording = test_recording(store_root.clone());
    let mut observed_run_id = None;

    let error = recording
      .run_recorded_operation(
        RunSpec::new(RunType::Execute, "auv.example.typed_failure"),
        "Typed operation failure sample",
        |context| {
          observed_run_id = Some(context.run_id().to_string());
          context.in_span("typed.failure", |_context| Err::<(), _>("boom".to_string()))
        },
      )
      .expect_err("recorded operation should fail");

    let run_id = observed_run_id.expect("closure should observe run id");
    let run = recording
      .recording_backend()
      .read_run(&run_id)
      .expect("failed run should persist");
    assert!(error.contains("boom"));
    assert!(error.contains("recorded run:"));
    assert_eq!(run.run.status_code, TraceStatusCode::Error);
    assert_eq!(
      run.run.summary.as_deref(),
      Some("Typed operation failure sample failed")
    );
    assert_eq!(
      run
        .run
        .failure
        .as_ref()
        .map(|failure| failure.message.as_str()),
      Some("boom")
    );
    let child_span = run
      .spans
      .iter()
      .find(|span| span.name == "typed.failure")
      .expect("failed child span should be recorded");
    assert_eq!(child_span.status_code, TraceStatusCode::Error);
    assert_eq!(
      child_span
        .failure
        .as_ref()
        .map(|failure| failure.message.as_str()),
      Some("boom")
    );
    assert!(
      run
        .events
        .iter()
        .any(|event| event.name == "operation.failed")
    );

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  fn test_recording(store_root: PathBuf) -> crate::recording::RecordingHandle {
    let store = LocalStore::new(store_root).expect("store should initialize");
    RunRecordingBackend::local_only(store).handle()
  }

  fn temp_dir(label: &str) -> PathBuf {
    let path = env::temp_dir().join(format!("auv-{}-{}", label, crate::time::now_millis()));
    let _ = fs::remove_dir_all(&path);
    path
  }
}
