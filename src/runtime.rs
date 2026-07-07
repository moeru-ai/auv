// File: src/runtime.rs
//! Runtime execution engine.
//!
//! `Runtime` is a shrinking compatibility facade for legacy recorded operations
//! and recording access while invoke execution has moved to `auv-cli-invoke`.
//!
//! Boundary: this layer executes *given* requests. It is not a planner/LLM
//! agent, and it does not choose strategies beyond what the request/cmd
//! specifies.

use std::path::PathBuf;
use std::sync::Arc;

use crate::model::AuvResult;
use auv_tracing_driver::store::LocalStore;
use auv_tracing_driver::trace::RunType;
use auv_tracing_driver::{MemoryRunRecorder, RunRecordingBackend};

pub struct Runtime {
  recording: RunRecordingBackend,
}

impl Runtime {
  pub fn new(_project_root: PathBuf, store: LocalStore) -> Self {
    Self {
      recording: RunRecordingBackend::new(store, Arc::new(MemoryRunRecorder::new())),
    }
  }

  pub fn open_session(&self, options: crate::session::SessionOptions) -> crate::session::SessionRuntime {
    crate::session::SessionRuntime::new(options)
  }
  pub fn read_run(&self, run_id: &str) -> AuvResult<auv_tracing_driver::store::CanonicalRun> {
    self.recording.read_run(run_id)
  }
  pub fn run_recorded_operation<T, E, F>(
    &self,
    spec: auv_tracing_driver::run_builder::RunSpec,
    operation_label: impl Into<String>,
    operation: F,
  ) -> AuvResult<auv_tracing_driver::recorded_operation::RecordedOperationOutput<T>>
  where
    E: std::fmt::Display,
    F: FnOnce(&mut auv_tracing_driver::recorded_operation::RecordedOperationContext<'_>) -> Result<T, E>,
  {
    self.recording.handle().run_recorded_operation(spec, operation_label, operation)
  }
  pub fn run_candidate_action_command(
    &self,
    request: crate::candidate_action_command::CandidateActionCommandRequest,
  ) -> AuvResult<
    auv_tracing_driver::recorded_operation::RecordedOperationOutput<crate::candidate_action_command::CandidateActionCommandOutput>,
  > {
    self.run_recorded_operation(
      auv_tracing_driver::run_builder::RunSpec::new(RunType::Execute, "auv.candidate.action.command"),
      "Consent-gated candidate action command",
      |context| crate::candidate_action_command::execute_candidate_action_command(context, &request),
    )
  }
  #[cfg(test)]
  pub(crate) fn recording_backend(&self) -> &RunRecordingBackend {
    &self.recording
  }

  pub fn recording(&self) -> &RunRecordingBackend {
    &self.recording
  }

  pub fn with_recording(mut self, recording: RunRecordingBackend) -> Self {
    self.recording = recording;
    self
  }
}

#[cfg(test)]
mod tests {
  use serde_json::json;
  use std::env;
  use std::fs;
  use std::path::PathBuf;

  use super::Runtime;
  use crate::model::now_millis;
  use auv_tracing_driver::store::LocalStore;
  fn temp_dir(label: &str) -> PathBuf {
    let path = env::temp_dir().join(format!("auv-{}-{}", label, now_millis()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("temp dir should be creatable");
    path
  }

  #[test]
  fn start_run_with_default_spec_stamps_local_default_attributes() {
    let project_root = temp_dir("runtime-default-device-project");
    let store_root = temp_dir("runtime-default-device-store");
    let runtime = runtime_with_success_driver(project_root.clone(), store_root.clone());

    let run = runtime
      .recording()
      .handle()
      .start_run(auv_tracing_driver::run_builder::RunSpec::new(auv_tracing_driver::trace::RunType::Command, "auv.command"))
      .expect("default-spec run should start");
    assert_eq!(run.device_id().as_str(), "local");
    assert_eq!(run.session_id().as_str(), "default");

    runtime
      .recording()
      .handle()
      .finish_run(
        run,
        auv_tracing_driver::run_builder::RunFinish {
          status_code: auv_tracing_driver::trace::TraceStatusCode::Ok,
          summary: Some("default".to_string()),
          failure: None,
        },
      )
      .expect("default-spec run should finish");

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn start_run_with_explicit_device_session_overrides_defaults() {
    let project_root = temp_dir("runtime-explicit-device-project");
    let store_root = temp_dir("runtime-explicit-device-store");
    let runtime = runtime_with_success_driver(project_root.clone(), store_root.clone());

    let spec = auv_tracing_driver::run_builder::RunSpec::new(auv_tracing_driver::trace::RunType::Command, "auv.command")
      .with_device(auv_tracing_driver::trace::DeviceId::new("remote-mac"))
      .with_session(auv_tracing_driver::trace::SessionId::new("music"));
    let run = runtime.recording().handle().start_run(spec).expect("explicit-device run should start");
    assert_eq!(run.device_id().as_str(), "remote-mac");
    assert_eq!(run.session_id().as_str(), "music");

    runtime
      .recording()
      .handle()
      .finish_run(
        run,
        auv_tracing_driver::run_builder::RunFinish {
          status_code: auv_tracing_driver::trace::TraceStatusCode::Ok,
          summary: Some("explicit".to_string()),
          failure: None,
        },
      )
      .expect("explicit-device run should finish");

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn run_snapshot_stores_device_session_in_attributes() {
    let project_root = temp_dir("runtime-attr-roundtrip-project");
    let store_root = temp_dir("runtime-attr-roundtrip-store");
    let runtime = runtime_with_success_driver(project_root.clone(), store_root.clone());

    let spec = auv_tracing_driver::run_builder::RunSpec::new(auv_tracing_driver::trace::RunType::Command, "auv.command")
      .with_device(auv_tracing_driver::trace::DeviceId::new("local"))
      .with_session(auv_tracing_driver::trace::SessionId::new("scan"));
    let run = runtime.recording().handle().start_run(spec).expect("run should start");
    let run_id = run.id().as_str().to_string();
    runtime
      .recording()
      .handle()
      .finish_run(
        run,
        auv_tracing_driver::run_builder::RunFinish {
          status_code: auv_tracing_driver::trace::TraceStatusCode::Ok,
          summary: Some("attr".to_string()),
          failure: None,
        },
      )
      .expect("run should finish");

    let canonical = runtime.recording().read_run(&run_id).expect("run snapshot should read");
    let attrs = &canonical.run.attributes;
    assert_eq!(attrs.get(auv_tracing_driver::trace::RUN_ATTR_DEVICE_ID), Some(&json!("local")));
    assert_eq!(attrs.get(auv_tracing_driver::trace::RUN_ATTR_SESSION_ID), Some(&json!("scan")));

    // Old on-disk layout invariant: `.auv/runs/{run_id}/` directory, no
    // per-device or per-session subdir inserted.
    let run_dir = store_root.join("runs").join(&run_id);
    assert!(run_dir.exists(), "run dir must remain at runs/{{run_id}}");

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  fn runtime_with_success_driver(project_root: PathBuf, store_root: PathBuf) -> Runtime {
    Runtime::new(project_root, LocalStore::new(store_root).expect("store should initialize"))
  }
}
