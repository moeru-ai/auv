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
pub mod driver;
pub mod inference_recognition;
pub mod inspect;
pub mod inspect_server;
pub mod mcp;
pub mod model;
pub mod osu;
pub mod recorded_operation;
pub mod recording;
pub mod run_builder;
mod run_read;
pub mod runtime;
pub mod scroll_scan;
pub mod stability;
pub mod store;
pub mod trace;

use std::path::PathBuf;

use driver::default_driver_registry;
use model::AuvResult;
use runtime::Runtime;
use store::LocalStore;

pub fn build_default_runtime(project_root: PathBuf) -> AuvResult<Runtime> {
  let store_root = default_project_store_root(project_root.clone());
  build_runtime_with_store_root(project_root, store_root)
}

pub fn build_runtime_with_store_root(
  project_root: PathBuf,
  store_root: PathBuf,
) -> AuvResult<Runtime> {
  let store = LocalStore::new(store_root)?;
  let drivers = default_driver_registry();
  Ok(Runtime::new(project_root, drivers, store))
}

pub fn default_project_store_root(project_root: PathBuf) -> PathBuf {
  project_root.join(".auv")
}

pub fn build_default_store(project_root: PathBuf) -> AuvResult<LocalStore> {
  LocalStore::new(default_project_store_root(project_root))
}
