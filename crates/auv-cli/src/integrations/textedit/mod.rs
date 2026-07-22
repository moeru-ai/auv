//! TextEdit product invoke: recorded `app.textedit.document.write`.

use std::fs;
use std::path::PathBuf;
#[cfg(test)]
use std::time::Duration;

use auv_apple_textedit::{
  DocumentCommand, DocumentCommandReport, DocumentWrite, TextEditDriver, VerificationOutcome, run_document_command_with_checkpoint,
};
#[cfg(test)]
use auv_apple_textedit::{StepOutcome, run_document_command};
use auv_cli_invoke::arg::TEXTEDIT_DOCUMENT_WRITE_ARGS;
use auv_cli_invoke::{
  ArtifactInstrumentationReceipt, ArtifactPublication, CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult,
  InvokeReport, InvokeReportField, InvokeReportSection, InvokeResult, invoke_command,
};
use auv_driver::INPUT_ACTION_RESULT_ARTIFACT_ROLE;
#[cfg(test)]
use auv_driver::{InputActionResult, InputDeliveryPath};
use auv_runtime::contract::{
  ArtifactRef, FailureLayer, OPERATION_RESULT_API_VERSION, OperationOutput, OperationResult, OperationStatus,
  VERIFICATION_RESULT_API_VERSION, VerificationMethod, VerificationResult,
};
use auv_tracing_driver::artifact::ArtifactBytesSource;
use auv_tracing_driver::trace::{EVENT_API_VERSION, EventRecordV1Alpha1, new_event_id};
use auv_tracing_driver::{RecordingRun, RunId, RunRecordingBackend, SpanRef, now_millis};

pub const DOCUMENT_WRITE_COMMAND_ID: &str = "app.textedit.document.write";
pub const TEXTEDIT_DOCUMENT_WRITE_KNOWN_LIMIT: &str = "auv.product.textedit.document_write.v0";
pub const TEXTEDIT_DOCUMENT_WRITE_STATE_CHANGED_KNOWN_LIMIT: &str =
  "TextEdit document.write observes only post-write AX text; without a pre-write observation it cannot prove state_changed.";

pub fn group() -> CommandGroup {
  CommandGroup::new("textedit", "TEXTEDIT").command(document_write_invoke_command())
}

/// The typed [`write_document`] function is the shared domain entrypoint. The
/// generated command is the CLI adapter; the finalizer below is retained only
/// for the Task22 legacy recorded adapter.

#[invoke_command(
  id = "app.textedit.document.write",
  group = "app",
  summary = "Write TextEdit document body through typed AX focus, clipboard paste, and optional AX verification.",
  args = TEXTEDIT_DOCUMENT_WRITE_ARGS,
)]
async fn document_write(input: InvokeCommandInput) -> InvokeCommandResult {
  document_write_impl(input).await
}

async fn document_write_impl(input: InvokeCommandInput) -> InvokeCommandResult {
  reject_production_fixture_inputs(&input.inputs)?;
  let command = parse_document_write(&input)?;
  if input.dry_run {
    let mut output = InvokeCommandOutput::new("dry run: app.textedit.document.write");
    output.verification = Some("dry-run; no semantic success claim".to_string());
    output.known_limits.push("app.textedit.document.write dry-run does not touch TextEdit or stage run artifacts.".to_string());
    return Ok(output);
  }

  let (report, instrumentation) = write_document(command.clone(), input.cancellation).await?.into_parts();

  let mut output = build_invoke_output_from_report(&report, &command)?;
  output.artifact_failures = instrumentation.into_failures();
  Ok(output)
}

/// Executes TextEdit document write and returns the app-owned report directly.
pub async fn write_document(
  command: DocumentWrite,
  cancellation: auv_cli_invoke::InvokeCancellation,
) -> Result<ArtifactPublication<DocumentCommandReport>, String> {
  #[cfg(target_os = "macos")]
  {
    cancellation.check().map_err(|error| error.to_string())?;
    let driver = auv_apple_textedit::MacosTextEditDriver::open_local()?;
    // TODO(textedit-driver-cancellation): checkpoints cannot interrupt one
    // synchronous MacosTextEditDriver call already in progress. Thread
    // cancellation through the driver only after its API gains an
    // owner-approved cancellable call contract.
    write_document_with_driver(command, cancellation, driver).await
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = (command, cancellation);
    Err("app.textedit.document.write live driver requires macOS".to_string())
  }
}

async fn write_document_with_driver(
  command: DocumentWrite,
  cancellation: auv_cli_invoke::InvokeCancellation,
  mut driver: impl TextEditDriver,
) -> Result<ArtifactPublication<DocumentCommandReport>, String> {
  let report = run_document_command_with_checkpoint(&DocumentCommand::Write(command), &mut driver, || {
    cancellation.check().map_err(|error| error.to_string())
  })?;
  cancellation.check().map_err(|error| error.to_string())?;
  let instrumentation = emit_document_write_artifacts(&report, &cancellation).await?;
  Ok(ArtifactPublication::new(report, instrumentation))
}

