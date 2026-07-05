//! Hermetic run storage for qqmusic search select proof (ACP-B).

use std::path::Path;

use auv_tracing_driver::recorded_operation::RecordedOperationContext;
use auv_tracing_driver::{LocalStore, RunRecordingBackend, RunSpec, RunType};

pub const QQ_MUSIC_SEARCH_SELECT_RESULT_ROLE: &str = "qqmusic-search-select-result";

pub fn persist_search_select_proof(
  store_root: &Path,
  build_result_json: impl FnOnce(&str) -> Result<Vec<u8>, String>,
) -> Result<String, String> {
  let store = LocalStore::new(store_root.to_path_buf()).map_err(|error| error.to_string())?;
  let recording = RunRecordingBackend::local_only(store).handle();

  let output = recording
    .run_recorded_operation(
      RunSpec::new(RunType::Command, "auv.qqmusic.search.select"),
      "qqmusic search select store proof",
      |ctx| persist_select_proof_in_recorded_context(ctx, build_result_json),
    )
    .map_err(|error| error.to_string())?;

  Ok(output.run_id.as_str().to_string())
}

fn persist_select_proof_in_recorded_context<F>(
  ctx: &mut RecordedOperationContext<'_>,
  build_result_json: F,
) -> Result<String, String>
where
  F: FnOnce(&str) -> Result<Vec<u8>, String>,
{
  let run_id = ctx.run_id().as_str().to_string();
  let result_json = build_result_json(&run_id)?;
  ctx
    .stage_artifact_bytes_with_ref(
      QQ_MUSIC_SEARCH_SELECT_RESULT_ROLE,
      result_json,
      "qqmusic-search-select-result.json",
      Some("qqmusic search select proof".to_string()),
    )
    .map_err(|error| error.to_string())?;
  Ok(run_id)
}
