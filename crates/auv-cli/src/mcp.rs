//! Product MCP bootstrap: inject product inspect composer + product invoke registry.
//!
//! Product owns composer/registry assembly only; the MCP tool surface stays in
//! `auv-runtime` instead of forking `auv_runtime::mcp::McpServer` here.

use std::path::PathBuf;
use std::sync::Arc;

/// Serve product MCP (CLI `auv mcp serve`) with the shared product inspect composer
/// and product invoke registry (includes TextEdit invoke finalize inside the
/// recorded lifecycle).
pub async fn serve_stdio(project_root: PathBuf) -> Result<(), String> {
  let composer = crate::inspect::build_product_inspect_composer().map_err(|error| error.to_string())?;
  let registry = Arc::new(crate::product_registry());
  let finalize = Arc::new(crate::integrations::textedit::finalize_recorded_invoke);
  auv_runtime::mcp::serve_stdio_with_composer_and_registry(project_root, composer, registry, Some(finalize)).await
}