#[cfg(test)]
pub(crate) async fn write_document_with_test_driver(
  command: DocumentWrite,
  observed_text: Option<String>,
  cancellation: auv_cli_invoke::InvokeCancellation,
) -> Result<ArtifactPublication<DocumentCommandReport>, String> {
  let mut driver = FixtureTextEditDriver::from_write(&command);
  driver.observed_override = observed_text;
  write_document_with_driver(command, cancellation, driver).await
}

#[cfg(test)]
pub(crate) fn test_document_write_invoke_command() -> auv_cli_invoke::InvokeCommand {
  fn handler(input: InvokeCommandInput) -> auv_cli_invoke::InvokeCommandFuture {
    Box::pin(async move {
      let command = parse_document_write(&input)?;
      if input.dry_run {
        let mut output = InvokeCommandOutput::new("dry run: app.textedit.document.write");
        output.verification = Some("dry-run; no semantic success claim".to_string());
        return Ok(output);
      }
      let observed_text = input.inputs.get("fixture_observed_text").cloned();
      let (report, instrumentation) =
        write_document_with_test_driver(command.clone(), observed_text, input.cancellation).await?.into_parts();
      let mut output = build_invoke_output_from_report(&report, &command)?;
      output.artifact_failures = instrumentation.into_failures();
      Ok(output)
    })
  }

  auv_cli_invoke::command::spec(
    DOCUMENT_WRITE_COMMAND_ID,
    auv_cli_invoke::InvokeNamespace::App,
    "Test-only typed TextEdit document write adapter.",
    TEXTEDIT_DOCUMENT_WRITE_ARGS,
    handler,
  )
}

fn reject_production_fixture_inputs(inputs: &std::collections::BTreeMap<String, String>) -> Result<(), String> {
  for name in ["driver", "fixture_observed_text"] {
    if inputs.contains_key(name) {
      return Err(format!("app.textedit.document.write does not accept --{name}"));
    }
  }
  Ok(())
}

/// Maps the typed report into CLI presentation.
pub(crate) fn build_invoke_output_from_report(report: &DocumentCommandReport, command: &DocumentWrite) -> InvokeCommandResult {
  let semantic_matched = report.verification.as_ref().map(|verification| verification.semantic_matched);
  let mut output = InvokeCommandOutput::new(format!(
    "TextEdit document.write completed ({} steps, verify={}, semantic_matched={:?})",
    report.outcomes.len(),
    report.verification.is_some(),
    semantic_matched
  ));
  output.backend = Some("auv-apple-textedit.DocumentWrite".to_string());
  output.signals.insert("textedit.command".to_string(), report.command.to_string());
  output.signals.insert("textedit.app_id".to_string(), command.app_id.clone());
  output.signals.insert("textedit.replace".to_string(), command.replace.to_string());
  output.signals.insert("textedit.verify_requested".to_string(), command.verify.to_string());
  output.signals.insert("textedit.verification_present".to_string(), report.verification.is_some().to_string());
  if let Some(matched) = semantic_matched {
    output.signals.insert("textedit.semantic_matched".to_string(), matched.to_string());
  }

  if let Some(verification) = &report.verification {
    output.signals.insert("textedit.matched_role".to_string(), verification.matched_role.clone());
    output.signals.insert("textedit.matched_text_len".to_string(), verification.matched_text.len().to_string());
  }

  output.verification = Some(match semantic_matched {
    Some(true) => "semantic verification recorded as VerificationResult method=ax_text matched=true".to_string(),
    Some(false) => "semantic verification recorded as VerificationResult method=ax_text matched=false".to_string(),
    None => "activation and input delivery only; verify=false so no semantic VerificationResult was attached".to_string(),
  });
  output.known_limits.push(TEXTEDIT_DOCUMENT_WRITE_KNOWN_LIMIT.to_string());
  if report.verification.is_some() {
    output.known_limits.push(TEXTEDIT_DOCUMENT_WRITE_STATE_CHANGED_KNOWN_LIMIT.to_string());
  }
  output.report = Some(document_write_report(report, command));
  if semantic_matched == Some(false) {
    let verification = report.verification.as_ref().expect("semantic match status requires verification");
    let observed = truncate(&verification.matched_text, 80);
    output.summary =
      format!("TextEdit document.write failed semantic verification (role={}, observed={observed})", verification.matched_role);
    output.failure_message = Some(format!(
      "TextEdit semantic verification failed: expected content was not present in observed AX text role={} observed={observed}",
      verification.matched_role
    ));
  }
  // TODO(task22-textedit-legacy-evidence): the recorded adapter intentionally
  // receives no path evidence; remove its evidence reconstruction when the
  // legacy finalizer is retired in Task 22.
  Ok(output)
}

