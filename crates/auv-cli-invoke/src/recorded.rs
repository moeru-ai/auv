use auv_tracing_driver::{
  AuvResult, RecordingRun, RunFinish, RunRecordingBackend, RunSpec, RunType, SessionId, SpanFinish, SpanRef, TraceStatusCode,
  running_span_record, string_attr,
};

use crate::{InvokeCommand, InvokeCommandInput, InvokeRegistry, InvokeRequest, InvokeResult, RunStatus};

/// Product-specific work performed after handler artifacts are recorded and
/// before the command span and run status are finalized.
///
/// A hook may append artifacts or update `InvokeResult` status. Returning an
/// error records the command/run as failed and returns that error to the caller.
pub type InvokeFinalizeHook = dyn Fn(&RunRecordingBackend, &mut RecordingRun, &SpanRef, &mut InvokeResult) -> AuvResult<()> + Send + Sync;

/// Run a recorded command invoke under the default session.
///
/// The recorded invoke path owns the default `session_id` so CLI/MCP callers
/// that have no session concept still produce a session-scoped run (see
/// `docs/TERMS_AND_CONCEPTS.md`: every run carries a `session_id`; the runtime
/// owns the default). Callers that can name a session use
/// [`invoke_recorded_with_session`].
pub fn invoke_recorded(recording: &RunRecordingBackend, registry: &InvokeRegistry, request: InvokeRequest) -> AuvResult<InvokeResult> {
  invoke_recorded_with_session_and_finalize(recording, registry, request, SessionId::default_session(), None)
}

/// Run a recorded command invoke under the default session with an in-lifecycle
/// finalize hook.
pub fn invoke_recorded_with_finalize(
  recording: &RunRecordingBackend,
  registry: &InvokeRegistry,
  request: InvokeRequest,
  finalize: &InvokeFinalizeHook,
) -> AuvResult<InvokeResult> {
  invoke_recorded_with_session_and_finalize(recording, registry, request, SessionId::default_session(), Some(finalize))
}

/// Run a recorded command invoke and stamp `session` onto the recorded run.
///
/// This is the session-aware invoke seam: it threads an explicit caller-chosen
/// `session_id` through the existing run/trace session plumbing
/// (`RunSpec::with_session`) so a session-aware frontend can run invoke for a
/// specific session instead of relying on the runtime default. The session
/// applies only to the new run this entrypoint starts; the in-span variants
/// inherit their session from the parent run they record under.
pub fn invoke_recorded_with_session(
  recording: &RunRecordingBackend,
  registry: &InvokeRegistry,
  request: InvokeRequest,
  session: SessionId,
) -> AuvResult<InvokeResult> {
  invoke_recorded_with_session_and_finalize(recording, registry, request, session, None)
}

