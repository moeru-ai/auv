use std::path::PathBuf;
use std::sync::Arc;

use crate::catalog::CommandCatalog;
use crate::driver::DriverRegistry;
use crate::model::{
  AuvResult, DriverCall, DriverDescriptor, InvokeRequest, InvokeResult, RunStatus, now_millis,
};
use crate::recording::{MemoryRunEventSink, RunEventSink};
use crate::store::LocalStore;
use crate::trace::{
  EVENT_API_VERSION, EventRecordV1Alpha1, RUN_API_VERSION, RunId, RunRecordV1Alpha1, RunType,
  SPAN_API_VERSION, SpanRecordV1Alpha1, TraceFailure, TraceState, TraceStatusCode, new_event_id,
  new_run_id, new_span_id, new_trace_id, string_attr,
};

pub struct Runtime {
  project_root: PathBuf,
  commands: CommandCatalog,
  drivers: DriverRegistry,
  store: LocalStore,
  event_sink: Arc<dyn RunEventSink>,
}

impl Runtime {
  pub fn new(
    project_root: PathBuf,
    commands: CommandCatalog,
    drivers: DriverRegistry,
    store: LocalStore,
  ) -> Self {
    Self {
      project_root,
      commands,
      drivers,
      store,
      event_sink: Arc::new(MemoryRunEventSink::new()),
    }
  }

  pub fn list_commands(&self) -> &[crate::model::CommandSpec] {
    self.commands.all()
  }

  pub fn list_drivers(&self) -> Vec<DriverDescriptor> {
    self.drivers.descriptors()
  }

  pub fn inspect(&self, run_id: &str) -> AuvResult<String> {
    let canonical = self.store.read_run(run_id)?;
    Ok(render_canonical_inspection(&canonical))
  }

  pub fn read_run(&self, run_id: &str) -> AuvResult<crate::store::CanonicalRun> {
    self.store.read_run(run_id)
  }

  pub fn event_sink(&self) -> Arc<dyn RunEventSink> {
    self.event_sink.clone()
  }

  pub fn with_event_sink(mut self, event_sink: Arc<dyn RunEventSink>) -> Self {
    self.event_sink = event_sink;
    self
  }