async fn emit_document_write_artifacts(
  report: &DocumentCommandReport,
  cancellation: &auv_cli_invoke::InvokeCancellation,
) -> Result<ArtifactInstrumentationReceipt, String> {
  let mut instrumentation = ArtifactInstrumentationReceipt::default();
  for outcome in &report.outcomes {
    if let Some(result) = &outcome.input_action_result {
      cancellation.check().map_err(|error| error.to_string())?;
      instrumentation.publish_json("auv.driver.input_action_result", result).await;
    }
  }
  if let Some(verification) = &report.verification {
    cancellation.check().map_err(|error| error.to_string())?;
    instrumentation.publish_json("auv.textedit.ax_text_observation", verification).await;
  }
  cancellation.check().map_err(|error| error.to_string())?;
  instrumentation.publish_json("auv.textedit.document_write_result", report).await;
  Ok(instrumentation)
}

/// Finalizes TextEdit output for the Task22 legacy recorded adapter.
pub fn finalize_recorded_invoke(
  recording: &RunRecordingBackend,
  run: &mut RecordingRun,
  producer_span: &SpanRef,
  result: &mut InvokeResult,
) -> Result<(), String> {
  if result.command_id != DOCUMENT_WRITE_COMMAND_ID {
    return Ok(());
  }
  if result.dry_run_like() {
    return Ok(());
  }
  if result.artifacts.iter().any(|artifact| artifact.role == "operation-result") {
    return Ok(());
  }

  let observation = read_ax_observation(recording, result)?;
  if let Some(verification) = observation.as_ref()
    && !verification.semantic_matched
  {
    apply_semantic_mismatch(result, verification);
  }

  let operation = build_canonical_operation_result(result, observation.as_ref());
  let rendered = serde_json::to_string_pretty(&operation).map_err(|error| format!("serialize operation-result: {error}"))? + "\n";
  let (artifact, path) = stage_operation_result_artifact(recording, run, producer_span, rendered.into_bytes())?;
  result.artifacts.push(artifact);
  result.artifact_paths.push(path);
  Ok(())
}

fn build_canonical_operation_result(result: &InvokeResult, observation: Option<&VerificationOutcome>) -> OperationResult {
  let run_id = RunId::new(result.run_id.as_str());
  let evidence_artifacts = result
    .artifacts
    .iter()
    .filter(|artifact| artifact.role == INPUT_ACTION_RESULT_ARTIFACT_ROLE || artifact.role == "ax-text-observation")
    .map(|artifact| ArtifactRef {
      run_id: run_id.clone(),
      artifact_id: artifact.artifact_id.clone(),
      span_id: artifact.span_id.clone(),
      captured_event_id: artifact.event_id.clone(),
    })
    .collect::<Vec<_>>();
  let semantic_matched = observation.map(|value| value.semantic_matched);
  let verifications = observation
    .map(|verification| {
      vec![VerificationResult {
        api_version: VERIFICATION_RESULT_API_VERSION.to_string(),
        method: VerificationMethod::AxText,
        executed: true,
        // NOTICE(textedit-state-changed): only post-write AX text is recorded.
        // Keep this false until a pre-write observation is available for comparison.
        state_changed: false,
        semantic_matched: Some(verification.semantic_matched),
        failure_layer: (!verification.semantic_matched).then_some(FailureLayer::SemanticMismatch),
        evidence: evidence_artifacts
          .iter()
          .filter(|artifact_ref| {
            result
              .artifacts
              .iter()
              .any(|artifact| artifact.artifact_id == artifact_ref.artifact_id && artifact.role == "ax-text-observation")
          })
          .cloned()
          .collect(),
        consumed_candidate_ref: None,
        consumed_node_ref: None,
        consumed_recognition_artifact_ref: None,
        consumed_recognition_id: None,
        consumed_recognized_item_id: None,
        observed_label: Some(format!("{} / {}", verification.matched_role, truncate(&verification.matched_text, 80))),
      }]
    })
    .unwrap_or_default();
  let status = match (result.status.clone(), semantic_matched) {
    (_, Some(false)) => OperationStatus::Failed,
    (auv_cli_invoke::RunStatus::Failed, _) => OperationStatus::Failed,
    (auv_cli_invoke::RunStatus::Completed, _) => OperationStatus::Completed,
  };
  let mut known_limits = result.known_limits.clone();
  if !known_limits.iter().any(|limit| limit == TEXTEDIT_DOCUMENT_WRITE_KNOWN_LIMIT) {
    known_limits.push(TEXTEDIT_DOCUMENT_WRITE_KNOWN_LIMIT.to_string());
  }
  if observation.is_some() && !known_limits.iter().any(|limit| limit == TEXTEDIT_DOCUMENT_WRITE_STATE_CHANGED_KNOWN_LIMIT) {
    known_limits.push(TEXTEDIT_DOCUMENT_WRITE_STATE_CHANGED_KNOWN_LIMIT.to_string());
  }
  OperationResult {
    api_version: OPERATION_RESULT_API_VERSION.to_string(),
    run_id: run_id.clone(),
    status,
    operation_id: DOCUMENT_WRITE_COMMAND_ID.to_string(),
    evidence_artifacts,
    output: OperationOutput::Acknowledged {
      message: Some(result.output_summary.clone()),
    },
    verifications,
    freshness_basis: None,
    known_limits,
  }
}

