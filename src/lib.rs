pub mod api;
pub mod app;
pub mod candidate_promotion;
pub mod contract;
pub mod inspect;
pub mod mcp;
pub mod model;
pub mod run_read;
pub mod runtime;
pub mod scene_state_read;
pub mod scroll_scan;
pub mod session;
pub mod stability;

use std::path::PathBuf;

use model::AuvResult;
pub fn default_project_store_root(project_root: PathBuf) -> PathBuf {
  project_root.join(".auv").join("store")
}

pub fn build_default_store(project_root: PathBuf) -> AuvResult<auv_tracing::FileRunStore> {
  auv_tracing::FileRunStore::open(default_project_store_root(project_root)).map_err(|error| error.to_string())
}
