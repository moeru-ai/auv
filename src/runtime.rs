// File: src/runtime.rs
//! Runtime execution engine.
//!
//! `Runtime` is the shared invoke facade used by CLI and other frontends: it executes
//! resolved command specs, invokes drivers, and delegates durable run/span/event
//! and artifact recording to `auv-tracing-driver`.
//!
//! Boundary: this layer executes *given* requests. It is not a planner/LLM
//! agent, and it does not choose strategies beyond what the request/cmd
//! specifies.

use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const TELEMETRY_SAMPLE_ARTIFACT_ROLE: &str = "telemetry-sample";
pub const TELEMETRY_SAMPLE_MAX_BYTES: u64 = 128 * 1024;
pub const MINECRAFT_PROJECTION_ARTIFACT_ROLE: &str = "minecraft-projection";

use crate::contract::ArtifactRef;
use crate::model::{AuvResult, InvokeRequest, InvokeResult, RunStatus};
use auv_tracing_driver::store::LocalStore;
use auv_tracing_driver::trace::{RunType, TraceStatusCode, string_attr};
use auv_tracing_driver::{MemoryRunRecorder, RunRecorder, RunRecordingBackend};

pub struct Runtime {
  project_root: PathBuf,
  recording: RunRecordingBackend,
}

// NOTICE(mc2-live-telemetry-tail-artifact): live telemetry.jsonl can grow to
// multi-GB files during long Minecraft sessions. The current artifact staging
// path still copies and server-uploads the artifact as a whole, so recording
// the full live file would amplify disk and memory use badly. For the current
// MC-2 slice we only persist a capped tail sample as durable evidence. If a
// future slice needs full-session archival, that path must stream copy/upload
// instead of routing the live file through the existing artifact staging seam.
fn prepare_telemetry_sample_artifact(path: &Path) -> AuvResult<Option<PathBuf>> {
  let metadata = std::fs::metadata(path).map_err(|error| {
    format!(
      "failed to stat telemetry sample artifact {}: {error}",
      path.display()
    )
  })?;
  if metadata.len() <= TELEMETRY_SAMPLE_MAX_BYTES {
    return Ok(None);
  }

  let file = std::fs::File::open(path).map_err(|error| {
    format!(
      "failed to open telemetry sample artifact {}: {error}",
      path.display()
    )
  })?;
  let mut reader = BufReader::new(file);
  let start = metadata.len().saturating_sub(TELEMETRY_SAMPLE_MAX_BYTES);
  reader.seek(SeekFrom::Start(start)).map_err(|error| {
    format!(
      "failed to seek telemetry sample artifact {}: {error}",
      path.display()
    )
  })?;

  if start > 0 {
    let mut discarded = String::new();
    reader.read_line(&mut discarded).map_err(|error| {
      format!(
        "failed to align telemetry sample artifact {} to next line: {error}",
        path.display()
      )
    })?;
  }

  let temp_path = std::env::temp_dir().join(format!(
    "auv-telemetry-tail-{}-{}.jsonl",
    std::process::id(),
    crate::model::now_millis()
  ));
  let mut temp = std::fs::File::create(&temp_path).map_err(|error| {
    format!(
      "failed to create trimmed telemetry sample artifact {}: {error}",
      temp_path.display()
    )
  })?;
  std::io::copy(&mut reader, &mut temp).map_err(|error| {
    format!(
      "failed to trim telemetry sample artifact {}: {error}",
      path.display()
    )
  })?;
  temp.flush().map_err(|error| {
    format!(
      "failed to flush trimmed telemetry sample artifact {}: {error}",
      temp_path.display()
    )
  })?;
  drop(temp);

  Ok(Some(temp_path))
}

impl Runtime {
  pub fn new(project_root: PathBuf, store: LocalStore) -> Self {
    Self {
      project_root,
      recording: RunRecordingBackend::new(store, Arc::new(MemoryRunRecorder::new())),
    }
  }

  pub fn project_root(&self) -> &Path {
    &self.project_root
  }

  pub fn inspect(&self, run_id: &str) -> AuvResult<String> {
    crate::inspect::inspect_run(self.recording.store(), run_id)
  }

  pub fn read_run(&self, run_id: &str) -> AuvResult<auv_tracing_driver::store::CanonicalRun> {
    self.recording.read_run(run_id)
  }

  pub fn list_verifications(
    &self,
    run_id: &str,
  ) -> AuvResult<Vec<crate::contract::VerificationResult>> {
    crate::run_read::list_verifications(self.recording.store(), run_id)
  }

  pub fn list_observation_snapshots(
    &self,
    run_id: &str,
  ) -> AuvResult<Vec<crate::contract::ObservationSnapshot>> {
    crate::run_read::list_observation_snapshots(self.recording.store(), run_id)
  }

  pub fn list_detector_recognition_lineage(
    &self,
    run_id: &str,
  ) -> AuvResult<Vec<crate::run_read::DetectorRecognitionLineage>> {
    crate::run_read::list_detector_recognition_lineage(self.recording.store(), run_id)
  }

