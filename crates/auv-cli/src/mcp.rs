//! Product MCP bootstrap: inject product inspect metadata and typed adapters.
//!
//! Product-owned adapters call app domain APIs and map their values to MCP
//! presentation without executing the CLI registry.

use std::path::PathBuf;
use std::sync::Arc;

use auv_apple_textedit::DocumentWrite;
use auv_runtime::mcp::{McpInvokeAdapter, McpInvokeInput, McpInvokeOutcome};

/// Serve product MCP (CLI `auv mcp serve`) with the shared product inspect composer
/// and product invoke metadata/adapters.
pub async fn serve_stdio(project_root: PathBuf) -> Result<(), String> {
  let registry = Arc::new(crate::product_registry());
  auv_runtime::mcp::serve_stdio_with_registry(project_root, registry, product_invoke_adapters()).await
}

/// Builds the product MCP server for embedded transports and tests.
pub fn server(project_root: PathBuf) -> Result<auv_runtime::mcp::McpServer, String> {
  auv_runtime::mcp::McpServer::with_registry(project_root, Arc::new(crate::product_registry()), product_invoke_adapters())
}

pub(crate) fn product_invoke_adapters() -> Vec<McpInvokeAdapter> {
  let mut adapters = auv_runtime::mcp::core_invoke_adapters();
  adapters.push(textedit_document_write_adapter());
  adapters
}

fn textedit_document_write_adapter() -> McpInvokeAdapter {
  McpInvokeAdapter::new(crate::integrations::textedit::DOCUMENT_WRITE_COMMAND_ID, |input| async move {
    invoke_textedit_document_write(input).await
  })
}

#[cfg(test)]
fn textedit_document_write_adapter_with_fixture_driver(observed_text: Option<String>) -> McpInvokeAdapter {
  McpInvokeAdapter::new(crate::integrations::textedit::DOCUMENT_WRITE_COMMAND_ID, move |input| {
    let observed_text = observed_text.clone();
    async move {
      invoke_textedit_document_write_with(input, move |command| Ok(crate::integrations::textedit::fixture_driver(command, observed_text)))
        .await
    }
  })
}

async fn invoke_textedit_document_write(input: McpInvokeInput) -> Result<McpInvokeOutcome, String> {
  invoke_textedit_document_write_with(input, |_| auv_apple_textedit::MacosTextEditDriver::open_local()).await
}

async fn invoke_textedit_document_write_with<D>(
  input: McpInvokeInput,
  open_driver: impl FnOnce(&DocumentWrite) -> Result<D, String>,
) -> Result<McpInvokeOutcome, String>
where
  D: auv_apple_textedit::TextEditDriver,
{
  reject_production_fixture_inputs(&input)?;
  let command = parse_document_write(&input)?;
  if input.dry_run {
    return Ok(McpInvokeOutcome::completed("dry run: app.textedit.document.write", serde_json::Value::Null));
  }
  let driver = open_driver(&command)?;
  map_textedit_document_write(command, input.cancellation, driver).await.map(|(outcome, _)| outcome)
}

async fn map_textedit_document_write<D>(
  command: DocumentWrite,
  cancellation: auv_cli_invoke::InvokeCancellation,
  driver: D,
) -> Result<(McpInvokeOutcome, auv_apple_textedit::DocumentCommandReport), String>
where
  D: auv_apple_textedit::TextEditDriver,
{
  let report = crate::integrations::textedit::write_document(command.clone(), cancellation, driver).await?;
  let outcome = document_write_outcome(command, report.clone())?;
  Ok((outcome, report))
}

fn document_write_outcome(command: DocumentWrite, report: auv_apple_textedit::DocumentCommandReport) -> Result<McpInvokeOutcome, String> {
  let semantic_matched = report.verification.as_ref().map(|verification| verification.semantic_matched);
  let evidence = report
    .outcomes
    .iter()
    .filter_map(|outcome| outcome.input_action_result.as_ref().map(|_| "auv.driver.input_action_result"))
    .chain(report.verification.iter().map(|_| "auv.textedit.document_write.verification"))
    .collect::<Vec<_>>();
  let mut outcome = McpInvokeOutcome::completed(
    format!(
      "TextEdit document.write completed ({} steps, verify={}, semantic_matched={semantic_matched:?})",
      report.outcomes.len(),
      report.verification.is_some(),
    ),
    serde_json::json!({ "evidence_kinds": evidence }),
  );
  outcome.insert_signal("textedit.app_id", command.app_id);
  outcome.insert_signal("textedit.semantic_matched", serde_json::to_value(semantic_matched).map_err(|error| error.to_string())?);
  if let Some(verification) = report.verification.as_ref().filter(|verification| !verification.semantic_matched) {
    let observed = truncate(&verification.matched_text, 80);
    outcome.mark_failed(
      format!("TextEdit document.write failed semantic verification (role={}, observed={observed})", verification.matched_role),
      format!(
        "TextEdit semantic verification failed: expected content was not present in observed AX text role={} observed={observed}",
        verification.matched_role
      ),
    );
  }
  Ok(outcome)
}

