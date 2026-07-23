//! TextEdit product invoke backed by the app-owned typed command report.

#[cfg(test)]
use std::cell::RefCell;
#[cfg(test)]
use std::future::Future;
#[cfg(test)]
use std::time::Duration;

#[cfg(test)]
use auv_apple_textedit::StepOutcome;
use auv_apple_textedit::{
  DocumentCommand, DocumentCommandReport, DocumentWrite, TextEditDriver, VerificationOutcome, run_document_command_with_checkpoint,
};
use auv_cli_invoke::arg::TEXTEDIT_DOCUMENT_WRITE_ARGS;
use auv_cli_invoke::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, InvokeReport, InvokeReportField, InvokeReportSection,
  invoke_command,
};
#[cfg(test)]
use auv_driver::{InputActionResult, InputDeliveryPath};
use auv_runtime::contract::{FailureLayer, VERIFICATION_RESULT_API_VERSION, VerificationMethod, VerificationResult};
use auv_tracing::{Context, EventPayload};

pub const DOCUMENT_WRITE_COMMAND_ID: &str = "app.textedit.document.write";
pub const TEXTEDIT_DOCUMENT_WRITE_KNOWN_LIMIT: &str = "auv.product.textedit.document_write.v0";
pub const TEXTEDIT_DOCUMENT_WRITE_STATE_CHANGED_KNOWN_LIMIT: &str =
  "TextEdit document.write observes only post-write AX text; without a pre-write observation it cannot prove state_changed.";

pub fn group() -> CommandGroup {
  CommandGroup::new("textedit", "TEXTEDIT").command(document_write_invoke_command())
}

#[invoke_command(
  id = "app.textedit.document.write",
  group = "app",
  summary = "Write TextEdit document body through typed AX focus, clipboard paste, and optional AX verification.",
  args = TEXTEDIT_DOCUMENT_WRITE_ARGS,
)]
async fn document_write(input: InvokeCommandInput) -> InvokeCommandResult {
  reject_production_fixture_inputs(&input.inputs)?;
  let command = parse_document_write(&input)?;
  if input.dry_run {
    let mut output = InvokeCommandOutput::new("dry run: app.textedit.document.write");
    output.verification = Some("dry-run; no semantic success claim".to_string());
    output.known_limits.push("app.textedit.document.write dry-run does not touch TextEdit or publish run artifacts.".to_string());
    return Ok(output);
  }
  #[cfg(test)]
  if let Some(driver) = take_fixture_driver() {
    return map_document_write_cli(command, input.cancellation, driver).await.map(|(output, _)| output);
  }
  #[cfg(target_os = "macos")]
  {
    let driver = auv_apple_textedit::MacosTextEditDriver::open_local()?;
    return map_document_write_cli(command, input.cancellation, driver).await.map(|(output, _)| output);
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = (command, input.cancellation);
    Err("app.textedit.document.write live driver requires macOS".to_string())
  }
}

/// Executes the shared TextEdit document-write domain function with a caller-owned driver.
pub async fn write_document<D>(
  command: DocumentWrite,
  cancellation: auv_cli_invoke::InvokeCancellation,
  mut driver: D,
) -> Result<DocumentCommandReport, String>
where
  D: TextEditDriver,
{
  // TODO(textedit-driver-cancellation): checkpoints cannot interrupt one
  // synchronous native driver call; reopen this only when the driver owns a
  // cancellable operation contract.
  cancellation.check().map_err(|error| error.to_string())?;
  let report = run_document_command_with_checkpoint(&DocumentCommand::Write(command), &mut driver, || {
    cancellation.check().map_err(|error| error.to_string())
  })?;
  cancellation.check().map_err(|error| error.to_string())?;
  let context = Context::current();
  for outcome in &report.outcomes {
    if let Some(result) = &outcome.input_action_result {
      cancellation.check().map_err(|error| error.to_string())?;
      auv_runtime::run_read::publish_input_action_result(Some(&context), result)
        .await
        .map_err(|error| format!("failed to publish TextEdit input action result: {error}"))?;
    }
  }
  if let Some(verification) = report.verification.as_ref() {
    cancellation.check().map_err(|error| error.to_string())?;
    context.in_scope(|| {
      auv_tracing::emit_event!(TextEditDocumentWriteVerificationEvent {
        verification: map_verification_result(verification),
      });
    });
  }
  Ok(report)
}

