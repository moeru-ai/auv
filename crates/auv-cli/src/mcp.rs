//! Product MCP bootstrap for typed invoke adapters.
//!
//! Product-owned adapters call app domain APIs and map their values to MCP
//! presentation without executing the CLI registry.

use std::path::PathBuf;
use std::sync::Arc;

use auv_apple_textedit::DocumentWrite;
use auv_runtime::mcp::{McpInvokeAdapter, McpInvokeInput, McpInvokeSuccess};

/// Serve product MCP (CLI `auv mcp serve`) with product invoke metadata/adapters.
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