fn reject_production_fixture_inputs(input: &McpInvokeInput) -> Result<(), String> {
  for name in ["driver", "fixture_observed_text"] {
    if input.inputs.contains_key(name) {
      return Err(format!("app.textedit.document.write does not accept --{name}"));
    }
  }
  Ok(())
}

fn parse_document_write(input: &McpInvokeInput) -> Result<DocumentWrite, String> {
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
mod tests {
  use std::path::{Path, PathBuf};

  use auv_runtime::contract::VerificationResult;
  use auv_runtime::run_read::list_input_action_results;
  use auv_tracing::{FileRunStore, RunId, RunStore};
  use rmcp::{
    ClientHandler, ServiceExt,
    model::{CallToolRequestParam, ClientInfo},
  };
  use serde_json::Value;

  use super::*;

  #[derive(Debug, Clone, Default)]
  struct DummyClientHandler;

  impl ClientHandler for DummyClientHandler {
    fn get_info(&self) -> ClientInfo {
      ClientInfo::default()
    }
  }

  #[derive(serde::Deserialize)]
  struct RecordedVerification {
    verification: VerificationResult,
  }

  struct TempStores {
    root: PathBuf,
  }

  impl TempStores {
    fn new() -> Self {
      let root = std::env::temp_dir().join(format!("auv-textedit-parity-{}", RunId::new()));
      std::fs::create_dir_all(&root).expect("create TextEdit parity root");
      Self { root }
    }

    fn path(&self, frontend: &str) -> PathBuf {
      self.root.join(frontend)
    }
  }

  impl Drop for TempStores {
    fn drop(&mut self) {
      let _ = std::fs::remove_dir_all(&self.root);
    }
  }

  #[tokio::test]
  async fn textedit_cli_and_mcp_use_real_frontend_lifecycles_with_typed_parity() {
    let stores = TempStores::new();
    let cli_store_root = stores.path("cli");
    let mcp_store_root = stores.path("mcp");
    let marker = "AUV_TEXTEDIT_FIXTURE_MARKER";
    let command = DocumentWrite::defaults_with_content(marker);

    let cli_command = crate::cli::parse_cli(&[
      "invoke".to_string(),
      crate::integrations::textedit::DOCUMENT_WRITE_COMMAND_ID.to_string(),
      "--content".to_string(),
      marker.to_string(),
      "--verify".to_string(),
      "true".to_string(),
      "--target".to_string(),
      command.app_id.clone(),
      "--json".to_string(),
      "--store-root".to_string(),
      cli_store_root.display().to_string(),
      "--inspect-server-write".to_string(),
      "false".to_string(),
    ])
    .expect("parse CLI TextEdit invoke");
    let cli_exit = crate::integrations::textedit::with_fixture_driver(
      &command,
      Some("different".to_string()),
      crate::cli_frontend::dispatch(cli_command),
    )
    .await
    .expect("dispatch CLI TextEdit invoke");
    let cli_run_id = only_recorded_run(&cli_store_root);
    let cli_store = FileRunStore::open(&cli_store_root).expect("open CLI store");
    let cli_snapshot =
      cli_store.load_snapshot(cli_run_id).await.expect("load CLI snapshot").expect("CLI run flushed before dispatch returned");

    let mut adapters = auv_runtime::mcp::core_invoke_adapters();
    adapters.push(textedit_document_write_adapter_with_fixture_driver(Some("different".to_string())));
    let server =
      auv_runtime::mcp::McpServer::with_registry(PathBuf::from(env!("CARGO_MANIFEST_DIR")), Arc::new(crate::product_registry()), adapters)
        .expect("build product MCP server");
    let (server_transport, client_transport) = tokio::io::duplex(16384);
    let server_handle = tokio::spawn(async move {
      let service = server.serve(server_transport).await.expect("serve product MCP");
      service.waiting().await.expect("wait for product MCP");
    });
    let client = DummyClientHandler.serve(client_transport).await.expect("serve MCP client");
    let response = client
      .call_tool(CallToolRequestParam {
        name: "invoke".into(),
        arguments: Some(
          serde_json::json!({
            "command_id": crate::integrations::textedit::DOCUMENT_WRITE_COMMAND_ID,
            "target": { "application_id": command.app_id },
            "inputs": { "content": marker, "verify": "true" },
            "inspect": { "store_root": mcp_store_root.display().to_string() }
          })
          .as_object()
          .expect("MCP arguments")
          .clone(),
        ),
      })
      .await
      .expect("invoke TextEdit through MCP");
    let presentation: Value =
      serde_json::from_str(&response.content.first().and_then(|content| content.raw.as_text()).expect("MCP response text").text)
        .expect("MCP presentation JSON");
    let mcp_run_id = presentation["run_id"].as_str().expect("MCP run id").parse::<RunId>().expect("valid MCP run id");
    let mcp_store = FileRunStore::open(&mcp_store_root).expect("open MCP store");
    let mcp_snapshot =
      mcp_store.load_snapshot(mcp_run_id).await.expect("load MCP snapshot").expect("MCP run flushed before response returned");

    assert_eq!(cli_exit, 1);
    assert_eq!(response.is_error, Some(true));
    assert_eq!(presentation["status"], "failed");
    assert!(presentation["recording_failure"].is_null());
    assert_ne!(cli_run_id, mcp_run_id);

    let cli_actions = list_input_action_results(&cli_store, &cli_snapshot).await.expect("read CLI typed input actions");
    let mcp_actions = list_input_action_results(&mcp_store, &mcp_snapshot).await.expect("read MCP typed input actions");
    assert_eq!(cli_actions, mcp_actions);
    assert_eq!(cli_actions.len(), 2);

    let cli_verification = recorded_verification(&cli_snapshot);
    let mcp_verification = recorded_verification(&mcp_snapshot);
    assert_eq!(cli_verification, mcp_verification);
    assert_eq!(cli_verification.semantic_matched, Some(false));
    assert_eq!(cli_snapshot.artifacts().len(), 2);
    assert_eq!(mcp_snapshot.artifacts().len(), 2);
    assert_eq!(presentation["artifacts"].as_array().map(Vec::len), Some(2));
    assert_eq!(frontend_lifecycle(&cli_snapshot), "cli");
    assert_eq!(frontend_lifecycle(&mcp_snapshot), "mcp");

    client.cancel().await.expect("stop MCP client");
    server_handle.await.expect("join MCP server");
  }

  #[test]
  fn product_help_lists_textedit_command_once() {
    let help = auv_cli_invoke::render_help_index(&crate::product_registry());
    assert_eq!(help.matches(crate::integrations::textedit::DOCUMENT_WRITE_COMMAND_ID).count(), 1);
    let command =
      crate::product_registry().resolve(crate::integrations::textedit::DOCUMENT_WRITE_COMMAND_ID).expect("TextEdit command").clone();
    assert!(!auv_cli_invoke::render_command_help(&command).contains("--driver"));
    assert!(
      !auv_cli_invoke::render_help_index(&auv_cli_invoke::default_registry())
        .contains(crate::integrations::textedit::DOCUMENT_WRITE_COMMAND_ID)
    );
  }

  fn only_recorded_run(store_root: &Path) -> RunId {
    let runs = std::fs::read_dir(store_root.join("runs"))
      .expect("run directory")
      .map(|entry| entry.expect("run entry").file_name().to_string_lossy().parse::<RunId>().expect("run id"))
      .collect::<Vec<_>>();
    assert_eq!(runs.len(), 1, "frontend must create exactly one run");
    runs[0]
  }

  fn recorded_verification(snapshot: &auv_tracing::RunSnapshot) -> VerificationResult {
    let event = snapshot
      .events()
      .iter()
      .find(|event| event.schema().name().as_str() == "auv.textedit.document_write.verification")
      .expect("app-owned TextEdit verification event");
    serde_json::from_str::<RecordedVerification>(event.payload().get()).expect("typed TextEdit verification").verification
  }

  fn frontend_lifecycle(snapshot: &auv_tracing::RunSnapshot) -> String {
    let event =
      snapshot.events().iter().find(|event| event.schema().name().as_str() == "auv.frontend.lifecycle").expect("frontend lifecycle event");
    serde_json::from_str::<Value>(event.payload().get()).expect("frontend lifecycle JSON")["frontend"]
      .as_str()
      .expect("frontend name")
      .to_string()
  }
}
