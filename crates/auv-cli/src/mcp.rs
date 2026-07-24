//! Product MCP bootstrap: inject product inspect metadata and typed adapters.
//!
//! Product-owned adapters call app domain APIs and map their values to MCP
//! presentation without executing the CLI registry.

use std::path::PathBuf;
use std::sync::Arc;

use auv_apple_textedit::DocumentWrite;
use auv_runtime::mcp::{McpInvokeAdapter, McpInvokeInput, McpInvokeSuccess};

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
  adapters.push(balatro_blind_select_adapter());
  adapters.push(balatro_blind_skip_adapter());
  adapters.push(balatro_cards_clear_adapter());
  adapters.push(balatro_cards_discard_adapter());
  adapters.push(balatro_cards_play_adapter());
  adapters.push(balatro_cards_select_adapter());
  adapters.push(balatro_cash_out_adapter());
  adapters.push(balatro_consumable_sell_adapter());
  adapters.push(balatro_consumable_use_adapter());
  adapters.push(balatro_game_restart_adapter());
  adapters.push(balatro_joker_sell_adapter());
  adapters.push(balatro_pack_choose_adapter());
  adapters.push(balatro_pack_skip_adapter());
  adapters.push(balatro_store_buy_adapter());
  adapters.push(balatro_store_next_round_adapter());
  adapters.push(textedit_document_write_adapter());
  adapters
}

fn balatro_blind_select_adapter() -> McpInvokeAdapter {
  McpInvokeAdapter::new(
    crate::integrations::balatro::BLIND_SELECT_COMMAND_ID,
    |input| async move { invoke_balatro_blind_select(input).await },
  )
}

async fn invoke_balatro_blind_select(input: McpInvokeInput) -> Result<McpInvokeSuccess, String> {
  let request = crate::integrations::balatro::parse_blind_select_request(input.target_application_id.as_deref(), &input.inputs)?;
  if input.dry_run {
    return Ok(McpInvokeSuccess::empty());
  }
  let result = crate::integrations::balatro::execute_blind_select(request, input.cancellation).await?;
  McpInvokeSuccess::from_result(&result)
}

fn balatro_blind_skip_adapter() -> McpInvokeAdapter {
  McpInvokeAdapter::new(crate::integrations::balatro::BLIND_SKIP_COMMAND_ID, |input| async move { invoke_balatro_blind_skip(input).await })
}

async fn invoke_balatro_blind_skip(input: McpInvokeInput) -> Result<McpInvokeSuccess, String> {
  let request = crate::integrations::balatro::parse_blind_skip_request(input.target_application_id.as_deref(), &input.inputs)?;
  if input.dry_run {
    return Ok(McpInvokeSuccess::empty());
  }
  let result = crate::integrations::balatro::execute_blind_skip(request, input.cancellation).await?;
  McpInvokeSuccess::from_result(&result)
}

fn balatro_cards_clear_adapter() -> McpInvokeAdapter {
  McpInvokeAdapter::new(crate::integrations::balatro::CARDS_CLEAR_COMMAND_ID, |input| async move { invoke_balatro_cards_clear(input).await })
}

async fn invoke_balatro_cards_clear(input: McpInvokeInput) -> Result<McpInvokeSuccess, String> {
  let request = crate::integrations::balatro::parse_cards_clear_request(input.target_application_id.as_deref(), &input.inputs)?;
  if input.dry_run {
    return Ok(McpInvokeSuccess::empty());
  }
  let result = crate::integrations::balatro::execute_cards_clear(request, input.cancellation).await?;
  McpInvokeSuccess::from_result(&result)
}

fn balatro_cards_select_adapter() -> McpInvokeAdapter {
  McpInvokeAdapter::new(
    crate::integrations::balatro::CARDS_SELECT_COMMAND_ID,
    |input| async move { invoke_balatro_cards_select(input).await },
  )
}

async fn invoke_balatro_cards_select(input: McpInvokeInput) -> Result<McpInvokeSuccess, String> {
  let request = crate::integrations::balatro::parse_cards_select_request(input.target_application_id.as_deref(), &input.inputs)?;
  if input.dry_run {
    return Ok(McpInvokeSuccess::empty());
  }
  let result = crate::integrations::balatro::execute_cards_select(request, input.cancellation).await?;
  McpInvokeSuccess::from_result(&result)
}

fn balatro_cards_play_adapter() -> McpInvokeAdapter {
  McpInvokeAdapter::new(crate::integrations::balatro::CARDS_PLAY_COMMAND_ID, |input| async move { invoke_balatro_cards_play(input).await })
}

