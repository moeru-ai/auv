use super::ViewMemory;
use super::*;
use crate::ViewBounds;
use crate::memory::{VIEW_MEMORY_SCHEMA_VERSION, ViewMemoryScopeSnapshot};
use auv_tracing::{ArtifactId, ArtifactUri, RunId};

fn sample_memory(last_millis: u64) -> ViewMemory {
  ViewMemory {
    schema_version: VIEW_MEMORY_SCHEMA_VERSION.to_string(),
    source_scan_uri: ArtifactUri::from_ids(RunId::new(), ArtifactId::new()),
    memory_id: "app:scope".into(),
    app_bundle_id: "app".into(),
    scope_id: "scope".into(),
    last_reconstructed_at_millis: last_millis,
    anchors: Vec::new(),
    landmarks: Vec::new(),
    node_snapshots: Default::default(),
    scope_snapshot: ViewMemoryScopeSnapshot {
      region_id: "playlist_sidebar".into(),
      region_bounds_window_local: ViewBounds::default(),
      baseline_width: 240,
      schema_version_view_ir: "view-ir-v0".into(),
    },
    diagnostics: Vec::new(),
  }
}

#[test]
fn read_rejects_expired_memory() {
  let memory = sample_memory(1_000);
  let config = MemoryReadConfig {
    now_millis: 1_000 + DEFAULT_MEMORY_TTL_MILLIS + 1,
    ..Default::default()
  };
  match read_memory(memory, &config, None) {
    MemoryReadOutcome::Rejected {
      reason: StaleReason::MemoryRejectedAtFreshness,
    } => {}
    other => panic!("expected freshness rejection, got {other:?}"),
  }
}

#[test]
fn read_rejects_schema_mismatch() {
  let mut memory = sample_memory(1_000);
  memory.schema_version = "view-memory-v99".into();
  let config = MemoryReadConfig {
    now_millis: 1_000,
    ..Default::default()
  };
  match read_memory(memory, &config, None) {
    MemoryReadOutcome::Rejected {
      reason: StaleReason::SchemaMismatch,
    } => {}
    other => panic!("expected schema rejection, got {other:?}"),
  }
}

#[test]
fn read_rejects_baseline_mismatch() {
  let memory = sample_memory(1_000);
  let config = MemoryReadConfig {
    now_millis: 1_000,
    ..Default::default()
  };
  match read_memory(memory, &config, Some(400)) {
    MemoryReadOutcome::Rejected {
      reason: StaleReason::BaselineMismatchHard,
    } => {}
    other => panic!("expected baseline rejection, got {other:?}"),
  }
}
