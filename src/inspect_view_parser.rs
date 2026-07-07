//! Thin inspect wrappers for SceneBridge A8 view-parser read surface.

use auv_tracing_driver::store::{CanonicalRun, LocalStore};
use auv_view::memory::{ViewMemory, ViewParserInspect, format_view_resolution_summary_text};

use crate::inspect::read_run;
use crate::model::AuvResult;
use crate::view_parser_read;

pub fn list_view_memory_writes(store: &LocalStore, run_id: &str) -> AuvResult<Vec<ViewMemory>> {
  view_parser_read::list_view_memory_writes(store, run_id)
}

pub fn build_view_parser_inspect_for_run(store: &LocalStore, run: &CanonicalRun) -> AuvResult<ViewParserInspect> {
  view_parser_read::build_view_parser_inspect(store, run)
}

pub fn view_parser_inspect(store: &LocalStore, run_id: &str) -> AuvResult<ViewParserInspect> {
  let run = read_run(store, run_id)?;
  build_view_parser_inspect_for_run(store, &run)
}

pub fn append_view_parser_proof_text_from_run(store: &LocalStore, run: &CanonicalRun, output: &mut String) -> AuvResult<()> {
  let view_parser = build_view_parser_inspect_for_run(store, run)?;
  if view_parser.resolution_summaries.is_empty() {
    return Ok(());
  }
  output.push_str("\nView parser proof:\n");
  for summary in &view_parser.resolution_summaries {
    output.push_str(&format_view_resolution_summary_text(summary));
  }
  Ok(())
}

pub fn append_view_parser_proof_text(store: &LocalStore, run_id: &str, output: &mut String) -> AuvResult<()> {
  let run = read_run(store, run_id)?;
  append_view_parser_proof_text_from_run(store, &run, output)
}
