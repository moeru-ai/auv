//! Product-side inspect composer assembly (locked golden order).

use std::sync::Arc;

use auv_inspect_model::{InspectComposer, InspectError};
use auv_tracing_driver::store::LocalStore;

/// Build the product composer used by CLI, MCP, and inspect-server projection.
pub fn build_product_inspect_composer() -> Result<Arc<InspectComposer>, InspectError> {
  crate::inspect::sections::build_product_inspect_composer()
}

/// Inspect text using an explicit composer (shared CLI / MCP path).
pub fn inspect_run_with(composer: &InspectComposer, store: &LocalStore, run_id: &str) -> Result<String, String> {
  composer.inspect_text(store, run_id).map_err(|error| error.to_string())
}
