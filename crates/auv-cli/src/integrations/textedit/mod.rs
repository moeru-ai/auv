//! TextEdit product invoke: recorded `app.textedit.document.write`.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use auv_apple_textedit::{
  DocumentCommand, DocumentCommandReport, DocumentWrite, StepOutcome, TextEditDriver, VerificationOutcome, run_document_command,
};
use auv_cli_invoke::arg::TEXTEDIT_DOCUMENT_WRITE_ARGS;
use auv_cli_invoke::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, InvokeReport, InvokeReportField, InvokeReportSection,
  InvokeResult, invoke_command,
};
use auv_driver::{InputActionResult, InputDeliveryPath};
use auv_runtime::contract::{
  ArtifactRef, FailureLayer, OPERATION_RESULT_API_VERSION, OperationOutput, OperationResult, OperationStatus,
  VERIFICATION_RESULT_API_VERSION, VerificationMethod, VerificationResult,
};
use auv_tracing_driver::artifact::ArtifactBytesSource;
use auv_tracing_driver::store::LocalStore;
use auv_tracing_driver::{ProducedArtifact, RunId, now_millis};

pub const DOCUMENT_WRITE_COMMAND_ID: &str = "app.textedit.document.write";
pub const TEXTEDIT_DOCUMENT_WRITE_KNOWN_LIMIT: &str = "auv.product.textedit.document_write.v0";

pub fn group() -> CommandGroup {
  CommandGroup::new("textedit", "TEXTEDIT").command(document_write_invoke_command())
}

/// Product invoke entry is [`crate::invoke::invoke_recorded`].
/// This module only owns the TextEdit handler + operation finalize.

#[invoke_command(
  id = "app.textedit.document.write",
  group = "app",
  summary = "Write TextEdit document body through typed AX focus, clipboard paste, and optional AX verification.",
  args = TEXTEDIT_DOCUMENT_WRITE_ARGS,
)]
fn document_write(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  document_write_impl(input)
}

fn document_write_impl(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  if input.dry_run {
    let mut output = InvokeCommandOutput::new("dry run: app.textedit.document.write");
    output.verification = Some("dry-run; no semantic success claim".to_string());
    output.known_limits.push("app.textedit.document.write dry-run does not touch TextEdit or stage run artifacts.".to_string());
    return Ok(output);
  }

  let command = parse_document_write(&input)?;
  let driver_kind = input.inputs.get("driver").map(String::as_str).unwrap_or("live");
  let report = match driver_kind {
    "fixture" => {
      let mut driver = FixtureTextEditDriver::from_write(&command);
      run_document_command(&DocumentCommand::Write(command.clone()), &mut driver)?
    }
    "live" => {
      #[cfg(target_os = "macos")]
      {
        let mut driver = auv_apple_textedit::MacosTextEditDriver::open_local()?;
        run_document_command(&DocumentCommand::Write(command.clone()), &mut driver)?
      }
      #[cfg(not(target_os = "macos"))]
      {
        return Err("app.textedit.document.write live driver requires macOS".to_string());
      }
    }
    other => return Err(format!("app.textedit.document.write unknown --driver {other}; expected live or fixture")),
  };

  build_invoke_output_from_report(&report, &command)
}

/// Stages evidence artifacts only. Canonical `operation-result` is written after
/// `invoke_recorded` assigns the real run id (see [`persist_canonical_operation_result`]).
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

  for outcome in &report.outcomes {
    if let Some(result) = &outcome.input_action_result {
      output.artifacts.push(json_artifact(
        "input-action-result",
        &format!("textedit-{}-input-action", outcome.step_id.replace('.', "-")),
        result,
        "Typed InputActionResult from TextEdit document.write step.",
      )?);
    }
  }

  if let Some(verification) = &report.verification {
    output.artifacts.push(json_artifact(
      "ax-text-observation",
      "textedit-ax-text-observation",
      verification,
      "AX text observation used for TextEdit semantic verification.",
    )?);
    output.signals.insert("textedit.matched_role".to_string(), verification.matched_role.clone());
    output.signals.insert("textedit.matched_text_len".to_string(), verification.matched_text.len().to_string());
  }

  output.verification = Some(match semantic_matched {
    Some(true) => "semantic verification recorded as VerificationResult method=ax_text matched=true".to_string(),
    Some(false) => "semantic verification recorded as VerificationResult method=ax_text matched=false".to_string(),
    None => "activation and input delivery only; verify=false so no semantic VerificationResult was attached".to_string(),
  });
  output.known_limits.push(TEXTEDIT_DOCUMENT_WRITE_KNOWN_LIMIT.to_string());
  output.report = Some(document_write_report(report, command));
  Ok(output)
}