fn apply_semantic_mismatch(result: &mut InvokeResult, verification: &VerificationOutcome) {
  let observed = truncate(&verification.matched_text, 80);
  result.status = auv_cli_invoke::RunStatus::Failed;
  result.output_summary =
    format!("TextEdit document.write failed semantic verification (role={}, observed={observed})", verification.matched_role);
  result.failure_message = Some(format!(
    "TextEdit semantic verification failed: expected content was not present in observed AX text role={} observed={observed}",
    verification.matched_role
  ));
}

fn stage_operation_result_artifact(
  recording: &RunRecordingBackend,
  run: &mut RecordingRun,
  producer_span: &SpanRef,
  bytes: Vec<u8>,
) -> Result<(auv_tracing_driver::trace::ArtifactRecordV1Alpha1, PathBuf), String> {
  let event_id = new_event_id();
  let artifact = recording
    .stage_artifact_bytes(
      run.id(),
      run.artifact_count(),
      producer_span.id(),
      Some(event_id.clone()),
      ArtifactBytesSource {
        role: "operation-result".to_string(),
        bytes,
        preferred_name: "operation-result.json".to_string(),
        summary: Some("Canonical TextEdit document.write OperationResult".to_string()),
      },
    )
    .map_err(|error| error.to_string())?;
  run.record_event(EventRecordV1Alpha1 {
    api_version: EVENT_API_VERSION.to_string(),
    event_id,
    span_id: producer_span.id().clone(),
    name: "artifact.captured".to_string(),
    timestamp_millis: now_millis(),
    attributes: Default::default(),
    message: Some(render_artifact_event(&artifact)),
    artifact_ids: vec![artifact.artifact_id.clone()],
  });
  let staged_path = recording.run_dir(run.id()).map_err(|error| error.to_string())?.join(&artifact.path);
  run.record_artifact(artifact.clone());
  recording.record_artifact_bytes(run.id(), &artifact, &staged_path).map_err(|error| error.to_string())?;
  Ok((artifact, staged_path))
}

fn render_artifact_event(artifact: &auv_tracing_driver::trace::ArtifactRecordV1Alpha1) -> String {
  let note = artifact.summary.clone().unwrap_or_else(|| "n/a".to_string());
  format!("{} kind={} path={} note={}", artifact.artifact_id, artifact.role, artifact.path, note)
}

trait InvokeResultExt {
  fn dry_run_like(&self) -> bool;
}

impl InvokeResultExt for InvokeResult {
  fn dry_run_like(&self) -> bool {
    self.verification.as_deref().is_some_and(|value| value.contains("dry-run")) && self.artifacts.is_empty()
  }
}

fn read_ax_observation(recording: &RunRecordingBackend, result: &InvokeResult) -> Result<Option<VerificationOutcome>, String> {
  let Some(artifact) = result.artifacts.iter().find(|artifact| artifact.role == "ax-text-observation") else {
    return Ok(None);
  };
  let path = recording.run_dir(result.run_id.as_str()).map_err(|error| error.to_string())?.join(&artifact.path);
  let body = fs::read_to_string(&path).map_err(|error| format!("read ax-text-observation: {error}"))?;
  let observation: VerificationOutcome = serde_json::from_str(&body).map_err(|error| format!("decode ax-text-observation: {error}"))?;
  Ok(Some(observation))
}

