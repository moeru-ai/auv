use auv_tracing_driver::{
  AuvResult, RecordingRun, RunFinish, RunRecordingBackend, RunSpec, RunType, SpanFinish, SpanRef,
  TraceStatusCode, running_span_record, string_attr,
};

use crate::{
  InvokeCommand, InvokeCommandInput, InvokeRegistry, InvokeRequest, InvokeResult, RunStatus,
};

pub fn invoke_recorded(
  recording: &RunRecordingBackend,
  registry: &InvokeRegistry,
  request: InvokeRequest,
) -> AuvResult<InvokeResult> {
  let mut run = recording
    .handle()
    .start_run(RunSpec::new(RunType::Command, "auv.command"))?;
  let root = run.root_span();
  let result = match invoke_recorded_in_span(recording, registry, &mut run, &root, request) {
    Ok(result) => result,
    Err(error) => {
      if let Err(finish_error) = recording.handle().finish_run(
        run,
        RunFinish {
          status_code: TraceStatusCode::Error,
          summary: Some(format!(
            "Invocation failed. Inspect the run for details: {error}"
          )),
          failure: Some(error.clone()),
        },
      ) {
        return Err(format!(
          "{error}; additionally failed to persist failed run: {finish_error}"
        ));
      }
      return Err(error);
    }
  };
  let status_code = if result.status == RunStatus::Completed {
    TraceStatusCode::Ok
  } else {
    TraceStatusCode::Error
  };
  recording.handle().finish_run(
    run,
    RunFinish {
      status_code,
      summary: Some(result.output_summary.clone()),
      failure: result.failure_message.clone(),
    },
  )?;
  Ok(result)
}

pub fn invoke_recorded_in_span(
  recording: &RunRecordingBackend,
  registry: &InvokeRegistry,
  run: &mut RecordingRun,
  parent: &SpanRef,
  request: InvokeRequest,
) -> AuvResult<InvokeResult> {
  let command_id = request.command_id.clone();
  let command = registry.resolve(&command_id).ok_or_else(|| {
    format!(
      "unknown command {command_id}; use `auv-cli invoke --help` to inspect available entries"
    )
  })?;
  invoke_resolved_recorded_in_span(recording, run, parent, command, request)
}

pub fn invoke_resolved_recorded_in_span(
  recording: &RunRecordingBackend,
  run: &mut RecordingRun,
  parent: &SpanRef,
  command: &InvokeCommand,
  request: InvokeRequest,
) -> AuvResult<InvokeResult> {
  let command_span = run.start_span(
    parent,
    running_span_record(
      "auv.command.invoke",
      command_attributes(command.id, request.target.application_id.as_deref()),
    ),
  )?;
  record_event(
    run,
    command_span.id(),
    "command.resolved",
    Some(format!("resolved {}", command.id)),
  );

  let output = match command.invoke(InvokeCommandInput {
    command_id: command.id,
    target_application_id: request.target.application_id.as_deref(),
    inputs: &request.inputs,
    dry_run: request.dry_run,
  }) {
    Ok(output) => output,
    Err(error) => {
      let failure_message = format!("command {} handler failed: {error}", command.id);
      let output_summary = format!(
        "Command invocation failed after run creation. Inspect {} for the recorded trace.",
        run.id()
      );
      record_event(
        run,
        command_span.id(),
        "command.failed",
        Some(failure_message.clone()),
      );
      run.finish_span(
        &command_span,
        SpanFinish {
          status_code: TraceStatusCode::Error,
          summary: Some(output_summary.clone()),
          failure: Some(failure_message.clone()),
        },
      )?;
      return Ok(InvokeResult {
        run_id: run.id().to_string(),
        producer_span_id: command_span.id().clone(),
        status: RunStatus::Failed,
        output_summary,
        signals: Default::default(),
        artifacts: Vec::new(),
        artifact_paths: Vec::new(),
        failure_message: Some(failure_message),
      });
    }
  };

  if let Some(backend) = &output.backend {
    record_event(
      run,
      command_span.id(),
      "command.backend",
      Some(format!("backend={backend}")),
    );
  }

  for note in &output.notes {
    record_event(run, command_span.id(), "command.note", Some(note.clone()));
  }

  if let Some(verification) = &output.verification {
    record_event(
      run,
      command_span.id(),
      "command.verification",
      Some(verification.clone()),
    );
  }

  for known_limit in &output.known_limits {
    record_event(
      run,
      command_span.id(),
      "command.known_limit",
      Some(known_limit.clone()),
    );
  }

  let artifact_result = recording.record_produced_artifacts(run, &command_span, output.artifacts);
  let (artifact_records, artifact_paths) = match artifact_result {
    Ok(recorded) => (recorded.records, recorded.paths),
    Err(failure) => {
      let failure_message = format!(
        "command {} artifact recording failed: {}",
        command.id, failure.message
      );
      let output_summary = format!(
        "Command invocation produced output, but artifact recording failed. Inspect {} for the recorded trace.",
        run.id()
      );
      run.finish_span(
        &command_span,
        SpanFinish {
          status_code: TraceStatusCode::Error,
          summary: Some(output_summary.clone()),
          failure: Some(failure_message.clone()),
        },
      )?;
      return Ok(InvokeResult {
        run_id: run.id().to_string(),
        producer_span_id: command_span.id().clone(),
        status: RunStatus::Failed,
        output_summary,
        signals: output.signals,
        artifacts: failure.recorded.records,
        artifact_paths: failure.recorded.paths,
        failure_message: Some(failure_message),
      });
    }
  };

  record_event(
    run,
    command_span.id(),
    "run.completed",
    Some(output.summary.clone()),
  );

  run.finish_span(
    &command_span,
    SpanFinish {
      status_code: TraceStatusCode::Ok,
      summary: Some(output.summary.clone()),
      failure: None,
    },
  )?;

  Ok(InvokeResult {
    run_id: run.id().to_string(),
    producer_span_id: command_span.id().clone(),
    status: RunStatus::Completed,
    output_summary: output.summary,
    signals: output.signals,
    artifacts: artifact_records,
    artifact_paths,
    failure_message: None,
  })
}

