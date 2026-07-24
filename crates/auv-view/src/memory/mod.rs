//! ViewMemory construction, validation, and anchor reacquisition.
//!
//! Legacy string-pair trace builders are not part of the public memory API:
//!
//! ```compile_fail
//! use auv_view::memory::{
//!   memory_write_span_attributes,
//!   reacquire_memory_load_span_attributes,
//!   reacquire_root_span_name,
//!   reacquire_stage_span_name,
//! };
//! ```
//!
//! Obsolete app-specific inspect projections are not part of the view-memory
//! domain API:
//!
//! ```compile_fail
//! use auv_view::memory::ViewParserInspect;
//! ```

mod reacquire;
mod reacquire_adapter;
mod read;
mod write;

pub use reacquire::{
  ReacquireCandidate, ReacquireConfig, ReacquireObservation, ReacquireOutcome, ReacquireStrategy, ReacquireTarget, ReacquiredNode, reacquire,
};
pub use reacquire_adapter::{ReacquireDriverAdapter, outcome_label, strategy_name};
pub use read::{MemoryReadConfig, MemoryReadOutcome, StaleReason, read_memory};
pub use write::{MemoryWriteInput, build_memory_id, try_build_memory};

use std::collections::BTreeMap;

use auv_tracing::ArtifactUri;
use serde::{Deserialize, Serialize};

use crate::{
  Confidence, ParserDiagnostic, VIEW_IR_SCHEMA_VERSION, ViewAnchor, ViewBounds, ViewLandmark, ViewNodeKind, ViewNodeRecord,
  ViewReconstructionRecord,
};

pub const VIEW_MEMORY_SCHEMA_VERSION: &str = "view-memory-v0";