fn document_write_report(report: &DocumentCommandReport, command: &DocumentWrite) -> InvokeReport {
  let mut sections = vec![InvokeReportSection {
    title: "Steps".to_string(),
    fields: report
      .outcomes
      .iter()
      .map(|outcome| InvokeReportField {
        label: outcome.step_id.to_string(),
        value: outcome.summary.clone(),
      })
      .collect(),
  }];
  if let Some(verification) = &report.verification {
    sections.push(InvokeReportSection {
      title: "Verification".to_string(),
      fields: vec![
        InvokeReportField {
          label: "role".to_string(),
          value: verification.matched_role.clone(),
        },
        InvokeReportField {
          label: "observed_text".to_string(),
          value: truncate(&verification.matched_text, 120),
        },
        InvokeReportField {
          label: "semantic_matched".to_string(),
          value: verification.semantic_matched.to_string(),
        },
      ],
    });
  }
  InvokeReport::new(
    vec![
      InvokeReportField {
        label: "Command".to_string(),
        value: DOCUMENT_WRITE_COMMAND_ID.to_string(),
      },
      InvokeReportField {
        label: "App".to_string(),
        value: command.app_id.clone(),
      },
      InvokeReportField {
        label: "Replace".to_string(),
        value: command.replace.to_string(),
      },
      InvokeReportField {
        label: "Verify".to_string(),
        value: command.verify.to_string(),
      },
    ],
    sections,
  )
}

fn parse_document_write(input: &InvokeCommandInput) -> Result<DocumentWrite, String> {
  let content = input
    .inputs
    .get("content")
    .map(String::as_str)
    .ok_or_else(|| "app.textedit.document.write missing required flag --content".to_string())?;
  let mut command = DocumentWrite::defaults_with_content(content);
  if let Some(target) = &input.target_application_id {
    command.app_id = target.clone();
  }
  if let Some(replace) = input.inputs.get("replace") {
    command.replace = parse_bool(replace, "replace")?;
  }
  if let Some(verify) = input.inputs.get("verify") {
    command.verify = parse_bool(verify, "verify")?;
  }
  Ok(command)
}

fn parse_bool(value: &str, name: &str) -> Result<bool, String> {
  match value.trim().to_ascii_lowercase().as_str() {
    "true" | "1" | "yes" => Ok(true),
    "false" | "0" | "no" => Ok(false),
    other => Err(format!("invalid --{name} value {other}; expected true or false")),
  }
}

fn truncate(value: &str, max_chars: usize) -> String {
  let mut chars = value.chars();
  let head: String = chars.by_ref().take(max_chars).collect();
  if chars.next().is_some() {
    format!("{head}…")
  } else {
    head
  }
}

/// Hermetic TextEdit driver available only to this module's unit tests.
#[cfg(test)]
#[derive(Clone, Debug)]
struct FixtureTextEditDriver {
  content: String,
  role: String,
  /// When set, verify reads this observed body instead of pasted content.
  observed_override: Option<String>,
}

#[cfg(test)]
impl FixtureTextEditDriver {
  fn from_write(command: &DocumentWrite) -> Self {
    Self {
      content: command.content.clone(),
      role: command.compare_role.clone(),
      observed_override: None,
    }
  }
}

#[cfg(test)]
impl TextEditDriver for FixtureTextEditDriver {
  fn activate_app(&mut self, app_id: &str, settle: Duration) -> Result<StepOutcome, String> {
    Ok(StepOutcome {
      step_id: "activate",
      summary: format!("fixture activated {app_id} settle_ms={}", settle.as_millis()),
      input_action_result: None,
    })
  }

  fn focus_text_input(&mut self, app_id: &str, query: &str, candidate: &str) -> Result<StepOutcome, String> {
    Ok(StepOutcome {
      step_id: "focus",
      summary: format!("fixture focused {app_id} query={query} candidate={candidate}"),
      input_action_result: Some(InputActionResult::single_success(InputDeliveryPath::AxFocus)),
    })
  }

  fn paste_text_preserve_clipboard(
    &mut self,
    app_id: &str,
    text: &str,
    replace_existing: bool,
    settle: Duration,
  ) -> Result<StepOutcome, String> {
    self.content = text.to_string();
    Ok(StepOutcome {
      step_id: "paste",
      summary: format!("fixture pasted into {app_id} replace={replace_existing} settle_ms={}", settle.as_millis()),
      input_action_result: Some(InputActionResult::single_success(InputDeliveryPath::ClipboardPaste)),
    })
  }

  fn verify_ax_text(&mut self, _app_id: &str, target_text: &str, target_role: &str) -> Result<VerificationOutcome, String> {
    self.role = target_role.to_string();
    let observed = self.observed_override.clone().unwrap_or_else(|| self.content.clone());
    Ok(VerificationOutcome {
      matched_role: target_role.to_string(),
      matched_text: observed.clone(),
      artifact_count: 1,
      semantic_matched: observed.contains(target_text),
      observation_path: Some("fixture.0.1.2".to_string()),
      observation_pid: Some(0),
    })
  }
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;
  use std::sync::Arc;
  use std::time::Duration;

