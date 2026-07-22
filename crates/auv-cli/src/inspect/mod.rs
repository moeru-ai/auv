//! Product inspect: composer assembly and golden fixtures.

pub(crate) mod query_wired_minecraft;
pub(crate) mod query_wired_osu;
pub(crate) mod sections;

#[cfg(test)]
mod goldens;

use auv_inspect_model::InspectComposer;
use auv_runtime::model::AuvResult;
use auv_tracing_driver::store::LocalStore;

pub use sections::{
  ProductInspectDocument, ProductInspectError, ProductInspectSection, ProductInspectTextDocument, build_product_inspect_composer,
  build_product_inspect_document, build_product_inspect_text_document,
};

/// Inspect text using an explicit composer shared by CLI and MCP frontends.
pub fn inspect_run_with(composer: &InspectComposer, store: &LocalStore, run_id: &str) -> AuvResult<String> {
  composer.inspect_text(store, run_id).map_err(|error| error.to_string())
}

/// Product inspect text via the shared composer path.
pub fn inspect_run(store: &LocalStore, run_id: &str) -> AuvResult<String> {
  let composer = build_product_inspect_composer().map_err(|error| error.to_string())?;
  inspect_run_with(&composer, store, run_id)
}