pub const DEFAULT_MEMORY_TTL_MILLIS: u64 = 24 * 60 * 60 * 1000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewMemory {
  pub schema_version: String,
  pub source_scan_uri: ArtifactUri,
  pub memory_id: String,
  pub app_bundle_id: String,
  pub scope_id: String,
  pub last_reconstructed_at_millis: u64,
  pub anchors: Vec<ViewAnchor>,
  pub landmarks: Vec<ViewLandmark>,
  pub node_snapshots: BTreeMap<String, ViewNodeSnapshot>,
  pub scope_snapshot: ViewMemoryScopeSnapshot,
  pub diagnostics: Vec<ParserDiagnostic>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewNodeSnapshot {
  pub node_id: String,
  pub kind: String,
  pub domain_kind: Option<String>,
  pub label: Option<String>,
  pub parent: Option<String>,
  pub section_hint: Option<String>,
  pub bounds_window_local: Option<ViewBounds>,
  pub viewport_fingerprint_hint: Option<String>,
  pub last_seen_observation_index: usize,
  pub confidence: Confidence,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewMemoryScopeSnapshot {
  pub region_id: String,
  pub region_bounds_window_local: ViewBounds,
  pub baseline_width: u32,
  pub schema_version_view_ir: String,
}

pub fn node_kind_wire(kind: ViewNodeKind) -> &'static str {
  match kind {
    ViewNodeKind::Container => "container",
    ViewNodeKind::Collection => "collection",
    ViewNodeKind::Section => "section",
    ViewNodeKind::Item => "item",
    ViewNodeKind::Text => "text",
    ViewNodeKind::Icon => "icon",
    ViewNodeKind::Unknown => "unknown",
  }
}

pub fn snapshot_from_node(
  node: &ViewNodeRecord,
  parent: Option<String>,
  section_hint: Option<String>,
  observation_index: usize,
) -> ViewNodeSnapshot {
  ViewNodeSnapshot {
    node_id: node.id.clone(),
    kind: node_kind_wire(node.kind).to_string(),
    domain_kind: node.domain_kind.clone(),
    label: node.label.clone(),
    parent,
    section_hint,
    bounds_window_local: Some(node.bounds),
    viewport_fingerprint_hint: node.label.as_ref().map(|label| crate::normalize_identity(label)),
    last_seen_observation_index: observation_index,
    confidence: Confidence::Medium,
  }
}

pub fn collect_node_snapshots(
  node: &ViewNodeRecord,
  parent: Option<String>,
  section_hint: Option<String>,
  observation_index: usize,
  out: &mut BTreeMap<String, ViewNodeSnapshot>,
) {
  if node.kind == ViewNodeKind::Unknown {
    return;
  }

  let section_hint = if node.kind == ViewNodeKind::Section {
    node.domain_kind.clone().or(section_hint)
  } else {
    section_hint
  };

  out.insert(node.id.clone(), snapshot_from_node(node, parent.clone(), section_hint.clone(), observation_index));

  for child in &node.children {
    collect_node_snapshots(child, Some(node.id.clone()), section_hint.clone(), observation_index, out);
  }
}

pub fn memory_from_reconstruction_parts(input: MemoryWriteInput<'_>, reconstruction: &ViewReconstructionRecord) -> Option<ViewMemory> {
  if !input.clean {
    return None;
  }

  let has_anchor = !reconstruction.anchor_index.is_empty();
  let mut snapshots = BTreeMap::new();
  collect_node_snapshots(input.root, None, None, 0, &mut snapshots);
  let has_item = snapshots.values().any(|snap| snap.kind == "item");
  if !has_anchor && !has_item {
    return None;
  }

  let memory_id = build_memory_id(input.app_bundle_id, input.scope_id);
  Some(ViewMemory {
    schema_version: VIEW_MEMORY_SCHEMA_VERSION.to_string(),
    source_scan_uri: input.source_scan_uri,
    memory_id,
    app_bundle_id: input.app_bundle_id.to_string(),
    scope_id: input.scope_id.to_string(),
    last_reconstructed_at_millis: input.last_reconstructed_at_millis,
    anchors: reconstruction.anchor_index.clone(),
    landmarks: reconstruction.landmark_index.clone(),
    node_snapshots: snapshots,
    scope_snapshot: ViewMemoryScopeSnapshot {
      region_id: input.scope_snapshot.region_id,
      region_bounds_window_local: input.scope_snapshot.region_bounds_window_local,
      baseline_width: input.scope_snapshot.baseline_width,
      schema_version_view_ir: VIEW_IR_SCHEMA_VERSION.to_string(),
    },
    diagnostics: Vec::new(),
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{AnchorStrength, ViewAnchor};
  use auv_tracing::{ArtifactId, RunId};

  fn source_scan_uri() -> ArtifactUri {
    ArtifactUri::from_ids(RunId::new(), ArtifactId::new())
  }

  fn sample_root() -> ViewNodeRecord {
    ViewNodeRecord {
      id: "root".into(),
      kind: ViewNodeKind::Collection,
      children: vec![ViewNodeRecord {
        id: "item.coding-bgm".into(),
        kind: ViewNodeKind::Item,
        label: Some("Coding BGM".into()),
        domain_kind: Some("my_playlists".into()),
        bounds: ViewBounds::new(32.0, 74.0, 120.0, 20.0),
        anchors: vec![ViewAnchor {
          id: "anchor.coding-bgm".into(),
          label: "Coding BGM".into(),
          strength: AnchorStrength::Strong,
          bounds: ViewBounds::new(32.0, 74.0, 120.0, 20.0),
          evidence_ids: Vec::new(),
        }],
        ..Default::default()
      }],
      ..Default::default()
    }
  }

  #[test]
  fn memory_roundtrip_serde() {
    let root = sample_root();
    let reconstruction = ViewReconstructionRecord {
      root: root.clone(),
      anchor_index: root.children[0].anchors.clone(),
      landmark_index: Vec::new(),
    };
    let memory = memory_from_reconstruction_parts(
      MemoryWriteInput {
        source_scan_uri: source_scan_uri(),
        app_bundle_id: "com.netease.163music",
        scope_id: "playlist_sidebar",
        root: &root,
        scope_snapshot: ViewMemoryScopeSnapshot {
          region_id: "playlist_sidebar".into(),
          region_bounds_window_local: ViewBounds::new(0.0, 0.0, 240.0, 400.0),
          baseline_width: 240,
          schema_version_view_ir: VIEW_IR_SCHEMA_VERSION.to_string(),
        },
        last_reconstructed_at_millis: 1_719_744_000_000,
        clean: true,
      },
      &reconstruction,
    )
    .expect("memory should build");

    let json = serde_json::to_string(&memory).expect("serialize");
    let decoded: ViewMemory = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(decoded, memory);
    assert_eq!(decoded.memory_id, "com.netease.163music:playlist_sidebar");
  }

  #[test]
  fn memory_write_skips_empty_reconstruction() {
    let root = ViewNodeRecord {
      kind: ViewNodeKind::Collection,
      ..Default::default()
    };
    let reconstruction = ViewReconstructionRecord {
      root: root.clone(),
      anchor_index: Vec::new(),
      landmark_index: Vec::new(),
    };
    let memory = memory_from_reconstruction_parts(
      MemoryWriteInput {
        source_scan_uri: source_scan_uri(),
        app_bundle_id: "com.netease.163music",
        scope_id: "playlist_sidebar",
        root: &root,
        scope_snapshot: ViewMemoryScopeSnapshot {
          region_id: "playlist_sidebar".into(),
          region_bounds_window_local: ViewBounds::default(),
          baseline_width: 240,
          schema_version_view_ir: VIEW_IR_SCHEMA_VERSION.to_string(),
        },
        last_reconstructed_at_millis: 0,
        clean: true,
      },
      &reconstruction,
    );
    assert!(memory.is_none());
  }

  #[test]
  fn memory_id_stable_for_app_scope_pair() {
    assert_eq!(build_memory_id("com.netease.163music", "playlist_sidebar"), "com.netease.163music:playlist_sidebar");
  }
}