async fn invoke_balatro_cards_play(input: McpInvokeInput) -> Result<McpInvokeSuccess, String> {
  let request = crate::integrations::balatro::parse_card_commit_request(
    crate::integrations::balatro::CARDS_PLAY_COMMAND_ID,
    input.target_application_id.as_deref(),
    &input.inputs,
  )?;
  if input.dry_run {
    return Ok(McpInvokeSuccess::empty());
  }
  let result = crate::integrations::balatro::execute_cards_play(request, input.cancellation).await?;
  McpInvokeSuccess::from_result(&result)
}

fn balatro_cards_discard_adapter() -> McpInvokeAdapter {
  McpInvokeAdapter::new(
    crate::integrations::balatro::CARDS_DISCARD_COMMAND_ID,
    |input| async move { invoke_balatro_cards_discard(input).await },
  )
}

async fn invoke_balatro_cards_discard(input: McpInvokeInput) -> Result<McpInvokeSuccess, String> {
  let request = crate::integrations::balatro::parse_card_commit_request(
    crate::integrations::balatro::CARDS_DISCARD_COMMAND_ID,
    input.target_application_id.as_deref(),
    &input.inputs,
  )?;
  if input.dry_run {
    return Ok(McpInvokeSuccess::empty());
  }
  let result = crate::integrations::balatro::execute_cards_discard(request, input.cancellation).await?;
  McpInvokeSuccess::from_result(&result)
}

fn balatro_cash_out_adapter() -> McpInvokeAdapter {
  McpInvokeAdapter::new(crate::integrations::balatro::CASH_OUT_COMMAND_ID, |input| async move { invoke_balatro_cash_out(input).await })
}

async fn invoke_balatro_cash_out(input: McpInvokeInput) -> Result<McpInvokeSuccess, String> {
  let request = crate::integrations::balatro::parse_cash_out_request(input.target_application_id.as_deref(), &input.inputs)?;
  if input.dry_run {
    return Ok(McpInvokeSuccess::empty());
  }
  let result = crate::integrations::balatro::execute_cash_out(request, input.cancellation).await?;
  McpInvokeSuccess::from_result(&result)
}

fn balatro_consumable_sell_adapter() -> McpInvokeAdapter {
  McpInvokeAdapter::new(crate::integrations::balatro::CONSUMABLE_SELL_COMMAND_ID, |input| async move {
    invoke_balatro_consumable_sell(input).await
  })
}

async fn invoke_balatro_consumable_sell(input: McpInvokeInput) -> Result<McpInvokeSuccess, String> {
  let request = crate::integrations::balatro::parse_consumable_sell_request(input.target_application_id.as_deref(), &input.inputs)?;
  if input.dry_run {
    return Ok(McpInvokeSuccess::empty());
  }
  let result = crate::integrations::balatro::execute_object_sell(request, input.cancellation).await?;
  McpInvokeSuccess::from_result(&result)
}

fn balatro_consumable_use_adapter() -> McpInvokeAdapter {
  McpInvokeAdapter::new(crate::integrations::balatro::CONSUMABLE_USE_COMMAND_ID, |input| async move {
    invoke_balatro_consumable_use(input).await
  })
}

async fn invoke_balatro_consumable_use(input: McpInvokeInput) -> Result<McpInvokeSuccess, String> {
  let request = crate::integrations::balatro::parse_consumable_use_request(input.target_application_id.as_deref(), &input.inputs)?;
  if input.dry_run {
    return Ok(McpInvokeSuccess::empty());
  }
  let result = crate::integrations::balatro::execute_consumable_use(request, input.cancellation).await?;
  McpInvokeSuccess::from_result(&result)
}

fn balatro_game_restart_adapter() -> McpInvokeAdapter {
  McpInvokeAdapter::new(
    crate::integrations::balatro::GAME_RESTART_COMMAND_ID,
    |input| async move { invoke_balatro_game_restart(input).await },
  )
}

async fn invoke_balatro_game_restart(input: McpInvokeInput) -> Result<McpInvokeSuccess, String> {
  let request = crate::integrations::balatro::parse_game_restart_request(input.target_application_id.as_deref(), &input.inputs)?;
  if input.dry_run {
    return Ok(McpInvokeSuccess::empty());
  }
  let result = crate::integrations::balatro::execute_game_restart(request, input.cancellation).await?;
  McpInvokeSuccess::from_result(&result)
}

fn balatro_joker_sell_adapter() -> McpInvokeAdapter {
  McpInvokeAdapter::new(crate::integrations::balatro::JOKER_SELL_COMMAND_ID, |input| async move { invoke_balatro_joker_sell(input).await })
}

async fn invoke_balatro_joker_sell(input: McpInvokeInput) -> Result<McpInvokeSuccess, String> {
  let request = crate::integrations::balatro::parse_joker_sell_request(input.target_application_id.as_deref(), &input.inputs)?;
  if input.dry_run {
    return Ok(McpInvokeSuccess::empty());
  }
  let result = crate::integrations::balatro::execute_object_sell(request, input.cancellation).await?;
  McpInvokeSuccess::from_result(&result)
}

