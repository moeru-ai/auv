//! ViewMemory persistence and anchor reacquisition (SceneBridge A3-min).
//!
//! NOTICE(artifact-dir-bridge-a3): Without `--store-root`, reads/writes JSON under
//! product `--artifact-dir` paths with a compatibility placeholder run id.
//! Run-storage `view-memory` artifact role lands in A7-min when store recording
//! is enabled.

mod inspect;
mod reacquire;
mod reacquire_adapter;
mod read;
mod store;
mod trace;
mod write;

pub use inspect::{
  GeometryProofSummary, IdentityProofSummary, MemoryProofSummary, PLAYLIST_SELECT_RESULT_ARTIFACT_ROLE, ReacquisitionRecord,
  ReplayProofSummary, ResolutionProofSummary, VerificationProofSummary, ViewParserInspect, ViewParserListSummary, ViewParserReacquireWire,
  ViewParserSelectResultWire, ViewParserSelectStepWire, ViewParserSelectTargetWire, ViewParserSelectVerificationWire, ViewResolutionSummary,
  format_view_resolution_summary_text, summarize_view_parser_inspect,
};
pub use reacquire::{
  ReacquireCandidate, ReacquireConfig, ReacquireObservation, ReacquireOutcome, ReacquireStrategy, ReacquireTarget, ReacquiredNode, reacquire,
};
pub use reacquire_adapter::{ReacquireDriverAdapter, outcome_label, strategy_name};
pub use read::{MemoryReadConfig, MemoryReadOutcome, StaleReason, read_memory};
pub use store::{
  load_memory_file, memory_file_name, memory_file_path, parse_memory_file, serialize_memory_bytes, view_memory_lineage_ref_wire,
  write_memory_file,
};
pub use trace::{
  ATTR_MEMORY_ANCHOR_COUNT, ATTR_MEMORY_EVICTION_COUNT, ATTR_MEMORY_LANDMARK_COUNT, ATTR_MEMORY_LAST_RECONSTRUCTED_AT_MILLIS,
  ATTR_MEMORY_LOAD_MEMORY_ID, ATTR_MEMORY_LOAD_SOURCE_RUN_ID, ATTR_MEMORY_MEMORY_ID, ATTR_MEMORY_NODE_SNAPSHOT_COUNT,
  ATTR_REACQUIRE_FATAL_DIAGNOSTIC_KIND, ATTR_REACQUIRE_OBSERVATION_COUNT, ATTR_REACQUIRE_OUTCOME, ATTR_REACQUIRE_SCOPE_ID,
  ATTR_REACQUIRE_SKIPPED_RESCAN_REPLAY, ATTR_REACQUIRE_STAGE_USED, ATTR_REACQUIRE_TARGET_KIND, SPAN_MEMORY_WRITE,
  SPAN_REACQUIRE_MEMORY_LOAD, SPAN_REACQUIRE_ROOT_PREFIX, SPAN_REACQUIRE_STAGE_PREFIX, memory_write_span_attributes,
  reacquire_memory_load_span_attributes, reacquire_root_span_name, reacquire_stage_span_name,
};
pub use write::{ARTIFACT_DIR_BRIDGE_RUN_ID, MemoryWriteInput, build_memory_id, try_build_memory};

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
  Confidence, ParserDiagnostic, VIEW_IR_SCHEMA_VERSION, ViewAnchor, ViewBounds, ViewLandmark, ViewNodeKind, ViewNodeRecord,
  ViewReconstructionRecord,
};

pub const VIEW_MEMORY_SCHEMA_VERSION: &str = "view-memory-v0";

/// Donor-neutral run artifact role for persisted [`ViewMemory`] payloads.
pub const VIEW_MEMORY_ARTIFACT_ROLE: &str = "view-memory";

pub const DEFAULT_MEMORY_TTL_MILLIS: u64 = 24 * 60 * 60 * 1000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewMemory {
  pub schema_version: String,
  pub memory_id: String,
  pub app_bundle_id: String,
  pub scope_id: String,
  pub last_reconstructed_at_millis: u64,
  pub source_run_id: String,
  pub source_reconstruction_ref: String,
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
    memory_id,
    app_bundle_id: input.app_bundle_id.to_string(),
    scope_id: input.scope_id.to_string(),
    last_reconstructed_at_millis: input.last_reconstructed_at_millis,
    source_run_id: input.source_run_id,
    source_reconstruction_ref: input.source_reconstruction_ref,
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
        app_bundle_id: "com.netease.163music",
        scope_id: "playlist_sidebar",
        root: &root,
        scope_snapshot: ViewMemoryScopeSnapshot {
          region_id: "playlist_sidebar".into(),
          region_bounds_window_local: ViewBounds::new(0.0, 0.0, 240.0, 400.0),
          baseline_width: 240,
          schema_version_view_ir: VIEW_IR_SCHEMA_VERSION.to_string(),
        },
        source_reconstruction_ref: "playlist-scan-cache.json".into(),
        source_run_id: ARTIFACT_DIR_BRIDGE_RUN_ID.into(),
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
        app_bundle_id: "com.netease.163music",
        scope_id: "playlist_sidebar",
        root: &root,
        scope_snapshot: ViewMemoryScopeSnapshot {
          region_id: "playlist_sidebar".into(),
          region_bounds_window_local: ViewBounds::default(),
          baseline_width: 240,
          schema_version_view_ir: VIEW_IR_SCHEMA_VERSION.to_string(),
        },
        source_reconstruction_ref: String::new(),
        source_run_id: ARTIFACT_DIR_BRIDGE_RUN_ID.into(),
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