  pub fn list_candidate_promotion_lineage(
    &self,
    run_id: &str,
  ) -> AuvResult<Vec<crate::run_read::CandidatePromotionLineage>> {
    crate::run_read::list_candidate_promotion_lineage(self.recording.store(), run_id)
  }

  pub fn list_candidate_action_decision_lineage(
    &self,
    run_id: &str,
  ) -> AuvResult<Vec<crate::run_read::CandidateActionDecisionLineage>> {
    crate::run_read::list_candidate_action_decision_lineage(self.recording.store(), run_id)
  }

  pub fn run_recorded_operation<T, E, F>(
    &self,
    spec: auv_tracing_driver::run_builder::RunSpec,
    operation_label: impl Into<String>,
    operation: F,
  ) -> AuvResult<auv_tracing_driver::recorded_operation::RecordedOperationOutput<T>>
  where
    E: std::fmt::Display,
    F: FnOnce(
      &mut auv_tracing_driver::recorded_operation::RecordedOperationContext<'_>,
    ) -> Result<T, E>,
  {
    self
      .recording
      .handle()
      .run_recorded_operation(spec, operation_label, operation)
  }

  pub fn record_candidate_action_decision(
    &self,
    promotion: &crate::candidate_promotion_recording::CandidatePromotionArtifact,
    request: crate::candidate_action_decision::CandidateActionDecisionRequest,
  ) -> AuvResult<
    auv_tracing_driver::recorded_operation::RecordedOperationOutput<(
      ArtifactRef,
      crate::candidate_action_decision::CandidateActionDecisionArtifact,
    )>,
  > {
    self.run_recorded_operation(
      auv_tracing_driver::run_builder::RunSpec::new(
        RunType::Execute,
        "auv.candidate.action.decide_only",
      ),
      "Candidate action decide-only artifact recording",
      |context| {
        crate::candidate_action_decision::record_candidate_action_decision_artifact(
          context, promotion, &request,
        )
      },
    )
  }

  pub fn record_telemetry_sample_artifact(
    &self,
    sample_path: impl Into<PathBuf>,
  ) -> AuvResult<auv_tracing_driver::recorded_operation::RecordedOperationOutput<ArtifactRef>> {
    let sample_path = sample_path.into();
    let preferred_name = sample_path
      .file_name()
      .and_then(|name| name.to_str())
      .ok_or_else(|| {
        format!(
          "telemetry sample path {:?} has no valid file name",
          sample_path
        )
      })?
      .to_string();

    if !sample_path.is_file() {
      return Err(format!(
        "telemetry sample path {:?} is not a readable file",
        sample_path
      ));
    }

    self.run_recorded_operation(
      auv_tracing_driver::run_builder::RunSpec::new(
        RunType::Execute,
        "auv.minecraft.telemetry.sample",
      ),
      "Minecraft telemetry sample artifact recording",
      |context| {
        let artifact_source =
          prepare_telemetry_sample_artifact(&sample_path)?.unwrap_or_else(|| sample_path.clone());
        let stage_result = context.stage_artifact_file_with_ref(
          TELEMETRY_SAMPLE_ARTIFACT_ROLE,
          &artifact_source,
          &preferred_name,
          Some("durable minecraft telemetry sample".to_string()),
        );
        if artifact_source != sample_path {
          let _ = std::fs::remove_file(&artifact_source);
        }
        let (_, artifact_ref) = stage_result?;
        Ok::<_, String>(artifact_ref)
      },
    )
  }

  pub fn record_minecraft_projection_artifact(
    &self,
    projection_artifact: auv_game_minecraft::MinecraftProjectionArtifact,
  ) -> AuvResult<auv_tracing_driver::recorded_operation::RecordedOperationOutput<ArtifactRef>> {
    projection_artifact.validate()?;
    let artifact_json = serde_json::to_string_pretty(&projection_artifact)
      .map_err(|error| format!("failed to serialize minecraft projection artifact: {error}"))?;

    self.run_recorded_operation(
      auv_tracing_driver::run_builder::RunSpec::new(
        RunType::Execute,
        "auv.minecraft.projection.artifact",
      ),
      "Minecraft projection artifact recording",
      |context| {
        let temp_root = std::env::temp_dir();
        let artifact_path = temp_root.join(format!(
          "auv-minecraft-projection-{}-{}.json",
          context.run_id(),
          crate::model::now_millis()
        ));
        std::fs::write(&artifact_path, artifact_json.as_bytes())
          .map_err(|error| format!("failed to write minecraft projection artifact: {error}"))?;
        let (_, artifact_ref) = context.stage_artifact_file_with_ref(
          MINECRAFT_PROJECTION_ARTIFACT_ROLE,
          &artifact_path,
          "projection-artifact.json",
          Some("durable minecraft projection artifact".to_string()),
        )?;
        let _ = std::fs::remove_file(&artifact_path);
        Ok::<_, String>(artifact_ref)
      },
    )
  }

  pub fn list_candidate_action_execution_lineage(
    &self,
    run_id: &str,
  ) -> AuvResult<Vec<crate::run_read::CandidateActionExecutionLineage>> {
    crate::run_read::list_candidate_action_execution_lineage(self.recording.store(), run_id)
  }

