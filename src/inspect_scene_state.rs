//! Thin inspect wrappers for S6b scene-state run-read text bridge.

use auv_tracing_driver::store::{CanonicalRun, LocalStore};

use crate::model::AuvResult;
use crate::scene_state_read;

pub fn append_scene_state_text_from_run(store: &LocalStore, run: &CanonicalRun, output: &mut String) -> AuvResult<()> {
  let outcome = scene_state_read::build_scene_state_inspect_for_run(store, run).map_err(|error| error.to_string())?;
  output.push_str(&scene_state_read::format_scene_state_read_text(&outcome));
  Ok(())
}