  use auv_cli_invoke::InvokeNamespace;
  use auv_runtime::model::{ExecutionTarget, InvokeRequest};
  use auv_tracing_driver::{MemoryRunRecorder, RunRecordingBackend, TraceStatusCode, store::LocalStore};

  use super::*;
  use crate::product_registry;

  fn fixture_registry() -> auv_cli_invoke::InvokeRegistry {
    auv_cli_invoke::InvokeRegistry::from_groups(vec![
      auv_cli_invoke::CommandGroup::new("textedit-test", "TEXTEDIT TEST").command(test_document_write_invoke_command()),
    ])
  }

  struct ScopedTestFile(PathBuf);

  impl Drop for ScopedTestFile {
    fn drop(&mut self) {
      let _ = std::fs::remove_file(&self.0);
    }
  }

  struct CancelAfterActivateDriver {
    cancellation: auv_cli_invoke::InvokeCancellation,
    resource_path: PathBuf,
    resource: Option<ScopedTestFile>,
    phases: Arc<std::sync::Mutex<Vec<&'static str>>>,
  }

  impl TextEditDriver for CancelAfterActivateDriver {
    fn activate_app(&mut self, _app_id: &str, _settle: Duration) -> Result<StepOutcome, String> {
      self.phases.lock().unwrap().push("activate");
      std::fs::write(&self.resource_path, b"acquired").map_err(|error| error.to_string())?;
      self.resource = Some(ScopedTestFile(self.resource_path.clone()));
      self.cancellation.cancel();
      Ok(StepOutcome {
        step_id: "activate",
        summary: "activated".to_string(),
        input_action_result: None,
      })
    }

    fn focus_text_input(&mut self, _app_id: &str, _query: &str, _candidate: &str) -> Result<StepOutcome, String> {
      self.phases.lock().unwrap().push("focus");
      unreachable!("focus must not run after cancellation")
    }

    fn paste_text_preserve_clipboard(
      &mut self,
      _app_id: &str,
      _text: &str,
      _replace_existing: bool,
      _settle: Duration,
    ) -> Result<StepOutcome, String> {
      self.phases.lock().unwrap().push("paste");
      unreachable!("paste must not run after cancellation")
    }

    fn verify_ax_text(&mut self, _app_id: &str, _target_text: &str, _target_role: &str) -> Result<VerificationOutcome, String> {
      self.phases.lock().unwrap().push("verify");
      unreachable!("verification must not run after cancellation")
    }
  }

  #[tokio::test]
  async fn production_document_write_rejects_fixture_input() {
    let input = InvokeCommandInput {
      command_id: DOCUMENT_WRITE_COMMAND_ID.to_string(),
      target_application_id: None,
      inputs: BTreeMap::from([
        ("content".to_string(), "fixture".to_string()),
        ("driver".to_string(), "fixture".to_string()),
      ]),
      dry_run: false,
      cancellation: auv_cli_invoke::InvokeCancellation::new(),
    };

    let error = document_write_impl(input).await.expect_err("production fixture selection must be rejected");
    assert!(error.contains("does not accept --driver"));
  }

  #[tokio::test]
  async fn cancellation_after_activate_cleans_the_resource_and_skips_later_textedit_phases() {
    let cancellation = auv_cli_invoke::InvokeCancellation::new();
    let resource_path = std::env::temp_dir().join(format!("auv-textedit-cancel-resource-{}-{}", std::process::id(), now_millis()));
    let phases = Arc::new(std::sync::Mutex::new(Vec::new()));
    let driver = CancelAfterActivateDriver {
      cancellation: cancellation.clone(),
      resource_path: resource_path.clone(),
      resource: None,
      phases: phases.clone(),
    };

    let error = write_document_with_driver(DocumentWrite::defaults_with_content("cancel"), cancellation, driver)
      .await
      .expect_err("cancellation must stop the TextEdit workflow");

    assert_eq!(error, "invoke cancelled");
    assert_eq!(*phases.lock().unwrap(), vec!["activate"]);
    assert!(!resource_path.exists(), "cancellation must drop the acquired scoped resource");
  }