  pub fn run_candidate_action_command(
    &self,
    request: crate::candidate_action_command::CandidateActionCommandRequest,
  ) -> AuvResult<
    auv_tracing_driver::recorded_operation::RecordedOperationOutput<
      crate::candidate_action_command::CandidateActionCommandOutput,
    >,
  > {
    self.run_recorded_operation(
      auv_tracing_driver::run_builder::RunSpec::new(
        RunType::Execute,
        "auv.candidate.action.command",
      ),
      "Consent-gated candidate action command",
      |context| {
        crate::candidate_action_command::execute_candidate_action_command(context, &request)
      },
    )
  }

  pub fn record_candidate_action_execution(
    &self,
    promotion: &crate::candidate_promotion_recording::CandidatePromotionArtifact,
    decision: &crate::candidate_action_decision::CandidateActionDecisionArtifact,
    request: crate::candidate_action_decision::CandidateActionExecutionRequest,
    input_action_result: auv_driver::InputActionResult,
  ) -> AuvResult<
    auv_tracing_driver::recorded_operation::RecordedOperationOutput<(
      ArtifactRef,
      crate::candidate_action_decision::CandidateActionExecutionArtifact,
    )>,
  > {
    self.run_recorded_operation(
      auv_tracing_driver::run_builder::RunSpec::new(
        RunType::Execute,
        "auv.candidate.action.execute_single",
      ),
      "Candidate action execution artifact recording",
      |context| {
        crate::candidate_action_decision::record_candidate_action_execution_artifact(
          context,
          promotion,
          decision,
          &request,
          input_action_result,
        )
      },
    )
  }

  pub fn run_dir(&self, run_id: impl AsRef<str>) -> AuvResult<PathBuf> {
    self.recording.run_dir(run_id)
  }