fn balatro_pack_choose_adapter() -> McpInvokeAdapter {
  McpInvokeAdapter::new(crate::integrations::balatro::PACK_CHOOSE_COMMAND_ID, |input| async move { invoke_balatro_pack_choose(input).await })
}

async fn invoke_balatro_pack_choose(input: McpInvokeInput) -> Result<McpInvokeSuccess, String> {
  let request = crate::integrations::balatro::parse_pack_choose_request(input.target_application_id.as_deref(), &input.inputs)?;
  if input.dry_run {
    return Ok(McpInvokeSuccess::empty());
  }
  let result = crate::integrations::balatro::execute_pack_choose(request, input.cancellation).await?;
  McpInvokeSuccess::from_result(&result)
}

fn balatro_pack_skip_adapter() -> McpInvokeAdapter {
  McpInvokeAdapter::new(crate::integrations::balatro::PACK_SKIP_COMMAND_ID, |input| async move { invoke_balatro_pack_skip(input).await })
}

async fn invoke_balatro_pack_skip(input: McpInvokeInput) -> Result<McpInvokeSuccess, String> {
  let request = crate::integrations::balatro::parse_pack_skip_request(input.target_application_id.as_deref(), &input.inputs)?;
  if input.dry_run {
    return Ok(McpInvokeSuccess::empty());
  }
  let result = crate::integrations::balatro::execute_pack_skip(request, input.cancellation).await?;
  McpInvokeSuccess::from_result(&result)
}

fn balatro_store_buy_adapter() -> McpInvokeAdapter {
  McpInvokeAdapter::new(crate::integrations::balatro::STORE_BUY_COMMAND_ID, |input| async move { invoke_balatro_store_buy(input).await })
}

async fn invoke_balatro_store_buy(input: McpInvokeInput) -> Result<McpInvokeSuccess, String> {
  let request = crate::integrations::balatro::parse_store_buy_request(input.target_application_id.as_deref(), &input.inputs)?;
  if input.dry_run {
    return Ok(McpInvokeSuccess::empty());
  }
  let result = crate::integrations::balatro::execute_store_buy(request, input.cancellation).await?;
  McpInvokeSuccess::from_result(&result)
}

fn balatro_store_next_round_adapter() -> McpInvokeAdapter {
  McpInvokeAdapter::new(crate::integrations::balatro::STORE_NEXT_ROUND_COMMAND_ID, |input| async move {
    invoke_balatro_store_next_round(input).await
  })
}