  fn textedit_temp_artifacts() -> Vec<PathBuf> {
    let mut paths = std::fs::read_dir(std::env::temp_dir())
      .expect("read temp directory")
      .filter_map(Result::ok)
      .map(|entry| entry.path())
      .filter(|path| path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with("auv-invoke-textedit-")))
      .collect::<Vec<_>>();
    paths.sort();
    paths
  }

  #[tokio::test]
  async fn fixture_document_write_without_authority_creates_no_path_artifacts() {
    let command = DocumentWrite::defaults_with_content("hello-fixture");
    let publication =
      write_document_with_test_driver(command.clone(), None, auv_cli_invoke::InvokeCancellation::new()).await.expect("test driver write");
    let (report, instrumentation) = publication.into_parts();
    let output = build_invoke_output_from_report(&report, &command).expect("fixture output");

    assert!(output.artifacts.is_empty());
    assert!(instrumentation.failures().is_empty());
    assert_eq!(output.backend.as_deref(), Some("auv-apple-textedit.DocumentWrite"));
  }

  #[tokio::test]
  async fn fixture_document_write_without_ax_verification_omits_state_change_known_limit() {
    let mut command = DocumentWrite::defaults_with_content("hello-fixture");
    command.verify = false;
    let report = write_document_with_test_driver(command.clone(), None, auv_cli_invoke::InvokeCancellation::new())
      .await
      .expect("test driver write without verification");
    let output = build_invoke_output_from_report(report.value(), &command).expect("fixture output");

    assert!(!output.known_limits.iter().any(|limit| limit == TEXTEDIT_DOCUMENT_WRITE_STATE_CHANGED_KNOWN_LIMIT));
  }

  #[test]
  fn document_write_command_metadata_uses_app_namespace() {
    let command = document_write_invoke_command();
    assert_eq!(command.id, DOCUMENT_WRITE_COMMAND_ID);
    assert_eq!(command.namespace, InvokeNamespace::App);
  }

  #[test]
  fn invoke_recorded_finalize_backfills_run_id_and_result_artifacts() {
    let root = std::env::temp_dir().join(format!("auv-textedit-finalize-{}-{}", std::process::id(), now_millis()));
    std::fs::create_dir_all(&root).expect("temp");
    let store = LocalStore::new(root.clone()).expect("store");
    let recording = RunRecordingBackend::new(store.clone(), Arc::new(MemoryRunRecorder::new()));
    let mut inputs = BTreeMap::new();
    inputs.insert("content".to_string(), "hello-fixture".to_string());
    let result = crate::invoke::invoke_recorded(
      &recording,
      &fixture_registry(),
      InvokeRequest {
        command_id: DOCUMENT_WRITE_COMMAND_ID.to_string(),
        target: ExecutionTarget::default(),
        inputs,
        dry_run: false,
      },
    )
    .expect("invoke");

    let operation = auv_runtime::run_read::read_operation_result(&store, &result.run_id).expect("read").expect("operation-result");
    assert_eq!(operation.run_id.as_str(), result.run_id);
    assert!(operation.evidence_artifacts.is_empty());
    assert!(operation.known_limits.iter().any(|limit| limit == TEXTEDIT_DOCUMENT_WRITE_KNOWN_LIMIT));
    assert!(operation.verifications.is_empty());
    assert!(operation.known_limits.iter().any(|limit| limit == TEXTEDIT_DOCUMENT_WRITE_STATE_CHANGED_KNOWN_LIMIT));
    assert!(result.artifacts.iter().any(|artifact| artifact.role == "operation-result"));
    assert_eq!(result.artifacts.len(), result.artifact_paths.len());
    let _ = std::fs::remove_dir_all(root);
  }

  #[test]
  fn fixture_semantic_mismatch_persists_failed_verification() {
    let mut driver = FixtureTextEditDriver {
      content: "pasted".to_string(),
      role: "AXTextArea".to_string(),
      observed_override: Some("observed-without-expected".to_string()),
    };
    let command = DocumentWrite::defaults_with_content("expected-marker");
    let report = run_document_command(&DocumentCommand::Write(command.clone()), &mut driver).expect("report");
    let verification = report.verification.as_ref().expect("verification");
    assert!(!verification.semantic_matched);
    assert_eq!(verification.matched_text, "observed-without-expected");

    let output = build_invoke_output_from_report(&report, &command).expect("output");
    assert_eq!(output.signals.get("textedit.semantic_matched").map(String::as_str), Some("false"));
  }

  #[tokio::test]
  async fn typed_document_write_returns_semantic_mismatch_without_cli_context() {
    let command = DocumentWrite::defaults_with_content("expected-marker");
    let report =
      write_document_with_test_driver(command, Some("observed-without-expected".to_string()), auv_cli_invoke::InvokeCancellation::new())
        .await
        .expect("typed TextEdit call");

    assert_eq!(report.value().verification.as_ref().map(|verification| verification.semantic_matched), Some(false));
  }

  #[tokio::test]
  async fn no_context_success_and_error_leave_no_textedit_temp_artifacts() {
    let before = textedit_temp_artifacts();
    let command = DocumentWrite::defaults_with_content("fixture");

    let publication =
      write_document_with_test_driver(command.clone(), None, auv_cli_invoke::InvokeCancellation::new()).await.expect("fixture write");
    assert!(publication.value().outcomes.iter().any(|outcome| outcome.input_action_result.is_some()));

    let invalid = document_write_impl(InvokeCommandInput {
      command_id: DOCUMENT_WRITE_COMMAND_ID.to_string(),
      target_application_id: None,
      inputs: BTreeMap::from([
        ("content".to_string(), "fixture".to_string()),
        ("driver".to_string(), "invalid".to_string()),
      ]),
      dry_run: false,
      cancellation: auv_cli_invoke::InvokeCancellation::new(),
    })
    .await;
    assert!(invalid.is_err());

    assert_eq!(textedit_temp_artifacts(), before);
  }

  // ROOT CAUSE:
  //
  // The in-lifecycle finalizer tried to resolve the AX artifact through
  // `LocalStore::artifact_file`, which requires the not-yet-written `run.json`.
  //
  // Before the fix, finalization failed before it could synchronize the invoke,
  // run, and operation statuses. The fix reads the already-staged artifact.
  // https://github.com/moeru-ai/auv/pull/102#issuecomment-4958351155
  #[test]
  fn recorded_semantic_mismatch_keeps_result_run_and_operation_in_sync() {
    let root = std::env::temp_dir().join(format!("auv-textedit-mismatch-{}-{}", std::process::id(), now_millis()));
    std::fs::create_dir_all(&root).expect("temp");
    let store = LocalStore::new(root.clone()).expect("store");
    let recording = RunRecordingBackend::new(store.clone(), Arc::new(MemoryRunRecorder::new()));
    let mut inputs = BTreeMap::new();
    inputs.insert("content".to_string(), "expected-marker".to_string());
    inputs.insert("fixture_observed_text".to_string(), "observed-without-expected".to_string());
    let result = crate::invoke::invoke_recorded(
      &recording,
      &fixture_registry(),
      InvokeRequest {
        command_id: DOCUMENT_WRITE_COMMAND_ID.to_string(),
        target: ExecutionTarget::default(),
        inputs,
        dry_run: false,
      },
    )
    .expect("invoke");

    assert_eq!(result.status, auv_cli_invoke::RunStatus::Failed);
    assert!(result.failure_message.as_deref().is_some_and(|message| message.contains("semantic verification failed")));
    assert!(result.artifacts.iter().any(|artifact| artifact.role == "operation-result"));
    assert_eq!(result.artifacts.len(), result.artifact_paths.len());

    let canonical = store.read_run(&result.run_id).expect("run");
    assert_eq!(canonical.run.status_code, TraceStatusCode::Error);
    let command_span = canonical.spans.iter().find(|span| span.name == "auv.command.invoke").expect("command span");
    assert_eq!(command_span.status_code, TraceStatusCode::Error);
    assert!(canonical.artifacts.iter().any(|artifact| artifact.role == "operation-result"));

    let operation = auv_runtime::run_read::read_operation_result(&store, &result.run_id).expect("read").expect("operation-result");
    assert_eq!(operation.status, OperationStatus::Failed);
    assert!(operation.verifications.is_empty());
    assert!(operation.evidence_artifacts.is_empty());
    let operation_artifact =
      canonical.artifacts.iter().find(|artifact| artifact.role == "operation-result").expect("operation-result artifact");
    assert_eq!(Some(&operation_artifact.span_id), result.producer_span_id.as_ref());

    let _ = std::fs::remove_dir_all(root);
  }

  #[test]
  fn rejected_production_fixture_input_after_run_creation_is_inspectable() {
    let root = std::env::temp_dir().join(format!("auv-textedit-failed-run-{}-{}", std::process::id(), now_millis()));
    std::fs::create_dir_all(&root).expect("temp");
    let store = LocalStore::new(root.clone()).expect("store");
    let recording = RunRecordingBackend::new(store.clone(), Arc::new(MemoryRunRecorder::new()));
    let mut inputs = BTreeMap::new();
    inputs.insert("content".to_string(), "x".to_string());
    inputs.insert("driver".to_string(), "not-a-driver".to_string());
    let result = crate::invoke::invoke_recorded(
      &recording,
      &product_registry(),
      InvokeRequest {
        command_id: DOCUMENT_WRITE_COMMAND_ID.to_string(),
        target: ExecutionTarget::default(),
        inputs,
        dry_run: false,
      },
    )
    .expect("failed handler still returns InvokeResult after run creation");
    assert_eq!(result.status, auv_cli_invoke::RunStatus::Failed);
    assert!(result.artifacts.iter().any(|artifact| artifact.role == "operation-result"));
    let canonical = store.read_run(&result.run_id).expect("failed run must remain readable");
    assert_eq!(canonical.run.status_code, TraceStatusCode::Error);
    let operation = auv_runtime::run_read::read_operation_result(&store, &result.run_id).expect("read").expect("operation-result");
    assert_eq!(operation.status, OperationStatus::Failed);
    let _ = std::fs::remove_dir_all(root);
  }
}
