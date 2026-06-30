use std::time::{SystemTime, UNIX_EPOCH};

use auv_view::memory::{
  ARTIFACT_DIR_BRIDGE_RUN_ID, MemoryReadConfig, MemoryWriteInput, ViewMemoryScopeSnapshot,
  load_memory_file, memory_file_path, try_build_memory, write_memory_file,
};
use auv_view::{VIEW_IR_SCHEMA_VERSION, ViewBounds};
use serde::{Deserialize, Serialize};

use crate::PlaylistSidebarScan;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaylistReacquireSummary {
  pub outcome: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub strategy_used: Option<String>,
  pub observation_count: usize,
  pub skipped_rescan_replay: bool,
}

pub const PLAYLIST_SIDEBAR_SCOPE_ID: &str = "playlist_sidebar";
pub const PLAYLIST_SCAN_CACHE_FILE_NAME: &str = "playlist-scan-cache.json";

pub fn enabled() -> bool {
  enabled_with_env(std::env::var("AUV_NETEASE_VIEW_MEMORY").ok().as_deref())
}

pub(crate) fn enabled_with_env(value: Option<&str>) -> bool {
  matches!(value, Some("1"))
}

pub fn system_time_millis() -> u64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|duration| duration.as_millis() as u64)
    .unwrap_or(0)
}

pub fn write_from_scan(inputs: &crate::Inputs, scan: &PlaylistSidebarScan) -> Result<(), String> {
  if !enabled() {
    return Ok(());
  }

  let reconstruction = scan.reconstruction();
  let sidebar_bounds = scan
    .sidebar_region()
    .bounds
    .unwrap_or_else(|| ViewBounds::new(0.0, 0.0, 240.0, 400.0));
  let baseline_width = sidebar_bounds.width.round().max(1.0) as u32;
  let memory = try_build_memory(
    MemoryWriteInput {
      app_bundle_id: &inputs.app_id,
      scope_id: PLAYLIST_SIDEBAR_SCOPE_ID,
      root: &reconstruction.root,
      scope_snapshot: ViewMemoryScopeSnapshot {
        region_id: PLAYLIST_SIDEBAR_SCOPE_ID.to_string(),
        region_bounds_window_local: sidebar_bounds,
        baseline_width,
        schema_version_view_ir: VIEW_IR_SCHEMA_VERSION.to_string(),
      },
      source_reconstruction_ref: PLAYLIST_SCAN_CACHE_FILE_NAME.to_string(),
      source_run_id: ARTIFACT_DIR_BRIDGE_RUN_ID.to_string(),
      last_reconstructed_at_millis: system_time_millis(),
      clean: scan.diagnostics().is_empty(),
    },
    reconstruction,
  )
  .ok_or_else(|| "scan did not produce writable ViewMemory".to_string())?;

  let path = memory_file_path(&inputs.artifact_dir, PLAYLIST_SIDEBAR_SCOPE_ID);
  write_memory_file(&path, &memory)
}

pub fn load_for_sidebar(
  inputs: &crate::Inputs,
  current_baseline_width: Option<u32>,
) -> Option<auv_view::memory::ViewMemory> {
  if !enabled() {
    return None;
  }
  let path = memory_file_path(&inputs.artifact_dir, PLAYLIST_SIDEBAR_SCOPE_ID);
  load_memory_file(
    &path,
    &MemoryReadConfig {
      now_millis: system_time_millis(),
      ..Default::default()
    },
    current_baseline_width,
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn enabled_with_env_requires_exact_value() {
    assert!(!enabled_with_env(None));
    assert!(!enabled_with_env(Some("0")));
    assert!(!enabled_with_env(Some("true")));
    assert!(enabled_with_env(Some("1")));
  }
}
