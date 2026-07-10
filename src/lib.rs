// File: src/lib.rs
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
pub mod verticals;
pub mod view_parser_read;

pub use verticals::balatro;
pub use verticals::minecraft::{
  self as minecraft, query_live_action as minecraft_query_live_action, session as minecraft_session, verification as minecraft_verification,
};
pub use verticals::osu::{self as osu, query_live_action as osu_query_live_action};

use std::path::PathBuf;

use auv_tracing_driver::store::LocalStore;
use model::AuvResult;
use runtime::Runtime;

#[derive(Clone, Debug)]
pub struct RootInspectReadProjection;

impl auv_inspect_server::InspectReadProjection for RootInspectReadProjection {
  fn run_enrichment(
    &self,
    store: &auv_tracing_driver::store::LocalStore,
    run: &auv_tracing_driver::store::CanonicalRun,
  ) -> Result<auv_inspect_server::InspectRunEnrichment, String> {
    let view_parser = view_parser_read::build_view_parser_inspect(store, run)?;
    let view_parser_summary = auv_view::memory::summarize_view_parser_inspect(&view_parser);
    Ok(auv_inspect_server::InspectRunEnrichment {
      command_boundary_claims: extract_command_boundary_claims_for_inspect(run),
      verifications: run_read::extract_verifications(store, run)?
        .into_iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to encode verification inspect values: {error}"))?,
      observation_snapshots: run_read::extract_observation_snapshots(store, run)?
        .into_iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to encode observation snapshot inspect values: {error}"))?,
      detector_recognition_lineage: run_read::extract_detector_recognition_lineage(store, run)?
        .into_iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to encode detector recognition lineage inspect values: {error}"))?,
      view_parser,
      view_parser_summary,
    })
  }

  fn run_json_extension(
    &self,
    extension: &str,
    store: &auv_tracing_driver::store::LocalStore,
    run_id: &str,
  ) -> Result<serde_json::Value, String> {
    match extension {
      "minecraft-quality-baseline-report" => serde_json::to_value(run_read::quality_baseline_report_with_verdicts_for_run(store, run_id)?)
        .map_err(|error| format!("failed to encode minecraft quality baseline report: {error}")),
      other => Err(format!("inspect run extension {other:?} is not supported by the root projection")),
    }
  }
}

fn extract_command_boundary_claims_for_inspect(
  run: &auv_tracing_driver::store::CanonicalRun,
) -> Vec<auv_inspect_server::CommandBoundaryClaim> {
  run
    .events
    .iter()
    .filter_map(|event| match event.name.as_str() {
      "command.verification" => Some(auv_inspect_server::CommandBoundaryClaim {
        span_id: event.span_id.clone(),
        kind: "verification".to_string(),
        message: event.message.clone().unwrap_or_default(),
      }),
      "command.known_limit" => Some(auv_inspect_server::CommandBoundaryClaim {
        span_id: event.span_id.clone(),
        kind: "known_limit".to_string(),
        message: event.message.clone().unwrap_or_default(),
      }),
      _ => None,
    })
    .collect()
}

pub fn build_default_runtime(project_root: PathBuf) -> AuvResult<Runtime> {
  let store_root = default_project_store_root(project_root.clone());
  build_runtime_with_store_root(project_root, store_root)
}

pub fn build_runtime_with_store_root(project_root: PathBuf, store_root: PathBuf) -> AuvResult<Runtime> {
  let store = LocalStore::new(store_root)?;
  Ok(Runtime::new(project_root, store))
}

pub fn default_project_store_root(project_root: PathBuf) -> PathBuf {
  project_root.join(".auv")
}

pub fn build_default_store(project_root: PathBuf) -> AuvResult<LocalStore> {
  LocalStore::new(default_project_store_root(project_root))
}
