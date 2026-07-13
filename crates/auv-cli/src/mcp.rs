//! Product MCP bootstrap: inject product inspect composer into core MCP frontend.
//!
//! Product owns composer assembly only; the MCP tool surface stays in `auv-runtime`
//! instead of forking `auv_runtime::mcp::McpServer` here.

use std::path::PathBuf;

/// Serve product MCP (CLI `auv mcp serve`) with the shared product inspect composer.
pub async fn serve_stdio(project_root: PathBuf) -> Result<(), String> {
  let composer = crate::inspect::build_product_inspect_composer().map_err(|error| error.to_string())?;
  auv_runtime::mcp::serve_stdio_with_composer(project_root, composer).await
}
