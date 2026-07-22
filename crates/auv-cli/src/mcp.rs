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
  adapters.push(McpInvokeAdapter::new(crate::integrations::textedit::DOCUMENT_WRITE_COMMAND_ID, |input| async move {
    invoke_textedit_document_write(input).await
  }));
  adapters
}

async fn invoke_textedit_document_write(input: McpInvokeInput) -> Result<McpInvokeOutcome, String> {
  use crate::integrations::textedit::{DocumentWriteDriver, write_document};

  if input.dry_run {
    return Ok(McpInvokeOutcome::completed("dry run: app.textedit.document.write", serde_json::Value::Null));
  }
  let command = parse_document_write(&input)?;
  let driver = match input.inputs.get("driver").map(String::as_str).unwrap_or("live") {
    "fixture" => DocumentWriteDriver::Fixture {
      observed_text: input.inputs.get("fixture_observed_text").cloned(),
    },
    "live" => DocumentWriteDriver::Live,
    other => return Err(format!("app.textedit.document.write unknown --driver {other}; expected live or fixture")),
  };
  let (report, instrumentation) = write_document(command.clone(), driver).await?.into_parts();
  let semantic_matched = report.verification.as_ref().map(|verification| verification.semantic_matched);
  let evidence = report
    .outcomes
    .iter()
    .filter_map(|outcome| outcome.input_action_result.as_ref().map(|_| "auv.driver.input_action_result"))
    .chain(report.verification.iter().map(|_| "auv.textedit.ax_text_observation"))
    .chain(std::iter::once("auv.textedit.document_write_result"))
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
  Ok(outcome.with_artifact_instrumentation(instrumentation))
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