async fn map_document_write_cli<D>(
  command: DocumentWrite,
  cancellation: auv_cli_invoke::InvokeCancellation,
  driver: D,
) -> Result<(InvokeCommandOutput, DocumentCommandReport), String>
where
  D: TextEditDriver,
{
  let report = write_document(command.clone(), cancellation, driver).await?;
  let output = build_invoke_output_from_report(&report, &command)?;
  Ok((output, report))
}

#[derive(serde::Serialize)]
struct TextEditDocumentWriteVerificationEvent {
  verification: VerificationResult,
}

impl EventPayload for TextEditDocumentWriteVerificationEvent {
  const NAME: &'static str = "auv.textedit.document_write.verification";
  const VERSION: u32 = 1;
}

pub fn map_verification_result(verification: &VerificationOutcome) -> VerificationResult {
  VerificationResult {
    api_version: VERIFICATION_RESULT_API_VERSION.to_string(),
    method: VerificationMethod::AxText,
    executed: true,
    state_changed: false,
    semantic_matched: Some(verification.semantic_matched),
    failure_layer: (!verification.semantic_matched).then_some(FailureLayer::SemanticMismatch),
    evidence: Vec::new(),
    consumed_candidate_ref: None,
    consumed_node_ref: None,
    consumed_recognition_artifact_ref: None,
    consumed_recognition_id: None,
    consumed_recognized_item_id: None,
    observed_label: Some(verification.matched_role.clone()),
  }
}

fn reject_production_fixture_inputs(inputs: &std::collections::BTreeMap<String, String>) -> Result<(), String> {
  for name in ["driver", "fixture_observed_text"] {
    if inputs.contains_key(name) {
      return Err(format!("app.textedit.document.write does not accept --{name}"));
    }
  }
  Ok(())
}