fn command_attributes(
  command_id: &str,
  target_application_id: Option<&str>,
) -> auv_tracing_driver::Attributes {
  let mut attributes = auv_tracing_driver::Attributes::new();
  attributes.insert("command_id".to_string(), string_attr(command_id));
  attributes.insert("auv.command.id".to_string(), string_attr(command_id));
  if let Some(target_application_id) = target_application_id {
    attributes.insert(
      "target_application_id".to_string(),
      string_attr(target_application_id),
    );
    attributes.insert(
      "auv.target.application_id".to_string(),
      string_attr(target_application_id),
    );
  }
  attributes
}

fn record_event(
  run: &mut RecordingRun,
  span_id: &auv_tracing_driver::SpanId,
  name: &str,
  message: Option<String>,
) {
  run.record_event_in_span(span_id, name, message, Vec::new());
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;
  use std::env;
  use std::fs;
  use std::path::PathBuf;
  use std::sync::Arc;

  use auv_tracing_driver::{
    LocalStore, MemoryRunRecorder, RunRecordingBackend, RunType, RunUpdate, TraceStatusCode,
  };
  use serde_json::json;

  use crate::{
    CommandGroup, ExecutionTarget, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult,
    InvokeNamespace, InvokeRegistry, InvokeRequest, RunStatus, arg::NO_ARGS,
  };

  const FIXTURE_COMMAND_ID: &str = "fixture.recorded";
  const FAILING_COMMAND_ID: &str = "fixture.failing";

  fn temp_dir(label: &str) -> PathBuf {
    let path = env::temp_dir().join(format!(
      "auv-cli-invoke-{label}-{}-{}",
      std::process::id(),
      auv_tracing_driver::now_millis()
    ));
    let _ = fs::remove_dir_all(&path);
    path
  }

  fn recording(label: &str) -> (RunRecordingBackend, PathBuf) {
    let store_root = temp_dir(label);
    let backend = RunRecordingBackend::new(
      LocalStore::new(store_root.clone()).expect("store should create"),
      Arc::new(MemoryRunRecorder::new()),
    );
    (backend, store_root)
  }

  fn fixture_handler(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
    let mut output = InvokeCommandOutput::new("fixture observed");
    output
      .signals
      .insert("fixture".to_string(), "observed".to_string());
    Ok(output)
  }

  fn failing_handler(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
    Err("boom".to_string())
  }

  fn fixture_registry() -> InvokeRegistry {
    InvokeRegistry::from_groups(vec![CommandGroup::new("fixture", "FIXTURE").command(
      crate::command::spec(
        FIXTURE_COMMAND_ID,
        InvokeNamespace::Fixture,
        "Fixture recorded invoke command.",
        NO_ARGS,
        fixture_handler,
      ),
    )])
  }

  fn failing_registry() -> InvokeRegistry {
    InvokeRegistry::from_groups(vec![CommandGroup::new("fixture", "FIXTURE").command(
      crate::command::spec(
        FAILING_COMMAND_ID,
        InvokeNamespace::Fixture,
        "Failing recorded invoke command.",
        NO_ARGS,
        failing_handler,
      ),
    )])
  }

  #[test]
  fn invoke_recorded_records_successful_handler_output() {
    let (recording, store_root) = recording("success");
    let registry = fixture_registry();

    let result = super::invoke_recorded(
      &recording,
      &registry,
      InvokeRequest {
        command_id: FIXTURE_COMMAND_ID.to_string(),
        target: ExecutionTarget::default(),
        inputs: BTreeMap::new(),
        dry_run: false,
      },
    )
    .expect("invoke should succeed");

    assert_eq!(result.status, RunStatus::Completed);
    assert_eq!(result.output_summary, "fixture observed");
    let canonical = recording
      .read_run(result.run_id.as_str())
      .expect("run should persist");
    assert_eq!(canonical.run.run_type, RunType::Command);
    assert_eq!(canonical.run.status_code, TraceStatusCode::Ok);
    let command_span = canonical
      .spans
      .iter()
      .find(|span| span.name == "auv.command.invoke")
      .expect("command span should be recorded");
    assert_eq!(result.producer_span_id, command_span.span_id);
    assert_eq!(
      command_span.attributes.get("auv.command.id"),
      Some(&json!(FIXTURE_COMMAND_ID))
    );
    assert!(
      canonical
        .events
        .iter()
        .any(|event| event.span_id == command_span.span_id && event.name == "command.resolved")
    );
    assert!(
      canonical
        .events
        .iter()
        .any(|event| event.span_id == command_span.span_id && event.name == "run.completed")
    );

    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn invoke_recorded_records_handler_failure_as_failed_result() {
    let (recording, store_root) = recording("handler-failure");
    let registry = failing_registry();

    let result = super::invoke_recorded(
      &recording,
      &registry,
      InvokeRequest {
        command_id: FAILING_COMMAND_ID.to_string(),
        target: ExecutionTarget::default(),
        inputs: BTreeMap::new(),
        dry_run: false,
      },
    )
    .expect("handler failure should return an inspectable result");

    assert_eq!(result.status, RunStatus::Failed);
    assert!(
      result
        .failure_message
        .as_deref()
        .is_some_and(|message| message.contains("handler failed: boom"))
    );
    let canonical = recording
      .read_run(result.run_id.as_str())
      .expect("run should persist");
    let command_span = canonical
      .spans
      .iter()
      .find(|span| span.name == "auv.command.invoke")
      .expect("command span should be recorded");
    assert_eq!(command_span.status_code, TraceStatusCode::Error);
    assert!(
      command_span
        .failure
        .as_ref()
        .is_some_and(|failure| failure.message.contains("handler failed: boom"))
    );

    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn invoke_recorded_rejects_unknown_command_and_finishes_failed_run() {
    let store_root = temp_dir("unknown-command");
    let recorder = Arc::new(MemoryRunRecorder::new());
    let recording = RunRecordingBackend::new(
      LocalStore::new(store_root.clone()).expect("store should create"),
      recorder.clone(),
    );
    let registry = InvokeRegistry::from_groups(Vec::new());

    let error = super::invoke_recorded(
      &recording,
      &registry,
      InvokeRequest {
        command_id: "missing.command".to_string(),
        target: ExecutionTarget::default(),
        inputs: BTreeMap::new(),
        dry_run: false,
      },
    )
    .expect_err("unknown command should fail");

    assert!(error.contains("auv-cli invoke --help"));
    let run_id = recorder
      .drain_for_test()
      .into_iter()
      .find_map(|update| match update {
        RunUpdate::RunFinished { run, .. } => Some(run.run_id),
        _ => None,
      })
      .expect("failed implicit run should finish");
    let canonical = recording
      .read_run(run_id.as_str())
      .expect("failed run snapshot should persist");
    assert_eq!(canonical.run.status_code, TraceStatusCode::Error);
    assert!(
      canonical
        .run
        .failure
        .as_ref()
        .is_some_and(|failure| failure.message.contains("missing.command"))
    );

    let _ = fs::remove_dir_all(store_root);
  }
}
