use std::path::PathBuf;

use crate::catalog::CommandCatalog;
use crate::driver::DriverRegistry;
use crate::model::{
  ArtifactRecord, AuvResult, DriverCall, DriverDescriptor, EventRecord, InvokeRequest,
  InvokeResult, RunRecord, RunStatus, new_run_id, now_millis,
};
use crate::store::LocalStore;

pub struct Runtime {
  project_root: PathBuf,
  commands: CommandCatalog,
  drivers: DriverRegistry,
  store: LocalStore,
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
    }
  }

  pub fn list_commands(&self) -> &[crate::model::CommandSpec] {
    self.commands.all()
  }

  pub fn list_drivers(&self) -> Vec<DriverDescriptor> {
    self.drivers.descriptors()
  }

  pub fn inspect(&self, run_id: &str) -> AuvResult<String> {
    self.store.render_inspection(run_id)
  }

  pub fn invoke(&self, request: InvokeRequest) -> AuvResult<InvokeResult> {
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

    let run_id = new_run_id();
    let mut run = RunRecord {
      run_id: run_id.clone(),
      command_id: command.id.to_string(),
      driver_id: command.driver_id.to_string(),
      operation: command.operation.to_string(),
      target_application_id: request.target.application_id.clone(),
      runtime_version: env!("CARGO_PKG_VERSION").to_string(),
      started_at_millis: now_millis(),
      finished_at_millis: None,
      status: RunStatus::Failed,
      inputs: request.inputs.clone(),
      output_summary: String::new(),
      events: Vec::new(),
      artifacts: Vec::new(),
    };

    push_event(
      &mut run.events,
      "run.created",
      format!("implicit run created for command {}", command.id),
    );
    push_event(
      &mut run.events,
      "command.resolved",
      format!(
        "resolved {} -> {}.{}",
        command.id, command.driver_id, command.operation
      ),
    );

    let call = DriverCall {
      operation: command.operation.to_string(),
      target: request.target,
      inputs: request.inputs,
      working_directory: self.project_root.clone(),
    };

    push_event(
      &mut run.events,
      "driver.invoke",
      format!("invoking {}.{}", command.driver_id, command.operation),
    );

    let mut failure_message = None;

    match driver.invoke(&call) {
      Ok(response) => {
        if let Some(backend) = &response.backend {
          push_event(
            &mut run.events,
            "driver.backend",
            format!("backend={backend}"),
          );
        }

        for note in &response.notes {
          push_event(&mut run.events, "driver.note", note.clone());
        }

        let mut persisted_artifacts = Vec::new();
        let mut artifact_failure = None;
        for (index, artifact) in response.artifacts.into_iter().enumerate() {
          match self.store.stage_artifact(&run.run_id, index, artifact) {
            Ok(stored_artifact) => {
              push_event(
                &mut run.events,
                "artifact.captured",
                render_artifact_event(&stored_artifact),
              );
              persisted_artifacts.push(stored_artifact);
            }
            Err(error) => {
              push_event(
                &mut run.events,
                "artifact.failed",
                format!("artifact staging failed: {error}"),
              );
              artifact_failure = Some(error);
              break;
            }
          }
        }

        run.artifacts = persisted_artifacts;
        if let Some(error) = artifact_failure {
          run.status = RunStatus::Failed;
          run.output_summary = format!(
            "Artifact staging failed after run creation. Inspect {} for the recorded trace.",
            run.run_id
          );
          failure_message = Some(error.clone());
          push_event(
            &mut run.events,
            "run.failed",
            format!("artifact staging failed after driver success: {error}"),
          );
        } else {
          run.status = RunStatus::Completed;
          run.output_summary = response.summary.clone();
          push_event(&mut run.events, "run.completed", response.summary);
        }
      }
      Err(error) => {
        run.status = RunStatus::Failed;
        run.output_summary = format!(
          "Driver invocation failed after run creation. Inspect {} for the recorded trace.",
          run.run_id
        );
        failure_message = Some(error.clone());
        push_event(&mut run.events, "driver.failed", error);
      }
    }

    run.finished_at_millis = Some(now_millis());
    self.store.persist_run(&run)?;

    let artifact_paths = run
      .artifacts
      .iter()
      .map(|artifact| artifact.path.clone())
      .collect::<Vec<_>>();
    let run_id = run.run_id.clone();
    let status = run.status.clone();
    let output_summary = run.output_summary.clone();

    Ok(InvokeResult {
      run_id,
      status,
      output_summary,
      artifact_paths,
      failure_message,
    })
  }
}

fn push_event(events: &mut Vec<EventRecord>, kind: &str, message: String) {
  events.push(EventRecord {
    at_millis: now_millis(),
    kind: kind.to_string(),
    message,
  });
}

fn render_artifact_event(artifact: &ArtifactRecord) -> String {
  let note = artifact.note.clone().unwrap_or_else(|| "n/a".to_string());
  format!(
    "{} kind={} path={} note={}",
    artifact.id,
    artifact.kind,
    artifact.path.display(),
    note
  )
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
}
