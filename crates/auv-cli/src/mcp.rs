//! Product MCP bootstrap: inject product inspect metadata and typed adapters.
//!
//! Product-owned adapters call app domain APIs and map their values to MCP
//! presentation without executing the CLI registry.

use std::path::PathBuf;
use std::sync::Arc;

use auv_apple_textedit::DocumentWrite;
use auv_runtime::mcp::{McpInvokeAdapter, McpInvokeInput};

/// Serve product MCP (CLI `auv mcp serve`) with the shared product inspect composer
/// and product invoke metadata/adapters.
pub async fn serve_stdio(project_root: PathBuf) -> Result<(), String> {
  let composer = crate::inspect::build_product_inspect_composer().map_err(|error| error.to_string())?;
  let registry = Arc::new(crate::product_registry());
  auv_runtime::mcp::serve_stdio_with_composer_and_registry(project_root, composer, registry, product_invoke_adapters()).await
}

/// Builds the product MCP server for embedded transports and tests.
pub fn server(project_root: PathBuf) -> Result<auv_runtime::mcp::McpServer, String> {
  let composer = crate::inspect::build_product_inspect_composer().map_err(|error| error.to_string())?;
  Ok(auv_runtime::mcp::McpServer::with_inspect_composer_and_registry(
    project_root,
    composer,
    Arc::new(crate::product_registry()),
    product_invoke_adapters(),
  ))
}

pub(crate) fn product_invoke_adapters() -> Vec<McpInvokeAdapter> {
  let mut adapters = auv_runtime::mcp::core_invoke_adapters();
  adapters.push(McpInvokeAdapter::new(crate::integrations::textedit::DOCUMENT_WRITE_COMMAND_ID, |input| async move {
    invoke_textedit_document_write(input).await
  }));
  adapters
}

async fn invoke_textedit_document_write(input: McpInvokeInput) -> Result<serde_json::Value, String> {
  use crate::integrations::textedit::{DocumentWriteDriver, write_document};

  if input.dry_run {
    return Ok(serde_json::json!({
      "status": "completed",
      "output_summary": "dry run: app.textedit.document.write",
      "signals": {},
      "artifacts": [],
      "failure_message": null,
    }));
  }
  let command = parse_document_write(&input)?;
  let driver = match input.inputs.get("driver").map(String::as_str).unwrap_or("live") {
    "fixture" => DocumentWriteDriver::Fixture {
      observed_text: input.inputs.get("fixture_observed_text").cloned(),
    },
    "live" => DocumentWriteDriver::Live,
    other => return Err(format!("app.textedit.document.write unknown --driver {other}; expected live or fixture")),
  };
  let report = write_document(command.clone(), driver).await?;
  let semantic_matched = report.verification.as_ref().map(|verification| verification.semantic_matched);
  let evidence = report
    .outcomes
    .iter()
    .filter_map(|outcome| outcome.input_action_result.as_ref().map(|_| "auv.driver.input_action_result"))
    .chain(report.verification.iter().map(|_| "auv.textedit.ax_text_observation"))
    .chain(std::iter::once("auv.textedit.document_write_result"))
    .collect::<Vec<_>>();
  let mut value = serde_json::json!({
    "status": "completed",
    "output_summary": format!(
      "TextEdit document.write completed ({} steps, verify={}, semantic_matched={semantic_matched:?})",
      report.outcomes.len(),
      report.verification.is_some(),
    ),
    "signals": {
      "textedit.app_id": command.app_id,
      "textedit.semantic_matched": semantic_matched,
    },
    "artifacts": evidence,
    "failure_message": null,
  });
  if let Some(verification) = report.verification.as_ref().filter(|verification| !verification.semantic_matched) {
    let observed = truncate(&verification.matched_text, 80);
    value["status"] = serde_json::Value::String("failed".to_string());
    value["output_summary"] = serde_json::Value::String(format!(
      "TextEdit document.write failed semantic verification (role={}, observed={observed})",
      verification.matched_role
    ));
    value["failure_message"] = serde_json::Value::String(format!(
      "TextEdit semantic verification failed: expected content was not present in observed AX text role={} observed={observed}",
      verification.matched_role
    ));
  }
  Ok(value)
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
    format!("{head}…")
  } else {
    head
  }
}
