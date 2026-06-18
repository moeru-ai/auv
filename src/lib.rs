// File: src/lib.rs
mod action_resolver_decision;
pub mod app;
#[cfg(target_os = "macos")]
pub mod ax_recognition;
pub mod candidate_action_command;
pub mod candidate_action_decision;
pub mod candidate_promotion;
pub mod candidate_promotion_recording;
pub mod contract;
pub mod inference_recognition;
pub mod inspect;
pub mod inspect_server;
pub mod mcp;
pub mod minecraft;
pub mod minecraft_session;
pub mod model;
pub mod osu;
mod run_read;
pub mod runtime;
pub mod scroll_scan;
pub mod session;
pub mod stability;

use std::path::PathBuf;

use auv_tracing_driver::store::LocalStore;
use model::AuvResult;
use runtime::Runtime;

pub fn build_default_runtime(project_root: PathBuf) -> AuvResult<Runtime> {
  let store_root = default_project_store_root(project_root.clone());
  build_runtime_with_store_root(project_root, store_root)
}

pub fn build_runtime_with_store_root(
  project_root: PathBuf,
  store_root: PathBuf,
) -> AuvResult<Runtime> {
  let store = LocalStore::new(store_root)?;
  Ok(Runtime::new(project_root, store))
}

pub fn default_project_store_root(project_root: PathBuf) -> PathBuf {
  project_root.join(".auv")
}

pub fn build_default_store(project_root: PathBuf) -> AuvResult<LocalStore> {
  LocalStore::new(default_project_store_root(project_root))
}
