//! Post-hoc run storage for playlist ls scan + view-memory artifacts (A7-min).

use std::path::{Path, PathBuf};

use auv_tracing_driver::recorded_operation::RecordedOperationContext;
use auv_tracing_driver::run_builder::Attributes;
use auv_tracing_driver::{LocalStore, RunRecordingBackend, RunSpec, RunType};
use auv_view::memory::{
  ARTIFACT_DIR_BRIDGE_RUN_ID, PLAYLIST_SELECT_RESULT_ARTIFACT_ROLE, SPAN_MEMORY_WRITE,
  SPAN_REACQUIRE_MEMORY_LOAD, VIEW_MEMORY_ARTIFACT_ROLE, ViewMemory, memory_file_path,
  memory_write_span_attributes, reacquire_memory_load_span_attributes, reacquire_root_span_name,
  serialize_memory_bytes,
};
use serde::{Deserialize, Serialize};

use crate::view_memory::ReacquireTraceEvidence;
use crate::{Inputs, PlaylistSidebarScan};

pub const NETEASE_PLAYLIST_SIDEBAR_SCAN_ROLE: &str = "netease-playlist-sidebar-scan";
pub const NETEASE_PLAYLIST_SELECT_RESULT_ROLE: &str = PLAYLIST_SELECT_RESULT_ARTIFACT_ROLE;
pub const VIEW_MEMORY_RUN_LINEAGE_FILE: &str = "view-memory-run-lineage.json";
pub const VIEW_MEMORY_LINEAGE_SCHEMA_VERSION: &str = "view-memory-lineage-v0";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewMemoryRunLineage {
  pub schema_version: String,
  pub run_id: String,
  pub scan_artifact_id: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub memory_artifact_id: Option<String>,
  pub memory_id: String,
  pub scope_id: String,
  pub app_bundle_id: String,
  pub written_at_millis: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PersistedLineage {
  pub lineage: ViewMemoryRunLineage,
  pub memory: Option<ViewMemory>,
}

pub fn lineage_manifest_path(artifact_dir: &Path) -> PathBuf {
  artifact_dir.join(VIEW_MEMORY_RUN_LINEAGE_FILE)
}

pub fn read_lineage_manifest(artifact_dir: &Path) -> Option<ViewMemoryRunLineage> {
  let path = lineage_manifest_path(artifact_dir);
  let json = std::fs::read_to_string(&path).ok()?;
  let lineage: ViewMemoryRunLineage = serde_json::from_str(&json).ok()?;
  if lineage.schema_version != VIEW_MEMORY_LINEAGE_SCHEMA_VERSION {
    return None;
  }
  Some(lineage)
}

pub fn read_lineage_manifest_for_inputs(
  artifact_dir: &Path,
  inputs: &Inputs,
) -> Option<ViewMemoryRunLineage> {
  let lineage = read_lineage_manifest(artifact_dir)?;
  if lineage.app_bundle_id != inputs.app_id {
    return None;
  }
  if lineage.scope_id != crate::view_memory::PLAYLIST_SIDEBAR_SCOPE_ID {
    return None;
  }
  Some(lineage)
}

/// Parse `artifact_id` from a [`view_memory_lineage_ref_wire`] payload.
pub fn parse_lineage_scan_artifact_id(source_reconstruction_ref: &str) -> Option<String> {
  for token in source_reconstruction_ref.split_whitespace() {
    if let Some(artifact_id) = token.strip_prefix("artifact_id=") {
      let artifact_id = artifact_id.trim();
      if !artifact_id.is_empty() {
        return Some(artifact_id.to_string());
      }
    }
  }
  None
}

pub fn write_lineage_manifest(
  artifact_dir: &Path,
  lineage: &ViewMemoryRunLineage,
) -> Result<(), String> {
  std::fs::create_dir_all(artifact_dir)
    .map_err(|error| format!("failed to create {}: {error}", artifact_dir.display()))?;
  let path = lineage_manifest_path(artifact_dir);
  let json = serde_json::to_string_pretty(lineage)
    .map_err(|error| format!("failed to serialize lineage manifest: {error}"))?;
  let tmp = path.with_extension("json.tmp");
  std::fs::write(&tmp, json)
    .map_err(|error| format!("failed to write {}: {error}", tmp.display()))?;
  std::fs::rename(&tmp, &path).map_err(|error| {
    format!(
      "failed to rename {} to {}: {error}",
      tmp.display(),
      path.display()
    )
  })
}

/// Remove artifact-dir view-memory so store-first scan cannot pair with stale memory.
pub fn clear_artifact_dir_view_memory(inputs: &Inputs) -> Result<(), String> {
  let path = memory_file_path(
    &inputs.artifact_dir,
    crate::view_memory::PLAYLIST_SIDEBAR_SCOPE_ID,
  );
  if path.is_file() {
    std::fs::remove_file(&path)
      .map_err(|error| format!("failed to remove {}: {error}", path.display()))?;
  }
  Ok(())
}

/// Drop a stale manifest so readers can fall back to a freshly mirrored artifact-dir scan.
pub fn remove_lineage_manifest(artifact_dir: &Path) {
  let path = lineage_manifest_path(artifact_dir);
  let _ = std::fs::remove_file(path);
}

pub fn persist_playlist_ls_artifacts(
  store_root: &Path,
  scan: &PlaylistSidebarScan,
  inputs: &Inputs,
  memory_enabled: bool,
) -> Result<PersistedLineage, String> {
  let store = LocalStore::new(store_root.to_path_buf()).map_err(|error| error.to_string())?;
  let recording = RunRecordingBackend::local_only(store).handle();
  let scan_json = serde_json::to_vec_pretty(scan)
    .map_err(|error| format!("failed to serialize playlist scan: {error}"))?;

  let output = recording
    .run_recorded_operation(
      RunSpec::new(RunType::Command, "auv.netease.playlist.ls"),
      "playlist ls store artifacts",
      |ctx| persist_in_recorded_context(ctx, &scan_json, inputs, scan, memory_enabled),
    )
    .map_err(|error| error.to_string())?;

  Ok(output.value)
}

fn persist_in_recorded_context(
  ctx: &mut RecordedOperationContext<'_>,
  scan_json: &[u8],
  inputs: &Inputs,
  scan: &PlaylistSidebarScan,
  memory_enabled: bool,
) -> Result<PersistedLineage, String> {
  let (_, scan_ref) = ctx
    .stage_artifact_bytes_with_ref(
      NETEASE_PLAYLIST_SIDEBAR_SCAN_ROLE,
      scan_json,
      "playlist-scan-cache.json",
      Some("playlist sidebar scan".to_string()),
    )
    .map_err(|error| error.to_string())?;

  let run_id = ctx.run_id().as_str().to_string();
  let scan_artifact_id = scan_ref.artifact_id.as_str().to_string();
  let memory = if memory_enabled {
    crate::view_memory::try_build_writable_memory(inputs, scan, &run_id, &scan_artifact_id)
  } else {
    None
  };
  let memory_artifact_id = if let Some(memory) = &memory {
    let run_id_for_attrs = ctx.run_id().as_str();
    let attrs = span_attributes_from_pairs(memory_write_span_attributes(memory, run_id_for_attrs));
    ctx
      .in_span_with_attributes(SPAN_MEMORY_WRITE, attrs, |ctx| {
        let bytes = serialize_memory_bytes(memory).map_err(|error| error.to_string())?;
        let (_, memory_ref) = ctx
          .stage_artifact_bytes_with_ref(
            VIEW_MEMORY_ARTIFACT_ROLE,
            bytes,
            "view-memory-playlist_sidebar.json",
            Some("view memory".to_string()),
          )
          .map_err(|error| error.to_string())?;
        Ok::<_, String>(memory_ref.artifact_id.as_str().to_string())
      })?
      .into()
  } else {
    None
  };
  let memory_id = auv_view::memory::build_memory_id(
    &inputs.app_id,
    crate::view_memory::PLAYLIST_SIDEBAR_SCOPE_ID,
  );
  Ok(PersistedLineage {
    lineage: ViewMemoryRunLineage {
      schema_version: VIEW_MEMORY_LINEAGE_SCHEMA_VERSION.to_string(),
      run_id,
      scan_artifact_id,
      memory_artifact_id,
      memory_id,
      scope_id: crate::view_memory::PLAYLIST_SIDEBAR_SCOPE_ID.to_string(),
      app_bundle_id: inputs.app_id.clone(),
      written_at_millis: crate::view_memory::system_time_millis(),
    },
    memory,
  })
}

pub fn load_scan_from_store(
  store_root: &Path,
  lineage: &ViewMemoryRunLineage,
) -> Option<PlaylistSidebarScan> {
  let bytes = read_artifact_bytes(store_root, &lineage.run_id, &lineage.scan_artifact_id)?;
  let json = std::str::from_utf8(&bytes).ok()?;
  crate::decode_playlist_sidebar_scan_json(json).ok()
}

pub fn load_memory_from_store(
  store_root: &Path,
  lineage: &ViewMemoryRunLineage,
) -> Option<ViewMemory> {
  let artifact_id = lineage.memory_artifact_id.as_deref()?;
  let bytes = read_artifact_bytes(store_root, &lineage.run_id, artifact_id)?;
  serde_json::from_slice(&bytes).ok()
}

// NOTICE(store_root_read_bias_v1): When store_root is set, consumers prefer manifest →
// store over artifact-dir files. Freshness reconciliation is intentionally deferred.
pub fn try_load_scan_cache(inputs: &Inputs) -> Option<PlaylistSidebarScan> {
  try_load_scan_cache_with_limits(inputs).0
}

pub fn try_load_scan_cache_with_limits(
  inputs: &Inputs,
) -> (Option<PlaylistSidebarScan>, Vec<String>) {
  let mut known_limits = Vec::new();
  if let Some(store_root) = &inputs.store_root {
    if let Some(lineage) = read_lineage_manifest_for_inputs(&inputs.artifact_dir, inputs) {
      if let Some(scan) = load_scan_from_store(store_root, &lineage) {
        return (Some(scan), known_limits);
      }
      known_limits.push(format!(
        "store scan artifact missing for run {}; using artifact-dir fallback",
        lineage.run_id
      ));
    } else if read_lineage_manifest(&inputs.artifact_dir).is_some() {
      known_limits.push(
        "lineage manifest rejected for current app/scope; using artifact-dir fallback".into(),
      );
    } else {
      known_limits
        .push("lineage manifest missing with --store-root; using artifact-dir fallback".into());
    }
    if let Some(scan) = try_load_scan_from_memory_lineage(inputs, store_root) {
      return (Some(scan), known_limits);
    }
  }
  let cache_path = inputs.artifact_dir.join(crate::PLAYLIST_SCAN_CACHE_FILE);
  let json = match std::fs::read_to_string(&cache_path) {
    Ok(json) => json,
    Err(_) => return (None, known_limits),
  };
  (
    crate::decode_playlist_sidebar_scan_json(&json).ok(),
    known_limits,
  )
}

pub fn try_load_view_memory(inputs: &Inputs) -> Option<ViewMemory> {
  if let Some(store_root) = &inputs.store_root {
    if let Some(lineage) = read_lineage_manifest_for_inputs(&inputs.artifact_dir, inputs) {
      return load_memory_from_store(store_root, &lineage);
    }
    if let Some(memory) = try_load_memory_from_artifact_lineage(inputs, store_root) {
      return Some(memory);
    }
    return None;
  }
  let path = memory_file_path(
    &inputs.artifact_dir,
    crate::view_memory::PLAYLIST_SIDEBAR_SCOPE_ID,
  );
  auv_view::memory::parse_memory_file(&path)
}

fn try_load_scan_from_memory_lineage(
  inputs: &Inputs,
  store_root: &Path,
) -> Option<PlaylistSidebarScan> {
  let memory = load_artifact_dir_memory(inputs)?;
  if !artifact_memory_pairs_with_store(memory.source_run_id.as_str()) {
    return None;
  }
  let scan_artifact_id = parse_lineage_scan_artifact_id(&memory.source_reconstruction_ref)?;
  let lineage = lineage_from_memory(&memory, scan_artifact_id, None);
  load_scan_from_store(store_root, &lineage)
}

fn try_load_memory_from_artifact_lineage(inputs: &Inputs, store_root: &Path) -> Option<ViewMemory> {
  let memory_file = load_artifact_dir_memory(inputs)?;
  if !artifact_memory_pairs_with_store(memory_file.source_run_id.as_str()) {
    return None;
  }
  let scan_artifact_id = parse_lineage_scan_artifact_id(&memory_file.source_reconstruction_ref)?;
  let store = LocalStore::new(store_root.to_path_buf()).ok()?;
  let canonical = store.read_run(&memory_file.source_run_id).ok()?;
  let scan_present = canonical
    .artifacts
    .iter()
    .any(|artifact| artifact.artifact_id.as_str() == scan_artifact_id);
  if !scan_present {
    return None;
  }
  let memory_artifact_id = canonical
    .artifacts
    .iter()
    .find(|artifact| {
      artifact.role == VIEW_MEMORY_ARTIFACT_ROLE
        && artifact.path.ends_with("view-memory-playlist_sidebar.json")
    })
    .map(|artifact| artifact.artifact_id.as_str().to_string())?;
  let lineage = lineage_from_memory(&memory_file, scan_artifact_id, Some(memory_artifact_id));
  load_memory_from_store(store_root, &lineage)
}

fn artifact_memory_pairs_with_store(source_run_id: &str) -> bool {
  source_run_id != ARTIFACT_DIR_BRIDGE_RUN_ID
}

fn load_artifact_dir_memory(inputs: &Inputs) -> Option<ViewMemory> {
  let path = memory_file_path(
    &inputs.artifact_dir,
    crate::view_memory::PLAYLIST_SIDEBAR_SCOPE_ID,
  );
  auv_view::memory::parse_memory_file(&path)
}

fn lineage_from_memory(
  memory: &ViewMemory,
  scan_artifact_id: String,
  memory_artifact_id: Option<String>,
) -> ViewMemoryRunLineage {
  ViewMemoryRunLineage {
    schema_version: VIEW_MEMORY_LINEAGE_SCHEMA_VERSION.to_string(),
    run_id: memory.source_run_id.clone(),
    scan_artifact_id,
    memory_artifact_id,
    memory_id: memory.memory_id.clone(),
    scope_id: memory.scope_id.clone(),
    app_bundle_id: memory.app_bundle_id.clone(),
    written_at_millis: memory.last_reconstructed_at_millis,
  }
}

fn read_artifact_bytes(store_root: &Path, run_id: &str, artifact_id: &str) -> Option<Vec<u8>> {
  let store = LocalStore::new(store_root.to_path_buf()).ok()?;
  let (_, path) = store.artifact_file(run_id, artifact_id).ok()?;
  std::fs::read(&path).ok()
}

fn span_attributes_from_pairs(pairs: Vec<(String, String)>) -> Attributes {
  pairs
    .into_iter()
    .map(|(key, value)| (key, serde_json::Value::String(value)))
    .collect()
}

/// Post-hoc durable proof run for `playlist select --store-root` (A8a).
pub fn persist_playlist_select_proof(
  store_root: &Path,
  evidence: Option<&ReacquireTraceEvidence>,
  memory: Option<&ViewMemory>,
  build_result_json: impl FnOnce(&str) -> Result<Vec<u8>, String>,
) -> Result<String, String> {
  let store = LocalStore::new(store_root.to_path_buf()).map_err(|error| error.to_string())?;
  let recording = RunRecordingBackend::local_only(store).handle();

  let output = recording
    .run_recorded_operation(
      RunSpec::new(RunType::Command, "auv.netease.playlist.select"),
      "playlist select store proof",
      |ctx| persist_select_proof_in_recorded_context(ctx, evidence, memory, build_result_json),
    )
    .map_err(|error| error.to_string())?;

  Ok(output.run_id.as_str().to_string())
}

fn persist_select_proof_in_recorded_context<F>(
  ctx: &mut RecordedOperationContext<'_>,
  evidence: Option<&ReacquireTraceEvidence>,
  memory: Option<&ViewMemory>,
  build_result_json: F,
) -> Result<String, String>
where
  F: FnOnce(&str) -> Result<Vec<u8>, String>,
{
  let run_id = ctx.run_id().as_str().to_string();
  if let Some(evidence) = evidence {
    let root_name = reacquire_root_span_name(&evidence.scope_id);
    let root_attrs = span_attributes_from_pairs(evidence.to_reacquire_root_attributes());

    ctx.in_span_with_attributes(&root_name, root_attrs, |ctx| {
      // NOTICE(a8-controlled-subset): root + memory_load + winning stage only — not full 6-stage tree.
      if let Some(memory) = memory {
        let load_attrs = span_attributes_from_pairs(reacquire_memory_load_span_attributes(memory));
        ctx.in_span_with_attributes(SPAN_REACQUIRE_MEMORY_LOAD, load_attrs, |ctx| {
          emit_winning_reacquire_stage_span(ctx, evidence)?;
          Ok::<_, String>(())
        })?;
      } else {
        emit_winning_reacquire_stage_span(ctx, evidence)?;
      }
      Ok::<_, String>(())
    })?;
  }

  let result_json = build_result_json(&run_id)?;
  ctx
    .stage_artifact_bytes_with_ref(
      NETEASE_PLAYLIST_SELECT_RESULT_ROLE,
      result_json,
      "netease-playlist-select-result.json",
      Some("playlist select proof".to_string()),
    )
    .map_err(|error| error.to_string())?;

  Ok(run_id)
}

fn emit_winning_reacquire_stage_span(
  ctx: &mut RecordedOperationContext<'_>,
  evidence: &ReacquireTraceEvidence,
) -> Result<(), String> {
  if let Some(stage_name) = evidence.winning_stage_span_name() {
    // NOTICE(a8-controlled-subset): only the winning stage span is recorded in A8 v1.
    ctx.in_span(&stage_name, |ctx| {
      ctx.record_event("reacquire.stage.completed", None);
      Ok::<_, String>(())
    })?;
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use auv_view::memory::{
    VIEW_MEMORY_SCHEMA_VERSION, ViewMemoryScopeSnapshot, build_memory_id,
    view_memory_lineage_ref_wire, write_memory_file,
  };
  use auv_view::{VIEW_IR_SCHEMA_VERSION, ViewBounds};

  fn minimal_scan_json() -> String {
    serde_json::json!({
      "schema_version": VIEW_IR_SCHEMA_VERSION,
      "app": {},
      "window": {},
      "sidebar_region": {
        "bounds": {"x": 0.0, "y": 220.0, "width": 240.0, "height": 400.0}
      },
      "observations": [],
      "reconstruction": {
        "root": {
          "id": "root.sidebar",
          "kind": "collection",
          "bounds": {"x": 0.0, "y": 0.0, "width": 240.0, "height": 400.0},
          "anchors": [],
          "landmarks": [],
          "actions": [],
          "evidence": [],
          "children": [{
            "id": "item.test",
            "kind": "item",
            "label": "Store Label",
            "bounds": {"x": 32.0, "y": 74.0, "width": 120.0, "height": 20.0},
            "anchors": [{
              "id": "anchor.test",
              "label": "Store Label",
              "strength": "strong",
              "bounds": {"x": 32.0, "y": 74.0, "width": 120.0, "height": 20.0},
              "evidence_ids": []
            }],
            "landmarks": [],
            "actions": [],
            "evidence": [],
            "children": []
          }]
        },
        "anchor_index": [],
        "landmark_index": []
      },
      "projection": {
        "sections": [{
          "id": "section-created",
          "kind": "my_playlists",
          "label": "创建的歌单",
          "items": [{
            "id": "item.test",
            "label": "Store Label",
            "confidence": "high",
            "candidate_id": "obs1.candidate.test",
            "anchor_id": "anchor.test"
          }]
        }]
      },
      "boundary": {"top": "unknown", "bottom": "unknown", "left": "unknown", "right": "unknown"},
      "diagnostics": [],
      "known_limits": []
    })
    .to_string()
  }

  fn minimal_blocking_scan_json() -> String {
    let mut value: serde_json::Value =
      serde_json::from_str(&minimal_scan_json()).expect("minimal scan json");
    value["diagnostics"] = serde_json::json!([{
      "code": "parser_no_reliable_candidates",
      "message": "blocking",
      "node_id": null
    }]);
    value.to_string()
  }

  #[test]
  fn persist_and_read_scan_via_manifest() {
    let root = std::env::temp_dir().join(format!("auv-recording-persist-{}", std::process::id()));
    let artifact_dir = root.join("artifacts");
    let store_root = root.join("store");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&artifact_dir).expect("artifact dir");
    let scan = crate::decode_playlist_sidebar_scan_json(&minimal_scan_json()).expect("scan");
    let mut inputs = Inputs::with_defaults();
    inputs.app_id = "com.netease.163music".to_string();
    let persisted =
      persist_playlist_ls_artifacts(&store_root, &scan, &inputs, true).expect("persist");
    assert_ne!(persisted.lineage.run_id, ARTIFACT_DIR_BRIDGE_RUN_ID);
    assert!(persisted.lineage.memory_artifact_id.is_some());
    write_lineage_manifest(&artifact_dir, &persisted.lineage).expect("manifest");

    let mut inputs = Inputs::with_defaults();
    inputs.artifact_dir = artifact_dir.clone();
    inputs.store_root = Some(store_root.clone());
    let loaded = try_load_scan_cache(&inputs).expect("load scan");
    assert_eq!(
      loaded.projection().sections[0].items[0].label,
      "Store Label"
    );

    let _ = std::fs::remove_dir_all(&root);
  }

  #[test]
  fn store_first_beats_stale_artifact_dir_scan_cache() {
    let root = std::env::temp_dir().join(format!("auv-recording-bias-{}", std::process::id()));
    let artifact_dir = root.join("artifacts");
    let store_root = root.join("store");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&artifact_dir).expect("artifact dir");
    let scan = crate::decode_playlist_sidebar_scan_json(&minimal_scan_json()).expect("scan");
    let mut inputs = Inputs::with_defaults();
    inputs.app_id = "com.netease.163music".to_string();
    let persisted =
      persist_playlist_ls_artifacts(&store_root, &scan, &inputs, true).expect("persist");
    write_lineage_manifest(&artifact_dir, &persisted.lineage).expect("manifest");

    let stale = minimal_scan_json().replace("Store Label", "Stale Artifact Dir Label");
    std::fs::write(artifact_dir.join(crate::PLAYLIST_SCAN_CACHE_FILE), stale).expect("stale cache");

    let mut inputs = Inputs::with_defaults();
    inputs.artifact_dir = artifact_dir;
    inputs.store_root = Some(store_root);
    let loaded = try_load_scan_cache(&inputs).expect("store wins");
    assert_eq!(
      loaded.projection().sections[0].items[0].label,
      "Store Label"
    );

    let _ = std::fs::remove_dir_all(&root);
  }

  #[test]
  fn parse_lineage_scan_artifact_id_reads_wire_form() {
    let wire = view_memory_lineage_ref_wire("run_abc", "artifact_0001");
    assert_eq!(
      parse_lineage_scan_artifact_id(&wire).as_deref(),
      Some("artifact_0001")
    );
  }

  #[test]
  fn play_candidate_id_path_loads_scan_from_store_first() {
    let root = std::env::temp_dir().join(format!("auv-recording-candidate-{}", std::process::id()));
    let artifact_dir = root.join("artifacts");
    let store_root = root.join("store");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&artifact_dir).expect("artifact dir");
    let scan = crate::decode_playlist_sidebar_scan_json(&minimal_scan_json()).expect("scan");
    let mut inputs = Inputs::with_defaults();
    inputs.app_id = "com.netease.163music".to_string();
    let persisted =
      persist_playlist_ls_artifacts(&store_root, &scan, &inputs, false).expect("persist");
    write_lineage_manifest(&artifact_dir, &persisted.lineage).expect("manifest");

    let stale = minimal_scan_json().replace("obs1.candidate.test", "obs9.candidate.stale");
    std::fs::write(artifact_dir.join(crate::PLAYLIST_SCAN_CACHE_FILE), stale).expect("stale cache");

    let mut inputs = Inputs::with_defaults();
    inputs.artifact_dir = artifact_dir;
    inputs.store_root = Some(store_root);
    let loaded = try_load_scan_cache(&inputs).expect("store-first scan");
    let target = loaded
      .select_target_by_candidate_id("obs1.candidate.test")
      .expect("candidate id resolves");
    assert_eq!(target.label, "Store Label");
  }

  #[test]
  fn play_query_path_loads_scan_from_store_first() {
    let root = std::env::temp_dir().join(format!("auv-recording-query-{}", std::process::id()));
    let artifact_dir = root.join("artifacts");
    let store_root = root.join("store");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&artifact_dir).expect("artifact dir");
    let scan = crate::decode_playlist_sidebar_scan_json(&minimal_scan_json()).expect("scan");
    let mut inputs = Inputs::with_defaults();
    inputs.app_id = "com.netease.163music".to_string();
    let persisted =
      persist_playlist_ls_artifacts(&store_root, &scan, &inputs, true).expect("persist");
    write_lineage_manifest(&artifact_dir, &persisted.lineage).expect("manifest");

    let stale = minimal_scan_json().replace("Store Label", "Stale Query Label");
    std::fs::write(artifact_dir.join(crate::PLAYLIST_SCAN_CACHE_FILE), stale).expect("stale cache");

    let mut inputs = Inputs::with_defaults();
    inputs.artifact_dir = artifact_dir;
    inputs.store_root = Some(store_root);
    let loaded = try_load_scan_cache(&inputs).expect("store-first scan");
    let target = loaded.select_target("Store").expect("query resolves");
    assert_eq!(target.label, "Store Label");

    let _ = std::fs::remove_dir_all(&root);
  }

  #[test]
  fn without_store_root_reads_artifact_dir_scan_cache() {
    let root = std::env::temp_dir().join(format!("auv-recording-a6-{}", std::process::id()));
    let artifact_dir = root.join("artifacts");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&artifact_dir).expect("artifact dir");
    let scan = crate::decode_playlist_sidebar_scan_json(&minimal_scan_json()).expect("scan");
    let json = serde_json::to_string_pretty(&scan).expect("json");
    std::fs::write(artifact_dir.join(crate::PLAYLIST_SCAN_CACHE_FILE), json).expect("cache");

    let mut inputs = Inputs::with_defaults();
    inputs.artifact_dir = artifact_dir;
    assert!(inputs.store_root.is_none());
    let loaded = try_load_scan_cache(&inputs).expect("artifact-dir scan");
    assert_eq!(
      loaded.projection().sections[0].items[0].label,
      "Store Label"
    );

    let _ = std::fs::remove_dir_all(&root);
  }

  #[test]
  fn manifest_missing_with_store_root_reports_known_limits() {
    let mut inputs = Inputs::with_defaults();
    inputs.store_root = Some(std::env::temp_dir());
    let (_scan, limits) = try_load_scan_cache_with_limits(&inputs);
    assert!(_scan.is_none());
    assert!(
      limits
        .iter()
        .any(|limit| limit.contains("lineage manifest missing"))
    );
  }

  #[test]
  fn stale_artifact_memory_not_used_when_store_scan_has_no_memory() {
    let root = std::env::temp_dir().join(format!("auv-recording-stale-mem-{}", std::process::id()));
    let artifact_dir = root.join("artifacts");
    let store_root = root.join("store");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&artifact_dir).expect("artifact dir");
    let blocking =
      crate::decode_playlist_sidebar_scan_json(&minimal_blocking_scan_json()).expect("scan");
    let mut inputs = Inputs::with_defaults();
    inputs.app_id = "com.netease.163music".to_string();
    inputs.artifact_dir = artifact_dir.clone();
    let stale_memory = ViewMemory {
      schema_version: VIEW_MEMORY_SCHEMA_VERSION.to_string(),
      memory_id: build_memory_id("com.netease.163music", "playlist_sidebar"),
      app_bundle_id: "com.netease.163music".into(),
      scope_id: "playlist_sidebar".into(),
      last_reconstructed_at_millis: 1,
      source_run_id: "run_old".into(),
      source_reconstruction_ref: view_memory_lineage_ref_wire("run_old", "artifact_0001"),
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
    write_memory_file(
      &memory_file_path(&artifact_dir, "playlist_sidebar"),
      &stale_memory,
    )
    .expect("stale memory");

    let persisted =
      persist_playlist_ls_artifacts(&store_root, &blocking, &inputs, true).expect("persist");
    assert!(persisted.lineage.memory_artifact_id.is_none());
    write_lineage_manifest(&artifact_dir, &persisted.lineage).expect("manifest");

    let mut inputs = Inputs::with_defaults();
    inputs.artifact_dir = artifact_dir;
    inputs.store_root = Some(store_root);
    inputs.app_id = "com.netease.163music".to_string();
    assert!(try_load_view_memory(&inputs).is_none());

    let _ = std::fs::remove_dir_all(&root);
  }

  #[test]
  fn removed_manifest_falls_back_to_fresh_artifact_dir_scan() {
    let root = std::env::temp_dir().join(format!(
      "auv-recording-manifest-drop-{}",
      std::process::id()
    ));
    let artifact_dir = root.join("artifacts");
    let store_root = root.join("store");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&artifact_dir).expect("artifact dir");

    let old_scan = crate::decode_playlist_sidebar_scan_json(&minimal_scan_json()).expect("scan");
    let mut inputs = Inputs::with_defaults();
    inputs.app_id = "com.netease.163music".to_string();
    let old_persisted =
      persist_playlist_ls_artifacts(&store_root, &old_scan, &inputs, false).expect("old persist");
    write_lineage_manifest(&artifact_dir, &old_persisted.lineage).expect("old manifest");

    let fresh_scan = crate::decode_playlist_sidebar_scan_json(
      &minimal_scan_json().replace("Store Label", "Fresh Mirror Label"),
    )
    .expect("fresh scan");
    let fresh_json = serde_json::to_string_pretty(&fresh_scan).expect("json");
    std::fs::write(
      artifact_dir.join(crate::PLAYLIST_SCAN_CACHE_FILE),
      fresh_json,
    )
    .expect("mirror");
    remove_lineage_manifest(&artifact_dir);

    let mut inputs = Inputs::with_defaults();
    inputs.artifact_dir = artifact_dir;
    inputs.store_root = Some(store_root);
    inputs.app_id = "com.netease.163music".to_string();
    let (loaded, limits) = try_load_scan_cache_with_limits(&inputs);
    let loaded = loaded.expect("artifact-dir fallback scan");
    assert_eq!(
      loaded.projection().sections[0].items[0].label,
      "Fresh Mirror Label"
    );
    assert!(
      limits
        .iter()
        .any(|limit| limit.contains("lineage manifest missing"))
    );

    let _ = std::fs::remove_dir_all(&root);
  }
  fn sample_select_result_json(run_id: &str) -> Result<Vec<u8>, String> {
    let json = serde_json::json!({
      "command": "playlist.select",
      "query": "Test",
      "app": {},
      "window": {},
      "target": {
        "label": "Test Playlist",
        "section_id": "section.created",
        "section_kind": "my_playlists",
        "item_id": "item.test",
        "anchor_id": null,
        "candidate_id": "item.test",
        "observation_index": 0,
        "bounds": {"x": 32.0, "y": 74.0, "width": 120.0, "height": 20.0}
      },
      "steps": [{"name": "reacquire-target", "target_bounds": null, "delivery_path": null, "fallback_reason": null}],
      "verification": {"status": "passed", "method": "main_title_ocr_full_window_v1", "observed_title": "Test Playlist", "artifact": null, "note": null},
      "diagnostics": [],
      "known_limits": [],
      "reacquire": {
        "outcome": "reacquired",
        "strategy_used": "label_current_viewport",
        "observation_count": 1,
        "skipped_rescan_replay": true
      },
      "run_id": run_id
    });
    serde_json::to_vec_pretty(&json).map_err(|error| error.to_string())
  }

  fn sample_reacquire_evidence() -> ReacquireTraceEvidence {
    ReacquireTraceEvidence {
      scope_id: "playlist_sidebar".into(),
      target_kind: "label".into(),
      outcome: "reacquired".into(),
      stage_used: "label_current_viewport".into(),
      observation_count: 1,
      skipped_rescan_replay: true,
      stale_reason: None,
      strategy_used: Some("label_current_viewport".into()),
    }
  }

  fn sample_store_memory(run_id: &str) -> ViewMemory {
    ViewMemory {
      schema_version: VIEW_MEMORY_SCHEMA_VERSION.to_string(),
      memory_id: build_memory_id("com.netease.163music", "playlist_sidebar"),
      app_bundle_id: "com.netease.163music".into(),
      scope_id: "playlist_sidebar".into(),
      last_reconstructed_at_millis: 1_719_744_000_000,
      source_run_id: run_id.into(),
      source_reconstruction_ref: view_memory_lineage_ref_wire(run_id, "artifact_0001"),
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
    }
  }

  #[test]
  fn memory_write_span_on_store_persist() {
    let root = std::env::temp_dir().join(format!("auv-a8-mem-span-{}", std::process::id()));
    let store_root = root.join("store");
    let _ = std::fs::remove_dir_all(&root);
    let scan = crate::decode_playlist_sidebar_scan_json(&minimal_scan_json()).expect("scan");
    let mut inputs = Inputs::with_defaults();
    inputs.app_id = "com.netease.163music".to_string();
    let persisted =
      persist_playlist_ls_artifacts(&store_root, &scan, &inputs, true).expect("persist");
    let store = LocalStore::new(store_root.clone()).expect("store");
    let run = store.read_run(&persisted.lineage.run_id).expect("run");
    let span = run
      .spans
      .iter()
      .find(|span| span.name == SPAN_MEMORY_WRITE)
      .expect("memory_write span");
    assert_eq!(span.attributes.len(), 6);
    assert_eq!(
      span
        .attributes
        .get(auv_view::memory::ATTR_MEMORY_MEMORY_ID)
        .and_then(|value| value.as_str()),
      Some("com.netease.163music:playlist_sidebar")
    );
    let _ = std::fs::remove_dir_all(&root);
  }

  #[test]
  fn no_memory_write_span_when_gate_off() {
    let root = std::env::temp_dir().join(format!("auv-a8-gate-off-{}", std::process::id()));
    let store_root = root.join("store");
    let _ = std::fs::remove_dir_all(&root);
    let scan = crate::decode_playlist_sidebar_scan_json(&minimal_scan_json()).expect("scan");
    let mut inputs = Inputs::with_defaults();
    inputs.app_id = "com.netease.163music".to_string();
    let persisted =
      persist_playlist_ls_artifacts(&store_root, &scan, &inputs, false).expect("persist");
    let store = LocalStore::new(store_root.clone()).expect("store");
    let run = store.read_run(&persisted.lineage.run_id).expect("run");
    assert!(!run.spans.iter().any(|span| span.name == SPAN_MEMORY_WRITE));
    let _ = std::fs::remove_dir_all(&root);
  }

  #[test]
  fn no_durable_spans_without_store_root() {
    let root = std::env::temp_dir().join(format!("auv-a8-no-store-{}", std::process::id()));
    let store_root = root.join("store");
    let _ = std::fs::remove_dir_all(&root);
    let store = LocalStore::new(store_root.clone()).expect("store");
    assert_eq!(store.list_runs().expect("list runs").len(), 0);

    // CLI contract: default inputs omit store_root, so persist is never invoked.
    let inputs = Inputs::with_defaults();
    assert!(inputs.store_root.is_none());
    assert_eq!(store.list_runs().expect("list runs").len(), 0);

    // Contrast: explicit store_root + persist creates durable reacquire spans.
    let evidence = sample_reacquire_evidence();
    let memory = sample_store_memory("run_no_store_contrast");
    let run_id = persist_playlist_select_proof(
      &store_root,
      Some(&evidence),
      Some(&memory),
      sample_select_result_json,
    )
    .expect("persist select proof");
    let runs = store.list_runs().expect("list runs");
    assert_eq!(runs.len(), 1);
    let run = store.read_run(&run_id).expect("run");
    assert!(
      run
        .spans
        .iter()
        .any(|span| span.name.starts_with("view.reacquire"))
    );
    let _ = std::fs::remove_dir_all(&root);
  }

  #[test]
  fn select_proof_run_emits_reacquire_root() {
    let root = std::env::temp_dir().join(format!("auv-a8-select-root-{}", std::process::id()));
    let store_root = root.join("store");
    let _ = std::fs::remove_dir_all(&root);
    let evidence = sample_reacquire_evidence();
    let memory = sample_store_memory("run_ls");
    let run_id = persist_playlist_select_proof(
      &store_root,
      Some(&evidence),
      Some(&memory),
      sample_select_result_json,
    )
    .expect("persist select proof");
    let store = LocalStore::new(store_root.clone()).expect("store");
    let run = store.read_run(&run_id).expect("run");
    let root_span = run
      .spans
      .iter()
      .find(|span| span.name == "view.reacquire.playlist_sidebar")
      .expect("reacquire root span");
    assert_eq!(
      root_span
        .attributes
        .get(auv_view::memory::ATTR_REACQUIRE_OUTCOME)
        .and_then(|value| value.as_str()),
      Some("reacquired")
    );
    assert_eq!(
      root_span
        .attributes
        .get(auv_view::memory::ATTR_REACQUIRE_STAGE_USED)
        .and_then(|value| value.as_str()),
      Some("label_current_viewport")
    );
    assert!(
      run
        .spans
        .iter()
        .any(|span| span.name == SPAN_REACQUIRE_MEMORY_LOAD)
    );
    assert!(
      run
        .spans
        .iter()
        .any(|span| { span.name == "view.reacquire.stage.3.label_current_viewport" })
    );
    let _ = std::fs::remove_dir_all(&root);
  }

  #[test]
  fn select_result_artifact_staged() {
    let root = std::env::temp_dir().join(format!("auv-a8-select-artifact-{}", std::process::id()));
    let store_root = root.join("store");
    let _ = std::fs::remove_dir_all(&root);
    let evidence = sample_reacquire_evidence();
    let run_id = persist_playlist_select_proof(
      &store_root,
      Some(&evidence),
      None,
      sample_select_result_json,
    )
    .expect("persist");
    let store = LocalStore::new(store_root.clone()).expect("store");
    let run = store.read_run(&run_id).expect("run");
    let artifact = run
      .artifacts
      .iter()
      .find(|artifact| artifact.role == NETEASE_PLAYLIST_SELECT_RESULT_ROLE)
      .expect("select result artifact");
    let bytes = read_artifact_bytes(&store_root, &run_id, artifact.artifact_id.as_str())
      .expect("artifact bytes");
    let value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(
      value["verification"]["method"],
      "main_title_ocr_full_window_v1"
    );
    assert!(
      value["steps"]
        .as_array()
        .is_some_and(|steps| !steps.is_empty())
    );
    let _ = std::fs::remove_dir_all(&root);
  }

  #[test]
  fn select_run_id_in_json() {
    let root = std::env::temp_dir().join(format!("auv-a8-run-id-{}", std::process::id()));
    let store_root = root.join("store");
    let _ = std::fs::remove_dir_all(&root);
    let evidence = sample_reacquire_evidence();
    let run_id = persist_playlist_select_proof(
      &store_root,
      Some(&evidence),
      None,
      sample_select_result_json,
    )
    .expect("persist");
    let store = LocalStore::new(store_root.clone()).expect("store");
    let run = store.read_run(&run_id).expect("run");
    let artifact = run
      .artifacts
      .iter()
      .find(|artifact| artifact.role == NETEASE_PLAYLIST_SELECT_RESULT_ROLE)
      .expect("artifact");
    let bytes =
      read_artifact_bytes(&store_root, &run_id, artifact.artifact_id.as_str()).expect("bytes");
    let decoded: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(decoded["run_id"].as_str(), Some(run_id.as_str()));
    let _ = std::fs::remove_dir_all(&root);
  }

  #[test]
  fn select_proof_without_reacquire_summary_emits_no_reacquire_spans() {
    let root =
      std::env::temp_dir().join(format!("auv-a8-no-reacquire-span-{}", std::process::id()));
    let store_root = root.join("store");
    let _ = std::fs::remove_dir_all(&root);
    let run_id = persist_playlist_select_proof(&store_root, None, None, sample_select_result_json)
      .expect("persist");
    let store = LocalStore::new(store_root.clone()).expect("store");
    let run = store.read_run(&run_id).expect("run");
    assert!(
      !run
        .spans
        .iter()
        .any(|span| span.name.starts_with("view.reacquire")),
      "unexpected reacquire spans: {:?}",
      run
        .spans
        .iter()
        .map(|span| span.name.as_str())
        .collect::<Vec<_>>()
    );
    assert!(
      run
        .artifacts
        .iter()
        .any(|artifact| artifact.role == NETEASE_PLAYLIST_SELECT_RESULT_ROLE)
    );
    let _ = std::fs::remove_dir_all(&root);
  }
}