pub(crate) fn build_invoke_output_from_report(report: &DocumentCommandReport, command: &DocumentWrite) -> InvokeCommandResult {
  let semantic_matched = report.verification.as_ref().map(|verification| verification.semantic_matched);
  let mut output = InvokeCommandOutput::new(format!(
    "TextEdit document.write completed ({} steps, verify={}, semantic_matched={semantic_matched:?})",
    report.outcomes.len(),
    report.verification.is_some(),
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
  output.verification = Some(match semantic_matched {
    Some(true) => "semantic verification recorded as TextEdit AX text matched=true".to_string(),
    Some(false) => "semantic verification recorded as TextEdit AX text matched=false".to_string(),
    None => "activation and input delivery only; verify=false".to_string(),
  });
  output.known_limits.push(TEXTEDIT_DOCUMENT_WRITE_KNOWN_LIMIT.to_string());
  if report.verification.is_some() {
    output.known_limits.push(TEXTEDIT_DOCUMENT_WRITE_STATE_CHANGED_KNOWN_LIMIT.to_string());
  }
  output.report = Some(document_write_report(report, command));
  if let Some(verification) = report.verification.as_ref().filter(|verification| !verification.semantic_matched) {
    let observed = truncate(&verification.matched_text, 80);
    output.summary =
      format!("TextEdit document.write failed semantic verification (role={}, observed={observed})", verification.matched_role);
    output.failure_message = Some(format!(
      "TextEdit semantic verification failed: expected content was not present in observed AX text role={} observed={observed}",
      verification.matched_role
    ));
  }
  Ok(output)
}

fn document_write_report(report: &DocumentCommandReport, command: &DocumentWrite) -> InvokeReport {
  let mut sections = Vec::new();
  sections.push(InvokeReportSection {
    title: "Steps".to_string(),
    fields: report
      .outcomes
      .iter()
      .map(|outcome| InvokeReportField {
        label: outcome.step_id.to_string(),
        value: outcome.summary.clone(),
      })
      .collect(),
  });
  if let Some(verification) = &report.verification {
    sections.push(InvokeReportSection {
      title: "Verification".to_string(),
      fields: vec![
        InvokeReportField {
          label: "role".to_string(),
          value: verification.matched_role.clone(),
        },
        InvokeReportField {
          label: "observed".to_string(),
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
    format!("{head}...")
  } else {
    head
  }
}

#[cfg(test)]
#[derive(Clone, Debug)]
struct FixtureTextEditDriver {
  content: String,
  role: String,
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
pub(crate) fn fixture_driver(command: &DocumentWrite, observed_text: Option<String>) -> impl TextEditDriver + use<> {
  let mut driver = FixtureTextEditDriver::from_write(command);
  driver.observed_override = observed_text;
  driver
}

#[cfg(test)]
tokio::task_local! {
  static FIXTURE_DRIVER: RefCell<Option<FixtureTextEditDriver>>;
}

#[cfg(test)]
fn take_fixture_driver() -> Option<FixtureTextEditDriver> {
  FIXTURE_DRIVER.try_with(|driver| driver.borrow_mut().take()).ok().flatten()
}

#[cfg(test)]
pub(crate) async fn with_fixture_driver<T>(command: &DocumentWrite, observed_text: Option<String>, future: impl Future<Output = T>) -> T {
  let mut driver = FixtureTextEditDriver::from_write(command);
  driver.observed_override = observed_text;
  FIXTURE_DRIVER.scope(RefCell::new(Some(driver)), future).await
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;

  use auv_tracing::{
    AuthorityId, DispatchTask, ErrorCode, MemoryRunStore, RunId, RunStore, TaskSpawnError, TaskSpawner, TelemetryError, TelemetryItem,
    TelemetryProjector, TelemetryRoutePolicy, configure, dispatcher,
  };

  use super::*;

  struct InvalidInputActionDriver(FixtureTextEditDriver);

  impl InvalidInputActionDriver {
    fn new(command: &DocumentWrite) -> Self {
      Self(FixtureTextEditDriver::from_write(command))
    }
  }

  impl TextEditDriver for InvalidInputActionDriver {
    fn activate_app(&mut self, app_id: &str, settle: Duration) -> Result<StepOutcome, String> {
      self.0.activate_app(app_id, settle)
    }

    fn focus_text_input(&mut self, app_id: &str, query: &str, candidate: &str) -> Result<StepOutcome, String> {
      let mut outcome = self.0.focus_text_input(app_id, query, candidate)?;
      outcome.input_action_result.as_mut().expect("fixture focus action").selected_path = InputDeliveryPath::ClipboardPaste;
      Ok(outcome)
    }

    fn paste_text_preserve_clipboard(
      &mut self,
      app_id: &str,
      text: &str,
      replace_existing: bool,
      settle: Duration,
    ) -> Result<StepOutcome, String> {
      self.0.paste_text_preserve_clipboard(app_id, text, replace_existing, settle)
    }

    fn verify_ax_text(&mut self, app_id: &str, target_text: &str, target_role: &str) -> Result<VerificationOutcome, String> {
      self.0.verify_ax_text(app_id, target_text, target_role)
    }
  }

  struct RejectingSpawner;

  impl TaskSpawner for RejectingSpawner {
    fn spawn(&self, _task: DispatchTask) -> Result<(), TaskSpawnError> {
      Err(TaskSpawnError::new(ErrorCode::parse("auv.test.textedit_spawn_rejected").expect("test error code")))
    }
  }

  struct NoopProjector;

  impl TelemetryProjector for NoopProjector {
    fn project(&self, _item: TelemetryItem) -> auv_tracing::BoxFuture<'_, Result<(), TelemetryError>> {
      Box::pin(async { Ok(()) })
    }

    fn flush(&self) -> auv_tracing::BoxFuture<'_, Result<(), TelemetryError>> {
      Box::pin(async { Ok(()) })
    }
  }

  #[tokio::test]
  async fn direct_fixture_report_preserves_semantic_mismatch() {
    let command = DocumentWrite::defaults_with_content("expected");
    let driver = fixture_driver(&command, Some("different".to_string()));
    let report = write_document(command, auv_cli_invoke::InvokeCancellation::new(), driver).await.expect("fixture report");
    assert_eq!(report.verification.as_ref().map(|value| value.semantic_matched), Some(false));
  }

  #[tokio::test]
  async fn enabled_context_propagates_input_action_validation_failure() {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store).build().expect("memory dispatch");
    let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
    let command = DocumentWrite::defaults_with_content("expected");
    let future =
      root.in_scope(|| write_document(command.clone(), auv_cli_invoke::InvokeCancellation::new(), InvalidInputActionDriver::new(&command)));

    let error = root.instrument(future).await.expect_err("invalid typed evidence must fail enabled publication");

    assert!(error.contains("successful input attempt must match selected_path"), "unexpected publication error: {error}");
  }

  #[tokio::test]
  async fn enabled_context_propagates_input_action_enqueue_failure() {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store).task_spawner(Arc::new(RejectingSpawner)).build().expect("rejecting dispatch");
    let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
    let command = DocumentWrite::defaults_with_content("expected");
    let driver = fixture_driver(&command, None);
    let future = root.in_scope(|| write_document(command, auv_cli_invoke::InvokeCancellation::new(), driver));

    let error = root.instrument(future).await.expect_err("enqueue failure must fail enabled publication");

    assert!(error.contains("failed to publish root artifact"), "unexpected publication error: {error}");
  }

  #[tokio::test]
  async fn disabled_and_telemetry_only_contexts_skip_input_action_publication() {
    let command = DocumentWrite::defaults_with_content("expected");
    let disabled_report =
      write_document(command.clone(), auv_cli_invoke::InvokeCancellation::new(), InvalidInputActionDriver::new(&command))
        .await
        .expect("disabled publication is a no-op");

    let dispatch = configure()
      .project_telemetry(Arc::new(NoopProjector), TelemetryRoutePolicy::fixed_fields_only())
      .build()
      .expect("telemetry-only dispatch");
    let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
    let telemetry_command = command.clone();
    let future = root.in_scope(|| {
      write_document(telemetry_command.clone(), auv_cli_invoke::InvokeCancellation::new(), InvalidInputActionDriver::new(&telemetry_command))
    });
    let telemetry_report = root.instrument(future).await.expect("telemetry-only publication is a no-op");
    dispatch.flush().await.expect("telemetry-only event flush");

    assert_eq!(disabled_report, telemetry_report);
  }

  #[tokio::test]
  async fn fixture_write_records_typed_actions_and_textedit_verification_event() {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
    let command = DocumentWrite::defaults_with_content("expected");
    let driver = fixture_driver(&command, Some("different".to_string()));
    let future = root.in_scope(|| write_document(command, auv_cli_invoke::InvokeCancellation::new(), driver));

    let report = root.instrument(future).await.expect("fixture report");
    dispatch.flush().await.expect("flush TextEdit facts");
    let snapshot = store.load_snapshot(run_id).await.expect("snapshot read").expect("TextEdit run");

    assert_eq!(report.verification.as_ref().map(|value| value.semantic_matched), Some(false));
    assert_eq!(
      snapshot.artifacts().values().filter(|artifact| artifact.metadata().purpose().as_str() == "auv.driver.input_action_result").count(),
      2
    );
    assert!(snapshot.artifacts().values().all(|artifact| artifact.metadata().content_type().to_string() == "application/json"));
    let event = snapshot
      .events()
      .iter()
      .find(|event| event.schema().name().as_str() == "auv.textedit.document_write.verification")
      .expect("TextEdit verification event");
    assert_eq!(event.schema().version().get(), 1);
    let payload: serde_json::Value = serde_json::from_str(event.payload().get()).expect("verification event JSON");
    assert_eq!(payload["verification"]["semantic_matched"], false);
  }
}
