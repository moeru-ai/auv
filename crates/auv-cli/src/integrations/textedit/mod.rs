//! TextEdit product invoke backed by the app-owned typed command report.

use auv_apple_textedit::{
  DocumentCommand, DocumentCommandReport, DocumentWrite, TextEditAction, TextEditDriver, VerificationOutcome,
  run_document_command_with_checkpoint,
};
use auv_cli_invoke::arg::TEXTEDIT_DOCUMENT_WRITE_ARGS;
use auv_cli_invoke::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, InvokeReport, InvokeReportField, InvokeReportSection,
  invoke_command,
};
use auv_driver::DriverError;
use auv_tracing::{Context, EventPayload};

pub const DOCUMENT_WRITE_COMMAND_ID: &str = "app.textedit.document.write";

pub fn group() -> CommandGroup {
  CommandGroup::new("textedit", "TEXTEDIT").command(document_write_invoke_command())
}

#[invoke_command(
  id = "app.textedit.document.write",
  group = "app",
  description = "Write TextEdit document body through typed AX focus, clipboard paste, and optional AX verification.",
  args = TEXTEDIT_DOCUMENT_WRITE_ARGS,
)]
async fn document_write(input: InvokeCommandInput) -> InvokeCommandResult {
  reject_production_fixture_inputs(&input.inputs)?;
  let command = parse_document_write(&input)?;
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  #[cfg(target_os = "macos")]
  {
    let driver = auv_apple_textedit::MacosTextEditDriver::open_local().map_err(|error| error.to_string())?;
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
  driver: D,
) -> Result<DocumentCommandReport, String>
where
  D: TextEditDriver,
{
  execute_document_write(command, cancellation, driver).await.map_err(DocumentWriteFailure::into_message)
}

#[derive(Debug)]
pub(crate) struct DocumentWriteFailure {
  message: String,
}

impl DocumentWriteFailure {
  fn new(message: String) -> Self {
    Self { message }
  }

  pub(crate) fn into_message(self) -> String {
    self.message
  }
}

impl From<DriverError> for DocumentWriteFailure {
  fn from(error: DriverError) -> Self {
    Self::new(error.to_string())
  }
}

pub(crate) async fn execute_document_write<D>(
  command: DocumentWrite,
  cancellation: auv_cli_invoke::InvokeCancellation,
  mut driver: D,
) -> Result<DocumentCommandReport, DocumentWriteFailure>
where
  D: TextEditDriver,
{
  // TODO(textedit-driver-cancellation): checkpoints cannot interrupt one
  // synchronous native driver call; reopen this only when the driver owns a
  // cancellable operation contract.
  cancellation.check().map_err(|error| DocumentWriteFailure::new(error.to_string()))?;
  let report = run_document_command_with_checkpoint(&DocumentCommand::Write(command), &mut driver, || {
    cancellation.check().map_err(|error| DocumentWriteFailure::new(error.to_string()))
  })?;
  if let Err(error) = cancellation.check() {
    return Err(DocumentWriteFailure::new(error.to_string()));
  }
  let context = Context::current();
  for action in &report.actions {
    if let Some(result) = &action.input_action_result {
      context.in_scope(|| auv_runtime::run_read::emit_input_action_result(result));
    }
  }
  if report.verification.is_some()
    && let Err(error) = cancellation.check()
  {
    return Err(DocumentWriteFailure::new(error.to_string()));
  }
  if let Some(verification) = report.verification.as_ref() {
    context.in_scope(|| {
      auv_tracing::emit_event!(TextEditDocumentWriteVerificationEvent {
        verification: verification.clone(),
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
  match execute_document_write(command.clone(), cancellation, driver).await {
    Ok(report) => Ok((build_invoke_output_from_report(&report, &command)?, report)),
    Err(failure) => Err(failure.into_message()),
  }
}

#[derive(serde::Serialize)]
struct TextEditDocumentWriteVerificationEvent {
  verification: VerificationOutcome,
}

impl EventPayload for TextEditDocumentWriteVerificationEvent {
  const NAME: &'static str = "auv.textedit.document_write.verification";
  const VERSION: u32 = 1;
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
  let mut output = InvokeCommandOutput::from_result(report)?;
  output.report = Some(document_write_report(report, command));
  Ok(output)
}

fn document_write_report(report: &DocumentCommandReport, command: &DocumentWrite) -> InvokeReport {
  let mut sections = Vec::new();
  sections.push(InvokeReportSection {
    title: "Actions".to_string(),
    fields: report
      .actions
      .iter()
      .map(|action| InvokeReportField {
        label: action_name(action.action).to_string(),
        value: action
          .input_action_result
          .as_ref()
          .map(|result| format!("{:?}", result.selected_path))
          .unwrap_or_else(|| "completed".to_string()),
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

fn action_name(action: TextEditAction) -> &'static str {
  match action {
    TextEditAction::Activate => "activate",
    TextEditAction::FocusTextInput => "focus_text_input",
    TextEditAction::PasteText => "paste_text",
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
