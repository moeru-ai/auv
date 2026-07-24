//! TextEdit product invoke backed by the app-owned typed command report.

#[cfg(test)]
use std::cell::RefCell;
#[cfg(test)]
use std::future::Future;
#[cfg(test)]
use std::time::Duration;

use auv_apple_textedit::{
  DocumentCommand, DocumentCommandReport, DocumentWrite, TextEditAction, TextEditDriver, VerificationOutcome,
  run_document_command_with_checkpoint,
};
#[cfg(test)]
use auv_apple_textedit::{MatchedAxNode, TextEditActionResult};
use auv_cli_invoke::arg::TEXTEDIT_DOCUMENT_WRITE_ARGS;
use auv_cli_invoke::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, InvokeReport, InvokeReportField, InvokeReportSection,
  invoke_command,
};
use auv_driver::DriverError;
#[cfg(test)]
use auv_driver::{InputActionResult, InputDeliveryPath};
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
  #[cfg(test)]
  if let Some(driver) = take_fixture_driver() {
    return map_document_write_cli(command, input.cancellation, driver).await.map(|(output, _)| output);
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
  fn activate_app(&mut self, _app_id: &str, _settle: Duration) -> Result<TextEditActionResult, DriverError> {
    Ok(TextEditActionResult {
      action: TextEditAction::Activate,
      input_action_result: None,
    })
  }

  fn focus_text_input(&mut self, _app_id: &str, _query: &str, _candidate: &str) -> Result<TextEditActionResult, DriverError> {
    Ok(TextEditActionResult {
      action: TextEditAction::FocusTextInput,
      input_action_result: Some(InputActionResult::single_success(InputDeliveryPath::AxFocus)),
    })
  }

  fn paste_text_preserve_clipboard(
    &mut self,
    _app_id: &str,
    text: &str,
    _replace_existing: bool,
    _settle: Duration,
  ) -> Result<TextEditActionResult, DriverError> {
    self.content = text.to_string();
    Ok(TextEditActionResult {
      action: TextEditAction::PasteText,
      input_action_result: Some(InputActionResult::single_success(InputDeliveryPath::ClipboardPaste)),
    })
  }

  fn verify_ax_text(&mut self, _app_id: &str, target_text: &str, target_role: &str) -> Result<VerificationOutcome, DriverError> {
    self.role = target_role.to_string();
    let observed = self.observed_override.clone().unwrap_or_else(|| self.content.clone());
    Ok(VerificationOutcome {
      matched_role: target_role.to_string(),
      matched_text: observed.clone(),
      artifact_count: 1,
      semantic_matched: observed.contains(target_text),
      matched_node: Some(MatchedAxNode {
        path: "fixture.0.1.2".to_string(),
        process_id: 0,
      }),
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
  use std::sync::atomic::{AtomicUsize, Ordering};

  use auv_tracing::{
    ArtifactBody, ArtifactReader, ArtifactUri, ArtifactWriteError, AuthorityId, BoxFuture, CommitError, CommitResult, DispatchTask,
    ErrorCode, IdempotencyKey, MemoryRunStore, PageLimit, ReadError, RunCommit, RunCommitPage, RunCommitRequest, RunId, RunRevision,
    RunStore, RunSubscription, StoreArtifactRequest, TaskSpawnError, TaskSpawner, TelemetryError, TelemetryItem, TelemetryProjector,
    TelemetryRoutePolicy, configure, dispatcher,
  };

  use super::*;

  struct InvalidInputActionDriver(FixtureTextEditDriver);

  impl InvalidInputActionDriver {
    fn new(command: &DocumentWrite) -> Self {
      Self(FixtureTextEditDriver::from_write(command))
    }
  }

  impl TextEditDriver for InvalidInputActionDriver {
    fn activate_app(&mut self, app_id: &str, settle: Duration) -> Result<TextEditActionResult, DriverError> {
      self.0.activate_app(app_id, settle)
    }

    fn focus_text_input(&mut self, app_id: &str, query: &str, candidate: &str) -> Result<TextEditActionResult, DriverError> {
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
    ) -> Result<TextEditActionResult, DriverError> {
      self.0.paste_text_preserve_clipboard(app_id, text, replace_existing, settle)
    }

    fn verify_ax_text(&mut self, app_id: &str, target_text: &str, target_role: &str) -> Result<VerificationOutcome, DriverError> {
      self.0.verify_ax_text(app_id, target_text, target_role)
    }
  }

  struct RejectingSpawner;

  impl TaskSpawner for RejectingSpawner {
    fn spawn(&self, _task: DispatchTask) -> Result<(), TaskSpawnError> {
      Err(TaskSpawnError::new(ErrorCode::parse("auv.test.textedit_spawn_rejected").expect("test error code")))
    }
  }

  struct FailNthArtifactStore {
    inner: MemoryRunStore,
    fail_at: usize,
    writes: AtomicUsize,
  }

  impl FailNthArtifactStore {
    fn new(fail_at: usize) -> Self {
      Self {
        inner: MemoryRunStore::new(AuthorityId::new()),
        fail_at,
        writes: AtomicUsize::new(0),
      }
    }
  }

  impl RunStore for FailNthArtifactStore {
    fn authority_id(&self) -> AuthorityId {
      self.inner.authority_id()
    }

    fn commit(&self, request: RunCommitRequest) -> BoxFuture<'_, Result<CommitResult, CommitError>> {
      self.inner.commit(request)
    }

    fn write_artifact(&self, request: StoreArtifactRequest, body: ArtifactBody) -> BoxFuture<'_, Result<CommitResult, ArtifactWriteError>> {
      let write = self.writes.fetch_add(1, Ordering::SeqCst) + 1;
      if write == self.fail_at {
        return Box::pin(async {
          Err(ArtifactWriteError::Rejected(ErrorCode::parse("auv.test.textedit_publication_rejected").expect("test error code")))
        });
      }
      self.inner.write_artifact(request, body)
    }

    fn lookup_commit(&self, run_id: RunId, key: IdempotencyKey) -> BoxFuture<'_, Result<Option<RunCommit>, ReadError>> {
      self.inner.lookup_commit(run_id, key)
    }

    fn load_snapshot(&self, run_id: RunId) -> BoxFuture<'_, Result<Option<auv_tracing::RunSnapshot>, ReadError>> {
      self.inner.load_snapshot(run_id)
    }

    fn commits_after(&self, run_id: RunId, after: RunRevision, limit: PageLimit) -> BoxFuture<'_, Result<RunCommitPage, ReadError>> {
      self.inner.commits_after(run_id, after, limit)
    }

    fn subscribe(&self, run_id: RunId, after: RunRevision) -> BoxFuture<'_, Result<RunSubscription, ReadError>> {
      self.inner.subscribe(run_id, after)
    }

    fn open_artifact(&self, uri: ArtifactUri) -> BoxFuture<'_, Result<ArtifactReader, ReadError>> {
      self.inner.open_artifact(uri)
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
  async fn invoke_mapping_keeps_semantic_mismatch_in_the_completed_typed_result() {
    let command = DocumentWrite::defaults_with_content("expected");
    let driver = fixture_driver(&command, Some("different".to_string()));
    let report = write_document(command.clone(), auv_cli_invoke::InvokeCancellation::new(), driver).await.expect("fixture report");
    let expected = serde_json::to_value(&report).expect("serialize typed TextEdit report");

    let output = build_invoke_output_from_report(&report, &command).expect("semantic mismatch is not an execution failure");
    let result = auv_cli_invoke::InvokeResult::from_command_result(RunId::new(), &document_write_invoke_command(), Ok(output));

    assert_eq!(result.status(), auv_cli_invoke::InvokeStatus::Completed);
    assert_eq!(result.result(), Some(&expected));
  }

  #[tokio::test]
  async fn enabled_context_keeps_input_action_validation_failure_out_of_the_direct_result() {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store).build().expect("memory dispatch");
    let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
    let command = DocumentWrite::defaults_with_content("expected");
    let future =
      root.in_scope(|| write_document(command.clone(), auv_cli_invoke::InvokeCancellation::new(), InvalidInputActionDriver::new(&command)));

    let report = root.instrument(future).await.expect("invalid recording evidence must not replace the TextEdit result");
    dispatch.flush().await.expect("preparation diagnostic flush");

    assert_eq!(report.actions.len(), 3);
  }

  #[tokio::test]
  async fn enabled_context_keeps_input_action_enqueue_failure_out_of_the_direct_result() {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store).task_spawner(Arc::new(RejectingSpawner)).build().expect("rejecting dispatch");
    let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
    let command = DocumentWrite::defaults_with_content("expected");
    let driver = fixture_driver(&command, None);
    let future = root.in_scope(|| write_document(command, auv_cli_invoke::InvokeCancellation::new(), driver));

    let report = root.instrument(future).await.expect("enqueue failure must not replace the TextEdit result");
    dispatch.flush().await.expect_err("enqueue failure remains on the tracing dispatch");

    assert_eq!(report.actions.len(), 3);
  }

  #[tokio::test]
  async fn frontend_mapping_is_unchanged_when_a_later_artifact_write_fails() {
    let store = Arc::new(FailNthArtifactStore::new(2));
    let dispatch = configure().run_store(store).build().expect("memory dispatch");
    let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
    let command = DocumentWrite::defaults_with_content("expected");
    let driver = fixture_driver(&command, None);
    let future = root.in_scope(|| map_document_write_cli(command, auv_cli_invoke::InvokeCancellation::new(), driver));

    let (output, report) = root.instrument(future).await.expect("artifact failure must not replace the frontend value");
    dispatch.flush().await.expect_err("artifact write failure remains on the tracing dispatch");

    assert!(output.report.is_some());
    assert_eq!(report.actions.len(), 3);
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