async fn invoke_balatro_store_next_round(input: McpInvokeInput) -> Result<McpInvokeSuccess, String> {
  let request = crate::integrations::balatro::parse_store_next_round_request(input.target_application_id.as_deref(), &input.inputs)?;
  if input.dry_run {
    return Ok(McpInvokeSuccess::empty());
  }
  let result = crate::integrations::balatro::execute_store_next_round(request, input.cancellation).await?;
  McpInvokeSuccess::from_result(&result)
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

async fn invoke_textedit_document_write(input: McpInvokeInput) -> Result<McpInvokeSuccess, String> {
  invoke_textedit_document_write_with(input, |_| auv_apple_textedit::MacosTextEditDriver::open_local().map_err(|error| error.to_string()))
    .await
}

async fn invoke_textedit_document_write_with<D>(
  input: McpInvokeInput,
  open_driver: impl FnOnce(&DocumentWrite) -> Result<D, String>,
) -> Result<McpInvokeSuccess, String>
where
  D: auv_apple_textedit::TextEditDriver,
{
  reject_production_fixture_inputs(&input)?;
  let command = parse_document_write(&input)?;
  if input.dry_run {
    return Ok(McpInvokeSuccess::empty());
  }
  let driver = open_driver(&command)?;
  map_textedit_document_write(command, input.cancellation, driver).await.map(|(outcome, _)| outcome)
}

async fn map_textedit_document_write<D>(
  command: DocumentWrite,
  cancellation: auv_cli_invoke::InvokeCancellation,
  driver: D,
) -> Result<(McpInvokeSuccess, auv_apple_textedit::DocumentCommandReport), String>
where
  D: auv_apple_textedit::TextEditDriver,
{
  let report = crate::integrations::textedit::execute_document_write(command.clone(), cancellation, driver)
    .await
    .map_err(crate::integrations::textedit::DocumentWriteFailure::into_message)?;
  let outcome = document_write_outcome(&report)?;
  Ok((outcome, report))
}

fn document_write_outcome(report: &auv_apple_textedit::DocumentCommandReport) -> Result<McpInvokeSuccess, String> {
  McpInvokeSuccess::from_result(report)
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

#[cfg(test)]
mod tests {
  use std::path::{Path, PathBuf};

  use auv_apple_textedit::VerificationOutcome;
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
    verification: VerificationOutcome,
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
    adapters.push(balatro_blind_select_adapter());
    adapters.push(balatro_blind_skip_adapter());
    adapters.push(balatro_cards_clear_adapter());
    adapters.push(balatro_cards_discard_adapter());
    adapters.push(balatro_cards_play_adapter());
    adapters.push(balatro_cards_select_adapter());
    adapters.push(balatro_cash_out_adapter());
    adapters.push(balatro_consumable_sell_adapter());
    adapters.push(balatro_consumable_use_adapter());
    adapters.push(balatro_game_restart_adapter());
    adapters.push(balatro_joker_sell_adapter());
    adapters.push(balatro_pack_choose_adapter());
    adapters.push(balatro_pack_skip_adapter());
    adapters.push(balatro_store_buy_adapter());
    adapters.push(balatro_store_next_round_adapter());
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

    assert_eq!(cli_exit, 0);
    assert_eq!(response.is_error, Some(false));
    assert_eq!(presentation["status"], "completed");
    assert_eq!(presentation["result"]["verification"]["semantic_matched"], false);
    assert_eq!(presentation["result"]["actions"].as_array().map(Vec::len), Some(3));
    assert!(presentation["recording_failure"].is_null());
    assert_ne!(cli_run_id, mcp_run_id);

    let cli_actions = list_input_action_results(&cli_store, &cli_snapshot).await.expect("read CLI typed input actions");
    let mcp_actions = list_input_action_results(&mcp_store, &mcp_snapshot).await.expect("read MCP typed input actions");
    assert_eq!(cli_actions, mcp_actions);
    assert_eq!(cli_actions.len(), 2);

    let cli_verification = recorded_verification(&cli_snapshot);
    let mcp_verification = recorded_verification(&mcp_snapshot);
    assert_eq!(cli_verification, mcp_verification);
    assert!(!cli_verification.semantic_matched);
    assert_eq!(cli_snapshot.artifacts().len(), 2);
    assert_eq!(mcp_snapshot.artifacts().len(), 2);
    assert!(presentation.get("artifacts").is_none());
    assert_eq!(frontend_lifecycle(&cli_snapshot), "cli");
    assert_eq!(frontend_lifecycle(&mcp_snapshot), "mcp");

    client.cancel().await.expect("stop MCP client");
    server_handle.await.expect("join MCP server");
  }

  #[test]
  fn product_help_lists_app_commands_once() {
    let help = auv_cli_invoke::render_help_index(&crate::product_registry());
    assert_eq!(help.matches(crate::integrations::balatro::BLIND_SELECT_COMMAND_ID).count(), 1);
    assert_eq!(help.matches(crate::integrations::balatro::BLIND_SKIP_COMMAND_ID).count(), 1);
    assert_eq!(help.matches(crate::integrations::balatro::CARDS_CLEAR_COMMAND_ID).count(), 1);
    assert_eq!(help.matches(crate::integrations::balatro::CARDS_DISCARD_COMMAND_ID).count(), 1);
    assert_eq!(help.matches(crate::integrations::balatro::CARDS_PLAY_COMMAND_ID).count(), 1);
    assert_eq!(help.matches(crate::integrations::balatro::CARDS_SELECT_COMMAND_ID).count(), 1);
    assert_eq!(help.matches(crate::integrations::balatro::CASH_OUT_COMMAND_ID).count(), 1);
    assert_eq!(help.matches(crate::integrations::balatro::CONSUMABLE_SELL_COMMAND_ID).count(), 1);
    assert_eq!(help.matches(crate::integrations::balatro::CONSUMABLE_USE_COMMAND_ID).count(), 1);
    assert_eq!(help.matches(crate::integrations::balatro::GAME_RESTART_COMMAND_ID).count(), 1);
    assert_eq!(help.matches(crate::integrations::balatro::JOKER_SELL_COMMAND_ID).count(), 1);
    assert_eq!(help.matches(crate::integrations::balatro::PACK_CHOOSE_COMMAND_ID).count(), 1);
    assert_eq!(help.matches(crate::integrations::balatro::PACK_SKIP_COMMAND_ID).count(), 1);
    assert_eq!(help.matches(crate::integrations::balatro::STORE_BUY_COMMAND_ID).count(), 1);
    assert_eq!(help.matches(crate::integrations::balatro::STORE_NEXT_ROUND_COMMAND_ID).count(), 1);
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

  fn recorded_verification(snapshot: &auv_tracing::RunSnapshot) -> VerificationOutcome {
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
