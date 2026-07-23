//! Product inspection over canonical run snapshots.

pub(crate) mod query_wired_minecraft;
pub(crate) mod query_wired_osu;
pub(crate) mod sections;

#[cfg(test)]
mod goldens;

use auv_tracing::{RunSnapshot, RunStore};

pub use sections::{ProductInspectDocument, ProductInspectError, ProductInspectSection, build_product_inspect_document};

/// Renders the product view from one authority snapshot.
pub async fn inspect_run(store: &dyn RunStore, snapshot: &RunSnapshot) -> Result<String, ProductInspectError> {
  Ok(build_product_inspect_document(store, snapshot).await?.render_text())
}
