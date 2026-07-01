//! Donor-neutral ViewMemory / reacquisition trace span names and attribute builders.

use super::ViewMemory;

pub const SPAN_MEMORY_WRITE: &str = "view.parse.memory_write";

pub const SPAN_REACQUIRE_ROOT_PREFIX: &str = "view.reacquire.";
pub const SPAN_REACQUIRE_MEMORY_LOAD: &str = "view.reacquire.memory_load";
pub const SPAN_REACQUIRE_STAGE_PREFIX: &str = "view.reacquire.stage.";

pub const ATTR_MEMORY_MEMORY_ID: &str = "view.memory.memory_id";
pub const ATTR_MEMORY_NODE_SNAPSHOT_COUNT: &str = "view.memory.node_snapshot_count";
pub const ATTR_MEMORY_ANCHOR_COUNT: &str = "view.memory.anchor_count";
pub const ATTR_MEMORY_LANDMARK_COUNT: &str = "view.memory.landmark_count";
pub const ATTR_MEMORY_EVICTION_COUNT: &str = "view.memory.eviction_count";
pub const ATTR_MEMORY_LAST_RECONSTRUCTED_AT_MILLIS: &str =
  "view.memory.last_reconstructed_at_millis";

pub const ATTR_REACQUIRE_SCOPE_ID: &str = "view.reacquire.scope_id";
pub const ATTR_REACQUIRE_TARGET_KIND: &str = "view.reacquire.target_kind";
pub const ATTR_REACQUIRE_OUTCOME: &str = "view.reacquire.outcome";
pub const ATTR_REACQUIRE_STAGE_USED: &str = "view.reacquire.stage_used";
pub const ATTR_REACQUIRE_OBSERVATION_COUNT: &str = "view.reacquire.observation_count";
pub const ATTR_REACQUIRE_FATAL_DIAGNOSTIC_KIND: &str = "view.reacquire.fatal_diagnostic_kind";
pub const ATTR_REACQUIRE_SKIPPED_RESCAN_REPLAY: &str = "view.reacquire.skipped_rescan_replay";

pub const ATTR_MEMORY_LOAD_MEMORY_ID: &str = "view.memory.memory_id";
pub const ATTR_MEMORY_LOAD_SOURCE_RUN_ID: &str = "view.memory.source_run_id";

/// Root reacquisition span name for a scope, e.g. `view.reacquire.playlist_sidebar`.
pub fn reacquire_root_span_name(scope_id: &str) -> String {
  format!("{SPAN_REACQUIRE_ROOT_PREFIX}{scope_id}")
}

/// Winning-stage span name: `view.reacquire.stage.<n>.<strategy>`.
pub fn reacquire_stage_span_name(stage_index: u8, strategy: &str) -> String {
  format!("{SPAN_REACQUIRE_STAGE_PREFIX}{stage_index}.{strategy}")
}

/// Required `view.parse.memory_write` attributes per view-memory-v0.
///
/// `source_run_id` is accepted for caller context (persist run id); it is not
/// emitted as a span attribute in A8a — the six required keys come from
/// [`ViewMemory`] fields only.
pub fn memory_write_span_attributes(
  memory: &ViewMemory,
  _source_run_id: &str,
) -> Vec<(String, String)> {
  vec![
    (ATTR_MEMORY_MEMORY_ID.to_string(), memory.memory_id.clone()),
    (
      ATTR_MEMORY_NODE_SNAPSHOT_COUNT.to_string(),
      memory.node_snapshots.len().to_string(),
    ),
    (
      ATTR_MEMORY_ANCHOR_COUNT.to_string(),
      memory.anchors.len().to_string(),
    ),
    (
      ATTR_MEMORY_LANDMARK_COUNT.to_string(),
      memory.landmarks.len().to_string(),
    ),
    (ATTR_MEMORY_EVICTION_COUNT.to_string(), "0".to_string()),
    (
      ATTR_MEMORY_LAST_RECONSTRUCTED_AT_MILLIS.to_string(),
      memory.last_reconstructed_at_millis.to_string(),
    ),
  ]
}

