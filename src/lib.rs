pub mod app;
pub mod bundle;
pub mod catalog;
pub mod driver;
pub mod model;
pub mod recording;
pub mod runtime;
pub mod skill;
pub mod store;
pub mod trace;

use std::path::PathBuf;

use catalog::default_command_catalog;
use driver::default_driver_registry;
use model::AuvResult;
use runtime::Runtime;
use store::LocalStore;

pub fn build_default_runtime(project_root: PathBuf) -> AuvResult<Runtime> {
  let store = LocalStore::new(project_root.join(".auv"))?;
  let commands = default_command_catalog();
  let drivers = default_driver_registry();
  Ok(Runtime::new(project_root, commands, drivers, store))
}