  pub fn start_run(
    &self,
    spec: crate::recording::RunSpec,
  ) -> AuvResult<crate::recording::RecordingRun> {
    let run_id = new_run_id();
    let root_span_id = new_span_id();
    let started = now_millis();
    let run = RunRecordV1Alpha1 {
      api_version: RUN_API_VERSION.to_string(),
      run_id: run_id.clone(),
      trace_id: new_trace_id(),
      run_type: spec.run_type,
      state: TraceState::Running,
      status_code: TraceStatusCode::Unset,
      started_at_millis: started,
      finished_at_millis: None,
      root_span_id: root_span_id.clone(),
      attributes: spec.attributes.clone(),
      summary: None,
      failure: None,
    };
    let root_span = SpanRecordV1Alpha1 {
      api_version: SPAN_API_VERSION.to_string(),
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
    Ok(crate::recording::RecordingRun::new(
      run,
      root_span,
      self.event_sink.clone(),
    ))
  }

  pub fn finish_run(
    &self,
    run: crate::recording::RecordingRun,
    finish: crate::recording::RunFinish,
  ) -> AuvResult<RunId> {
    let failure = finish.failure.map(|message| TraceFailure { message });
    let recorded = run.finish(finish.status_code, finish.summary, failure);
    let run_id = recorded.snapshot.run.run_id.clone();
    self.store.write_run_snapshot(&recorded.snapshot)?;
    self
      .event_sink
      .on_event(crate::recording::RunStreamEvent::RunFinished {
        run_id: run_id.clone(),
        run: recorded.snapshot.run,
      });
    Ok(run_id)
  }

  pub fn invoke(&self, request: InvokeRequest) -> AuvResult<InvokeResult> {
    let mut run = self.start_run(crate::recording::RunSpec::new(
      RunType::Command,
      "auv.command",
    ))?;
    let root = run.root_span();
    let result = self.invoke_in_span(&mut run, &root, request)?;
    let status_code = if result.status == RunStatus::Completed {
      TraceStatusCode::Ok
    } else {
      TraceStatusCode::Error
    };
    self.finish_run(
      run,
      crate::recording::RunFinish {
        status_code,
        summary: Some(result.output_summary.clone()),
        failure: result.failure_message.clone(),
      },
    )?;
    Ok(result)
  }

  pub fn invoke_in_span(
    &self,
    run: &mut crate::recording::RecordingRun,
    parent: &crate::recording::SpanRef,
    request: InvokeRequest,
  ) -> AuvResult<InvokeResult> {
    let command = self.commands.resolve(&request.command_id).ok_or_else(|| {
      format!(
        "unknown command {}; use `list-commands` to see available entries",
        request.command_id
      )
    })?;
    let driver = self.drivers.get(command.driver_id).ok_or_else(|| {
      format!(
        "command {} resolved to missing driver {}",
        command.id, command.driver_id
      )
    })?;

    let command_span = run.start_span(
      parent,
      span_record(
        "auv.command.invoke",
        command_attributes(
          command.id,
          command.driver_id,
          command.operation,
          request.target.application_id.as_deref(),
        ),
      ),
    );
    record_event(
      run,
      command_span.id(),
      "command.resolved",
      Some(format!(
        "resolved {} -> {}.{}",
        command.id, command.driver_id, command.operation
      )),
    );

    let call = DriverCall {
      operation: command.operation.to_string(),
      target: request.target,
      inputs: request.inputs,
      working_directory: self.project_root.clone(),
    };

    let driver_span = run.start_span(
      &command_span,
      span_record(
        "auv.driver.invoke",
        command_attributes(
          command.id,
          command.driver_id,
          command.operation,
          call.target.application_id.as_deref(),
        ),
      ),
    );
    record_event(
      run,
      driver_span.id(),
      "driver.invoke",
      Some(format!(
        "invoking {}.{}",
        command.driver_id, command.operation
      )),
    );

    let mut artifact_paths = Vec::new();

    let (status, output_summary, failure_message) = match driver.invoke(&call) {
      Ok(response) => {
        if let Some(backend) = &response.backend {
          record_event(
            run,
            driver_span.id(),
            "driver.backend",
            Some(format!("backend={backend}")),
          );
        }

        for note in &response.notes {
          record_event(run, driver_span.id(), "driver.note", Some(note.clone()));
        }

        let mut artifact_failure = None;
        for (index, artifact) in response.artifacts.into_iter().enumerate() {
          match self
            .store
            .stage_artifact(run.id(), index, artifact, driver_span.id(), None)
          {
            Ok(stored_artifact) => {
              record_event(
                run,
                driver_span.id(),
                "artifact.captured",
                Some(render_artifact_event(&stored_artifact)),
              );
              artifact_paths.push(PathBuf::from(&stored_artifact.path));
              run.record_artifact(stored_artifact);
            }
            Err(error) => {
              record_event(
                run,
                driver_span.id(),
                "artifact.failed",
                Some(format!("artifact staging failed: {error}")),
              );
              artifact_failure = Some(error);
              break;
            }
          }
        }

        if let Some(error) = artifact_failure {
          let output_summary = format!(
            "Artifact staging failed after run creation. Inspect {} for the recorded trace.",
            run.id()
          );
          record_event(
            run,
            driver_span.id(),
            "run.failed",
            Some(format!(
              "artifact staging failed after driver success: {error}"
            )),
          );
          (RunStatus::Failed, output_summary, Some(error))
        } else {
          let output_summary = response.summary.clone();
          record_event(
            run,
            command_span.id(),
            "run.completed",
            Some(response.summary),
          );
          (RunStatus::Completed, output_summary, None)
        }
      }
      Err(error) => {
        let output_summary = format!(
          "Driver invocation failed after run creation. Inspect {} for the recorded trace.",
          run.id()
        );
        record_event(run, driver_span.id(), "driver.failed", Some(error.clone()));
        (RunStatus::Failed, output_summary, Some(error))
      }
    };

    let status_code = if status == RunStatus::Completed {
      TraceStatusCode::Ok
    } else {
      TraceStatusCode::Error
    };
    let span_failure = failure_message.clone();
    run.finish_span(
      &driver_span,
      crate::recording::SpanFinish {
        status_code,
        summary: Some(output_summary.clone()),
        failure: span_failure.clone(),
      },
    );
    run.finish_span(
      &command_span,
      crate::recording::SpanFinish {
        status_code,
        summary: Some(output_summary.clone()),
        failure: span_failure,
      },
    );

    Ok(InvokeResult {
      run_id: run.id().to_string(),
      status,
      output_summary,
      artifact_paths,
      failure_message,
    })
  }
}

fn span_record(
  name: impl Into<String>,
  attributes: crate::recording::Attributes,
) -> SpanRecordV1Alpha1 {
  SpanRecordV1Alpha1 {
    api_version: SPAN_API_VERSION.to_string(),
    span_id: new_span_id(),
    parent_span_id: None,
    name: name.into(),
    state: TraceState::Running,
    status_code: TraceStatusCode::Unset,
    started_at_millis: now_millis(),
    finished_at_millis: None,
    attributes,
    summary: None,
    failure: None,
  }
}

fn command_attributes(
  command_id: &str,
  driver_id: &str,
  operation: &str,
  target_application_id: Option<&str>,
) -> crate::recording::Attributes {
  let mut attributes = crate::recording::Attributes::new();
  attributes.insert("command_id".to_string(), string_attr(command_id));
  attributes.insert("driver_id".to_string(), string_attr(driver_id));
  attributes.insert("operation".to_string(), string_attr(operation));
  if let Some(target_application_id) = target_application_id {
    attributes.insert(
      "target_application_id".to_string(),
      string_attr(target_application_id),
    );
  }
  attributes
}

fn record_event(
  run: &mut crate::recording::RecordingRun,
  span_id: &crate::trace::SpanId,
  name: &str,
  message: Option<String>,
) {
  run.record_event(EventRecordV1Alpha1 {
    api_version: EVENT_API_VERSION.to_string(),
    event_id: new_event_id(),
    span_id: span_id.clone(),
    name: name.to_string(),
    timestamp_millis: now_millis(),
    attributes: Default::default(),
    message,
    artifact_ids: Vec::new(),
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

fn render_canonical_inspection(snapshot: &crate::store::CanonicalRun) -> String {
  let status = match snapshot.run.status_code {
    TraceStatusCode::Ok => "completed",
    TraceStatusCode::Error => "failed",
    TraceStatusCode::Unset => "running",
  };
  let mut output = format!(
    "Run: {}\nStatus: {}\nRun Type: {}\n",
    snapshot.run.run_id,
    status,
    snapshot.run.run_type.as_str()
  );
  if let Some(summary) = &snapshot.run.summary {
    output.push_str(&format!("Summary: {summary}\n"));
  }
  if let Some(failure) = &snapshot.run.failure {
    output.push_str(&format!("Failure: {}\n", failure.message));
  }
  output.push_str("\nSpans:\n");
  for span in &snapshot.spans {
    output.push_str(&format!(
      "- {} {} parent={}\n",
      span.span_id,
      span.name,
      span
        .parent_span_id
        .as_ref()
        .map(|span_id| span_id.as_str())
        .unwrap_or("n/a")
    ));
  }
  output.push_str("\nEvents:\n");
  for event in &snapshot.events {
    output.push_str(&format!(
      "- {} {} {}\n",
      event.span_id,
      event.name,
      event.message.as_deref().unwrap_or("")
    ));
  }
  output.push_str("\nArtifacts:\n");
  for artifact in &snapshot.artifacts {
    output.push_str(&format!(
      "- {} {} {}\n",
      artifact.artifact_id, artifact.role, artifact.path
    ));
  }
  output
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;
  use std::env;
  use std::fs;
  use std::path::PathBuf;

  use super::Runtime;
  use crate::catalog::CommandCatalog;
  use crate::driver::{Driver, DriverRegistry};
  use crate::model::{
    AuvResult, CommandSpec, DriverCall, DriverDescriptor, DriverResponse, ExecutionTarget,
    InvokeRequest, ProducedArtifact, RunStatus, now_millis,
  };
  use crate::store::LocalStore;

  struct ArtifactFailureDriver;
  struct SuccessDriver;

  impl Driver for ArtifactFailureDriver {
    fn descriptor(&self) -> DriverDescriptor {
      DriverDescriptor {
        id: "test.driver",
        summary: "Test driver",
        capabilities: &["test.artifact-failure"],
        donor_boundary: "test-only",
      }
    }

    fn invoke(&self, _call: &DriverCall) -> AuvResult<DriverResponse> {
      Ok(DriverResponse {
        summary: "driver succeeded before artifact staging".to_string(),
        backend: Some("test.backend".to_string()),
        notes: vec!["note".to_string()],
        artifacts: vec![ProducedArtifact {
          kind: "text".to_string(),
          source_path: PathBuf::from("/definitely/missing/artifact.txt"),
          preferred_name: "artifact.txt".to_string(),
          note: Some("missing".to_string()),
        }],
      })
    }
  }

  impl Driver for SuccessDriver {
    fn descriptor(&self) -> DriverDescriptor {
      DriverDescriptor {
        id: "test.driver",
        summary: "Test driver",
        capabilities: &["test.success"],
        donor_boundary: "test-only",
      }
    }

    fn invoke(&self, _call: &DriverCall) -> AuvResult<DriverResponse> {
      Ok(DriverResponse {
        summary: "driver ok".to_string(),
        backend: Some("test.backend".to_string()),
        notes: vec![],
        artifacts: vec![],
      })
    }
  }

  #[test]
  fn invoke_in_span_adds_command_under_parent_span() {
    let project_root = temp_dir("runtime-recorded-project");
    let store_root = temp_dir("runtime-recorded-store");
    let runtime = runtime_with_success_driver(project_root.clone(), store_root.clone());

    let mut run = runtime
      .start_run(crate::recording::RunSpec::new(
        crate::trace::RunType::Execute,
        "auv.execute",
      ))
      .expect("run should start");
    let parent = run.root_span();
    let result = runtime
      .invoke_in_span(
        &mut run,
        &parent,
        InvokeRequest {
          command_id: "test.invoke".to_string(),
          target: ExecutionTarget::default(),
          inputs: BTreeMap::new(),
        },
      )
      .expect("recorded invoke should succeed");
    assert_eq!(result.status, RunStatus::Completed);
    let run_id = runtime
      .finish_run(
        run,
        crate::recording::RunFinish {
          status_code: crate::trace::TraceStatusCode::Ok,
          summary: Some("done".to_string()),
          failure: None,
        },
      )
      .expect("run should finish");

    let canonical = runtime.read_run(run_id.as_str()).expect("run should read");
    assert_eq!(canonical.run.run_type, crate::trace::RunType::Execute);
    assert!(
      canonical
        .spans
        .iter()
        .any(|span| span.name == "auv.command.invoke")
    );
    assert!(
      canonical
        .spans
        .iter()
        .any(|span| span.parent_span_id.as_ref() == Some(parent.id()))
    );

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn invoke_persists_failed_run_when_artifact_staging_breaks() {
    let project_root = temp_dir("runtime-tests-project");
    let store_root = temp_dir("runtime-tests-store");
    let runtime = Runtime::new(
      project_root.clone(),
      CommandCatalog::new(vec![CommandSpec {
        id: "test.invoke",
        summary: "Test invoke",
        driver_id: "test.driver",
        operation: "test_operation",
        disturbance_classes: &[crate::model::DisturbanceClass::None],
        max_disturbance: crate::model::DisturbanceClass::None,
      }]),
      DriverRegistry::new(vec![Box::new(ArtifactFailureDriver)]),
      LocalStore::new(store_root.clone()).expect("store should initialize"),
    );

    let result = runtime
      .invoke(InvokeRequest {
        command_id: "test.invoke".to_string(),
        target: ExecutionTarget::default(),
        inputs: BTreeMap::new(),
      })
      .expect("artifact staging failures should still return an inspectable run");

    assert_eq!(result.status, RunStatus::Failed);
    assert!(result.failure_message.is_some());

    let inspection = runtime
      .inspect(&result.run_id)
      .expect("failed run should still be inspectable");
    assert!(inspection.contains("Status: failed"));
    assert!(inspection.contains("artifact staging failed"));

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  fn temp_dir(label: &str) -> PathBuf {
    let path = env::temp_dir().join(format!("auv-{}-{}", label, now_millis()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("temp dir should be creatable");
    path
  }

  fn runtime_with_success_driver(project_root: PathBuf, store_root: PathBuf) -> Runtime {
    Runtime::new(
      project_root,
      CommandCatalog::new(vec![CommandSpec {
        id: "test.invoke",
        summary: "Test invoke",
        driver_id: "test.driver",
        operation: "test_operation",
        disturbance_classes: &[crate::model::DisturbanceClass::None],
        max_disturbance: crate::model::DisturbanceClass::None,
      }]),
      DriverRegistry::new(vec![Box::new(SuccessDriver)]),
      LocalStore::new(store_root).expect("store should initialize"),
    )
  }
}