/// Minimal attributes for `view.reacquire.memory_load`.
pub fn reacquire_memory_load_span_attributes(memory: &ViewMemory) -> Vec<(String, String)> {
  vec![
    (
      ATTR_MEMORY_LOAD_MEMORY_ID.to_string(),
      memory.memory_id.clone(),
    ),
    (
      ATTR_MEMORY_LOAD_SOURCE_RUN_ID.to_string(),
      memory.source_run_id.clone(),
    ),
  ]
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::memory::{VIEW_MEMORY_SCHEMA_VERSION, ViewMemoryScopeSnapshot, ViewNodeSnapshot};
  use crate::{VIEW_IR_SCHEMA_VERSION, ViewBounds};

  fn sample_memory() -> ViewMemory {
    ViewMemory {
      schema_version: VIEW_MEMORY_SCHEMA_VERSION.to_string(),
      memory_id: "com.example.app:playlist_sidebar".into(),
      app_bundle_id: "com.example.app".into(),
      scope_id: "playlist_sidebar".into(),
      last_reconstructed_at_millis: 1_719_744_000_000,
      source_run_id: "run_abc".into(),
      source_reconstruction_ref: "scan.json".into(),
      anchors: vec![crate::ViewAnchor {
        id: "anchor.a".into(),
        label: "A".into(),
        strength: crate::AnchorStrength::Strong,
        bounds: ViewBounds::new(0.0, 0.0, 10.0, 10.0),
        evidence_ids: Vec::new(),
      }],
      landmarks: vec![
        crate::ViewLandmark {
          id: "landmark.a".into(),
          label: "L1".into(),
          landmark_use: crate::LandmarkUse::SectionAssignment,
          bounds: ViewBounds::new(0.0, 0.0, 10.0, 10.0),
          evidence_ids: Vec::new(),
        },
        crate::ViewLandmark {
          id: "landmark.b".into(),
          label: "L2".into(),
          landmark_use: crate::LandmarkUse::SectionAssignment,
          bounds: ViewBounds::new(0.0, 0.0, 10.0, 10.0),
          evidence_ids: Vec::new(),
        },
      ],
      node_snapshots: [(
        "item.a".into(),
        ViewNodeSnapshot {
          node_id: "item.a".into(),
          kind: "item".into(),
          domain_kind: None,
          label: Some("A".into()),
          parent: None,
          section_hint: None,
          bounds_window_local: Some(ViewBounds::new(0.0, 0.0, 10.0, 10.0)),
          viewport_fingerprint_hint: None,
          last_seen_observation_index: 0,
          confidence: crate::Confidence::Medium,
        },
      )]
      .into(),
      scope_snapshot: ViewMemoryScopeSnapshot {
        region_id: "playlist_sidebar".into(),
        region_bounds_window_local: ViewBounds::new(0.0, 0.0, 240.0, 400.0),
        baseline_width: 240,
        schema_version_view_ir: VIEW_IR_SCHEMA_VERSION.to_string(),
      },
      diagnostics: Vec::new(),
    }
  }

  #[test]
  fn memory_write_span_attributes_returns_six_required_attrs() {
    let memory = sample_memory();
    let attrs = memory_write_span_attributes(&memory, "run_persist");
    assert_eq!(attrs.len(), 6);
    assert_eq!(attrs[0].0, ATTR_MEMORY_MEMORY_ID);
    assert_eq!(attrs[0].1, "com.example.app:playlist_sidebar");
    assert_eq!(attrs[1].0, ATTR_MEMORY_NODE_SNAPSHOT_COUNT);
    assert_eq!(attrs[1].1, "1");
    assert_eq!(attrs[2].0, ATTR_MEMORY_ANCHOR_COUNT);
    assert_eq!(attrs[2].1, "1");
    assert_eq!(attrs[3].0, ATTR_MEMORY_LANDMARK_COUNT);
    assert_eq!(attrs[3].1, "2");
    assert_eq!(attrs[4].0, ATTR_MEMORY_EVICTION_COUNT);
    assert_eq!(attrs[4].1, "0");
    assert_eq!(attrs[5].0, ATTR_MEMORY_LAST_RECONSTRUCTED_AT_MILLIS);
    assert_eq!(attrs[5].1, "1719744000000");
  }

  #[test]
  fn reacquire_root_span_name_appends_scope_id() {
    assert_eq!(
      reacquire_root_span_name("playlist_sidebar"),
      "view.reacquire.playlist_sidebar"
    );
  }

  #[test]
  fn reacquire_stage_span_name_formats_index_and_strategy() {
    assert_eq!(
      reacquire_stage_span_name(3, "label_current_viewport"),
      "view.reacquire.stage.3.label_current_viewport"
    );
  }
}
