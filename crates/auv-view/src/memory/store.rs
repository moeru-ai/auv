use std::path::{Path, PathBuf};

use super::{MemoryReadConfig, ViewMemory, read_memory};

pub fn memory_file_name(scope_id: &str) -> String {
  format!("view-memory-{scope_id}.json")
}

pub fn memory_file_path(artifact_dir: &Path, scope_id: &str) -> PathBuf {
  artifact_dir.join(memory_file_name(scope_id))
}

pub fn serialize_memory_bytes(memory: &ViewMemory) -> Result<Vec<u8>, String> {
  serde_json::to_vec_pretty(memory).map_err(|error| format!("failed to serialize ViewMemory: {error}"))
}

// NOTICE(view-memory-lineage-wire-v0): Wire form for ViewMemory.source_reconstruction_ref
// only. Example: run_id=run_abc artifact_id=artifact_0001
pub fn view_memory_lineage_ref_wire(run_id: &str, scan_artifact_id: &str) -> String {
  format!("run_id={run_id} artifact_id={scan_artifact_id}")
}

pub fn write_memory_file(path: &Path, memory: &ViewMemory) -> Result<(), String> {
  if let Some(parent) = path.parent() {
    std::fs::create_dir_all(parent).map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
  }
  let json = serde_json::to_string_pretty(memory).map_err(|error| format!("failed to serialize ViewMemory: {error}"))?;
  std::fs::write(path, json).map_err(|error| format!("failed to write {}: {error}", path.display()))
}

pub fn parse_memory_file(path: &Path) -> Option<ViewMemory> {
  let json = std::fs::read_to_string(path).ok()?;
  serde_json::from_str(&json).ok()
}

pub fn load_memory_file(path: &Path, config: &MemoryReadConfig, current_baseline_width: Option<u32>) -> Option<ViewMemory> {
  let json = std::fs::read_to_string(path).ok()?;
  let memory: ViewMemory = serde_json::from_str(&json).ok()?;
  match read_memory(memory, config, current_baseline_width) {
    super::MemoryReadOutcome::Accepted(memory) => Some(memory),
    super::MemoryReadOutcome::Rejected { .. } => None,
  }
}

#[cfg(test)]
mod tests {
  use super::ViewMemory;
  use super::*;
  use crate::ViewBounds;
  use crate::memory::{ARTIFACT_DIR_BRIDGE_RUN_ID, MemoryReadConfig, VIEW_MEMORY_SCHEMA_VERSION, ViewMemoryScopeSnapshot};

  #[test]
  fn view_memory_lineage_ref_wire_formats_scan_artifact_pointer() {
    assert_eq!(view_memory_lineage_ref_wire("run_abc", "artifact_0001"), "run_id=run_abc artifact_id=artifact_0001");
  }

  #[test]
  fn serialize_memory_bytes_round_trips_json() {
    let memory = ViewMemory {
      schema_version: VIEW_MEMORY_SCHEMA_VERSION.to_string(),
      memory_id: "com.netease.163music:playlist_sidebar".into(),
      app_bundle_id: "com.netease.163music".into(),
      scope_id: "playlist_sidebar".into(),
      last_reconstructed_at_millis: 1,
      source_run_id: "run_test".into(),
      source_reconstruction_ref: view_memory_lineage_ref_wire("run_test", "artifact_0001"),
      anchors: Vec::new(),
      landmarks: Vec::new(),
      node_snapshots: Default::default(),
      scope_snapshot: ViewMemoryScopeSnapshot {
        region_id: "playlist_sidebar".into(),
        region_bounds_window_local: ViewBounds::new(0.0, 0.0, 240.0, 400.0),
        baseline_width: 240,
        schema_version_view_ir: "view-ir-v0".into(),
      },
      diagnostics: Vec::new(),
    };
    let bytes = serialize_memory_bytes(&memory).expect("serialize");
    let decoded: ViewMemory = serde_json::from_slice(&bytes).expect("decode");
    assert_eq!(decoded.source_reconstruction_ref, memory.source_reconstruction_ref);
  }

  #[test]
  fn store_roundtrip_load_latest() {
    let dir = std::env::temp_dir().join(format!("auv-view-memory-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");

    let memory = ViewMemory {
      schema_version: VIEW_MEMORY_SCHEMA_VERSION.to_string(),
      memory_id: "com.netease.163music:playlist_sidebar".into(),
      app_bundle_id: "com.netease.163music".into(),
      scope_id: "playlist_sidebar".into(),
      last_reconstructed_at_millis: 1_719_744_000_000,
      source_run_id: ARTIFACT_DIR_BRIDGE_RUN_ID.into(),
      source_reconstruction_ref: "playlist-scan-cache.json".into(),
      anchors: Vec::new(),
      landmarks: Vec::new(),
      node_snapshots: Default::default(),
      scope_snapshot: ViewMemoryScopeSnapshot {
        region_id: "playlist_sidebar".into(),
        region_bounds_window_local: ViewBounds::new(0.0, 0.0, 240.0, 400.0),
        baseline_width: 240,
        schema_version_view_ir: "view-ir-v0".into(),
      },
      diagnostics: Vec::new(),
    };

    let path = memory_file_path(&dir, "playlist_sidebar");
    write_memory_file(&path, &memory).expect("write memory");
    let loaded = load_memory_file(
      &path,
      &MemoryReadConfig {
        now_millis: memory.last_reconstructed_at_millis,
        ..Default::default()
      },
      Some(240),
    )
    .expect("load memory");
    assert_eq!(loaded.memory_id, memory.memory_id);

    let _ = std::fs::remove_dir_all(&dir);
  }
}