  pub fn recorder(&self) -> Arc<dyn RunRecorder> {
    self.recording.recorder()
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

  pub fn with_recorder(mut self, recorder: Arc<dyn RunRecorder>) -> Self {
    let store = self.recording.store().clone();
    self.recording = RunRecordingBackend::new(store, recorder);
    self
  }

  pub fn invoke(&self, request: InvokeRequest) -> AuvResult<InvokeResult> {
    self.invoke_in_command_run(|runtime, run, root| runtime.invoke_in_span(run, root, request))
  }

  pub fn invoke_resolved(
    &self,
    request: InvokeRequest,
    command: &auv_cli_invoke::InvokeCommand,
  ) -> AuvResult<InvokeResult> {
    self.invoke_in_command_run(|runtime, run, root| {
      runtime.invoke_metadata_command_in_span(run, root, command, request)
    })
  }

  fn invoke_in_command_run(
    &self,
    invoke: impl FnOnce(
      &Self,
      &mut auv_tracing_driver::run_builder::RecordingRun,
      &auv_tracing_driver::run_builder::SpanRef,
    ) -> AuvResult<InvokeResult>,
  ) -> AuvResult<InvokeResult> {
    let mut run =
      self
        .recording
        .handle()
        .start_run(auv_tracing_driver::run_builder::RunSpec::new(
          RunType::Command,
          "auv.command",
        ))?;
    let root = run.root_span();
    let result = match invoke(self, &mut run, &root) {
      Ok(result) => result,
      Err(error) => {
        if let Err(finish_error) = self.recording.handle().finish_run(
          run,
          auv_tracing_driver::run_builder::RunFinish {
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
    self.recording.handle().finish_run(
      run,
      auv_tracing_driver::run_builder::RunFinish {
        status_code,
        summary: Some(result.output_summary.clone()),
        failure: result.failure_message.clone(),
      },
    )?;
    Ok(result)
  }

  pub fn invoke_in_span(
    &self,
    run: &mut auv_tracing_driver::run_builder::RecordingRun,
    parent: &auv_tracing_driver::run_builder::SpanRef,
    request: InvokeRequest,
  ) -> AuvResult<InvokeResult> {
    let command_id = request.command_id.clone();
    // TODO(invoke-boundary): accept a resolved invoke command descriptor instead
    // of resolving the CLI registry here. This stays only until CLI, MCP,
    // app-probe, and scroll-scan callers share the next typed invoke request.
    let registry = auv_cli_invoke::default_registry();
    let command = registry.resolve(&command_id).ok_or_else(|| {
      format!(
        "unknown command {command_id}; use `auv-cli invoke --help` to inspect available entries"
      )
    })?;
    self.invoke_metadata_command_in_span(run, parent, command, request)
  }

  fn invoke_metadata_command_in_span(
    &self,
    run: &mut auv_tracing_driver::run_builder::RecordingRun,
    parent: &auv_tracing_driver::run_builder::SpanRef,
    command: &auv_cli_invoke::InvokeCommand,
    request: InvokeRequest,
  ) -> AuvResult<InvokeResult> {
    let command_span = run.start_span(
      parent,
      auv_tracing_driver::run_builder::running_span_record(
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

    let output = match command.invoke(auv_cli_invoke::InvokeCommandInput {
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
          auv_tracing_driver::run_builder::SpanFinish {
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

    let artifact_result =
      self
        .recording
        .record_produced_artifacts(run, &command_span, output.artifacts);
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
          auv_tracing_driver::run_builder::SpanFinish {
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
      auv_tracing_driver::run_builder::SpanFinish {
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
}

fn command_attributes(
  command_id: &str,
  target_application_id: Option<&str>,
) -> auv_tracing_driver::run_builder::Attributes {
  let mut attributes = auv_tracing_driver::run_builder::Attributes::new();
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
  run: &mut auv_tracing_driver::run_builder::RecordingRun,
  span_id: &auv_tracing_driver::trace::SpanId,
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

  use auv_tracing_driver::ProducedArtifact;
  use serde_json::json;

  use super::{
    MINECRAFT_PROJECTION_ARTIFACT_ROLE, Runtime, TELEMETRY_SAMPLE_ARTIFACT_ROLE,
    TELEMETRY_SAMPLE_MAX_BYTES,
  };
  use crate::model::{AuvResult, ExecutionTarget, InvokeRequest, RunStatus, now_millis};
  use auv_tracing_driver::store::LocalStore;
  use auv_tracing_driver::{MemoryRunRecorder, RunRecorder, RunUpdate};

  struct FailRunFinishedRecorder;
  struct RequiredFailRunStartedRecorder;
  struct RequiredFailRunFinishedRecorder;

  const TEST_COMMAND_ID: &str = "fixture.observe";
  const REGISTERED_HANDLER_COMMAND_ID: &str = "test.registeredHandler";

  #[test]
  fn fixture_registry_command_has_direct_handler() {
    let registry = auv_cli_invoke::default_registry();
    let inputs = BTreeMap::new();
    let command = registry
      .resolve(TEST_COMMAND_ID)
      .expect("fixture command should be registered");
    let output = command
      .invoke(auv_cli_invoke::InvokeCommandInput {
        command_id: command.id,
        target_application_id: None,
        inputs: &inputs,
        dry_run: true,
      })
      .expect("fixture command should have a direct handler");

    assert_eq!(output.summary, "fixture observed");
  }

  impl RunRecorder for FailRunFinishedRecorder {
    fn record(&self, update: RunUpdate) -> AuvResult<()> {
      match update {
        RunUpdate::RunFinished { .. } => Err("run finished recorder failure".to_string()),
        _ => Ok(()),
      }
    }
  }

  impl RunRecorder for RequiredFailRunStartedRecorder {
    fn record(&self, update: RunUpdate) -> AuvResult<()> {
      match update {
        RunUpdate::RunStarted { .. } => Err("run started recorder failure".to_string()),
        _ => Ok(()),
      }
    }

    fn requires_successful_delivery(&self) -> bool {
      true
    }
  }

  impl RunRecorder for RequiredFailRunFinishedRecorder {
    fn record(&self, update: RunUpdate) -> AuvResult<()> {
      match update {
        RunUpdate::RunFinished { .. } => Err("run finished recorder failure".to_string()),
        _ => Ok(()),
      }
    }

    fn requires_successful_delivery(&self) -> bool {
      true
    }
  }

  #[test]
  fn record_telemetry_sample_artifact_persists_sample_for_inspect() {
    let project_root = temp_dir("runtime-telemetry-project");
    let store_root = temp_dir("runtime-telemetry-store");
    fs::create_dir_all(&project_root).expect("project root should exist");
    let source_path = project_root.join("telemetry.jsonl");
    fs::write(&source_path, "{\"sample\":true}\n").expect("telemetry sample should write");

    let runtime = runtime_with_success_driver(project_root.clone(), store_root.clone());
    let output = runtime
      .record_telemetry_sample_artifact(source_path.clone())
      .expect("telemetry sample recording should succeed");

    assert_eq!(output.value.run_id.as_str(), output.run_id.as_str());
    let run = runtime
      .read_run(output.run_id.as_str())
      .expect("run should persist");
    assert_eq!(run.artifacts.len(), 1);
    assert_eq!(run.artifacts[0].role, TELEMETRY_SAMPLE_ARTIFACT_ROLE);
    assert_eq!(
      run.artifacts[0].path,
      "artifacts/artifact_0001_telemetry.jsonl"
    );

    let inspect_text = runtime
      .inspect(output.run_id.as_str())
      .expect("inspect should render run");
    assert!(inspect_text.contains("Artifacts:"));
    assert!(inspect_text.contains("role=telemetry-sample"));
    assert!(inspect_text.contains("path=artifacts/artifact_0001_telemetry.jsonl"));

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn record_telemetry_sample_artifact_rejects_missing_file() {
    let project_root = temp_dir("runtime-telemetry-missing-project");
    let store_root = temp_dir("runtime-telemetry-missing-store");
    fs::create_dir_all(&project_root).expect("project root should exist");
    let source_path = project_root.join("missing-telemetry.jsonl");

    let runtime = runtime_with_success_driver(project_root.clone(), store_root.clone());
    let error = runtime
      .record_telemetry_sample_artifact(source_path.clone())
      .expect_err("missing telemetry sample should fail");

    assert!(error.contains("is not a readable file"));

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn record_telemetry_sample_artifact_keeps_large_source_file_intact() {
    let project_root = temp_dir("runtime-telemetry-large-project");
    let store_root = temp_dir("runtime-telemetry-large-store");
    fs::create_dir_all(&project_root).expect("project root should exist");
    let source_path = project_root.join("telemetry.jsonl");
    let original_body = (0..6000)
      .map(|index| {
        format!(
          "{{\"sample\":{index},\"payload\":\"{}\"}}\n",
          "x".repeat(32)
        )
      })
      .collect::<String>();
    fs::write(&source_path, &original_body).expect("large telemetry sample should write");
    let original_size = fs::metadata(&source_path).expect("source metadata").len();
    assert!(
      original_size > TELEMETRY_SAMPLE_MAX_BYTES,
      "fixture must exceed trimming threshold"
    );

    let runtime = runtime_with_success_driver(project_root.clone(), store_root.clone());
    let output = runtime
      .record_telemetry_sample_artifact(source_path.clone())
      .expect("telemetry sample recording should succeed");

    let persisted_source = fs::read_to_string(&source_path).expect("source file should remain");
    assert_eq!(persisted_source, original_body);

    let run = runtime
      .read_run(output.run_id.as_str())
      .expect("run should persist");
    let staged_path = store_root
      .join("runs")
      .join(output.run_id.as_str())
      .join(&run.artifacts[0].path);
    let staged_size = fs::metadata(&staged_path)
      .expect("staged artifact metadata")
      .len();
    assert!(staged_size <= TELEMETRY_SAMPLE_MAX_BYTES);

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn record_minecraft_projection_artifact_persists_artifact_for_inspect() {
    let project_root = temp_dir("runtime-minecraft-projection-project");
    let store_root = temp_dir("runtime-minecraft-projection-store");
    fs::create_dir_all(&project_root).expect("project root should exist");

    let runtime = runtime_with_success_driver(project_root.clone(), store_root.clone());
    let projection_artifact = auv_game_minecraft::MinecraftProjectionArtifact {
      spatial_frame_id: "frame-1".to_string(),
      world_tick: 42,
      monotonic_timestamp_ms: 1_000,
      screenshot_artifact_ref: Some("artifact://screenshot-1".to_string()),
      mc_capture_skew_ms: Some(180),
      viewport_bounds: auv_game_minecraft::ProjectionViewportBounds {
        x: 0.0,
        y: 0.0,
        width: 800.0,
        height: 600.0,
      },
      projected_point: Some(auv_game_minecraft::MinecraftProjectedPoint {
        screen_point: Some(auv_driver::geometry::Point::new(320.0, 240.0)),
        visibility: auv_game_minecraft::ProjectionVisibility::Visible,
        match_radius_px: 12.0,
        basis_frame_id: "frame-1".to_string(),
        confidence: 1.0,
      }),
      visibility: auv_game_minecraft::ProjectionVisibility::Visible,
      raycast_block_id: Some("minecraft:stone".to_string()),
      screen_state: Some("menu".to_string()),
      mismatch_refusal_reason: Some(
        auv_game_minecraft::verify::MismatchRefusalReason::MenuLoadingScreen,
      ),
      verification_reference: Some("verification-1".to_string()),
    };

    let output = runtime
      .record_minecraft_projection_artifact(projection_artifact)
      .expect("minecraft projection artifact recording should succeed");

    let run = runtime
      .read_run(output.run_id.as_str())
      .expect("run should persist");
    assert_eq!(run.artifacts.len(), 1);
    assert_eq!(run.artifacts[0].role, MINECRAFT_PROJECTION_ARTIFACT_ROLE);
    assert_eq!(
      run.artifacts[0].path,
      "artifacts/artifact_0001_projection-artifact.json"
    );

    let inspect_text = runtime
      .inspect(output.run_id.as_str())
      .expect("inspect should render run");
    assert!(inspect_text.contains("MC-2 Projection Artifacts:"));
    assert!(inspect_text.contains("frame=frame-1"));
    assert!(inspect_text.contains("screenshot_artifact_ref=artifact://screenshot-1"));
    assert!(inspect_text.contains("capture_skew_ms=180"));
    assert!(inspect_text.contains("screen_state=menu"));
    assert!(inspect_text.contains("refusal_reason=MenuLoadingScreen"));
    assert!(inspect_text.contains("verification_reference=verification-1"));

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn invoke_in_span_adds_command_under_parent_span() {
    let project_root = temp_dir("runtime-recorded-project");
    let store_root = temp_dir("runtime-recorded-store");
    let runtime = runtime_with_success_driver(project_root.clone(), store_root.clone());

    let mut run = runtime
      .recording()
      .handle()
      .start_run(auv_tracing_driver::run_builder::RunSpec::new(
        auv_tracing_driver::trace::RunType::Execute,
        "auv.execute",
      ))
      .expect("run should start");
    let parent = run.root_span();
    let result = runtime
      .invoke_in_span(
        &mut run,
        &parent,
        InvokeRequest {
          command_id: TEST_COMMAND_ID.to_string(),
          target: ExecutionTarget::default(),
          inputs: BTreeMap::new(),
          dry_run: false,
        },
      )
      .expect("recorded invoke should succeed");
    assert_eq!(result.status, RunStatus::Completed);
    assert_eq!(result.output_summary, "fixture observed");
    assert!(result.signals.is_empty());
    let run_id = runtime
      .recording()
      .handle()
      .finish_run(
        run,
        auv_tracing_driver::run_builder::RunFinish {
          status_code: auv_tracing_driver::trace::TraceStatusCode::Ok,
          summary: Some("done".to_string()),
          failure: None,
        },
      )
      .expect("run should finish");

    let canonical = runtime.read_run(run_id.as_str()).expect("run should read");
    assert_eq!(
      canonical.run.run_type,
      auv_tracing_driver::trace::RunType::Execute
    );
    assert!(
      canonical
        .spans
        .iter()
        .any(|span| span.name == "auv.command.invoke")
    );
    let command_span = canonical
      .spans
      .iter()
      .find(|span| span.name == "auv.command.invoke")
      .expect("command span should be recorded");
    assert_eq!(
      command_span.attributes.get("auv.command.id"),
      Some(&json!(TEST_COMMAND_ID))
    );
    assert!(command_span.attributes.get("auv.driver.id").is_none());
    assert!(
      command_span
        .attributes
        .get("auv.driver.operation")
        .is_none()
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

  fn registered_handler_command(
    input: auv_cli_invoke::InvokeCommandInput<'_>,
  ) -> auv_cli_invoke::InvokeCommandResult {
    if input.command_id != REGISTERED_HANDLER_COMMAND_ID {
      return Err(format!("unexpected command_id {}", input.command_id));
    }
    if input.target_application_id != Some("test.app") {
      return Err(format!(
        "unexpected target_application_id {:?}",
        input.target_application_id
      ));
    }
    if input.inputs.get("marker").map(String::as_str) != Some("handler-input") {
      return Err("handler input marker was not forwarded".to_string());
    }
    if !input.dry_run {
      return Err("dry_run was not forwarded".to_string());
    }
    let mut output = auv_cli_invoke::InvokeCommandOutput::new("direct output completed");
    output.backend = Some("test.direct-handler".to_string());
    output
      .signals
      .insert("marker".to_string(), "handler-output".to_string());
    output.notes.push("handler note".to_string());
    Ok(output)
  }

  fn failing_registered_handler_command(
    _input: auv_cli_invoke::InvokeCommandInput<'_>,
  ) -> auv_cli_invoke::InvokeCommandResult {
    Err("handler refused test command".to_string())
  }

  fn artifact_registered_handler_command(
    input: auv_cli_invoke::InvokeCommandInput<'_>,
  ) -> auv_cli_invoke::InvokeCommandResult {
    let source_path = temp_dir("runtime-direct-handler-artifact").join("source.txt");
    fs::write(&source_path, "direct handler artifact\n").expect("artifact source should write");

    let mut output = auv_cli_invoke::InvokeCommandOutput::new("direct artifact completed");
    output.artifacts.push(ProducedArtifact {
      kind: "direct-handler-report".to_string(),
      source_path,
      preferred_name: format!("{}-report.txt", input.command_id.replace('.', "-")),
      note: Some("Direct handler artifact.".to_string()),
    });
    output
      .known_limits
      .push("fixture direct handler evidence only".to_string());
    output.verification = Some("fixture-only; no semantic success claim".to_string());
    Ok(output)
  }

  #[test]
  fn invoke_resolved_records_direct_command_output_without_driver_span() {
    let project_root = temp_dir("runtime-resolved-project");
    let store_root = temp_dir("runtime-resolved-store");
    let runtime = runtime_with_success_driver(project_root.clone(), store_root.clone());
    let command = auv_cli_invoke::command::spec(
      REGISTERED_HANDLER_COMMAND_ID,
      auv_cli_invoke::InvokeNamespace::Fixture,
      "Test registered command handler routing.",
      auv_cli_invoke::arg::NO_ARGS,
      registered_handler_command,
    );

    let result = runtime
      .invoke_resolved(
        InvokeRequest {
          command_id: REGISTERED_HANDLER_COMMAND_ID.to_string(),
          target: ExecutionTarget {
            application_id: Some("test.app".to_string()),
            target_label: None,
          },
          inputs: BTreeMap::from([("marker".to_string(), "handler-input".to_string())]),
          dry_run: true,
        },
        &command,
      )
      .expect("resolved invoke should use the registered handler");

    assert_eq!(result.status, RunStatus::Completed);
    assert_eq!(result.output_summary, "direct output completed");
    assert_eq!(
      result.signals.get("marker"),
      Some(&"handler-output".to_string())
    );
    let canonical = runtime.read_run(&result.run_id).expect("run should read");
    assert_eq!(
      canonical.run.status_code,
      auv_tracing_driver::trace::TraceStatusCode::Ok
    );
    let command_span = canonical
      .spans
      .iter()
      .find(|span| span.name == "auv.command.invoke")
      .expect("command span should be recorded");
    assert_eq!(
      command_span.attributes.get("auv.command.id"),
      Some(&json!(REGISTERED_HANDLER_COMMAND_ID))
    );
    assert!(
      !canonical
        .spans
        .iter()
        .any(|span| span.name == "auv.driver.invoke")
    );
    assert_eq!(result.producer_span_id, command_span.span_id);
    assert!(canonical.events.iter().any(|event| {
      event.name == "command.backend"
        && event
          .message
          .as_deref()
          .is_some_and(|message| message.contains("test.direct-handler"))
    }));
    assert!(canonical.events.iter().any(|event| {
      event.name == "command.note"
        && event
          .message
          .as_deref()
          .is_some_and(|message| message.contains("handler note"))
    }));

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn invoke_resolved_records_direct_handler_artifacts() {
    let project_root = temp_dir("runtime-resolved-artifact-project");
    let store_root = temp_dir("runtime-resolved-artifact-store");
    let runtime = runtime_with_success_driver(project_root.clone(), store_root.clone());
    let command = auv_cli_invoke::command::spec(
      REGISTERED_HANDLER_COMMAND_ID,
      auv_cli_invoke::InvokeNamespace::Fixture,
      "Test registered command handler artifacts.",
      auv_cli_invoke::arg::NO_ARGS,
      artifact_registered_handler_command,
    );

    let result = runtime
      .invoke_resolved(
        InvokeRequest {
          command_id: REGISTERED_HANDLER_COMMAND_ID.to_string(),
          ..InvokeRequest::default()
        },
        &command,
      )
      .expect("resolved invoke should record direct handler artifacts");

    assert_eq!(result.status, RunStatus::Completed);
    assert_eq!(result.artifacts.len(), 1);
    assert_eq!(result.artifacts[0].role, "direct-handler-report");
    assert_eq!(result.artifact_paths.len(), 1);
    let canonical = runtime.read_run(&result.run_id).expect("run should read");
    assert_eq!(canonical.artifacts.len(), 1);
    assert!(canonical.events.iter().any(|event| {
      event.name == "artifact.captured"
        && event.artifact_ids == vec![result.artifacts[0].artifact_id.clone()]
    }));
    assert!(canonical.events.iter().any(|event| {
      event.name == "command.verification"
        && event
          .message
          .as_deref()
          .is_some_and(|message| message.contains("fixture-only"))
    }));
    assert!(canonical.events.iter().any(|event| {
      event.name == "command.known_limit"
        && event
          .message
          .as_deref()
          .is_some_and(|message| message.contains("evidence only"))
    }));

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn invoke_resolved_records_registered_handler_error_with_command_id() {
    let project_root = temp_dir("runtime-resolved-handler-error-project");
    let store_root = temp_dir("runtime-resolved-handler-error-store");
    let runtime = runtime_with_success_driver(project_root.clone(), store_root.clone());
    let command = auv_cli_invoke::command::spec(
      REGISTERED_HANDLER_COMMAND_ID,
      auv_cli_invoke::InvokeNamespace::Fixture,
      "Test registered command handler failure.",
      auv_cli_invoke::arg::NO_ARGS,
      failing_registered_handler_command,
    );

    let result = runtime
      .invoke_resolved(
        InvokeRequest {
          command_id: REGISTERED_HANDLER_COMMAND_ID.to_string(),
          ..InvokeRequest::default()
        },
        &command,
      )
      .expect("handler failure should still return an inspectable invoke result");

    assert_eq!(result.status, RunStatus::Failed);
    let failure = result
      .failure_message
      .as_deref()
      .expect("failure message should be recorded");
    assert!(failure.contains("command test.registeredHandler handler failed"));
    assert!(failure.contains("handler refused test command"));
    let canonical = runtime.read_run(&result.run_id).expect("run should read");
    assert_eq!(
      canonical.run.status_code,
      auv_tracing_driver::trace::TraceStatusCode::Error
    );
    assert!(canonical.events.iter().any(|event| {
      event.name == "command.failed"
        && event
          .message
          .as_deref()
          .is_some_and(|message| message.contains("handler refused test command"))
    }));

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn invoke_unknown_command_finishes_failed_implicit_run() {
    let project_root = temp_dir("runtime-unknown-command-project");
    let store_root = temp_dir("runtime-unknown-command-store");
    let recorder = Arc::new(MemoryRunRecorder::new());
    let runtime = runtime_with_success_driver(project_root.clone(), store_root.clone())
      .with_recorder(recorder.clone());

    let error = runtime
      .invoke(InvokeRequest {
        command_id: "test.missing".to_string(),
        target: ExecutionTarget::default(),
        inputs: BTreeMap::new(),
        dry_run: false,
      })
      .expect_err("unknown command should fail");

    assert!(error.contains("unknown command"));
    let run_id = recorder
      .drain_for_test()
      .into_iter()
      .find_map(|update| match update {
        RunUpdate::RunFinished { run, .. } => Some(run.run_id),
        _ => None,
      })
      .expect("failed implicit run should finish");
    let canonical = runtime.read_run(run_id.as_str()).expect("run should read");
    assert_eq!(
      canonical.run.status_code,
      auv_tracing_driver::trace::TraceStatusCode::Error
    );
    assert!(
      canonical
        .run
        .failure
        .expect("failure")
        .message
        .contains("unknown command")
    );
    assert!(
      canonical
        .spans
        .iter()
        .all(|span| span.state == auv_tracing_driver::trace::TraceState::Ended)
    );

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn invoke_unknown_command_points_to_invoke_help_only() {
    let project_root = temp_dir("unknown-command-no-bundle-hint");
    let store_root = temp_dir("unknown-command-no-bundle-store");
    let runtime = Runtime::new(
      project_root.clone(),
      LocalStore::new(store_root.clone()).expect("store should create"),
    );
    let request = InvokeRequest {
      command_id: "missing.command".to_string(),
      dry_run: false,
      target: ExecutionTarget::default(),
      inputs: BTreeMap::new(),
    };

    let error = runtime
      .invoke(request)
      .expect_err("unknown command should fail");

    assert!(error.contains("unknown command missing.command"));
    assert!(error.contains("auv-cli invoke --help"));
    assert!(!error.contains("skill bundle"));

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn invoke_succeeds_when_run_finished_recorder_delivery_fails_after_snapshot_write() {
    let project_root = temp_dir("runtime-run-finished-recorder-failure-project");
    let store_root = temp_dir("runtime-run-finished-recorder-failure-store");
    let runtime = runtime_with_success_driver(project_root.clone(), store_root.clone())
      .with_recorder(Arc::new(FailRunFinishedRecorder));

    let result = runtime
      .invoke(InvokeRequest {
        command_id: TEST_COMMAND_ID.to_string(),
        target: ExecutionTarget::default(),
        inputs: BTreeMap::new(),
        dry_run: false,
      })
      .expect("recorder failure after snapshot write should not fail invoke");

    let canonical = runtime
      .read_run(&result.run_id)
      .expect("snapshot should still be persisted");
    assert_eq!(
      canonical.run.status_code,
      auv_tracing_driver::trace::TraceStatusCode::Ok
    );

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn invoke_aborts_before_command_when_required_initial_recording_fails() {
    let project_root = temp_dir("runtime-required-initial-recorder-failure-project");
    let store_root = temp_dir("runtime-required-initial-recorder-failure-store");
    let runtime = Runtime::new(
      project_root.clone(),
      LocalStore::new(store_root.clone()).expect("store should initialize"),
    )
    .with_recorder(Arc::new(RequiredFailRunStartedRecorder));

    let error = runtime
      .invoke(InvokeRequest {
        command_id: TEST_COMMAND_ID.to_string(),
        target: ExecutionTarget::default(),
        inputs: BTreeMap::new(),
        dry_run: false,
      })
      .expect_err("required initial recording failure should abort invoke");

    assert!(error.contains("run recording delivery failed"));
    assert!(error.contains("run started recorder failure"));
    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn invoke_fails_when_required_run_finished_recorder_delivery_fails() {
    let project_root = temp_dir("runtime-required-recorder-failure-project");
    let store_root = temp_dir("runtime-required-recorder-failure-store");
    let runtime = runtime_with_success_driver(project_root.clone(), store_root.clone())
      .with_recorder(Arc::new(RequiredFailRunFinishedRecorder));

    let error = runtime
      .invoke(InvokeRequest {
        command_id: TEST_COMMAND_ID.to_string(),
        target: ExecutionTarget::default(),
        inputs: BTreeMap::new(),
        dry_run: false,
      })
      .expect_err("required recorder delivery failure should fail invoke");

    assert!(error.contains("run recording delivery failed"));
    assert!(error.contains("run finished recorder failure"));
    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

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
      .start_run(auv_tracing_driver::run_builder::RunSpec::new(
        auv_tracing_driver::trace::RunType::Command,
        "auv.command",
      ))
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

    let spec = auv_tracing_driver::run_builder::RunSpec::new(
      auv_tracing_driver::trace::RunType::Command,
      "auv.command",
    )
    .with_device(auv_tracing_driver::trace::DeviceId::new("remote-mac"))
    .with_session(auv_tracing_driver::trace::SessionId::new("music"));
    let run = runtime
      .recording()
      .handle()
      .start_run(spec)
      .expect("explicit-device run should start");
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

    let spec = auv_tracing_driver::run_builder::RunSpec::new(
      auv_tracing_driver::trace::RunType::Command,
      "auv.command",
    )
    .with_device(auv_tracing_driver::trace::DeviceId::new("local"))
    .with_session(auv_tracing_driver::trace::SessionId::new("scan"));
    let run = runtime
      .recording()
      .handle()
      .start_run(spec)
      .expect("run should start");
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

    let canonical = runtime.read_run(&run_id).expect("run snapshot should read");
    let attrs = &canonical.run.attributes;
    assert_eq!(
      attrs.get(auv_tracing_driver::trace::RUN_ATTR_DEVICE_ID),
      Some(&json!("local"))
    );
    assert_eq!(
      attrs.get(auv_tracing_driver::trace::RUN_ATTR_SESSION_ID),
      Some(&json!("scan"))
    );

    // Old on-disk layout invariant: `.auv/runs/{run_id}/` directory, no
    // per-device or per-session subdir inserted.
    let run_dir = store_root.join("runs").join(&run_id);
    assert!(run_dir.exists(), "run dir must remain at runs/{{run_id}}");

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  fn runtime_with_success_driver(project_root: PathBuf, store_root: PathBuf) -> Runtime {
    Runtime::new(
      project_root,
      LocalStore::new(store_root).expect("store should initialize"),
    )
  }
}
