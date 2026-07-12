//! Product inspect: composer assembly and golden fixtures.

pub(crate) mod query_wired_minecraft;
pub(crate) mod query_wired_osu;
pub(crate) mod sections;

#[cfg(test)]
mod goldens;

use auv_cli::model::AuvResult;
use auv_tracing_driver::store::LocalStore;

/// Product inspect text via the shared composer path.
pub fn inspect_run(store: &LocalStore, run_id: &str) -> AuvResult<String> {
  let composer = crate::product_inspect::build_product_inspect_composer().map_err(|error| error.to_string())?;
  crate::product_inspect::inspect_run_with(&composer, store, run_id)
}