/// Persist a lineage-complete `operation-result` after invoke assigns `run_id`.
pub fn persist_canonical_operation_result(store: &LocalStore, result: &InvokeResult) -> Result<(), String> {
  if result.command_id != DOCUMENT_WRITE_COMMAND_ID {
    return Ok(());
  }
  if result.dry_run_like() {
    return Ok(());
  }

  let run_id = RunId::new(result.run_id.as_str());
  let evidence_artifacts = result
    .artifacts
    .iter()
    .filter(|artifact| artifact.role == "input-action-result" || artifact.role == "ax-text-observation")
    .map(|artifact| ArtifactRef {
      run_id: run_id.clone(),
      artifact_id: artifact.artifact_id.clone(),
      span_id: artifact.span_id.clone(),
      captured_event_id: artifact.event_id.clone(),
    })
    .collect::<Vec<_>>();

  let observation = read_ax_observation(store, result)?;
  let semantic_matched = observation.as_ref().map(|value| value.semantic_matched);
  let verifications = observation
    .as_ref()
    .map(|verification| {
      vec![VerificationResult {
        api_version: VERIFICATION_RESULT_API_VERSION.to_string(),
        method: VerificationMethod::AxText,
        executed: true,
        state_changed: true,
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

  let operation = OperationResult {
    api_version: OPERATION_RESULT_API_VERSION.to_string(),
    run_id: run_id.clone(),
    status,
    operation_id: DOCUMENT_WRITE_COMMAND_ID.to_string(),
    evidence_artifacts: evidence_artifacts.clone(),
    output: OperationOutput::Acknowledged {
      message: Some(result.output_summary.clone()),
    },
    verifications,
    freshness_basis: None,
    known_limits: known_limits.clone(),
  };

  let rendered = serde_json::to_string_pretty(&operation).map_err(|error| format!("serialize operation-result: {error}"))? + "\n";
  let mut canonical = store.read_run(result.run_id.as_str()).map_err(|error| error.to_string())?;
  if canonical.artifacts.iter().any(|artifact| artifact.role == "operation-result") {
    // Already finalized (or synthetic). Prefer first match; do not duplicate.
    return Ok(());
  }
  let artifact = store
    .stage_artifact_bytes(
      &run_id,
      canonical.artifacts.len(),
      &result.producer_span_id,
      None,
      ArtifactBytesSource {
        role: "operation-result".to_string(),
        bytes: rendered.into_bytes(),
        preferred_name: "operation-result.json".to_string(),
        summary: Some("Canonical TextEdit document.write OperationResult".to_string()),
      },
    )
    .map_err(|error| error.to_string())?;
  canonical.artifacts.push(artifact);
  store.replace_run_snapshot(&canonical).map_err(|error| error.to_string())?;
  let _ = evidence_artifacts;
  let _ = known_limits;
  Ok(())
}

trait InvokeResultExt {
  fn dry_run_like(&self) -> bool;
}

impl InvokeResultExt for InvokeResult {
  fn dry_run_like(&self) -> bool {
    self.verification.as_deref().is_some_and(|value| value.contains("dry-run")) && self.artifacts.is_empty()
  }
}

fn read_ax_observation(store: &LocalStore, result: &InvokeResult) -> Result<Option<VerificationOutcome>, String> {
  let Some(artifact) = result.artifacts.iter().find(|artifact| artifact.role == "ax-text-observation") else {
    return Ok(None);
  };
  let (_record, path) = store.artifact_file(result.run_id.as_str(), artifact.artifact_id.as_str()).map_err(|error| error.to_string())?;
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

fn parse_document_write(input: &InvokeCommandInput<'_>) -> Result<DocumentWrite, String> {
  let content = input
    .inputs
    .get("content")
    .map(String::as_str)
    .ok_or_else(|| "app.textedit.document.write missing required flag --content".to_string())?;
  let mut command = DocumentWrite::defaults_with_content(content);
  if let Some(target) = input.target_application_id {
    command.app_id = target.to_string();
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

fn json_artifact<T: serde::Serialize>(kind: &str, label: &str, value: &T, note: &str) -> Result<ProducedArtifact, String> {
  let source_path =
    PathBuf::from(std::env::temp_dir()).join(format!("auv-invoke-textedit-{label}-{}-{}.json", std::process::id(), now_millis()));
  let body = serde_json::to_vec_pretty(value).map_err(|error| format!("failed to serialize {kind} artifact: {error}"))?;
  fs::write(&source_path, body).map_err(|error| format!("failed to write {kind} artifact: {error}"))?;
  Ok(ProducedArtifact {
    kind: kind.to_string(),
    source_path,
    preferred_name: format!("{label}.json"),
    note: Some(note.to_string()),
  })
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

/// Hermetic TextEdit driver for CI parity (`--driver fixture`).
#[derive(Clone, Debug)]
struct FixtureTextEditDriver {
  content: String,
  role: String,
  /// When set, verify reads this observed body instead of pasted content.
  observed_override: Option<String>,
}

impl FixtureTextEditDriver {
  fn from_write(command: &DocumentWrite) -> Self {
    Self {
      content: command.content.clone(),
      role: command.compare_role.clone(),
      observed_override: None,
    }
  }
}

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

  use auv_cli_invoke::InvokeNamespace;
  use auv_runtime::model::{ExecutionTarget, InvokeRequest};
  use auv_tracing_driver::{MemoryRunRecorder, RunRecordingBackend, store::LocalStore};

  use super::*;
  use crate::product_registry;

  #[test]
  fn fixture_document_write_stages_evidence_artifacts_without_unassigned_operation() {
    let mut inputs = BTreeMap::new();
    inputs.insert("content".to_string(), "hello-fixture".to_string());
    inputs.insert("driver".to_string(), "fixture".to_string());
    let input = InvokeCommandInput {
      command_id: DOCUMENT_WRITE_COMMAND_ID,
      target_application_id: None,
      inputs: &inputs,
      dry_run: false,
    };
    let output = document_write_impl(input).expect("fixture write");
    assert!(output.artifacts.iter().any(|artifact| artifact.kind == "input-action-result"));
    assert!(output.artifacts.iter().any(|artifact| artifact.kind == "ax-text-observation"));
    assert!(!output.artifacts.iter().any(|artifact| artifact.kind == "operation-result"));
    assert_eq!(output.backend.as_deref(), Some("auv-apple-textedit.DocumentWrite"));
  }

  #[test]
  fn document_write_command_metadata_uses_app_namespace() {
    let command = document_write_invoke_command();
    assert_eq!(command.id, DOCUMENT_WRITE_COMMAND_ID);
    assert_eq!(command.namespace, InvokeNamespace::App);
  }

  #[test]
  fn persist_canonical_operation_result_backfills_run_id_and_evidence() {
    let root = std::env::temp_dir().join(format!("auv-textedit-finalize-{}-{}", std::process::id(), now_millis()));
    std::fs::create_dir_all(&root).expect("temp");
    let store = LocalStore::new(root.clone()).expect("store");
    let recording = RunRecordingBackend::new(store.clone(), Arc::new(MemoryRunRecorder::new()));
    let mut inputs = BTreeMap::new();
    inputs.insert("content".to_string(), "hello-fixture".to_string());
    inputs.insert("driver".to_string(), "fixture".to_string());
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
    .expect("invoke");

    let operation = auv_runtime::run_read::read_operation_result(&store, &result.run_id).expect("read").expect("operation-result");
    assert_eq!(operation.run_id.as_str(), result.run_id);
    assert!(!operation.evidence_artifacts.is_empty());
    assert!(operation.evidence_artifacts.iter().all(|artifact| artifact.run_id.as_str() == result.run_id));
    assert!(operation.known_limits.iter().any(|limit| limit == TEXTEDIT_DOCUMENT_WRITE_KNOWN_LIMIT));
    assert_eq!(operation.verifications[0].semantic_matched, Some(true));
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

  #[test]
  fn fixture_document_write_failure_after_run_creation_is_inspectable() {
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
    assert!(store.read_run(&result.run_id).is_ok(), "failed run must remain readable");
    let _ = std::fs::remove_dir_all(root);
  }
}