fn invoke_recorded_with_session_and_finalize(
  recording: &RunRecordingBackend,
  registry: &InvokeRegistry,
  request: InvokeRequest,
  session: SessionId,
  finalize: Option<&InvokeFinalizeHook>,
) -> AuvResult<InvokeResult> {
  let mut run = recording.handle().start_run(RunSpec::new(RunType::Command, "auv.command").with_session(session))?;
  let root = run.root_span();
  let result = match invoke_recorded_in_span_with_finalize(recording, registry, &mut run, &root, request, finalize) {
    Ok(result) => result,
    Err(error) => {
      if let Err(finish_error) = recording.handle().finish_run(
        run,
        RunFinish {
          status_code: TraceStatusCode::Error,
          summary: Some(format!("Invocation failed. Inspect the run for details: {error}")),
          failure: Some(error.clone()),
        },
      ) {
        return Err(format!("{error}; additionally failed to persist failed run: {finish_error}"));
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
      failure: result.failure_message.clone().or_else(|| (result.status == RunStatus::Failed).then(|| result.output_summary.clone())),
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
  invoke_recorded_in_span_with_finalize(recording, registry, run, parent, request, None)
}

fn invoke_recorded_in_span_with_finalize(
  recording: &RunRecordingBackend,
  registry: &InvokeRegistry,
  run: &mut RecordingRun,
  parent: &SpanRef,
  request: InvokeRequest,
  finalize: Option<&InvokeFinalizeHook>,
) -> AuvResult<InvokeResult> {
  let command_id = request.command_id.clone();
  let command = registry
    .resolve(&command_id)
    .ok_or_else(|| format!("unknown command {command_id}; use `auv invoke --help` to inspect available entries"))?;
  invoke_resolved_recorded_in_span_with_finalize(recording, run, parent, command, request, finalize)
}

pub fn invoke_resolved_recorded_in_span(
  recording: &RunRecordingBackend,
  run: &mut RecordingRun,
  parent: &SpanRef,
  command: &InvokeCommand,
  request: InvokeRequest,
) -> AuvResult<InvokeResult> {
  invoke_resolved_recorded_in_span_with_finalize(recording, run, parent, command, request, None)
}

fn invoke_resolved_recorded_in_span_with_finalize(
  recording: &RunRecordingBackend,
  run: &mut RecordingRun,
  parent: &SpanRef,
  command: &InvokeCommand,
  request: InvokeRequest,
  finalize: Option<&InvokeFinalizeHook>,
) -> AuvResult<InvokeResult> {
  let command_span = run.start_span(
    parent,
    running_span_record("auv.command.invoke", command_attributes(command.id, request.target.application_id.as_deref())),
  )?;
  record_event(run, command_span.id(), "command.resolved", Some(format!("resolved {}", command.id)));

  // NOTICE(task22-legacy-runtime): This adapter may be called from a runtime
  // thread, so registered handlers must remain runtime-agnostic until Task 22
  // removes the synchronous block_on surface. New frontends await handlers.
  let command_result = futures_executor::block_on(command.invoke(InvokeCommandInput {
    command_id: command.id.to_string(),
    target_application_id: request.target.application_id.clone(),
    inputs: request.inputs.clone(),
    dry_run: request.dry_run,
    cancellation: crate::InvokeCancellation::new(),
  }));
  let mut result = match command_result {
    Ok(output) => {
      if let Some(backend) = &output.backend {
        record_event(run, command_span.id(), "command.backend", Some(format!("backend={backend}")));
      }
      for note in &output.notes {
        record_event(run, command_span.id(), "command.note", Some(note.clone()));
      }
      if let Some(verification) = &output.verification {
        record_event(run, command_span.id(), "command.verification", Some(verification.clone()));
      }
      for known_limit in &output.known_limits {
        record_event(run, command_span.id(), "command.known_limit", Some(known_limit.clone()));
      }

      match recording.record_produced_artifacts(run, &command_span, output.artifacts) {
        Ok(recorded) => InvokeResult {
          run_id: run.id().to_string(),
          producer_span_id: Some(command_span.id().clone()),
          command_id: command.id.to_string(),
          command_summary: command.summary.to_string(),
          status: if output.failure_message.is_some() {
            RunStatus::Failed
          } else {
            RunStatus::Completed
          },
          output_summary: output.summary,
          backend: output.backend,
          signals: output.signals,
          notes: output.notes,
          known_limits: output.known_limits,
          verification: output.verification,
          report: output.report,
          artifacts: recorded.records,
          artifact_paths: recorded.paths,
          canonical_artifacts: Vec::new(),
          artifact_failures: output.artifact_failures,
          failure_message: output.failure_message,
        },
        Err(failure) => {
          let failure_message = format!("command {} artifact recording failed: {}", command.id, failure.message);
          let output_summary =
            format!("Command invocation produced output, but artifact recording failed. Inspect {} for the recorded trace.", run.id());
          InvokeResult {
            run_id: run.id().to_string(),
            producer_span_id: Some(command_span.id().clone()),
            command_id: command.id.to_string(),
            command_summary: command.summary.to_string(),
            status: RunStatus::Failed,
            output_summary,
            backend: output.backend,
            signals: output.signals,
            notes: output.notes,
            known_limits: output.known_limits,
            verification: output.verification,
            report: output.report,
            artifacts: failure.recorded.records,
            artifact_paths: failure.recorded.paths,
            canonical_artifacts: Vec::new(),
            artifact_failures: output.artifact_failures,
            failure_message: Some(failure_message),
          }
        }
      }
    }
    Err(error) => {
      let failure_message = format!("command {} handler failed: {error}", command.id);
      let output_summary = format!("Command invocation failed after run creation. Inspect {} for the recorded trace.", run.id());
      record_event(run, command_span.id(), "command.failed", Some(failure_message.clone()));
      InvokeResult {
        run_id: run.id().to_string(),
        producer_span_id: Some(command_span.id().clone()),
        command_id: command.id.to_string(),
        command_summary: command.summary.to_string(),
        status: RunStatus::Failed,
        output_summary,
        backend: None,
        signals: Default::default(),
        notes: Vec::new(),
        known_limits: Vec::new(),
        verification: None,
        report: None,
        artifacts: Vec::new(),
        artifact_paths: Vec::new(),
        canonical_artifacts: Vec::new(),
        artifact_failures: Vec::new(),
        failure_message: Some(failure_message),
      }
    }
  };

  if let Some(finalize) = finalize
    && let Err(error) = finalize(recording, run, &command_span, &mut result)
  {
    let failure_message = format!("command {} finalize failed: {error}", command.id);
    let output_summary = format!("Command invocation finalization failed after run creation. Inspect {} for the recorded trace.", run.id());
    record_event(run, command_span.id(), "command.finalize.failed", Some(failure_message.clone()));
    run.finish_span(
      &command_span,
      SpanFinish {
        status_code: TraceStatusCode::Error,
        summary: Some(output_summary),
        failure: Some(failure_message.clone()),
      },
    )?;
    return Err(failure_message);
  }

  let status_code = if result.status == RunStatus::Completed {
    TraceStatusCode::Ok
  } else {
    TraceStatusCode::Error
  };
  let event_name = if result.status == RunStatus::Completed {
    "run.completed"
  } else {
    "run.failed"
  };
  let failure = result.failure_message.clone().or_else(|| (result.status == RunStatus::Failed).then(|| result.output_summary.clone()));
  record_event(run, command_span.id(), event_name, Some(result.output_summary.clone()));

  run.finish_span(
    &command_span,
    SpanFinish {
      status_code,
      summary: Some(result.output_summary.clone()),
      failure,
    },
  )?;

  Ok(result)
}

fn command_attributes(command_id: &str, target_application_id: Option<&str>) -> auv_tracing_driver::Attributes {
  let mut attributes = auv_tracing_driver::Attributes::new();
  attributes.insert("command_id".to_string(), string_attr(command_id));
  attributes.insert("auv.command.id".to_string(), string_attr(command_id));
  if let Some(target_application_id) = target_application_id {
    attributes.insert("target_application_id".to_string(), string_attr(target_application_id));
    attributes.insert("auv.target.application_id".to_string(), string_attr(target_application_id));
  }
  attributes
}

fn record_event(run: &mut RecordingRun, span_id: &auv_tracing_driver::SpanId, name: &str, message: Option<String>) {
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
    LocalStore, MemoryRunRecorder, ProducedArtifact, RunFinish, RunRecordingBackend, RunSpec, RunType, RunUpdate, TraceStatusCode,
  };
  use serde_json::json;

  use crate::{
    CommandGroup, ExecutionTarget, InvokeCommandFuture, InvokeCommandInput, InvokeCommandOutput, InvokeNamespace, InvokeRegistry,
    InvokeReport, InvokeReportField, InvokeReportSection, InvokeRequest, RunStatus, arg::NO_ARGS,
  };

  const FIXTURE_COMMAND_ID: &str = "fixture.recorded";
  const FAILING_COMMAND_ID: &str = "fixture.failing";
  const ARTIFACT_FAILING_COMMAND_ID: &str = "fixture.artifactFailing";

  fn temp_dir(label: &str) -> PathBuf {
    let path = env::temp_dir().join(format!("auv-cli-invoke-{label}-{}-{}", std::process::id(), auv_tracing_driver::now_millis()));
    let _ = fs::remove_dir_all(&path);
    path
  }

  fn recording(label: &str) -> (RunRecordingBackend, PathBuf) {
    let store_root = temp_dir(label);
    let backend =
      RunRecordingBackend::new(LocalStore::new(store_root.clone()).expect("store should create"), Arc::new(MemoryRunRecorder::new()));
    (backend, store_root)
  }

  fn fixture_handler(_input: InvokeCommandInput) -> InvokeCommandFuture {
    Box::pin(async {
      let mut output = InvokeCommandOutput::new("fixture observed");
      output.backend = Some("fixture.backend".to_string());
      output.notes.push("fixture note".to_string());
      output.known_limits.push("fixture limit".to_string());
      output.verification = Some("read-only; no semantic success claim".to_string());
      output.report = Some(InvokeReport::new(
        vec![InvokeReportField {
          label: "Result".to_string(),
          value: "Observed".to_string(),
        }],
        vec![InvokeReportSection {
          title: "Fixture".to_string(),
          fields: vec![InvokeReportField {
            label: "Signal".to_string(),
            value: "observed".to_string(),
          }],
        }],
      ));
      output.signals.insert("fixture".to_string(), "observed".to_string());
      Ok(output)
    })
  }

  fn failing_handler(_input: InvokeCommandInput) -> InvokeCommandFuture {
    Box::pin(async { Err("boom".to_string()) })
  }

  fn artifact_failing_handler(input: InvokeCommandInput) -> InvokeCommandFuture {
    Box::pin(async move {
      let mut output = InvokeCommandOutput::new("artifact output");
      output.backend = Some("fixture.artifact.backend".to_string());
      output.notes.push("artifact failure fixture note".to_string());
      output.known_limits.push("artifact failure fixture limit".to_string());
      output.verification = Some("capture-only; no semantic success claim".to_string());
      output.report = Some(InvokeReport::new(
        vec![InvokeReportField {
          label: "Result".to_string(),
          value: "Artifact pending".to_string(),
        }],
        vec![InvokeReportSection {
          title: "Artifact Failure Fixture".to_string(),
          fields: vec![InvokeReportField {
            label: "Backend".to_string(),
            value: "fixture.artifact.backend".to_string(),
          }],
        }],
      ));
      output.artifacts.push(ProducedArtifact {
        kind: "missing-fixture-artifact".to_string(),
        source_path: temp_dir("missing-artifact-source").join("missing.txt"),
        preferred_name: format!("{}-artifact.txt", input.command_id.replace('.', "-")),
        note: Some("Missing artifact source used to cover recording failure.".to_string()),
      });
      Ok(output)
    })
  }

  fn fixture_registry() -> InvokeRegistry {
    InvokeRegistry::from_groups(vec![
      CommandGroup::new("fixture", "FIXTURE").command(crate::command::spec(
        FIXTURE_COMMAND_ID,
        InvokeNamespace::Fixture,
        "Fixture recorded invoke command.",
        NO_ARGS,
        fixture_handler,
      )),
    ])
  }

  fn failing_registry() -> InvokeRegistry {
    InvokeRegistry::from_groups(vec![
      CommandGroup::new("fixture", "FIXTURE").command(crate::command::spec(
        FAILING_COMMAND_ID,
        InvokeNamespace::Fixture,
        "Failing recorded invoke command.",
        NO_ARGS,
        failing_handler,
      )),
    ])
  }

  fn artifact_failing_registry() -> InvokeRegistry {
    InvokeRegistry::from_groups(vec![
      CommandGroup::new("fixture", "FIXTURE").command(crate::command::spec(
        ARTIFACT_FAILING_COMMAND_ID,
        InvokeNamespace::Fixture,
        "Artifact failing recorded invoke command.",
        NO_ARGS,
        artifact_failing_handler,
      )),
    ])
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
    assert_eq!(result.command_id, FIXTURE_COMMAND_ID);
    assert_eq!(result.command_summary, "Fixture recorded invoke command.");
    assert_eq!(result.output_summary, "fixture observed");
    let canonical = recording.read_run(result.run_id.as_str()).expect("run should persist");
    assert_eq!(canonical.run.run_type, RunType::Command);
    assert_eq!(canonical.run.status_code, TraceStatusCode::Ok);
    let command_span = canonical.spans.iter().find(|span| span.name == "auv.command.invoke").expect("command span should be recorded");
    assert_eq!(result.producer_span_id, Some(command_span.span_id.clone()));
    assert_eq!(command_span.attributes.get("auv.command.id"), Some(&json!(FIXTURE_COMMAND_ID)));
    assert!(canonical.events.iter().any(|event| event.span_id == command_span.span_id && event.name == "command.resolved"));
    assert!(canonical.events.iter().any(|event| event.span_id == command_span.span_id && event.name == "run.completed"));

    let _ = fs::remove_dir_all(store_root);
  }

  #[tokio::test(flavor = "current_thread")]
  async fn runtime_agnostic_recorded_handler_completes_inside_current_thread_tokio() {
    let (recording, store_root) = recording("current-thread-runtime");
    let result = super::invoke_recorded(
      &recording,
      &fixture_registry(),
      InvokeRequest {
        command_id: FIXTURE_COMMAND_ID.to_string(),
        target: ExecutionTarget::default(),
        inputs: BTreeMap::new(),
        dry_run: false,
      },
    )
    .expect("runtime-agnostic legacy handler");

    assert_eq!(result.status, RunStatus::Completed);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn invoke_recorded_propagates_report_and_detail_evidence() {
    let (recording, store_root) = recording("success-report");
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

    assert_eq!(result.backend.as_deref(), Some("fixture.backend"));
    assert_eq!(result.notes, vec!["fixture note"]);
    assert_eq!(result.known_limits, vec!["fixture limit"]);
    assert_eq!(result.verification.as_deref(), Some("read-only; no semantic success claim"));
    assert_eq!(
      result.report,
      Some(InvokeReport::new(
        vec![InvokeReportField {
          label: "Result".to_string(),
          value: "Observed".to_string(),
        }],
        vec![InvokeReportSection {
          title: "Fixture".to_string(),
          fields: vec![InvokeReportField {
            label: "Signal".to_string(),
            value: "observed".to_string(),
          }],
        }],
      ))
    );

    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn invoke_recorded_with_session_stamps_explicit_session_on_run() {
    let (recording, store_root) = recording("explicit-session");
    let registry = fixture_registry();

    let result = super::invoke_recorded_with_session(
      &recording,
      &registry,
      InvokeRequest {
        command_id: FIXTURE_COMMAND_ID.to_string(),
        target: ExecutionTarget::default(),
        inputs: BTreeMap::new(),
        dry_run: false,
      },
      auv_tracing_driver::SessionId::new("session-p5"),
    )
    .expect("invoke should succeed");

    let canonical = recording.read_run(result.run_id.as_str()).expect("run should persist");
    assert_eq!(
      canonical.run.attributes.get(auv_tracing_driver::RUN_ATTR_SESSION_ID).and_then(|value| value.as_str()),
      Some("session-p5"),
      "explicit session id should reach the recorded run"
    );

    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn invoke_recorded_stamps_default_session_on_run() {
    let (recording, store_root) = recording("default-session");
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

    let default_session = auv_tracing_driver::SessionId::default_session();
    let canonical = recording.read_run(result.run_id.as_str()).expect("run should persist");
    assert_eq!(
      canonical.run.attributes.get(auv_tracing_driver::RUN_ATTR_SESSION_ID).and_then(|value| value.as_str()),
      Some(default_session.as_str()),
      "invoke_recorded should stamp the runtime default session when none is given"
    );

    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn invoke_recorded_in_span_adds_command_under_parent_span() {
    let (recording, store_root) = recording("in-span-parent");
    let registry = fixture_registry();
    let mut run = recording.handle().start_run(RunSpec::new(RunType::Execute, "auv.execute")).expect("run should start");
    let parent = run.root_span();

    let result = super::invoke_recorded_in_span(
      &recording,
      &registry,
      &mut run,
      &parent,
      InvokeRequest {
        command_id: FIXTURE_COMMAND_ID.to_string(),
        target: ExecutionTarget::default(),
        inputs: BTreeMap::new(),
        dry_run: false,
      },
    )
    .expect("recorded invoke should succeed inside parent span");

    assert_eq!(result.status, RunStatus::Completed);
    assert_eq!(result.output_summary, "fixture observed");
    let run_id = recording
      .handle()
      .finish_run(
        run,
        RunFinish {
          status_code: TraceStatusCode::Ok,
          summary: Some("done".to_string()),
          failure: None,
        },
      )
      .expect("run should finish");

    let canonical = recording.read_run(run_id.as_str()).expect("run should persist");
    assert_eq!(canonical.run.run_type, RunType::Execute);
    let command_span = canonical.spans.iter().find(|span| span.name == "auv.command.invoke").expect("command span should be recorded");
    assert_eq!(command_span.attributes.get("auv.command.id"), Some(&json!(FIXTURE_COMMAND_ID)));
    assert!(!command_span.attributes.contains_key("auv.driver.id"));
    assert!(!command_span.attributes.contains_key("auv.driver.operation"));
    assert_eq!(command_span.parent_span_id.as_ref(), Some(parent.id()));

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
    assert_eq!(result.command_id, FAILING_COMMAND_ID);
    assert_eq!(result.command_summary, "Failing recorded invoke command.");
    assert_eq!(result.backend, None);
    assert!(result.notes.is_empty());
    assert!(result.known_limits.is_empty());
    assert_eq!(result.verification, None);
    assert_eq!(result.report, None);
    assert!(result.failure_message.as_deref().is_some_and(|message| message.contains("handler failed: boom")));
    let canonical = recording.read_run(result.run_id.as_str()).expect("run should persist");
    let command_span = canonical.spans.iter().find(|span| span.name == "auv.command.invoke").expect("command span should be recorded");
    assert_eq!(command_span.status_code, TraceStatusCode::Error);
    assert!(command_span.failure.as_ref().is_some_and(|failure| failure.message.contains("handler failed: boom")));

    let _ = fs::remove_dir_all(store_root);
  }

  // ROOT CAUSE:
  //
  // Moving product finalization into recorded invoke initially left the
  // handler-failure early return outside the hook, dropping canonical
  // finalization for failed attempts after run creation.
  #[test]
  fn invoke_recorded_with_finalize_runs_hook_for_handler_failure() {
    let (recording, store_root) = recording("handler-failure-finalize");
    let registry = failing_registry();
    let finalize = |_recording: &RunRecordingBackend,
                    _run: &mut auv_tracing_driver::RecordingRun,
                    _span: &auv_tracing_driver::SpanRef,
                    result: &mut crate::InvokeResult| {
      result.known_limits.push("fixture.finalized".to_string());
      Ok(())
    };

    let result = super::invoke_recorded_with_finalize(
      &recording,
      &registry,
      InvokeRequest {
        command_id: FAILING_COMMAND_ID.to_string(),
        target: ExecutionTarget::default(),
        inputs: BTreeMap::new(),
        dry_run: false,
      },
      &finalize,
    )
    .expect("failed handler should remain an inspectable InvokeResult");

    assert_eq!(result.status, RunStatus::Failed);
    assert!(result.known_limits.iter().any(|limit| limit == "fixture.finalized"));
    let canonical = recording.read_run(&result.run_id).expect("failed run should persist");
    assert_eq!(canonical.run.status_code, TraceStatusCode::Error);

    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn invoke_recorded_with_finalize_uses_final_status_for_span_and_run() {
    let (recording, store_root) = recording("finalize-status");
    let registry = fixture_registry();
    let finalize = |_recording: &RunRecordingBackend,
                    _run: &mut auv_tracing_driver::RecordingRun,
                    _span: &auv_tracing_driver::SpanRef,
                    result: &mut crate::InvokeResult| {
      result.status = RunStatus::Failed;
      result.output_summary = "semantic verification failed".to_string();
      result.failure_message = Some("semantic mismatch".to_string());
      Ok(())
    };

    let result = super::invoke_recorded_with_finalize(
      &recording,
      &registry,
      InvokeRequest {
        command_id: FIXTURE_COMMAND_ID.to_string(),
        target: ExecutionTarget::default(),
        inputs: BTreeMap::new(),
        dry_run: false,
      },
      &finalize,
    )
    .expect("finalized failure should remain inspectable");

    assert_eq!(result.status, RunStatus::Failed);
    let canonical = recording.read_run(&result.run_id).expect("finalized run should persist");
    assert_eq!(canonical.run.status_code, TraceStatusCode::Error);
    let command_span = canonical.spans.iter().find(|span| span.name == "auv.command.invoke").expect("command span");
    assert_eq!(command_span.status_code, TraceStatusCode::Error);
    assert_eq!(command_span.failure.as_ref().map(|failure| failure.message.as_str()), Some("semantic mismatch"));

    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn invoke_recorded_finalize_error_persists_failed_run() {
    let store_root = temp_dir("finalize-error");
    let recorder = Arc::new(MemoryRunRecorder::new());
    let recording = RunRecordingBackend::new(LocalStore::new(store_root.clone()).expect("store should create"), recorder.clone());
    let registry = fixture_registry();
    let finalize = |_recording: &RunRecordingBackend,
                    _run: &mut auv_tracing_driver::RecordingRun,
                    _span: &auv_tracing_driver::SpanRef,
                    _result: &mut crate::InvokeResult| Err("fixture finalize failure".to_string());

    let error = super::invoke_recorded_with_finalize(
      &recording,
      &registry,
      InvokeRequest {
        command_id: FIXTURE_COMMAND_ID.to_string(),
        target: ExecutionTarget::default(),
        inputs: BTreeMap::new(),
        dry_run: false,
      },
      &finalize,
    )
    .expect_err("finalize failure should fail the caller");

    assert!(error.contains("fixture finalize failure"));
    let run_id = recorder
      .drain_for_test()
      .into_iter()
      .find_map(|update| match update {
        RunUpdate::RunFinished { run, .. } => Some(run.run_id),
        _ => None,
      })
      .expect("failed finalized run should finish");
    let canonical = recording.read_run(run_id.as_str()).expect("failed finalized run should persist");
    assert_eq!(canonical.run.status_code, TraceStatusCode::Error);
    let command_span = canonical.spans.iter().find(|span| span.name == "auv.command.invoke").expect("command span");
    assert_eq!(command_span.status_code, TraceStatusCode::Error);
    assert!(canonical.events.iter().any(|event| event.name == "command.finalize.failed"));

    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn invoke_recorded_records_artifact_failure_as_failed_result() {
    let (recording, store_root) = recording("artifact-failure");
    let registry = artifact_failing_registry();

    let result = super::invoke_recorded(
      &recording,
      &registry,
      InvokeRequest {
        command_id: ARTIFACT_FAILING_COMMAND_ID.to_string(),
        target: ExecutionTarget::default(),
        inputs: BTreeMap::new(),
        dry_run: false,
      },
    )
    .expect("artifact recording failure should return an inspectable result");

    assert_eq!(result.status, RunStatus::Failed);
    assert_eq!(result.command_id, ARTIFACT_FAILING_COMMAND_ID);
    assert_eq!(result.command_summary, "Artifact failing recorded invoke command.");
    assert_eq!(result.backend.as_deref(), Some("fixture.artifact.backend"));
    assert_eq!(result.notes, vec!["artifact failure fixture note"]);
    assert_eq!(result.known_limits, vec!["artifact failure fixture limit"]);
    assert_eq!(result.verification.as_deref(), Some("capture-only; no semantic success claim"));
    assert_eq!(
      result.report,
      Some(InvokeReport::new(
        vec![InvokeReportField {
          label: "Result".to_string(),
          value: "Artifact pending".to_string(),
        }],
        vec![InvokeReportSection {
          title: "Artifact Failure Fixture".to_string(),
          fields: vec![InvokeReportField {
            label: "Backend".to_string(),
            value: "fixture.artifact.backend".to_string(),
          }],
        }],
      ))
    );
    assert!(result.failure_message.as_deref().is_some_and(|message| message.contains("artifact recording failed")));
    assert!(result.artifacts.is_empty());
    assert!(result.artifact_paths.is_empty());

    let canonical = recording.read_run(result.run_id.as_str()).expect("run should persist");
    assert_eq!(canonical.run.status_code, TraceStatusCode::Error);
    let command_span = canonical.spans.iter().find(|span| span.name == "auv.command.invoke").expect("command span should be recorded");
    assert_eq!(command_span.status_code, TraceStatusCode::Error);
    assert!(command_span.failure.as_ref().is_some_and(|failure| failure.message.contains("artifact recording failed")));

    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn invoke_recorded_rejects_unknown_command_and_finishes_failed_run() {
    let store_root = temp_dir("unknown-command");
    let recorder = Arc::new(MemoryRunRecorder::new());
    let recording = RunRecordingBackend::new(LocalStore::new(store_root.clone()).expect("store should create"), recorder.clone());
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

    assert!(error.contains("auv invoke --help"));
    let run_id = recorder
      .drain_for_test()
      .into_iter()
      .find_map(|update| match update {
        RunUpdate::RunFinished { run, .. } => Some(run.run_id),
        _ => None,
      })
      .expect("failed implicit run should finish");
    let canonical = recording.read_run(run_id.as_str()).expect("failed run snapshot should persist");
    assert_eq!(canonical.run.status_code, TraceStatusCode::Error);
    assert!(canonical.run.failure.as_ref().is_some_and(|failure| failure.message.contains("missing.command")));

    let _ = fs::remove_dir_all(store_root);
  }
}
